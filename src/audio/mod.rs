use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use hound::{WavSpec, WavWriter};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub struct AudioRecorder {
    is_recording: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<i16>>>,
    stream: Option<Stream>,
    sample_rate: u32,
    channels: u16,
}

impl AudioRecorder {
    pub fn new() -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("No default audio input device found")?;

        let default_config = device
            .default_input_config()
            .context("Failed to get default input config")?;

        let sample_rate = default_config.sample_rate().0;
        let channels = default_config.channels();

        Ok(Self {
            is_recording: Arc::new(AtomicBool::new(false)),
            samples: Arc::new(Mutex::new(Vec::with_capacity(sample_rate as usize * 10))),
            stream: None,
            sample_rate,
            channels,
        })
    }

    pub fn start(&mut self) -> Result<()> {
        if self.is_recording.load(Ordering::SeqCst) {
            return Ok(());
        }

        // Clear previous audio samples
        {
            let mut buf = self.samples.lock().unwrap();
            buf.clear();
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("No default audio input device")?;

        let config = device.default_input_config()?;
        self.sample_rate = config.sample_rate().0;
        self.channels = config.channels();

        let samples_clone = Arc::clone(&self.samples);
        let is_rec = Arc::clone(&self.is_recording);
        is_rec.store(true, Ordering::SeqCst);

        let err_fn = |err| eprintln!("Audio stream error: {}", err);

        let stream = match config.sample_format() {
            SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if is_rec.load(Ordering::SeqCst) {
                        if let Ok(mut buf) = samples_clone.lock() {
                            for &sample in data {
                                // Clamp and convert f32 (-1.0..1.0) to i16
                                let s = (sample.max(-1.0).min(1.0) * 32767.0) as i16;
                                buf.push(s);
                            }
                        }
                    }
                },
                err_fn,
                None,
            )?,
            SampleFormat::I16 => device.build_input_stream(
                &config.into(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if is_rec.load(Ordering::SeqCst) {
                        if let Ok(mut buf) = samples_clone.lock() {
                            buf.extend_from_slice(data);
                        }
                    }
                },
                err_fn,
                None,
            )?,
            SampleFormat::U16 => device.build_input_stream(
                &config.into(),
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    if is_rec.load(Ordering::SeqCst) {
                        if let Ok(mut buf) = samples_clone.lock() {
                            for &sample in data {
                                let s = (sample as i32 - 32768) as i16;
                                buf.push(s);
                            }
                        }
                    }
                },
                err_fn,
                None,
            )?,
            _ => return Err(anyhow::anyhow!("Unsupported audio sample format")),
        };

        stream.play()?;
        self.stream = Some(stream);
        Ok(())
    }

    pub fn stop(&mut self) -> Result<Option<Vec<u8>>> {
        if !self.is_recording.load(Ordering::SeqCst) {
            return Ok(None);
        }

        self.is_recording.store(false, Ordering::SeqCst);
        self.stream = None; // Drops and closes stream

        let samples = {
            let mut buf = self.samples.lock().unwrap();
            std::mem::take(&mut *buf)
        };

        if samples.is_empty() {
            return Ok(None);
        }

        // Encode to WAV format in memory
        let spec = WavSpec {
            channels: self.channels,
            sample_rate: self.sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let mut cursor = Cursor::new(Vec::with_capacity(samples.len() * 2 + 44));
        {
            let mut writer = WavWriter::new(&mut cursor, spec)?;
            for sample in samples {
                writer.write_sample(sample)?;
            }
            writer.finalize()?;
        }

        Ok(Some(cursor.into_inner()))
    }

    #[allow(dead_code)]
    pub fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::SeqCst)
    }
}

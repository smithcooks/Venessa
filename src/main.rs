#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod config;
mod gemini;
mod gui;
mod hotkey;
mod overlay;
mod tray;
mod typer;

use audio::AudioRecorder;
use config::VenessaConfig;
use crossbeam_channel::unbounded;
use gemini::GeminiClient;
use gui::SettingsGui;
use hotkey::{HotkeyConfig, HotkeyEvent, HotkeyManager};
use overlay::StatusOverlay;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tray::{TrayCommand, TrayIcon};

#[derive(Debug, Clone, Copy)]
pub enum SoundCue {
    Start,
    Stop,
    Success,
    Error,
}

#[cfg(windows)]
pub fn play_sound_cue(cue: SoundCue) {
    std::thread::spawn(move || {
        #[link(name = "kernel32")]
        extern "system" {
            fn Beep(dw_freq: u32, dw_duration: u32) -> i32;
        }
        unsafe {
            match cue {
                SoundCue::Start => {
                    let _ = Beep(880, 70); // A5 high beep
                }
                SoundCue::Stop => {
                    let _ = Beep(440, 70); // A4 lower processing tone
                }
                SoundCue::Success => {
                    let _ = Beep(1046, 50); // C6 cheerful ping
                }
                SoundCue::Error => {
                    let _ = Beep(300, 150);
                    let _ = Beep(200, 200);
                }
            }
        }
    });
}

#[cfg(not(windows))]
pub fn play_sound_cue(_cue: SoundCue) {}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Venessa: Ultra-lean Voice Typing (Gemini Powered) ===");

    // 1. Load or initialize configuration
    let initial_cfg = VenessaConfig::load();
    let cfg_path = VenessaConfig::config_path();
    println!("Configuration path: {}", cfg_path.display());

    let config_mutex = Arc::new(Mutex::new(initial_cfg.clone()));

    // 2. Channels
    let (hotkey_tx, hotkey_rx) = unbounded::<HotkeyEvent>();
    let (tray_tx, tray_rx) = unbounded::<TrayCommand>();
    let (gui_tx, gui_rx) = unbounded::<VenessaConfig>();

    // 3. Initialize Settings GUI
    let settings_gui = SettingsGui::new(Arc::clone(&config_mutex), gui_tx);

    // 4. Initialize Status Overlay
    let overlay = Arc::new(StatusOverlay::new());

    // 5. Initialize Audio Recorder (kept on main thread for WASAPI reliability)
    let mut recorder = match AudioRecorder::new() {
        Ok(rec) => rec,
        Err(e) => {
            let err_msg = format!("Audio device error: {}", e);
            eprintln!("{}", err_msg);
            TrayIcon::notify("Venessa Microphone Error", &err_msg, true);
            return Err(e);
        }
    };

    // 6. Initialize Gemini Client
    let gemini = Arc::new(GeminiClient::new(
        initial_cfg.gemini_api_key.clone(),
        initial_cfg.gemini_model.clone(),
        initial_cfg.prompt.clone(),
    ));

    // 7. Start System Tray Icon
    let tip = format!("Venessa: Voice Dictation (Hold {} to speak)", initial_cfg.hotkey);
    let _tray = TrayIcon::new(tray_tx, &tip);

    // If API key is empty on initial launch, open Settings GUI automatically
    if initial_cfg.gemini_api_key.trim().is_empty() {
        println!("\n[!] NOTICE: Gemini API key is not configured.");
        println!("Opening Venessa Settings GUI...\n");
        TrayIcon::notify(
            "Venessa Setup Required",
            "Please enter your Google Gemini API key in Settings to begin voice typing.",
            false,
        );
        settings_gui.show();
    } else {
        TrayIcon::notify(
            "Venessa is Ready",
            &format!("Hold {} to speak, release to transcribe at cursor.", initial_cfg.hotkey),
            false,
        );
    }

    // 8. Start Global Hotkey Listener
    let hk_config = HotkeyConfig::parse(&initial_cfg.hotkey);
    let mut _hotkey_mgr = HotkeyManager::start(hk_config, hotkey_tx);

    let is_recording = Arc::new(AtomicBool::new(false));
    let mut mode = initial_cfg.mode.clone();
    let mut sound_feedback = initial_cfg.sound_feedback;
    let mut type_delay_ms = initial_cfg.type_delay_ms;

    println!("Venessa is active in the system tray. Ready to listen!");

    // Main event handling loop
    loop {
        tokio::select! {
            // Check hotkey events
            Ok(event) = async { hotkey_rx.recv().map_err(|e| anyhow::anyhow!(e)) } => {
                match event {
                    HotkeyEvent::Pressed => {
                        let currently_recording = is_recording.load(Ordering::SeqCst);
                        if mode == "toggle" && currently_recording {
                            // Toggle mode: stop recording
                            is_recording.store(false, Ordering::SeqCst);
                            if sound_feedback {
                                play_sound_cue(SoundCue::Stop);
                            }
                            overlay.show_transcribing();

                            if let Ok(Some(wav_bytes)) = recorder.stop() {
                                let gemini_clone = Arc::clone(&gemini);
                                let overlay_clone = Arc::clone(&overlay);
                                tokio::spawn(async move {
                                    match gemini_clone.transcribe_and_clean(&wav_bytes).await {
                                        Ok(text) => {
                                            overlay_clone.hide();
                                            if sound_feedback {
                                                play_sound_cue(SoundCue::Success);
                                            }
                                            if !text.is_empty() {
                                                typer::type_text(&text, type_delay_ms);
                                            }
                                        }
                                        Err(err) => {
                                            let err_str = format!("{}", err);
                                            eprintln!("Transcription error: {}", err_str);
                                            if sound_feedback {
                                                play_sound_cue(SoundCue::Error);
                                            }
                                            overlay_clone.show_error("Transcription Failed");
                                            TrayIcon::notify("Venessa Transcription Error", &err_str, true);
                                        }
                                    }
                                });
                            } else {
                                overlay.hide();
                            }
                        } else if !currently_recording {
                            // Start recording
                            is_recording.store(true, Ordering::SeqCst);
                            if sound_feedback {
                                play_sound_cue(SoundCue::Start);
                            }
                            overlay.show_listening();

                            if let Err(e) = recorder.start() {
                                let err_msg = format!("Failed to start recording: {}", e);
                                eprintln!("{}", err_msg);
                                is_recording.store(false, Ordering::SeqCst);
                                overlay.show_error("Microphone Error");
                                TrayIcon::notify("Recording Error", &err_msg, true);
                            }
                        }
                    }
                    HotkeyEvent::Released => {
                        if mode == "hold" && is_recording.load(Ordering::SeqCst) {
                            // Hold mode: stop recording on release
                            is_recording.store(false, Ordering::SeqCst);
                            if sound_feedback {
                                play_sound_cue(SoundCue::Stop);
                            }
                            overlay.show_transcribing();

                            if let Ok(Some(wav_bytes)) = recorder.stop() {
                                let gemini_clone = Arc::clone(&gemini);
                                let overlay_clone = Arc::clone(&overlay);
                                tokio::spawn(async move {
                                    match gemini_clone.transcribe_and_clean(&wav_bytes).await {
                                        Ok(text) => {
                                            overlay_clone.hide();
                                            if sound_feedback {
                                                play_sound_cue(SoundCue::Success);
                                            }
                                            if !text.is_empty() {
                                                typer::type_text(&text, type_delay_ms);
                                            }
                                        }
                                        Err(err) => {
                                            let err_str = format!("{}", err);
                                            eprintln!("Transcription error: {}", err_str);
                                            if sound_feedback {
                                                play_sound_cue(SoundCue::Error);
                                            }
                                            overlay_clone.show_error("Transcription Failed");
                                            TrayIcon::notify("Venessa Transcription Error", &err_str, true);
                                        }
                                    }
                                });
                            } else {
                                overlay.hide();
                            }
                        }
                    }
                }
            }

            // Check tray commands
            Ok(cmd) = async { tray_rx.recv().map_err(|e| anyhow::anyhow!(e)) } => {
                match cmd {
                    TrayCommand::OpenSettings => {
                        settings_gui.show();
                    }
                    TrayCommand::OpenConfig => {
                        let path = VenessaConfig::config_path();
                        let _ = Command::new("notepad.exe")
                            .arg(path)
                            .spawn();
                    }
                    TrayCommand::Quit => {
                        println!("Exiting Venessa...");
                        break;
                    }
                }
            }

            // Check settings updates from GUI
            Ok(new_cfg) = async { gui_rx.recv().map_err(|e| anyhow::anyhow!(e)) } => {
                println!("Settings updated via GUI:");
                println!("  Model: {}", new_cfg.gemini_model);
                println!("  Hotkey: {}", new_cfg.hotkey);
                println!("  Mode: {}", new_cfg.mode);

                // Update runtime values
                mode = new_cfg.mode.clone();
                sound_feedback = new_cfg.sound_feedback;
                type_delay_ms = new_cfg.type_delay_ms;

                // Update Gemini client
                gemini.update(
                    new_cfg.gemini_api_key.clone(),
                    new_cfg.gemini_model.clone(),
                    new_cfg.prompt.clone(),
                );

                // Update global hotkey hook
                let new_hk = HotkeyConfig::parse(&new_cfg.hotkey);
                hotkey::update_hotkey(new_hk);

                // Update tray tooltip & notification
                let tip = format!("Venessa: Voice Dictation (Hold {} to speak)", new_cfg.hotkey);
                TrayIcon::update_tip(&tip);
                TrayIcon::notify(
                    "Settings Applied",
                    &format!("Hotkey: {} | Model: {}", new_cfg.hotkey, new_cfg.gemini_model),
                    false,
                );
            }
        }
    }

    Ok(())
}

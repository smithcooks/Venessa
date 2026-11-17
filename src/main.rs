#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod config;
mod gemini;
mod gui;
mod hotkey;
mod tray;
mod typer;

use audio::AudioRecorder;
use config::VenessaConfig;
use crossbeam_channel::unbounded;
use gemini::GeminiClient;
use gui::SettingsGui;
use hotkey::{HotkeyConfig, HotkeyEvent, HotkeyManager};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tray::{TrayCommand, TrayIcon};

#[cfg(windows)]
fn play_feedback_sound(_start: bool) {
    #[link(name = "user32")]
    extern "system" {
        fn MessageBeep(u_type: u32) -> i32;
    }
    unsafe {
        let _ = MessageBeep(0);
    }
}

#[cfg(not(windows))]
fn play_feedback_sound(_start: bool) {}

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

    // If API key is empty on initial launch, open Settings GUI automatically
    if initial_cfg.gemini_api_key.trim().is_empty() {
        println!("\n[!] NOTICE: Gemini API key is not configured.");
        println!("Opening Venessa Settings GUI...\n");
        settings_gui.show();
    } else {
        println!("Using Gemini Model: {}", initial_cfg.gemini_model);
    }
    println!("Hotkey: {} (mode: {})", initial_cfg.hotkey, initial_cfg.mode);

    // 4. Initialize Audio Recorder (kept on main thread)
    let mut recorder = match AudioRecorder::new() {
        Ok(rec) => rec,
        Err(e) => {
            eprintln!("Failed to initialize audio capture device: {}", e);
            return Err(e);
        }
    };

    // 5. Initialize Gemini Client
    let gemini = Arc::new(GeminiClient::new(
        initial_cfg.gemini_api_key.clone(),
        initial_cfg.gemini_model.clone(),
        initial_cfg.prompt.clone(),
    ));

    // 6. Start System Tray Icon
    let _tray = TrayIcon::new(tray_tx);

    // 7. Start Global Hotkey Listener
    let hk_config = HotkeyConfig::parse(&initial_cfg.hotkey);
    let mut _hotkey_mgr = HotkeyManager::start(hk_config, hotkey_tx);

    let is_recording = Arc::new(AtomicBool::new(false));
    let mut mode = initial_cfg.mode.clone();
    let mut sound_feedback = initial_cfg.sound_feedback;
    let mut type_delay_ms = initial_cfg.type_delay_ms;

    println!("Venessa is active in the system tray. Ready to listen!");

    // Event handling loop
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
                                play_feedback_sound(false);
                            }
                            if let Ok(Some(wav_bytes)) = recorder.stop() {
                                let gemini_clone = Arc::clone(&gemini);
                                tokio::spawn(async move {
                                    match gemini_clone.transcribe_and_clean(&wav_bytes).await {
                                        Ok(text) => {
                                            if !text.is_empty() {
                                                typer::type_text(&text, type_delay_ms);
                                            }
                                        }
                                        Err(err) => {
                                            eprintln!("Transcription error: {}", err);
                                        }
                                    }
                                });
                            }
                        } else if !currently_recording {
                            // Start recording
                            is_recording.store(true, Ordering::SeqCst);
                            if sound_feedback {
                                play_feedback_sound(true);
                            }
                            if let Err(e) = recorder.start() {
                                eprintln!("Failed to start recording: {}", e);
                            }
                        }
                    }
                    HotkeyEvent::Released => {
                        if mode == "hold" && is_recording.load(Ordering::SeqCst) {
                            // Hold mode: stop recording on release
                            is_recording.store(false, Ordering::SeqCst);
                            if sound_feedback {
                                play_feedback_sound(false);
                            }
                            if let Ok(Some(wav_bytes)) = recorder.stop() {
                                let gemini_clone = Arc::clone(&gemini);
                                tokio::spawn(async move {
                                    match gemini_clone.transcribe_and_clean(&wav_bytes).await {
                                        Ok(text) => {
                                            if !text.is_empty() {
                                                typer::type_text(&text, type_delay_ms);
                                            }
                                        }
                                        Err(err) => {
                                            eprintln!("Transcription error: {}", err);
                                        }
                                    }
                                });
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
            }
        }
    }

    Ok(())
}

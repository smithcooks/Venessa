use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VenessaConfig {
    /// Your Google Gemini API Key
    /// Obtain a free key from https://aistudio.google.com/app/apikey
    #[serde(default = "default_api_key")]
    pub gemini_api_key: String,

    /// Gemini model to use for speech-to-text & cleanup (e.g. "gemini-2.5-flash", "gemini-1.5-flash")
    #[serde(default = "default_model")]
    pub gemini_model: String,

    /// Hotkey to trigger dictation (e.g. "Ctrl+Space", "Alt+Space", "Ctrl+Shift+V", "F8", "CapsLock")
    #[serde(default = "default_hotkey")]
    pub hotkey: String,

    /// Trigger mode: "hold" (Push-to-Talk: hold hotkey to record, release to transcribe) or "toggle" (press once to start, press again to stop)
    #[serde(default = "default_mode")]
    pub mode: String,

    /// Delay in milliseconds between typed characters (0 = maximum speed)
    #[serde(default = "default_type_delay")]
    pub type_delay_ms: u64,

    /// Play subtle audio beeps when recording starts and ends
    #[serde(default = "default_sound_feedback")]
    pub sound_feedback: bool,

    /// Instruction prompt passed to Gemini for transcription & cleanup
    #[serde(default = "default_prompt")]
    pub prompt: String,
}

fn default_api_key() -> String {
    "".to_string()
}

fn default_model() -> String {
    "gemini-2.5-flash".to_string()
}

fn default_hotkey() -> String {
    "Ctrl+Space".to_string()
}

fn default_mode() -> String {
    "hold".to_string()
}

fn default_type_delay() -> u64 {
    2
}

fn default_sound_feedback() -> bool {
    true
}

fn default_prompt() -> String {
    "Please transcribe this audio accurately. Lightly clean up speech disfluencies (such as filler words like um, uh, stutters, and accidental repetitions) while preserving the exact intended meaning, vocabulary, and natural tone of the speaker. Format with proper capitalization and punctuation. Return ONLY the transcribed text without any introductory remarks, notes, quotes, or conversational commentary.".to_string()
}

impl Default for VenessaConfig {
    fn default() -> Self {
        Self {
            gemini_api_key: default_api_key(),
            gemini_model: default_model(),
            hotkey: default_hotkey(),
            mode: default_mode(),
            type_delay_ms: default_type_delay(),
            sound_feedback: default_sound_feedback(),
            prompt: default_prompt(),
        }
    }
}

impl VenessaConfig {
    pub fn config_path() -> PathBuf {
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("Venessa");
        path.push("config.toml");
        path
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(config) = toml::from_str::<VenessaConfig>(&content) {
                    return config;
                }
            }
        }

        // Create default config if missing
        let default_cfg = VenessaConfig::default();
        let _ = default_cfg.save();
        default_cfg
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let toml_str = toml::to_string_pretty(self).unwrap_or_default();
        let header = "# Venessa - Ultra-lean Voice Typing Configuration\n# Edit this file and restart Venessa to apply changes.\n\n";
        fs::write(&path, format!("{}{}", header, toml_str))?;
        Ok(())
    }
}

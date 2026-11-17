use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use reqwest::Client;
use serde_json::json;
use std::sync::RwLock;

pub struct GeminiClient {
    client: Client,
    api_key: RwLock<String>,
    model: RwLock<String>,
    prompt: RwLock<String>,
}

impl GeminiClient {
    pub fn new(api_key: String, model: String, prompt: String) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
            api_key: RwLock::new(api_key),
            model: RwLock::new(Self::normalize_model(&model)),
            prompt: RwLock::new(prompt),
        }
    }

    fn normalize_model(m: &str) -> String {
        let clean = m.trim().trim_start_matches("models/");
        match clean {
            // Automatically upgrade deprecated models that return 404 to active modern models
            "gemini-2.5-flash" | "gemini-1.5-flash" | "gemini-2.0-flash" | "" => {
                "gemini-3.5-flash-lite".to_string()
            }
            other => other.to_string(),
        }
    }

    pub fn update(&self, api_key: String, model: String, prompt: String) {
        if let Ok(mut k) = self.api_key.write() {
            *k = api_key;
        }
        if let Ok(mut m) = self.model.write() {
            *m = Self::normalize_model(&model);
        }
        if let Ok(mut p) = self.prompt.write() {
            *p = prompt;
        }
    }

    pub async fn transcribe_and_clean(&self, wav_bytes: &[u8]) -> Result<String> {
        let (api_key, primary_model, prompt) = {
            let k = self.api_key.read().unwrap().clone();
            let m = self.model.read().unwrap().clone();
            let p = self.prompt.read().unwrap().clone();
            (k, m, p)
        };

        if api_key.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "Gemini API key is not set. Please right-click the Venessa tray icon and open Settings to enter your key."
            ));
        }

        let audio_base64 = BASE64.encode(wav_bytes);

        // Candidate model chain: try primary model, then fall back if 404 or 503 occurs
        let mut candidate_models = vec![primary_model.clone()];
        for fallback in &["gemini-3.5-flash-lite", "gemini-3-flash-preview", "gemini-3.1-flash-lite"] {
            if *fallback != primary_model {
                candidate_models.push(fallback.to_string());
            }
        }

        let mut last_err = String::new();

        for model_name in candidate_models {
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
                model_name, api_key.trim()
            );

            let payload = json!({
                "contents": [
                    {
                        "parts": [
                            {
                                "inline_data": {
                                    "mime_type": "audio/wav",
                                    "data": audio_base64
                                }
                            },
                            {
                                "text": prompt
                            }
                        ]
                    }
                ],
                "generationConfig": {
                    "temperature": 0.1,
                    "maxOutputTokens": 2048
                }
            });

            let resp = match self
                .client
                .post(&url)
                .header("Content-Type", "application/json")
                .json(&payload)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    last_err = format!("Network request failed: {}", e);
                    continue;
                }
            };

            let status = resp.status();
            if !status.is_success() {
                let err_text = resp.text().await.unwrap_or_default();
                // Try parsing JSON error message from Google
                let parsed_msg = if let Ok(v) = serde_json::from_str::<serde_json::Value>(&err_text) {
                    v["error"]["message"].as_str().map(|s| s.to_string()).unwrap_or(err_text)
                } else {
                    err_text
                };

                // If model is not found (404) or busy (503), try next fallback model
                if status.as_u16() == 404 || status.as_u16() == 503 {
                    last_err = format!("Model {} unavailable ({}): {}", model_name, status, parsed_msg);
                    continue;
                }

                return Err(anyhow::anyhow!("Gemini API error ({}): {}", status, parsed_msg));
            }

            let body: serde_json::Value = resp
                .json()
                .await
                .context("Failed to parse Gemini API JSON response")?;

            // Extract transcription text
            let text = body["candidates"][0]["content"]["parts"][0]["text"]
                .as_str()
                .unwrap_or("");

            let cleaned = text.trim();
            // Strip markdown code fences if model enclosed text in ```
            let final_text = if cleaned.starts_with("```") && cleaned.ends_with("```") {
                let lines: Vec<&str> = cleaned.lines().collect();
                if lines.len() >= 2 {
                    lines[1..lines.len() - 1].join("\n").trim().to_string()
                } else {
                    cleaned.to_string()
                }
            } else {
                cleaned.to_string()
            };

            return Ok(final_text);
        }

        Err(anyhow::anyhow!("All Gemini model attempts failed: {}", last_err))
    }
}

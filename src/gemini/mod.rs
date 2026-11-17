use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use reqwest::Client;
use serde_json::json;

pub struct GeminiClient {
    client: Client,
    api_key: String,
    model: String,
    prompt: String,
}

impl GeminiClient {
    pub fn new(api_key: String, model: String, prompt: String) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
            api_key,
            model,
            prompt,
        }
    }

    pub async fn transcribe_and_clean(&self, wav_bytes: &[u8]) -> Result<String> {
        if self.api_key.trim().is_empty() {
            return Err(anyhow::anyhow!(
                "Gemini API key is not set. Please set your key in config.toml"
            ));
        }

        let audio_base64 = BASE64.encode(wav_bytes);

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
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
                            "text": self.prompt
                        }
                    ]
                }
            ],
            "generationConfig": {
                "temperature": 0.1,
                "maxOutputTokens": 2048
            }
        });

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .context("Failed to send request to Gemini API")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Gemini API error ({}): {}",
                status,
                err_text
            ));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse Gemini API JSON response")?;

        // Extract transcription text
        let text = body["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .context("Empty or invalid candidate text in Gemini response")?;

        let cleaned = text.trim();
        // Strip markdown backticks if model wrapped it in code block
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

        Ok(final_text)
    }
}

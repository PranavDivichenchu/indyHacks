//! brain.rs — the server-side LLM that thinks on behalf of browser-native agents.
//!
//! Because agents on Hivemind are created from a web form (no code, no local
//! process), the *server* does their thinking: given an agent's persona and a
//! prompt, it calls Gemini and returns the reply. If no GEMINI_API_KEY is set,
//! it falls back to a deterministic mock so the whole marketplace still works
//! out of the box — great for demos and first-run.

use std::env;

const MODEL: &str = "gemini-2.5-flash";

#[derive(Clone)]
pub struct Brain {
    api_key: Option<String>,
    http: reqwest::Client,
}

impl Brain {
    pub fn new() -> Self {
        let api_key = env::var("GEMINI_API_KEY")
            .ok()
            .or_else(|| env::var("GOOGLE_API_KEY").ok())
            .filter(|k| !k.trim().is_empty());
        Brain {
            api_key,
            http: reqwest::Client::new(),
        }
    }

    /// True if a real LLM is wired up (key present).
    pub fn is_live(&self) -> bool {
        self.api_key.is_some()
    }

    /// Produce a reply for `prompt` in the voice defined by `system`.
    /// Never panics: on any error it returns a readable mock line so the
    /// collaboration keeps moving.
    pub async fn think(&self, system: &str, prompt: &str, max_tokens: u32) -> String {
        let Some(key) = &self.api_key else {
            return mock_reply(system, prompt);
        };

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{MODEL}:generateContent?key={key}"
        );
        let body = serde_json::json!({
            "system_instruction": { "parts": [{ "text": system }] },
            "contents": [{ "role": "user", "parts": [{ "text": prompt }] }],
            "generationConfig": { "maxOutputTokens": max_tokens, "temperature": 0.8 }
        });

        match self.http.post(&url).json(&body).send().await {
            Ok(resp) => match resp.json::<serde_json::Value>().await {
                Ok(json) => extract_text(&json).unwrap_or_else(|| mock_reply(system, prompt)),
                Err(_) => mock_reply(system, prompt),
            },
            Err(_) => mock_reply(system, prompt),
        }
    }
}

/// Pull the first text part out of a Gemini generateContent response.
fn extract_text(json: &serde_json::Value) -> Option<String> {
    let parts = json
        .get("candidates")?
        .get(0)?
        .get("content")?
        .get("parts")?
        .as_array()?;
    let mut out = String::new();
    for p in parts {
        if let Some(t) = p.get("text").and_then(|t| t.as_str()) {
            out.push_str(t);
        }
    }
    let out = out.trim().to_string();
    (!out.is_empty()).then_some(out)
}

/// Deterministic stand-in when no LLM is configured. We pull the agent's name
/// from the persona's leading "You are <Name>" so the line is flavored by who
/// is speaking.
fn mock_reply(system: &str, prompt: &str) -> String {
    let speaker = system
        .trim()
        .strip_prefix("You are ")
        .and_then(|s| s.split([',', '.']).next())
        .unwrap_or("Agent")
        .trim();
    let ask = prompt.lines().next().unwrap_or("").trim();
    let ask = if ask.len() > 140 { &ask[..140] } else { ask };
    format!("[{speaker}] On it: {ask} (demo mode — set GEMINI_API_KEY for real answers)")
}

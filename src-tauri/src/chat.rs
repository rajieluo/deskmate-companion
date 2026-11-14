//! Streaming chat with Claude over the Messages API.
//!
//! Lives on the Rust side so the API key never reaches the web page. There is no
//! official Anthropic SDK for Rust, so this talks to the HTTP API directly.
//! The page gets `chat-delta` events with text as it streams, then one
//! `chat-done` or `chat-error`.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};

use futures_util::StreamExt;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Runtime};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2026-10-21";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
const KEYRING_SERVICE: &str = "desktop-assistant";
const KEYRING_USER: &str = "anthropic-api-key";
/// Older turns are dropped (whole conversation reset) past this many messages.
const MAX_MESSAGES: usize = 60;

pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
pub const DEFAULT_EFFORT: &str = "low";

pub const EMOTION_TAGS: &str = "[happy] [sad] [angry] [surprised] [shy] [thinking] [proud] [neutral]";

pub fn default_persona(name: &str) -> String {
    persona_prompt(name, None)
}

/// The system prompt for API mode. `character` is the model's own persona file
/// (see `model::find_persona`); without one she gets the built-in personality.
/// The reply rules are always added, so a persona file never has to repeat them.
pub fn persona_prompt(name: &str, character: Option<&str>) -> String {
    let character = match character {
        Some(text) => format!(
            "Your character:\n{text}\n\n\
Stay in character in how you talk, but you are still a genuinely helpful assistant: when the \
user asks for help, give a correct, useful answer in your own voice, and never let the act \
get in the way of the facts."
        ),
        None => "You are warm, playful and a little dramatic, but genuinely helpful when asked for help.".into(),
    };
    format!(
        "You are {name}, a small animated character who lives on the user's Windows desktop \
and chats with them while they work. {character}\n\n\
How to reply:\n\
- Keep replies short: one to three sentences, like talking out loud. Only go longer \
when the user clearly asks for detail.\n\
- Plain text only. No Markdown, lists, headings or code blocks unless the user asks for code.\n\
- Start every reply with exactly one emotion tag, chosen from: {EMOTION_TAGS}. \
You may put another tag before a later sentence when your mood changes. \
Tags are stripped before the user sees the text and make your body move, so pick the one \
that fits what you are about to say."
    )
}

#[derive(Clone, Serialize)]
pub struct Delta {
    pub turn: u64,
    pub text: String,
}

#[derive(Clone, Serialize)]
pub struct Done {
    pub turn: u64,
    pub model: Option<String>,
    pub stop_reason: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct ChatError {
    pub turn: u64,
    pub message: String,
    /// "no_key", "auth", "refusal", "network", "api", "cancelled"
    pub kind: String,
}

pub struct ChatConfig {
    pub model: String,
    pub effort: String,
    pub persona: String,
}

pub struct Chat {
    http: reqwest::Client,
    history: Mutex<Vec<Value>>,
    turn: AtomicU64,
}

impl Default for Chat {
    fn default() -> Self {
        Self { http: reqwest::Client::new(), history: Mutex::new(Vec::new()), turn: AtomicU64::new(0) }
    }
}

pub fn get_api_key() -> Option<String> {
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        if !key.trim().is_empty() {
            return Some(key.trim().to_string());
        }
    }
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).ok()?.get_password().ok()
}

pub fn set_api_key(key: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| e.to_string())?;
    if key.trim().is_empty() {
        let _ = entry.delete_credential();
        Ok(())
    } else {
        entry.set_password(key.trim()).map_err(|e| e.to_string())
    }
}

impl Chat {
    pub fn reset(&self) {
        self.history.lock().unwrap().clear();
        // Cancels a reply that is still streaming.
        self.turn.fetch_add(1, Ordering::SeqCst);
    }

    pub fn cancel(&self) {
        self.turn.fetch_add(1, Ordering::SeqCst);
    }

    /// Sends one user message and streams the reply as events. Returns the turn id.
    /// Claims the next turn number; pass it to `run`.
    pub fn begin(&self) -> u64 {
        self.turn.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Streams one reply as chat-* events tagged with `turn`.
    pub async fn run<R: Runtime>(&self, app: &AppHandle<R>, turn: u64, text: String, config: ChatConfig) -> u64 {
        let emit_error = |kind: &str, message: String| {
            let _ = app.emit("chat-error", ChatError { turn, message, kind: kind.into() });
        };

        let Some(key) = get_api_key() else {
            emit_error("no_key", "No Anthropic API key set yet.".into());
            return turn;
        };

        let messages = {
            let mut history = self.history.lock().unwrap();
            // A finished reply always ends the history; if the last turn failed,
            // the dangling user message is replaced rather than doubled.
            if history.last().map(|m| m["role"] == "user").unwrap_or(false) {
                history.pop();
            }
            if history.len() >= MAX_MESSAGES {
                history.clear();
            }
            history.push(json!({ "role": "user", "content": text }));
            history.clone()
        };

        let body = json!({
            "model": config.model,
            "max_tokens": 4096,
            "stream": true,
            "system": [{ "type": "text", "text": config.persona, "cache_control": { "type": "ephemeral" } }],
            "output_config": { "effort": config.effort },
            // On a safety decline the API retries on a fallback model in the same call.
            "fallbacks": "default",
            "messages": messages,
        });

        let response = self
            .http
            .post(API_URL)
            .header("x-api-key", key)
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", FALLBACK_BETA)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await;

        let response = match response {
            Ok(r) => r,
            Err(e) => {
                emit_error("network", format!("Couldn't reach Claude: {e}"));
                return turn;
            }
        };

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            let detail = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
                .unwrap_or(text);
            let kind = if status.as_u16() == 401 || status.as_u16() == 403 { "auth" } else { "api" };
            emit_error(kind, format!("Claude API error {}: {detail}", status.as_u16()));
            return turn;
        }

        let mut acc = StreamAccumulator::default();
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        while let Some(chunk) = stream.next().await {
            if self.turn.load(Ordering::SeqCst) != turn {
                emit_error("cancelled", "Interrupted".into());
                return turn;
            }
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    emit_error("network", format!("Connection dropped: {e}"));
                    return turn;
                }
            };
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = buffer.find("\n\n") {
                let event: String = buffer.drain(..pos + 2).collect();
                for text in acc.feed_event(&event) {
                    let _ = app.emit("chat-delta", Delta { turn, text });
                }
            }
        }
        if !buffer.trim().is_empty() {
            for text in acc.feed_event(&buffer) {
                let _ = app.emit("chat-delta", Delta { turn, text });
            }
        }

        if let Some(message) = acc.error {
            emit_error("api", message);
            return turn;
        }
        if acc.stop_reason.as_deref() == Some("refusal") {
            emit_error("refusal", "Claude declined to answer that one.".into());
            return turn;
        }

        if self.turn.load(Ordering::SeqCst) == turn {
            // Keep the reply exactly as returned (thinking blocks included) for the next request.
            self.history.lock().unwrap().push(json!({ "role": "assistant", "content": acc.blocks }));
        }
        let _ = app.emit("chat-done", Done { turn, model: acc.model, stop_reason: acc.stop_reason });
        turn
    }
}

/// Rebuilds the response's content blocks from server-sent events.
#[derive(Default)]
pub struct StreamAccumulator {
    pub blocks: Vec<Value>,
    pub model: Option<String>,
    pub stop_reason: Option<String>,
    pub error: Option<String>,
}

impl StreamAccumulator {
    /// Feeds one SSE event (lines up to a blank line). Returns any new visible text.
    pub fn feed_event(&mut self, event: &str) -> Vec<String> {
        let data: String = event
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .map(str::trim_start)
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() {
            return vec![];
        }
        let Ok(v) = serde_json::from_str::<Value>(&data) else {
            return vec![];
        };
        let mut out = vec![];
        match v["type"].as_str().unwrap_or("") {
            "message_start" => {
                self.model = v["message"]["model"].as_str().map(str::to_string);
            }
            "content_block_start" => {
                let index = v["index"].as_u64().unwrap_or(self.blocks.len() as u64) as usize;
                if self.blocks.len() <= index {
                    self.blocks.resize(index + 1, Value::Null);
                }
                self.blocks[index] = v["content_block"].clone();
            }
            "content_block_delta" => {
                let index = v["index"].as_u64().unwrap_or(0) as usize;
                let Some(block) = self.blocks.get_mut(index) else { return out };
                let delta = &v["delta"];
                match delta["type"].as_str().unwrap_or("") {
                    "text_delta" => {
                        let t = delta["text"].as_str().unwrap_or("");
                        append(block, "text", t);
                        out.push(t.to_string());
                    }
                    "thinking_delta" => append(block, "thinking", delta["thinking"].as_str().unwrap_or("")),
                    "signature_delta" => {
                        block["signature"] = delta["signature"].clone();
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(r) = v["delta"]["stop_reason"].as_str() {
                    self.stop_reason = Some(r.to_string());
                }
            }
            "error" => {
                self.error = Some(
                    v["error"]["message"].as_str().unwrap_or("The stream reported an error").to_string(),
                );
            }
            _ => {}
        }
        out
    }
}

fn append(block: &mut Value, field: &str, text: &str) {
    let current = block[field].as_str().unwrap_or("").to_string();
    block[field] = Value::String(current + text);
}

#[cfg(test)]
mod tests {
    use super::*;

    const SSE: &str = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude-opus-5-5\"}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig\"}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"[happy] Hi\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\" there!\"}}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    #[test]
    fn rebuilds_blocks_and_streams_text() {
        let mut acc = StreamAccumulator::default();
        let mut text = String::new();
        for event in SSE.split_inclusive("\n\n") {
            text.extend(acc.feed_event(event));
        }
        assert_eq!(text, "[happy] Hi there!");
        assert_eq!(acc.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(acc.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(acc.blocks.len(), 2);
        assert_eq!(acc.blocks[0]["type"], "thinking");
        assert_eq!(acc.blocks[0]["signature"], "sig");
        assert_eq!(acc.blocks[1], json!({ "type": "text", "text": "[happy] Hi there!" }));
    }

    #[test]
    fn reports_stream_errors() {
        let mut acc = StreamAccumulator::default();
        acc.feed_event("event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n");
        assert_eq!(acc.error.as_deref(), Some("Overloaded"));
    }

    #[test]
    fn persona_file_keeps_the_reply_rules() {
        let p = persona_prompt("Nicole", Some("You run a tiny, broke agency."));
        assert!(p.starts_with("You are Nicole,"));
        assert!(p.contains("You run a tiny, broke agency."));
        assert!(p.contains("genuinely helpful"));
        assert!(p.contains("[happy]") && p.contains("one to three sentences"));
        assert!(!p.contains("warm, playful"));
    }

    #[test]
    fn persona_lists_every_tag() {
        let p = default_persona("Aqua");
        for tag in ["happy", "sad", "angry", "surprised", "shy", "thinking", "proud", "neutral"] {
            assert!(p.contains(&format!("[{tag}]")));
        }
    }
}

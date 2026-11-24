//! Streaming chat for plain chat mode: Claude over the Messages API, or Google
//! Gemini (which has a free tier) over its streamGenerateContent API.
//!
//! Lives on the Rust side so the API key never reaches the web page. There is no
//! official Anthropic or Gemini SDK for Rust, so this talks to the HTTP APIs directly.
//! The page gets `chat-delta` events with text as it streams, then one
//! `chat-done` or `chat-error`, whichever provider answered.

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
const GEMINI_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";
const GEMINI_KEYRING_USER: &str = "gemini-api-key";
/// Older turns are dropped (whole conversation reset) past this many messages.
const MAX_MESSAGES: usize = 60;

pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
pub const DEFAULT_EFFORT: &str = "low";
/// A stable model that Gemini's free tier covers.
pub const DEFAULT_GEMINI_MODEL: &str = "gemini-3.5-flash-lite";

pub const EMOTION_TAGS: &str = "[happy] [sad] [angry] [surprised] [shy] [thinking] [proud] [neutral]";
/// Short emotes the character can do mid-reply (see src/gestures.ts).
pub const GESTURE_TAGS: &str = "[laugh] [giggle] [yawn] [sigh] [blush] [gasp]";

/// Who answers in plain chat mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Provider {
    Anthropic,
    Gemini,
}

impl Provider {
    /// Reads the `chat_provider` setting; anything but "gemini" means Anthropic.
    pub fn from_setting(value: Option<&str>) -> Self {
        match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("gemini") | Some("google") => Provider::Gemini,
            _ => Provider::Anthropic,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::Gemini => "gemini",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Provider::Anthropic => "Claude",
            Provider::Gemini => "Gemini",
        }
    }

    fn env_var(self) -> &'static str {
        match self {
            Provider::Anthropic => "ANTHROPIC_API_KEY",
            Provider::Gemini => "GEMINI_API_KEY",
        }
    }

    fn keyring_user(self) -> &'static str {
        match self {
            Provider::Anthropic => KEYRING_USER,
            Provider::Gemini => GEMINI_KEYRING_USER,
        }
    }
}

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
that fits what you are about to say.\n\
- When you laugh, giggle, yawn, sigh, blush or gasp, you can also put one of {GESTURE_TAGS} \
right before that sentence, and your body does it. Use them now and then, not in every reply."
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
    /// "no_key", "auth", "refusal", "rate_limit", "network", "api", "cancelled"
    pub kind: String,
}

pub struct ChatConfig {
    pub provider: Provider,
    pub model: String,
    pub effort: String,
    pub persona: String,
}

pub struct Chat {
    http: reqwest::Client,
    /// Messages in the format of `history_provider`'s API.
    history: Mutex<Vec<Value>>,
    history_provider: Mutex<Provider>,
    turn: AtomicU64,
}

impl Default for Chat {
    fn default() -> Self {
        Self {
            http: reqwest::Client::new(),
            history: Mutex::new(Vec::new()),
            history_provider: Mutex::new(Provider::Anthropic),
            turn: AtomicU64::new(0),
        }
    }
}

/// The key for `provider`: from its environment variable, else Windows Credential Manager.
pub fn get_api_key(provider: Provider) -> Option<String> {
    if let Ok(key) = std::env::var(provider.env_var()) {
        if !key.trim().is_empty() {
            return Some(key.trim().to_string());
        }
    }
    keyring::Entry::new(KEYRING_SERVICE, provider.keyring_user()).ok()?.get_password().ok()
}

/// Saves `provider`'s key in Windows Credential Manager (an empty key deletes it).
pub fn set_api_key(provider: Provider, key: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, provider.keyring_user()).map_err(|e| e.to_string())?;
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
        let provider = config.provider;
        let who = provider.label();

        let Some(key) = get_api_key(provider) else {
            let message = match provider {
                Provider::Anthropic => "No Anthropic API key set yet.",
                Provider::Gemini => "No Gemini API key set yet.",
            };
            emit_error("no_key", message.into());
            return turn;
        };

        let messages = {
            let mut history = self.history.lock().unwrap();
            // The two APIs keep turns in different shapes, so switching provider
            // starts the conversation over.
            let mut owner = self.history_provider.lock().unwrap();
            if *owner != provider {
                history.clear();
                *owner = provider;
            }
            // A finished reply always ends the history; if the last turn failed,
            // the dangling user message is replaced rather than doubled.
            if history.last().map(|m| m["role"] == "user").unwrap_or(false) {
                history.pop();
            }
            if history.len() >= MAX_MESSAGES {
                history.clear();
            }
            history.push(match provider {
                Provider::Anthropic => json!({ "role": "user", "content": text }),
                Provider::Gemini => json!({ "role": "user", "parts": [{ "text": text }] }),
            });
            history.clone()
        };

        let request = match provider {
            Provider::Anthropic => self
                .http
                .post(API_URL)
                .header("x-api-key", key)
                .header("anthropic-version", API_VERSION)
                .header("anthropic-beta", FALLBACK_BETA)
                .json(&json!({
                    "model": config.model,
                    "max_tokens": 4096,
                    "stream": true,
                    "system": [{ "type": "text", "text": config.persona, "cache_control": { "type": "ephemeral" } }],
                    "output_config": { "effort": config.effort },
                    // On a safety decline the API retries on a fallback model in the same call.
                    "fallbacks": "default",
                    "messages": messages,
                })),
            Provider::Gemini => self
                .http
                .post(format!("{GEMINI_URL}/{}:streamGenerateContent?alt=sse", config.model.trim()))
                .header("x-goog-api-key", key)
                .json(&gemini_body(&config, messages)),
        };

        let response = match request.header("content-type", "application/json").send().await {
            Ok(r) => r,
            Err(e) => {
                emit_error("network", format!("Couldn't reach {who}: {e}"));
                return turn;
            }
        };

        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            let (kind, message) = match provider {
                Provider::Anthropic => anthropic_error(status.as_u16(), &text),
                Provider::Gemini => gemini_error(status.as_u16(), &text),
            };
            emit_error(kind, message);
            return turn;
        }

        let mut acc: Box<dyn Accumulator> = match provider {
            Provider::Anthropic => Box::new(StreamAccumulator::default()),
            Provider::Gemini => Box::new(GeminiAccumulator::default()),
        };
        let mut stream = response.bytes_stream();
        // Raw bytes, so a character split across two chunks isn't garbled.
        let mut buffer: Vec<u8> = Vec::new();
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
            buffer.extend_from_slice(&chunk);
            while let Some((end, sep)) = next_event_end(&buffer) {
                let event: Vec<u8> = buffer.drain(..end + sep).collect();
                for text in acc.feed(&String::from_utf8_lossy(&event)) {
                    let _ = app.emit("chat-delta", Delta { turn, text });
                }
            }
        }
        let rest = String::from_utf8_lossy(&buffer).to_string();
        if !rest.trim().is_empty() {
            for text in acc.feed(&rest) {
                let _ = app.emit("chat-delta", Delta { turn, text });
            }
        }

        let outcome = acc.finish();
        if let Some(message) = outcome.error {
            emit_error("api", message);
            return turn;
        }
        if outcome.refused {
            emit_error("refusal", format!("{who} declined to answer that one."));
            return turn;
        }

        if self.turn.load(Ordering::SeqCst) == turn {
            // Keep the reply exactly as returned (thinking blocks and thought
            // signatures included) for the next request.
            self.history.lock().unwrap().push(outcome.reply);
        }
        let _ = app.emit("chat-done", Done { turn, model: outcome.model, stop_reason: outcome.stop_reason });
        turn
    }
}

/// Where the first SSE event in `buffer` ends: (index of the blank line, its length).
/// Accepts both `\n\n` and `\r\n\r\n` separators (Gemini sends the latter).
fn next_event_end(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer.windows(2).position(|w| w == b"\n\n").map(|i| (i, 2));
    let crlf = buffer.windows(4).position(|w| w == b"\r\n\r\n").map(|i| (i, 4));
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (a, b) => a.or(b),
    }
}

/// The JSON data of one SSE event (its `data:` lines joined).
fn sse_data(event: &str) -> Option<Value> {
    let data: String = event
        .lines()
        .filter_map(|l| l.strip_prefix("data:"))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() {
        return None;
    }
    serde_json::from_str::<Value>(&data).ok()
}

/// What a finished stream left behind, whichever provider sent it.
pub struct Outcome {
    /// The assistant turn to add to the history, in the provider's own format.
    pub reply: Value,
    pub model: Option<String>,
    pub stop_reason: Option<String>,
    pub refused: bool,
    pub error: Option<String>,
}

trait Accumulator: Send {
    /// Feeds one SSE event. Returns any new visible text.
    fn feed(&mut self, event: &str) -> Vec<String>;
    fn finish(self: Box<Self>) -> Outcome;
}

fn anthropic_error(status: u16, body: &str) -> (&'static str, String) {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_else(|| body.to_string());
    let kind = if status == 401 || status == 403 { "auth" } else { "api" };
    (kind, format!("Claude API error {status}: {detail}"))
}

/// Rebuilds the response's content blocks from server-sent events.
#[derive(Default)]
pub struct StreamAccumulator {
    pub blocks: Vec<Value>,
    pub model: Option<String>,
    pub stop_reason: Option<String>,
    pub error: Option<String>,
}

impl Accumulator for StreamAccumulator {
    fn feed(&mut self, event: &str) -> Vec<String> {
        self.feed_event(event)
    }

    fn finish(self: Box<Self>) -> Outcome {
        Outcome {
            refused: self.stop_reason.as_deref() == Some("refusal"),
            reply: json!({ "role": "assistant", "content": self.blocks }),
            model: self.model,
            stop_reason: self.stop_reason,
            error: self.error,
        }
    }
}

impl StreamAccumulator {
    /// Feeds one SSE event (lines up to a blank line). Returns any new visible text.
    pub fn feed_event(&mut self, event: &str) -> Vec<String> {
        let Some(v) = sse_data(event) else {
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

// --- Gemini

/// The streamGenerateContent request body. `contents` is the history in Gemini's shape.
fn gemini_body(config: &ChatConfig, contents: Vec<Value>) -> Value {
    let mut generation = json!({ "maxOutputTokens": 4096 });
    if let Some(level) = thinking_level(&config.model, &config.effort) {
        generation["thinkingConfig"] = json!({ "thinkingLevel": level });
    }
    json!({
        "systemInstruction": { "parts": [{ "text": config.persona }] },
        "contents": contents,
        "generationConfig": generation,
    })
}

/// Maps `chat_effort` onto Gemini's thinking level. Gemini 2.x models reject the
/// setting, so they get none and think at their default.
fn thinking_level(model: &str, effort: &str) -> Option<&'static str> {
    let model = model.trim().to_ascii_lowercase();
    if model.starts_with("gemini-1") || model.starts_with("gemini-2") {
        return None;
    }
    Some(match effort.trim().to_ascii_lowercase().as_str() {
        "medium" => "MEDIUM",
        "high" => "HIGH",
        "xhigh" | "max" => "XHIGH",
        _ => "LOW",
    })
}

/// Turns a failed Gemini response into an error kind and a message for the user.
pub fn gemini_error(status: u16, body: &str) -> (&'static str, String) {
    let v = serde_json::from_str::<Value>(body).unwrap_or(Value::Null);
    let error = &v["error"];
    let detail = error["message"].as_str().map(str::to_string).unwrap_or_else(|| body.trim().to_string());
    let reasons: Vec<&str> =
        error["details"].as_array().into_iter().flatten().filter_map(|d| d["reason"].as_str()).collect();
    if status == 429 {
        // Per-minute and per-day quotas both answer 429; the quota id says which.
        let message = if body.contains("PerMinute") && !body.contains("PerDay") {
            "Too many messages for Gemini's free tier in one minute. Wait a minute and try again."
        } else {
            "Gemini's free daily limit has been reached. It resets at midnight Pacific time; try again then."
        };
        return ("rate_limit", message.into());
    }
    if status == 401
        || status == 403
        || reasons.iter().any(|r| *r == "API_KEY_INVALID")
        || detail.contains("API key not valid")
    {
        return ("auth", format!("Gemini API error {status}: {detail}"));
    }
    ("api", format!("Gemini API error {status}: {detail}"))
}

/// Rebuilds a Gemini reply from streamed GenerateContentResponse chunks.
#[derive(Default)]
pub struct GeminiAccumulator {
    /// The reply's parts as returned (thought parts and signatures included).
    pub parts: Vec<Value>,
    pub model: Option<String>,
    pub finish_reason: Option<String>,
    pub block_reason: Option<String>,
    pub error: Option<String>,
}

impl GeminiAccumulator {
    /// Feeds one SSE event. Returns any new visible text.
    pub fn feed_event(&mut self, event: &str) -> Vec<String> {
        let Some(v) = sse_data(event) else {
            return vec![];
        };
        let mut out = vec![];
        if v["error"].is_object() {
            self.error =
                Some(v["error"]["message"].as_str().unwrap_or("The stream reported an error").to_string());
            return out;
        }
        if let Some(m) = v["modelVersion"].as_str() {
            self.model = Some(m.to_string());
        }
        if let Some(r) = v["promptFeedback"]["blockReason"].as_str() {
            self.block_reason = Some(r.to_string());
        }
        let candidate = &v["candidates"][0];
        if let Some(r) = candidate["finishReason"].as_str() {
            self.finish_reason = Some(r.to_string());
        }
        for part in candidate["content"]["parts"].as_array().into_iter().flatten() {
            let thought = part["thought"].as_bool().unwrap_or(false);
            let Some(text) = part["text"].as_str() else {
                self.parts.push(part.clone());
                continue;
            };
            if !thought {
                if !text.is_empty() {
                    out.push(text.to_string());
                }
                // Streamed text arrives as many small parts; join plain ones up.
                if part.get("thoughtSignature").is_none() {
                    if let Some(last) = self.parts.last_mut() {
                        if last["text"].is_string() && !last["thought"].as_bool().unwrap_or(false) {
                            append(last, "text", text);
                            continue;
                        }
                    }
                }
            }
            self.parts.push(part.clone());
        }
        out
    }

    fn refused(&self) -> bool {
        self.block_reason.is_some()
            || matches!(
                self.finish_reason.as_deref(),
                Some("SAFETY" | "PROHIBITED_CONTENT" | "BLOCKLIST" | "SPII" | "RECITATION")
            )
    }
}

impl Accumulator for GeminiAccumulator {
    fn feed(&mut self, event: &str) -> Vec<String> {
        self.feed_event(event)
    }

    fn finish(self: Box<Self>) -> Outcome {
        let refused = self.refused();
        // The API rejects a turn with no parts.
        let parts = if self.parts.is_empty() { vec![json!({ "text": "" })] } else { self.parts };
        Outcome {
            refused,
            reply: json!({ "role": "model", "parts": parts }),
            model: self.model,
            stop_reason: self.finish_reason.map(|r| r.to_ascii_lowercase()),
            error: self.error,
        }
    }
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

    // Gemini's alt=sse stream: one GenerateContentResponse per event, CRLF-separated.
    const GEMINI_SSE: &str = concat!(
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Pondering\",\"thought\":true}],\"role\":\"model\"}}],\"modelVersion\":\"gemini-3.5-flash-lite\"}\r\n\r\n",
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"[happy] Hi\",\"thoughtSignature\":\"c2ln\"}],\"role\":\"model\"}}],\"modelVersion\":\"gemini-3.5-flash-lite\"}\r\n\r\n",
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\" there\"}],\"role\":\"model\"}}]}\r\n\r\n",
        "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"!\"}],\"role\":\"model\"},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"totalTokenCount\":9}}\r\n\r\n",
    );

    /// Splits a byte stream the way `Chat::run` does.
    fn events(stream: &str) -> Vec<String> {
        let mut buffer = stream.as_bytes().to_vec();
        let mut out = vec![];
        while let Some((end, sep)) = next_event_end(&buffer) {
            let event: Vec<u8> = buffer.drain(..end + sep).collect();
            out.push(String::from_utf8(event).unwrap());
        }
        out
    }

    #[test]
    fn gemini_streams_text_and_keeps_thoughts_out_of_it() {
        let mut acc = GeminiAccumulator::default();
        let mut text = String::new();
        let events = events(GEMINI_SSE);
        assert_eq!(events.len(), 4);
        for event in &events {
            text.extend(acc.feed_event(event));
        }
        assert_eq!(text, "[happy] Hi there!");
        assert_eq!(acc.model.as_deref(), Some("gemini-3.5-flash-lite"));
        assert!(!acc.refused());
        let outcome = Box::new(acc).finish();
        assert_eq!(outcome.stop_reason.as_deref(), Some("stop"));
        assert!(outcome.error.is_none());
        assert_eq!(
            outcome.reply,
            json!({ "role": "model", "parts": [
                { "text": "Pondering", "thought": true },
                { "text": "[happy] Hi there!", "thoughtSignature": "c2ln" },
            ] })
        );
    }

    #[test]
    fn gemini_blocks_count_as_refusals() {
        let mut acc = GeminiAccumulator::default();
        acc.feed_event("data: {\"promptFeedback\":{\"blockReason\":\"SAFETY\"}}\r\n\r\n");
        assert!(acc.refused());

        let mut acc = GeminiAccumulator::default();
        acc.feed_event("data: {\"candidates\":[{\"finishReason\":\"PROHIBITED_CONTENT\"}]}\n\n");
        assert!(acc.refused());
        let outcome = Box::new(acc).finish();
        assert_eq!(outcome.reply["parts"], json!([{ "text": "" }]));
    }

    #[test]
    fn gemini_reports_stream_errors() {
        let mut acc = GeminiAccumulator::default();
        acc.feed_event("data: {\"error\":{\"code\":503,\"message\":\"The model is overloaded.\",\"status\":\"UNAVAILABLE\"}}\r\n\r\n");
        assert_eq!(acc.error.as_deref(), Some("The model is overloaded."));
    }

    #[test]
    fn gemini_daily_limit_is_a_plain_message() {
        let body = r#"{"error":{"code":429,"message":"You exceeded your current quota.","status":"RESOURCE_EXHAUSTED",
            "details":[{"@type":"type.googleapis.com/google.rpc.QuotaFailure","violations":[
            {"quotaMetric":"generativelanguage.googleapis.com/generate_content_free_tier_requests",
             "quotaId":"GenerateRequestsPerDayPerProjectPerModel-FreeTier"}]}]}}"#;
        let (kind, message) = gemini_error(429, body);
        assert_eq!(kind, "rate_limit");
        assert!(message.contains("free daily limit"));
        assert!(!message.contains("RESOURCE_EXHAUSTED"));

        let minute = body.replace("PerDay", "PerMinute");
        let (kind, message) = gemini_error(429, &minute);
        assert_eq!(kind, "rate_limit");
        assert!(message.contains("one minute"));

        // A 429 with no readable body still reads as the daily limit.
        assert_eq!(gemini_error(429, "").0, "rate_limit");
    }

    #[test]
    fn gemini_bad_key_asks_for_a_new_one() {
        let body = r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT",
            "details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"API_KEY_INVALID"}]}}"#;
        assert_eq!(gemini_error(400, body).0, "auth");
        assert_eq!(gemini_error(403, r#"{"error":{"code":403,"message":"denied"}}"#).0, "auth");
        let (kind, message) = gemini_error(404, r#"{"error":{"code":404,"message":"models/nope is not found"}}"#);
        assert_eq!(kind, "api");
        assert_eq!(message, "Gemini API error 404: models/nope is not found");
    }

    #[test]
    fn gemini_body_has_persona_history_and_thinking() {
        let config = ChatConfig {
            provider: Provider::Gemini,
            model: "gemini-3.5-flash-lite".into(),
            effort: "low".into(),
            persona: "You are Aqua.".into(),
        };
        let contents = vec![json!({ "role": "user", "parts": [{ "text": "hi" }] })];
        let body = gemini_body(&config, contents.clone());
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "You are Aqua.");
        assert_eq!(body["contents"], json!(contents));
        assert_eq!(body["generationConfig"]["thinkingConfig"]["thinkingLevel"], "LOW");

        let old = ChatConfig { model: "gemini-2.5-flash-lite".into(), ..config };
        assert!(gemini_body(&old, vec![])["generationConfig"].get("thinkingConfig").is_none());
    }

    #[test]
    fn provider_setting_defaults_to_anthropic() {
        assert_eq!(Provider::from_setting(None), Provider::Anthropic);
        assert_eq!(Provider::from_setting(Some("anthropic")), Provider::Anthropic);
        assert_eq!(Provider::from_setting(Some("nonsense")), Provider::Anthropic);
        assert_eq!(Provider::from_setting(Some(" Gemini ")), Provider::Gemini);
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

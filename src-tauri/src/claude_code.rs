//! "Claude Code mode": instead of calling the API, each message runs the
//! `claude` CLI headless in a folder the user picked, using their own Claude
//! Code login. Replies stream back as the same `chat-delta` / `chat-done` /
//! `chat-error` events the API mode uses, plus `chat-activity` lines for tool use
//! ("Reading src/main.ts…").

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::chat::{ChatError, Delta, Done, EMOTION_TAGS};

pub const DEFAULT_PERMISSION_MODE: &str = "acceptEdits";

#[derive(Clone, Serialize)]
pub struct Activity {
    pub turn: u64,
    pub text: String,
}

pub fn avatar_prompt(name: &str) -> String {
    format!(
        "You are also {name}, an animated character on the user's desktop, and everything you write \
is shown in a small speech bubble and read aloud. Work on the user's request as usual, but keep \
what you write short and conversational: no Markdown, no lists, no code blocks in your messages \
(put code in files instead), and don't narrate each step, since the user already sees which tools \
you use. Finish with one to three spoken sentences saying what you did or found. Start each \
message with exactly one emotion tag from: {EMOTION_TAGS}. The tags are hidden from the user and \
make your body move."
    )
}

#[derive(Default)]
pub struct ClaudeCode {
    session: Mutex<Option<String>>,
    turn: AtomicU64,
}

pub struct RunConfig {
    pub folder: PathBuf,
    pub permission_mode: String,
    pub prompt_file: PathBuf,
    pub name: String,
}

/// Finds the `claude` executable: claude.exe (native installer) or claude.cmd (npm) on PATH.
pub fn find_claude() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) { &["claude.exe", "claude.cmd"] } else { &["claude"] };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    // Common install locations that may be missing from a GUI app's PATH.
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        let home = PathBuf::from(home);
        dirs.push(home.join(".local").join("bin"));
        dirs.push(home.join(".claude").join("local"));
        if cfg!(windows) {
            dirs.push(home.join("AppData").join("Roaming").join("npm"));
        }
    }
    for dir in dirs {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

impl ClaudeCode {
    pub fn reset(&self) {
        *self.session.lock().unwrap() = None;
        self.turn.fetch_add(1, Ordering::SeqCst);
    }

    pub fn cancel(&self) {
        self.turn.fetch_add(1, Ordering::SeqCst);
    }

    /// Claims the next turn number; pass it to `run`.
    pub fn begin(&self) -> u64 {
        self.turn.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Streams one reply as chat-* events tagged with `turn`.
    pub async fn run<R: Runtime>(&self, app: &AppHandle<R>, turn: u64, text: String, config: RunConfig) -> u64 {
        let error = |kind: &str, message: String| {
            let _ = app.emit("chat-error", ChatError { turn, message, kind: kind.into() });
        };

        let Some(claude) = find_claude() else {
            error("api", "I can't find Claude Code. Install it from https://claude.com/claude-code, sign in once in a terminal, then try again.".into());
            return turn;
        };
        if !config.folder.is_dir() {
            error("no_folder", "Pick a folder for Claude Code to work in first.".into());
            return turn;
        }
        if let Err(e) = std::fs::write(&config.prompt_file, avatar_prompt(&config.name)) {
            error("api", format!("Couldn't write the avatar prompt: {e}"));
            return turn;
        }

        let mut cmd = tokio::process::Command::new(&claude);
        cmd.current_dir(&config.folder)
            .args(["-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages"])
            .arg("--permission-mode")
            .arg(&config.permission_mode)
            .arg("--append-system-prompt-file")
            .arg(&config.prompt_file);
        if let Some(session) = self.session.lock().unwrap().clone() {
            cmd.arg("--resume").arg(session);
        }
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        #[cfg(windows)]
        {
            // No console window flashing up for each message.
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                error("api", format!("Couldn't start Claude Code: {e}"));
                return turn;
            }
        };
        // The message goes in on stdin, so it never has to be quoted on a command line.
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes()).await;
        }

        let stdout = child.stdout.take().expect("piped stdout");
        let mut lines = BufReader::new(stdout).lines();
        let mut parser = StreamParser::default();
        loop {
            if self.turn.load(Ordering::SeqCst) != turn {
                let _ = child.kill().await;
                error("cancelled", "Interrupted".into());
                return turn;
            }
            let line = match tokio::time::timeout(std::time::Duration::from_millis(250), lines.next_line()).await {
                Err(_) => continue, // re-check for cancellation
                Ok(Ok(Some(line))) => line,
                Ok(Ok(None)) => break,
                Ok(Err(e)) => {
                    error("api", format!("Lost Claude Code's output: {e}"));
                    return turn;
                }
            };
            for out in parser.feed(&line) {
                match out {
                    Out::Text(t) => {
                        let _ = app.emit("chat-delta", Delta { turn, text: t });
                    }
                    Out::Activity(t) => {
                        let _ = app.emit("chat-activity", Activity { turn, text: t });
                    }
                }
            }
        }

        let status = child.wait().await.ok();
        if let Some(id) = parser.session_id.clone() {
            *self.session.lock().unwrap() = Some(id);
        }
        match parser.result {
            Some(Ok(())) => {
                let _ = app.emit("chat-done", Done { turn, model: parser.model, stop_reason: None });
            }
            Some(Err(message)) => error("api", message),
            None => {
                let mut stderr = String::new();
                if let Some(mut e) = child.stderr.take() {
                    use tokio::io::AsyncReadExt;
                    let _ = e.read_to_string(&mut stderr).await;
                }
                let hint = if stderr.to_lowercase().contains("login") || stderr.contains("auth") {
                    " Open a terminal and run `claude` once to sign in."
                } else {
                    ""
                };
                error(
                    "api",
                    format!(
                        "Claude Code stopped ({}). {}{hint}",
                        status.map(|s| s.to_string()).unwrap_or_default(),
                        stderr.lines().last().unwrap_or("")
                    ),
                );
            }
        }
        turn
    }
}

pub enum Out {
    Text(String),
    Activity(String),
}

/// Reads Claude Code's `stream-json` output, one JSON object per line.
#[derive(Default)]
pub struct StreamParser {
    pub session_id: Option<String>,
    pub model: Option<String>,
    pub result: Option<Result<(), String>>,
    /// Text already streamed for the current assistant message (from partial events).
    streamed_partial: bool,
    /// Separates text from consecutive assistant messages.
    wrote_text: bool,
    need_break: bool,
}

impl StreamParser {
    pub fn feed(&mut self, line: &str) -> Vec<Out> {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { return vec![] };
        let mut out = vec![];
        match v["type"].as_str().unwrap_or("") {
            "system" => {
                if let Some(id) = v["session_id"].as_str() {
                    self.session_id = Some(id.to_string());
                }
                if let Some(m) = v["model"].as_str() {
                    self.model = Some(m.to_string());
                }
            }
            "stream_event" => {
                let e = &v["event"];
                match e["type"].as_str().unwrap_or("") {
                    "message_start" => {
                        self.streamed_partial = false;
                        if self.wrote_text {
                            self.need_break = true;
                        }
                    }
                    "content_block_delta" if e["delta"]["type"] == "text_delta" => {
                        let t = e["delta"]["text"].as_str().unwrap_or("");
                        if !t.is_empty() {
                            self.push_text(t, &mut out);
                            self.streamed_partial = true;
                        }
                    }
                    _ => {}
                }
            }
            "assistant" => {
                for block in v["message"]["content"].as_array().into_iter().flatten() {
                    match block["type"].as_str().unwrap_or("") {
                        // Without partial events, the whole text arrives here.
                        "text" if !self.streamed_partial => {
                            if self.wrote_text {
                                self.need_break = true;
                            }
                            let t = block["text"].as_str().unwrap_or("").to_string();
                            self.push_text(&t, &mut out);
                        }
                        "tool_use" => out.push(Out::Activity(describe_tool(block))),
                        _ => {}
                    }
                }
                self.streamed_partial = false;
            }
            "result" => {
                if let Some(id) = v["session_id"].as_str() {
                    self.session_id = Some(id.to_string());
                }
                let failed = v["is_error"].as_bool().unwrap_or(false) || v["subtype"].as_str().is_some_and(|s| s != "success");
                self.result = Some(if failed {
                    Err(v["result"].as_str().map(str::to_string).unwrap_or_else(|| {
                        format!("Claude Code reported an error ({})", v["subtype"].as_str().unwrap_or("unknown"))
                    }))
                } else {
                    Ok(())
                });
            }
            _ => {}
        }
        out
    }

    fn push_text(&mut self, t: &str, out: &mut Vec<Out>) {
        if self.need_break {
            out.push(Out::Text("\n".into()));
            self.need_break = false;
        }
        out.push(Out::Text(t.to_string()));
        self.wrote_text = true;
    }
}

/// A short human description of a tool call, e.g. "Reading main.ts".
pub fn describe_tool(block: &Value) -> String {
    let input = &block["input"];
    let file = |key: &str| {
        input[key].as_str().map(|p| Path::new(p).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string()))
    };
    let short = |s: &str| {
        let s = s.lines().next().unwrap_or("");
        if s.chars().count() > 48 { format!("{}…", s.chars().take(47).collect::<String>()) } else { s.to_string() }
    };
    let name = block["name"].as_str().unwrap_or("a tool");
    match name {
        "Read" => format!("Reading {}", file("file_path").unwrap_or_default()),
        "Write" => format!("Writing {}", file("file_path").unwrap_or_default()),
        "Edit" | "MultiEdit" => format!("Editing {}", file("file_path").unwrap_or_default()),
        "Bash" => format!("Running {}", short(input["command"].as_str().unwrap_or(""))),
        "Grep" => format!("Searching for {}", short(input["pattern"].as_str().unwrap_or(""))),
        "Glob" => format!("Looking for {}", short(input["pattern"].as_str().unwrap_or(""))),
        "WebSearch" => format!("Searching the web for {}", short(input["query"].as_str().unwrap_or(""))),
        "WebFetch" => "Reading a web page".into(),
        "TodoWrite" => "Planning".into(),
        "Task" | "Agent" => "Asking a helper".into(),
        other => format!("Using {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(out: Vec<Out>) -> (String, Vec<String>) {
        let mut text = String::new();
        let mut acts = vec![];
        for o in out {
            match o {
                Out::Text(t) => text.push_str(&t),
                Out::Activity(a) => acts.push(a),
            }
        }
        (text, acts)
    }

    #[test]
    fn streams_partial_text_and_tool_activity() {
        let mut p = StreamParser::default();
        let lines = [
            r#"{"type":"system","subtype":"init","session_id":"s1","model":"claude-opus-5-5"}"#,
            r#"{"type":"stream_event","event":{"type":"message_start","message":{}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"[thinking] Let me look"}}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"[thinking] Let me look"},{"type":"tool_use","name":"Read","input":{"file_path":"C:\\code\\src\\main.ts"}}]}}"#,
            r#"{"type":"stream_event","event":{"type":"message_start","message":{}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"[happy] Found it!"}}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"[happy] Found it!"}]}}"#,
            r#"{"type":"result","subtype":"success","is_error":false,"session_id":"s1","result":"[happy] Found it!"}"#,
        ];
        let mut all = vec![];
        for l in lines {
            all.extend(p.feed(l));
        }
        let (text, acts) = texts(all);
        assert_eq!(text, "[thinking] Let me look\n[happy] Found it!");
        assert_eq!(acts.len(), 1);
        assert!(acts[0].starts_with("Reading ") && acts[0].ends_with("main.ts"));
        assert_eq!(p.session_id.as_deref(), Some("s1"));
        assert!(matches!(p.result, Some(Ok(()))));
    }

    #[test]
    fn falls_back_to_whole_messages_and_reports_errors() {
        let mut p = StreamParser::default();
        let (text, _) = texts(p.feed(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"[sad] No access."}]}}"#));
        assert_eq!(text, "[sad] No access.");
        p.feed(r#"{"type":"result","subtype":"error_during_execution","is_error":true}"#);
        assert!(matches!(p.result, Some(Err(_))));
    }

    #[test]
    fn describes_bash_briefly() {
        let block: Value = serde_json::json!({"name":"Bash","input":{"command":"npm test -- --run a-very-long-argument-list-that-goes-on-and-on"}});
        let d = describe_tool(&block);
        assert!(d.starts_with("Running npm test") && d.chars().count() <= 57);
    }

    #[test]
    fn prompt_has_no_newlines() {
        assert!(!avatar_prompt("Aqua").contains('\n'));
    }
}

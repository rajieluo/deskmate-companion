//! Crew mode with her as the first mate. Instead of passing notes to a separate
//! first-mate session, the app runs firstmate's first mate itself: one long-running
//! headless Claude Code session in the firstmate home (`claude -p --input-format
//! stream-json`), with her character on top. The captain's messages go straight in
//! on stdin and her replies stream back as the usual chat-* events. Turns she starts
//! by herself (firstmate's watcher waking her for crew news) are read out as
//! `crew-reply`.
//!
//! Firstmate's startup runs once, when the session starts (about a minute), so the
//! session is started as soon as Crew mode is on and kept running.

use std::{
    collections::VecDeque,
    process::Stdio,
    sync::{Arc, Mutex},
};

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use crate::chat::{ChatError, Delta, Done, EMOTION_TAGS};
use crate::claude_code::{Activity, Out, StreamParser};
use crate::crew::{self, CrewConfig, Shell};
use crate::model::ModelStore;

pub const DEFAULT_PERMISSION_MODE: &str = "auto";

/// The app's first message to a new session: it only makes firstmate's session start run.
pub const WARM_UP: &str = "The desktop app just started this session. Do your session start as AGENTS.md says, \
handle anything it reports that needs handling, then reply with only the tag [neutral] unless the captain \
needs to hear something.";

/// True when Crew mode runs the first mate in the app (the default).
pub fn direct(store: &ModelStore) -> bool {
    store.settings().crew_direct.unwrap_or(true)
}

/// The instructions added to firstmate's own (AGENTS.md) for this session.
pub fn mate_prompt(name: &str, character: Option<&str>) -> String {
    let mut p = format!(
        "You are the first mate of this firstmate home, exactly as AGENTS.md describes, and you are also \
{name}, an animated character on the captain's desktop. The captain talks to you through {name}: \
their messages arrive here, typed or spoken, and everything you write in a reply is shown in a small \
speech bubble and read aloud. There is no terminal the captain is watching.\n\n\
So every reply is speech: one to three short spoken sentences, plain text, no Markdown, lists, code \
or paths unless the captain asks for one. Start each message with exactly one emotion tag from: \
{EMOTION_TAGS}. The tags are hidden and make {name} move. Detail that doesn't fit in speech belongs \
in a file, a PR or the task's own records, and you can say where it is.\n\n\
When you wake by yourself (a queued wake, crew status, a PR, a check), handle it as AGENTS.md says. \
If the captain needs to hear about it (a decision, a blocker, a failure, a PR ready or merged, a \
finished request), say so in the same spoken style. If it needs nothing from the captain, reply with \
only the tag [neutral] and no words, so {name} stays quiet.\n\n\
On this Windows home, `fm-inbox.sh ready` can report live_harness false even while you hold the \
session lock. If session start says this session holds the lock, it does.\n\n\
The work itself (tasks, briefs, PRs, commits, decisions, facts) stays exactly as careful and \
complete as AGENTS.md requires."
    );
    if let Some(text) = character {
        p.push_str(&format!(
            "\n\n{name}'s character, for how you talk to the captain:\n{text}\n\n\
Let the character color only your spoken replies. PR titles and bodies, commit messages, task \
briefs and status lines stay plain and factual, and crewmates get no character."
        ));
    }
    p
}

/// One stream-json input line carrying a user message.
pub fn user_line(text: &str) -> String {
    let mut line = serde_json::json!({
        "type": "user",
        "message": { "role": "user", "content": text },
    })
    .to_string();
    line.push('\n');
    line
}

/// True for a reply with nothing to say out loud: only tags, or firstmate's no-op line.
pub fn is_quiet(text: &str) -> bool {
    let mut plain = String::new();
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '[' => in_tag = true,
            ']' => in_tag = false,
            c if !in_tag => plain.push(c),
            _ => {}
        }
    }
    let plain = plain.trim().trim_end_matches('.').to_lowercase();
    plain.is_empty() || plain.ends_with("shipshape") && plain.len() < 40
}

/// What one stdout line means for turn bookkeeping.
#[derive(Debug, PartialEq)]
pub enum Edge {
    None,
    /// The model finished a reply (end_turn).
    EndTurn,
    /// The `result` line after a turn (after the Stop hooks), with its error if any.
    Result(Option<String>),
}

pub fn edge(line: &str) -> Edge {
    let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { return Edge::None };
    match v["type"].as_str().unwrap_or("") {
        "stream_event" if v["event"]["type"] == "message_delta" && v["event"]["delta"]["stop_reason"] == "end_turn" => {
            Edge::EndTurn
        }
        "result" => {
            let failed = v["is_error"].as_bool().unwrap_or(false) || v["subtype"].as_str().is_some_and(|s| s != "success");
            Edge::Result(failed.then(|| v["result"].as_str().unwrap_or("The first mate reported an error.").to_string()))
        }
        _ => Edge::None,
    }
}

/// Who the text being streamed belongs to.
#[derive(Default)]
struct Turns {
    /// Messages not yet answered, oldest first: the captain's turn, or None for the
    /// app's own start-up message.
    pending: VecDeque<Option<u64>>,
    /// The reply being streamed is for this message (None: a turn she started herself).
    current: Option<u64>,
    /// A reply is being streamed.
    open: bool,
    /// The reply answers the front of `pending` (else she started the turn herself).
    answering: bool,
    /// Text of a turn she started herself, read out when it ends.
    own: String,
}

struct Live {
    stdin: tokio::process::ChildStdin,
    /// Bumped per process, so a dead one's reader doesn't clear a newer one.
    generation: u64,
}

#[derive(Default)]
pub struct FirstMate {
    live: tokio::sync::Mutex<Option<Live>>,
    turns: Arc<Mutex<Turns>>,
    generation: Mutex<u64>,
}

impl FirstMate {
    /// Sends the captain's message `text` as turn `turn`. Starts the session if needed.
    pub async fn send<R: Runtime>(self: &Arc<Self>, app: &AppHandle<R>, turn: u64, text: &str) -> Result<(), String> {
        let mut live = self.live.lock().await;
        if live.is_none() {
            let _ = app.emit("chat-activity", Activity { turn, text: "Getting the crew's records out…".into() });
            *live = Some(self.spawn(app).await?);
        }
        {
            let mut t = self.turns.lock().unwrap();
            if t.pending.front() == Some(&None) {
                let text = "Still getting the crew's records out. The first start takes a few minutes…".into();
                let _ = app.emit("chat-activity", Activity { turn, text });
            }
            t.pending.push_back(Some(turn));
        }
        let l = live.as_mut().expect("started");
        if let Err(e) = l.stdin.write_all(user_line(text).as_bytes()).await {
            self.turns.lock().unwrap().pending.retain(|t| *t != Some(turn));
            *live = None;
            return Err(format!("I lost the first mate ({e}). Try again and I'll start it again."));
        }
        let _ = l.stdin.flush().await;
        Ok(())
    }

    /// Starts the session now, so the first message doesn't wait for firstmate's startup.
    pub fn warm<R: Runtime>(self: &Arc<Self>, app: AppHandle<R>) {
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut live = me.live.lock().await;
            if live.is_none() {
                match me.spawn(&app).await {
                    Ok(l) => *live = Some(l),
                    Err(e) => log::warn!("first mate: couldn't start: {e}"),
                }
            }
        });
    }

    /// Ends the session: closing stdin lets Claude Code finish and exit by itself.
    pub fn stop(self: &Arc<Self>) {
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            me.live.lock().await.take();
        });
    }

    async fn spawn<R: Runtime>(self: &Arc<Self>, app: &AppHandle<R>) -> Result<Live, String> {
        let store = app.state::<Arc<ModelStore>>().inner().clone();
        let config = CrewConfig::from_settings(&store);
        app.state::<Arc<crew::Crew>>().inner().clone().ensure_checked(app, &config).await?;
        let s = store.settings();
        let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let prompt_file = dir.join("first-mate-prompt.md");
        let persona = store.model_persona().or_else(|| s.persona.clone().filter(|p| !p.trim().is_empty()));
        std::fs::write(&prompt_file, mate_prompt(&store.character_name(), persona.as_deref()))
            .map_err(|e| format!("Couldn't write {}: {e}", prompt_file.display()))?;
        let prompt_arg = match config.shell {
            Shell::GitBash(_) => crew::to_git_bash_path(&prompt_file),
            Shell::Wsl => crew::to_wsl_path(&prompt_file),
            Shell::Unix => prompt_file.to_string_lossy().into_owned(),
        };
        let mode = s.crew_permission_mode.clone().unwrap_or_else(|| DEFAULT_PERMISSION_MODE.into());
        let resume = s.crew_session.clone().unwrap_or_default();
        let helper = crew::helper_path(app)?;
        let mut args = vec!["mate", prompt_arg.as_str(), mode.as_str()];
        if !resume.is_empty() {
            args.push(resume.as_str());
        }
        let mut cmd = crew::helper_command(&helper, &config, &args);
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| format!("Couldn't start the first mate: {e}"))?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        // Claude Code runs firstmate's session start only when the first message
        // arrives, and it can take minutes. Send one of the app's own now, so the
        // captain's first message doesn't wait for it.
        self.turns.lock().unwrap().pending.push_back(None);
        stdin
            .write_all(user_line(WARM_UP).as_bytes())
            .await
            .map_err(|e| format!("Couldn't reach the first mate: {e}"))?;
        let _ = stdin.flush().await;
        let stdout = child.stdout.take().expect("piped stdout");
        let mut stderr = child.stderr.take().expect("piped stderr");
        let generation = {
            let mut g = self.generation.lock().unwrap();
            *g += 1;
            *g
        };
        log::info!("first mate: started (resume: {})", if resume.is_empty() { "no" } else { resume.as_str() });

        let me = self.clone();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            let mut parser = StreamParser::default();
            let mut got_output = false;
            while let Ok(Some(line)) = lines.next_line().await {
                got_output |= line.contains("\"type\":\"assistant\"");
                me.handle_line(&app, &mut parser, &line);
            }
            let mut err = String::new();
            let _ = stderr.read_to_string(&mut err).await;
            let status = child.wait().await.ok();
            log::warn!("first mate: exited ({status:?}): {}", err.trim());
            // A session that can't be resumed fails before saying anything: start fresh next time.
            if !got_output && !resume.is_empty() {
                store.update(|s| s.crew_session = None);
            }
            {
                let mut live = me.live.lock().await;
                if live.as_ref().is_some_and(|l| l.generation == generation) {
                    *live = None;
                }
            }
            let failed: Vec<u64> = {
                let mut t = me.turns.lock().unwrap();
                t.current = None;
                t.open = false;
                t.own.clear();
                t.pending.drain(..).flatten().collect()
            };
            let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
            for turn in failed {
                let message = format!("The first mate stopped. {last}");
                let _ = app.emit("chat-error", ChatError { turn, message, kind: "crew".into() });
            }
        });
        Ok(Live { stdin, generation })
    }

    fn handle_line<R: Runtime>(&self, app: &AppHandle<R>, parser: &mut StreamParser, line: &str) {
        let before = parser.session_id.clone();
        let outs = parser.feed(line);
        if parser.session_id.is_some() && parser.session_id != before {
            let id = parser.session_id.clone();
            let store = app.state::<Arc<ModelStore>>();
            if store.settings().crew_session != id {
                store.update(|s| s.crew_session = id);
            }
        }
        let mut t = self.turns.lock().unwrap();
        for out in outs {
            if !t.open {
                t.open = true;
                t.current = t.pending.front().copied().flatten();
                t.answering = !t.pending.is_empty();
            }
            match (out, t.current) {
                (Out::Text(text), Some(turn)) => {
                    let _ = app.emit("chat-delta", Delta { turn, text });
                }
                (Out::Text(text), None) => t.own.push_str(&text),
                (Out::Activity(text), Some(turn)) => {
                    let _ = app.emit("chat-activity", Activity { turn, text });
                }
                (Out::Activity(_), None) => {}
            }
        }
        let end = match edge(line) {
            Edge::None => return,
            Edge::EndTurn => None,
            Edge::Result(error) => Some(error),
        };
        if !t.open {
            // Nothing was streamed. A reply that ended empty answers the oldest message;
            // so does a failed result. A plain result just follows a reply already ended.
            match end {
                None => {
                    if let Some(Some(turn)) = t.pending.pop_front() {
                        let _ = app.emit("chat-done", Done { turn, model: None, stop_reason: None });
                    }
                }
                Some(Some(message)) => {
                    if let Some(Some(turn)) = t.pending.pop_front() {
                        let _ = app.emit("chat-error", ChatError { turn, message, kind: "crew".into() });
                    }
                }
                Some(None) => {}
            }
            return;
        }
        t.open = false;
        parser.next_turn();
        if t.answering {
            t.pending.pop_front();
        }
        match t.current.take() {
            Some(turn) => {
                match end {
                    Some(Some(message)) => {
                        let _ = app.emit("chat-error", ChatError { turn, message, kind: "crew".into() });
                    }
                    _ => {
                        let _ = app.emit("chat-done", Done { turn, model: None, stop_reason: None });
                    }
                }
            }
            None => {
                let said = std::mem::take(&mut t.own);
                if !is_quiet(&said) {
                    let _ = app.emit("crew-reply", said.trim().to_string());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_line_is_one_json_line() {
        let line = user_line("fix \"login\"\nplease");
        assert!(line.ends_with('\n') && line.matches('\n').count() == 1);
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["message"]["content"], "fix \"login\"\nplease");
        assert_eq!(v["type"], "user");
    }

    #[test]
    fn quiet_replies_are_not_read_out() {
        assert!(is_quiet("[neutral]"));
        assert!(is_quiet("  [neutral]  "));
        assert!(is_quiet("[happy] Proxy, shipshape."));
        assert!(!is_quiet("[proud] fix login has a pull request ready."));
    }

    #[test]
    fn finds_turn_edges() {
        let end = r#"{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"}}}"#;
        let tool = r#"{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"tool_use"}}}"#;
        assert_eq!(edge(end), Edge::EndTurn);
        assert_eq!(edge(tool), Edge::None);
        assert_eq!(edge(r#"{"type":"result","subtype":"success","is_error":false}"#), Edge::Result(None));
        assert_eq!(
            edge(r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"boom"}"#),
            Edge::Result(Some("boom".into()))
        );
    }

    #[test]
    fn prompt_adds_the_character() {
        let p = mate_prompt("Nicole", Some("Boss of the Cunning Hares."));
        assert!(p.contains("first mate") && p.contains("[neutral]") && p.contains("Cunning Hares"));
        assert!(!mate_prompt("Aqua", None).contains("character, for how"));
    }
}

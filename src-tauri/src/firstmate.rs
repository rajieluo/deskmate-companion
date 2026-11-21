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

/// True for Claude Code's echo of a message it just read from stdin
/// (`--replay-user-messages`), as opposed to a tool result.
pub fn is_replayed_message(line: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { return false };
    v["type"] == "user"
        && v["isReplay"] == true
        && !v["message"]["content"]
            .as_array()
            .is_some_and(|blocks| blocks.iter().any(|b| b["type"] == "tool_result"))
}

/// Who the text being streamed belongs to.
#[derive(Default)]
struct Turns {
    /// Messages Claude Code hasn't read yet, oldest first: the captain's turn, or None
    /// for the app's own start-up message.
    pending: VecDeque<Option<u64>>,
    /// Messages Claude Code has read for the reply in progress. A message sent while she
    /// is mid-reply is usually read into that same reply, so one reply can answer several.
    covered: Vec<Option<u64>>,
    /// The reply being streamed is for this message (None: a turn she started herself).
    current: Option<u64>,
    /// A reply is being streamed.
    open: bool,
    /// The reply answers the front of `pending`: the fallback when Claude Code doesn't
    /// echo the messages it reads.
    answering: bool,
    /// Text of a turn she started herself, read out when it ends.
    own: String,
}

impl Turns {
    /// The app's start-up message hasn't been answered yet.
    fn starting(&self) -> bool {
        self.pending.front() == Some(&None) || self.covered.first() == Some(&None)
    }

    /// Claude Code read the oldest message: the reply now answers it (the newest of the
    /// captain's messages read so far is the one the chat is showing).
    fn read_one(&mut self) {
        if let Some(message) = self.pending.pop_front() {
            self.covered.push(message);
            if let Some(turn) = message {
                self.current = Some(turn);
            }
        }
    }

    /// The reply ended: returns the captain's message to report it for (None: a turn she
    /// started herself, or only the start-up message), and clears what it answered.
    fn finish(&mut self) -> Option<u64> {
        if !self.covered.is_empty() {
            self.covered.clear();
        } else if self.answering {
            self.pending.pop_front();
        }
        self.answering = false;
        self.current.take()
    }
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
    /// For the Crew window: "stopped", "starting", "busy" (a turn in flight) or "idle".
    pub fn status(&self) -> &'static str {
        let running = match self.live.try_lock() {
            // Held while the session starts (or for a moment while a message goes in).
            Err(_) => return if self.turns.lock().unwrap().starting() { "starting" } else { "busy" },
            Ok(live) => live.is_some(),
        };
        let t = self.turns.lock().unwrap();
        match running {
            false => "stopped",
            true if t.starting() => "starting",
            true if t.open || !t.pending.is_empty() || !t.covered.is_empty() => "busy",
            true => "idle",
        }
    }

    /// Sends the captain's message `text` as turn `turn`. Starts the session if needed.
    pub async fn send<R: Runtime>(self: &Arc<Self>, app: &AppHandle<R>, turn: u64, text: &str) -> Result<(), String> {
        let mut live = self.live.lock().await;
        if live.is_none() {
            let _ = app.emit("chat-activity", Activity { turn, text: "Getting the crew's records out…".into() });
            *live = Some(self.spawn(app).await?);
        }
        {
            let mut t = self.turns.lock().unwrap();
            if t.starting() {
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
                t.answering = false;
                t.own.clear();
                let covered: Vec<Option<u64>> = t.covered.drain(..).collect();
                covered.into_iter().chain(t.pending.drain(..)).flatten().collect()
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
        let (emits, closed) = self.turns.lock().unwrap().on_line(line, outs);
        if closed {
            parser.next_turn();
        }
        for e in emits {
            let _ = match e {
                Emit::Delta(turn, text) => app.emit("chat-delta", Delta { turn, text }),
                Emit::Activity(turn, text) => app.emit("chat-activity", Activity { turn, text }),
                Emit::Done(turn) => app.emit("chat-done", Done { turn, model: None, stop_reason: None }),
                Emit::Error(turn, message) => app.emit("chat-error", ChatError { turn, message, kind: "crew".into() }),
                Emit::Own(text) => app.emit("crew-reply", text),
            };
        }
    }
}

/// What one stdout line makes the app tell the chat.
#[derive(Debug, PartialEq)]
enum Emit {
    Delta(u64, String),
    Activity(u64, String),
    Done(u64),
    Error(u64, String),
    /// What she said in a turn she started herself.
    Own(String),
}

impl Turns {
    /// Turn bookkeeping for one stdout line and what the parser got from it. Returns
    /// what to tell the chat, and whether a reply ended.
    fn on_line(&mut self, line: &str, outs: Vec<Out>) -> (Vec<Emit>, bool) {
        let mut emits = vec![];
        if is_replayed_message(line) {
            self.read_one();
            return (emits, false);
        }
        for out in outs {
            if !self.open {
                self.open = true;
                if self.covered.is_empty() {
                    // Claude Code didn't say which message it read: the oldest one.
                    self.current = self.pending.front().copied().flatten();
                    self.answering = !self.pending.is_empty();
                }
            }
            match (out, self.current) {
                (Out::Text(text), Some(turn)) => emits.push(Emit::Delta(turn, text)),
                (Out::Text(text), None) => self.own.push_str(&text),
                (Out::Activity(text), Some(turn)) => emits.push(Emit::Activity(turn, text)),
                (Out::Activity(_), None) => {}
            }
        }
        let end = match edge(line) {
            Edge::None => return (emits, false),
            Edge::EndTurn => None,
            Edge::Result(error) => Some(error),
        };
        if !self.open {
            // Nothing was streamed. A reply that ended empty answers what was read for it
            // (or the oldest message); so does a failed result. A plain result just follows
            // a reply already ended.
            if let Some(None) = end {
                return (emits, false);
            }
            let turn = if self.covered.is_empty() { self.pending.pop_front().flatten() } else { self.finish() };
            self.own.clear();
            if let Some(turn) = turn {
                emits.push(match end {
                    Some(Some(message)) => Emit::Error(turn, message),
                    _ => Emit::Done(turn),
                });
            }
            return (emits, false);
        }
        self.open = false;
        let said = std::mem::take(&mut self.own);
        match self.finish() {
            Some(turn) => emits.push(match end {
                Some(Some(message)) => Emit::Error(turn, message),
                _ => Emit::Done(turn),
            }),
            None if !is_quiet(&said) => emits.push(Emit::Own(said.trim().to_string())),
            None => {}
        }
        (emits, true)
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

    const REPLAY: &str = r#"{"type":"user","message":{"role":"user","content":"hi"},"isReplay":true}"#;
    const TOOL_RESULT: &str =
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"x"}]},"isReplay":true}"#;
    const END: &str = r#"{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"}}}"#;
    const RESULT: &str = r#"{"type":"result","subtype":"success","is_error":false}"#;
    const OTHER: &str = r#"{"type":"stream_event","event":{"type":"content_block_delta"}}"#;

    fn text(t: &mut Turns, s: &str) -> Vec<Emit> {
        t.on_line(OTHER, vec![Out::Text(s.into())]).0
    }

    #[test]
    fn spots_replayed_messages() {
        assert!(is_replayed_message(REPLAY));
        assert!(!is_replayed_message(TOOL_RESULT));
        assert!(!is_replayed_message(RESULT));
    }

    #[test]
    fn a_message_sent_mid_reply_gets_the_rest_of_that_reply() {
        let mut t = Turns::default();
        t.pending.push_back(Some(1));
        t.on_line(REPLAY, vec![]);
        assert_eq!(text(&mut t, "[happy] Chatterbox"), vec![Emit::Delta(1, "[happy] Chatterbox".into())]);
        t.pending.push_back(Some(2)); // sent while she's still answering 1
        t.on_line(REPLAY, vec![]); // ...and Claude Code reads it into the same reply
        assert_eq!(text(&mut t, " and Turbo"), vec![Emit::Delta(2, " and Turbo".into())]);
        assert_eq!(t.on_line(END, vec![]), (vec![Emit::Done(2)], true));
        assert_eq!(t.on_line(RESULT, vec![]).0, vec![]);
        assert!(t.pending.is_empty() && t.covered.is_empty());
        // The next message is answered as itself, not one behind.
        t.pending.push_back(Some(3));
        t.on_line(REPLAY, vec![]);
        assert_eq!(text(&mut t, "Qwen"), vec![Emit::Delta(3, "Qwen".into())]);
        assert_eq!(t.on_line(END, vec![]).0, vec![Emit::Done(3)]);
    }

    #[test]
    fn a_message_read_after_the_reply_gets_its_own() {
        let mut t = Turns::default();
        t.pending.push_back(Some(1));
        t.on_line(REPLAY, vec![]);
        text(&mut t, "one");
        t.pending.push_back(Some(2)); // not read until the next turn
        assert_eq!(t.on_line(END, vec![]).0, vec![Emit::Done(1)]);
        t.on_line(RESULT, vec![]);
        t.on_line(REPLAY, vec![]);
        assert_eq!(text(&mut t, "two"), vec![Emit::Delta(2, "two".into())]);
        assert_eq!(t.on_line(END, vec![]).0, vec![Emit::Done(2)]);
    }

    #[test]
    fn without_replays_the_oldest_message_is_answered() {
        let mut t = Turns::default();
        t.pending.push_back(Some(1));
        assert_eq!(text(&mut t, "hi"), vec![Emit::Delta(1, "hi".into())]);
        assert_eq!(t.on_line(END, vec![]).0, vec![Emit::Done(1)]);
        assert!(t.pending.is_empty());
    }

    #[test]
    fn start_up_and_her_own_turns_are_not_chat_replies() {
        let mut t = Turns::default();
        t.pending.push_back(None);
        assert!(t.starting());
        t.on_line(REPLAY, vec![]);
        assert!(t.starting());
        assert_eq!(text(&mut t, "[neutral]"), vec![]);
        assert_eq!(t.on_line(END, vec![]).0, vec![]);
        assert!(!t.starting());
        text(&mut t, "[happy] The PR merged.");
        assert_eq!(t.on_line(END, vec![]).0, vec![Emit::Own("[happy] The PR merged.".into())]);
    }

    #[test]
    fn prompt_adds_the_character() {
        let p = mate_prompt("Nicole", Some("Boss of the Cunning Hares."));
        assert!(p.contains("first mate") && p.contains("[neutral]") && p.contains("Cunning Hares"));
        assert!(!mate_prompt("Aqua", None).contains("character, for how"));
    }
}

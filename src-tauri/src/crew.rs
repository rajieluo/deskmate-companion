//! "Crew mode": the avatar is the face of a firstmate home
//! (https://github.com/kunchenguid/firstmate) running in WSL2. Firstmate does the
//! orchestration: one Claude Code "first mate" that starts and supervises crewmates,
//! each in its own git worktree. The app only talks to it through the two surfaces
//! firstmate documents for outside tools:
//!
//! * `bin/fm-inbox.sh` - `note` queues a message for the first mate and wakes it,
//!   `receipts` returns the replies it publishes with `fm-inbox.sh reply`.
//! * `state/fleet-ledger.jsonl` - an append-only log of task events (dispatched,
//!   status lines, PR ready, merged, cleaned up), switched on by `config/fleet-ledger`.
//!
//! Every call goes through a small bash helper (`crew.sh`, written to the app data
//! folder) run with `wsl.exe --exec`, so no shell script ever has to survive Windows
//! command-line quoting. Message text goes in on stdin.

use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::io::AsyncWriteExt;

use crate::model::ModelStore;

pub const DEFAULT_HOME: &str = "~/firstmate";
const POLL_EVERY: Duration = Duration::from_secs(4);
/// Most ledger bytes read per poll; a long backlog is read over several polls.
const LEDGER_CHUNK: u64 = 4 * 1024 * 1024;
const LEDGER_MARK: &str = "@@LEDGER@@";

/// The helper that runs inside WSL. Usage: crew.sh <firstmate home> <command> [args...]
pub const HELPER: &str = r#"#!/usr/bin/env bash
# Written by Desktop Assistant (Crew mode). Runs inside WSL.
# Usage: crew.sh <firstmate home> <command> [args...]
home=$1
cmd=$2
shift 2
case $home in
  "~") home=$HOME ;;
  "~/"*) home=$HOME/${home#"~/"} ;;
esac
if [ ! -f "$home/AGENTS.md" ] || [ ! -x "$home/bin/fm-inbox.sh" ]; then
  echo "No firstmate home at $home" >&2
  exit 20
fi
cd "$home" || exit 20
export FM_HOME=$PWD
case $cmd in
  check)
    # Turn on the fleet ledger, which is how the app follows the crew's tasks.
    mkdir -p config && touch config/fleet-ledger
    for tool in claude tmux gh git python3; do
      command -v "$tool" >/dev/null 2>&1 || echo "missing $tool"
    done
    echo ok
    ;;
  note)
    exec bin/fm-inbox.sh note --json --request-id "$1" -
    ;;
  poll)
    after=$1
    offset=$2
    max=$3
    if [ -n "$after" ]; then
      out=$(bin/fm-inbox.sh receipts --after "$after" --all-replies 2>/dev/null)
    else
      out=$(bin/fm-inbox.sh receipts --all-replies 2>/dev/null)
    fi
    [ -n "$out" ] || out='{}'
    printf '%s\n' "$out"
    f=state/fleet-ledger.jsonl
    if [ -f "$f" ]; then size=$(wc -c <"$f"); else size=0; fi
    echo "@@LEDGER@@ $size"
    if [ "$size" -gt "$offset" ]; then
      tail -c +"$((offset + 1))" "$f" | head -c "$max"
    fi
    ;;
  status)
    exec bin/fm-inbox.sh status
    ;;
  open)
    if command -v tmux >/dev/null 2>&1; then
      exec tmux new-session -A -s firstmate claude
    fi
    exec claude
    ;;
  *)
    echo "unknown command $cmd" >&2
    exit 2
    ;;
esac
"#;

/// Added to every message so the first mate knows where it came from and how to answer.
pub fn note_body(name: &str, text: &str) -> String {
    format!(
        "{text}\n\n(Sent by the captain through {name}, the desktop avatar. Please answer this note with \
`bin/fm-inbox.sh reply <note id>` so the avatar can read it out: one to three short spoken sentences, \
no Markdown, starting with one emotion tag from {tags}.)",
        tags = crate::chat::EMOTION_TAGS
    )
}

#[derive(Clone, Debug)]
pub struct CrewConfig {
    /// WSL distribution, or None for the default one.
    pub distro: Option<String>,
    /// Firstmate home inside WSL, e.g. ~/firstmate.
    pub home: String,
}

impl CrewConfig {
    pub fn from_settings(store: &ModelStore) -> Self {
        let s = store.settings();
        Self {
            distro: s.crew_distro.filter(|d| !d.trim().is_empty()),
            home: s.crew_home.filter(|h| !h.trim().is_empty()).unwrap_or_else(|| DEFAULT_HOME.into()),
        }
    }
}

/// C:\Users\Alex\AppData\x -> /mnt/c/Users/Alex/AppData/x (WSL's default automount).
pub fn to_wsl_path(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        format!("/mnt/{}{}", (bytes[0] as char).to_ascii_lowercase(), &s[2..])
    } else {
        s
    }
}

fn helper_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("crew.sh");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(HELPER) {
        std::fs::write(&path, HELPER).map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
    }
    Ok(path)
}

fn helper_command(helper: &Path, config: &CrewConfig, args: &[&str]) -> tokio::process::Command {
    let mut cmd;
    if cfg!(windows) {
        cmd = tokio::process::Command::new("wsl.exe");
        if let Some(distro) = &config.distro {
            cmd.arg("-d").arg(distro);
        }
        cmd.args(["--exec", "/bin/bash", "--login"]).arg(to_wsl_path(helper));
    } else {
        // Linux/macOS (development): run firstmate directly.
        cmd = tokio::process::Command::new("bash");
        cmd.arg("--login").arg(helper);
    }
    cmd.arg(&config.home).args(args);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// wsl.exe prints its own errors in UTF-16, which shows up as text with NULs.
fn clean(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace('\0', "").trim_end().to_string()
}

pub async fn run_helper<R: Runtime>(
    app: &AppHandle<R>,
    config: &CrewConfig,
    args: &[&str],
    stdin: Option<&str>,
) -> Result<Output, String> {
    let helper = helper_path(app)?;
    let mut cmd = helper_command(&helper, config, args);
    cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| {
        if cfg!(windows) {
            format!("I couldn't start WSL ({e}). Crew mode needs WSL2 with firstmate installed; see docs/crew-setup.md.")
        } else {
            format!("Couldn't run bash: {e}")
        }
    })?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let _ = pipe.write_all(text.as_bytes()).await;
        drop(pipe);
    }
    let out = tokio::time::timeout(Duration::from_secs(60), child.wait_with_output())
        .await
        .map_err(|_| "firstmate didn't answer within a minute".to_string())?
        .map_err(|e| e.to_string())?;
    Ok(Output { code: out.status.code().unwrap_or(-1), stdout: clean(&out.stdout), stderr: clean(&out.stderr) })
}

/// Turns a failed helper run into something she can say.
pub fn explain_failure(config: &CrewConfig, out: &Output) -> String {
    match out.code {
        20 => format!(
            "I can't find firstmate at {} in WSL. Clone it there, or set crew_home in settings.json.",
            config.home
        ),
        127 => "WSL couldn't run the helper script. Is bash installed in your WSL distribution?".into(),
        _ if out.stderr.contains("no installed distributions") || out.stderr.contains("WSL_E_DEFAULT_DISTRO_NOT_FOUND") => {
            "WSL has no Linux distribution yet. Run `wsl --install -d Ubuntu` first; see docs/crew-setup.md.".into()
        }
        _ => {
            let detail = if out.stderr.is_empty() { &out.stdout } else { &out.stderr };
            format!("firstmate said: {}", detail.lines().last().unwrap_or("(nothing)"))
        }
    }
}

// --- Fleet ledger -> task list.

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Task {
    pub id: String,
    /// ship, scout or secondmate.
    pub kind: Option<String>,
    pub project: Option<String>,
    /// working, needs-decision, blocked, paused, done, failed, resolved, pr-ready, merged.
    pub state: String,
    /// Latest status line.
    pub text: String,
    pub pr: Option<String>,
    pub updated: i64,
}

/// Something worth saying out loud.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Attention {
    pub task: String,
    /// needs-decision, blocked, failed, pr-ready, done, merged.
    pub kind: String,
    /// What she says, starting with an emotion tag.
    pub speech: String,
}

#[derive(Deserialize)]
struct Record {
    event: String,
    task: String,
    #[serde(default)]
    ts: i64,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    pr: Option<String>,
}

fn short(text: &str, max: usize) -> String {
    let t = text.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    let cut: String = t.chars().take(max).collect();
    format!("{}…", cut.trim_end())
}

/// "fix-login" -> "fix login".
fn spoken_name(task: &str) -> String {
    task.replace(['-', '_'], " ")
}

/// Applies one ledger line. Returns what to announce, if anything.
pub fn apply(tasks: &mut BTreeMap<String, Task>, line: &str) -> Option<Attention> {
    let rec: Record = serde_json::from_str(line).ok()?;
    if rec.task.is_empty() {
        return None;
    }
    let name = spoken_name(&rec.task);
    let entry = |tasks: &mut BTreeMap<String, Task>| {
        tasks
            .entry(rec.task.clone())
            .or_insert_with(|| Task {
                id: rec.task.clone(),
                kind: None,
                project: None,
                state: "working".into(),
                text: String::new(),
                pr: None,
                updated: rec.ts,
            })
            .updated = rec.ts;
    };
    match rec.event.as_str() {
        "task.dispatched" => {
            entry(tasks);
            let t = tasks.get_mut(&rec.task)?;
            t.kind = rec.kind.clone();
            t.project = rec.project.clone();
            t.state = "working".into();
            None
        }
        "task.status" => {
            entry(tasks);
            let t = tasks.get_mut(&rec.task)?;
            let text = rec.text.as_deref().unwrap_or("").trim().to_string();
            if !text.is_empty() {
                t.text = text.clone();
            }
            let state = rec.state.as_deref().map(str::to_ascii_lowercase);
            if let Some(state) = &state {
                t.state = state.clone();
            }
            let line = short(&text, 160);
            match state.as_deref() {
                Some("needs-decision") | Some("blocked") => Some(Attention {
                    task: rec.task.clone(),
                    kind: state.clone().unwrap_or_default(),
                    speech: if line.is_empty() {
                        format!("[surprised] {name} needs you.")
                    } else {
                        format!("[surprised] {name} needs you: {line}")
                    },
                }),
                Some("failed") => Some(Attention {
                    task: rec.task.clone(),
                    kind: "failed".into(),
                    speech: format!("[sad] {name} failed. {line}"),
                }),
                Some("done") if t.kind.as_deref() == Some("scout") => Some(Attention {
                    task: rec.task.clone(),
                    kind: "done".into(),
                    speech: format!("[proud] {name} finished its report."),
                }),
                _ => None,
            }
        }
        "task.pr_ready" => {
            entry(tasks);
            let t = tasks.get_mut(&rec.task)?;
            t.state = "pr-ready".into();
            t.pr = rec.pr.clone();
            Some(Attention {
                task: rec.task.clone(),
                kind: "pr-ready".into(),
                speech: format!("[happy] {name} has a pull request ready for you to review."),
            })
        }
        "task.merged" => {
            entry(tasks);
            let t = tasks.get_mut(&rec.task)?;
            t.state = "merged".into();
            if rec.pr.is_some() {
                t.pr = rec.pr.clone();
            }
            Some(Attention { task: rec.task.clone(), kind: "merged".into(), speech: format!("[proud] {name} is merged.") })
        }
        "task.cleaned_up" => {
            tasks.remove(&rec.task);
            None
        }
        _ => None,
    }
}

/// Splits poll output into (receipts JSON, ledger size, ledger bytes).
pub fn split_poll(stdout: &str) -> Option<(Value, u64, &str)> {
    let at = stdout.find(LEDGER_MARK)?;
    let (head, rest) = stdout.split_at(at);
    // The receipts JSON is the last line before the marker that looks like JSON
    // (a login profile could print something first).
    let receipts = head
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with('{'))
        .and_then(|l| serde_json::from_str(l).ok())
        .unwrap_or(Value::Null);
    let rest = &rest[LEDGER_MARK.len()..];
    let (size_line, bytes) = rest.split_once('\n').unwrap_or((rest, ""));
    let size = size_line.trim().parse().ok()?;
    Some((receipts, size, bytes))
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Reply {
    pub id: String,
    pub body: String,
}

/// Replies in a receipts document, oldest first, and the cursor after them.
pub fn replies(receipts: &Value) -> (Vec<Reply>, Option<String>) {
    let list = receipts["replies"].as_array().cloned().unwrap_or_default();
    let replies = list
        .iter()
        .filter_map(|r| {
            Some(Reply { id: r["id"].as_str()?.to_string(), body: r["body"].as_str()?.trim().to_string() })
        })
        .filter(|r| !r.body.is_empty())
        .collect();
    let cursor = receipts["reply_cursor"].as_str().filter(|c| !c.is_empty()).map(String::from);
    (replies, cursor)
}

#[derive(Default)]
struct PollState {
    cursor: Option<String>,
    offset: u64,
    /// False until the existing ledger and replies have been read once, silently.
    primed: bool,
    replies_primed: bool,
    tasks: BTreeMap<String, Task>,
    /// Notes this app sent, so their replies are always read out.
    sent: HashSet<String>,
}

#[derive(Default)]
pub struct Crew {
    state: Mutex<PollState>,
    polling: AtomicBool,
    checked: AtomicBool,
}

impl Crew {
    pub fn tasks(&self) -> Vec<Task> {
        let mut list: Vec<Task> = self.state.lock().unwrap().tasks.values().cloned().collect();
        list.sort_by_key(|t| std::cmp::Reverse(t.updated));
        list
    }

    /// Queues a message for the first mate. Returns what she says right away.
    pub async fn send<R: Runtime>(&self, app: &AppHandle<R>, name: &str, text: &str) -> Result<String, String> {
        let config = CrewConfig::from_settings(&app.state::<std::sync::Arc<ModelStore>>());
        self.ensure_checked(app, &config).await?;
        let request_id = request_id();
        let out = run_helper(app, &config, &["note", &request_id], Some(&note_body(name, text))).await?;
        let json: Value = out.stdout.lines().rev().find_map(|l| serde_json::from_str(l).ok()).unwrap_or(Value::Null);
        if let Some(id) = json["id"].as_str() {
            self.state.lock().unwrap().sent.insert(id.to_string());
        }
        match out.code {
            0 => Ok("[happy] I passed that to the first mate. I'll tell you when it answers.".into()),
            3 => Ok("[thinking] I saved it for the first mate, but couldn't wake it. Is the crew terminal open?".into()),
            _ => Err(explain_failure(&config, &out)),
        }
    }

    /// First use: check the home exists and turn on the fleet ledger.
    async fn ensure_checked<R: Runtime>(&self, app: &AppHandle<R>, config: &CrewConfig) -> Result<(), String> {
        if self.checked.load(Ordering::SeqCst) {
            return Ok(());
        }
        let out = run_helper(app, config, &["check"], None).await?;
        if out.code != 0 {
            return Err(explain_failure(config, &out));
        }
        let missing: Vec<&str> = out.stdout.lines().filter_map(|l| l.strip_prefix("missing ")).collect();
        if !missing.is_empty() {
            log::warn!("crew: missing in WSL: {}", missing.join(", "));
        }
        if missing.contains(&"claude") {
            return Err("Claude Code isn't installed inside WSL yet. See docs/crew-setup.md.".into());
        }
        self.checked.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// One poll: new replies and ledger lines. Emits crew-reply, crew-attention, crew-tasks.
    async fn poll<R: Runtime>(&self, app: &AppHandle<R>) -> Result<(), String> {
        let config = CrewConfig::from_settings(&app.state::<std::sync::Arc<ModelStore>>());
        self.ensure_checked(app, &config).await?;
        let (cursor, offset) = {
            let s = self.state.lock().unwrap();
            (s.cursor.clone().unwrap_or_default(), s.offset)
        };
        let (offset_arg, max_arg) = (offset.to_string(), LEDGER_CHUNK.to_string());
        let out = run_helper(app, &config, &["poll", &cursor, &offset_arg, &max_arg], None).await?;
        if out.code != 0 {
            return Err(explain_failure(&config, &out));
        }
        let Some((receipts, size, bytes)) = split_poll(&out.stdout) else {
            return Err("unexpected poll output".into());
        };

        let mut said = Vec::new();
        let new_replies: Vec<Reply>;
        let mut changed = false;
        {
            let mut s = self.state.lock().unwrap();
            let (list, next) = replies(&receipts);
            if s.replies_primed {
                new_replies = list;
            } else {
                // Replies from before the app started: only ones to our own notes.
                new_replies = list.into_iter().filter(|r| s.sent.contains(&r.id)).collect();
                s.replies_primed = !receipts.is_null();
            }
            if next.is_some() {
                s.cursor = next;
            }

            if size < s.offset {
                // The ledger was replaced; start over.
                s.offset = 0;
                s.tasks.clear();
                changed = true;
            } else {
                // Only whole lines; a half-written last line waits for the next poll.
                let complete = bytes.rfind('\n').map(|i| &bytes[..=i]).unwrap_or("");
                let primed = s.primed;
                for line in complete.lines() {
                    if let Some(a) = apply(&mut s.tasks, line) {
                        if primed {
                            said.push(a);
                        }
                    }
                    changed = true;
                }
                s.offset += complete.len() as u64;
                if s.offset >= size {
                    s.primed = true;
                }
            }
        }
        for reply in new_replies {
            let _ = app.emit("crew-reply", reply);
        }
        for a in said {
            let _ = app.emit("crew-attention", a);
        }
        if changed {
            let _ = app.emit("crew-tasks", self.tasks());
        }
        Ok(())
    }

    /// Polls while Crew mode is on. Safe to call more than once.
    pub fn start_polling<R: Runtime>(self: &std::sync::Arc<Self>, app: AppHandle<R>) {
        if self.polling.swap(true, Ordering::SeqCst) {
            return;
        }
        let crew = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut last_error = String::new();
            loop {
                let on = app.state::<std::sync::Arc<ModelStore>>().settings().brain.as_deref() == Some("crew");
                if !on {
                    break;
                }
                match crew.poll(&app).await {
                    Ok(()) => last_error.clear(),
                    Err(e) => {
                        if e != last_error {
                            log::warn!("crew poll: {e}");
                            let _ = app.emit("crew-error", &e);
                            last_error = e;
                        }
                    }
                }
                tokio::time::sleep(POLL_EVERY).await;
            }
            crew.polling.store(false, Ordering::SeqCst);
        });
    }

    /// Opens the first mate in its own console window (tmux session "firstmate").
    pub fn open_terminal<R: Runtime>(&self, app: &AppHandle<R>) -> Result<(), String> {
        let config = CrewConfig::from_settings(&app.state::<std::sync::Arc<ModelStore>>());
        let helper = helper_path(app)?;
        if !cfg!(windows) {
            return Err("Opening the crew terminal is only supported on Windows.".into());
        }
        let mut cmd = std::process::Command::new("wsl.exe");
        if let Some(distro) = &config.distro {
            cmd.arg("-d").arg(distro);
        }
        cmd.args(["--exec", "/bin/bash", "--login"]).arg(to_wsl_path(&helper)).arg(&config.home).arg("open");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0000_0010); // CREATE_NEW_CONSOLE
        }
        cmd.spawn().map(|_| ()).map_err(|e| format!("Couldn't open WSL: {e}"))
    }
}

/// A unique id for `note --request-id`, so a retried send never queues twice.
fn request_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("avatar-{nanos:x}-{:x}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEDGER: &str = r#"{"v":1,"ts":1790132857,"event":"task.dispatched","task":"fix-login","kind":"ship","project":"webapp","harness":"claude","model":null}
{"v":1,"ts":1790132870,"event":"task.status","task":"fix-login","state":"working","key":null,"text":" bug reproduced"}
{"v":1,"ts":1790133000,"event":"task.status","task":"fix-login","state":"needs-decision","key":"k1","text":" keep the old API or break it?"}
{"v":1,"ts":1790133400,"event":"task.status","task":"fix-login","state":"done","key":null,"text":" PR https://github.com/acme/webapp/pull/7 checks green"}
{"v":1,"ts":1790133410,"event":"task.pr_ready","task":"fix-login","pr":"https://github.com/acme/webapp/pull/7"}
{"v":1,"ts":1790133900,"event":"task.merged","task":"fix-login","via":"pr","pr":"https://github.com/acme/webapp/pull/7"}
{"v":1,"ts":1790133960,"event":"task.cleaned_up","task":"fix-login"}"#;

    #[test]
    fn follows_a_task_through_the_ledger() {
        let mut tasks = BTreeMap::new();
        let lines: Vec<&str> = LEDGER.lines().collect();
        assert_eq!(apply(&mut tasks, lines[0]), None);
        assert_eq!(tasks["fix-login"].state, "working");
        assert_eq!(tasks["fix-login"].project.as_deref(), Some("webapp"));
        assert_eq!(apply(&mut tasks, lines[1]), None);
        assert_eq!(tasks["fix-login"].text, "bug reproduced");

        let a = apply(&mut tasks, lines[2]).unwrap();
        assert_eq!(a.kind, "needs-decision");
        assert_eq!(a.speech, "[surprised] fix login needs you: keep the old API or break it?");
        assert_eq!(tasks["fix-login"].state, "needs-decision");

        assert_eq!(apply(&mut tasks, lines[3]), None, "a ship task's done line waits for the PR");
        let a = apply(&mut tasks, lines[4]).unwrap();
        assert_eq!(a.kind, "pr-ready");
        assert_eq!(tasks["fix-login"].pr.as_deref(), Some("https://github.com/acme/webapp/pull/7"));
        assert_eq!(apply(&mut tasks, lines[5]).unwrap().kind, "merged");
        assert_eq!(apply(&mut tasks, lines[6]), None);
        assert!(tasks.is_empty());
    }

    #[test]
    fn ignores_unknown_and_broken_lines() {
        let mut tasks = BTreeMap::new();
        assert_eq!(apply(&mut tasks, "not json"), None);
        assert_eq!(apply(&mut tasks, r#"{"v":2,"ts":1,"event":"task.someday","task":"x"}"#), None);
        assert!(tasks.is_empty());
        // A status line before its dispatch record still creates the task.
        let a = apply(&mut tasks, r#"{"v":1,"ts":5,"event":"task.status","task":"scan","state":"failed","key":null,"text":" tests broke"}"#);
        assert_eq!(a.unwrap().speech, "[sad] scan failed. tests broke");
        assert_eq!(tasks["scan"].state, "failed");
    }

    #[test]
    fn splits_poll_output() {
        let out = "welcome banner\n{\"schema\":\"fm-inbox-receipts.v1\",\"replies\":[{\"id\":\"n1\",\"at\":\"x\",\"body\":\"[happy] On it.\\n\",\"cursor\":\"000000000001\"}],\"reply_cursor\":\"000000000001\"}\n@@LEDGER@@ 42\n{\"a\":1}\n{\"b\"";
        let (receipts, size, bytes) = split_poll(out).unwrap();
        assert_eq!(size, 42);
        assert_eq!(bytes, "{\"a\":1}\n{\"b\"");
        let (list, cursor) = replies(&receipts);
        assert_eq!(list, vec![Reply { id: "n1".into(), body: "[happy] On it.".into() }]);
        assert_eq!(cursor.as_deref(), Some("000000000001"));
        assert!(split_poll("no marker").is_none());
    }

    #[test]
    fn converts_windows_paths_for_wsl() {
        assert_eq!(
            to_wsl_path(Path::new(r"C:\Users\Alex\AppData\Roaming\x\crew.sh")),
            "/mnt/c/Users/Alex/AppData/Roaming/x/crew.sh"
        );
        assert_eq!(to_wsl_path(Path::new("/home/me/crew.sh")), "/home/me/crew.sh");
    }

    #[test]
    fn note_asks_for_a_spoken_reply() {
        let body = note_body("Aqua", "fix the flaky test");
        assert!(body.starts_with("fix the flaky test\n\n"));
        assert!(body.contains("fm-inbox.sh reply"));
        assert!(body.contains("[happy]"));
    }
}

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
    collections::{BTreeMap, HashSet, VecDeque},
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
const SAY_MARK: &str = "@@SAY@@ ";

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
# Native Windows (firstmate-for-windows under Git Bash): load its overlay, and let
# python3 mean the python.exe that the Windows installer provides.
if [ -f platform/windows/env.sh ]; then
  . platform/windows/env.sh >/dev/null 2>&1
fi
if ! python3 -c '' >/dev/null 2>&1; then
  for py in python py; do
    if command -v "$py" >/dev/null 2>&1 && "$py" -c 'import sys; sys.exit(sys.version_info[0] != 3)' >/dev/null 2>&1; then
      eval "python3() { $py \"\$@\"; }"
      export -f python3
      break
    fi
  done
fi
case $cmd in
  check)
    # Turn on the fleet ledger, which is how the app follows the crew's tasks.
    mkdir -p config && touch config/fleet-ledger
    for tool in claude gh git; do
      command -v "$tool" >/dev/null 2>&1 || echo "missing $tool"
    done
    python3 -c '' >/dev/null 2>&1 || echo "missing python3"
    command -v tmux >/dev/null 2>&1 || command -v herdr >/dev/null 2>&1 || echo "missing tmux-or-herdr"
    echo ok
    ;;
  note)
    bin/fm-inbox.sh note --json --request-id "$1" -
    code=$?
    # An idle firstmate only reads its inbox when it gets a turn (its watcher runs only
    # while there's crew work to supervise), so ring the first mate's Herdr pane: the
    # Claude pane whose folder is this home. Claude Code queues it if it's mid-turn.
    if [ "$code" -eq 0 ] || [ "$code" -eq 3 ]; then
      if command -v herdr >/dev/null 2>&1 && command -v jq >/dev/null 2>&1; then
        want=$(basename "$PWD")
        pane=$(herdr pane list 2>/dev/null | jq -r --arg want "$want" '
          [.result.panes[]?
           | select(((.agent // "") | ascii_downcase) == "claude")
           | select(((.cwd // "") | gsub("\\\\"; "/") | rtrimstr("/") | split("/") | last | ascii_downcase) == ($want | ascii_downcase))]
          | .[0].pane_id // empty' 2>/dev/null)
        if [ -n "$pane" ]; then
          herdr pane run "$pane" "You have a new avatar note: check your inbox." >/dev/null 2>&1 || true
        fi
      fi
    fi
    exit "$code"
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
    # Announcements the first mate wants said: one file per line, oldest first. Only
    # *.txt, so a file still being written under a temporary name waits. Each one goes
    # out as hex (so any text survives) and is then removed; empty or unreadable files
    # are removed without a line.
    LC_COLLATE=C
    for f in state/avatar-say/*.txt; do
      [ -f "$f" ] || continue
      hex=$(head -c 8192 "$f" 2>/dev/null | od -An -v -tx1 | tr -d ' \n')
      if [ -n "$hex" ]; then
        echo "@@SAY@@ $(basename "$f") $hex"
      fi
      rm -f "$f"
    done
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
    if command -v herdr >/dev/null 2>&1; then
      # Windows: Herdr is the crew's terminal. When no Herdr was running yet, type
      # `claude` into its first pane once it's up, which starts the first mate.
      # (With Herdr already running, its panes may be crewmates, so leave them alone.)
      if [ "$1" != watch ] && ! herdr pane list >/dev/null 2>&1 && ! bin/fm-inbox.sh ready 2>/dev/null | grep -q '"can_receive":true'; then
        winhome=$(cygpath -w "$PWD" 2>/dev/null || pwd)
        (
          for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
            sleep 1
            pane=$(herdr pane list 2>/dev/null | jq -r '.result.panes[0].pane_id // empty' 2>/dev/null)
            if [ -n "$pane" ]; then
              sleep 2
              herdr pane run "$pane" "cd '$winhome'; claude"
              break
            fi
          done
        ) >/dev/null 2>&1 &
      fi
      exec herdr
    fi
    if command -v tmux >/dev/null 2>&1; then
      [ "$1" = watch ] && exec tmux new-session -A -s firstmate
      exec tmux new-session -A -s firstmate claude
    fi
    [ "$1" = watch ] && exec bash --login
    exec claude
    ;;
  mate)
    # The app is the first mate: one long-running headless Claude Code session that
    # reads the captain's messages as stream-json on stdin.
    # Usage: mate <prompt file> <permission mode> [<session to resume>]
    # Give session start room to finish inside Claude Code's 180 s hook limit, so the
    # first mate doesn't have to run it again (Windows is slow at it).
    export FM_SESSION_START_TIMEOUT=${FM_SESSION_START_TIMEOUT:-170}
    set -- -p --input-format stream-json --output-format stream-json --verbose \
      --include-partial-messages --permission-mode "$2" --append-system-prompt-file "$1" \
      ${3:+--resume "$3"}
    exec claude "$@"
    ;;
  *)
    echo "unknown command $cmd" >&2
    exit 2
    ;;
esac
"#;

/// Added to every message so the first mate knows where it came from and how to answer.
/// How the first mate's answers to an avatar note should sound.
pub enum Character<'a> {
    /// No persona: plain short speech.
    None,
    /// The persona text, sent the first time and whenever it changes.
    Full(&'a str),
    /// The same persona as an earlier note.
    Same,
}

pub fn note_body(name: &str, text: &str, character: Character) -> String {
    let mut body = format!(
        "{text}\n\n(Sent by the captain through {name}, the desktop avatar. Please answer this note with \
`bin/fm-inbox.sh reply <note id>` so the avatar can read it out: one to three short spoken sentences, \
no Markdown, starting with one emotion tag from {tags}.",
        tags = crate::chat::EMOTION_TAGS
    );
    match character {
        Character::None => {}
        Character::Full(persona) => body.push_str(&format!(
            " Write those replies in character as {name}, described below. Let the character color only \
how the replies sound; your work, decisions and facts stay exactly as careful as usual, and your \
terminal output is unchanged.\n\n{name}'s character:\n{persona}\n"
        )),
        Character::Same => body.push_str(&format!(" Keep replying in character as {name}, as in the earlier avatar notes.")),
    }
    body.push(')');
    body
}

/// How the app reaches firstmate.
#[derive(Clone, Debug, PartialEq)]
pub enum Shell {
    /// Windows: firstmate-for-windows under Git Bash (this is bash.exe).
    GitBash(PathBuf),
    /// Windows: firstmate inside WSL2.
    Wsl,
    /// Linux/macOS (development): plain bash.
    Unix,
}

#[derive(Clone, Debug)]
pub struct CrewConfig {
    pub shell: Shell,
    /// WSL distribution, or None for the default one.
    pub distro: Option<String>,
    /// Firstmate home inside WSL, e.g. ~/firstmate.
    pub home: String,
}

impl CrewConfig {
    pub fn from_settings(store: &ModelStore) -> Self {
        let s = store.settings();
        let shell = if !cfg!(windows) {
            Shell::Unix
        } else if s.crew_shell.as_deref() == Some("wsl") {
            Shell::Wsl
        } else {
            match find_git_bash() {
                Some(bash) => Shell::GitBash(bash),
                None if s.crew_shell.as_deref() == Some("git-bash") => Shell::GitBash("bash.exe".into()),
                None => Shell::Wsl,
            }
        };
        let distro = if shell == Shell::Wsl {
            s.crew_distro.filter(|d| !d.trim().is_empty()).or_else(auto_distro)
        } else {
            None
        };
        Self {
            shell,
            distro,
            home: s.crew_home.filter(|h| !h.trim().is_empty()).unwrap_or_else(|| DEFAULT_HOME.into()),
        }
    }
}

/// Git for Windows' bash.exe. Never System32\bash.exe, which is WSL's.
pub fn find_git_bash() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(dir) = std::env::var_os(var) {
            candidates.push(PathBuf::from(dir).join("Git").join("bin").join("bash.exe"));
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local).join("Programs").join("Git").join("bin").join("bash.exe"));
    }
    // git.exe on PATH lives in <Git>\cmd or <Git>\bin.
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            if dir.join("git.exe").is_file() {
                if let Some(root) = dir.parent() {
                    candidates.push(root.join("bin").join("bash.exe"));
                }
            }
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// C:\Users\x\crew.sh -> C:/Users/x/crew.sh, which Git Bash accepts.
pub(crate) fn to_git_bash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Distributions that can't host firstmate (Docker Desktop's internal ones).
fn usable_distro(name: &str) -> bool {
    !name.is_empty() && !name.starts_with("docker-desktop")
}

/// Picks the distro when `crew_distro` isn't set. WSL's default is often Docker
/// Desktop's internal distro, so prefer an Ubuntu, then any other usable one.
pub fn pick_distro(names: &[String]) -> Option<String> {
    let usable: Vec<&String> = names.iter().filter(|n| usable_distro(n)).collect();
    usable
        .iter()
        .find(|n| n.to_ascii_lowercase().starts_with("ubuntu"))
        .or(usable.first())
        .map(|n| n.to_string())
}

/// `wsl.exe -l -q` prints UTF-16LE names, one per line.
pub fn parse_distro_list(bytes: &[u8]) -> Vec<String> {
    let text = if bytes.len() >= 2 && bytes[1] == 0 {
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    text.trim_start_matches('\u{feff}').lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
}

fn auto_distro() -> Option<String> {
    if !cfg!(windows) {
        return None;
    }
    static PICKED: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    PICKED
        .get_or_init(|| {
            let mut cmd = std::process::Command::new("wsl.exe");
            cmd.args(["-l", "-q"]);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_0000);
            }
            let names = cmd.output().map(|o| parse_distro_list(&o.stdout)).unwrap_or_default();
            let picked = pick_distro(&names);
            log::info!("crew: WSL distros {names:?}, using {picked:?}");
            picked
        })
        .clone()
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

pub(crate) fn helper_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("crew.sh");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(HELPER) {
        std::fs::write(&path, HELPER).map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
    }
    Ok(path)
}

/// The program and arguments that run the helper: `<program> <args...> <home> <command...>`.
fn helper_argv(helper: &Path, config: &CrewConfig) -> (PathBuf, Vec<String>) {
    match &config.shell {
        Shell::GitBash(bash) => (bash.clone(), vec!["--login".into(), to_git_bash_path(helper)]),
        Shell::Wsl => {
            let mut args = Vec::new();
            if let Some(distro) = &config.distro {
                args.extend(["-d".to_string(), distro.clone()]);
            }
            args.extend(["--exec", "/bin/bash", "--login"].map(String::from));
            args.push(to_wsl_path(helper));
            ("wsl.exe".into(), args)
        }
        Shell::Unix => ("bash".into(), vec!["--login".into(), helper.to_string_lossy().into_owned()]),
    }
}

pub(crate) fn helper_command(helper: &Path, config: &CrewConfig, args: &[&str]) -> tokio::process::Command {
    let (program, pre) = helper_argv(helper, config);
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(pre).arg(&config.home).args(args);
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
    let mut child = cmd.spawn().map_err(|e| match config.shell {
        Shell::Wsl => format!("I couldn't start WSL ({e}). See docs/crew-setup.md."),
        Shell::GitBash(_) => format!("I couldn't start Git Bash ({e}). Is Git for Windows installed?"),
        Shell::Unix => format!("Couldn't run bash: {e}"),
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
            "I can't find firstmate at {}{}. Clone it there, or set crew_home in settings.json.",
            config.home,
            if config.shell == Shell::Wsl { " in WSL" } else { "" }
        ),
        127 => "Bash couldn't run the helper script.".into(),
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
    /// Agent tool and model the worker was launched with (from its dispatch record).
    pub harness: Option<String>,
    pub model: Option<String>,
    /// When it was dispatched (or first seen).
    pub started: i64,
    /// When it was cleaned up, for finished tasks.
    pub ended: Option<i64>,
    /// What happened to it, oldest first, for the Crew window (the latest MAX_EVENTS).
    pub events: Vec<TaskEvent>,
}

/// One line in a task's timeline.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct TaskEvent {
    pub ts: i64,
    /// dispatched, status, pr-ready, merged or cleaned-up.
    pub kind: String,
    /// The status line's state word, for status events.
    pub state: Option<String>,
    pub text: String,
}

/// Timeline entries kept per task.
pub const MAX_EVENTS: usize = 60;
/// Cleaned-up tasks kept for the Crew window's Finished list.
pub const MAX_FINISHED: usize = 30;

impl Task {
    fn log(&mut self, ts: i64, kind: &str, state: Option<String>, text: String) {
        let event = TaskEvent { ts, kind: kind.into(), state, text };
        // The ledger can repeat records after an interrupted capture.
        if self.events.iter().rev().take(8).any(|e| *e == event) {
            return;
        }
        self.events.push(event);
        if self.events.len() > MAX_EVENTS {
            self.events.drain(..self.events.len() - MAX_EVENTS);
        }
    }
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
    #[serde(default)]
    harness: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    via: Option<String>,
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
    apply_tracked(tasks, &mut VecDeque::new(), line)
}

/// Like `apply`, and moves cleaned-up tasks to the front of `finished`.
pub fn apply_tracked(tasks: &mut BTreeMap<String, Task>, finished: &mut VecDeque<Task>, line: &str) -> Option<Attention> {
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
                harness: None,
                model: None,
                started: rec.ts,
                ended: None,
                events: Vec::new(),
            })
            .updated = rec.ts;
    };
    match rec.event.as_str() {
        "task.dispatched" => {
            entry(tasks);
            let t = tasks.get_mut(&rec.task)?;
            t.kind = rec.kind.clone();
            t.project = rec.project.clone();
            t.harness = rec.harness.clone();
            t.model = rec.model.clone();
            t.state = "working".into();
            t.started = rec.ts;
            let what = [rec.kind.as_deref(), rec.project.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" in ");
            t.log(rec.ts, "dispatched", None, if what.is_empty() { "Started".into() } else { format!("Started ({what})") });
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
            t.log(rec.ts, "status", state.clone(), text.clone());
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
            t.log(rec.ts, "pr-ready", None, rec.pr.clone().unwrap_or_default());
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
            let how = if rec.via.as_deref() == Some("local") { "Landed locally".to_string() } else { rec.pr.clone().unwrap_or_default() };
            t.log(rec.ts, "merged", None, how);
            Some(Attention { task: rec.task.clone(), kind: "merged".into(), speech: format!("[proud] {name} is merged.") })
        }
        "task.cleaned_up" => {
            if let Some(mut t) = tasks.remove(&rec.task) {
                t.ended = Some(rec.ts);
                t.updated = rec.ts;
                t.log(rec.ts, "cleaned-up", None, "Worker and its copy removed".into());
                finished.retain(|f| f.id != t.id);
                finished.push_front(t);
                finished.truncate(MAX_FINISHED);
            }
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

fn from_hex(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok()).collect()
}

/// Announcements in poll output (`@@SAY@@ <file> <hex>` lines before the ledger marker),
/// oldest first. Spoken like replies; the id is `say:<file>`.
pub fn announcements(stdout: &str) -> Vec<Reply> {
    let head = stdout.find(LEDGER_MARK).map_or(stdout, |at| &stdout[..at]);
    head.lines()
        .filter_map(|l| {
            let mut parts = l.strip_prefix(SAY_MARK)?.split_whitespace();
            let (name, hex) = (parts.next()?, parts.next()?);
            let body = String::from_utf8_lossy(&from_hex(hex)?).trim().to_string();
            (!body.is_empty()).then(|| Reply { id: format!("say:{name}"), body })
        })
        .collect()
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
    /// Cleaned-up tasks, newest first.
    finished: VecDeque<Task>,
    /// Unix time of the last successful poll, and the last poll error.
    polled: Option<i64>,
    error: Option<String>,
    /// Notes this app sent, so their replies are always read out.
    sent: HashSet<String>,
    /// The name and persona the last note carried in full.
    persona_sent: Option<String>,
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

    /// Tasks that were cleaned up while the app watched (or in the ledger), newest first.
    pub fn finished(&self) -> Vec<Task> {
        self.state.lock().unwrap().finished.iter().cloned().collect()
    }

    /// When the crew was last read, and what went wrong last time, if it did.
    pub fn poll_status(&self) -> (Option<i64>, Option<String>) {
        let s = self.state.lock().unwrap();
        (s.polled, s.error.clone())
    }

    /// Queues a message for the first mate. Returns what she says right away.
    /// Queues `text` for the first mate. `persona` is the current model's character, sent
    /// in full the first time (per app run) and whenever it changes, and referred to otherwise.
    pub async fn send<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        name: &str,
        text: &str,
        persona: Option<&str>,
    ) -> Result<String, String> {
        let config = CrewConfig::from_settings(&app.state::<std::sync::Arc<ModelStore>>());
        self.ensure_checked(app, &config).await?;
        let request_id = request_id();
        let key = persona.map(|p| format!("{name}\n{p}"));
        let character = match persona {
            None => Character::None,
            Some(_) if self.state.lock().unwrap().persona_sent == key => Character::Same,
            Some(p) => Character::Full(p),
        };
        let out = run_helper(app, &config, &["note", &request_id], Some(&note_body(name, text, character))).await?;
        let json: Value = out.stdout.lines().rev().find_map(|l| serde_json::from_str(l).ok()).unwrap_or(Value::Null);
        {
            let mut state = self.state.lock().unwrap();
            if let Some(id) = json["id"].as_str() {
                state.sent.insert(id.to_string());
            }
            if matches!(out.code, 0 | 3) {
                state.persona_sent = key;
            }
        }
        match out.code {
            0 => Ok("[happy] I passed that to the first mate. I'll tell you when it answers.".into()),
            3 => Ok("[thinking] I saved it for the first mate, but couldn't wake it. Is the crew terminal open?".into()),
            _ => Err(explain_failure(&config, &out)),
        }
    }

    /// First use: check the home exists and turn on the fleet ledger.
    pub(crate) async fn ensure_checked<R: Runtime>(&self, app: &AppHandle<R>, config: &CrewConfig) -> Result<(), String> {
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
            return Err(if config.shell == Shell::Wsl {
                "Claude Code isn't installed inside WSL yet. See docs/crew-setup.md.".into()
            } else {
                "Git Bash can't find Claude Code. See docs/crew-setup.md.".into()
            });
        }
        if missing.contains(&"python3") {
            return Err("firstmate's inbox needs Python 3, and I can't find it. See docs/crew-setup.md.".into());
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
                s.finished.clear();
                changed = true;
            } else {
                // Only whole lines; a half-written last line waits for the next poll.
                let complete = bytes.rfind('\n').map(|i| &bytes[..=i]).unwrap_or("");
                let primed = s.primed;
                let PollState { tasks, finished, .. } = &mut *s;
                for line in complete.lines() {
                    if let Some(a) = apply_tracked(tasks, finished, line) {
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
        // Announcements are always said, even ones written while the app was closed: the
        // helper removes each file as it hands it over, so each one comes once.
        for reply in new_replies.into_iter().chain(announcements(&out.stdout)) {
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
                let result = crew.poll(&app).await;
                {
                    let mut s = crew.state.lock().unwrap();
                    match &result {
                        Ok(()) => {
                            s.polled = Some(now_secs());
                            s.error = None;
                        }
                        Err(e) => s.error = Some(e.clone()),
                    }
                }
                match result {
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
        let (program, pre) = helper_argv(&helper, &config);
        let mut cmd = std::process::Command::new(program);
        cmd.args(pre).arg(&config.home).arg("open");
        // When she is the first mate, the terminal is only for watching the crew.
        if crate::firstmate::direct(&app.state::<std::sync::Arc<ModelStore>>()) {
            cmd.arg("watch");
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0000_0010); // CREATE_NEW_CONSOLE
        }
        cmd.spawn().map(|_| ()).map_err(|e| format!("Couldn't open the crew terminal: {e}"))
    }
}

/// A unique id for `note --request-id`, so a retried send never queues twice.
fn now_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

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
    fn keeps_a_timeline_and_finished_tasks() {
        let mut tasks = BTreeMap::new();
        let mut finished = VecDeque::new();
        let lines: Vec<&str> = LEDGER.lines().collect();
        for line in &lines[..5] {
            apply_tracked(&mut tasks, &mut finished, line);
        }
        // A repeated record (interrupted capture) is not logged twice.
        apply_tracked(&mut tasks, &mut finished, lines[3]);
        let t = &tasks["fix-login"];
        assert_eq!(t.harness.as_deref(), Some("claude"));
        assert_eq!(t.started, 1790132857);
        let kinds: Vec<&str> = t.events.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds, ["dispatched", "status", "status", "status", "pr-ready"]);
        assert_eq!(t.events[0].text, "Started (ship in webapp)");
        assert_eq!(t.events[2].state.as_deref(), Some("needs-decision"));
        for line in &lines[5..] {
            apply_tracked(&mut tasks, &mut finished, line);
        }
        assert!(tasks.is_empty());
        assert_eq!(finished.len(), 1);
        let f = &finished[0];
        assert_eq!((f.state.as_str(), f.ended), ("merged", Some(1790133960)));
        assert_eq!(f.events.last().unwrap().kind, "cleaned-up");
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
    fn reads_announcements_from_poll_output() {
        let hex = |s: &str| s.bytes().map(|b| format!("{b:02x}")).collect::<String>();
        let out = format!(
            "{{\"replies\":[]}}\n@@SAY@@ 1790000000-a.txt {}\n@@SAY@@ 1790000001-b.txt {}\n@@SAY@@ 1790000002-c.txt {}\n@@SAY@@ bad.txt 4g\n@@LEDGER@@ 3\n@@SAY@@ x.txt {}\n",
            hex("[happy] fix login is done. Its pull request is ready.\n"),
            hex("  \n"),
            hex("[proud] Café ✓"),
            hex("[sad] not an announcement"),
        );
        assert_eq!(
            announcements(&out),
            vec![
                Reply { id: "say:1790000000-a.txt".into(), body: "[happy] fix login is done. Its pull request is ready.".into() },
                Reply { id: "say:1790000002-c.txt".into(), body: "[proud] Café ✓".into() },
            ]
        );
        // The receipts line is still found with announcement lines after it.
        let (receipts, size, _) = split_poll(&out).unwrap();
        assert!(receipts["replies"].is_array());
        assert_eq!(size, 3);
        assert!(announcements("{}\n@@LEDGER@@ 0\n").is_empty());
    }

    #[test]
    fn helper_announcements_round_trip() {
        if cfg!(windows) || std::process::Command::new("bash").arg("-c").arg("true").status().is_err() {
            return;
        }
        let home = std::env::temp_dir().join(format!("crew-say-{}", request_id()));
        std::fs::create_dir_all(home.join("bin")).unwrap();
        std::fs::create_dir_all(home.join("state/avatar-say")).unwrap();
        std::fs::write(home.join("AGENTS.md"), "").unwrap();
        let inbox = home.join("bin/fm-inbox.sh");
        std::fs::write(&inbox, "#!/bin/sh\necho '{}'\n").unwrap();
        std::process::Command::new("chmod").arg("+x").arg(&inbox).status().unwrap();
        let say = home.join("state/avatar-say");
        std::fs::write(say.join("2-b.txt"), "[happy] Second.").unwrap();
        std::fs::write(say.join("1-a.txt"), "[happy] First.").unwrap();
        std::fs::write(say.join("0-empty.txt"), "").unwrap();
        std::fs::write(say.join("3-c.tmp"), "[happy] Not yet.").unwrap();
        let helper = home.join("crew.sh");
        std::fs::write(&helper, HELPER).unwrap();
        let run = || {
            let out = std::process::Command::new("bash")
                .arg(&helper)
                .arg(&home)
                .args(["poll", "", "0", "100"])
                .output()
                .unwrap();
            String::from_utf8(out.stdout).unwrap()
        };
        let bodies: Vec<String> = announcements(&run()).into_iter().map(|r| r.body).collect();
        assert_eq!(bodies, vec!["[happy] First.", "[happy] Second."]);
        assert!(announcements(&run()).is_empty(), "each announcement is said once");
        assert!(say.join("3-c.tmp").exists() && !say.join("0-empty.txt").exists());
        std::fs::remove_dir_all(&say).unwrap();
        assert!(split_poll(&run()).is_some(), "an absent folder is fine");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn builds_the_helper_command_for_each_shell() {
        let helper = Path::new(r"C:\Users\Alex\AppData\Roaming\x\crew.sh");
        let cfg = |shell, distro: Option<&str>| CrewConfig { shell, distro: distro.map(String::from), home: "~/firstmate".into() };
        let (p, a) = helper_argv(helper, &cfg(Shell::GitBash(r"C:\Program Files\Git\bin\bash.exe".into()), None));
        assert_eq!(p, PathBuf::from(r"C:\Program Files\Git\bin\bash.exe"));
        assert_eq!(a, vec!["--login", "C:/Users/Alex/AppData/Roaming/x/crew.sh"]);
        let (p, a) = helper_argv(helper, &cfg(Shell::Wsl, Some("Ubuntu")));
        assert_eq!(p, PathBuf::from("wsl.exe"));
        assert_eq!(a, vec!["-d", "Ubuntu", "--exec", "/bin/bash", "--login", "/mnt/c/Users/Alex/AppData/Roaming/x/crew.sh"]);
    }

    #[test]
    fn skips_docker_desktop_distros() {
        let list = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(pick_distro(&list(&["docker-desktop", "Ubuntu-24.04"])).as_deref(), Some("Ubuntu-24.04"));
        assert_eq!(pick_distro(&list(&["docker-desktop", "docker-desktop-data"])), None);
        assert_eq!(pick_distro(&list(&["docker-desktop", "Debian", "Ubuntu"])).as_deref(), Some("Ubuntu"));
        let utf16: Vec<u8> = "docker-desktop\r\nUbuntu\r\n".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(parse_distro_list(&utf16), list(&["docker-desktop", "Ubuntu"]));
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
        let body = note_body("Aqua", "fix the flaky test", Character::None);
        assert!(body.starts_with("fix the flaky test\n\n"));
        assert!(body.contains("fm-inbox.sh reply"));
        assert!(body.contains("[happy]"));
        assert!(!body.contains("in character") && body.ends_with(".)"));
        let body = note_body("Nicole", "hi", Character::Full("Greedy but kind."));
        assert!(body.contains("in character as Nicole") && body.contains("Greedy but kind.\n)"));
        assert!(note_body("Nicole", "hi", Character::Same).contains("Keep replying in character as Nicole"));
    }
}

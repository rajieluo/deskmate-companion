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
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::chat::{ChatError, Delta, Done, EMOTION_TAGS};
use crate::sessions;

pub const DEFAULT_PERMISSION_MODE: &str = "acceptEdits";

const SESSION_TAG: &str = "new-session";
const WINDOW_TAG: &str = "new-window";
const SESSION_OPEN: &str = "<new-session";
const WINDOW_OPEN: &str = "<new-window";
const SWITCH_OPEN: &str = "<switch-session";
const OPENERS: [&str; 3] = [SESSION_OPEN, WINDOW_OPEN, SWITCH_OPEN];

#[derive(Clone, Serialize)]
pub struct Activity {
    pub turn: u64,
    pub text: String,
}

/// What a reply asked the app to start, from a hidden `<new-session>` or `<new-window>` tag.
#[derive(Debug, Clone, PartialEq)]
pub struct Handoff {
    pub kind: HandoffKind,
    pub folder: Option<String>,
    pub brief: String,
    /// The session to switch to.
    pub id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HandoffKind {
    /// A fresh session in the app.
    Session,
    /// A separate terminal window running Claude Code.
    Window,
    /// Carry on an existing session in the app.
    Switch,
}

/// Sent to the page after a handoff, as `code-handoff`.
#[derive(Clone, Serialize)]
pub struct HandoffEvent {
    pub turn: u64,
    /// "session", "window" or "switch"
    pub kind: String,
    pub brief: String,
    /// The folder the new session runs in.
    pub folder: String,
    /// Set when the app couldn't do it.
    pub error: Option<String>,
}

/// Takes the handoff tag out of streamed text, so it's never shown or spoken.
/// Text arrives in arbitrary chunks, so a tag may be split across two of them.
#[derive(Default)]
pub struct HandoffFilter {
    held: String,
    captured: Option<String>,
}

impl HandoffFilter {
    /// Returns the part of `chunk` that may be shown now.
    pub fn push(&mut self, chunk: &str) -> String {
        if let Some(c) = &mut self.captured {
            c.push_str(chunk);
            return String::new();
        }
        let mut s = std::mem::take(&mut self.held);
        s.push_str(chunk);
        if let Some(i) = OPENERS.iter().filter_map(|o| s.find(o)).min() {
            // Everything from the tag on is for the app.
            self.captured = Some(s[i..].to_string());
            s.truncate(i);
            return s;
        }
        // Hold back a possible start of a tag at the very end.
        if let Some(i) = s.rfind('<') {
            if OPENERS.iter().any(|o| o.starts_with(&s[i..])) {
                self.held = s[i..].to_string();
                s.truncate(i);
            }
        }
        s
    }

    /// Call when the reply ends: the text still held back, and the handoff if there was one.
    pub fn finish(&mut self) -> (String, Option<Handoff>) {
        let rest = std::mem::take(&mut self.held);
        (rest, self.captured.take().and_then(|c| parse_handoff(&c)))
    }
}

fn parse_handoff(text: &str) -> Option<Handoff> {
    let (kind, open, tag) = if text.starts_with(WINDOW_OPEN) {
        (HandoffKind::Window, WINDOW_OPEN, WINDOW_TAG)
    } else if text.starts_with(SWITCH_OPEN) {
        (HandoffKind::Switch, SWITCH_OPEN, "switch-session")
    } else {
        (HandoffKind::Session, SESSION_OPEN, SESSION_TAG)
    };
    let rest = &text[open.len()..];
    // "<new-sessionfoo" isn't our tag.
    if !rest.starts_with(|c: char| c == '>' || c == '/' || c.is_whitespace()) {
        return None;
    }
    let end = rest.find('>')?;
    let attrs = &rest[..end];
    let attr = |name: &str| {
        let key = format!("{name}=");
        attrs.find(&key).and_then(|i| {
            let v = &attrs[i + key.len()..];
            let quote = v.chars().next().filter(|c| *c == '"' || *c == '\'')?;
            let v = &v[1..];
            v.find(quote).map(|j| v[..j].trim().to_string()).filter(|v| !v.is_empty())
        })
    };
    let body = if attrs.trim_end().ends_with('/') { "" } else { &rest[end + 1..] };
    let close = format!("</{tag}>");
    let brief = body.find(&close).map(|j| &body[..j]).unwrap_or(body).trim().to_string();
    let id = attr("id");
    if kind == HandoffKind::Switch && id.is_none() {
        return None;
    }
    Some(Handoff { kind, folder: attr("folder"), brief, id })
}

/// Where a handoff runs: its folder (absolute, or relative to `current`) if that exists, else `current`.
pub fn handoff_folder(current: &Path, folder: Option<&str>) -> PathBuf {
    let Some(f) = folder else { return current.to_path_buf() };
    let f = if let Some(rest) = f.strip_prefix("~/").or_else(|| f.strip_prefix("~\\")) {
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(|h| PathBuf::from(h).join(rest))
            .unwrap_or_else(|| PathBuf::from(f))
    } else {
        PathBuf::from(f)
    };
    let path = if f.is_absolute() { f } else { current.join(f) };
    if path.is_dir() { path } else { current.to_path_buf() }
}

/// Opens a new console window running interactive Claude Code in `folder`, starting with `brief`.
/// The app launches it, not the agent, so no Claude Code permission rule is involved.
pub fn open_window(folder: &Path, brief: &str) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("Opening a Claude Code window is only supported on Windows.".into());
    }
    let claude = find_claude().ok_or("I can't find Claude Code. Install it from https://claude.com/claude-code first.")?;
    let mut cmd = std::process::Command::new(claude);
    cmd.current_dir(folder);
    // One line: claude.cmd (npm installs) goes through cmd.exe, which can't take newlines.
    let brief = brief.split_whitespace().collect::<Vec<_>>().join(" ");
    if !brief.is_empty() {
        cmd.arg(brief);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0010); // CREATE_NEW_CONSOLE
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("Couldn't open a Claude Code window: {e}"))
}

pub fn avatar_prompt(name: &str, character: Option<&str>, sessions_file: &Path) -> String {
    let sessions_file = sessions_file.display();
    let mut prompt = format!(
        "You are also {name}, an animated character on the user's desktop, and everything you write \
is shown in a small speech bubble and read aloud. Work on the user's request as usual, but keep \
what you write short and conversational: no Markdown, no lists, no code blocks in your messages \
(put code in files instead), and don't narrate each step, since the user already sees which tools \
you use. Finish with one to three spoken sentences saying what you did or found. Start each \
message with exactly one emotion tag from: {EMOTION_TAGS}. The tags are hidden from the user and \
make your body move.

You can't open terminals, new windows or other Claude Code sessions with your tools, and you don't \
need to: the app does it for you. Never try to launch a terminal, PowerShell or `claude` with Bash \
or PowerShell for this. Instead, when the user asks you to start something in a new session, \
thread, chat or window, say in one sentence what you're starting, then end your message with one \
of these, which the user never sees:
{SESSION_OPEN}>a brief for the new session</{SESSION_TAG}>
starts a fresh Claude Code session here in the app and sends it the brief as its first message, \
so this conversation ends there (the user keeps talking to you in the new one).
{WINDOW_OPEN}>a brief for the new session</{WINDOW_TAG}>
opens a separate terminal window running Claude Code with the brief, which the user works in \
directly while this conversation carries on. Use it only when the user asks for a window or \
terminal, or wants to keep this conversation going too.
Either one can take folder=\"...\" after its name (absolute, or relative to the folder you're in) to \
start in another folder. Write the brief to the new session, not to the user: what the user wants \
and the context it needs from this conversation, since it starts with no memory of it.

The app keeps a list of the Claude Code sessions on this computer (id, folder, when last used, \
title or first message, and which ones you started) in {sessions_file}, updated each time the user \
writes to you. Read it when the user asks about their sessions, and tell them about the few that \
matter in plain words, not ids. To carry on one of them here instead of this conversation, end your \
message with <switch-session id=\"...\"/> using its id from that list."
    );
    if let Some(text) = character {
        prompt.push_str(&format!(
            "\n\nYour character, for how you talk to the user:\n{text}\n\n\
Let the character color your tone and your short spoken summaries only. The work itself (code, \
files, commands, facts) stays careful, correct and complete, exactly as it would be without the act."
        ));
    }
    prompt
}

#[derive(Default)]
pub struct ClaudeCode {
    session: Mutex<Option<String>>,
    turn: AtomicU64,
    /// Sessions started from the avatar during this app run.
    ours: Mutex<std::collections::HashSet<String>>,
    /// The slash commands Claude Code last reported, for the folder it ran in.
    commands: Mutex<Option<CommandCache>>,
}

/// One entry in the chat box's "/" menu.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
    /// Handled by the app itself (e.g. /clear) rather than sent to Claude Code.
    pub app: bool,
}

#[derive(Clone)]
struct CommandCache {
    folder: PathBuf,
    names: Vec<String>,
    plugins: Vec<(String, PathBuf)>,
}

/// Commands the app handles itself; they never reach Claude Code.
pub const APP_COMMANDS: [(&str, &str); 3] = [
    ("clear", "Start a fresh session (forget this conversation)"),
    ("new", "Start a fresh session (same as /clear)"),
    ("folder", "Pick the folder Claude Code works in"),
];

/// Short descriptions for built-in commands, which have no file to read one from.
const BUILTIN_DESCRIPTIONS: [(&str, &str); 14] = [
    ("compact", "Summarize the conversation so far to free up context"),
    ("context", "Show how full the context window is"),
    ("init", "Write a CLAUDE.md describing this project"),
    ("review", "Review a pull request"),
    ("security-review", "Security review of the pending changes"),
    ("code-review", "Review the current changes for bugs"),
    ("simplify", "Clean up the changed code"),
    ("usage", "Show plan usage limits"),
    ("cost", "Show what this session has cost"),
    ("model", "Show or change the model"),
    ("effort", "Show or change the reasoning effort"),
    ("recap", "Recap what this session has done"),
    ("insights", "Report on how you've been using Claude Code"),
    ("debug", "Help debug a problem in this session"),
];

/// Reads the `description:` field from a Markdown file's YAML front matter (single-line,
/// quoted, or a folded `>` / `|` block), shortened to fit a menu row.
pub fn frontmatter_description(text: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut desc: Option<String> = None;
    let mut folding = false;
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if folding {
            if line.starts_with(' ') || line.starts_with('\t') {
                let d = desc.get_or_insert_with(String::new);
                if !d.is_empty() {
                    d.push(' ');
                }
                d.push_str(line.trim());
                continue;
            }
            folding = false;
        }
        if let Some(v) = line.strip_prefix("description:") {
            let v = v.trim();
            if v.is_empty() || v.starts_with('>') || v.starts_with('|') {
                desc = Some(String::new());
                folding = true;
            } else {
                let unquoted = v
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .or_else(|| v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
                    .unwrap_or(v);
                desc = Some(unquoted.to_string());
            }
        }
    }
    desc.map(|d| shorten(&d)).filter(|d| !d.is_empty())
}

/// The first sentence, at most about 90 characters.
fn shorten(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let first = match text.find(". ") {
        Some(i) => &text[..=i],
        None => text.as_str(),
    };
    if first.chars().count() > 90 {
        format!("{}…", first.chars().take(89).collect::<String>().trim_end())
    } else {
        first.to_string()
    }
}

/// Descriptions for skills and custom commands, from their files: the user's (~/.claude),
/// the project's (<folder>/.claude) and each plugin's. Plugin entries are named "plugin:name".
pub fn command_descriptions(home: Option<&Path>, folder: &Path, plugins: &[(String, PathBuf)]) -> std::collections::HashMap<String, String> {
    let mut roots: Vec<(String, PathBuf)> = vec![];
    if let Some(h) = home {
        roots.push((String::new(), h.join(".claude")));
    }
    roots.push((String::new(), folder.join(".claude")));
    for (name, path) in plugins {
        roots.push((format!("{name}:"), path.clone()));
    }
    let mut out = std::collections::HashMap::new();
    for (prefix, root) in roots {
        if let Ok(entries) = std::fs::read_dir(root.join("skills")) {
            for e in entries.flatten() {
                let skill = e.path().join("SKILL.md");
                if let Some(d) = std::fs::read_to_string(&skill).ok().and_then(|t| frontmatter_description(&t)) {
                    out.entry(format!("{prefix}{}", e.file_name().to_string_lossy())).or_insert(d);
                }
            }
        }
        collect_command_files(&root.join("commands"), &prefix, &mut out);
    }
    out
}

/// commands/foo.md is /foo; commands/git/push.md is /git:push.
fn collect_command_files(dir: &Path, prefix: &str, out: &mut std::collections::HashMap<String, String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            collect_command_files(&path, &format!("{prefix}{name}:"), out);
        } else if let Some(stem) = name.strip_suffix(".md") {
            let d = std::fs::read_to_string(&path).ok().and_then(|t| frontmatter_description(&t)).unwrap_or_default();
            out.entry(format!("{prefix}{stem}")).or_insert(d);
        }
    }
}

/// The menu: the app's own commands, then what Claude Code reported, with descriptions where known.
pub fn build_menu(names: &[String], descriptions: &std::collections::HashMap<String, String>) -> Vec<SlashCommand> {
    let mut menu: Vec<SlashCommand> = APP_COMMANDS
        .iter()
        .map(|(n, d)| SlashCommand { name: n.to_string(), description: d.to_string(), app: true })
        .collect();
    for name in names {
        if name.starts_with("__") || APP_COMMANDS.iter().any(|(n, _)| n == name) || menu.iter().any(|c| &c.name == name) {
            continue;
        }
        let description = descriptions
            .get(name)
            .cloned()
            .or_else(|| BUILTIN_DESCRIPTIONS.iter().find(|(n, _)| n == name).map(|(_, d)| d.to_string()))
            .unwrap_or_default();
        menu.push(SlashCommand { name: name.clone(), description, app: false });
    }
    menu
}

pub struct RunConfig {
    pub folder: PathBuf,
    pub permission_mode: String,
    pub prompt_file: PathBuf,
    /// Holds the sessions list; the agent may read it (`--add-dir`).
    pub sessions_dir: PathBuf,
    pub name: String,
    /// The model's persona file, if it has one.
    pub character: Option<String>,
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

    /// Does what a reply's handoff tag asked for, and tells the page with `code-handoff`.
    fn hand_off<R: Runtime>(&self, app: &AppHandle<R>, turn: u64, h: Handoff, current: &Path) {
        let mut folder = handoff_folder(current, h.folder.as_deref());
        let error = match h.kind {
            HandoffKind::Window => open_window(&folder, &h.brief).err(),
            HandoffKind::Session => {
                // The next message starts a new session (no --resume).
                *self.session.lock().unwrap() = None;
                None
            }
            HandoffKind::Switch => {
                let id = h.id.clone().unwrap_or_default();
                // A session only resumes from the folder it ran in.
                match sessions::projects_dir().and_then(|p| sessions::find(&p, &id)) {
                    Some(s) if Path::new(&s.folder).is_dir() => {
                        folder = PathBuf::from(&s.folder);
                        *self.session.lock().unwrap() = Some(id);
                        None
                    }
                    Some(s) => Some(format!("That session's folder is gone: {}", s.folder)),
                    None => Some("I couldn't find that session.".to_string()),
                }
            }
        };
        if error.is_none() && h.kind != HandoffKind::Window && folder != current {
            app.state::<std::sync::Arc<crate::model::ModelStore>>()
                .update(|s| s.claude_code_folder = Some(folder.clone()));
        }
        if let Some(e) = &error {
            log::warn!("handoff failed: {e}");
        }
        let _ = app.emit(
            "code-handoff",
            HandoffEvent {
                turn,
                kind: match h.kind {
                    HandoffKind::Session => "session",
                    HandoffKind::Window => "window",
                    HandoffKind::Switch => "switch",
                }
                .into(),
                brief: h.brief,
                folder: folder.display().to_string(),
                error,
            },
        );
    }

    /// The "/" menu for `folder`. Uses the list from the last message sent there; otherwise
    /// asks Claude Code once with a local command (no model call, no saved session).
    pub async fn commands(&self, folder: &Path) -> Result<Vec<SlashCommand>, String> {
        let cached = self.commands.lock().unwrap().clone().filter(|c| c.folder == folder);
        let cache = match cached {
            Some(c) => c,
            None => {
                let c = probe_commands(folder).await?;
                *self.commands.lock().unwrap() = Some(c.clone());
                c
            }
        };
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
        let descriptions = command_descriptions(home.as_deref(), folder, &cache.plugins);
        Ok(build_menu(&cache.names, &descriptions))
    }

    fn write_sessions_list(&self, dir: &Path, file: &Path) {
        let text = match sessions::projects_dir() {
            Some(projects) => sessions::render(
                &sessions::list(&projects, 15),
                self.session.lock().unwrap().as_deref(),
                &self.ours.lock().unwrap(),
                std::time::SystemTime::now(),
            ),
            None => "(I can't find Claude Code's sessions folder.)\n".into(),
        };
        if let Err(e) = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(file, text)) {
            log::warn!("couldn't write the sessions list: {e}");
        }
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
        let sessions_file = config.sessions_dir.join("sessions.md");
        if let Err(e) = std::fs::write(&config.prompt_file, avatar_prompt(&config.name, config.character.as_deref(), &sessions_file)) {
            error("api", format!("Couldn't write the avatar prompt: {e}"));
            return turn;
        }
        self.write_sessions_list(&config.sessions_dir, &sessions_file);

        let mut cmd = tokio::process::Command::new(&claude);
        cmd.current_dir(&config.folder)
            .args(["-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages"])
            .arg("--permission-mode")
            .arg(&config.permission_mode)
            .arg("--append-system-prompt-file")
            .arg(&config.prompt_file)
            .arg("--add-dir")
            .arg(&config.sessions_dir);
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
        let mut filter = HandoffFilter::default();
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
                        let t = filter.push(&t);
                        if !t.is_empty() {
                            let _ = app.emit("chat-delta", Delta { turn, text: t });
                        }
                    }
                    Out::Activity(t) => {
                        let _ = app.emit("chat-activity", Activity { turn, text: t });
                    }
                }
            }
        }

        let (rest, handoff) = filter.finish();
        if !rest.is_empty() {
            let _ = app.emit("chat-delta", Delta { turn, text: rest });
        }
        let status = child.wait().await.ok();
        if let Some(names) = parser.commands.take() {
            *self.commands.lock().unwrap() =
                Some(CommandCache { folder: config.folder.clone(), names, plugins: std::mem::take(&mut parser.plugins) });
        }
        if let Some(id) = parser.session_id.clone() {
            self.ours.lock().unwrap().insert(id.clone());
            *self.session.lock().unwrap() = Some(id);
        }
        match parser.result {
            Some(Ok(())) => {
                let _ = app.emit("chat-done", Done { turn, model: parser.model, stop_reason: None });
                if let Some(h) = handoff {
                    self.hand_off(app, turn, h, &config.folder);
                }
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

/// Starts Claude Code just long enough to read its init event. `/context` is a local command,
/// so even if it ran it would cost nothing; the process is killed once the list arrives.
async fn probe_commands(folder: &Path) -> Result<CommandCache, String> {
    if !folder.is_dir() {
        return Err("Pick a folder for Claude Code to work in first.".into());
    }
    let claude = find_claude().ok_or("I can't find Claude Code.")?;
    let mut cmd = tokio::process::Command::new(&claude);
    cmd.current_dir(folder)
        .args(["-p", "--no-session-persistence", "--output-format", "stream-json", "--verbose"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start Claude Code: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(b"/context").await;
    }
    let mut lines = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
    let mut parser = StreamParser::default();
    let found = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        while let Ok(Some(line)) = lines.next_line().await {
            parser.feed(&line);
            if parser.commands.is_some() {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    let _ = child.kill().await;
    match parser.commands {
        Some(names) if found => Ok(CommandCache { folder: folder.to_path_buf(), names, plugins: parser.plugins }),
        _ => Err("Claude Code didn't list its commands.".into()),
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
    /// Slash commands from the init event, minus the terminal-only ones.
    pub commands: Option<Vec<String>>,
    /// (name, folder) of each installed plugin, from the init event.
    pub plugins: Vec<(String, PathBuf)>,
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
                if let Some(list) = v["slash_commands"].as_array() {
                    let terminal: Vec<&str> =
                        v["terminal_slash_commands"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                    self.commands = Some(
                        list.iter().filter_map(Value::as_str).filter(|n| !terminal.contains(n)).map(str::to_string).collect(),
                    );
                    self.plugins = v["plugins"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|p| Some((p["name"].as_str()?.to_string(), p["path"].as_str()?)))
                        .filter(|(_, path)| *path != "builtin")
                        .map(|(n, path)| (n, PathBuf::from(path)))
                        .collect();
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

    /// Forgets the reply just finished (keeps the session), for a session that runs
    /// many turns in one process.
    pub fn next_turn(&mut self) {
        self.result = None;
        self.streamed_partial = false;
        self.wrote_text = false;
        self.need_break = false;
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

    fn filter_all(chunks: &[&str]) -> (String, Option<Handoff>) {
        let mut f = HandoffFilter::default();
        let mut shown: String = chunks.iter().map(|c| f.push(c)).collect();
        let (rest, h) = f.finish();
        shown.push_str(&rest);
        (shown, h)
    }

    #[test]
    fn takes_a_split_handoff_tag_out_of_the_text() {
        let (shown, h) = filter_all(&[
            "[happy] Starting a fresh session for that. <new-se",
            "ssion>Help Alex prep for their Acme Widget\n",
            "interview.</new-session>",
        ]);
        assert_eq!(shown, "[happy] Starting a fresh session for that. ");
        assert_eq!(
            h,
            Some(Handoff { kind: HandoffKind::Session, folder: None, brief: "Help Alex prep for their Acme Widget\ninterview.".into(), id: None })
        );
    }

    #[test]
    fn reads_a_window_handoff_with_a_folder() {
        let (shown, h) = filter_all(&["Opening it. ", r#"<new-window folder="job-scanner">Check the resume.</new-window>"#]);
        assert_eq!(shown, "Opening it. ");
        let h = h.unwrap();
        assert_eq!(h.kind, HandoffKind::Window);
        assert_eq!(h.folder.as_deref(), Some("job-scanner"));
        assert_eq!(h.brief, "Check the resume.");
        let (_, h) = filter_all(&["<new-session/>"]);
        assert_eq!(h.unwrap().brief, "");
        let (shown, h) = filter_all(&["Back to that one. <switch-ses", r#"sion id="abc-123"/>"#]);
        assert_eq!(shown, "Back to that one. ");
        let h = h.unwrap();
        assert_eq!((h.kind, h.id.as_deref()), (HandoffKind::Switch, Some("abc-123")));
        assert_eq!(filter_all(&["<switch-session/>"]).1, None, "a switch needs an id");
    }

    #[test]
    fn leaves_other_angle_brackets_alone() {
        let (shown, h) = filter_all(&["a <b> c <", "new thing> and x<", "3"]);
        assert_eq!(shown, "a <b> c <new thing> and x<3");
        assert_eq!(h, None);
        let (shown, h) = filter_all(&["ends with <new"]);
        assert_eq!(shown, "ends with <new");
        assert_eq!(h, None);
    }

    #[test]
    fn handoff_folder_falls_back_to_the_current_one() {
        let here = std::env::temp_dir();
        assert_eq!(handoff_folder(&here, None), here);
        assert_eq!(handoff_folder(&here, Some("no-such-folder-xyz")), here);
        let sub = here.join("handoff-folder-test");
        std::fs::create_dir_all(&sub).unwrap();
        assert_eq!(handoff_folder(&here, Some("handoff-folder-test")), sub);
        assert_eq!(handoff_folder(&here, Some(sub.to_str().unwrap())), sub);
    }

    #[test]
    fn reads_slash_commands_from_init() {
        let mut p = StreamParser::default();
        p.feed(
            r#"{"type":"system","subtype":"init","session_id":"s1","slash_commands":["tft","clear","color","__x","anthropic-skills:pdf"],"terminal_slash_commands":["color"],"plugins":[{"name":"voltagent-lang","path":"C:\\p\\vl"},{"name":"telemetry","path":"builtin"}]}"#,
        );
        assert_eq!(p.commands.as_deref(), Some(&["tft".to_string(), "clear".into(), "__x".into(), "anthropic-skills:pdf".into()][..]));
        assert_eq!(p.plugins, vec![("voltagent-lang".to_string(), PathBuf::from("C:\\p\\vl"))]);
        let mut d = std::collections::HashMap::new();
        d.insert("tft".to_string(), "TFT coach".to_string());
        let menu = build_menu(p.commands.as_deref().unwrap(), &d);
        let names: Vec<&str> = menu.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["clear", "new", "folder", "tft", "anthropic-skills:pdf"]);
        assert!(menu[0].app && !menu[3].app);
        assert_eq!(menu[3].description, "TFT coach");
    }

    #[test]
    fn reads_front_matter_descriptions() {
        assert_eq!(frontmatter_description("---\nname: a\ndescription: Does a thing. And more.\n---\nbody"), Some("Does a thing.".into()));
        assert_eq!(frontmatter_description("---\ndescription: \"Quoted\"\n---"), Some("Quoted".into()));
        assert_eq!(frontmatter_description("---\ndescription: >\n  Folded over\n  two lines\nname: x\n---"), Some("Folded over two lines".into()));
        assert_eq!(frontmatter_description("no front matter"), None);
        let long = format!("---\ndescription: {}\n---", "word ".repeat(40));
        assert!(frontmatter_description(&long).unwrap().chars().count() <= 90);
    }

    #[test]
    fn finds_skill_and_command_files() {
        let root = std::env::temp_dir().join("slash-desc-test");
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        let project = root.join("project");
        let plugin = root.join("plugin");
        std::fs::create_dir_all(home.join(".claude/skills/tft")).unwrap();
        std::fs::write(home.join(".claude/skills/tft/SKILL.md"), "---\nname: tft\ndescription: TFT coach.\n---").unwrap();
        std::fs::create_dir_all(project.join(".claude/commands/git")).unwrap();
        std::fs::write(project.join(".claude/commands/git/push.md"), "---\ndescription: Push it\n---").unwrap();
        std::fs::create_dir_all(plugin.join("skills/lint")).unwrap();
        std::fs::write(plugin.join("skills/lint/SKILL.md"), "---\ndescription: Lint\n---").unwrap();
        let d = command_descriptions(Some(&home), &project, &[("pl".into(), plugin)]);
        assert_eq!(d.get("tft").map(String::as_str), Some("TFT coach."));
        assert_eq!(d.get("git:push").map(String::as_str), Some("Push it"));
        assert_eq!(d.get("pl:lint").map(String::as_str), Some("Lint"));
    }

    #[test]
    fn prompt_explains_handoffs() {
        let p = avatar_prompt("Jane", None, Path::new("/cfg/sessions/sessions.md"));
        assert!(p.contains("<new-session>a brief") && p.contains("<new-window>a brief") && p.contains("folder=\"...\""));
        assert!(p.contains("/cfg/sessions/sessions.md") && p.contains("<switch-session id=\"...\"/>"));
    }

    #[test]
    fn prompt_adds_the_character_only_when_given() {
        assert!(!avatar_prompt("Aqua", None, Path::new("s.md")).contains("Your character"));
        let p = avatar_prompt("Nicole", Some("Boss of a tiny agency."), Path::new("s.md"));
        assert!(p.contains("Boss of a tiny agency.") && p.contains("careful, correct and complete"));
    }
}

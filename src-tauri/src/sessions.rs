//! Lists the Claude Code sessions saved on this computer (`~/.claude/projects/<folder>/<id>.jsonl`),
//! so the avatar can tell the user about them and switch to one. The app writes the list to a file
//! the agent is allowed to read (`--add-dir`), instead of the agent digging through `~/.claude`.

use std::{
    collections::HashSet,
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct SessionInfo {
    pub id: String,
    pub folder: String,
    pub modified: SystemTime,
    pub title: String,
}

/// Where Claude Code keeps its sessions.
pub fn projects_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return Some(PathBuf::from(dir).join("projects"));
    }
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
    Some(PathBuf::from(home).join(".claude").join("projects"))
}

/// The `limit` most recently used sessions, newest first.
pub fn list(projects: &Path, limit: usize) -> Vec<SessionInfo> {
    let mut files: Vec<(SystemTime, PathBuf)> = vec![];
    for dir in std::fs::read_dir(projects).into_iter().flatten().flatten() {
        // Only the top level of each project folder: subfolders hold helper (subagent) sessions.
        for f in std::fs::read_dir(dir.path()).into_iter().flatten().flatten() {
            let path = f.path();
            if path.extension().is_some_and(|e| e == "jsonl") {
                if let Ok(m) = f.metadata().and_then(|m| m.modified()) {
                    files.push((m, path));
                }
            }
        }
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files.into_iter().filter_map(|(m, p)| read_session(&p, m)).take(limit).collect()
}

/// The session with this id, in any project folder.
pub fn find(projects: &Path, id: &str) -> Option<SessionInfo> {
    // Ids are uuids; anything else could walk out of the folder.
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    for dir in std::fs::read_dir(projects).ok()?.flatten() {
        let path = dir.path().join(format!("{id}.jsonl"));
        if let Ok(m) = path.metadata().and_then(|m| m.modified()) {
            if let Some(s) = read_session(&path, m) {
                return Some(s);
            }
        }
    }
    None
}

/// Reads a session's id, folder and title from the start and end of its file.
pub fn read_session(path: &Path, modified: SystemTime) -> Option<SessionInfo> {
    const HEAD: u64 = 128 * 1024;
    const TAIL: u64 = 64 * 1024;
    let file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut id = path.file_stem()?.to_string_lossy().into_owned();
    let mut folder = None;
    let mut first_message = None;
    for line in BufReader::new(file.take(HEAD)).lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        if folder.is_none() {
            folder = v["cwd"].as_str().map(str::to_string);
        }
        if let Some(s) = v["sessionId"].as_str() {
            id = s.to_string();
        }
        if first_message.is_none() && v["type"] == "user" && !v["isMeta"].as_bool().unwrap_or(false) {
            first_message = user_text(&v["message"]["content"]);
        }
        if folder.is_some() && first_message.is_some() {
            break;
        }
    }
    // Titles (set with /rename, or written by Claude Code) are appended later, so look at the end too.
    let mut tail = vec![];
    let mut file = File::open(path).ok()?;
    if file.seek(SeekFrom::Start(len.saturating_sub(TAIL))).is_ok() {
        let _ = file.read_to_end(&mut tail);
    }
    let (mut custom, mut other) = (None, None);
    for line in String::from_utf8_lossy(&tail).lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let pick = |key: &str| v[key].as_str().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
        if let Some(t) = pick("customTitle") {
            custom = Some(t);
        }
        if let Some(t) = pick("aiTitle").or_else(|| pick("summary")) {
            other = Some(t);
        }
    }
    let title = custom.or(other);
    let title = title.or(first_message)?; // an empty session isn't worth listing
    Some(SessionInfo { id, folder: folder.unwrap_or_default(), modified, title: shorten(&title, 90) })
}

/// The user's own words from a message, skipping app-added blocks like `<command-name>`.
fn user_text(content: &Value) -> Option<String> {
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks.iter().filter_map(|b| (b["type"] == "text").then(|| b["text"].as_str()).flatten()).collect::<Vec<_>>().join(" "),
        _ => return None,
    };
    let text = text.trim();
    (!text.is_empty() && !text.starts_with('<')).then(|| text.to_string())
}

fn shorten(s: &str, max: usize) -> String {
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() > max {
        format!("{}…", one_line.chars().take(max - 1).collect::<String>())
    } else {
        one_line
    }
}

fn ago(modified: SystemTime, now: SystemTime) -> String {
    let secs = now.duration_since(modified).unwrap_or(Duration::ZERO).as_secs();
    match secs {
        0..=89 => "just now".into(),
        90..=5399 => format!("{} min ago", (secs + 30) / 60),
        5400..=129_599 => format!("{} h ago", (secs + 1800) / 3600),
        _ => format!("{} days ago", (secs + 43_200) / 86_400),
    }
}

/// The list as the agent reads it.
pub fn render(sessions: &[SessionInfo], current: Option<&str>, ours: &HashSet<String>, now: SystemTime) -> String {
    let mut out = String::from(
        "# Claude Code sessions on this computer, most recent first\n\
# id | folder | last used | title or first message | notes\n",
    );
    if sessions.is_empty() {
        out.push_str("(none yet)\n");
    }
    for s in sessions {
        let mut notes = vec![];
        if current == Some(s.id.as_str()) {
            notes.push("this conversation");
        }
        if ours.contains(&s.id) {
            notes.push("started from the avatar");
        }
        out.push_str(&format!("{} | {} | {} | {} | {}\n", s.id, s.folder, ago(s.modified, now), s.title, notes.join(", ")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_lists_sessions() {
        let root = std::env::temp_dir().join(format!("sessions-test-{}", std::process::id()));
        let proj = root.join("C--Users-Alex-Documents-projects");
        std::fs::create_dir_all(proj.join("abc").join("subagents")).unwrap();
        std::fs::write(
            proj.join("abc.jsonl"),
            [
                r#"{"type":"user","isMeta":true,"cwd":"C:\\Users\\Alex\\Documents\\projects","sessionId":"abc","message":{"role":"user","content":"<local-command-caveat>x"}}"#,
                r#"{"type":"user","cwd":"C:\\Users\\Alex\\Documents\\projects","sessionId":"abc","message":{"role":"user","content":"Help me prep for the Acme Widget interview"}}"#,
                r#"{"type":"assistant","message":{"content":[]}}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        std::fs::write(
            proj.join("def.jsonl"),
            [
                r#"{"type":"user","cwd":"C:\\x","sessionId":"def","message":{"content":[{"type":"text","text":"fix the build"}]}}"#,
                r#"{"type":"custom-title","customTitle":"Build fix","sessionId":"def"}"#,
            ]
            .join("\n"),
        )
        .unwrap();
        std::fs::write(proj.join("empty.jsonl"), "").unwrap();
        std::fs::write(proj.join("abc").join("subagents").join("agent-1.jsonl"), "{}").unwrap();

        let list = list(&root, 10);
        assert_eq!(list.len(), 2);
        let abc = list.iter().find(|s| s.id == "abc").unwrap();
        assert_eq!(abc.title, "Help me prep for the Acme Widget interview");
        assert_eq!(abc.folder, "C:\\Users\\Alex\\Documents\\projects");
        assert_eq!(list.iter().find(|s| s.id == "def").unwrap().title, "Build fix");
        assert_eq!(find(&root, "def").unwrap().folder, "C:\\x");
        assert_eq!(find(&root, "nope"), None);
        assert_eq!(find(&root, "../abc"), None);

        let ours: HashSet<String> = ["abc".to_string()].into();
        let text = render(&list, Some("def"), &ours, SystemTime::now());
        assert!(text.contains("abc | C:\\Users\\Alex\\Documents\\projects | just now | Help me prep"));
        assert!(text.contains("started from the avatar"));
        assert!(text.contains("Build fix | this conversation"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn says_how_long_ago() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let at = |s| now - Duration::from_secs(s);
        assert_eq!(ago(at(10), now), "just now");
        assert_eq!(ago(at(600), now), "10 min ago");
        assert_eq!(ago(at(3 * 3600), now), "3 h ago");
        assert_eq!(ago(at(3 * 86_400), now), "3 days ago");
    }
}

//! Chat history: every message is appended to `history.jsonl` in the app's data
//! folder, one JSON object per line, so it survives restarts and can be read by
//! the Chat history window (or any text editor).

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

/// The history window shows at most this many of the latest messages.
const MAX_READ: usize = 5000;

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Entry {
    /// Unix time in milliseconds.
    pub time: u64,
    /// "user" or "assistant"
    pub role: String,
    pub text: String,
    /// Character name at the time, e.g. "Nicole".
    #[serde(default)]
    pub name: String,
    /// "api" or "claude_code"
    #[serde(default)]
    pub mode: String,
}

pub struct History {
    path: PathBuf,
    lock: Mutex<()>,
}

impl History {
    pub fn new(dir: PathBuf) -> Self {
        Self { path: dir.join("history.jsonl"), lock: Mutex::new(()) }
    }

    #[cfg(test)]
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn append(&self, role: &str, text: &str, name: &str, mode: &str) -> Result<(), String> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(());
        }
        let time = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        let entry = Entry { time, role: role.into(), text: text.into(), name: name.into(), mode: mode.into() };
        let line = serde_json::to_string(&entry).map_err(|e| e.to_string())?;
        let _guard = self.lock.lock().unwrap();
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let mut f = OpenOptions::new().create(true).append(true).open(&self.path).map_err(|e| e.to_string())?;
        writeln!(f, "{line}").map_err(|e| e.to_string())
    }

    /// The latest entries, oldest first. Unreadable lines are skipped.
    pub fn read(&self) -> Vec<Entry> {
        let _guard = self.lock.lock().unwrap();
        let Ok(text) = fs::read_to_string(&self.path) else { return Vec::new() };
        let mut entries: Vec<Entry> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        if entries.len() > MAX_READ {
            entries.drain(..entries.len() - MAX_READ);
        }
        entries
    }

    pub fn clear(&self) -> Result<(), String> {
        let _guard = self.lock.lock().unwrap();
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_and_reads_back() {
        let dir = std::env::temp_dir().join(format!("da-history-{}", std::process::id()));
        let h = History::new(dir.clone());
        h.clear().unwrap();
        h.append("user", "hi", "Nicole", "api").unwrap();
        h.append("assistant", "  ", "Nicole", "api").unwrap(); // blank: skipped
        h.append("assistant", "Hello!\nHow are you?", "Nicole", "api").unwrap();
        let e = h.read();
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].role, "user");
        assert_eq!(e[1].text, "Hello!\nHow are you?");
        fs::write(h.path(), "not json\n").unwrap();
        assert!(h.read().is_empty());
        h.clear().unwrap();
        let _ = fs::remove_dir_all(dir);
    }
}

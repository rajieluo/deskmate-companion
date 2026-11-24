//! Kokoro's English voices, and the voice a model folder asks for (Kokoro's,
//! and the cloned voice for Pocket TTS).

use std::path::Path;

/// (id, name, group). Groups appear in the tray in this order.
pub const VOICES: &[(&str, &str, &str)] = &[
    ("af_heart", "Heart ★", "American female"),
    ("af_bella", "Bella ★", "American female"),
    ("af_nicole", "Nicole (soft, whispery)", "American female"),
    ("af_aoede", "Aoede", "American female"),
    ("af_kore", "Kore", "American female"),
    ("af_sarah", "Sarah", "American female"),
    ("af_nova", "Nova", "American female"),
    ("af_sky", "Sky", "American female"),
    ("af_alloy", "Alloy", "American female"),
    ("af_jessica", "Jessica", "American female"),
    ("af_river", "River", "American female"),
    ("am_michael", "Michael", "American male"),
    ("am_fenrir", "Fenrir", "American male"),
    ("am_puck", "Puck", "American male"),
    ("am_echo", "Echo", "American male"),
    ("am_eric", "Eric", "American male"),
    ("am_liam", "Liam", "American male"),
    ("am_onyx", "Onyx", "American male"),
    ("am_adam", "Adam", "American male"),
    ("am_santa", "Santa", "American male"),
    ("bf_emma", "Emma ★", "British female"),
    ("bf_isabella", "Isabella", "British female"),
    ("bf_alice", "Alice", "British female"),
    ("bf_lily", "Lily", "British female"),
    ("bm_george", "George", "British male"),
    ("bm_fable", "Fable", "British male"),
    ("bm_lewis", "Lewis", "British male"),
    ("bm_daniel", "Daniel", "British male"),
];

pub const GROUPS: &[&str] = &["American female", "American male", "British female", "British male"];

pub const DEFAULT: &str = "af_heart";

pub fn is_voice(id: &str) -> bool {
    VOICES.iter().any(|(v, _, _)| *v == id)
}

pub fn name(id: &str) -> &str {
    VOICES.iter().find(|(v, _, _)| *v == id).map(|(_, n, _)| *n).unwrap_or(id)
}

/// The voice a model folder sets: the first line of `voice.txt`, or a
/// `voice:` line in the front matter of `persona.md`.
pub fn folder_voice(dir: &Path) -> Option<String> {
    let valid = |v: &str| {
        let v = v.trim().trim_matches(|c| c == '"' || c == '\'').to_lowercase();
        is_voice(&v).then_some(v)
    };
    if let Ok(text) = std::fs::read_to_string(dir.join("voice.txt")) {
        if let Some(v) = text.lines().next().and_then(valid) {
            return Some(v);
        }
    }
    let persona = std::fs::read_to_string(dir.join("persona.md")).ok()?;
    front_matter(&persona, "voice").and_then(|v| valid(&v))
}

/// The voice Pocket TTS should use for a model folder, if it has one:
/// `pocket_voice:` in persona.md's front matter (a preset name such as `alba`,
/// or a .safetensors / .wav file, relative to the folder), else the folder's
/// `voice.safetensors` (made with `pocket-tts export-voice`), else `voice.wav`.
/// Returns a preset name or an absolute file path.
pub fn pocket_voice(dir: &Path) -> Option<String> {
    let persona = std::fs::read_to_string(dir.join("persona.md")).unwrap_or_default();
    if let Some(v) = front_matter(&persona, "pocket_voice") {
        let v = v.trim_matches(|c| c == '"' || c == '\'');
        let is_file = v.contains(['.', '/', '\\']);
        if is_file {
            let path = dir.join(v);
            if path.is_file() {
                return Some(path.display().to_string());
            }
            log::warn!("pocket_voice {} not found in {}", v, dir.display());
        } else if !v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Some(v.to_lowercase());
        }
    }
    ["voice.safetensors", "voice.wav"]
        .iter()
        .map(|f| dir.join(f))
        .find(|p| p.is_file())
        .map(|p| p.display().to_string())
}

fn front_matter(text: &str, key: &str) -> Option<String> {
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    for line in lines {
        let line = line.trim();
        if line == "---" {
            break;
        }
        if let Some(v) = line.strip_prefix(key).and_then(|rest| rest.strip_prefix(':')) {
            return Some(v.trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_voice_from_files() {
        let dir = std::env::temp_dir().join(format!("voice-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(folder_voice(&dir), None);
        std::fs::write(dir.join("persona.md"), "---\nname: Nicole\nvoice: \"af_nicole\"\n---\nYou are Nicole.").unwrap();
        assert_eq!(folder_voice(&dir).as_deref(), Some("af_nicole"));
        std::fs::write(dir.join("voice.txt"), "BF_EMMA\n").unwrap();
        assert_eq!(folder_voice(&dir).as_deref(), Some("bf_emma"));
        std::fs::write(dir.join("voice.txt"), "nobody\n").unwrap();
        assert_eq!(folder_voice(&dir).as_deref(), Some("af_nicole"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn finds_the_pocket_voice() {
        let dir = std::env::temp_dir().join(format!("pocket-voice-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(pocket_voice(&dir), None);
        std::fs::write(dir.join("voice.wav"), b"RIFF").unwrap();
        assert_eq!(pocket_voice(&dir), Some(dir.join("voice.wav").display().to_string()));
        std::fs::write(dir.join("voice.safetensors"), b"x").unwrap();
        assert_eq!(pocket_voice(&dir), Some(dir.join("voice.safetensors").display().to_string()));
        // `voice:` and `pocket_voice:` don't get mixed up.
        std::fs::write(dir.join("persona.md"), "---\nvoice: af_bella\npocket_voice: Alba\n---\nHi").unwrap();
        assert_eq!(pocket_voice(&dir).as_deref(), Some("alba"));
        assert_eq!(folder_voice(&dir).as_deref(), Some("af_bella"));
        std::fs::write(dir.join("persona.md"), "---\npocket_voice: \"other.wav\"\n---\n").unwrap();
        // A missing file falls back to the folder's own voice file.
        assert_eq!(pocket_voice(&dir), Some(dir.join("voice.safetensors").display().to_string()));
        std::fs::write(dir.join("other.wav"), b"RIFF").unwrap();
        assert_eq!(pocket_voice(&dir), Some(dir.join("other.wav").display().to_string()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn every_voice_has_a_known_group() {
        assert!(VOICES.iter().all(|(_, _, g)| GROUPS.contains(g)));
        assert!(is_voice(DEFAULT));
    }
}

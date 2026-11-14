//! Which Live2D model is loaded, where it lives, and the `model://` protocol that serves it.

use std::{
    fs,
    path::{Component, Path, PathBuf},
    sync::Mutex,
};

use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use tauri::http::{header, Response, StatusCode};

/// Default model on the developer's machine: Aqua, under Documents\live2d models.
const DEFAULT_MODEL: &[&str] = &["live2d models", "1014100aqua", "1014100.model3.json"];

/// Everything saved in `settings.json` in the app's config folder.
/// Unknown or missing fields fall back to defaults, so the file can be hand-edited.
#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub model_path: Option<PathBuf>,
    /// What the character calls itself. Default: taken from the model's folder name.
    pub character_name: Option<String>,
    /// Claude model id. Default: claude-opus-5-5.
    pub chat_model: Option<String>,
    /// low | medium | high | xhigh | max. Default: low (fast, cheap replies).
    pub chat_effort: Option<String>,
    /// Replaces the whole built-in persona prompt when set.
    pub persona: Option<String>,
    /// Speak replies out loud with Kokoro. Default: true.
    pub voice_enabled: Option<bool>,
    /// Kokoro voice id, e.g. af_heart, af_bella, af_nicole. Default: af_heart.
    pub voice: Option<String>,
    /// Speaking speed, 0.5 to 2. Default: 1.
    pub voice_speed: Option<f32>,
    /// Voice pitch as a factor, 0.75 (lower) to 1.35 (higher). Default: 1.
    pub voice_pitch: Option<f32>,
    /// Where Kokoro runs: "cpu" (default, reliable) or "gpu" (DirectML in the app;
    /// WebGPU in the slower built-in voice, which garbles on some graphics drivers).
    pub voice_device: Option<String>,
    /// Voice model variant for the native voice: q8 (default, ~90 MB), q8f16,
    /// fp16 or fp32 (~325 MB). Whichever runs fastest on this CPU.
    pub voice_model: Option<String>,
    /// Whisper model for voice input: tiny.en, base.en (default), small.en, medium.en, or multilingual ones.
    pub stt_model: Option<String>,
    /// "api" (default): chat through the Claude API with an API key.
    /// "claude_code": run Claude Code in `claude_code_folder` with your Claude Code login.
    /// "crew": talk to a firstmate home in WSL, which runs several Claude Code sessions (see crew.rs).
    pub brain: Option<String>,
    /// Folder Claude Code works in.
    pub claude_code_folder: Option<PathBuf>,
    /// Claude Code permission mode: acceptEdits (default), auto, manual, bypassPermissions.
    pub claude_code_permission_mode: Option<String>,
    /// Spoken language code for voice input, e.g. "en" (default) or "auto" with a multilingual model.
    pub stt_language: Option<String>,
    /// Per-model overrides for which parameters follow the mouse, keyed by the
    /// model's folder name, e.g. {"xch003_01": {"angle_x": "Param12"}}. Keys:
    /// angle_x, angle_y, angle_z, eye_x, eye_y, body_x. Use "" to turn one off.
    pub look_params: Option<serde_json::Map<String, serde_json::Value>>,
    /// Per-model parameter values held every frame, keyed by folder name, e.g.
    /// {"Nicole": {"MouthJawOpen": 0}}. Lip sync still moves the mouth while speaking.
    pub model_params: Option<serde_json::Map<String, serde_json::Value>>,
    /// Global hotkeys, e.g. {"talk": "Ctrl+Shift+Space", "listen": "Ctrl+Alt+M", "mute": "Ctrl+Alt+U"}.
    /// Use "" to turn one off.
    pub hotkeys: Option<std::collections::HashMap<String, String>>,
    /// Fade out the bottom edge of the figure. Default: true.
    pub fade_bottom: Option<bool>,
    /// Head and eyes follow the mouse cursor. Default: true.
    pub follow_mouse: Option<bool>,
    /// Voice picked in the tray for a model, keyed by folder name, e.g. {"Nicole": "af_nicole"}.
    /// Wins over the folder's own voice.txt / persona.md and over `voice`.
    pub model_voices: Option<serde_json::Map<String, serde_json::Value>>,
    /// Last good size of the main window in logical pixels, [width, height].
    /// Saved here rather than by the window-state plugin, which on Windows could
    /// save the 160x28 minimized size.
    pub window_size: Option<[f64; 2]>,
    /// Crew mode on Windows: "git-bash" (firstmate-for-windows, the default when Git for
    /// Windows is installed) or "wsl".
    pub crew_shell: Option<String>,
    /// Crew mode: WSL distribution firstmate runs in. Default: WSL's default distribution.
    pub crew_distro: Option<String>,
    /// Crew mode: the firstmate home inside WSL. Default: ~/firstmate.
    pub crew_home: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct ModelInfo {
    /// File name of the .model3.json inside the model folder.
    pub file_name: String,
    /// Full path, for display.
    pub path: String,
    /// Bumped on every model change; used as the first URL segment so the
    /// webview never serves a cached file from a previous model.
    pub generation: u64,
    /// This model's entry from `look_params`, if any.
    pub look: Option<serde_json::Value>,
    /// This model's entry from `model_params`, if any.
    pub params: Option<serde_json::Value>,
}

struct Inner {
    settings: Settings,
    generation: u64,
}

pub struct ModelStore {
    settings_file: PathBuf,
    inner: Mutex<Inner>,
}

impl ModelStore {
    pub fn load(config_dir: PathBuf, documents: Option<PathBuf>) -> Self {
        let settings_file = config_dir.join("settings.json");
        let saved: Settings = fs::read_to_string(&settings_file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let mut settings = saved;
        settings.model_path = settings.model_path.filter(|p| p.is_file()).or_else(|| {
            let default = DEFAULT_MODEL.iter().fold(documents?, |p, part| p.join(part));
            default.is_file().then_some(default)
        });
        Self { settings_file, inner: Mutex::new(Inner { settings, generation: 1 }) }
    }

    pub fn update(&self, change: impl FnOnce(&mut Settings)) {
        change(&mut self.inner.lock().unwrap().settings);
        self.save();
    }

    pub fn settings(&self) -> Settings {
        self.inner.lock().unwrap().settings.clone()
    }

    pub fn current(&self) -> Option<ModelInfo> {
        let inner = self.inner.lock().unwrap();
        let path = inner.settings.model_path.as_ref()?;
        let folder = path.parent().and_then(|p| p.file_name()).map(|f| f.to_string_lossy().into_owned());
        let look = inner.settings.look_params.as_ref().zip(folder.clone()).and_then(|(m, f)| m.get(&f).cloned());
        let params = inner.settings.model_params.as_ref().zip(folder).and_then(|(m, f)| m.get(&f).cloned());
        Some(ModelInfo {
            file_name: path.file_name()?.to_string_lossy().into_owned(),
            path: path.display().to_string(),
            generation: inner.generation,
            look,
            params,
        })
    }

    /// What the character is called: `character_name` if set, else the model folder's name.
    pub fn character_name(&self) -> String {
        let s = self.settings();
        s.character_name
            .filter(|n| !n.trim().is_empty())
            .or_else(|| {
                let folder = s.model_path.as_ref()?.parent()?.file_name()?.to_string_lossy().into_owned();
                name_from_folder(&folder)
            })
            .unwrap_or_else(|| "Assistant".into())
    }

    /// The current model's persona file, if it has one. See `find_persona`.
    pub fn model_persona(&self) -> Option<String> {
        find_persona(self.settings_file.parent()?, &self.current_dir()?)
    }

    /// Name of the current model's folder.
    pub fn folder_name(&self) -> Option<String> {
        let s = self.settings();
        Some(s.model_path.as_ref()?.parent()?.file_name()?.to_string_lossy().into_owned())
    }

    /// The voice for the current model and whether it was picked in the tray:
    /// the tray pick, else the folder's own voice, else `voice`, else af_heart.
    pub fn voice(&self) -> (String, bool) {
        let s = self.settings();
        if let Some(v) = self
            .folder_name()
            .and_then(|f| s.model_voices.as_ref()?.get(&f)?.as_str().map(str::to_string))
            .filter(|v| crate::voices::is_voice(v))
        {
            return (v, true);
        }
        let v = self
            .current_dir()
            .and_then(|d| crate::voices::folder_voice(&d))
            .or(s.voice.filter(|v| !v.trim().is_empty()))
            .unwrap_or_else(|| crate::voices::DEFAULT.into());
        (v, false)
    }

    /// Picks (Some) or clears (None) the tray voice for the current model.
    pub fn pick_voice(&self, voice: Option<&str>) {
        let folder = self.folder_name();
        self.update(|s| match folder {
            Some(f) => {
                let map = s.model_voices.get_or_insert_with(Default::default);
                match voice {
                    Some(v) => {
                        map.insert(f, v.into());
                    }
                    None => {
                        map.remove(&f);
                    }
                }
            }
            None => s.voice = voice.map(str::to_string),
        });
    }

    /// The folder holding the current model's .model3.json.
    pub fn current_dir(&self) -> Option<PathBuf> {
        let inner = self.inner.lock().unwrap();
        inner.settings.model_path.as_ref()?.parent().map(Path::to_path_buf)
    }

    pub fn set(&self, path: &Path) -> Result<ModelInfo, String> {
        let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        if !name.ends_with(".model3.json") {
            return Err(format!(
                "{} is not a Cubism 3/4 model file (expected *.model3.json)",
                path.display()
            ));
        }
        if !path.is_file() {
            return Err(format!("{} does not exist", path.display()));
        }
        {
            let mut inner = self.inner.lock().unwrap();
            inner.settings.model_path = Some(path.to_path_buf());
            inner.generation += 1;
        }
        self.save();
        Ok(self.current().expect("model just set"))
    }

    pub fn save(&self) {
        let settings = self.settings();
        if let Some(dir) = self.settings_file.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(&settings) {
            let _ = fs::write(&self.settings_file, json);
        }
    }

    /// Serves `/<generation>/<relative path>` from the current model folder.
    pub fn serve(&self, uri_path: &str) -> Response<Vec<u8>> {
        match self.dir().and_then(|dir| resolve(&dir, uri_path)) {
            Some(file) => match fs::read(&file) {
                Ok(bytes) => respond(StatusCode::OK, content_type(&file), bytes),
                Err(_) => respond(StatusCode::NOT_FOUND, "text/plain", b"not found".to_vec()),
            },
            None => respond(StatusCode::NOT_FOUND, "text/plain", b"not found".to_vec()),
        }
    }

    fn dir(&self) -> Option<PathBuf> {
        self.current_dir()
    }
}

/// Motion and expression files in a model folder (relative paths with "/"), so
/// files the .model3.json doesn't list (common with VTube Studio models) can be
/// offered too. Looks three folders deep and stops at 500 files.
pub fn list_model_files(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, depth: u32, out: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            if out.len() >= 500 {
                return;
            }
            let path = e.path();
            if path.is_dir() {
                if depth < 3 {
                    walk(root, &path, depth + 1, out);
                }
                continue;
            }
            let name = e.file_name().to_string_lossy().to_lowercase();
            if name.ends_with(".motion3.json") || name.ends_with(".exp3.json") {
                if let Ok(rel) = path.strip_prefix(root) {
                    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
                    out.push(parts.join("/"));
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, 0, &mut out);
    out
}

/// Maps a request path onto a file inside `dir`, refusing anything that escapes it.
pub fn resolve(dir: &Path, uri_path: &str) -> Option<PathBuf> {
    let trimmed = uri_path.trim_start_matches('/');
    // Drop the generation segment.
    let (_, rest) = trimmed.split_once('/')?;
    let decoded = percent_decode_str(rest).decode_utf8().ok()?;
    let relative = Path::new(decoded.as_ref());
    if relative.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    let root = dir.canonicalize().ok()?;
    let file = root.join(relative).canonicalize().ok()?;
    (file.starts_with(&root) && file.is_file()).then_some(file)
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("wav") => "audio/wav",
        Some("mp3") => "audio/mpeg",
        Some("ogg") => "audio/ogg",
        _ => "application/octet-stream",
    }
}

fn respond(status: StatusCode, mime: &str, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, mime)
        // The page (tauri.localhost) and model.localhost are different origins.
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        // The page is cross-origin isolated (for multi-threaded voice).
        .header("Cross-Origin-Resource-Policy", "cross-origin")
        .body(body)
        .expect("valid response")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_model() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("da-model-test-{}", std::process::id()));
        fs::create_dir_all(dir.join("motions")).unwrap();
        fs::write(dir.join("m.model3.json"), "{}").unwrap();
        fs::write(dir.join("motions/00_Wait_01.motion3.json"), "{}").unwrap();
        fs::write(dir.join(".physics3.json"), "{}").unwrap();
        fs::write(dir.parent().unwrap().join("secret.txt"), "x").unwrap();
        dir
    }

    #[test]
    fn lists_motion_and_expression_files() {
        let dir = temp_model();
        fs::write(dir.join("Love eye.exp3.json"), "{}").unwrap();
        let files = list_model_files(&dir);
        assert!(files.contains(&"motions/00_Wait_01.motion3.json".to_string()), "{files:?}");
        assert!(files.contains(&"Love eye.exp3.json".to_string()), "{files:?}");
        assert!(!files.iter().any(|f| f.ends_with("model3.json") && !f.ends_with("motion3.json")));
    }

    #[test]
    fn serves_files_inside_the_model_folder() {
        let dir = temp_model();
        assert!(resolve(&dir, "/3/m.model3.json").is_some());
        assert!(resolve(&dir, "/3/motions/00_Wait_01.motion3.json").is_some());
        assert!(resolve(&dir, "/3/motions%2F00_Wait_01.motion3.json").is_some());
        assert!(resolve(&dir, "/3/.physics3.json").is_some());
    }

    #[test]
    fn refuses_paths_outside_the_model_folder() {
        let dir = temp_model();
        assert!(resolve(&dir, "/3/../secret.txt").is_none());
        assert!(resolve(&dir, "/3/%2E%2E/secret.txt").is_none());
        assert!(resolve(&dir, "/3/motions/../../secret.txt").is_none());
        assert!(resolve(&dir, "/3//etc/passwd").is_none());
        assert!(resolve(&dir, "/3/missing.json").is_none());
        assert!(resolve(&dir, "/no-generation").is_none());
    }
}

/// A display name from a model folder name: "1014100aqua" -> "Aqua", "Nicole" -> "Nicole",
/// "mori_suit" -> "Mori Suit", "weixi_2" -> "Weixi", "l2d01.u" -> "L2d01".
/// A model's own personality, read fresh on every message so edits apply right away.
/// Looks for `personas\<model folder name>.md` (or `.txt`) in the app's config folder,
/// then `persona.md` (or `.txt`) inside the model's folder.
pub fn find_persona(config_dir: &Path, model_dir: &Path) -> Option<String> {
    let folder = model_dir.file_name()?.to_string_lossy().into_owned();
    let personas = config_dir.join("personas");
    let candidates = [
        personas.join(format!("{folder}.md")),
        personas.join(format!("{folder}.txt")),
        model_dir.join("persona.md"),
        model_dir.join("persona.txt"),
    ];
    candidates.iter().find_map(|p| {
        let text = fs::read_to_string(p).ok()?;
        // Notepad may save a byte-order mark.
        let text = strip_front_matter(text.trim_start_matches('\u{feff}')).trim();
        (!text.is_empty()).then(|| text.to_string())
    })
}

/// Drops a `---` front matter block at the top (settings such as `voice:`, read by
/// `voices::folder_voice`), so only the character description reaches Claude.
fn strip_front_matter(text: &str) -> &str {
    let mut lines = text.split_inclusive('\n');
    if lines.next().map(str::trim) != Some("---") {
        return text;
    }
    let mut offset = text.split_inclusive('\n').next().map_or(0, str::len);
    for line in lines {
        offset += line.len();
        if line.trim() == "---" {
            return &text[offset..];
        }
    }
    // No closing line: not front matter after all.
    text
}

pub fn name_from_folder(folder: &str) -> Option<String> {
    let base = folder.strip_suffix(".u").unwrap_or(folder);
    let base = base.trim_start_matches(|c: char| c.is_ascii_digit());
    let words: Vec<String> = base
        .split(|c: char| c == '_' || c == '-' || c == ' ' || c == '.')
        .filter(|w| !w.is_empty() && !w.chars().all(|c| c.is_ascii_digit()))
        .map(|w| {
            let mut cs = w.chars();
            cs.next().map(|f| f.to_uppercase().chain(cs).collect()).unwrap_or_default()
        })
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

#[cfg(test)]
mod persona_tests {
    use super::find_persona;
    use std::fs;

    #[test]
    fn finds_persona_files_in_order() {
        let root = std::env::temp_dir().join(format!("persona-test-{}", std::process::id()));
        let config = root.join("config");
        let model = root.join("models").join("Nicole");
        fs::create_dir_all(config.join("personas")).unwrap();
        fs::create_dir_all(&model).unwrap();
        assert_eq!(find_persona(&config, &model), None);

        fs::write(model.join("persona.md"), "\u{feff}  From the model folder.\n").unwrap();
        assert_eq!(find_persona(&config, &model).as_deref(), Some("From the model folder."));

        fs::write(model.join("persona.md"), "---\r\nvoice: af_bella\r\n---\r\nFrom the model folder.\r\n").unwrap();
        assert_eq!(find_persona(&config, &model).as_deref(), Some("From the model folder."));
        fs::write(model.join("persona.md"), "---\nnot closed\n").unwrap();
        assert_eq!(find_persona(&config, &model).as_deref(), Some("---\nnot closed"));
        fs::write(model.join("persona.md"), "From the model folder.").unwrap();

        fs::write(config.join("personas").join("Nicole.md"), "From the config folder.").unwrap();
        assert_eq!(find_persona(&config, &model).as_deref(), Some("From the config folder."));

        fs::write(config.join("personas").join("Nicole.md"), "   ").unwrap();
        assert_eq!(find_persona(&config, &model).as_deref(), Some("From the model folder."));
        fs::remove_dir_all(&root).unwrap();
    }
}

#[cfg(test)]
mod name_tests {
    use super::name_from_folder;

    #[test]
    fn names_from_folders() {
        assert_eq!(name_from_folder("1014100aqua").as_deref(), Some("Aqua"));
        assert_eq!(name_from_folder("Nicole").as_deref(), Some("Nicole"));
        assert_eq!(name_from_folder("jane").as_deref(), Some("Jane"));
        assert_eq!(name_from_folder("mori_suit").as_deref(), Some("Mori Suit"));
        assert_eq!(name_from_folder("weixi_2").as_deref(), Some("Weixi"));
        assert_eq!(name_from_folder("l2d01.u").as_deref(), Some("L2d01"));
        assert_eq!(name_from_folder("12345"), None);
    }
}

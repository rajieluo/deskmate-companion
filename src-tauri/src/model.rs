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

#[derive(Default, Serialize, Deserialize)]
struct Settings {
    model_path: Option<PathBuf>,
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
}

struct Inner {
    path: Option<PathBuf>,
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
        let path = saved.model_path.filter(|p| p.is_file()).or_else(|| {
            let default = DEFAULT_MODEL.iter().fold(documents?, |p, part| p.join(part));
            default.is_file().then_some(default)
        });
        Self { settings_file, inner: Mutex::new(Inner { path, generation: 1 }) }
    }

    pub fn current(&self) -> Option<ModelInfo> {
        let inner = self.inner.lock().unwrap();
        let path = inner.path.as_ref()?;
        Some(ModelInfo {
            file_name: path.file_name()?.to_string_lossy().into_owned(),
            path: path.display().to_string(),
            generation: inner.generation,
        })
    }

    /// The folder holding the current model's .model3.json.
    pub fn current_dir(&self) -> Option<PathBuf> {
        let inner = self.inner.lock().unwrap();
        inner.path.as_ref()?.parent().map(Path::to_path_buf)
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
            inner.path = Some(path.to_path_buf());
            inner.generation += 1;
        }
        self.save();
        Ok(self.current().expect("model just set"))
    }

    fn save(&self) {
        let settings = Settings { model_path: self.inner.lock().unwrap().path.clone() };
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

//! Local speech-to-text with whisper.cpp (through whisper-rs).
//!
//! The page records the microphone, resamples to 16 kHz mono and sends the raw
//! f32 samples here. The Whisper model is downloaded from Hugging Face the first
//! time it's needed and kept in the app's data folder.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use futures_util::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub const DEFAULT_MODEL: &str = "base.en";
const MODEL_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-";

#[derive(Clone, Serialize)]
pub struct Progress {
    pub percent: f32,
}

#[derive(Default)]
pub struct Stt {
    loaded: Mutex<Option<(String, Arc<WhisperContext>)>>,
    /// Held while a model downloads, so two requests don't fetch it twice.
    download: tokio::sync::Mutex<()>,
}

/// Only plain model names like "base.en" or "small"; they become part of a file name and URL.
pub fn valid_model_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && !name.contains("..")
}

pub fn model_file(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("ggml-{name}.bin"))
}

impl Stt {
    /// Makes sure the model file exists, downloading it with progress events if needed.
    pub async fn ensure_model<R: Runtime>(&self, app: &AppHandle<R>, dir: &Path, name: &str) -> Result<PathBuf, String> {
        if !valid_model_name(name) {
            return Err(format!("\"{name}\" is not a Whisper model name (try base.en or small.en)"));
        }
        let path = model_file(dir, name);
        let _guard = self.download.lock().await;
        if path.is_file() {
            return Ok(path);
        }
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let part = path.with_extension("bin.part");
        let response = reqwest::get(format!("{MODEL_URL}{name}.bin"))
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("Couldn't download the Whisper model: {e}"))?;
        let total = response.content_length().unwrap_or(0) as f32;
        let mut file = fs::File::create(&part).map_err(|e| e.to_string())?;
        let mut stream = response.bytes_stream();
        let (mut done, mut last) = (0f32, -1i32);
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Download interrupted: {e}"))?;
            file.write_all(&chunk).map_err(|e| e.to_string())?;
            done += chunk.len() as f32;
            let percent = if total > 0.0 { done / total * 100.0 } else { 0.0 };
            if percent as i32 != last {
                last = percent as i32;
                let _ = app.emit("stt-progress", Progress { percent });
            }
        }
        drop(file);
        fs::rename(&part, &path).map_err(|e| e.to_string())?;
        Ok(path)
    }

    fn context(&self, name: &str, path: &Path) -> Result<Arc<WhisperContext>, String> {
        let mut loaded = self.loaded.lock().unwrap();
        if let Some((n, ctx)) = loaded.as_ref() {
            if n == name {
                return Ok(ctx.clone());
            }
        }
        let path_str = path.to_str().ok_or("model path is not valid UTF-8")?;
        let ctx = WhisperContext::new_with_params(path_str, WhisperContextParameters::default())
            .map_err(|e| format!("Couldn't load the Whisper model: {e}"))?;
        let ctx = Arc::new(ctx);
        *loaded = Some((name.to_string(), ctx.clone()));
        Ok(ctx)
    }

    /// Transcribes 16 kHz mono f32 audio. Blocking: call from a worker thread.
    pub fn transcribe(&self, name: &str, path: &Path, samples: &[f32], language: &str) -> Result<String, String> {
        let ctx = self.context(name, path)?;
        let mut state = ctx.create_state().map_err(|e| e.to_string())?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(language));
        params.set_n_threads(std::thread::available_parallelism().map(|n| n.get().min(8) as i32).unwrap_or(4));
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_no_context(true);
        state.full(params, samples).map_err(|e| format!("Transcription failed: {e}"))?;
        let text: String = state.as_iter().map(|s| s.to_string()).collect();
        Ok(clean(&text))
    }
}

/// Drops Whisper's non-speech markers like "[BLANK_AUDIO]" or "(music)".
pub fn clean(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in text.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Raw IPC body (little-endian f32 samples) to a sample vector.
pub fn samples_from_bytes(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_names_are_restricted() {
        assert!(valid_model_name("base.en"));
        assert!(valid_model_name("large-v3-turbo"));
        assert!(!valid_model_name("../evil"));
        assert!(!valid_model_name("a/b"));
        assert!(!valid_model_name(""));
    }

    #[test]
    fn removes_non_speech_markers() {
        assert_eq!(clean(" [BLANK_AUDIO] "), "");
        assert_eq!(clean(" Hello there. (laughs) How are you?"), "Hello there. How are you?");
    }

    #[test]
    fn decodes_little_endian_floats() {
        let bytes: Vec<u8> = [0.5f32, -1.0].iter().flat_map(|f| f.to_le_bytes()).collect();
        assert_eq!(samples_from_bytes(&bytes), vec![0.5, -1.0]);
    }
}

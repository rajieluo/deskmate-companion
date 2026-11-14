//! Kokoro text-to-speech run natively with ONNX Runtime.
//!
//! The page still turns text into Kokoro's tokens and picks the voice's style
//! vector (kokoro-js does that in a worker); this module only runs the model,
//! which in the webview's WebAssembly build was slower than real time.
//! onnxruntime.dll is shipped next to the app (from the onnxruntime-node npm
//! package) and loaded at runtime. The model (~90 MB) is downloaded from
//! Hugging Face the first time it's needed and kept in the app's data folder.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use futures_util::StreamExt;
use ort::session::{builder::GraphOptimizationLevel, Session};
use tauri::{AppHandle, Emitter, Manager, Runtime};

const MODEL_URL: &str = "https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/resolve/main/onnx/model_quantized.onnx";
const MODEL_FILE: &str = "kokoro-v1.0-q8.onnx";

#[derive(Default)]
pub struct Tts {
    session: Mutex<Option<Arc<Session>>>,
    /// Held while preparing, so two requests don't download or load twice.
    prepare: tokio::sync::Mutex<()>,
}

/// Threads for one synthesis: about one per physical core, at most 8.
pub fn threads() -> usize {
    let logical = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    (logical / 2).clamp(1, 8)
}

fn runtime_library<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    if let Ok(path) = std::env::var("ORT_DYLIB_PATH") {
        return Some(PathBuf::from(path));
    }
    let name = if cfg!(windows) { "onnxruntime.dll" } else { "libonnxruntime.so" };
    let dir = app.path().resource_dir().ok()?;
    [dir.join(name), dir.join("resources").join(name)].into_iter().find(|p| p.is_file())
}

impl Tts {
    /// Loads ONNX Runtime and the model, downloading the model if needed.
    pub async fn prepare<R: Runtime>(&self, app: &AppHandle<R>) -> Result<(), String> {
        let _guard = self.prepare.lock().await;
        if self.session.lock().unwrap().is_some() {
            return Ok(());
        }
        let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("voice");
        let model = download(app, &dir).await?;
        let library = runtime_library(app).ok_or("onnxruntime.dll is missing from the app folder")?;
        let session = tokio::task::spawn_blocking(move || load(&library, &model))
            .await
            .map_err(|e| e.to_string())??;
        *self.session.lock().unwrap() = Some(Arc::new(session));
        Ok(())
    }

    /// Runs Kokoro: token ids (with the pad tokens at both ends), the voice's
    /// 256-value style vector for this length, and speed. Returns 24 kHz samples.
    /// Blocking: call from a worker thread.
    pub fn synthesize(&self, ids: Vec<i64>, style: Vec<f32>, speed: f32) -> Result<Vec<f32>, String> {
        let session = self.session.lock().unwrap().clone().ok_or("the voice isn't loaded yet")?;
        if style.len() != 256 {
            return Err(format!("style vector has {} values, expected 256", style.len()));
        }
        let n = ids.len();
        let tensor_err = |e: ort::Error| e.to_string();
        let input_ids = ort::value::Tensor::from_array(([1usize, n], ids)).map_err(tensor_err)?;
        let style = ort::value::Tensor::from_array(([1usize, 256], style)).map_err(tensor_err)?;
        let speed = ort::value::Tensor::from_array(([1usize], vec![speed])).map_err(tensor_err)?;
        let inputs = ort::inputs!["input_ids" => input_ids, "style" => style, "speed" => speed].map_err(tensor_err)?;
        let outputs = session.run(inputs).map_err(|e| e.to_string())?;
        let (_, audio) = outputs["waveform"].try_extract_raw_tensor::<f32>().map_err(|e| e.to_string())?;
        Ok(audio.to_vec())
    }
}

fn load(library: &Path, model: &Path) -> Result<Session, String> {
    let started = std::time::Instant::now();
    ort::init_from(library.display().to_string())
        .with_name("kokoro")
        .commit()
        .map_err(|e| format!("Couldn't load ONNX Runtime from {}: {e}", library.display()))?;
    let session = Session::builder()
        .and_then(|b| b.with_optimization_level(GraphOptimizationLevel::Level3))
        .and_then(|b| b.with_intra_threads(threads()))
        .and_then(|b| b.commit_from_file(model))
        .map_err(|e| format!("Couldn't load the voice model: {e}"))?;
    log::info!("voice model loaded natively in {:.1} s ({} threads)", started.elapsed().as_secs_f32(), threads());
    Ok(session)
}

async fn download<R: Runtime>(app: &AppHandle<R>, dir: &Path) -> Result<PathBuf, String> {
    let path = dir.join(MODEL_FILE);
    if path.is_file() {
        return Ok(path);
    }
    log::info!("downloading the voice model to {}", path.display());
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let part = path.with_extension("onnx.part");
    let response = reqwest::get(MODEL_URL)
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Couldn't download the voice model: {e}"))?;
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
            let _ = app.emit("tts-progress", crate::stt::Progress { percent });
        }
    }
    drop(file);
    if total > 0.0 && done != total {
        let _ = fs::remove_file(&part);
        return Err(format!("Download of the voice model stopped early ({done} of {total} bytes)"));
    }
    fs::rename(&part, &path).map_err(|e| e.to_string())?;
    log::info!("voice model downloaded ({done} bytes)");
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs ORT_DYLIB_PATH (e.g. node_modules/onnxruntime-node/.../libonnxruntime.so.1)
    /// and FAKE_KOKORO (a tiny model with Kokoro's inputs and outputs).
    #[test]
    fn runs_a_model_with_kokoros_inputs() {
        let (Ok(lib), Ok(model)) = (std::env::var("ORT_DYLIB_PATH"), std::env::var("FAKE_KOKORO")) else {
            eprintln!("skipped: set ORT_DYLIB_PATH and FAKE_KOKORO");
            return;
        };
        let tts = Tts::default();
        *tts.session.lock().unwrap() = Some(Arc::new(load(Path::new(&lib), Path::new(&model)).unwrap()));
        let audio = tts.synthesize(vec![0, 5, 7, 0], vec![0.5; 256], 2.0).unwrap();
        assert_eq!(audio, vec![128.0, 138.0, 142.0, 128.0]);
        assert!(tts.synthesize(vec![0], vec![0.0; 3], 1.0).is_err());
    }
}

//! Kokoro text-to-speech run natively with ONNX Runtime.
//!
//! The page still turns text into Kokoro's tokens and picks the voice's style
//! vector (kokoro-js does that in a worker); this module only runs the model,
//! which in the webview's WebAssembly build was slower than real time.
//! onnxruntime.dll 1.31 is shipped next to the app (from the onnxruntime-node
//! npm package, installed as the "onnxruntime-desktop" alias so it doesn't clash
//! with the 1.21 copy transformers.js pulls in) and loaded at runtime. 1.21 ran
//! the q8 model ~4x slower. The model (~90 MB) is downloaded from
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

const MODEL_BASE: &str = "https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX/resolve/main/onnx/";
pub const DEFAULT_VARIANT: &str = "q8";

/// Model variants (settings `voice_model`): name -> file in the Hugging Face repo.
/// q8 is smallest, but its integer ops can be slow on the CPU; fp32 is the
/// full-precision model (~325 MB).
const VARIANTS: &[(&str, &str)] = &[
    ("q8", "model_quantized.onnx"),
    ("q8f16", "model_q8f16.onnx"),
    ("fp16", "model_fp16.onnx"),
    ("fp32", "model.onnx"),
];

/// How to run the model: variant name, and whether to try the GPU (DirectML).
#[derive(Clone, Debug)]
pub struct Options {
    pub variant: String,
    pub gpu: bool,
}

fn variant_file(variant: &str) -> Option<&'static str> {
    VARIANTS.iter().find(|(v, _)| *v == variant).map(|(_, f)| *f)
}

pub struct Tts {
    session: Mutex<Option<Arc<Session>>>,
    /// Held while preparing, so two requests don't download or load twice.
    prepare: tokio::sync::Mutex<()>,
    /// At most two sentences at once, first come first served. Kokoro alone keeps
    /// only a few cores busy, so a second run uses the idle ones, while more
    /// would slow down the sentence that's needed next.
    pub runs: tokio::sync::Semaphore,
}

impl Default for Tts {
    fn default() -> Self {
        Tts { session: Mutex::default(), prepare: tokio::sync::Mutex::default(), runs: tokio::sync::Semaphore::new(2) }
    }
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
    pub async fn prepare<R: Runtime>(&self, app: &AppHandle<R>, options: Options) -> Result<(), String> {
        let _guard = self.prepare.lock().await;
        if self.session.lock().unwrap().is_some() {
            return Ok(());
        }
        let dir = crate::data_dir(app)?.join("voice");
        let variant = if variant_file(&options.variant).is_some() { options.variant.as_str() } else { DEFAULT_VARIANT };
        let model = download(app, &dir, variant).await?;
        let library = runtime_library(app).ok_or("onnxruntime.dll is missing from the app folder")?;
        let gpu = options.gpu;
        let session = tokio::task::spawn_blocking(move || load(&library, &model, gpu))
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
        let started = std::time::Instant::now();
        let outputs = session.run(inputs).map_err(|e| e.to_string())?;
        let (_, audio) = outputs["waveform"].try_extract_raw_tensor::<f32>().map_err(|e| e.to_string())?;
        log::info!(
            "voice: {:.1} s of speech ({n} tokens) in {:.2} s",
            audio.len() as f32 / 24_000.0,
            started.elapsed().as_secs_f32()
        );
        Ok(audio.to_vec())
    }
}

fn load(library: &Path, model: &Path, gpu: bool) -> Result<Session, String> {
    let started = std::time::Instant::now();
    ort::init_from(library.display().to_string())
        .with_name("kokoro")
        .commit()
        .map_err(|e| format!("Couldn't load ONNX Runtime from {}: {e}", library.display()))?;
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut builder = Session::builder()
        .and_then(|b| b.with_optimization_level(GraphOptimizationLevel::Level3))
        .and_then(|b| b.with_intra_threads(threads()))
        .map_err(|e| format!("Couldn't set up the voice model: {e}"))?;
    if gpu {
        // DirectML (any DirectX 12 GPU). It needs memory patterns and parallel execution off.
        #[cfg(windows)]
        {
            use ort::execution_providers::DirectMLExecutionProvider;
            builder = builder
                .with_memory_pattern(false)
                .and_then(|b| b.with_parallel_execution(false))
                .and_then(|b| b.with_execution_providers([DirectMLExecutionProvider::default().build()]))
                .map_err(|e| format!("Couldn't set up DirectML: {e}"))?;
        }
        #[cfg(not(windows))]
        log::warn!("the GPU voice needs Windows (DirectML); using the CPU");
    }
    let session = builder.commit_from_file(model).map_err(|e| format!("Couldn't load the voice model: {e}"))?;
    log::info!(
        "voice model {} loaded natively in {:.1} s ({} threads{})",
        model.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
        started.elapsed().as_secs_f32(),
        threads(),
        if gpu { ", DirectML" } else { "" }
    );
    Ok(session)
}

async fn download<R: Runtime>(app: &AppHandle<R>, dir: &Path, variant: &str) -> Result<PathBuf, String> {
    let file = variant_file(variant).unwrap_or("model_quantized.onnx");
    let path = dir.join(format!("kokoro-v1.0-{variant}.onnx"));
    if path.is_file() {
        return Ok(path);
    }
    log::info!("downloading the voice model to {}", path.display());
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let part = path.with_extension("onnx.part");
    let response = reqwest::get(format!("{MODEL_BASE}{file}"))
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Couldn't download the voice model: {e}"))?;
    let total = response.content_length().unwrap_or(0);
    let mut file = fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut stream = response.bytes_stream();
    // Count bytes as integers: an f32 loses whole bytes past 16 MB, which made a
    // complete 92 MB download look short.
    let (mut done, mut last) = (0u64, -1i32);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download interrupted: {e}"))?;
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        done += chunk.len() as u64;
        let percent = if total > 0 { done as f32 / total as f32 * 100.0 } else { 0.0 };
        if percent as i32 != last {
            last = percent as i32;
            let _ = app.emit("tts-progress", crate::stt::Progress { percent });
        }
    }
    drop(file);
    if total > 0 && done != total {
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
        *tts.session.lock().unwrap() = Some(Arc::new(load(Path::new(&lib), Path::new(&model), false).unwrap()));
        let audio = tts.synthesize(vec![0, 5, 7, 0], vec![0.5; 256], 2.0).unwrap();
        assert_eq!(audio, vec![128.0, 138.0, 142.0, 128.0]);
        assert!(tts.synthesize(vec![0], vec![0.0; 3], 1.0).is_err());
    }
}

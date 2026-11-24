//! Kyutai Pocket TTS, a second voice engine that can clone a voice from a short
//! recording (settings `voice_engine: "pocket"`).
//!
//! Pocket TTS is a PyTorch model, so it runs in a Python sidecar
//! (resources/pocket_server.py) started the first time it's needed, with the
//! Python from settings `pocket_python` (a venv with `pip install pocket-tts`).
//! Requests and audio go over the sidecar's stdin/stdout as JSON lines; audio
//! comes back in chunks as it's generated (16-bit PCM, 24 kHz), so playback can
//! start ~0.1 s after a sentence is sent. If the sidecar dies it's started again
//! on the next request; it exits by itself when the app closes its stdin.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use base64::Engine as _;
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin},
    sync::{mpsc, oneshot},
};

/// How long to wait for the model to load when the sidecar starts.
const START_TIMEOUT: Duration = Duration::from_secs(120);
/// How long a sentence may wait for its first chunk (it may be queued behind others).
const FIRST_CHUNK_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest gap between two chunks of one sentence.
const CHUNK_TIMEOUT: Duration = Duration::from_secs(10);

/// Where the sidecar comes from.
#[derive(Clone, Debug)]
pub struct Config {
    pub python: PathBuf,
    pub script: PathBuf,
    /// Where voices encoded from .wav files are cached.
    pub cache_dir: PathBuf,
    /// Dynamic int8 quantization: ~20% faster on the CPU, same first-chunk time.
    pub quantize: bool,
}

/// What the sidecar sends back for one request.
#[derive(Debug)]
pub enum Reply {
    /// 16-bit little-endian mono PCM.
    Chunk(Vec<u8>),
    Done(Value),
    Error(String),
}

/// One line of sidecar output, parsed.
#[derive(Debug, PartialEq)]
pub enum Line {
    Ready { rate: u32, load_ms: u64 },
    Chunk { id: u64, pcm: Vec<u8> },
    Done { id: u64, stats: Value },
    Error { id: Option<u64>, message: String },
}

pub fn parse_line(line: &str) -> Result<Line, String> {
    let v: Value = serde_json::from_str(line).map_err(|e| format!("bad line from the sidecar ({e}): {line:.200}"))?;
    let id = v.get("id").and_then(Value::as_u64);
    match v.get("type").and_then(Value::as_str) {
        Some("ready") => Ok(Line::Ready {
            rate: v.get("rate").and_then(Value::as_u64).unwrap_or(24_000) as u32,
            load_ms: v.get("load_ms").and_then(Value::as_u64).unwrap_or(0),
        }),
        Some("chunk") => {
            let pcm = v.get("pcm").and_then(Value::as_str).ok_or("chunk without pcm")?;
            let pcm = base64::engine::general_purpose::STANDARD.decode(pcm).map_err(|e| e.to_string())?;
            Ok(Line::Chunk { id: id.ok_or("chunk without id")?, pcm })
        }
        Some("done") => Ok(Line::Done { id: id.ok_or("done without id")?, stats: v }),
        Some("error") => Ok(Line::Error {
            id,
            message: v.get("message").and_then(Value::as_str).unwrap_or("unknown error").to_string(),
        }),
        _ => Err(format!("unknown message from the sidecar: {line:.200}")),
    }
}

type Pending = Arc<Mutex<HashMap<u64, mpsc::UnboundedSender<Reply>>>>;

struct Sidecar {
    child: Child,
    stdin: ChildStdin,
    /// Set when the sidecar's stdout closes (it exited or crashed).
    dead: Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Default)]
pub struct Pocket {
    sidecar: tokio::sync::Mutex<Option<Sidecar>>,
    pending: Pending,
    next_id: AtomicU64,
    /// Sample rate the model reported (24 kHz).
    pub rate: std::sync::atomic::AtomicU32,
}

impl Pocket {
    /// Starts the sidecar if it isn't running and waits until the model is loaded.
    async fn ensure<'a>(
        &self,
        guard: &'a mut tokio::sync::MutexGuard<'_, Option<Sidecar>>,
        config: &Config,
    ) -> Result<&'a mut Sidecar, String> {
        if guard.as_ref().is_some_and(|s| s.dead.load(Ordering::SeqCst)) {
            log::warn!("pocket voice: the sidecar had stopped; starting it again");
            if let Some(mut old) = guard.take() {
                let _ = old.child.start_kill();
            }
        }
        if guard.is_none() {
            **guard = Some(self.spawn(config).await?);
        }
        Ok(guard.as_mut().unwrap())
    }

    async fn spawn(&self, config: &Config) -> Result<Sidecar, String> {
        if !config.python.is_file() {
            return Err(format!(
                "Python for Pocket TTS not found at {} (set pocket_python in settings.json to a venv with pocket-tts)",
                config.python.display()
            ));
        }
        if !config.script.is_file() {
            return Err(format!("{} is missing from the app folder", config.script.display()));
        }
        let started = std::time::Instant::now();
        let mut cmd = tokio::process::Command::new(&config.python);
        // -I: don't pick up modules from the current folder or user site-packages.
        cmd.arg("-I").arg(&config.script).arg("--cache-dir").arg(&config.cache_dir);
        if config.quantize {
            cmd.arg("--quantize");
        }
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        cmd.env("PYTHONUNBUFFERED", "1").env("PYTHONIOENCODING", "utf-8");
        #[cfg(windows)]
        {
            #[allow(unused_imports)]
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = cmd.spawn().map_err(|e| format!("Couldn't start Pocket TTS with {}: {e}", config.python.display()))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let stderr = child.stderr.take().ok_or("no stderr")?;
        log::info!("pocket voice: started the sidecar (pid {:?}, {})", child.id(), config.python.display());

        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim();
                if !line.is_empty() && !line.contains("UserWarning") && !line.starts_with("qweight =") {
                    log::info!("[pocket] {line}");
                }
            }
        });

        let dead = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (ready_tx, ready_rx) = oneshot::channel::<(u32, u64)>();
        let pending = self.pending.clone();
        let dead_flag = dead.clone();
        tauri::async_runtime::spawn(async move {
            let mut ready = Some(ready_tx);
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                match parse_line(&line) {
                    Ok(Line::Ready { rate, load_ms }) => {
                        if let Some(tx) = ready.take() {
                            let _ = tx.send((rate, load_ms));
                        }
                    }
                    Ok(Line::Chunk { id, pcm }) => send(&pending, id, Reply::Chunk(pcm), false),
                    Ok(Line::Done { id, stats }) => send(&pending, id, Reply::Done(stats), true),
                    Ok(Line::Error { id: Some(id), message }) => send(&pending, id, Reply::Error(message), true),
                    Ok(Line::Error { id: None, message }) => log::warn!("pocket voice: {message}"),
                    Err(e) => log::warn!("pocket voice: {e}"),
                }
            }
            dead_flag.store(true, Ordering::SeqCst);
            // Dropping the senders ends every request that was waiting.
            let n = pending.lock().unwrap().drain().count();
            log::warn!("pocket voice: the sidecar exited ({n} request(s) dropped)");
        });

        match tokio::time::timeout(START_TIMEOUT, ready_rx).await {
            Ok(Ok((rate, load_ms))) => {
                self.rate.store(rate, Ordering::SeqCst);
                log::info!(
                    "pocket voice: model loaded in {:.1} s ({:.1} s since start{})",
                    load_ms as f32 / 1000.0,
                    started.elapsed().as_secs_f32(),
                    if config.quantize { ", int8" } else { "" }
                );
                Ok(Sidecar { child, stdin, dead })
            }
            Ok(Err(_)) => {
                let _ = child.start_kill();
                Err("Pocket TTS exited while loading (is pocket-tts installed in that Python? see the log)".into())
            }
            Err(_) => {
                let _ = child.start_kill();
                Err("Pocket TTS took too long to load".into())
            }
        }
    }

    /// Sends one request and returns the channel its replies arrive on.
    async fn request(&self, config: &Config, mut msg: Value) -> Result<(u64, mpsc::UnboundedReceiver<Reply>), String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        msg["id"] = id.into();
        let (tx, rx) = mpsc::unbounded_channel();
        let mut guard = self.sidecar.lock().await;
        let sidecar = self.ensure(&mut guard, config).await?;
        self.pending.lock().unwrap().insert(id, tx);
        let line = format!("{msg}\n");
        if let Err(e) = sidecar.stdin.write_all(line.as_bytes()).await {
            self.pending.lock().unwrap().remove(&id);
            sidecar.dead.store(true, Ordering::SeqCst);
            return Err(format!("Couldn't talk to Pocket TTS: {e}"));
        }
        let _ = sidecar.stdin.flush().await;
        Ok((id, rx))
    }

    /// Starts the sidecar and encodes the voice, so the first sentence is quick.
    pub async fn prepare(&self, config: &Config, voice: &str) -> Result<(), String> {
        let (id, mut rx) = self.request(config, serde_json::json!({ "cmd": "voice", "voice": voice })).await?;
        let reply = tokio::time::timeout(START_TIMEOUT, rx.recv()).await;
        self.pending.lock().unwrap().remove(&id);
        match reply {
            Ok(Some(Reply::Done(_))) => Ok(()),
            Ok(Some(Reply::Error(e))) => Err(e),
            Ok(_) => Err("Pocket TTS stopped while loading the voice".into()),
            Err(_) => Err("Pocket TTS took too long to load the voice".into()),
        }
    }

    /// Speaks one sentence, handing each chunk of 16-bit PCM to `on_chunk` as it
    /// arrives. Returns the sidecar's timing stats.
    pub async fn speak(
        &self,
        config: &Config,
        voice: &str,
        text: &str,
        mut on_chunk: impl FnMut(Vec<u8>),
    ) -> Result<Value, String> {
        let (id, mut rx) =
            self.request(config, serde_json::json!({ "cmd": "speak", "voice": voice, "text": text })).await?;
        let mut timeout = FIRST_CHUNK_TIMEOUT;
        let result = loop {
            match tokio::time::timeout(timeout, rx.recv()).await {
                Ok(Some(Reply::Chunk(pcm))) => {
                    timeout = CHUNK_TIMEOUT;
                    on_chunk(pcm);
                }
                Ok(Some(Reply::Done(stats))) => break Ok(stats),
                Ok(Some(Reply::Error(e))) => break Err(e),
                Ok(None) => break Err("Pocket TTS stopped".into()),
                Err(_) => {
                    // Drop it on the sidecar's side too, so it doesn't hold up the next one.
                    self.cancel_upto(id).await;
                    break Err("Pocket TTS timed out".into());
                }
            }
        };
        self.pending.lock().unwrap().remove(&id);
        result
    }

    /// Drops every request sent so far (stops the one being generated).
    pub async fn cancel(&self) {
        let upto = self.next_id.load(Ordering::SeqCst);
        self.cancel_upto(upto).await;
    }

    async fn cancel_upto(&self, upto: u64) {
        let mut guard = self.sidecar.lock().await;
        if let Some(sidecar) = guard.as_mut().filter(|s| !s.dead.load(Ordering::SeqCst)) {
            let line = format!("{}\n", serde_json::json!({ "cmd": "cancel", "upto": upto }));
            let _ = sidecar.stdin.write_all(line.as_bytes()).await;
            let _ = sidecar.stdin.flush().await;
        }
    }

    /// Stops the sidecar (app exit).
    pub fn kill(&self) {
        if let Ok(mut guard) = self.sidecar.try_lock() {
            if let Some(mut s) = guard.take() {
                let _ = s.child.start_kill();
            }
        }
    }
}

fn send(pending: &Pending, id: u64, reply: Reply, last: bool) {
    let mut map = pending.lock().unwrap();
    if let Some(tx) = map.get(&id) {
        let _ = tx.send(reply);
    }
    if last {
        map.remove(&id);
    }
}

/// The sidecar script shipped with the app.
pub fn script_path(resource_dir: &Path) -> PathBuf {
    let direct = resource_dir.join("pocket_server.py");
    if direct.is_file() {
        return direct;
    }
    resource_dir.join("resources").join("pocket_server.py")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sidecar_lines() {
        assert_eq!(
            parse_line(r#"{"type": "ready", "rate": 24000, "load_ms": 900}"#).unwrap(),
            Line::Ready { rate: 24000, load_ms: 900 }
        );
        // "AQACAA==" is the bytes 01 00 02 00: two int16 samples, 1 and 2.
        assert_eq!(
            parse_line(r#"{"id": 3, "type": "chunk", "pcm": "AQACAA=="}"#).unwrap(),
            Line::Chunk { id: 3, pcm: vec![1, 0, 2, 0] }
        );
        assert!(matches!(parse_line(r#"{"id": 3, "type": "done", "ms": 5}"#).unwrap(), Line::Done { id: 3, .. }));
        assert_eq!(
            parse_line(r#"{"id": 4, "type": "error", "message": "nope"}"#).unwrap(),
            Line::Error { id: Some(4), message: "nope".into() }
        );
        assert!(parse_line("not json").is_err());
        assert!(parse_line(r#"{"type": "chunk"}"#).is_err());
    }

    #[test]
    fn finds_the_script_in_either_resource_layout() {
        let dir = std::env::temp_dir().join(format!("pocket-script-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("resources")).unwrap();
        assert_eq!(script_path(&dir), dir.join("resources").join("pocket_server.py"));
        std::fs::write(dir.join("pocket_server.py"), "").unwrap();
        assert_eq!(script_path(&dir), dir.join("pocket_server.py"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Runs the real sidecar when POCKET_PYTHON points at a Python with pocket-tts.
    #[test]
    fn speaks_through_the_sidecar() {
        let Ok(python) = std::env::var("POCKET_PYTHON") else {
            eprintln!("skipped: set POCKET_PYTHON");
            return;
        };
        let config = Config {
            python: python.into(),
            script: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/pocket_server.py"),
            cache_dir: std::env::temp_dir().join("pocket-test-cache"),
            quantize: true,
        };
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let pocket = Pocket::default();
            pocket.prepare(&config, "alba").await.unwrap();
            let mut bytes = 0;
            let stats = pocket.speak(&config, "alba", "Hello there.", |pcm| bytes += pcm.len()).await.unwrap();
            assert!(bytes > 24_000, "only {bytes} bytes of audio");
            assert!(stats.get("first_ms").is_some());
            assert!(pocket.speak(&config, "no-such-voice.wav", "Hi.", |_| {}).await.is_err());
            pocket.kill();
        });
    }
}

// Runs Kokoro-82M text-to-speech off the main thread so the avatar keeps animating.
// The model (~90 MB) is downloaded from Hugging Face on first use and cached by the webview.
import { KokoroTTS } from "kokoro-js";

type Request = { type: "load" } | { type: "speak"; id: number; text: string; voice: string; speed: number };

const MODEL = "onnx-community/Kokoro-82M-v1.0-ONNX";
let tts: Promise<KokoroTTS> | null = null;

function post(msg: unknown, transfer: Transferable[] = []) {
  (self as unknown as Worker).postMessage(msg, transfer);
}

async function load(): Promise<KokoroTTS> {
  const progress_callback = (p: { status: string; file?: string; progress?: number }) => {
    if (p.status === "progress" && p.file?.endsWith(".onnx")) post({ type: "progress", progress: p.progress ?? 0 });
  };
  // WebGPU is much faster when the GPU supports it; otherwise fall back to WASM on the CPU.
  if ("gpu" in navigator) {
    try {
      const t = await KokoroTTS.from_pretrained(MODEL, { dtype: "fp32", device: "webgpu", progress_callback });
      post({ type: "ready", device: "webgpu" });
      return t;
    } catch (e) {
      console.warn("[tts] WebGPU unavailable, using CPU", e);
    }
  }
  const t = await KokoroTTS.from_pretrained(MODEL, { dtype: "q8", device: "wasm", progress_callback });
  post({ type: "ready", device: "wasm" });
  return t;
}

self.onmessage = async (e: MessageEvent<Request>) => {
  const msg = e.data;
  try {
    tts ??= load();
    const engine = await tts;
    if (msg.type !== "speak") return;
    const out = await engine.generate(msg.text, { voice: msg.voice as never, speed: msg.speed });
    const audio = out.audio as Float32Array;
    post({ type: "audio", id: msg.id, audio, rate: out.sampling_rate }, [audio.buffer]);
  } catch (err) {
    tts = null;
    post({ type: "error", id: msg.type === "speak" ? msg.id : -1, message: String((err as Error)?.message ?? err) });
  }
};

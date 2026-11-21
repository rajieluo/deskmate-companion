// Kokoro-82M text-to-speech, off the main thread so the avatar keeps animating.
//
// In the desktop app the model itself runs natively (ONNX Runtime in Rust, see
// src-tauri/src/tts.rs): this worker only turns text into Kokoro's tokens and
// picks the voice's style vector, then asks the page to run the model. The
// WebAssembly build of the model is the fallback (browser dev mode, or if the
// native voice can't load); it's slower than real time on many CPUs.
//
// "kokoro-js" is aliased to its non-bundled build in vite.config.ts, so it
// shares @huggingface/transformers (for the tokenizer and thread settings).
import { AutoTokenizer, env } from "@huggingface/transformers";
import { KokoroTTS } from "kokoro-js";
import { looksGarbled } from "./garbled";

type Device = "cpu" | "gpu" | "native";
type Request =
  | { type: "load"; device?: Device }
  | { type: "speak"; id: number; text: string; voice: string; speed: number; device?: Device }
  | { type: "native-result"; id: number; audio?: Float32Array; error?: string };

const MODEL = "onnx-community/Kokoro-82M-v1.0-ONNX";
let tts: Promise<KokoroTTS> | null = null;
/** Set once WebGPU has produced garbage, so the rest of the session stays on the CPU. */
let gpuBroken = false;

const cores = navigator.hardwareConcurrency || 4;
// onnxruntime picks at most 4 threads by itself; use about one per physical core.
const onnxEnv = (env.backends.onnx as { wasm?: { numThreads?: number } }).wasm;
if (onnxEnv) onnxEnv.numThreads = self.crossOriginIsolated ? Math.max(1, Math.min(8, Math.floor(cores / 2))) : 1;

function post(msg: unknown, transfer: Transferable[] = []) {
  (self as unknown as Worker).postMessage(msg, transfer);
}

// Model runs handed to the page (which calls Rust), by id.
let nextRun = 0;
const runs = new Map<number, { resolve: (a: Float32Array) => void; reject: (e: Error) => void }>();

type KokoroInputs = { input_ids: { data: BigInt64Array | number[] }; style: { data: Float32Array }; speed: { data: Float32Array | number[] } };

/** Stands in for the ONNX model: kokoro-js calls it with its tensors. */
async function nativeModel(inputs: KokoroInputs) {
  const id = ++nextRun;
  const ids = Array.from(inputs.input_ids.data as ArrayLike<bigint | number>, Number);
  const audio = await new Promise<Float32Array>((resolve, reject) => {
    runs.set(id, { resolve, reject });
    post({ type: "native", id, ids, style: inputs.style.data, speed: Number(inputs.speed.data[0]) });
  });
  return { waveform: { data: audio } };
}

async function load(device: Device): Promise<KokoroTTS> {
  const progress_callback = (p: { status: string; file?: string; progress?: number }) => {
    if (p.status === "progress" && p.file?.endsWith(".onnx")) post({ type: "progress", progress: p.progress ?? 0 });
  };
  const ready = (name: string, threads: number) => post({ type: "ready", device: name, isolated: self.crossOriginIsolated, cores, threads });
  if (device === "native") {
    const tokenizer = await AutoTokenizer.from_pretrained(MODEL);
    ready("native", 0);
    return new KokoroTTS(nativeModel as never, tokenizer);
  }
  // WebGPU is faster, but on some graphics drivers it produces garbled speech, so
  // the CPU (WASM) is the default and WebGPU is opt-in.
  if (device === "gpu" && "gpu" in navigator) {
    try {
      const t = await KokoroTTS.from_pretrained(MODEL, { dtype: "fp32", device: "webgpu", progress_callback });
      ready("webgpu", 0);
      return t;
    } catch (e) {
      console.warn("[tts] WebGPU unavailable, using CPU", e);
    }
  }
  const t = await KokoroTTS.from_pretrained(MODEL, { dtype: "q8", device: "wasm", progress_callback });
  ready("wasm", onnxEnv?.numThreads ?? 1);
  return t;
}

self.onmessage = async (e: MessageEvent<Request>) => {
  const msg = e.data;
  if (msg.type === "native-result") {
    const run = runs.get(msg.id);
    runs.delete(msg.id);
    if (msg.audio) run?.resolve(msg.audio);
    else run?.reject(new Error(msg.error ?? "native voice failed"));
    return;
  }
  try {
    tts ??= load(gpuBroken && msg.device === "gpu" ? "cpu" : (msg.device ?? "cpu")).catch((err) => {
      tts = null;
      throw err;
    });
    let engine = await tts;
    if (msg.type !== "speak") return;
    const started = performance.now();
    let out = await engine.generate(msg.text, { voice: msg.voice as never, speed: msg.speed });
    if (msg.device === "gpu" && looksGarbled(out.audio as Float32Array) && !gpuBroken) {
      // Seen with WebGPU on some GPUs (e.g. an RX 7900 XT): switch to the CPU and redo this sentence.
      gpuBroken = true;
      post({ type: "garbled" });
      tts = load("cpu");
      engine = await tts;
      out = await engine.generate(msg.text, { voice: msg.voice as never, speed: msg.speed });
    }
    const audio = out.audio as Float32Array;
    post({ type: "audio", id: msg.id, audio, rate: out.sampling_rate, ms: performance.now() - started }, [audio.buffer]);
  } catch (err) {
    post({ type: "error", id: msg.type === "speak" ? msg.id : -1, message: String((err as Error)?.message ?? err) });
  }
};

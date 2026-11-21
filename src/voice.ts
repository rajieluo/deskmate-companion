import { inTauri, log, onTtsProgress, ttsPrepare, ttsSynthesize } from "./host";

// Speaks reply text sentence by sentence and exposes a mouth-open level for lip sync.
//
// Every sentence is sent for synthesis as soon as it arrives, and each clip is
// scheduled to start the moment the previous one ends, so there are no gaps
// as long as synthesis keeps ahead of playback.

export interface SpeechEngine {
  /** Starts loading the model early (optional). */
  warmUp?(): void;
  synthesize(text: string): Promise<{ audio: Float32Array; rate: number }>;
}

/**
 * Kokoro: text processing in a Web Worker, and the model itself natively in the
 * desktop app (falling back to WebAssembly in the worker).
 */
export class KokoroEngine implements SpeechEngine {
  private worker: Worker | null = null;
  private nextId = 0;
  private pending = new Map<number, { resolve: (v: { audio: Float32Array; rate: number }) => void; reject: (e: Error) => void }>();
  onStatus: (status: string) => void = () => {};
  private timed = 0;

  /** "cpu" (native in the app, else WASM) or "gpu" (WebGPU). Read when the model first loads. */
  device: "cpu" | "gpu" = "cpu";
  private mode: Promise<"cpu" | "gpu" | "native"> | null = null;

  /**
   * Pitch factor. Clips are played back this much faster, which raises the pitch,
   * and made this much slower to begin with, so the pace stays at `speed`.
   */
  pitch = 1;

  constructor(
    public voice = "af_heart",
    public speed = 1,
  ) {}

  private ensure(): Worker {
    if (this.worker) return this.worker;
    const w = new Worker(new URL("./tts.worker.ts", import.meta.url), { type: "module" });
    w.onmessage = (e) => {
      const m = e.data;
      if (m.type === "progress") this.onStatus(`Downloading voice… ${Math.round(m.progress)}%`);
      else if (m.type === "garbled") log("warn", "[voice] the GPU produced garbled audio; switched to the CPU");
      else if (m.type === "native") {
        // The worker asks for one model run; do it in Rust and hand the audio back.
        ttsSynthesize(m.ids, m.style, m.speed).then(
          (audio) => w.postMessage({ type: "native-result", id: m.id, audio }, [audio.buffer]),
          (err) => w.postMessage({ type: "native-result", id: m.id, error: String(err) }),
        );
        return;
      }
      else if (m.type === "ready") {
        // Without cross-origin isolation onnxruntime runs on a single thread, which is
        // slower than real time on many CPUs and leaves pauses between sentences.
        log("info", `[voice] Kokoro running on ${m.device}${m.device === "wasm" ? `; ${m.threads} threads (${m.cores} cores, isolated ${m.isolated})` : ""}`);
        this.onStatus("");
      }
      else if (m.type === "audio") {
        if (this.timed++ < 3) {
          const secs = m.audio.length / m.rate;
          log("info", `[voice] ${secs.toFixed(1)} s of speech took ${(m.ms / 1000).toFixed(1)} s to make`);
        }
        this.pending.get(m.id)?.resolve({ audio: m.audio, rate: m.rate });
      }
      else if (m.type === "error") {
        if (m.id === -1) this.onStatus(`Voice failed to load: ${m.message}`);
        log("error", `[voice] ${m.message}`);
        this.pending.get(m.id)?.reject(new Error(m.message));
      }
      if ((m.type === "audio" || m.type === "error") && m.id !== undefined) this.pending.delete(m.id);
    };
    this.worker = w;
    return w;
  }

  /** Native in the desktop app unless WebGPU was asked for; WASM if the native voice won't load. */
  private chooseMode() {
    return (this.mode ??= (async () => {
      if (!inTauri) return this.device;
      onTtsProgress((p) => this.onStatus(`Downloading voice… ${Math.floor(p)}%`));
      try {
        const threads = await ttsPrepare();
        log("info", `[voice] native voice ready (${threads} threads)`);
        return "native" as const;
      } catch (e) {
        log("warn", `[voice] native voice unavailable, using the slower built-in one: ${e}`);
        return this.device;
      } finally {
        this.onStatus("");
      }
    })());
  }

  warmUp() {
    void this.chooseMode().then((device) => this.ensure().postMessage({ type: "load", device }));
  }

  async synthesize(text: string) {
    const device = await this.chooseMode();
    const id = ++this.nextId;
    const pitch = this.pitch;
    const speed = Math.min(2, Math.max(0.5, this.speed / pitch));
    return new Promise<{ audio: Float32Array; rate: number }>((resolve, reject) => {
      // A higher sample rate makes the clip play faster and higher.
      this.pending.set(id, { resolve: (clip) => resolve({ ...clip, rate: clip.rate * pitch }), reject });
      this.ensure().postMessage({ type: "speak", id, text, voice: this.voice, speed, device });
    });
  }
}

/** A stand-in that "speaks" with syllable-shaped beeps; used in browser dev mode and tests. */
export class BeepEngine implements SpeechEngine {
  async synthesize(text: string) {
    const rate = 24000;
    const syllables = Math.max(1, Math.round(text.length / 4));
    const per = 0.16;
    const audio = new Float32Array(Math.round(syllables * per * rate));
    for (let i = 0; i < audio.length; i++) {
      const t = i / rate;
      const envelope = Math.sin(Math.PI * ((t % per) / per)) ** 2;
      audio[i] = 0.25 * envelope * Math.sin(2 * Math.PI * 220 * t);
    }
    return { audio, rate };
  }
}

export class Voice {
  enabled = true;
  private ctx: AudioContext | null = null;
  private analyser: AnalyserNode | null = null;
  private samples = new Float32Array(1024);
  private queue: Promise<{ audio: Float32Array; rate: number } | null>[] = [];
  private playing = false;
  private sources = new Set<AudioBufferSourceNode>();
  /** AudioContext time at which the last scheduled clip ends. */
  private nextStart = 0;
  /** Wakes the drain loop when a sentence is queued or speech stops. */
  private wake: (() => void) | null = null;
  private generation = 0;
  private smoothed = 0;

  constructor(public engine: SpeechEngine) {}

  private audio(): AudioContext {
    if (!this.ctx) {
      this.ctx = new AudioContext();
      this.analyser = this.ctx.createAnalyser();
      this.analyser.fftSize = 1024;
      this.analyser.connect(this.ctx.destination);
    }
    if (this.ctx.state === "suspended") void this.ctx.resume();
    return this.ctx;
  }

  get speaking() {
    return this.playing;
  }

  /** Queues one sentence. Synthesis starts right away; playback waits its turn. */
  say(text: string) {
    if (!this.enabled || !text) return;
    this.audio();
    const job = this.engine.synthesize(text).catch((e) => {
      console.warn("[voice]", e);
      return null;
    });
    this.queue.push(job);
    this.wake?.();
    if (!this.playing) void this.drain(this.generation);
  }

  /** Stops speaking and drops anything queued. */
  stop() {
    this.generation++;
    this.queue = [];
    for (const src of this.sources) {
      try {
        src.stop();
      } catch {
        /* already stopped */
      }
    }
    this.sources.clear();
    this.nextStart = 0;
    this.playing = false;
    this.wake?.();
  }

  private async drain(gen: number) {
    this.playing = true;
    while (gen === this.generation) {
      if (this.queue.length) {
        const clip = await this.queue.shift()!;
        if (clip && gen === this.generation) this.schedule(clip);
        continue;
      }
      if (!this.sources.size) break;
      // Wait for the last clip to end, or for another sentence to arrive.
      await new Promise<void>((resolve) => {
        this.wake = resolve;
        for (const src of this.sources) src.addEventListener("ended", () => resolve(), { once: true });
      });
      this.wake = null;
    }
    if (gen === this.generation) this.playing = false;
  }

  /** Starts a clip right after the one before it (or now, if nothing is playing). */
  private schedule({ audio, rate }: { audio: Float32Array; rate: number }) {
    const ctx = this.audio();
    const buffer = ctx.createBuffer(1, audio.length, rate);
    buffer.copyToChannel(audio as Float32Array<ArrayBuffer>, 0);
    const src = ctx.createBufferSource();
    src.buffer = buffer;
    src.connect(this.analyser!);
    const start = Math.max(ctx.currentTime + 0.01, this.nextStart);
    this.nextStart = start + buffer.duration;
    this.sources.add(src);
    src.onended = () => this.sources.delete(src);
    src.start(start);
  }

  /** Mouth openness 0..1 from the loudness of what's playing right now. */
  level(): number {
    if (!this.analyser || !this.playing) {
      this.smoothed *= 0.6;
      return this.smoothed < 0.01 ? 0 : this.smoothed;
    }
    this.analyser.getFloatTimeDomainData(this.samples);
    let sum = 0;
    for (const s of this.samples) sum += s * s;
    const rms = Math.sqrt(sum / this.samples.length);
    // Speech RMS sits around 0.02-0.2; map that onto 0..1 with a little gate.
    const target = Math.min(1, Math.max(0, (rms - 0.01) * 7));
    // Open fast, close a bit slower, like a real mouth.
    const k = target > this.smoothed ? 0.6 : 0.3;
    this.smoothed += (target - this.smoothed) * k;
    return this.smoothed;
  }
}

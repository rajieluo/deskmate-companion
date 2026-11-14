import { log } from "./host";

// Speaks reply text sentence by sentence and exposes a mouth-open level for lip sync.
//
// Sentences are synthesized one ahead of playback, so the next one is usually
// ready by the time the current one finishes.

export interface SpeechEngine {
  /** Starts loading the model early (optional). */
  warmUp?(): void;
  synthesize(text: string): Promise<{ audio: Float32Array; rate: number }>;
}

/** Kokoro running in a Web Worker. */
export class KokoroEngine implements SpeechEngine {
  private worker: Worker | null = null;
  private nextId = 0;
  private pending = new Map<number, { resolve: (v: { audio: Float32Array; rate: number }) => void; reject: (e: Error) => void }>();
  onStatus: (status: string) => void = () => {};

  /** "cpu" (WASM) or "gpu" (WebGPU). Read when the model first loads. */
  device: "cpu" | "gpu" = "cpu";

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
      else if (m.type === "ready") {
        log("info", `[voice] Kokoro running on ${m.device}`);
        this.onStatus("");
      }
      else if (m.type === "audio") this.pending.get(m.id)?.resolve({ audio: m.audio, rate: m.rate });
      else if (m.type === "error") {
        if (m.id === -1) this.onStatus(`Voice failed to load: ${m.message}`);
        log("error", `[voice] ${m.message}`);
        this.pending.get(m.id)?.reject(new Error(m.message));
      }
      if (m.id !== undefined) this.pending.delete(m.id);
    };
    this.worker = w;
    return w;
  }

  warmUp() {
    this.ensure().postMessage({ type: "load", device: this.device });
  }

  synthesize(text: string) {
    const id = ++this.nextId;
    return new Promise<{ audio: Float32Array; rate: number }>((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      this.ensure().postMessage({ type: "speak", id, text, voice: this.voice, speed: this.speed, device: this.device });
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
  private source: AudioBufferSourceNode | null = null;
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
    if (!this.playing) void this.drain(this.generation);
  }

  /** Stops speaking and drops anything queued. */
  stop() {
    this.generation++;
    this.queue = [];
    try {
      this.source?.stop();
    } catch {
      /* already stopped */
    }
    this.source = null;
    this.playing = false;
  }

  private async drain(gen: number) {
    this.playing = true;
    while (this.queue.length && gen === this.generation) {
      const clip = await this.queue.shift()!;
      if (!clip || gen !== this.generation) continue;
      await this.play(clip);
    }
    if (gen === this.generation) this.playing = false;
  }

  private play({ audio, rate }: { audio: Float32Array; rate: number }) {
    const ctx = this.audio();
    const buffer = ctx.createBuffer(1, audio.length, rate);
    buffer.copyToChannel(audio as Float32Array<ArrayBuffer>, 0);
    const src = ctx.createBufferSource();
    src.buffer = buffer;
    src.connect(this.analyser!);
    this.source = src;
    return new Promise<void>((resolve) => {
      src.onended = () => resolve();
      src.start();
    });
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

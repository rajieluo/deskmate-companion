// Records one spoken message from the microphone as 16 kHz mono audio.
// Stops by itself after a pause in speech, or when stop() is called.

export interface Recording {
  samples: Float32Array;
  heardSpeech: boolean;
}

export interface MicOptions {
  /** RMS above this counts as speech. */
  threshold?: number;
  /** Silence after speech that ends the recording (ms). */
  silenceMs?: number;
  /** Give up if nobody speaks for this long (ms). */
  noSpeechMs?: number;
  maxMs?: number;
  onLevel?: (level: number) => void;
}

const RATE = 16000;

/** Tracks speech/silence from successive RMS readings. Separate from the audio code so it can be tested. */
export class Endpointer {
  heardSpeech = false;
  private silentFor = 0;
  private elapsed = 0;

  constructor(private o: Required<Omit<MicOptions, "onLevel">>) {}

  /** Feeds one block's RMS; returns true when recording should stop. */
  push(rms: number, ms: number): boolean {
    this.elapsed += ms;
    if (rms > this.o.threshold) {
      this.heardSpeech = true;
      this.silentFor = 0;
    } else {
      this.silentFor += ms;
    }
    if (this.elapsed >= this.o.maxMs) return true;
    if (this.heardSpeech) return this.silentFor >= this.o.silenceMs;
    return this.elapsed >= this.o.noSpeechMs;
  }
}

export class Mic {
  private stopFn: (() => void) | null = null;

  get recording() {
    return this.stopFn !== null;
  }

  stop() {
    this.stopFn?.();
  }

  async record(options: MicOptions = {}): Promise<Recording> {
    const o = { threshold: 0.015, silenceMs: 1200, noSpeechMs: 8000, maxMs: 30000, ...options };
    const stream = await navigator.mediaDevices.getUserMedia({
      audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true },
    });
    // The browser resamples the microphone to the context's rate.
    const ctx = new AudioContext({ sampleRate: RATE });
    const source = ctx.createMediaStreamSource(stream);
    const processor = ctx.createScriptProcessor(2048, 1, 1);
    const chunks: Float32Array[] = [];
    const end = new Endpointer(o);

    return new Promise<Recording>((resolve) => {
      const finish = () => {
        if (!this.stopFn) return;
        this.stopFn = null;
        processor.disconnect();
        source.disconnect();
        stream.getTracks().forEach((t) => t.stop());
        void ctx.close();
        const total = chunks.reduce((n, c) => n + c.length, 0);
        const samples = new Float32Array(total);
        let offset = 0;
        for (const c of chunks) {
          samples.set(c, offset);
          offset += c.length;
        }
        resolve({ samples, heardSpeech: end.heardSpeech });
      };
      this.stopFn = finish;
      processor.onaudioprocess = (e) => {
        const block = new Float32Array(e.inputBuffer.getChannelData(0));
        chunks.push(block);
        let sum = 0;
        for (const s of block) sum += s * s;
        const rms = Math.sqrt(sum / block.length);
        o.onLevel?.(Math.min(1, rms * 12));
        if (end.push(rms, (block.length / RATE) * 1000)) finish();
      };
      source.connect(processor);
      // ScriptProcessor only runs while connected to the output; it writes silence.
      processor.connect(ctx.destination);
    });
  }
}

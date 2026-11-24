import { beforeEach, describe, expect, it, vi } from "vitest";

// The real host module talks to Tauri; stand in for the Pocket TTS calls.
const host = vi.hoisted(() => ({
  pocketPrepare: vi.fn(async () => 24000),
  pocketSpeak: vi.fn(async (_text: string, onChunk: (pcm: Int16Array) => void) => {
    onChunk(new Int16Array([16384, -16384]));
    onChunk(new Int16Array([32767]));
    return { ms: 10, audio_s: 0.1 };
  }),
  pocketCancel: vi.fn(),
  log: vi.fn(),
}));
vi.mock("../src/host", () => ({
  inTauri: false,
  onTtsProgress: () => {},
  ttsPrepare: async () => 0,
  ttsSynthesize: async () => new Float32Array(),
  ...host,
}));

import { PocketEngine, SpeechJob, pcm16ToFloat, type Clip, type SpeechEngine } from "../src/voice";

const clip = (n: number): Clip => ({ audio: new Float32Array(n), rate: 24000 });

class FakeKokoro implements SpeechEngine {
  said: string[] = [];
  async synthesize(text: string) {
    this.said.push(text);
    return clip(7);
  }
}

describe("SpeechJob", () => {
  it("hands over chunks in order, then null once finished", async () => {
    const job = new SpeechJob(true);
    const first = job.next();
    job.push(clip(1));
    job.push(clip(2));
    expect((await first)?.audio.length).toBe(1);
    expect((await job.next())?.audio.length).toBe(2);
    const end = job.next();
    job.finish();
    expect(await end).toBeNull();
    job.push(clip(3));
    expect(await job.next()).toBeNull();
  });
});

describe("pcm16ToFloat", () => {
  it("scales 16-bit samples to -1..1", () => {
    expect(Array.from(pcm16ToFloat(new Int16Array([0, 16384, -32768])))).toEqual([0, 0.5, -1]);
  });
});

describe("PocketEngine", () => {
  beforeEach(() => vi.clearAllMocks());

  it("streams chunks from the sidecar", async () => {
    const kokoro = new FakeKokoro();
    const pocket = new PocketEngine(kokoro);
    const chunks: Clip[] = [];
    await pocket.stream("Hi.", (c) => chunks.push(c));
    expect(chunks.map((c) => Array.from(c.audio))).toEqual([[0.5, -0.5], [32767 / 32768]]);
    expect(chunks[0].rate).toBe(24000);
    expect(kokoro.said).toEqual([]);
  });

  it("falls back to Kokoro for that sentence and the rest of the session", async () => {
    host.pocketSpeak.mockRejectedValueOnce(new Error("sidecar died"));
    const kokoro = new FakeKokoro();
    const pocket = new PocketEngine(kokoro);
    const chunks: Clip[] = [];
    await pocket.stream("One.", (c) => chunks.push(c));
    await pocket.stream("Two.", (c) => chunks.push(c));
    expect(kokoro.said).toEqual(["One.", "Two."]);
    expect(chunks.length).toBe(2);
    expect(host.pocketSpeak).toHaveBeenCalledTimes(1);
    pocket.reset();
    await pocket.stream("Three.", () => {});
    expect(host.pocketSpeak).toHaveBeenCalledTimes(2);
  });

  it("uses Kokoro when the sidecar can't start", async () => {
    host.pocketPrepare.mockRejectedValueOnce(new Error("no python"));
    const kokoro = new FakeKokoro();
    await new PocketEngine(kokoro).stream("Hello.", () => {});
    expect(kokoro.said).toEqual(["Hello."]);
    expect(host.pocketSpeak).not.toHaveBeenCalled();
  });

  it("doesn't repeat a sentence that failed halfway", async () => {
    host.pocketSpeak.mockImplementationOnce(async (_t, onChunk) => {
      onChunk(new Int16Array([1]));
      throw new Error("timed out");
    });
    const kokoro = new FakeKokoro();
    const pocket = new PocketEngine(kokoro);
    const chunks: Clip[] = [];
    await pocket.stream("Half.", (c) => chunks.push(c));
    expect(chunks.length).toBe(1);
    expect(kokoro.said).toEqual([]);
    expect(pocket.broken).toBe(true);
  });

  it("joins chunks for a whole-sentence request", async () => {
    const { audio } = await new PocketEngine(new FakeKokoro()).synthesize("Hi.");
    expect(audio.length).toBe(3);
  });
});

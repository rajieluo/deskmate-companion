// Detects the broken audio that Kokoro's WebGPU path produces on some GPUs.
/** Real speech stays within about ±1; broken WebGPU output peaks in the thousands. */
export function looksGarbled(audio: Float32Array): boolean {
  let peak = 0;
  for (let i = 0; i < audio.length; i++) {
    const v = Math.abs(audio[i]);
    if (!Number.isFinite(v)) return true;
    if (v > peak) peak = v;
  }
  return peak > 1.5;
}

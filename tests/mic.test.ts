import { describe, expect, it } from "vitest";
import { Endpointer } from "../src/mic";

const opts = { threshold: 0.02, silenceMs: 1000, noSpeechMs: 5000, maxMs: 20000 };

describe("Endpointer", () => {
  it("stops after a pause that follows speech", () => {
    const e = new Endpointer(opts);
    expect(e.push(0.1, 500)).toBe(false);
    expect(e.push(0.001, 500)).toBe(false);
    expect(e.push(0.001, 500)).toBe(true);
    expect(e.heardSpeech).toBe(true);
  });

  it("remembers where the speech was", () => {
    const e = new Endpointer(opts);
    e.push(0.001, 500);
    e.push(0.1, 500);
    e.push(0.1, 500);
    e.push(0.001, 500);
    expect(e.speechStart).toBe(500);
    expect(e.speechEnd).toBe(1500);
  });

  it("gives up when nobody speaks", () => {
    const e = new Endpointer(opts);
    for (let i = 0; i < 9; i++) expect(e.push(0, 500)).toBe(false);
    expect(e.push(0, 500)).toBe(true);
    expect(e.heardSpeech).toBe(false);
  });

  it("caps very long recordings", () => {
    const e = new Endpointer(opts);
    let stopped = false;
    for (let i = 0; i < 40 && !stopped; i++) stopped = e.push(0.5, 500);
    expect(stopped).toBe(true);
  });
});

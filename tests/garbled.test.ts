import { describe, expect, it } from "vitest";
import { looksGarbled } from "../src/garbled";

describe("looksGarbled", () => {
  it("accepts normal speech levels", () => {
    expect(looksGarbled(Float32Array.from([0, 0.4, -0.9, 0.2]))).toBe(false);
  });
  it("flags the huge values broken WebGPU output produces", () => {
    expect(looksGarbled(Float32Array.from([0.1, 2889, -40]))).toBe(true);
    expect(looksGarbled(Float32Array.from([0.1, NaN]))).toBe(true);
  });
});

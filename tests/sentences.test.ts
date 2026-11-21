import { describe, expect, it } from "vitest";
import { SentenceBuffer, speakable } from "../src/sentences";

describe("SentenceBuffer", () => {
  it("emits sentences as they complete across chunks", () => {
    const b = new SentenceBuffer();
    expect(b.push("Hello there, how ")).toEqual([]);
    expect(b.push("are you today? I'm fine")).toEqual(["Hello there, how are you today?"]);
    expect(b.push(" thanks. ")).toEqual(["I'm fine thanks."]);
    expect(b.flush()).toEqual([]);
  });

  it("joins very short sentences onto the next one", () => {
    const b = new SentenceBuffer();
    expect(b.push("Oh! That is a great idea. ")).toEqual(["Oh! That is a great idea."]);
  });

  it("splits on newlines and flushes the remainder", () => {
    const b = new SentenceBuffer();
    expect(b.push("First line\nsecond part")).toEqual(["First line"]);
    expect(b.flush()).toEqual(["second part"]);
  });

  it("does not split decimals", () => {
    const b = new SentenceBuffer();
    expect(b.push("It costs 3.50 dollars today. ")).toEqual(["It costs 3.50 dollars today."]);
  });
});

describe("speakable", () => {
  it("drops markdown, emoji and URLs", () => {
    expect(speakable("**Sure!** See https://x.com/a 😀 `npm i`")).toBe("Sure! See a link npm i");
  });
});

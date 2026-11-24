import { describe, expect, it } from "vitest";
import { BUILTIN_DANCE_MS, builtinDancePose, danceIntent, pickDance, type DanceInfo } from "../src/dance";

const dance = (name = null as string | null) => ({ kind: "dance", name });

describe("danceIntent", () => {
  it("spots requests to dance", () => {
    for (const text of [
      "dance",
      "Dance!",
      "please dance",
      "Nicole, dance for me",
      "ok now dance",
      "can you dance?",
      "Could you please do a dance?",
      "wanna dance?",
      "show me a dance",
      "do a little dance",
      "give us a dance!",
      "let's dance",
      "time to dance",
      "dance again",
      "start dancing",
      "踊って",
    ]) {
      expect(danceIntent(text), text).toEqual(dance());
    }
  });

  it("leaves talk about dancing alone", () => {
    for (const text of [
      "I went dancing last night",
      "what's your favourite dance?",
      "my sister teaches dance",
      "don't dance",
      "please don't dance right now",
      "how was the dance recital",
      "I dance salsa on Fridays",
      "",
    ]) {
      expect(danceIntent(text), text).toBeNull();
    }
  });

  it("picks a named dance", () => {
    const names = ["Lamb", "Lamb (short)", "Ghost_Rule", "a"];
    expect(danceIntent("dance Lamb", names)).toEqual(dance("Lamb"));
    expect(danceIntent("do the lamb dance", names)).toEqual(dance("Lamb"));
    expect(danceIntent("dance Lamb (short) please", names)).toEqual(dance("Lamb (short)"));
    expect(danceIntent("can you dance ghost rule?", names)).toEqual(dance("Ghost_Rule"));
    // One-letter names would match everywhere.
    expect(danceIntent("do a dance", names)).toEqual(dance());
  });

  it("spots requests to stop", () => {
    for (const text of ["stop dancing", "Stop the dance!", "ok stop", "enough dancing", "stop"]) {
      expect(danceIntent(text), text).toEqual({ kind: "stop" });
    }
    expect(danceIntent("stop worrying about it")).toBeNull();
  });
});

describe("pickDance", () => {
  const dances: DanceInfo[] = [
    { name: "Lamb", motions: ["Lamb/body.vmd"], audio: null },
    { name: "Wave", motions: ["Wave.vmd"], audio: "Wave.mp3" },
  ];

  it("picks by name, ignoring case", () => {
    expect(pickDance(dances, "wave")?.name).toBe("Wave");
  });

  it("picks at random, avoiding the last one", () => {
    expect(pickDance(dances, null, "Lamb", () => 0)?.name).toBe("Wave");
    expect(pickDance(dances, "nope", "Wave", () => 0.99)?.name).toBe("Lamb");
    expect(pickDance([dances[0]], null, "Lamb")?.name).toBe("Lamb");
    expect(pickDance([], null)).toBeNull();
  });
});

describe("builtinDancePose", () => {
  it("starts and ends at the resting pose", () => {
    const start = builtinDancePose(0, 0.62)!;
    expect(start.weight).toBe(0);
    expect(start.leftArm[2]).toBeCloseTo(-0.62);
    expect(start.rightArm[2]).toBeCloseTo(0.62);
    expect(start.dip).toBe(0);
    expect(builtinDancePose(BUILTIN_DANCE_MS, 0.62)).toBeNull();
    expect(builtinDancePose(BUILTIN_DANCE_MS - 1, 0.62)!.weight).toBeLessThan(0.01);
  });

  it("raises the arms in the middle", () => {
    const mid = builtinDancePose(9000, 0.62)!;
    expect(mid.weight).toBe(1);
    expect(mid.leftArm[2]).toBeGreaterThan(0.5);
    expect(mid.rightArm[2]).toBeLessThan(-0.5);
  });
});

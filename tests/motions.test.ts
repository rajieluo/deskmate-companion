import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  classifyMotion,
  emotionGroup,
  faceGroup,
  groupForEmotion,
  IDLE_GROUP,
  REACT_GROUP,
  regroupMotions,
} from "../src/motions";

// Motion list in the same shape as Aqua's manifest (one unnamed group).
const aquaFiles = [
  "00_Anger_03", "00_Happy_03", "00_Happy_01", "20_Expression_Tooth_01", "20_Expression_Upset_02",
  "00_Puzzle_01", "00_Cry_03", "00_Cry_04", "bound", "00_Cry_01", "20_Expression_Serious_01",
  "20_Expression_Anger_01", "00_Pride_01", "00_Shame_01", "00_Wait_01", "20_Expression_Sad_01",
  "20_Expression_Shame_01", "20_Expression_Puzzle_01", "00_Serious_01", "00_Upset_01",
  "20_Expression_Crymouth_01", "bound_double", "00_Doubt_01", "00_Anger_01", "00_Happy_02",
  "20_Expression_Upset_01", "00_Sad_01", "00_Cry_02", "bound_down", "00_Surprise_01",
  "00_Anger_02", "20_Expression_Smile_01",
].map((n) => ({ File: `motions/${n}.motion3.json` }));
const aqua = { Version: 3, FileReferences: { Moc: "m.moc3", Motions: { "": aquaFiles } } };

describe("classifyMotion", () => {
  it("reads idle and emotions from file names", () => {
    expect(classifyMotion("motions/00_Wait_01.motion3.json")).toEqual({ kind: "idle", layer: "body" });
    expect(classifyMotion("motions/00_Happy_02.motion3.json")).toMatchObject({ emotion: "happy", layer: "body" });
    expect(classifyMotion("motions/00_Cry_01.motion3.json")).toMatchObject({ emotion: "sad" });
    expect(classifyMotion("motions/00_Anger_01.motion3.json")).toMatchObject({ emotion: "angry" });
    expect(classifyMotion("motions/00_Surprise_01.motion3.json")).toMatchObject({ emotion: "surprised" });
    expect(classifyMotion("motions/20_Expression_Smile_01.motion3.json")).toMatchObject({ emotion: "happy", layer: "face" });
    expect(classifyMotion("motions/bound.motion3.json")).toEqual({ kind: "other", layer: "body" });
  });

  it("understands the pinyin names in the girl0x models", () => {
    expect(classifyMotion("stand.motion3.json").kind).toBe("idle");
    expect(classifyMotion("weixiao.motion3.json")).toMatchObject({ emotion: "happy" });
    expect(classifyMotion("shengqi.motion3.json")).toMatchObject({ emotion: "angry" });
    expect(classifyMotion("haixiu.motion3.json")).toMatchObject({ emotion: "shy" });
    expect(classifyMotion("jingya.motion3.json")).toMatchObject({ emotion: "surprised" });
  });
});

describe("regroupMotions", () => {
  it("builds Idle, emotion, face and React groups for Aqua", () => {
    const { settings, summary } = regroupMotions(aqua);
    const g = settings.FileReferences.Motions!;
    expect(g[IDLE_GROUP].map((m) => m.File)).toEqual(["motions/00_Wait_01.motion3.json"]);
    expect(g[emotionGroup("happy")]).toHaveLength(3);
    expect(g[emotionGroup("sad")].length).toBeGreaterThanOrEqual(5); // Sad, Cry x4, Upset
    expect(g[emotionGroup("angry")]).toHaveLength(3);
    expect(g[emotionGroup("surprised")]).toHaveLength(1);
    expect(g[faceGroup("happy")].map((m) => m.File)).toEqual(["motions/20_Expression_Smile_01.motion3.json"]);
    expect(g[REACT_GROUP].map((m) => m.File)).toEqual([
      "motions/bound.motion3.json",
      "motions/bound_double.motion3.json",
      "motions/bound_down.motion3.json",
    ]);
    expect(g[""]).toHaveLength(aquaFiles.length); // original group kept
    expect(summary.idle).toBe(1);
    expect(aqua.FileReferences.Motions[""]).toHaveLength(aquaFiles.length); // input not mutated
  });

  it("keeps an existing Idle group and doesn't treat its motions as reactions", () => {
    const haru = {
      FileReferences: {
        Motions: {
          Idle: [{ File: "motion/m07.motion3.json" }, { File: "motion/m08.motion3.json" }],
          Use: [{ File: "motion/m05.motion3.json" }, { File: "motion/m07.motion3.json" }],
        },
      },
    };
    const g = regroupMotions(haru).settings.FileReferences.Motions!;
    expect(g[IDLE_GROUP]).toHaveLength(2);
    expect(g[REACT_GROUP].map((m) => m.File)).toEqual(["motion/m05.motion3.json"]);
    expect(groupForEmotion(g, "happy")).toBe(REACT_GROUP);
  });

  it("returns null when there is nothing to play", () => {
    expect(groupForEmotion({}, "sad")).toBeNull();
  });
});

describe("real Aqua manifest (only when present locally)", () => {
  const path = process.env.AQUA_MANIFEST;
  it.skipIf(!path)("matches the hand-written list", () => {
    const json = JSON.parse(readFileSync(path!, "utf8"));
    expect(regroupMotions(json).summary.idle).toBe(1);
  });
});

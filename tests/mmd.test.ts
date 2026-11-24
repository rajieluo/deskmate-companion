import { describe, expect, it } from "vitest";
import { modelKind } from "../src/character";
import { blinkWeight, envelope, faceRig, sortVmds } from "../src/mmd-face";

// Morph names from the Ellen sample (standard Japanese MMD names).
const ELLEN = ["まばたき", "ウィンク", "笑い", "びっくり右", "びっくり左", "じと目", "怒り目", "悲しむ", "喜び", "真面目", "困る", "怒り", "にこり", "上", "あ", "い", "う", "え", "お", "にやり", "にっこり", "へ", "口角下げ"];

describe("modelKind", () => {
  it("treats .pmx and .pmd as MMD, even percent-encoded", () => {
    expect(modelKind("http://model.localhost/3/%E8%89%BE%E8%8E%B2.pmx")).toBe("mmd");
    expect(modelKind("/dev/Miku.PMD?x=1")).toBe("mmd");
    expect(modelKind("http://model.localhost/3/1014100.model3.json")).toBe("live2d");
  });
});

describe("faceRig", () => {
  it("finds blink, mouth and every emotion on a standard model", () => {
    const rig = faceRig(ELLEN);
    expect(rig.blink).toBe("まばたき");
    expect(rig.mouth).toBe("あ");
    expect(Object.keys(rig.emotions).sort()).toEqual(["angry", "happy", "proud", "sad", "shy", "surprised", "thinking"]);
    expect(rig.emotions.happy?.map(([n]) => n)).toEqual(["笑い", "にっこり", "にこり"]);
    // Ellen has only the one-sided "surprised" eyes; both are used.
    expect(rig.emotions.surprised?.map(([n]) => n)).toContain("びっくり左");
  });

  it("falls back to English names and leaves out what a model lacks", () => {
    const rig = faceRig(["blink", "a", "smile"]);
    expect(rig.blink).toBe("blink");
    expect(rig.mouth).toBe("a");
    expect(rig.emotions.happy).toEqual([["smile", 1]]);
    expect(rig.emotions.angry).toBeUndefined();
  });

  it("copes with a model without morphs", () => {
    expect(faceRig([])).toEqual({ blink: null, mouth: null, emotions: {}, roles: {} });
  });
});

describe("envelope and blink", () => {
  it("eases in, holds and eases out", () => {
    expect(envelope(0, 1000)).toBe(0);
    expect(envelope(125, 1000)).toBeCloseTo(0.5);
    expect(envelope(500, 1000)).toBe(1);
    expect(envelope(1000, 1000)).toBe(0);
  });

  it("closes the eyes halfway through a blink", () => {
    expect(blinkWeight(80)).toBeCloseTo(1);
    expect(blinkWeight(0)).toBe(0);
    expect(blinkWeight(200)).toBe(0);
  });
});

describe("sortVmds", () => {
  it("picks an idle loop and emotion motions from the folder's .vmd files", () => {
    const set = sortVmds(["motions/Dance.vmd", "motions/idle_breath.vmd", "happy.vmd", "Love eye.exp3.json", "wait 2.vmd"]);
    expect(set.all.map((v) => v.label)).toEqual(["Dance", "happy", "idle breath", "wait 2"]);
    expect(set.idle?.file).toBe("motions/idle_breath.vmd");
    expect(set.emotions.happy?.file).toBe("happy.vmd");
  });

  it("starts an idle pack with its plain standing motion", () => {
    const set = sortVmds(["pack/ot0510_待機_カッコつけ.vmd", "pack/ot0510_待機_素立ち.vmd", "pack/ot0510_Aスタンス.vmd"]);
    expect(set.idle?.label).toBe("ot0510 待機 素立ち");
    expect(set.all).toHaveLength(3);
  });

  it("has no idle when nothing looks like one", () => {
    expect(sortVmds(["Dance.vmd"]).idle).toBeNull();
  });
});

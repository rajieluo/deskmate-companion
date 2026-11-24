import { describe, expect, it } from "vitest";
import { GESTURES, GESTURE_MS, gestureCue, gestureFromFile, gestureIntent, gesturePose, gestureTag } from "../src/gestures";
import { faceRig, sortVmds } from "../src/mmd-face";
import { TagStream } from "../src/tags";

describe("gestureCue", () => {
  it("spots laughs, sighs and yawns in what she says", () => {
    expect(gestureCue("Haha, that's a good one!")).toBe("laugh");
    expect(gestureCue("lol okay")).toBe("laugh");
    expect(gestureCue("*laughs* You got me.")).toBe("laugh");
    expect(gestureCue("Hehe, maybe.")).toBe("giggle");
    expect(gestureCue("Tee hee!")).toBe("giggle");
    expect(gestureCue("*giggles*")).toBe("giggle");
    expect(gestureCue("*yawns* It's late.")).toBe("yawn");
    expect(gestureCue("Yaaawn, I'm sleepy.")).toBe("yawn");
    expect(gestureCue("Sigh. Another bug.")).toBe("sigh");
    expect(gestureCue("(sighs) Fine.")).toBe("sigh");
    expect(gestureCue("*blushes* Stop it.")).toBe("blush");
    expect(gestureCue("Gasp! Really?")).toBe("gasp");
    expect(gestureCue("ふふ、そうだね")).toBe("giggle");
  });

  it("takes the first cue in the line", () => {
    expect(gestureCue("*sighs* Okay, haha.")).toBe("sigh");
  });

  it("leaves ordinary words alone", () => {
    for (const line of [
      "That's what I thought.",
      "Have a look at www.example.com",
      "The hash is ready.",
      "She laughed at the joke.",
      "Look at the shell output.",
      "Thehe file is open.",
      "Ha, true.",
    ]) {
      expect(gestureCue(line), line).toBeNull();
    }
  });
});

describe("gestureIntent", () => {
  it("spots requests", () => {
    expect(gestureIntent("can you yawn?")).toBe("yawn");
    expect(gestureIntent("Nicole, laugh!")).toBe("laugh");
    expect(gestureIntent("giggle for me")).toBe("giggle");
    expect(gestureIntent("do a little sigh")).toBe("sigh");
    expect(gestureIntent("could you please blush")).toBe("blush");
    expect(gestureIntent("laugh again")).toBe("laugh");
  });

  it("leaves talk about them alone", () => {
    for (const text of ["I laughed so hard", "don't laugh", "my cat yawns a lot", "can you laugh at this joke", "we sigh"]) {
      expect(gestureIntent(text), text).toBeNull();
    }
  });
});

describe("gestureTag and TagStream", () => {
  it("knows the tag words", () => {
    expect(gestureTag("Laughs")).toBe("laugh");
    expect(gestureTag("sleepy")).toBe("yawn");
    expect(gestureTag("happy")).toBeNull();
  });

  it("pulls gesture tags out of the text", () => {
    const s = new TagStream();
    const p = s.feed("[happy] Oh no [laugh] that's too funny.");
    expect(p.text).toBe("Oh no that's too funny.");
    expect(p.moods).toEqual(["happy"]);
    expect(p.gestures).toEqual(["laugh"]);
  });
});

describe("gesturePose", () => {
  it("starts and ends at rest and is over after its length", () => {
    for (const g of GESTURES) {
      const start = gesturePose(g, 0)!;
      expect(start.weight, g).toBe(0);
      expect(Math.abs(start.head[0]) + start.shoulders, g).toBe(0);
      const mid = gesturePose(g, GESTURE_MS[g] / 2)!;
      expect(mid.weight, g).toBe(1);
      expect(Object.values(mid.face).some((w) => w > 0.3), g).toBe(true);
      for (const w of Object.values(mid.face)) expect(w).toBeLessThanOrEqual(1);
      expect(gesturePose(g, GESTURE_MS[g]), g).toBeNull();
    }
  });

  it("opens the mouth wide to yawn and tips the head back to laugh", () => {
    expect(gesturePose("yawn", GESTURE_MS.yawn * 0.5)!.face.mouthA).toBeGreaterThan(0.9);
    expect(gesturePose("laugh", 1000)!.head[0]).toBeGreaterThan(0.1);
    expect(gesturePose("blush", 1500)!.head[0]).toBeLessThan(0);
  });
});

describe("gesture motion files", () => {
  it("are found by name, English or Japanese", () => {
    expect(gestureFromFile("motions/laugh.vmd")).toBe("laugh");
    expect(gestureFromFile("笑いモーション/笑い2.vmd")).toBe("laugh");
    expect(gestureFromFile("うとうとあくび.vmd")).toBe("yawn");
    expect(gestureFromFile("ため息.vmd")).toBe("sigh");
    expect(gestureFromFile("giggle.vmd")).toBe("giggle");
    expect(gestureFromFile("idle.vmd")).toBeNull();
  });

  it("are sorted into the model's gestures", () => {
    const set = sortVmds(["idle.vmd", "motions/yawn.vmd", "motions/laugh 1.vmd", "motions/laugh 2.vmd"]);
    expect(set.gestures.yawn?.file).toBe("motions/yawn.vmd");
    expect(set.gestures.laugh?.file).toBe("motions/laugh 1.vmd");
    expect(set.gestures.sigh).toBeUndefined();
  });

  it("face roles come from the model's morphs", () => {
    const rig = faceRig(["まばたき", "あ", "お", "笑い", "にっこり", "困る", "びっくり"]);
    expect(rig.roles).toMatchObject({ eyesClosed: "まばたき", mouthA: "あ", mouthO: "お", smile: "笑い", grin: "にっこり", troubled: "困る", surprised: "びっくり" });
    expect(rig.roles.blush).toBeUndefined();
  });
});

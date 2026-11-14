import { describe, expect, it } from "vitest";
import { TagStream } from "../src/tags";

function run(chunks: string[]) {
  const s = new TagStream();
  let text = "";
  const moods: string[] = [];
  for (const c of chunks) {
    const p = s.feed(c);
    text += p.text;
    moods.push(...p.moods);
  }
  text += s.end().text;
  return { text, moods };
}

describe("TagStream", () => {
  it("strips a leading tag", () => {
    expect(run(["[happy] Hello there!"])).toEqual({ text: "Hello there!", moods: ["happy"] });
  });

  it("handles a tag split across chunks", () => {
    expect(run(["[ha", "ppy", "] Hi! [sur", "prised] Oh!"])).toEqual({
      text: "Hi! Oh!",
      moods: ["happy", "surprised"],
    });
  });

  it("maps near-synonyms and ignores case", () => {
    expect(run(["[Excited] Yay [CONFUSED] hm"]).moods).toEqual(["happy", "thinking"]);
  });

  it("leaves ordinary brackets alone", () => {
    expect(run(["[neutral] Use arr[0] or [see docs]."])).toEqual({
      text: "Use arr[0] or [see docs].",
      moods: ["neutral"],
    });
  });

  it("flushes an unfinished bracket at the end", () => {
    expect(run(["[neutral] a [b"]).text).toBe("a [b");
  });
});

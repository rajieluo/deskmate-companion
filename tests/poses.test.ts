import { describe, expect, it } from "vitest";
import { withFolderFiles } from "../src/poses";

describe("withFolderFiles", () => {
  const json = {
    FileReferences: {
      Motions: { "": [{ File: "motions/10_Happy.motion3.json" }, { File: "motions/2_Expression_smile.motion3.json" }, { File: "motions/2_Wait.motion3.json" }] },
      Expressions: [{ Name: "cry", File: "cry.exp3.json" }],
    },
  };
  const files = ["sitting position.exp3.json", "cry.exp3.json", "motions/extra 1.motion3.json", "motions/2_Wait.motion3.json"];
  const { json: out, actions } = withFolderFiles(json, files);

  it("adds unlisted files and sorts by name", () => {
    expect(actions.expressions.map((e) => e.label)).toEqual(["cry", "sitting position"]);
    expect(actions.motions.map((m) => m.label)).toEqual(["Expression smile", "extra 1", "Happy", "Wait"]);
    expect(out.FileReferences.Motions?.Extra?.map((m) => m.File)).toEqual(["motions/extra 1.motion3.json"]);
  });

  it("marks face clips", () => {
    expect(actions.motions.filter((m) => m.face).map((m) => m.label)).toEqual(["Expression smile"]);
  });
});

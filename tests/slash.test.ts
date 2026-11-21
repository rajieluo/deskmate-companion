import { describe, expect, it } from "vitest";
import { filterCommands, slashQuery, type SlashItem } from "../src/slash";

const items: SlashItem[] = ["clear", "new", "folder", "compact", "context", "tft", "code-review", "anthropic-skills:pdf"].map(
  (name) => ({ name, description: "", app: ["clear", "new", "folder"].includes(name) }),
);
const names = (q: string) => filterCommands(items, q).map((c) => c.name);

describe("slashQuery", () => {
  it("is the text after / while a command name is being typed", () => {
    expect(slashQuery("/")).toBe("");
    expect(slashQuery("/co")).toBe("co");
    expect(slashQuery("/tft what comp")).toBeNull();
    expect(slashQuery("hi /co")).toBeNull();
    expect(slashQuery("")).toBeNull();
  });
});

describe("filterCommands", () => {
  it("lists everything for a bare slash", () => {
    expect(names("")).toHaveLength(items.length);
  });
  it("puts prefix matches first, then plugin-name and inside matches", () => {
    expect(names("co")).toEqual(["compact", "context", "code-review"]);
    expect(names("pdf")).toEqual(["anthropic-skills:pdf"]);
    expect(names("view")).toEqual(["code-review"]);
    expect(names("CLE")).toEqual(["clear"]);
    expect(names("zzz")).toEqual([]);
  });
});

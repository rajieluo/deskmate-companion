import { describe, expect, it, vi } from "vitest";

vi.mock("../src/host", () => ({}));
const { formatPosition, parsePosition, toolbarY } = await import("../src/toolbar");

describe("parsePosition", () => {
  it("reads presets and dragged fractions", () => {
    expect(parsePosition("above")).toBe("above");
    expect(parsePosition("below")).toBe("below");
    expect(parsePosition("0.620")).toBeCloseTo(0.62);
  });
  it("falls back to the middle", () => {
    for (const raw of [undefined, null, "", "left", "2", "-0.1", "abc"]) expect(parsePosition(raw)).toBe("middle");
  });
  it("round-trips a dragged spot", () => {
    expect(parsePosition(formatPosition(0.4567))).toBeCloseTo(0.457);
  });
});

describe("toolbarY", () => {
  it("puts presets above the head, in the middle and at the feet", () => {
    expect(toolbarY("middle", 720, 150)).toBe(360);
    expect(toolbarY("above", 720, 150)).toBe(120);
    expect(toolbarY("below", 720, 150)).toBe(686);
  });
  it("keeps the toolbar inside the window", () => {
    expect(toolbarY("above", 720, 10)).toBe(24);
    expect(toolbarY(0, 720, null)).toBe(24);
    expect(toolbarY(1, 720, null)).toBe(696);
    expect(toolbarY(0.25, 720, null)).toBe(180);
  });
});

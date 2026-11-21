import { describe, expect, it } from "vitest";
import { alphaBounds, boxToCrop, layout } from "../src/fit";

function image(w: number, h: number, filled: [number, number][]) {
  const px = new Uint8Array(w * h * 4);
  for (const [x, y] of filled) px[(y * w + x) * 4 + 3] = 255;
  return px;
}

describe("alphaBounds", () => {
  it("finds the opaque region", () => {
    const px = image(10, 10, [[2, 3], [6, 7]]);
    expect(alphaBounds(px, 10, 10)).toEqual({ x0: 2, y0: 3, x1: 7, y1: 8 });
  });

  it("flips WebGL bottom-up rows", () => {
    // Row 0 in the buffer is the bottom of the image.
    const px = image(4, 4, [[1, 0]]);
    expect(alphaBounds(px, 4, 4, { bottomUp: true })).toEqual({ x0: 1, y0: 3, x1: 2, y1: 4 });
  });

  it("returns null for an empty image", () => {
    expect(alphaBounds(new Uint8Array(16), 2, 2)).toBeNull();
  });
});

describe("layout", () => {
  it("crops the empty space below the feet", () => {
    // Model 1000x2000 where the figure only fills the top 70%.
    const crop = boxToCrop({ x0: 300, y0: 100, x1: 700, y1: 1400 }, 0, 0, 1000, 2000);
    expect(crop).toEqual({ x0: 0.3, y0: 0.05, x1: 0.7, y1: 0.7 });
    const p = layout(crop, 1000, 2000, 360, 560, { pad: 0 });
    const feet = p.y + crop.y1 * 2000 * p.scale;
    const head = p.y + crop.y0 * 2000 * p.scale;
    expect(feet).toBeCloseTo(560);
    expect(head).toBeCloseTo(0);
    const centre = p.x + 0.5 * 1000 * p.scale;
    expect(centre).toBeCloseTo(180);
  });

  it("leaves room at the top when asked", () => {
    const p = layout({ x0: 0, y0: 0, x1: 1, y1: 1 }, 100, 100, 100, 300, { pad: 0, top: 100 });
    expect(p.y).toBeGreaterThanOrEqual(100 - 1e-6);
  });
});

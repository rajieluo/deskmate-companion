// Framing: find where the character actually is inside the model canvas
// (many models, Aqua included, have empty space around and below the figure)
// and scale/position it so the visible part fills the window.

/** A rectangle as fractions (0..1) of the model's own canvas. */
export interface Crop {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

export interface Box {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

/**
 * Bounding box of pixels with alpha above `threshold` in an RGBA buffer.
 * `bottomUp` is true for WebGL readPixels output (row 0 is the bottom row).
 * Returns top-down pixel coordinates, x1/y1 exclusive.
 */
export function alphaBounds(
  rgba: Uint8Array | Uint8ClampedArray,
  width: number,
  height: number,
  { threshold = 8, bottomUp = false } = {},
): Box | null {
  let x0 = width, y0 = height, x1 = -1, y1 = -1;
  for (let row = 0; row < height; row++) {
    const y = bottomUp ? height - 1 - row : row;
    const offset = row * width * 4;
    for (let x = 0; x < width; x++) {
      if (rgba[offset + x * 4 + 3] > threshold) {
        if (x < x0) x0 = x;
        if (x > x1) x1 = x;
        if (y < y0) y0 = y;
        if (y > y1) y1 = y;
      }
    }
  }
  return x1 < 0 ? null : { x0, y0, x1: x1 + 1, y1: y1 + 1 };
}

/** Converts a pixel box on screen into a crop of a model drawn at (mx, my) with size (mw, mh). */
export function boxToCrop(box: Box, mx: number, my: number, mw: number, mh: number): Crop {
  const clamp = (v: number) => Math.min(1, Math.max(0, v));
  return {
    x0: clamp((box.x0 - mx) / mw),
    y0: clamp((box.y0 - my) / mh),
    x1: clamp((box.x1 - mx) / mw),
    y1: clamp((box.y1 - my) / mh),
  };
}

export interface Placement {
  scale: number;
  x: number;
  y: number;
}

export interface LayoutOptions {
  /** Margin around the figure, as a fraction of the window size. */
  pad?: number;
  /** Space kept free at the top (CSS px), e.g. for a chat bubble. */
  top?: number;
  /** User zoom multiplier on top of the fitted scale. */
  zoom?: number;
  /** Space kept free at the bottom (CSS px), e.g. for a hover shadow. */
  bottom?: number;
}

/**
 * Places a model of native size (nw, nh) so its crop fits a view of (vw, vh),
 * centred horizontally with the feet on the bottom edge.
 */
export function layout(
  crop: Crop,
  nw: number,
  nh: number,
  vw: number,
  vh: number,
  { pad = 0.04, top = 0, zoom = 1, bottom = 0 }: LayoutOptions = {},
): Placement {
  const cw = Math.max(1e-6, (crop.x1 - crop.x0) * nw);
  const ch = Math.max(1e-6, (crop.y1 - crop.y0) * nh);
  const availW = vw * (1 - 2 * pad);
  const availH = Math.max(1, vh - top - bottom - vh * pad);
  const scale = Math.min(availW / cw, availH / ch) * zoom;
  const x = vw / 2 - ((crop.x0 + crop.x1) / 2) * nw * scale;
  const y = vh - bottom - vh * pad * 0.25 - crop.y1 * nh * scale;
  return { scale, x, y };
}

export const FULL_CROP: Crop = { x0: 0, y0: 0, x1: 1, y1: 1 };

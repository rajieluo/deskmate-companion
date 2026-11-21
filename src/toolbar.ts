// Where the chat box (text field + buttons) sits over the character: above her
// head, over her middle, at her feet, or wherever it was dragged by its handle.
// Saved in settings.json as `toolbar_position`.
import * as host from "./host";

export type ToolbarPreset = "above" | "middle" | "below";
/** A preset, or a dragged spot as a fraction (0..1) of the window height. */
export type ToolbarPosition = ToolbarPreset | number;

/** Half the toolbar's height plus a little room, in CSS px. */
const HALF = 24;

/** Reads a saved value ("above", "middle", "below" or "0.62"); anything else is "middle". */
export function parsePosition(raw: string | null | undefined): ToolbarPosition {
  if (raw === "above" || raw === "middle" || raw === "below") return raw;
  const n = Number(raw);
  if (raw && Number.isFinite(n) && n >= 0 && n <= 1) return n;
  return "middle";
}

export function formatPosition(pos: ToolbarPosition): string {
  return typeof pos === "number" ? pos.toFixed(3) : pos;
}

/**
 * The toolbar's vertical centre in CSS px for a window `vh` tall, where the
 * character's head starts at `head` (null before a model has loaded).
 */
export function toolbarY(pos: ToolbarPosition, vh: number, head: number | null): number {
  const clamp = (y: number) => Math.round(Math.min(vh - HALF, Math.max(HALF, y)));
  if (pos === "above") return clamp((head ?? vh * 0.2) - HALF - 6);
  if (pos === "below") return clamp(vh - HALF - 10);
  if (pos === "middle") return clamp(vh / 2);
  return clamp(pos * vh);
}

export class Toolbar {
  private pos: ToolbarPosition = "middle";
  private form = document.getElementById("chat") as HTMLFormElement;
  private grip = document.getElementById("toolbar-grip") as HTMLButtonElement;
  /** Called after the toolbar moves, so the bubble can get out of its way. */
  onMove: (() => void) | null = null;

  constructor(private headTop: () => number | null) {
    void host.chatStatus().then((s) => this.set(parsePosition(s.toolbar_position)));
    host.onToolbarPositionChanged((raw) => this.set(parsePosition(raw)));
    window.addEventListener("resize", () => requestAnimationFrame(() => this.apply()));
    this.grip.addEventListener("pointerdown", (e) => this.drag(e));
    // Double-click the handle to put it back in the middle.
    this.grip.addEventListener("dblclick", () => {
      this.set("middle");
      host.setToolbarPosition("middle");
    });
  }

  /** The toolbar's vertical centre in CSS px. */
  get y() {
    return toolbarY(this.pos, window.innerHeight, this.headTop());
  }

  get position() {
    return this.pos;
  }

  set(pos: ToolbarPosition) {
    this.pos = pos;
    this.apply();
  }

  /** Re-places it, e.g. after a new model loads and her head moves. */
  apply() {
    const y = this.y;
    const style = document.body.style;
    style.setProperty("--bar-y", `${y}px`);
    // Panels that open from the toolbar (poses, API key) go towards the roomier side.
    document.body.dataset.bar = y < window.innerHeight / 2 ? "upper" : "lower";
    this.onMove?.();
  }

  private drag(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    this.grip.setPointerCapture(e.pointerId);
    this.form.classList.add("dragging");
    const startY = e.clientY;
    const startBar = this.y;
    let moved = false;
    const move = (ev: PointerEvent) => {
      if (!moved && Math.abs(ev.clientY - startY) < 3) return;
      moved = true;
      this.set((startBar + ev.clientY - startY) / window.innerHeight);
    };
    const up = () => {
      this.grip.removeEventListener("pointermove", move);
      this.grip.removeEventListener("pointerup", up);
      this.grip.removeEventListener("pointercancel", up);
      this.form.classList.remove("dragging");
      if (!moved) return;
      // Save the clamped spot, not wherever the pointer was let go.
      const pos = this.y / window.innerHeight;
      this.set(pos);
      host.setToolbarPosition(formatPosition(pos));
    };
    this.grip.addEventListener("pointermove", move);
    this.grip.addEventListener("pointerup", up);
    this.grip.addEventListener("pointercancel", up);
  }
}

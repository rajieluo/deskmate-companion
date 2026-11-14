// The Live2D character: loading, framing, motions, gaze and pixel hit-testing.
import * as PIXI from "pixi.js";
import { Live2DModel, MotionPriority } from "pixi-live2d-display/cubism4";
import { alphaBounds, boxToCrop, FULL_CROP, layout, type Crop } from "./fit";
import { type Emotion, faceGroup, groupForEmotion, type ModelSettingsJson, regroupMotions, type MotionSummary } from "./motions";

// pixi-live2d-display looks for PIXI on window for some features.
(window as unknown as { PIXI: typeof PIXI }).PIXI = PIXI;
Live2DModel.registerTicker(PIXI.Ticker);

const nextFrame = () => new Promise((r) => requestAnimationFrame(() => r(null)));
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export class Avatar {
  readonly app: PIXI.Application;
  private model: Live2DModel | null = null;
  private crop: Crop = FULL_CROP;
  private groups: Record<string, unknown[]> = {};
  private loadId = 0;
  summary: MotionSummary | null = null;
  zoom = 1;
  /** CSS px kept free above the character for the speech bubble. */
  topReserve = 150;

  constructor(canvas: HTMLCanvasElement) {
    this.app = new PIXI.Application({
      view: canvas,
      resizeTo: window,
      backgroundAlpha: 0,
      antialias: true,
      autoDensity: true,
      resolution: window.devicePixelRatio || 1,
      // Keeps the last frame readable for pixel hit-testing.
      preserveDrawingBuffer: true,
    });
    window.addEventListener("resize", () => {
      // resizeTo runs on the next frame; lay out after it.
      requestAnimationFrame(() => this.place());
    });
  }

  get loaded() {
    return this.model !== null;
  }

  async load(url: string): Promise<MotionSummary> {
    const id = ++this.loadId;
    const res = await fetch(url);
    if (!res.ok) throw new Error(`Could not read ${url} (HTTP ${res.status})`);
    const json = (await res.json()) as ModelSettingsJson;
    const { settings, summary } = regroupMotions(json);

    const model = await Live2DModel.from({ ...settings, url } as never, { autoInteract: false });
    if (id !== this.loadId) {
      model.destroy();
      throw new Error("superseded");
    }

    if (this.model) {
      this.app.stage.removeChild(this.model);
      this.model.destroy();
    }
    this.model = model;
    this.groups = settings.FileReferences.Motions ?? {};
    this.summary = summary;
    model.alpha = 0;
    this.app.stage.addChild(model);

    await this.measureCrop();
    this.place();
    model.alpha = 1;
    return summary;
  }

  /** Draws the whole model canvas once and records where the visible figure is. */
  private async measureCrop() {
    const model = this.model!;
    const { width: vw, height: vh } = this.app.screen;
    const nw = model.internalModel.width;
    const nh = model.internalModel.height;
    const full = layout(FULL_CROP, nw, nh, vw, vh, { pad: 0 });
    model.scale.set(full.scale);
    model.position.set(full.x, full.y);
    model.alpha = 1;
    // Let the idle pose settle so the crop matches the resting figure.
    for (let i = 0; i < 3; i++) await nextFrame();
    await sleep(400);

    this.app.render();
    const gl = (this.app.renderer as PIXI.Renderer).gl;
    const w = gl.drawingBufferWidth;
    const h = gl.drawingBufferHeight;
    const px = new Uint8Array(w * h * 4);
    gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, px);
    model.alpha = 0;

    const box = alphaBounds(px, w, h, { bottomUp: true });
    if (!box) {
      this.crop = FULL_CROP;
      return;
    }
    const r = w / vw; // device pixels per CSS pixel
    const crop = boxToCrop(
      { x0: box.x0 / r, y0: box.y0 / r, x1: box.x1 / r, y1: box.y1 / r },
      full.x,
      full.y,
      nw * full.scale,
      nh * full.scale,
    );
    // Arms and hair move during motions; leave a little slack at the sides and top.
    const slackX = (crop.x1 - crop.x0) * 0.12;
    const slackY = (crop.y1 - crop.y0) * 0.03;
    this.crop = {
      x0: Math.max(0, crop.x0 - slackX),
      y0: Math.max(0, crop.y0 - slackY),
      x1: Math.min(1, crop.x1 + slackX),
      y1: crop.y1,
    };
  }

  place() {
    const model = this.model;
    if (!model) return;
    const { width: vw, height: vh } = this.app.screen;
    const p = layout(this.crop, model.internalModel.width, model.internalModel.height, vw, vh, {
      zoom: this.zoom,
      top: this.topReserve,
    });
    model.scale.set(p.scale);
    model.position.set(p.x, p.y);
  }

  /** Y of the top of the character's head in CSS px, or null before a model is loaded. */
  headTop(): number | null {
    const m = this.model;
    if (!m) return null;
    return m.y + this.crop.y0 * m.internalModel.height * m.scale.y;
  }

  /** True if the pixel under (x, y), in device pixels from the top-left, is part of the character. */
  hitTest(x: number, y: number): boolean {
    if (!this.model || this.model.alpha === 0) return false;
    const gl = (this.app.renderer as PIXI.Renderer).gl;
    const h = gl.drawingBufferHeight;
    if (x < 0 || y < 0 || x >= gl.drawingBufferWidth || y >= h) return false;
    const px = new Uint8Array(4);
    gl.readPixels(Math.floor(x), h - 1 - Math.floor(y), 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
    return px[3] > 16;
  }

  /** Plays the body motion for an emotion (and its face overlay when there is no body motion). */
  emote(emotion: Emotion): boolean {
    const model = this.model;
    if (!model) return false;
    const group = groupForEmotion(this.groups, emotion) ?? (this.groups[faceGroup(emotion)]?.length ? faceGroup(emotion) : null);
    if (!group) return false;
    void model.motion(group, undefined, MotionPriority.FORCE);
    return true;
  }

  /** Makes the character look towards a point in CSS pixels. */
  lookAt(x: number, y: number) {
    this.model?.focus(x, y);
  }
}

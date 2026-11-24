// The Live2D character: loading, framing, motions, gaze and pixel hit-testing.
import { detectLookParams, type LookParams } from "./look";
import { blend, type ExpressionParam, type ModelActions, parseExpression, withFolderFiles } from "./poses";
import * as PIXI from "pixi.js";
import { Live2DModel, MotionPriority } from "pixi-live2d-display/cubism4";
import { alphaBounds, boxToCrop, FULL_CROP, layout, type Crop } from "./fit";
import type { Character } from "./character";
import { type Emotion, faceGroup, groupForEmotion, type ModelSettingsJson, regroupMotions, type MotionSummary } from "./motions";

// pixi-live2d-display looks for PIXI on window for some features.
(window as unknown as { PIXI: typeof PIXI }).PIXI = PIXI;
Live2DModel.registerTicker(PIXI.Ticker);

const nextFrame = () => new Promise((r) => requestAnimationFrame(() => r(null)));
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export class Avatar implements Character {
  readonly app: PIXI.Application;
  model: Live2DModel | null = null;
  private crop: Crop = FULL_CROP;
  private groups: Record<string, unknown[]> = {};
  private loadId = 0;
  private lipSyncIds: string[] = [];
  private mouthSource: (() => number) | null = null;
  private mouthWasOpen = false;
  summary: MotionSummary | null = null;
  zoom = 1;
  /** CSS px kept free above the character for the speech bubble. */
  topReserve = 150;
  /** Fades the bottom of the figure out over this fraction of its height (0 = off). */
  fadeBottom = 0.16;
  private fadeEnd = Infinity;

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

  /** Parameters that follow the mouse for the current model (see look.ts). */
  lookParams: LookParams | null = null;

  async load(
    url: string,
    look: Partial<Record<string, string>> = {},
    params: Record<string, number> = {},
    files: string[] = [],
  ): Promise<MotionSummary> {
    const id = ++this.loadId;
    this.app.start();
    const res = await fetch(url);
    if (!res.ok) throw new Error(`Could not read ${url} (HTTP ${res.status})`);
    const raw = (await res.json()) as ModelSettingsJson;
    const { json, actions } = withFolderFiles(raw, files);
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
    this.actions = actions;
    this.modelUrl = url;
    this.activeExpressions.clear();
    this.lipSyncIds = lipSyncIds(json);
    const held = Object.entries(params).filter(([, v]) => typeof v === "number");
    model.internalModel.on("beforeModelUpdate", () => {
      if (held.length) {
        const core = model.internalModel.coreModel as unknown as { setParameterValueById(id: string, v: number): void };
        for (const [id, v] of held) core.setParameterValueById(id, v);
      }
      this.applyExpressions();
      this.applyMouth();
    });
    this.lookParams = applyLookParams(model, look);
    this.summary = summary;
    model.alpha = 0;
    this.app.stage.addChild(model);

    await this.measureCrop();
    // The textures are on the GPU once measureCrop has drawn the model.
    limitMipmaps(model, this.app.renderer as PIXI.Renderer);
    this.place();
    model.alpha = 1;
    return summary;
  }

  /** Removes the model and stops drawing (another kind of model is showing). */
  unload() {
    this.loadId++;
    if (this.model) {
      this.app.stage.removeChild(this.model);
      this.model.destroy();
      this.model = null;
    }
    this.actions = { motions: [], expressions: [] };
    this.activeExpressions.clear();
    this.app.stop();
  }

  /** Draws the whole model canvas once and records where the visible figure is. */
  /** True while measureCrop has the model at its measuring position; place() waits. */
  private measuring = false;

  private async measureCrop() {
    this.measuring = true;
    try {
      await this.measureCropInner();
    } finally {
      this.measuring = false;
    }
  }

  private async measureCropInner() {
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
    // A toggle (fade, zoom) arriving mid-measurement would move the model and spoil
    // the crop; load() calls place() again once measuring is done.
    if (!model || this.measuring) return;
    const { width: vw, height: vh } = this.app.screen;
    const p = layout(this.crop, model.internalModel.width, model.internalModel.height, vw, vh, {
      zoom: this.zoom,
      top: this.topReserve,
    });
    model.scale.set(p.scale);
    model.position.set(p.x, p.y);
    this.applyFade(p.y, p.scale);
  }

  /**
   * Softly fades out the bottom edge of the figure, for models drawn without
   * feet (like Aqua), so the cut-off reads as intentional.
   */
  /** Turns the bottom fade on (default band) or off. */
  setFade(on: boolean) {
    this.fadeBottom = on ? 0.16 : 0;
    this.place();
  }

  private applyFade(y: number, scale: number) {
    const model = this.model!;
    const canvas = this.app.view as HTMLCanvasElement;
    const nh = model.internalModel.height;
    const figure = (this.crop.y1 - this.crop.y0) * nh * scale;
    const end = y + this.crop.y1 * nh * scale;
    const start = end - figure * this.fadeBottom;
    if (this.fadeBottom <= 0) {
      canvas.style.removeProperty("mask-image");
      canvas.style.removeProperty("-webkit-mask-image");
      this.fadeEnd = Infinity;
      return;
    }
    // Ease the fade (more opaque near the top of the band) so there's no visible line.
    const mask = `linear-gradient(to bottom, #000 ${start}px, rgba(0,0,0,.75) ${start + (end - start) * 0.4}px, rgba(0,0,0,.3) ${start + (end - start) * 0.75}px, transparent ${end}px)`;
    canvas.style.setProperty("mask-image", mask);
    canvas.style.setProperty("-webkit-mask-image", mask);
    // Clicks on the faint part of the fade fall through like empty space.
    this.fadeEnd = start + (end - start) * 0.6;
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
    if (y / (window.devicePixelRatio || 1) > this.fadeEnd) return false;
    const gl = (this.app.renderer as PIXI.Renderer).gl;
    const h = gl.drawingBufferHeight;
    if (x < 0 || y < 0 || x >= gl.drawingBufferWidth || y >= h) return false;
    const px = new Uint8Array(4);
    gl.readPixels(Math.floor(x), h - 1 - Math.floor(y), 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
    return px[3] > 16;
  }

  /** Motions and expressions the current model has, for the picker. */
  actions: ModelActions = { motions: [], expressions: [] };
  private modelUrl = "";
  /** Expressions toggled on, by file: they stack and stay until toggled off. */
  private activeExpressions = new Map<string, ExpressionParam[]>();

  /** Plays one of the model's motions by group and index. */
  playMotion(group: string, index: number) {
    void this.model?.motion(group, index, MotionPriority.FORCE);
  }

  isExpressionOn(file: string) {
    return this.activeExpressions.has(file);
  }

  /** Turns an expression (pose, eye style, prop...) on or off. */
  async toggleExpression(file: string, on = !this.activeExpressions.has(file)): Promise<boolean> {
    if (!on) {
      this.activeExpressions.delete(file);
      return false;
    }
    const url = new URL(file.split("/").map(encodeURIComponent).join("/"), this.modelUrl).href;
    const res = await fetch(url);
    if (!res.ok) throw new Error(`Could not read ${file} (HTTP ${res.status})`);
    this.activeExpressions.set(file, parseExpression(await res.json()));
    return true;
  }

  private applyExpressions() {
    if (!this.activeExpressions.size || !this.model) return;
    const core = this.model.internalModel.coreModel as unknown as {
      getParameterValueById(id: string): number;
      setParameterValueById(id: string, v: number): void;
    };
    for (const params of this.activeExpressions.values()) {
      for (const p of params) core.setParameterValueById(p.id, blend(core.getParameterValueById(p.id), p));
    }
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

  /** Drives the mouth from a 0..1 level (e.g. voice loudness) every frame. */
  setMouthSource(source: () => number) {
    this.mouthSource = source;
  }

  private applyMouth() {
    const model = this.model;
    if (!model || !this.mouthSource) return;
    const v = this.mouthSource();
    // Only take over the mouth while speaking, so motions keep their own mouth shapes otherwise.
    if (v <= 0 && !this.mouthWasOpen) return;
    this.mouthWasOpen = v > 0;
    const core = model.internalModel.coreModel as unknown as {
      setParameterValueById(id: string, value: number, weight?: number): void;
    };
    for (const id of this.lipSyncIds) core.setParameterValueById(id, v);
  }

  /**
   * One-line summary of the model for the log: Cubism versions, and the mouth
   * parameters' values as the next frame is drawn (so "mouth stuck open" can be
   * told apart from art that just looks open).
   */
  diagnostics(): Promise<string> {
    const model = this.model;
    if (!model) return Promise.resolve("no model");
    const im = model.internalModel;
    type Params = { count: number; ids: string[]; defaultValues: Float32Array; minimumValues: Float32Array; maximumValues: Float32Array };
    const core = im.coreModel as unknown as { _model?: { parameters: Params }; getParameterValueByIndex(i: number): number };
    return new Promise((resolve) => {
      im.once("beforeModelUpdate", () => {
        const p = core._model?.parameters;
        const cubism = (window as unknown as { Live2DCubismCore?: { Version: { csmGetVersion(): number; csmGetLatestMocVersion(): number } } })
          .Live2DCubismCore?.Version;
        const v = cubism?.csmGetVersion() ?? 0;
        const parts = [
          `core ${(v >>> 24) & 0xff}.${(v >>> 16) & 0xff}.${v & 0xffff}`,
          `latest moc ${cubism?.csmGetLatestMocVersion()}`,
          `params ${p?.count}`,
        ];
        if (p) {
          for (let i = 0; i < p.count; i++) {
            if (!/mouth|jaw|lip/i.test(p.ids[i])) continue;
            const f = (n: number) => +n.toFixed(2);
            parts.push(`${p.ids[i]}=${f(core.getParameterValueByIndex(i))} (default ${f(p.defaultValues[i])}, ${f(p.minimumValues[i])}..${f(p.maximumValues[i])})`);
          }
        }
        parts.push(`lipsync ${this.lipSyncIds.join(",")}`);
        resolve(parts.join("; "));
      });
    });
  }

  private followMouse = true;

  /** Turns cursor following on or off; off, she looks straight ahead. */
  setFollowMouse(on: boolean) {
    this.followMouse = on;
    if (!on) this.model?.internalModel.focusController.focus(0, 0);
  }

  /** Makes the character look towards a point in CSS pixels. */
  lookAt(x: number, y: number) {
    if (this.followMouse) this.model?.focus(x, y);
  }
}

/**
 * Points the library's mouse-follow and breathing at this model's own parameter
 * IDs. It only knows the Cubism 3 names (ParamAngleX...), so models that use
 * PARAM_ANGLE_X-style or other names would otherwise ignore the cursor.
 */
function applyLookParams(model: Live2DModel, look: Partial<Record<string, string>>): LookParams | null {
  type Internal = Record<string, unknown> & {
    coreModel: { _model?: { parameters?: { ids?: string[] } } };
    breath?: { getParameters(): { parameterId: string }[] };
  };
  const internal = model.internalModel as unknown as Internal;
  const ids = internal.coreModel._model?.parameters?.ids;
  if (!ids) return null;
  const found = detectLookParams(Array.from(ids), look);
  const fields: [keyof LookParams, string][] = [
    ["angle_x", "idParamAngleX"],
    ["angle_y", "idParamAngleY"],
    ["angle_z", "idParamAngleZ"],
    ["eye_x", "idParamEyeBallX"],
    ["eye_y", "idParamEyeBallY"],
    ["body_x", "idParamBodyAngleX"],
    ["breath", "idParamBreath"],
  ];
  // A role with no parameter gets an id the model doesn't have, which Cubism ignores.
  for (const [role, field] of fields) internal[field] = found[role] ?? `__none_${role}`;
  // CubismBreath copied the ids at init, in this order.
  const breath = internal.breath?.getParameters() ?? [];
  const order: (keyof LookParams)[] = ["angle_x", "angle_y", "angle_z", "body_x", "breath"];
  breath.forEach((b, i) => {
    const role = order[i];
    if (role) b.parameterId = found[role] ?? `__none_${role}`;
  });
  console.info("[avatar] mouse-follow parameters", found);
  return found;
}

/**
 * Caps how far the model's textures are mipmapped. Live2D textures are atlases
 * with parts packed close together; in the small mip levels a texel spans
 * several neighbouring parts, so at desktop size their colours bleed into each
 * other. On Nicole that drew faint dark lines under her closed mouth. Level 2
 * (a quarter of full size) still smooths the minified figure without the bleed.
 */
export const MAX_MIP_LEVEL = 2;

function limitMipmaps(model: Live2DModel, renderer: PIXI.Renderer) {
  const gl = renderer.gl as WebGL2RenderingContext;
  if (!(typeof WebGL2RenderingContext !== "undefined" && gl instanceof WebGL2RenderingContext)) return;
  for (const texture of model.textures) {
    const base = texture.baseTexture;
    if (!base._glTextures[renderer.CONTEXT_UID]) continue;
    // Bind through Pixi so its record of the bound texture stays right.
    renderer.texture.bind(base, 0);
    gl.texParameterf(gl.TEXTURE_2D, gl.TEXTURE_MAX_LOD, MAX_MIP_LEVEL);
  }
}

/** Mouth parameter ids: the model's LipSync group, or the standard names. */
function lipSyncIds(settings: ModelSettingsJson): string[] {
  const groups = (settings as { Groups?: { Name?: string; Ids?: string[] }[] }).Groups ?? [];
  const ids = groups.find((g) => g.Name === "LipSync")?.Ids ?? [];
  return ids.length ? ids : ["ParamMouthOpenY", "PARAM_MOUTH_OPEN_Y"];
}

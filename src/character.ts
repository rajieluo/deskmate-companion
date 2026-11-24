// The character on screen, whichever kind of model it is. The Live2D Avatar and
// the MMD (3D) avatar each draw on their own canvas; Stage shows the one that
// fits the chosen model file and forwards every call to it.
import type { Avatar } from "./avatar";
import type { DanceMotion } from "./dance";
import type { Gesture } from "./gestures";
import type { MmdAvatar } from "./mmd";
import type { Emotion, MotionSummary } from "./motions";
import type { ModelActions } from "./poses";
import type { ModelSource } from "./host";

/** What the chat, the picker and the window need from a character. */
export interface Character {
  readonly loaded: boolean;
  actions: ModelActions;
  emote(emotion: Emotion): boolean;
  /** A short emote (laugh, yawn…); a model without them shows the nearest emotion. */
  gesture(g: Gesture): boolean;
  headTop(): number | null;
  /** (x, y) in device pixels from the window's top-left. */
  hitTest(x: number, y: number): boolean;
  /** (x, y) in CSS pixels. */
  lookAt(x: number, y: number): void;
  setFollowMouse(on: boolean): void;
  setFade(on: boolean): void;
  setMouthSource(source: () => number): void;
  playMotion(group: string, index: number): void;
  isExpressionOn(file: string): boolean;
  toggleExpression(file: string, on?: boolean): Promise<boolean>;
  diagnostics(): Promise<string>;
  /** Dances once (null: the built-in dance). False if this kind of model can't. */
  dance(motion: DanceMotion | null): Promise<boolean>;
  stopDance(): void;
  /** Whether it can dance at all (MMD models can). */
  readonly canDance: boolean;
}

export type ModelKind = "live2d" | "mmd";

/** MMD models are .pmx or .pmd files; everything else is treated as Live2D. */
export function modelKind(url: string): ModelKind {
  let path = url.split(/[?#]/)[0];
  try {
    path = decodeURIComponent(path);
  } catch {
    /* keep it encoded */
  }
  return /\.(pmx|pmd)$/i.test(path) ? "mmd" : "live2d";
}

const EMPTY: ModelActions = { motions: [], expressions: [] };

export class Stage implements Character {
  private mmd: MmdAvatar | null = null;
  private kind: ModelKind = "live2d";
  private mouth: (() => number) | null = null;
  private followMouse = true;
  private fade = true;

  constructor(
    readonly live2d: Avatar,
    private live2dCanvas: HTMLCanvasElement,
    private mmdCanvas: HTMLCanvasElement,
  ) {
    this.show("live2d");
  }

  private get active(): Character | null {
    return this.kind === "mmd" ? this.mmd : this.live2d;
  }

  get loaded() {
    return this.active?.loaded ?? false;
  }

  get actions() {
    return this.active?.actions ?? EMPTY;
  }

  /** The Live2D model's mouse-follow parameters, for the log. */
  get lookParams() {
    return this.kind === "live2d" ? this.live2d.lookParams : null;
  }

  private show(kind: ModelKind) {
    this.kind = kind;
    this.live2dCanvas.hidden = kind !== "live2d";
    this.mmdCanvas.hidden = kind !== "mmd";
  }

  async load(source: ModelSource, files: string[]): Promise<MotionSummary> {
    if (modelKind(source.url) === "mmd") {
      if (!this.mmd) {
        const { MmdAvatar } = await import("./mmd");
        this.mmd = new MmdAvatar(this.mmdCanvas);
        if (this.mouth) this.mmd.setMouthSource(this.mouth);
        this.mmd.setFollowMouse(this.followMouse);
        this.mmd.setFade(this.fade);
      }
      // Show the canvas before loading so it has its real size for framing.
      this.show("mmd");
      this.live2d.unload();
      return this.mmd.load(source.url, files, source.label);
    }
    this.show("live2d");
    this.mmd?.dispose();
    this.mmd = null;
    return this.live2d.load(source.url, source.look, source.params, files);
  }

  emote(emotion: Emotion) {
    return this.active?.emote(emotion) ?? false;
  }
  gesture(g: Gesture) {
    return this.active?.gesture(g) ?? false;
  }
  headTop() {
    return this.active?.headTop() ?? null;
  }
  hitTest(x: number, y: number) {
    return this.active?.hitTest(x, y) ?? false;
  }
  lookAt(x: number, y: number) {
    this.active?.lookAt(x, y);
  }
  setFollowMouse(on: boolean) {
    this.followMouse = on;
    this.live2d.setFollowMouse(on);
    this.mmd?.setFollowMouse(on);
  }
  setFade(on: boolean) {
    this.fade = on;
    this.live2d.setFade(on);
    this.mmd?.setFade(on);
  }
  setMouthSource(source: () => number) {
    this.mouth = source;
    this.live2d.setMouthSource(source);
    this.mmd?.setMouthSource(source);
  }
  playMotion(group: string, index: number) {
    this.active?.playMotion(group, index);
  }
  isExpressionOn(file: string) {
    return this.active?.isExpressionOn(file) ?? false;
  }
  toggleExpression(file: string, on?: boolean) {
    return this.active?.toggleExpression(file, on) ?? Promise.resolve(false);
  }
  diagnostics() {
    return this.active?.diagnostics() ?? Promise.resolve("no model");
  }
  get canDance() {
    return this.active?.canDance ?? false;
  }
  dance(motion: DanceMotion | null) {
    return this.active?.dance(motion) ?? Promise.resolve(false);
  }
  stopDance() {
    this.active?.stopDance();
  }
}

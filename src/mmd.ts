// An MMD (MikuMikuDance .pmx/.pmd) character, drawn with Babylon.js and babylon-mmd
// on its own canvas. It offers the same controls as the Live2D Avatar (emotions,
// lip sync, looking at the cursor, the poses picker, click-through hit testing),
// built from the model's morphs and bones instead of Live2D parameters.
//
// Motion: MMD models come in a T-pose and without idle animations. If the model's
// folder has .vmd motion files, an idle-looking one loops (see sortVmds); otherwise
// the arms are lowered and she breathes and sways a little. Hair and clothes follow
// the model's own rigid bodies through Bullet physics (WASM).
//
// This module is loaded on demand, so Live2D-only use never downloads Babylon.
import "@babylonjs/core/Materials/standardMaterial";
import "@babylonjs/core/Materials/Textures/Loaders/tgaTextureLoader";
// Registers the MMD toon material as the loaders' default.
import "babylon-mmd/esm/Loader/mmdModelLoader";
import "babylon-mmd/esm/Loader/pmxLoader";
import "babylon-mmd/esm/Loader/pmdLoader";
import "babylon-mmd/esm/Runtime/Animation/mmdRuntimeModelAnimation";

import { Camera } from "@babylonjs/core/Cameras/camera";
import { FreeCamera } from "@babylonjs/core/Cameras/freeCamera";
import { Engine } from "@babylonjs/core/Engines/engine";
import { DirectionalLight } from "@babylonjs/core/Lights/directionalLight";
import { HemisphericLight } from "@babylonjs/core/Lights/hemisphericLight";
import { LoadAssetContainerAsync } from "@babylonjs/core/Loading/sceneLoader";
import { Space } from "@babylonjs/core/Maths/math.axis";
import { Color3, Color4 } from "@babylonjs/core/Maths/math.color";
import { Quaternion, Vector3 } from "@babylonjs/core/Maths/math.vector";
import type { Mesh } from "@babylonjs/core/Meshes/mesh";
import { Scene } from "@babylonjs/core/scene";
import { VmdLoader } from "babylon-mmd/esm/Loader/vmdLoader";
import type { MmdModelMetadata } from "babylon-mmd/esm/Loader/mmdModelMetadata";
import { MmdStandardMaterialProxy } from "babylon-mmd/esm/Runtime/mmdStandardMaterialProxy";
import type { MmdModel } from "babylon-mmd/esm/Runtime/mmdModel";
import type { MmdRuntimeAnimationHandle } from "babylon-mmd/esm/Runtime/mmdRuntimeAnimationHandle";
import { MmdRuntime } from "babylon-mmd/esm/Runtime/mmdRuntime";
import type { IMmdRuntimeLinkedBone } from "babylon-mmd/esm/Runtime/IMmdRuntimeLinkedBone";
import { MmdWasmInstanceTypeSPR } from "babylon-mmd/esm/Runtime/Optimized/InstanceType/singlePhysicsRelease";
import { GetMmdWasmInstance } from "babylon-mmd/esm/Runtime/Optimized/mmdWasmInstance";
import { MultiPhysicsRuntime } from "babylon-mmd/esm/Runtime/Optimized/Physics/Bind/Impl/multiPhysicsRuntime";
import { MmdBulletPhysics } from "babylon-mmd/esm/Runtime/Optimized/Physics/mmdBulletPhysics";

import type { Character } from "./character";
import { BONES, blinkWeight, envelope, faceRig, findBone, sortVmds, type FaceRig, type VmdFile, type VmdSet } from "./mmd-face";
import type { Emotion, MotionSummary } from "./motions";
import type { ModelActions } from "./poses";

const EMOTE_MS = 3200;
const FPS = 30;
const VMD_GROUP = "vmd";
const BUILTIN_GROUP = "builtin";
const MORPH_PREFIX = "morph:";

/** Lowered-arm angle (radians) for models standing in the T-pose. */
const ARMS_DOWN = 0.62;

interface Playing {
  file: VmdFile;
  handle: MmdRuntimeAnimationHandle;
  endFrame: number;
  loop: boolean;
  start: number;
}

export class MmdAvatar implements Character {
  private engine: Engine;
  private scene: Scene;
  private camera: FreeCamera;
  private runtime: MmdRuntime | null = null;
  private physics: MultiPhysicsRuntime | null = null;
  private mesh: Mesh | null = null;
  private mmd: MmdModel | null = null;
  private loadId = 0;
  private disposed = false;

  private rig: FaceRig = { blink: null, mouth: null, emotions: {} };
  private morphNames: string[] = [];
  private bones = new Map<string, IMmdRuntimeLinkedBone>();
  private vmds: VmdSet = { idle: null, emotions: {}, all: [] };
  private handles = new Map<string, MmdRuntimeAnimationHandle>();
  private playing: Playing | null = null;
  /** A motion picked in the picker; loops instead of the folder's idle. */
  private pickedIdle: VmdFile | null | undefined = undefined;
  private modelUrl = "";
  private info = { name: "", rigidBodies: 0 };

  /** Model height range in world units (feet at minY). */
  private minY = 0;
  private maxY = 20;
  /** Pixels per world unit, and the world Y at the top of the window. */
  private pxPerUnit = 1;
  private topY = 20;

  private mouthSource: (() => number) | null = null;
  private mouthWasOpen = false;
  private emotion: { morphs: [string, number][]; start: number } | null = null;
  private nextBlink = performance.now() + 2000;
  private blinkStart = -1;
  private touched = new Set<string>();
  private toggled = new Set<string>();

  private followMouse = true;
  private look = { x: 0, y: 0 };
  private lookTarget = { x: 0, y: 0 };
  private fadeBottom = 0.16;
  private fadeEnd = Infinity;

  zoom = 1;
  /** CSS px kept free above the character for the speech bubble. */
  topReserve = 150;
  actions: ModelActions = { motions: [], expressions: [] };

  constructor(private canvas: HTMLCanvasElement) {
    this.engine = new Engine(
      canvas,
      true,
      // Keeps the last frame readable for pixel hit-testing; the window is transparent.
      { preserveDrawingBuffer: true, stencil: true, alpha: true, premultipliedAlpha: true },
      true,
    );
    this.scene = this.makeScene();
    this.camera = this.scene.activeCamera as FreeCamera;
    // 30 fps is plenty for a desktop character and halves the GPU and CPU cost.
    let last = 0;
    this.engine.runRenderLoop(() => {
      const now = performance.now();
      if (now - last < 1000 / FPS - 2) return;
      last = now;
      this.scene.render();
    });
    window.addEventListener("resize", this.onResize);
  }

  private onResize = () => {
    this.engine.resize();
    this.place();
  };

  private makeScene() {
    const scene = new Scene(this.engine);
    scene.clearColor = new Color4(0, 0, 0, 0);
    // The app handles the mouse itself. Babylon's own input would preventDefault()
    // every pointerdown, and then the canvas never gets mousedown/mouseup, so she
    // couldn't be dragged or clicked.
    scene.detachControl();
    scene.preventDefaultOnPointerDown = false;
    scene.preventDefaultOnPointerUp = false;
    scene.ambientColor = new Color3(0.5, 0.5, 0.5);
    // Babylon is left-handed; MMD models face -Z, so the camera looks along +Z.
    const camera = new FreeCamera("camera", new Vector3(0, 10, -100), scene);
    camera.setTarget(new Vector3(0, 10, 0));
    camera.mode = Camera.ORTHOGRAPHIC_CAMERA;
    camera.minZ = 1;
    camera.maxZ = 300;
    const sun = new DirectionalLight("sun", new Vector3(0.5, -1, 1), scene);
    sun.intensity = 0.9;
    const sky = new HemisphericLight("sky", new Vector3(0, 1, -1), scene);
    sky.intensity = 0.25;
    scene.onBeforeAnimationsObservable.add(() => this.beforeFrame(), undefined, true);
    return scene;
  }

  get loaded() {
    return this.mmd !== null;
  }

  /** Remembers the motion picked in the picker for the model `key` (its path). */
  private idleKey = "";

  async load(url: string, files: string[] = [], key = url): Promise<MotionSummary> {
    const id = ++this.loadId;
    if (!this.runtime) {
      // Single-threaded Bullet: no SharedArrayBuffer or worker setup needed.
      const wasm = await GetMmdWasmInstance(new MmdWasmInstanceTypeSPR());
      const physics = new MultiPhysicsRuntime(wasm);
      physics.setGravity(new Vector3(0, -98, 0));
      // One physics step per drawn frame, like MMD itself.
      physics.fixedTimeStep = 1 / FPS;
      physics.maxSubSteps = 2;
      physics.register(this.scene);
      this.physics = physics;
      this.runtime = new MmdRuntime(this.scene, new MmdBulletPhysics(physics));
      // Paused on purpose: beforeFrame() drives motions itself so it can loop them
      // and layer the head, breathing and face on top.
      this.runtime.register(this.scene);
    }
    const slash = url.lastIndexOf("/") + 1;
    const rootUrl = url.slice(0, slash);
    const fileName = url.slice(slash);
    const ext = /\.pmd$/i.test(decodeURIComponent(fileName)) ? ".pmd" : ".pmx";
    const container = await LoadAssetContainerAsync(fileName, this.scene, { rootUrl, pluginExtension: ext });
    if (id !== this.loadId || this.disposed) {
      container.dispose();
      throw new Error("superseded");
    }
    this.unloadModel();
    container.addAllToScene();
    const mesh = container.meshes[0] as Mesh;
    this.mesh = mesh;
    // createMmdModel trims the loader's metadata, so read what we need first.
    const meta = mesh.metadata as MmdModelMetadata;
    this.morphNames = [...new Set(meta.morphs.map((m) => m.name))];
    this.info = { name: meta.header.modelName, rigidBodies: meta.rigidBodies.length };
    this.mmd = this.runtime!.createMmdModel(mesh, { materialProxyConstructor: MmdStandardMaterialProxy });
    this.modelUrl = url;

    this.rig = faceRig(this.morphNames);
    this.bones = new Map(this.mmd.runtimeBones.map((b) => [b.name, b.linkedBone]));
    this.vmds = sortVmds(files);
    this.handles.clear();
    this.playing = null;
    this.idleKey = `mmd-idle:${key}`;
    this.pickedIdle = loadPickedIdle(this.idleKey, this.vmds);
    this.touched.clear();
    this.toggled.clear();
    this.emotion = null;
    this.actions = {
      expressions: this.morphNames.map((name) => ({ label: name, file: MORPH_PREFIX + name })),
      motions: [
        { label: "Built-in idle", group: BUILTIN_GROUP, index: 0, face: false },
        ...this.vmds.all.map((v, index) => ({ label: v.label, group: VMD_GROUP, index, face: false })),
      ],
    };

    // babylon-mmd pads skinned meshes' bounding boxes, so measure the rest-pose vertices.
    [this.minY, this.maxY] = restHeight(container.meshes as Mesh[]);
    this.place();
    await this.startIdle();
    // Start the hair and clothes from the first posed frame, not the T-pose.
    const mmd = this.mmd;
    this.scene.onAfterRenderObservable.addOnce(() => mmd.initializePhysics());

    const emotions: MotionSummary["emotions"] = {};
    for (const e of Object.keys({ ...this.rig.emotions, ...this.vmds.emotions }) as Emotion[]) emotions[e] = 1;
    return { idle: this.vmds.idle ? 1 : 0, react: this.vmds.all.length, emotions };
  }

  private unloadModel() {
    if (!this.mmd || !this.runtime) return;
    this.runtime.destroyMmdModel(this.mmd);
    this.mesh?.dispose(false, true);
    this.mmd = null;
    this.mesh = null;
  }

  dispose() {
    this.disposed = true;
    this.loadId++;
    window.removeEventListener("resize", this.onResize);
    this.engine.stopRenderLoop();
    this.unloadModel();
    this.runtime?.dispose(this.scene);
    this.physics?.dispose();
    this.scene.dispose();
    this.engine.dispose();
    this.canvas.style.removeProperty("mask-image");
    this.canvas.style.removeProperty("-webkit-mask-image");
  }

  // --- Framing ---------------------------------------------------------------

  place() {
    // The canvas may have been hidden (0x0) when the engine was made.
    this.engine.resize();
    const vw = this.canvas.clientWidth || window.innerWidth;
    const vh = this.canvas.clientHeight || window.innerHeight;
    const height = Math.max(1e-3, this.maxY - this.minY);
    const bottomPad = 8;
    const s = (Math.max(50, vh - this.topReserve - bottomPad) / height) * this.zoom;
    const bottom = this.minY - bottomPad / s;
    const top = bottom + vh / s;
    const cam = this.camera;
    cam.orthoLeft = -vw / 2 / s;
    cam.orthoRight = vw / 2 / s;
    cam.orthoBottom = bottom - (top + bottom) / 2;
    cam.orthoTop = top - (top + bottom) / 2;
    cam.position.set(0, (top + bottom) / 2, -100);
    cam.setTarget(new Vector3(0, (top + bottom) / 2, 0));
    this.pxPerUnit = s;
    this.topY = top;
    this.applyFade();
  }

  private yToScreen(y: number) {
    return (this.topY - y) * this.pxPerUnit;
  }

  setFade(on: boolean) {
    this.fadeBottom = on ? 0.16 : 0;
    this.applyFade();
  }

  private applyFade() {
    const style = this.canvas.style;
    if (this.fadeBottom <= 0 || !this.mesh) {
      style.removeProperty("mask-image");
      style.removeProperty("-webkit-mask-image");
      this.fadeEnd = Infinity;
      return;
    }
    const end = this.yToScreen(this.minY);
    const start = end - (this.maxY - this.minY) * this.pxPerUnit * this.fadeBottom;
    const mask = `linear-gradient(to bottom, #000 ${start}px, rgba(0,0,0,.75) ${start + (end - start) * 0.4}px, rgba(0,0,0,.3) ${start + (end - start) * 0.75}px, transparent ${end}px)`;
    style.setProperty("mask-image", mask);
    style.setProperty("-webkit-mask-image", mask);
    this.fadeEnd = start + (end - start) * 0.6;
  }

  headTop(): number | null {
    return this.mesh ? this.yToScreen(this.maxY) : null;
  }

  hitTest(x: number, y: number): boolean {
    if (!this.mesh) return false;
    if (y / (window.devicePixelRatio || 1) > this.fadeEnd) return false;
    const gl = (this.canvas.getContext("webgl2") ?? this.canvas.getContext("webgl")) as WebGLRenderingContext | null;
    if (!gl) return false;
    // Device pixels of the window map onto the canvas' drawing buffer.
    const r = gl.drawingBufferWidth / (this.canvas.clientWidth * (window.devicePixelRatio || 1) || 1);
    const bx = Math.floor(x * r);
    const by = Math.floor(y * r);
    const h = gl.drawingBufferHeight;
    if (bx < 0 || by < 0 || bx >= gl.drawingBufferWidth || by >= h) return false;
    const px = new Uint8Array(4);
    gl.readPixels(bx, h - 1 - by, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
    return px[3] > 16;
  }

  // --- Motions ---------------------------------------------------------------

  private async handleFor(file: VmdFile): Promise<MmdRuntimeAnimationHandle> {
    const known = this.handles.get(file.file);
    if (known) return known;
    const url = new URL(file.file.split("/").map(encodeURIComponent).join("/"), this.modelUrl).href;
    const animation = await new VmdLoader(this.scene).loadAsync(file.label, url);
    const handle = this.mmd!.createRuntimeAnimation(animation);
    this.handles.set(file.file, handle);
    return handle;
  }

  private async play(file: VmdFile | null, loop: boolean) {
    const mmd = this.mmd;
    if (!mmd) return;
    if (!file) {
      this.playing = null;
      mmd.setRuntimeAnimation(null);
      return;
    }
    const handle = await this.handleFor(file);
    if (this.mmd !== mmd) return;
    mmd.setRuntimeAnimation(handle);
    const endFrame = mmd.currentAnimation?.animation.endFrame ?? 0;
    this.playing = { file, handle, endFrame, loop, start: performance.now() };
  }

  private startIdle() {
    const idle = this.pickedIdle !== undefined ? this.pickedIdle : this.vmds.idle;
    return this.play(idle, true).catch((e) => console.warn("[mmd] idle motion failed", e));
  }

  /** Loops the picked motion (or the built-in idle) from now on, also next time. */
  playMotion(group: string, index: number) {
    if (group === BUILTIN_GROUP) this.pickedIdle = null;
    else if (group === VMD_GROUP && this.vmds.all[index]) this.pickedIdle = this.vmds.all[index];
    else return;
    try {
      localStorage.setItem(this.idleKey, this.pickedIdle ? this.pickedIdle.file : "");
    } catch {
      /* storage unavailable: just don't remember */
    }
    void this.startIdle();
  }

  emote(emotion: Emotion): boolean {
    const morphs = this.rig.emotions[emotion];
    const vmd = this.vmds.emotions[emotion];
    if (!this.mmd || (!morphs && !vmd)) return false;
    if (morphs) this.emotion = { morphs, start: performance.now() };
    if (vmd) void this.play(vmd, false).catch((e) => console.warn("[mmd] emotion motion failed", e));
    return true;
  }

  // --- Expressions (morph toggles) -------------------------------------------

  isExpressionOn(file: string) {
    return this.toggled.has(file.slice(MORPH_PREFIX.length));
  }

  async toggleExpression(file: string, on = !this.isExpressionOn(file)): Promise<boolean> {
    const name = file.slice(MORPH_PREFIX.length);
    if (!file.startsWith(MORPH_PREFIX) || !this.morphNames.includes(name)) return false;
    if (on) this.toggled.add(name);
    else this.toggled.delete(name);
    return on;
  }

  // --- Mouth, eyes, head -----------------------------------------------------

  setMouthSource(source: () => number) {
    this.mouthSource = source;
  }

  setFollowMouse(on: boolean) {
    this.followMouse = on;
    if (!on) this.lookTarget = { x: 0, y: 0 };
  }

  lookAt(x: number, y: number) {
    if (!this.followMouse || !this.mesh) return;
    const vw = this.canvas.clientWidth || window.innerWidth;
    const vh = this.canvas.clientHeight || window.innerHeight;
    const headY = this.yToScreen(this.maxY - (this.maxY - this.minY) * 0.07);
    const clamp = (v: number) => Math.max(-1, Math.min(1, v));
    this.lookTarget = { x: clamp((x - vw / 2) / (vw * 0.75)), y: clamp((y - headY) / (vh * 0.6)) };
  }

  /** Runs every frame before the MMD runtime solves bones, morphs and physics. */
  private beforeFrame() {
    const mmd = this.mmd;
    if (!mmd) return;
    const now = performance.now();
    const controlled = [BONES.upperBody, BONES.neck, BONES.head, BONES.eyes, BONES.leftArm, BONES.rightArm, BONES.leftElbow, BONES.rightElbow]
      .map((names) => findBone(this.bones, names))
      .filter((b): b is IMmdRuntimeLinkedBone => b !== null);
    const identity = Quaternion.Identity();
    for (const b of controlled) b.setRotationQuaternion(identity, Space.LOCAL);

    // 1. The motion, if any (looped by hand: the runtime itself stays paused).
    const p = this.playing;
    if (p && mmd.currentAnimation) {
      let frame = ((now - p.start) / 1000) * 30;
      if (frame >= p.endFrame) {
        if (p.loop) frame = p.endFrame > 0 ? frame % p.endFrame : 0;
        else {
          this.playing = null;
          void this.startIdle();
          frame = p.endFrame;
        }
      }
      mmd.currentAnimation.animate(frame);
    }

    // 2. Built-in idle: arms down, breathing and a slow sway.
    const t = now / 1000;
    const rotate = (names: readonly string[], x: number, y: number, z: number) => {
      const b = findBone(this.bones, names);
      if (!b) return;
      const q = b.rotationQuaternion.multiply(Quaternion.RotationYawPitchRoll(y, x, z));
      b.setRotationQuaternion(q, Space.LOCAL);
    };
    if (!this.playing) {
      rotate(BONES.leftArm, 0, 0, -ARMS_DOWN);
      rotate(BONES.rightArm, 0, 0, ARMS_DOWN);
      rotate(BONES.leftElbow, 0, -0.12, 0);
      rotate(BONES.rightElbow, 0, 0.12, 0);
      rotate(BONES.upperBody, Math.sin(t * 1.6) * 0.02, Math.sin(t * 0.37) * 0.03, Math.sin(t * 0.23) * 0.025);
    }

    // 3. Looking at the cursor, eased.
    const k = 0.12;
    this.look.x += (this.lookTarget.x - this.look.x) * k;
    this.look.y += (this.lookTarget.y - this.look.y) * k;
    // Signs checked on screen: the model faces the camera, so her left is screen right.
    const yaw = -this.look.x * 0.45;
    const pitch = -this.look.y * 0.3;
    // The body follows a little too: turns towards the cursor and leans to its side.
    rotate(BONES.upperBody, pitch * 0.25, yaw * 0.35, -this.look.x * 0.08);
    rotate(BONES.neck, pitch * 0.4, yaw * 0.4, 0);
    rotate(BONES.head, pitch * 0.6, yaw * 0.6, 0);
    rotate(BONES.eyes, pitch * 0.5, yaw * 0.6, 0);

    // 4. Face: toggled morphs, emotion, blinking and the talking mouth.
    const weights = new Map<string, number>();
    const add = (name: string | null, w: number) => {
      if (!name || w <= 0) return;
      weights.set(name, Math.min(1, Math.max(weights.get(name) ?? 0, w)));
    };
    for (const name of this.toggled) add(name, 1);
    if (this.emotion) {
      const e = envelope(now - this.emotion.start, EMOTE_MS);
      if (e <= 0 && now - this.emotion.start > EMOTE_MS) this.emotion = null;
      else for (const [name, w] of this.emotion.morphs) add(name, w * e);
    }
    if (now >= this.nextBlink && this.blinkStart < 0) this.blinkStart = now;
    if (this.blinkStart >= 0) {
      const w = blinkWeight(now - this.blinkStart);
      if (w <= 0 && now - this.blinkStart > 0) {
        this.blinkStart = -1;
        this.nextBlink = now + 2500 + Math.random() * 3500;
      } else add(this.rig.blink, w);
    }
    const v = this.mouthSource?.() ?? 0;
    if (v > 0 || this.mouthWasOpen) add(this.rig.mouth, v);
    this.mouthWasOpen = v > 0;

    for (const name of this.touched) if (!weights.has(name)) mmd.morph.setMorphWeight(name, 0);
    for (const [name, w] of weights) mmd.morph.setMorphWeight(name, w);
    this.touched = new Set(weights.keys());
  }

  diagnostics(): Promise<string> {
    if (!this.mmd || !this.mesh) return Promise.resolve("no model");
    const parts = [
      `mmd ${this.info.name}`,
      `bones ${this.bones.size}`,
      `morphs ${this.morphNames.length}`,
      `rigid bodies ${this.info.rigidBodies}`,
      `blink ${this.rig.blink}`,
      `mouth ${this.rig.mouth}`,
      `emotions ${Object.keys(this.rig.emotions).join(",")}`,
      `vmd ${this.vmds.all.length} (idle ${this.vmds.idle?.label ?? "built-in"})`,
      `height ${(this.maxY - this.minY).toFixed(1)}`,
    ];
    return Promise.resolve(parts.join("; "));
  }
}

/** Lowest and highest vertex Y of the model in its rest pose. */
function restHeight(meshes: Mesh[]): [number, number] {
  let min = Infinity;
  let max = -Infinity;
  for (const m of meshes) {
    const pos = m.getVerticesData?.("position");
    if (!pos) continue;
    for (let i = 1; i < pos.length; i += 3) {
      if (pos[i] < min) min = pos[i];
      if (pos[i] > max) max = pos[i];
    }
  }
  return min <= max ? [min, max] : [0, 20];
}

/** The idle picked last time: a motion, null (built-in idle) or undefined (none picked). */
function loadPickedIdle(key: string, vmds: VmdSet): VmdFile | null | undefined {
  let saved: string | null = null;
  try {
    saved = localStorage.getItem(key);
  } catch {
    return undefined;
  }
  if (saved === null) return undefined;
  if (saved === "") return null;
  return vmds.all.find((v) => v.file === saved);
}

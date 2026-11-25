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
import type { MmdAnimation } from "babylon-mmd/esm/Loader/Animation/mmdAnimation";
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
import * as host from "./host";
import { isChestBody, steadyPhysics } from "./mmd-physics";
import { upgradeVmd } from "./vmd";
import { builtinDancePose, type DanceMotion } from "./dance";
import { GESTURES, gesturePose, type FaceRole, type Gesture } from "./gestures";
import { BONES, blinkWeight, envelope, faceRig, findBone, sortVmds, type FaceRig, type VmdFile, type VmdSet } from "./mmd-face";
import type { Emotion, MotionSummary } from "./motions";
import type { ModelActions } from "./poses";

const EMOTE_MS = 3200;
const FPS = 30;
const VMD_GROUP = "vmd";
const BUILTIN_GROUP = "builtin";
const GESTURE_GROUP = "gesture";
const MORPH_PREFIX = "morph:";
/** A gesture asked for sooner than this after the last one is skipped. */
const GESTURE_GAP_MS = 1500;

/** Lowered-arm angle (radians) for models standing in the T-pose. */
const ARMS_DOWN = 0.62;

interface Playing {
  key: string;
  /** A dance: back to idle (and the music stops) when it ends. */
  dance: boolean;
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

  private rig: FaceRig = { blink: null, mouth: null, emotions: {}, roles: {} };
  private morphNames: string[] = [];
  private bones = new Map<string, IMmdRuntimeLinkedBone>();
  private vmds: VmdSet = { idle: null, emotions: {}, gestures: {}, all: [] };
  private handles = new Map<string, MmdRuntimeAnimationHandle>();
  private playing: Playing | null = null;
  /** When the built-in dance started, while it plays. */
  private builtinDance: number | null = null;
  private danceMusic: HTMLAudioElement | null = null;
  /** Cancels a dance still loading when something else starts. */
  private danceId = 0;
  /** How much the head and body follow the cursor (eased to 0 while dancing). */
  private lookWeight = 1;
  /** A motion picked in the picker; loops instead of the folder's idle. */
  private pickedIdle: VmdFile | null | undefined = undefined;
  private modelUrl = "";
  private info = { name: "", rigidBodies: 0, chest: [] as string[] };

  /** Model height range in world units (feet at minY). */
  private minY = 0;
  private maxY = 20;
  /** Pixels per world unit, and the world Y at the top of the window. */
  private pxPerUnit = 1;
  private topY = 20;

  private mouthSource: (() => number) | null = null;
  private mouthWasOpen = false;
  private emotion: { morphs: [string, number][]; start: number } | null = null;
  /** The built-in gesture playing, or one played from a .vmd (face still built in). */
  private gestureNow: { g: Gesture; start: number; motion: boolean } | null = null;
  private lastGesture = -Infinity;
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
      // Small steps (120 Hz, four per drawn frame): with one step per frame, fast
      // moves like dancing pulled hair and clothes apart before the joints caught up.
      physics.fixedTimeStep = 1 / 120;
      physics.maxSubSteps = 8;
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
    this.endDance();
    this.unloadModel();
    container.addAllToScene();
    const mesh = container.meshes[0] as Mesh;
    this.mesh = mesh;
    // createMmdModel trims the loader's metadata, so read what we need first.
    const meta = mesh.metadata as MmdModelMetadata;
    this.morphNames = [...new Set(meta.morphs.map((m) => m.name))];
    this.info = {
      name: meta.header.modelName,
      rigidBodies: meta.rigidBodies.length,
      chest: meta.rigidBodies.filter((b) => b.physicsMode !== 0 && isChestBody(b.name)).map((b) => b.name),
    };
    // Heavier, steadier hair, chest and clothes (read when the physics is built below).
    const steady = steadyPhysics(meta.rigidBodies, meta.joints);
    const writable = meta as { -readonly [K in "rigidBodies" | "joints"]: MmdModelMetadata[K] };
    writable.rigidBodies = steady.bodies;
    writable.joints = steady.joints;
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
    this.gestureNow = null;
    this.actions = {
      expressions: this.morphNames.map((name) => ({ label: name, file: MORPH_PREFIX + name })),
      motions: [
        { label: "Built-in idle", group: BUILTIN_GROUP, index: 0, face: false },
        ...this.vmds.all.map((v, index) => ({ label: v.label, group: VMD_GROUP, index, face: false })),
        // Each gesture once, built in or from its .vmd.
        ...GESTURES.map((g, index) => ({ label: `${g[0].toUpperCase()}${g.slice(1)}${this.vmds.gestures[g] ? "" : " (built-in)"}`, group: GESTURE_GROUP, index, face: false })),
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
    this.endDance();
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

  /** The model's handle for a motion made of `urls` (several .vmd files play as one). */
  private async handleFor(key: string, label: string, urls: string[]): Promise<MmdRuntimeAnimationHandle> {
    const known = this.handles.get(key);
    if (known) return known;
    const mmd = this.mmd!;
    // Fetched here (not by the loader) so old-format .vmd files can be upgraded first.
    const buffers = await Promise.all(
      urls.map(async (url) => {
        const res = await fetch(url);
        if (!res.ok) throw new Error(`HTTP ${res.status} for ${decodeURIComponent(url.split("/").pop() ?? url)}`);
        return upgradeVmd(await res.arrayBuffer());
      }),
    );
    const animation = await new Promise<MmdAnimation>((resolve, reject) =>
      new VmdLoader(this.scene).loadFromBuffer(label, buffers, resolve, undefined, reject),
    );
    if (this.mmd !== mmd) throw new Error("superseded");
    const handle = mmd.createRuntimeAnimation(animation);
    this.handles.set(key, handle);
    return handle;
  }

  private async play(file: VmdFile | null, loop: boolean) {
    const mmd = this.mmd;
    if (!mmd) return;
    if (!file) {
      this.playing = null;
      mmd.setRuntimeAnimation(null);
      resetState(mmd);
      return;
    }
    const url = new URL(file.file.split("/").map(encodeURIComponent).join("/"), this.modelUrl).href;
    const handle = await this.handleFor(file.file, file.label, [url]);
    if (this.mmd !== mmd) return;
    this.start(handle, file.file, loop, false);
  }

  private start(handle: MmdRuntimeAnimationHandle, key: string, loop: boolean, dance: boolean) {
    const mmd = this.mmd!;
    mmd.setRuntimeAnimation(handle);
    resetState(mmd);
    const endFrame = motionEnd(mmd.currentAnimation?.animation);
    this.playing = { key, dance, handle, endFrame, loop, start: performance.now() };
  }

  get canDance() {
    return this.mmd !== null;
  }

  get dancing() {
    return this.builtinDance !== null || this.playing?.dance === true;
  }

  /**
   * Plays a dance once (with its music), then goes back to the idle. With no
   * dance given she does the built-in one. Asking again while she dances does nothing.
   */
  async dance(motion: DanceMotion | null): Promise<boolean> {
    const mmd = this.mmd;
    if (!mmd) return false;
    if (this.dancing) return true;
    const id = ++this.danceId;
    if (!motion) {
      this.playing = null;
      mmd.setRuntimeAnimation(null);
      this.builtinDance = performance.now();
      return true;
    }
    const handle = await this.handleFor(`dance:${motion.urls.join("|")}`, motion.name, motion.urls);
    if (this.mmd !== mmd || id !== this.danceId) return false;
    let music: HTMLAudioElement | null = null;
    if (motion.audio) {
      music = new Audio(motion.audio);
      music.volume = 0.6;
      this.danceMusic = music;
      // Start the motion when the music really starts, so they stay together.
      await music.play().catch((e) => {
        console.warn("[mmd] dance music failed", e);
        music = null;
      });
      if (id !== this.danceId) return false;
    }
    this.start(handle, motion.name, false, true);
    host.log("info", `[mmd] dance ${motion.name}: ${(this.playing!.endFrame / 30).toFixed(1)} s of motion${music ? `, music ${music.duration.toFixed(1)} s` : ""}`);
    if (music) {
      // Follow the music's clock from now on; the motion's frames are 1/30 s.
      const m = music;
      const p = this.playing!;
      p.start = performance.now() - m.currentTime * 1000;
    }
    return true;
  }

  /** Stops a dance (and its music) and goes back to the idle. */
  stopDance() {
    if (!this.dancing && !this.danceMusic) return;
    this.endDance();
    void this.startIdle();
  }

  private endDance() {
    this.danceId++;
    if (this.builtinDance !== null) {
      this.builtinDance = null;
      this.resetCenter();
    }
    if (this.playing?.dance) this.playing = null;
    if (this.danceMusic) {
      // Fade out rather than cut: a motion often ends before its song does.
      const music = this.danceMusic;
      this.danceMusic = null;
      const from = music.volume;
      const started = performance.now();
      const fade = window.setInterval(() => {
        const k = 1 - (performance.now() - started) / 1500;
        if (k > 0) music.volume = from * k;
        else {
          window.clearInterval(fade);
          music.pause();
          music.removeAttribute("src");
        }
      }, 50);
    }
  }

  /** Puts the センター bone back where it rests (the built-in dance moves it). */
  private resetCenter() {
    const center = findBone(this.bones, BONES.center);
    if (!center) return;
    const rest = new Vector3();
    center.getRestMatrix().getTranslationToRef(rest);
    center.position = rest;
  }

  private startIdle() {
    const idle = this.pickedIdle !== undefined ? this.pickedIdle : this.vmds.idle;
    return this.play(idle, true).catch((e) => console.warn("[mmd] idle motion failed", e));
  }

  /** Loops the picked motion (or the built-in idle) from now on, also next time. */
  playMotion(group: string, index: number) {
    if (group === GESTURE_GROUP) {
      // Picked by hand: play it even if one just played.
      if (GESTURES[index]) {
        this.lastGesture = -Infinity;
        this.gestureNow = null;
        this.gesture(GESTURES[index]);
      }
      return;
    }
    this.endDance();
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
    // An emotion's motion would cut the dance short; the face still shows it.
    if (vmd && !this.dancing) void this.play(vmd, false).catch((e) => console.warn("[mmd] emotion motion failed", e));
    return true;
  }

  /**
   * A short emote: the model folder's .vmd for it if there is one, else the
   * built-in one from bones and face morphs. Not while dancing.
   */
  gesture(g: Gesture): boolean {
    if (!this.mmd || this.dancing) return false;
    const now = performance.now();
    // One at a time: a reply tagged [yawn] right after "can you yawn?" doesn't yawn twice.
    if (this.gestureNow || now - this.lastGesture < GESTURE_GAP_MS) return true;
    this.lastGesture = now;
    const vmd = this.vmds.gestures[g];
    // The face comes from the built-in gesture either way; a motion brings its own body.
    this.gestureNow = { g, start: now, motion: !!vmd };
    this.emotion = null;
    if (vmd) {
      void this.play(vmd, false).catch((e) => {
        console.warn("[mmd] gesture motion failed", e);
        // Fall back to the built-in body movement.
        if (this.gestureNow?.g === g) this.gestureNow.motion = false;
      });
    }
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
    const controlled = [
      BONES.upperBody,
      BONES.lowerBody,
      BONES.neck,
      BONES.head,
      BONES.eyes,
      BONES.leftArm,
      BONES.rightArm,
      BONES.leftElbow,
      BONES.rightElbow,
      BONES.leftShoulder,
      BONES.rightShoulder,
    ]
      .map((names) => findBone(this.bones, names))
      .filter((b): b is IMmdRuntimeLinkedBone => b !== null);
    const identity = Quaternion.Identity();
    for (const b of controlled) b.setRotationQuaternion(identity, Space.LOCAL);

    // 1. The motion, if any (looped by hand: the runtime itself stays paused).
    const p = this.playing;
    if (p && mmd.currentAnimation) {
      let frame = ((now - p.start) / 1000) * 30;
      let ended = false;
      if (frame >= p.endFrame) {
        if (p.loop) frame = p.endFrame > 0 ? frame % p.endFrame : 0;
        else ended = true;
      }
      if (ended) {
        if (p.dance) this.endDance();
        this.playing = null;
        // Resets the pose and whatever the motion switched (IK, morphs), then the idle takes over.
        void this.startIdle();
      } else mmd.currentAnimation.animate(frame);
    }

    // 2. Built-in idle: arms down, breathing and a slow sway.
    const t = now / 1000;
    const rotate = (names: readonly string[], x: number, y: number, z: number) => {
      const b = findBone(this.bones, names);
      if (!b) return;
      const q = b.rotationQuaternion.multiply(Quaternion.RotationYawPitchRoll(y, x, z));
      b.setRotationQuaternion(q, Space.LOCAL);
    };
    const dancePose = this.builtinDance !== null ? builtinDancePose(now - this.builtinDance, ARMS_DOWN) : null;
    if (this.builtinDance !== null && !dancePose) {
      this.builtinDance = null;
      this.resetCenter();
      void this.startIdle();
    }
    if (dancePose) {
      // 2b. The built-in dance, in place of the idle.
      rotate(BONES.upperBody, ...dancePose.upperBody);
      rotate(BONES.lowerBody, ...dancePose.lowerBody);
      rotate(BONES.head, ...dancePose.head);
      rotate(BONES.leftArm, ...dancePose.leftArm);
      rotate(BONES.rightArm, ...dancePose.rightArm);
      rotate(BONES.leftElbow, ...dancePose.leftElbow);
      rotate(BONES.rightElbow, ...dancePose.rightElbow);
      const center = findBone(this.bones, BONES.center);
      if (center) {
        const rest = new Vector3();
        center.getRestMatrix().getTranslationToRef(rest);
        rest.y -= dancePose.dip;
        center.position = rest;
      }
    } else if (!this.playing) {
      rotate(BONES.leftArm, 0, 0, -ARMS_DOWN);
      rotate(BONES.rightArm, 0, 0, ARMS_DOWN);
      rotate(BONES.leftElbow, 0, -0.12, 0);
      rotate(BONES.rightElbow, 0, 0.12, 0);
      rotate(BONES.upperBody, Math.sin(t * 1.6) * 0.02, Math.sin(t * 0.37) * 0.03, Math.sin(t * 0.23) * 0.025);
    }

    // 2c. A gesture (laugh, yawn…) on top of the idle or motion.
    let gesture = this.gestureNow && !this.dancing ? gesturePose(this.gestureNow.g, now - this.gestureNow.start) : null;
    if (this.gestureNow && (!gesture || this.dancing)) {
      if (!this.dancing && !this.playing && this.gestureNow.motion === false) this.resetCenter();
      this.gestureNow = null;
      gesture = null;
    }
    if (gesture && !this.gestureNow!.motion) {
      rotate(BONES.upperBody, ...gesture.upperBody);
      rotate(BONES.neck, ...gesture.neck);
      rotate(BONES.head, ...gesture.head);
      rotate(BONES.leftShoulder, 0, 0, gesture.shoulders);
      rotate(BONES.rightShoulder, 0, 0, -gesture.shoulders);
      rotate(BONES.leftArm, 0, 0, gesture.arms);
      rotate(BONES.rightArm, 0, 0, -gesture.arms);
      rotate(BONES.leftElbow, 0, -gesture.elbows, 0);
      rotate(BONES.rightElbow, 0, gesture.elbows, 0);
      if (!this.playing) {
        // A motion moves the センター bone itself; only dip her when none plays.
        const center = findBone(this.bones, BONES.center);
        if (center) {
          const rest = new Vector3();
          center.getRestMatrix().getTranslationToRef(rest);
          rest.y -= gesture.dip;
          center.position = rest;
        }
      }
    }

    // 3. Looking at the cursor, eased; not while dancing, the dance moves her head.
    const k = 0.12;
    this.look.x += (this.lookTarget.x - this.look.x) * k;
    this.look.y += (this.lookTarget.y - this.look.y) * k;
    this.lookWeight += ((this.dancing ? 0 : 1) - this.lookWeight) * 0.08;
    const lw = this.lookWeight;
    // Signs checked on screen: the model faces the camera, so her left is screen right.
    const yaw = -this.look.x * 0.45 * lw;
    const pitch = -this.look.y * 0.3 * lw;
    // The body follows a little too: turns towards the cursor and leans to its side.
    rotate(BONES.upperBody, pitch * 0.25, yaw * 0.35, -this.look.x * 0.08 * lw);
    // The hips follow just slightly; the leg IK keeps her feet where they are.
    rotate(BONES.lowerBody, 0, yaw * 0.12, -this.look.x * 0.03 * lw);
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
    if (gesture) {
      for (const [role, w] of Object.entries(gesture.face) as [FaceRole, number][]) add(this.rig.roles[role] ?? null, w);
    } else if (this.emotion) {
      const e = envelope(now - this.emotion.start, EMOTE_MS);
      if (e <= 0 && now - this.emotion.start > EMOTE_MS) this.emotion = null;
      else for (const [name, w] of this.emotion.morphs) add(name, w * e);
    }
    if (now >= this.nextBlink && this.blinkStart < 0) this.blinkStart = now;
    if (this.blinkStart >= 0 && !gesture?.face.eyesClosed) {
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
      `rigid bodies ${this.info.rigidBodies} (chest: ${this.info.chest.join(",") || "none"})`,
      `blink ${this.rig.blink}`,
      `mouth ${this.rig.mouth}`,
      `emotions ${Object.keys(this.rig.emotions).join(",")}`,
      `vmd ${this.vmds.all.length} (idle ${this.vmds.idle?.label ?? "built-in"})`,
      `dancing ${this.dancing}`,
      `gestures ${GESTURES.map((g) => (this.vmds.gestures[g] ? `${g}=vmd` : g)).join(",")}`,
      `height ${(this.maxY - this.minY).toFixed(1)}`,
    ];
    return Promise.resolve(parts.join("; "));
  }
}

/**
 * Undoes what a motion left behind besides the pose: morph weights, IK switched
 * off and rigid bodies switched to follow their bones. babylon-mmd only does this
 * itself while its runtime plays, and here the runtime stays paused.
 */
function resetState(mmd: MmdModel) {
  mmd.morph.resetMorphWeights();
  mmd.ikSolverStates.fill(1);
  mmd.rigidBodyStates.fill(1);
}

/**
 * The last frame that moves the model: bone and morph keyframes only. The IK
 * on/off and camera tracks some motions carry can run long past the dance, which
 * would leave her standing frozen until they end.
 */
function motionEnd(
  animation:
    | {
        endFrame: number;
        boneTracks?: readonly { endFrame: number }[];
        movableBoneTracks?: readonly { endFrame: number }[];
        morphTracks?: readonly { endFrame: number }[];
      }
    | undefined,
): number {
  if (!animation) return 0;
  let end = 0;
  for (const tracks of [animation.boneTracks, animation.movableBoneTracks, animation.morphTracks]) {
    for (const t of tracks ?? []) end = Math.max(end, t.endFrame);
  }
  return end > 0 ? Math.min(end, animation.endFrame) : animation.endFrame;
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

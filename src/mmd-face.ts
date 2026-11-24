// What an MMD (PMX/PMD) model can do, worked out from its morph names and the
// .vmd motion files in its folder. Kept free of Babylon so it can be unit tested.
//
// MMD models share a loose naming convention for morphs (Japanese names from
// the original MikuMikuDance sample models): まばたき blink, あ い う え お the
// vowels, 笑い smiling eyes, 怒り angry brows, and so on. English and Chinese
// names are listed as fallbacks for models that renamed them.
import { gestureFromFile, type FaceRole, type Gesture } from "./gestures";
import { classifyMotion, type Emotion } from "./motions";

/** A morph and how strongly to apply it (0..1). */
export type MorphWeight = [name: string, weight: number];

const BLINK = ["まばたき", "瞬き", "blink", "Blink", "眨眼"];
const MOUTH_OPEN = ["あ", "a", "A", "啊"];

// For each emotion: groups of alternatives. From each group the first morph the
// model has is used, so a model missing some still gets the rest.
const EMOTION_MORPHS: Record<Emotion, { names: string[]; weight: number }[]> = {
  happy: [
    { names: ["笑い", "smile", "Smile", "笑"], weight: 1 },
    { names: ["にっこり", "にやり", "ワ", "∧"], weight: 0.8 },
    { names: ["にこり"], weight: 0.8 },
  ],
  sad: [
    { names: ["悲しむ", "悲しい", "sad", "Sad"], weight: 0.9 },
    { names: ["困る", "困った"], weight: 1 },
    { names: ["口角下げ", "∧", "へ"], weight: 0.6 },
  ],
  angry: [
    { names: ["怒り", "angry", "Angry"], weight: 1 },
    { names: ["怒り目", "じと目"], weight: 0.7 },
    { names: ["へ", "口角下げ"], weight: 0.6 },
  ],
  surprised: [
    { names: ["びっくり", "驚き", "surprised", "Surprised"], weight: 1 },
    { names: ["びっくり左"], weight: 1 },
    { names: ["びっくり右"], weight: 1 },
    { names: ["上", "眉上"], weight: 0.8 },
    { names: ["お", "o", "O"], weight: 0.5 },
  ],
  shy: [
    { names: ["照れ", "赤面", "blush", "Blush", "脸红"], weight: 1 },
    { names: ["困る", "困った"], weight: 0.6 },
    { names: ["にこり"], weight: 0.5 },
    { names: ["笑い"], weight: 0.4 },
  ],
  thinking: [
    { names: ["真面目", "serious"], weight: 0.8 },
    { names: ["じと目", "ジト目"], weight: 0.5 },
    { names: ["口横狭め", "う"], weight: 0.3 },
  ],
  proud: [
    { names: ["にやり", "smirk"], weight: 1 },
    { names: ["じと目", "ジト目", "挑発する"], weight: 0.5 },
    { names: ["にこり"], weight: 0.6 },
  ],
};

// The morph for each face role the built-in gestures use (see gestures.ts).
const ROLE_MORPHS: Record<FaceRole, string[]> = {
  smile: ["笑い", "smile", "Smile", "笑"],
  grin: ["にっこり", "にこり", "ワ", "∧"],
  eyesClosed: ["まばたき", "瞬き", "blink", "Blink", "眨眼"],
  mouthA: MOUTH_OPEN,
  mouthO: ["お", "o", "O"],
  troubled: ["困る", "困った", "troubled"],
  surprised: ["びっくり", "驚き", "surprised", "Surprised"],
  blush: ["照れ", "赤面", "blush", "Blush", "脸红"],
  sad: ["悲しむ", "悲しい", "sad", "Sad"],
};

export interface FaceRig {
  blink: string | null;
  mouth: string | null;
  emotions: Partial<Record<Emotion, MorphWeight[]>>;
  /** The model's morph for each gesture face role it has. */
  roles: Partial<Record<FaceRole, string>>;
}

/** Picks the blink, mouth and emotion morphs a model has. */
export function faceRig(morphs: readonly string[]): FaceRig {
  const have = new Set(morphs);
  const first = (names: string[]) => names.find((n) => have.has(n)) ?? null;
  const emotions: FaceRig["emotions"] = {};
  for (const [emotion, groups] of Object.entries(EMOTION_MORPHS) as [Emotion, (typeof EMOTION_MORPHS)[Emotion]][]) {
    const picked: MorphWeight[] = [];
    for (const g of groups) {
      const name = first(g.names);
      if (name && !picked.some(([n]) => n === name)) picked.push([name, g.weight]);
    }
    if (picked.length) emotions[emotion] = picked;
  }
  const roles: FaceRig["roles"] = {};
  for (const [role, names] of Object.entries(ROLE_MORPHS) as [FaceRole, string[]][]) {
    const name = first(names);
    if (name) roles[role] = name;
  }
  return { blink: first(BLINK), mouth: first(MOUTH_OPEN), emotions, roles };
}

/**
 * How strongly an expression shows `t` ms after it started: eases in over
 * `fadeIn`, holds, and eases out over the last `fadeOut` of `duration`.
 */
export function envelope(t: number, duration: number, fadeIn = 250, fadeOut = 500): number {
  if (t <= 0 || t >= duration) return 0;
  const ease = (x: number) => x * x * (3 - 2 * x);
  if (t < fadeIn) return ease(t / fadeIn);
  if (t > duration - fadeOut) return ease((duration - t) / fadeOut);
  return 1;
}

/** Weight of a blink `t` ms into it: closes and reopens over `duration`. */
export function blinkWeight(t: number, duration = 160): number {
  if (t <= 0 || t >= duration) return 0;
  return Math.sin((t / duration) * Math.PI);
}

export interface VmdFile {
  file: string;
  label: string;
}

export interface VmdSet {
  /** Loops while nothing else plays. Null means the built-in breathing idle. */
  idle: VmdFile | null;
  /** Played once for an emotion, then back to idle. */
  emotions: Partial<Record<Emotion, VmdFile>>;
  /** Played once for a gesture (laugh.vmd, yawn.vmd…) in place of the built-in one. */
  gestures: Partial<Record<Gesture, VmdFile>>;
  /** Every motion, for the picker. */
  all: VmdFile[];
}

/** Names of a plain standing idle: 素立ち "just standing", 通常 "normal". */
const PLAIN_IDLE = /素立ち|通常|basic|default|normal|plain/i;

const byLabel = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

/** Sorts a model folder's .vmd files into an idle loop, emotions and the rest. */
export function sortVmds(files: readonly string[]): VmdSet {
  const all = files
    .filter((f) => /\.vmd$/i.test(f))
    .map((file) => {
      const base = file.split(/[\\/]/).pop() ?? file;
      return { file, label: base.replace(/\.vmd$/i, "").replace(/_/g, " ") };
    })
    .sort((a, b) => byLabel.compare(a.label, b.label));
  const idles: VmdFile[] = [];
  const emotions: VmdSet["emotions"] = {};
  const gestures: VmdSet["gestures"] = {};
  for (const v of all) {
    const g = gestureFromFile(v.file);
    if (g) gestures[g] ??= v;
    const kind = classifyMotion(v.file.replace(/\.vmd$/i, ""));
    if (kind.kind === "idle") idles.push(v);
    else if (kind.kind === "emotion") emotions[kind.emotion] ??= v;
  }
  // Idle packs come in styles (cool, cute, hands on hips…); start with the plain one.
  const idle = idles.find((v) => PLAIN_IDLE.test(v.label)) ?? idles[0] ?? null;
  return { idle, emotions, gestures, all };
}

/** Bone names of the standard MMD skeleton the app moves itself. */
export const BONES = {
  center: ["センター", "center", "Center"],
  upperBody: ["上半身", "upper body", "UpperBody"],
  lowerBody: ["下半身", "lower body", "LowerBody"],
  neck: ["首", "neck", "Neck"],
  head: ["頭", "head", "Head"],
  eyes: ["両目", "eyes", "Eyes"],
  leftArm: ["左腕", "arm_L", "LeftArm", "Left arm"],
  rightArm: ["右腕", "arm_R", "RightArm", "Right arm"],
  leftElbow: ["左ひじ", "elbow_L", "LeftElbow", "Left elbow"],
  rightElbow: ["右ひじ", "elbow_R", "RightElbow", "Right elbow"],
  leftShoulder: ["左肩", "shoulder_L", "LeftShoulder", "Left shoulder"],
  rightShoulder: ["右肩", "shoulder_R", "RightShoulder", "Right shoulder"],
} as const;

export function findBone<T>(byName: Map<string, T>, names: readonly string[]): T | null {
  for (const n of names) {
    const b = byName.get(n);
    if (b) return b;
  }
  return null;
}

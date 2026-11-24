// Sorts a model's motions into Idle and emotion groups.
//
// Many models (Aqua included) put every motion in one unnamed group, so the app
// can't rely on group names. Instead each motion is classified by its file name,
// e.g. "motions/00_Happy_01.motion3.json" -> happy, "00_Wait_01" -> idle.

export const EMOTIONS = [
  "happy",
  "sad",
  "angry",
  "surprised",
  "shy",
  "thinking",
  "proud",
] as const;
export type Emotion = (typeof EMOTIONS)[number];

export type MotionLayer = "body" | "face";
export type MotionKind =
  | { kind: "idle"; layer: MotionLayer }
  | { kind: "emotion"; emotion: Emotion; layer: MotionLayer }
  | { kind: "other"; layer: MotionLayer };

// Checked in order; first match wins. Includes the pinyin names some models use.
const RULES: [RegExp, "idle" | Emotion][] = [
  // 待機 "standby" and 立ち "standing" are the usual Japanese (MMD) names.
  [/wait|idle|stand|breath|loop|normal|daiji|待機|立ち/, "idle"],
  [/happy|smile|joy|laugh|glad|weixiao|kaixin/, "happy"],
  [/sad|cry|tear|upset|sorrow|kuqi|shangxin/, "sad"],
  [/anger|angry|mad|rage|shengqi/, "angry"],
  [/surprise|shock|amaze|jingya/, "surprised"],
  [/shame|shy|blush|embarrass|haixiu/, "shy"],
  [/puzzle|doubt|think|confus|serious|ponder/, "thinking"],
  [/pride|proud|smug|deyi/, "proud"],
];

const FACE = /expression|^face|^eye|^exp_/;

export function motionName(file: string): string {
  const base = file.split(/[\\/]/).pop() ?? file;
  return base.replace(/\.motion3\.json$/i, "").replace(/\.mtn$/i, "");
}

export function classifyMotion(file: string): MotionKind {
  // Strip numeric prefixes like "00_" / "20_" so they don't hide the name.
  const name = motionName(file).toLowerCase().replace(/^\d+[_-]/, "");
  const layer: MotionLayer = FACE.test(name) ? "face" : "body";
  for (const [re, label] of RULES) {
    if (re.test(name)) {
      return label === "idle" ? { kind: "idle", layer } : { kind: "emotion", emotion: label, layer };
    }
  }
  return { kind: "other", layer };
}

export interface MotionRef {
  File: string;
  [key: string]: unknown;
}

export interface ModelSettingsJson {
  FileReferences: { Motions?: Record<string, MotionRef[]>; [key: string]: unknown };
  [key: string]: unknown;
}

export const IDLE_GROUP = "Idle";
export const REACT_GROUP = "React";
export const emotionGroup = (e: Emotion) => `Emotion:${e}`;
export const faceGroup = (e: Emotion) => `Face:${e}`;

export interface MotionSummary {
  idle: number;
  react: number;
  emotions: Partial<Record<Emotion, number>>;
}

/**
 * Returns a copy of the model settings with extra motion groups added:
 * "Idle" (if the model has none), "Emotion:<name>" body motions,
 * "Face:<name>" face overlays, and "React" (everything else that isn't idle).
 * Original groups are kept unchanged.
 */
export function regroupMotions(settings: ModelSettingsJson): {
  settings: ModelSettingsJson;
  summary: MotionSummary;
} {
  const original = settings.FileReferences.Motions ?? {};
  const groups: Record<string, MotionRef[]> = {};
  for (const [name, list] of Object.entries(original)) groups[name] = [...list];

  const seen = new Map<string, MotionRef>();
  for (const list of Object.values(original)) for (const m of list) if (!seen.has(m.File)) seen.set(m.File, m);

  const add = (group: string, m: MotionRef) => {
    const list = (groups[group] ??= []);
    if (!list.some((x) => x.File === m.File)) list.push(m);
  };

  const hasIdle = (original[IDLE_GROUP]?.length ?? 0) > 0;
  const idleFiles = new Set((original[IDLE_GROUP] ?? []).map((m) => m.File));

  for (const m of seen.values()) {
    const c = classifyMotion(m.File);
    if (c.kind === "idle") {
      if (!hasIdle && c.layer === "body") add(IDLE_GROUP, m);
      continue;
    }
    if (idleFiles.has(m.File)) continue;
    if (c.kind === "emotion") add(c.layer === "face" ? faceGroup(c.emotion) : emotionGroup(c.emotion), m);
    else if (c.layer === "body") add(REACT_GROUP, m);
  }

  const summary: MotionSummary = {
    idle: groups[IDLE_GROUP]?.length ?? 0,
    react: groups[REACT_GROUP]?.length ?? 0,
    emotions: {},
  };
  for (const e of EMOTIONS) {
    const n = groups[emotionGroup(e)]?.length;
    if (n) summary.emotions[e] = n;
  }

  return {
    settings: { ...settings, FileReferences: { ...settings.FileReferences, Motions: groups } },
    summary,
  };
}

/** Group to play for an emotion: its body motions, else any reaction motion. */
export function groupForEmotion(groups: Record<string, unknown[]>, e: Emotion): string | null {
  if (groups[emotionGroup(e)]?.length) return emotionGroup(e);
  if (groups[REACT_GROUP]?.length) return REACT_GROUP;
  return null;
}

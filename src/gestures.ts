// Short emotes ("gestures"): laughing, giggling, yawning, sighing, blushing and
// gasping. MMD models play them from their bones and face morphs; a .vmd named
// after one (laugh.vmd, yawn.vmd…) in the model's folder plays instead. Live2D
// models show the nearest emotion.
//
// They start from a reply's tags ([laugh], [yawn]…), from cues in what she says
// ("haha", "*sigh*"), or when the user asks ("can you yawn?").
// Kept free of Babylon so it can be unit tested.
import type { Emotion } from "./motions";

export const GESTURES = ["laugh", "giggle", "yawn", "sigh", "blush", "gasp"] as const;
export type Gesture = (typeof GESTURES)[number];

/** The emotion a model without gestures (Live2D) shows instead. */
export const GESTURE_EMOTION: Record<Gesture, Emotion | null> = {
  laugh: "happy",
  giggle: "happy",
  yawn: null,
  sigh: "sad",
  blush: "shy",
  gasp: "surprised",
};

/** How long each built-in gesture lasts (ms). */
export const GESTURE_MS: Record<Gesture, number> = {
  laugh: 2600,
  giggle: 1800,
  yawn: 3600,
  sigh: 2800,
  blush: 3000,
  gasp: 1600,
};

// --- Spotting them ------------------------------------------------------------

/** Tag words (as in "[laugh]") for each gesture. */
const TAGS: Record<string, Gesture> = {
  laugh: "laugh",
  laughing: "laugh",
  laughs: "laugh",
  giggle: "giggle",
  giggles: "giggle",
  giggling: "giggle",
  chuckle: "giggle",
  yawn: "yawn",
  yawns: "yawn",
  yawning: "yawn",
  sleepy: "yawn",
  sigh: "sigh",
  sighs: "sigh",
  blush: "blush",
  blushes: "blush",
  blushing: "blush",
  gasp: "gasp",
  gasps: "gasp",
};

/** The gesture a tag word stands for ("Laughs" -> laugh), or null. */
export function gestureTag(word: string): Gesture | null {
  return TAGS[word.trim().toLowerCase()] ?? null;
}

// Cues in what she says. Stage directions (*sighs*, (yawns)) and the sounds themselves.
const CUES: [RegExp, Gesture][] = [
  [/[*(~]\s*(?:laugh|laughs|laughing|lol)\s*[*)~]|(?<![\p{L}])(?:ha){2,}h?(?![\p{L}])|(?<![\p{L}])(?:lol|lmao|rofl)(?![\p{L}])|笑笑|哈哈/iu, "laugh"],
  [/[*(~]\s*(?:giggle|giggles|giggling|chuckle|chuckles|snicker|snickers)\s*[*)~]|(?<![\p{L}])(?:he){2,}(?![\p{L}])|(?<![\p{L}])tee+[ -]?hee+(?![\p{L}])|(?<![\p{L}])fufu+(?![\p{L}])|ふふ|くすくす|嘻嘻/iu, "giggle"],
  [/[*(~]\s*(?:yawn|yawns|yawning)\s*[*)~]|(?<![\p{L}])ya+w+n+(?![\p{L}])|ふぁ[〜ー~]*あ|あくび/iu, "yawn"],
  [/[*(~]\s*(?:sigh|sighs|sighing)\s*[*)~]|^\s*(?:sigh|haa+h*|ha+h*\.{2,})(?![\p{L}])|はぁ|ため息|唉/iu, "sigh"],
  [/[*(~]\s*(?:blush|blushes|blushing)\s*[*)~]|\/\/\/+|照れ/iu, "blush"],
  [/[*(~]\s*(?:gasp|gasps|gasping)\s*[*)~]|(?<![\p{L}])(?:gasp|whoa+|omg)(?![\p{L}])[!?]?|えっ|ええっ/iu, "gasp"],
];

/** The first gesture cue in a line she says ("Haha, true!" -> laugh), or null. */
export function gestureCue(text: string): Gesture | null {
  let best: { at: number; g: Gesture } | null = null;
  for (const [re, g] of CUES) {
    const m = re.exec(text);
    if (m && (!best || m.index < best.at)) best = { at: m.index, g };
  }
  return best?.g ?? null;
}

const VERBS = "laugh|giggle|yawn|sigh|blush|gasp";
const ASK = new RegExp(
  [
    // "laugh", "yawn for me", "Nicole, giggle!"
    `^(?:(?:hey|ok|okay|now|so|come on|go on)[, ]+)*(?:(?!(?:i|we|they|you|she|he) )\\w+[, ]+)?(?:please[, ]+)?(${VERBS})(?: for (?:me|us)| again| please)?[.!?]*$`,
    // "can you yawn?", "could you please laugh", "wanna giggle?"
    `\\b(?:can|could|will|would|do|wanna|want to) (?:you )?(?:please )?(?:do (?:a|your) (?:little )?)?(${VERBS})\\b(?! (?:at|about))`,
    // "do a yawn", "give me a giggle", "show me your laugh"
    `\\b(?:do|give (?:me|us)|show (?:me|us)) (?:a|an|your|that|another) (?:little |big |cute )?(${VERBS})\\b`,
    // "make her laugh" is not a request to her, so only "make me" style is left out.
    `\\b(${VERBS}) (?:for (?:me|us)|again)\\b`,
  ].join("|"),
);

/** The gesture the user asks her to do ("can you yawn?"), or null. Talk about it is left alone. */
export function gestureIntent(text: string): Gesture | null {
  const t = text
    .toLowerCase()
    .replace(/[’`]/g, "'")
    .replace(/\s+/g, " ")
    .trim();
  if (/\b(?:don'?t|do not|never|stop|no more)\b/.test(t)) return null;
  const m = ASK.exec(t);
  if (!m) return null;
  const verb = m.slice(1).find(Boolean);
  return verb ? gestureTag(verb) : null;
}

// --- The built-in gestures ---------------------------------------------------

export type Euler = [x: number, y: number, z: number];

/** Face morph roles; mmd-face.ts finds the model's morph for each. */
export type FaceRole = "smile" | "grin" | "eyesClosed" | "mouthA" | "mouthO" | "troubled" | "surprised" | "blush" | "sad";

/**
 * A gesture's pose, added on top of whatever she is doing. Bone rotations are
 * in radians: X < 0 bows the head or leans forward, X > 0 tips it back; a
 * shoulder's Z > 0 raises it (the right side mirrors in mmd.ts).
 */
export interface GesturePose {
  upperBody: Euler;
  neck: Euler;
  head: Euler;
  shoulders: number;
  /** Raises both arms a little (Z, mirrored for the right arm). */
  arms: number;
  elbows: number;
  /** How far the hips dip (world units). */
  dip: number;
  face: Partial<Record<FaceRole, number>>;
  /** 0..1 blend in and out; the face's emotion pauses while this is above 0. */
  weight: number;
}

const ease = (x: number) => (x <= 0 ? 0 : x >= 1 ? 1 : x * x * (3 - 2 * x));
/** 0 -> 1 over [a, b], held, then 1 -> 0 over [c, d]. */
const window4 = (t: number, a: number, b: number, c: number, d: number) => (t < b ? ease((t - a) / (b - a)) : t < c ? 1 : 1 - ease((t - c) / (d - c)));

const ZERO: Euler = [0, 0, 0];

/** The built-in `g` pose `ms` into it, or null once it's over. */
export function gesturePose(g: Gesture, ms: number): GesturePose | null {
  const total = GESTURE_MS[g];
  if (ms < 0 || ms >= total) return null;
  const t = ms / total; // 0..1
  const s = ms / 1000;
  const weight = window4(ms, 0, 200, total - 350, total);
  let p: Omit<GesturePose, "weight">;
  switch (g) {
    case "laugh": {
      // Head tips back, shoulders shake with each "ha", eyes squeeze shut.
      const shake = Math.abs(Math.sin(s * Math.PI * 5.5)) * window4(ms, 150, 400, total - 700, total - 250);
      p = {
        upperBody: [0.06 - shake * 0.05, 0, Math.sin(s * 2.2) * 0.04],
        neck: [0.06, 0, 0],
        head: [0.12 + shake * 0.04, 0, 0.08],
        shoulders: shake * 0.09,
        arms: 0,
        elbows: 0.15,
        dip: shake * 0.12,
        face: { smile: 1, grin: 0.6, mouthA: 0.35 + shake * 0.45 },
      };
      break;
    }
    case "giggle": {
      // Small and quick: head ducks and tilts, shoulders bounce up, a closed smile.
      const bounce = Math.abs(Math.sin(s * Math.PI * 7)) * window4(ms, 100, 300, total - 500, total - 200);
      p = {
        upperBody: [-0.05, 0, 0.05],
        neck: [-0.05, 0, 0],
        head: [-0.1, 0.1, 0.16],
        shoulders: 0.06 + bounce * 0.07,
        arms: 0,
        elbows: 0.2,
        dip: bounce * 0.04,
        face: { smile: 0.8, grin: 0.4, blush: 0.4, mouthA: bounce * 0.15 },
      };
      break;
    }
    case "yawn": {
      // Breathes in with the head back and mouth wide, a little stretch, then droops.
      const open = window4(t, 0.1, 0.35, 0.65, 0.85);
      const droop = window4(t, 0.7, 0.85, 0.92, 1);
      p = {
        upperBody: [0.1 * open - 0.05 * droop, 0, 0],
        neck: [0.08 * open, 0, 0],
        head: [0.25 * open - 0.1 * droop, 0, 0.1 * open],
        shoulders: 0.14 * open,
        arms: 0.3 * open,
        elbows: 0.5 * open,
        dip: 0,
        face: { mouthA: open, mouthO: open * 0.4, eyesClosed: Math.max(open * 0.9, droop * 0.5), troubled: open * 0.5 },
      };
      break;
    }
    case "sigh": {
      // A breath in (chest and shoulders up), then a long breath out as she slumps.
      const inhale = window4(t, 0.05, 0.3, 0.3, 0.55);
      const slump = window4(t, 0.35, 0.6, 0.85, 1);
      p = {
        upperBody: [0.05 * inhale - 0.1 * slump, 0, 0],
        neck: [-0.06 * slump, 0, 0],
        head: [0.04 * inhale - 0.16 * slump, 0, 0.06 * slump],
        shoulders: 0.13 * inhale - 0.05 * slump,
        arms: 0,
        elbows: 0,
        dip: 0.08 * slump,
        face: { troubled: 0.8 * Math.max(inhale, slump), eyesClosed: 0.6 * slump, mouthO: 0.25 * slump },
      };
      break;
    }
    case "blush": {
      // Looks down and away, sways a little, cheeks red.
      const sway = Math.sin(s * Math.PI * 1.6);
      p = {
        upperBody: [-0.04, 0.05 * sway, 0.04 * sway],
        neck: [-0.08, 0, 0],
        head: [-0.18, -0.2, 0.12],
        shoulders: 0.08,
        arms: 0,
        elbows: 0.35,
        dip: 0,
        face: { blush: 1, troubled: 0.4, smile: 0.4 },
      };
      break;
    }
    case "gasp": {
      // A sharp breath in: jerks back, shoulders up, eyes and mouth wide.
      const jolt = window4(ms, 0, 120, 600, total);
      p = {
        upperBody: [0.1 * jolt, 0, 0],
        neck: [0.05 * jolt, 0, 0],
        head: [0.12 * jolt, 0, 0],
        shoulders: 0.15 * jolt,
        arms: 0.12 * jolt,
        elbows: 0.3 * jolt,
        dip: 0,
        face: { surprised: jolt, mouthO: 0.7 * jolt },
      };
      break;
    }
  }
  const scale = (e: Euler): Euler => (e === ZERO ? e : [e[0] * weight, e[1] * weight, e[2] * weight]);
  const face: GesturePose["face"] = {};
  for (const [k, v] of Object.entries(p.face) as [FaceRole, number][]) face[k] = Math.max(0, Math.min(1, v * weight));
  return {
    upperBody: scale(p.upperBody),
    neck: scale(p.neck),
    head: scale(p.head),
    shoulders: p.shoulders * weight,
    arms: p.arms * weight,
    elbows: p.elbows * weight,
    dip: p.dip * weight,
    face,
    weight,
  };
}

/** A motion file's gesture from its name ("laugh.vmd", "あくび.vmd"), or null. */
export function gestureFromFile(file: string): Gesture | null {
  const name = (file.split(/[\\/]/).pop() ?? file).replace(/\.vmd$/i, "").toLowerCase();
  if (/giggle|chuckle|くすくす|クスクス/.test(name)) return "giggle";
  if (/laugh|笑い|爆笑/.test(name)) return "laugh";
  if (/yawn|あくび|欠伸/.test(name)) return "yawn";
  if (/sigh|ため息|溜息|溜め息/.test(name)) return "sigh";
  if (/blush|照れ|赤面/.test(name)) return "blush";
  if (/gasp|驚き|びっくり/.test(name)) return "gasp";
  return null;
}

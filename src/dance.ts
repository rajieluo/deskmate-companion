// Dancing on request: spotting "dance" in what the user says, picking a dance
// from the dances folder, and the built-in dance for when the folder is empty.
// Kept free of Babylon so it can be unit tested.

/** A dance from the dances folder (see dances.rs). */
export interface DanceInfo {
  name: string;
  /** Paths relative to the dances folder. */
  motions: string[];
  audio: string | null;
}

/** A dance ready to play: its files as URLs. */
export interface DanceMotion {
  name: string;
  urls: string[];
  audio: string | null;
}

export type DanceIntent = { kind: "dance"; name: string | null } | { kind: "stop" } | null;

const DANCE_WORD = /\b(danc(e|es|ing)|boogie|groove|bust a move)\b|踊|ダンス|跳舞/;
const NEGATED = /\b(don'?t|do not|never|no more|stop|quit|enough|please no)\b[^.!?]{0,20}\b(danc|boogie|groove)/;
const STOP = /\b(stop|quit|enough|end|no more)\b[^.!?]{0,12}\b(danc\w*|boogie\w*|the music)\b|^(ok(ay)? )?stop( it| now)?$/;

// Ways of asking for a dance. Talking *about* dancing ("I went dancing") is left alone.
const ASKS = [
  // "dance", "please dance", "Nicole, dance!", "ok now dance"
  /^(?:(?:hey|ok|okay|now|so|alright|come on|go on)[, ]+)*(?:(?!(?:i|we|they|you|she|he|people|kids) )\w+[, ]+)?(?:please[, ]+)?(?:dance|boogie)\b/,
  // "can you dance?", "could you please do a dance", "wanna dance?"
  /\b(?:can|could|will|would|won'?t|do|wanna|want to|gonna|care to) (?:you )?(?:please )?(?:do (?:a|some|me a|us a) (?:little )?|show (?:me|us) (?:a|some|your) |)(?:danc(?:e|ing)|boogie)\b/,
  // "do a dance", "show me your dance", "do the Lamb dance", "give us a little dance"
  /\b(?:do|show (?:me|us)|give (?:me|us)|perform|play|start|try) (?:a|an|the|your|some|me a|us a|that|another)?\s*(?:[\w'-]+ ){0,4}(?:dance|dancing)\b/,
  /\blet'?s (?:dance|boogie)\b/,
  /\btime to (?:dance|boogie)\b/,
  /\b(?:dance|boogie) (?:for (?:me|us)|please|now|again|with me|to)\b/,
  /\b(?:start|keep|go) dancing\b/,
  /\bgo (?:dance|boogie)\b/,
  /\b(?:bust a move|groove for me)\b/,
  /踊って|ダンスして|跳舞/,
];

/** Lower case, plain spaces, only letters, digits and a little punctuation. */
function normalize(text: string): string {
  return text
    .toLowerCase()
    .replace(/[’`]/g, "'")
    .replace(/[_]+/g, " ")
    .replace(/[^\p{L}\p{N}' ,.!?-]+/gu, " ")
    .replace(/\s+/g, " ")
    .trim();
}

/** True if `name` (a dance's name) appears in `text` (normalized) as whole words. */
function mentions(text: string, name: string): boolean {
  const n = normalize(name).replace(/[,.!?]+/g, " ").trim();
  if (n.length < 2) return false;
  const esc = n.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`(^|[^\\p{L}\\p{N}])${esc}($|[^\\p{L}\\p{N}])`, "u").test(text);
}

/** Whether `text` asks her to dance (optionally one of `names`) or to stop dancing. */
export function danceIntent(text: string, names: readonly string[] = []): DanceIntent {
  const t = normalize(text);
  if (!t) return null;
  if (STOP.test(t.replace(/[.!?]+$/, ""))) return DANCE_WORD.test(t) || /^(ok(ay)? )?stop/.test(t) ? { kind: "stop" } : null;
  if (!DANCE_WORD.test(t) || NEGATED.test(t)) return null;
  if (!ASKS.some((re) => re.test(t))) return null;
  // The longest name that matches, so "Lamb (short)" wins over "Lamb".
  const name = [...names].sort((a, b) => b.length - a.length).find((n) => mentions(t, n)) ?? null;
  return { kind: "dance", name };
}

/** The dance called `name`, else a random one (not `last` if there's a choice); null if there are none. */
export function pickDance(dances: readonly DanceInfo[], name: string | null, last: string | null = null, random = Math.random): DanceInfo | null {
  if (!dances.length) return null;
  if (name) {
    const hit = dances.find((d) => d.name.toLowerCase() === name.toLowerCase());
    if (hit) return hit;
  }
  const pool = dances.length > 1 ? dances.filter((d) => d.name !== last) : dances;
  return pool[Math.floor(random() * pool.length) % pool.length];
}

// --- The built-in dance ------------------------------------------------------

/** How long the built-in dance lasts (ms): three 8-beat phrases at 120 bpm. */
export const BUILTIN_DANCE_MS = 12000;
const BEAT = 0.5; // seconds

export type Euler = [x: number, y: number, z: number];

export interface DancePose {
  upperBody: Euler;
  lowerBody: Euler;
  head: Euler;
  /** Rotations around Z raise (+) or lower (-) the left arm; the right arm mirrors. */
  leftArm: Euler;
  rightArm: Euler;
  leftElbow: Euler;
  rightElbow: Euler;
  /** How far the hips dip (world units, >= 0); the leg IK bends the knees. */
  dip: number;
  /** 0..1: blends in at the start and out at the end. */
  weight: number;
}

const ease = (x: number) => x * x * (3 - 2 * x);

/** The built-in dance's pose `ms` into it, or null once it's over. */
export function builtinDancePose(ms: number, armsDown: number): DancePose | null {
  if (ms < 0 || ms >= BUILTIN_DANCE_MS) return null;
  const t = ms / 1000;
  const beat = t / BEAT;
  const phase = Math.min(2, Math.floor(t / 4));
  const s = Math.sin(beat * Math.PI); // one side per beat
  const bounce = Math.abs(Math.sin(beat * Math.PI)); // a dip on every beat
  const weight = ms < 500 ? ease(ms / 500) : ms > BUILTIN_DANCE_MS - 700 ? ease((BUILTIN_DANCE_MS - ms) / 700) : 1;
  let pose: Omit<DancePose, "weight">;
  if (phase === 0) {
    // Hip sway with the arms waving overhead in turn.
    pose = {
      upperBody: [0, 0, s * 0.08],
      lowerBody: [0, 0, -s * 0.1],
      head: [bounce * 0.08, 0, -s * 0.1],
      leftArm: [0, 0, 0.55 + 0.35 * s],
      rightArm: [0, 0, -(0.55 - 0.35 * s)],
      leftElbow: [0, -0.5, 0],
      rightElbow: [0, 0.5, 0],
      dip: bounce * 0.25,
    };
  } else if (phase === 1) {
    // Twisting with the arms pumping at the sides.
    const pump = Math.sin(beat * Math.PI * 2);
    pose = {
      upperBody: [0.05, s * 0.25, 0],
      lowerBody: [0, -s * 0.1, 0],
      head: [bounce * 0.1, -s * 0.15, 0],
      leftArm: [0, 0.2, -0.25 + pump * 0.2],
      rightArm: [0, -0.2, 0.25 - pump * 0.2],
      leftElbow: [0, -1.1 - pump * 0.3, 0],
      rightElbow: [0, 1.1 + pump * 0.3, 0],
      dip: bounce * 0.35,
    };
  } else {
    // Both arms up, swaying side to side, bouncing.
    pose = {
      upperBody: [0, 0, s * 0.12],
      lowerBody: [0, 0, -s * 0.06],
      head: [0.05, 0, s * 0.15],
      leftArm: [0, 0, 1.0 + s * 0.15],
      rightArm: [0, 0, -(1.0 - s * 0.15)],
      leftElbow: [0, -0.35 + s * 0.2, 0],
      rightElbow: [0, 0.35 + s * 0.2, 0],
      dip: bounce * 0.4,
    };
  }
  // Blend from the lowered-arm rest pose at the start and the end.
  const mix = (a: number, b: number) => a + (b - a) * weight;
  const scale = (e: Euler): Euler => [e[0] * weight, e[1] * weight, e[2] * weight];
  return {
    upperBody: scale(pose.upperBody),
    lowerBody: scale(pose.lowerBody),
    head: scale(pose.head),
    leftArm: [pose.leftArm[0] * weight, pose.leftArm[1] * weight, mix(-armsDown, pose.leftArm[2])],
    rightArm: [pose.rightArm[0] * weight, pose.rightArm[1] * weight, mix(armsDown, pose.rightArm[2])],
    leftElbow: [0, mix(-0.12, pose.leftElbow[1]), 0],
    rightElbow: [0, mix(0.12, pose.rightElbow[1]), 0],
    dip: pose.dip * weight,
    weight,
  };
}

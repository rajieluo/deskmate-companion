// Splits streamed text into sentences so each one can be spoken as soon as it's complete.
import type { Emotion } from "./motions";

const END = /([.!?…]+["')\]]*)(\s+)|(\n+)/g;
const MIN_CHARS = 12;

export class SentenceBuffer {
  private buf = "";

  /** Adds streamed text; returns any sentences that are now complete. */
  push(text: string): string[] {
    this.buf += text;
    const out: string[] = [];
    let start = 0;
    END.lastIndex = 0;
    let m: RegExpExecArray | null;
    while ((m = END.exec(this.buf))) {
      const end = m.index + (m[1]?.length ?? 0);
      const sentence = this.buf.slice(start, end).trim();
      // Very short pieces ("Oh!") are joined to the next sentence for smoother speech.
      if (sentence.length >= MIN_CHARS || m[3]) {
        if (sentence) out.push(sentence);
        start = m.index + m[0].length;
      }
    }
    this.buf = this.buf.slice(start);
    return out;
  }

  /** Returns whatever is left (call when the reply ends). */
  flush(): string[] {
    const rest = this.buf.trim();
    this.buf = "";
    return rest ? [rest] : [];
  }
}

// "hehe", "hehehe"... as a whole word: Kokoro reads these as one short "heh".
const LAUGH = /(?<![\p{L}\p{N}'])(?:he){2,}(?![\p{L}\p{N}'])/giu;

/** Spells a laugh the way it should sound: "Hehe" -> "Hee hee". */
function laugh(word: string): string {
  const out = Array(word.length / 2).fill("hee").join(" ");
  return word[0] === "H" ? "H" + out.slice(1) : out;
}

// A roleplay action like "*yawns*" or "*tilts head*": one asterisk each side, one line.
// "**bold**" is left alone (the lookarounds reject a doubled asterisk).
const ACTION = /(?<!\*)\*(?![\s*])([^*\n]{1,40}?)(?<![\s*])\*(?!\*)/g;
const MAX_ACTION_WORDS = 4;
// A few actions are spoken as sound words the voice engines say naturally; the rest stay silent.
const SOUNDS: [RegExp, string][] = [
  [/^yawn(s|ing)?$/i, "Haaaah..."],
  [/^sigh(s|ing)?$/i, "Haah..."],
  [/^giggl(e|es|ing)$/i, "Hee hee"],
  [/^laugh(s|ing)?$/i, "Ha hah!"],
];

function actionSound(inner: string): string {
  return SOUNDS.find(([re]) => re.test(inner.trim()))?.[1] ?? "";
}

function isAction(inner: string): boolean {
  return inner.split(/\s+/).length <= MAX_ACTION_WORDS;
}

// Actions she can act out with an existing mood (alongside any sound word above).
const ACTION_MOODS: [RegExp, Emotion][] = [
  [/\b(giggl|laugh|chuckl|smil|grin|beam|cheer)/i, "happy"],
  [/\b(sigh|sob|cr(y|ies)|sniff|pout|frown|whimper)/i, "sad"],
  [/\b(huff|growl|glar|scowl|grumbl|stomp)/i, "angry"],
  [/\b(gasp|startl)/i, "surprised"],
  [/\b(blush|fidget)/i, "shy"],
  [/\b(ponder|think|hmm|tilts? (her )?head)/i, "thinking"],
  [/\b(smirk|wink)/i, "proud"],
];

/** Moods for the actions in a sentence ("*giggles*" -> happy), so she acts them out instead of saying them. */
export function actionMoods(text: string): Emotion[] {
  const out: Emotion[] = [];
  for (const m of text.matchAll(ACTION)) {
    if (!isAction(m[1])) continue;
    const hit = ACTION_MOODS.find(([re]) => re.test(m[1]));
    if (hit && !out.includes(hit[1])) out.push(hit[1]);
  }
  return out;
}

/** Removes things that sound bad read aloud (URLs, code, emoji, markdown symbols, *actions*). */
export function speakable(text: string): string {
  const out = text
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`([^`]*)`/g, "$1")
    .replace(/\[([^\]\n]+)\]\(https?:\/\/[^\s)]+\)/g, "$1")
    .replace(/https?:\/\/\S+/g, "a link")
    .replace(ACTION, (whole, inner: string) => (isAction(inner) ? ` ${actionSound(inner)} ` : whole))
    .replace(/[*_#>~]+/g, "")
    .replace(/\p{Extended_Pictographic}/gu, "")
    .replace(LAUGH, laugh)
    .replace(/\s+/g, " ")
    .replace(/\s+([,.!?…;:])/g, "$1")
    .replace(/^[\s,.!?…;:]+/, "")
    .trim();
  // Nothing left but punctuation (the reply was only an action): say nothing.
  return /[\p{L}\p{N}]/u.test(out) ? out : "";
}

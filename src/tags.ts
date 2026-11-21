// Pulls emotion tags like "[happy]" out of streamed text.
// Text arrives in arbitrary chunks, so a tag may be split across two of them.
import { EMOTIONS, type Emotion } from "./motions";

export type Mood = Emotion | "neutral";
const MOODS = new Set<string>([...EMOTIONS, "neutral"]);
// Words Claude might use that aren't in the list, mapped to the nearest one.
const ALIASES: Record<string, Mood> = {
  excited: "happy",
  joyful: "happy",
  laugh: "happy",
  smile: "happy",
  cheerful: "happy",
  upset: "sad",
  crying: "sad",
  annoyed: "angry",
  mad: "angry",
  shocked: "surprised",
  surprise: "surprised",
  embarrassed: "shy",
  curious: "thinking",
  confused: "thinking",
  smug: "proud",
  calm: "neutral",
};
const MAX_TAG = 16;

export interface Parsed {
  text: string;
  moods: Mood[];
}

export class TagStream {
  private pending = "";
  private atStart = true;

  feed(chunk: string): Parsed {
    let input = this.pending + chunk;
    this.pending = "";
    // Hold back a possible unfinished tag at the end.
    const open = input.lastIndexOf("[");
    if (open !== -1 && input.indexOf("]", open) === -1 && input.length - open <= MAX_TAG) {
      this.pending = input.slice(open);
      input = input.slice(0, open);
    }
    return this.parse(input);
  }

  /** Flushes anything held back (call when the stream ends). */
  end(): Parsed {
    const rest = this.pending;
    this.pending = "";
    return { text: rest, moods: [] };
  }

  private parse(input: string): Parsed {
    const moods: Mood[] = [];
    let text = input.replace(/\[([a-zA-Z ]{2,14})\]\s?/g, (whole, word: string) => {
      const key = word.trim().toLowerCase();
      const mood = MOODS.has(key) ? (key as Mood) : ALIASES[key];
      if (!mood) return whole;
      moods.push(mood);
      return "";
    });
    if (this.atStart) {
      text = text.replace(/^\s+/, "");
      if (text) this.atStart = false;
    }
    return { text, moods };
  }
}

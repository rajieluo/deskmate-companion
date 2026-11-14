// Splits streamed text into sentences so each one can be spoken as soon as it's complete.

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

/** Removes things that sound bad read aloud (URLs, code, emoji, markdown symbols). */
export function speakable(text: string): string {
  return text
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`([^`]*)`/g, "$1")
    .replace(/https?:\/\/\S+/g, "a link")
    .replace(/[*_#>~]+/g, "")
    .replace(/\p{Extended_Pictographic}/gu, "")
    .replace(/\s+/g, " ")
    .trim();
}

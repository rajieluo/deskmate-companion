// Turns web addresses in a message into clickable links that open in the browser.
import { openUrl } from "./host";

export type Part = { text: string; url?: string };

// [label](url), a full http(s) address, or a bare one like dashboard.stripe.com/settings.
const LINK =
  /\[([^\]\n]+)\]\((https?:\/\/[^\s)]+)\)|\bhttps?:\/\/[^\s<>"']+|\b(?:[a-z0-9-]+\.)+(?:com|org|net|io|dev|ai|gov|edu|co|uk|ca|info)\b(?:\/[^\s<>"']*)?/gi;

/** Drops sentence punctuation after an address, and a ")" that isn't part of it. */
function trimEnd(url: string): string {
  for (;;) {
    const last = url.at(-1);
    if (last && ".,;:!?'\"".includes(last)) url = url.slice(0, -1);
    else if (last === ")" && (url.match(/\(/g)?.length ?? 0) < (url.match(/\)/g)?.length ?? 0)) url = url.slice(0, -1);
    else return url;
  }
}

/** Splits text into plain runs and links. */
export function findLinks(text: string): Part[] {
  const parts: Part[] = [];
  let i = 0;
  const plain = (end: number) => end > i && parts.push({ text: text.slice(i, end) });
  for (const m of text.matchAll(LINK)) {
    const start = m.index;
    // Part of an email address, or the tail of a word: leave it alone.
    if (start > 0 && /[@\w.-]/.test(text[start - 1]) && !m[1]) continue;
    if (m[1]) {
      plain(start);
      parts.push({ text: m[1], url: m[2] });
      i = start + m[0].length;
      continue;
    }
    const found = trimEnd(m[0]);
    plain(start);
    parts.push({ text: found, url: /^https?:\/\//i.test(found) ? found : `https://${found}` });
    i = start + found.length;
  }
  plain(text.length);
  return parts;
}

export function linkElement(part: Required<Part>): HTMLAnchorElement {
  const a = document.createElement("a");
  a.href = part.url;
  a.textContent = part.text;
  a.title = part.url;
  a.addEventListener("click", (e) => {
    // Open in the browser instead of navigating the app's own window.
    e.preventDefault();
    void openUrl(part.url);
  });
  return a;
}

/** Replaces `el`'s contents with `text`, with its links clickable. */
export function setLinkedText(el: HTMLElement, text: string) {
  el.replaceChildren(...findLinks(text).map((p) => (p.url ? linkElement(p as Required<Part>) : p.text)));
}

// The Chat history window: every message saved in history.jsonl, grouped by day.
import * as host from "./host";

const log = document.getElementById("log")!;
const search = document.getElementById("search") as HTMLInputElement;
let entries: host.HistoryEntry[] = [];

const dayFmt = new Intl.DateTimeFormat(undefined, { weekday: "long", year: "numeric", month: "long", day: "numeric" });
const timeFmt = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });

function el(tag: string, cls: string, text?: string) {
  const e = document.createElement(tag);
  e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

/** Appends text to `parent`, wrapping matches of `q` in <mark>. */
function highlighted(parent: HTMLElement, text: string, q: string) {
  if (!q) return void parent.append(text);
  const lower = text.toLowerCase();
  let i = 0;
  for (let j = lower.indexOf(q); j !== -1; j = lower.indexOf(q, i)) {
    parent.append(text.slice(i, j), el("mark", "", text.slice(j, j + q.length)));
    i = j + q.length;
  }
  parent.append(text.slice(i));
}

function render() {
  const q = search.value.trim().toLowerCase();
  const shown = q ? entries.filter((e) => e.text.toLowerCase().includes(q)) : entries;
  log.replaceChildren();
  if (!shown.length) {
    log.append(el("p", "empty", q ? "No messages match." : "No messages yet. Say something to her!"));
    return;
  }
  let lastDay = "";
  for (const e of shown) {
    const date = new Date(e.time);
    const day = dayFmt.format(date);
    if (day !== lastDay) {
      log.append(el("div", "day", day));
      lastDay = day;
    }
    const who = e.role === "user" ? "You" : e.name || "Assistant";
    const mode = e.mode === "claude_code" ? " · Claude Code" : "";
    const msg = el("div", `msg ${e.role}`);
    msg.append(el("div", "meta", `${who} · ${timeFmt.format(date)}${mode}`));
    const body = el("div", "text");
    highlighted(body, e.text, q);
    msg.append(body);
    log.append(msg);
  }
  if (!q) log.scrollTop = log.scrollHeight;
}

async function refresh() {
  entries = await host.historyRead();
  render();
}

search.addEventListener("input", render);
document.getElementById("refresh")!.addEventListener("click", () => void refresh());
document.getElementById("clear")!.addEventListener("click", async () => {
  if (!confirm("Delete the whole chat history? This can't be undone.")) return;
  await host.historyClear();
  await refresh();
});
host.onHistoryRefresh(() => void refresh());
// Pick up new messages while the window is open.
window.addEventListener("focus", () => void refresh());
void refresh();

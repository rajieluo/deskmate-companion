// The Crew window: every crewmate firstmate is running (one per task, each in its own
// worktree and Herdr pane), what state it is in, and its timeline from the fleet
// ledger. Finished (cleaned-up) tasks are kept below. Data: crew_fleet in lib.rs.
import * as host from "./host";
import type { CrewFleet, CrewTask } from "./host";
import { ago, groupTasks, mateLabel, matches, stateLabel, taskName, toneFor } from "./crew-status";
import { findLinks, linkElement } from "./links";

const mateEl = document.getElementById("mate")!;
const notice = document.getElementById("notice")!;
const list = document.getElementById("list")!;
const detail = document.getElementById("detail")!;
const search = document.getElementById("search") as HTMLInputElement;

let fleet: CrewFleet | null = null;
let selected: string | null = null;
let showFinished = false;

const timeFmt = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls = "", text?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

/** Text with its links made clickable (they open in the browser). */
function linked(text: string): HTMLElement {
  const span = el("span");
  for (const part of findLinks(text)) span.append(part.url ? linkElement({ text: part.text, url: part.url }) : part.text);
  return span;
}

function allTasks(): CrewTask[] {
  return fleet ? [...fleet.tasks, ...fleet.finished] : [];
}

function renderHeader() {
  mateEl.replaceChildren();
  if (!fleet) {
    mateEl.textContent = "Crew (open this in the desktop app)";
    return;
  }
  const dot = el("span", `dot ${fleet.on ? fleet.mate : "stopped"}`);
  const who = el("span", "who", fleet.direct ? `First mate: ${mateLabel(fleet.mate)}` : "First mate: in its own terminal");
  const count = fleet.tasks.length;
  const sub = el("span", "sub", `${count} ${count === 1 ? "crewmate" : "crewmates"} · ${fleet.home}`);
  mateEl.append(dot, who, sub);
  mateEl.title = fleet.session ? `Session ${fleet.session}` : "";

  if (!fleet.on) {
    notice.hidden = false;
    notice.className = "notice";
    notice.textContent = "Crew mode is off, so this list isn't being updated. Turn on Crew mode (firstmate) in the tray menu.";
  } else if (fleet.error) {
    notice.hidden = false;
    notice.className = "notice error";
    notice.textContent = `Couldn't read the crew: ${fleet.error}`;
  } else {
    notice.hidden = true;
  }
}

function taskRow(t: CrewTask): HTMLButtonElement {
  const row = el("button", `row${t.id === selected ? " selected" : ""}${t.ended ? " finished" : ""}`);
  row.type = "button";
  const top = el("div", "row-top");
  top.append(el("span", `badge ${toneFor(t.state)}`), el("span", "name", taskName(t.id)), el("span", "when", ago(t.updated)));
  const meta = el("div", "row-meta", [stateLabel(t.state), t.project, t.kind].filter(Boolean).join(" · "));
  const text = el("div", "row-text", t.text || "No status yet.");
  row.append(top, meta, text);
  row.addEventListener("click", () => {
    selected = t.id;
    render();
  });
  return row;
}

function renderList() {
  const q = search.value;
  const tasks = (fleet?.tasks ?? []).filter((t) => matches(t, q));
  const finished = (fleet?.finished ?? []).filter((t) => matches(t, q));
  list.replaceChildren();
  if (!fleet?.tasks.length && !fleet?.finished.length) {
    list.append(el("p", "empty", "No crew yet. Ask her to start a task and its crewmate shows up here."));
    return;
  }
  if (!tasks.length && !finished.length) {
    list.append(el("p", "empty", "No tasks match."));
    return;
  }
  for (const group of groupTasks(tasks)) {
    list.append(el("h2", `section ${group.tone}`, `${group.title} (${group.tasks.length})`));
    for (const t of group.tasks) list.append(taskRow(t));
  }
  if (finished.length) {
    const toggle = el("button", "section toggle", `${showFinished || q ? "▾" : "▸"} Finished (${finished.length})`);
    toggle.type = "button";
    toggle.addEventListener("click", () => {
      showFinished = !showFinished;
      renderList();
    });
    list.append(toggle);
    if (showFinished || q) for (const t of finished) list.append(taskRow(t));
  }
}

function renderDetail() {
  detail.replaceChildren();
  const t = allTasks().find((x) => x.id === selected);
  if (!t) {
    detail.append(el("p", "empty", fleet?.tasks.length ? "Pick a crewmate to see what it has been doing." : ""));
    return;
  }
  const head = el("div", "detail-head");
  head.append(el("h1", "", taskName(t.id)), el("span", `pill ${toneFor(t.state)}`, t.ended ? `${stateLabel(t.state)} · finished` : stateLabel(t.state)));
  detail.append(head);

  const facts = el("dl", "facts");
  const fact = (k: string, v: string | null | undefined) => {
    if (!v) return;
    facts.append(el("dt", "", k), el("dd", "", v));
  };
  fact("Task id", t.id);
  fact("Kind", t.kind);
  fact("Project", t.project);
  fact("Agent", [t.harness, t.model].filter(Boolean).join(" · "));
  fact("Started", t.started ? `${timeFmt.format(new Date(t.started * 1000))} (${ago(t.started)})` : "");
  fact("Last update", t.updated ? `${timeFmt.format(new Date(t.updated * 1000))} (${ago(t.updated)})` : "");
  if (t.ended) fact("Cleaned up", timeFmt.format(new Date(t.ended * 1000)));
  detail.append(facts);

  const actions = el("div", "actions");
  if (t.pr) {
    const pr = el("button", "primary", "Open PR");
    pr.type = "button";
    pr.addEventListener("click", () => void host.openUrl(t.pr!));
    actions.append(pr);
  }
  if (!t.ended) {
    const term = el("button", "", "Watch in crew terminal");
    term.type = "button";
    term.title = "Opens Herdr, where this crewmate has its own pane";
    term.addEventListener("click", () => void host.crewOpenTerminal().catch((e) => showError(e)));
    actions.append(term);
  }
  if (actions.childElementCount) detail.append(actions);

  if (t.text) {
    detail.append(el("h2", "", "Latest status"));
    const latest = el("p", "latest");
    latest.append(linked(t.text));
    detail.append(latest);
  }

  const events = [...(t.events ?? [])].reverse();
  detail.append(el("h2", "", "Timeline"));
  if (!events.length) {
    detail.append(el("p", "muted", "Nothing recorded yet."));
    return;
  }
  const ol = el("ol", "timeline");
  for (const e of events) {
    const li = el("li", `ev ${e.kind} ${e.state ? toneFor(e.state) : ""}`);
    const label = e.kind === "status" ? stateLabel(e.state ?? "status") : e.kind === "pr-ready" ? "PR ready" : e.kind.replace("-", " ");
    const meta = el("div", "ev-meta");
    meta.append(el("span", "ev-kind", label), el("span", "ev-time", timeFmt.format(new Date(e.ts * 1000))));
    const body = el("div", "ev-text");
    if (e.text) body.append(linked(e.text));
    li.append(meta, body);
    ol.append(li);
  }
  detail.append(ol);
}

function render() {
  renderHeader();
  // Keep a selection: the first task needing attention, else the newest.
  const tasks = allTasks();
  if (!tasks.some((t) => t.id === selected)) {
    selected = groupTasks(fleet?.tasks ?? [])[0]?.tasks[0]?.id ?? null;
  }
  renderList();
  renderDetail();
}

function showError(e: unknown) {
  notice.hidden = false;
  notice.className = "notice error";
  notice.textContent = String(e);
}

async function refresh() {
  try {
    fleet = await host.crewFleet();
  } catch (e) {
    showError(e);
    return;
  }
  render();
}

search.addEventListener("input", () => renderList());
document.getElementById("refresh")!.addEventListener("click", () => void refresh());
document.getElementById("terminal")!.addEventListener("click", () => void host.crewOpenTerminal().catch((e) => showError(e)));
// The app pushes crew-tasks whenever the ledger changes; the rest (finished tasks,
// the first mate's state) comes with a fresh read.
host.onCrewTasks(() => void refresh());
host.onCrewError(() => void refresh());
window.addEventListener("focus", () => void refresh());
// Keeps the "5 min ago" times and the first mate's state current.
setInterval(() => void refresh(), 15_000);
void refresh();

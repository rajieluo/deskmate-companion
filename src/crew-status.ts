// Pure helpers for Crew mode's badges (kept apart from the DOM code so they can be tested).
import type { CrewTask } from "./host";

export type Tone = "working" | "attention" | "ready" | "failed" | "idle";

/** Which colour a task's badge gets. */
export function toneFor(state: string): Tone {
  switch (state) {
    case "needs-decision":
    case "blocked":
      return "attention";
    case "pr-ready":
    case "done":
    case "merged":
    case "resolved":
      return "ready";
    case "failed":
      return "failed";
    case "paused":
      return "idle";
    default:
      return "working";
  }
}

const LABELS: Record<string, string> = {
  "needs-decision": "needs you",
  blocked: "blocked",
  "pr-ready": "PR ready",
  merged: "merged",
  done: "done",
  failed: "failed",
  paused: "paused",
  working: "working",
  resolved: "resolved",
};

export function stateLabel(state: string): string {
  return LABELS[state] ?? state;
}

/** "fix-login" -> "fix login". */
export function taskName(id: string): string {
  return id.replace(/[-_]+/g, " ");
}

/** One line for the tray tooltip / summary, e.g. "2 working, 1 needs you". */
export function summarize(tasks: CrewTask[]): string {
  if (!tasks.length) return "No crew tasks";
  const counts = new Map<Tone, number>();
  for (const t of tasks) counts.set(toneFor(t.state), (counts.get(toneFor(t.state)) ?? 0) + 1);
  const parts: string[] = [];
  const add = (tone: Tone, label: string) => counts.get(tone) && parts.push(`${counts.get(tone)} ${label}`);
  add("attention", "need you");
  add("failed", "failed");
  add("ready", "ready");
  add("working", "working");
  add("idle", "paused");
  return parts.join(", ");
}

/** The Crew window's sections, in the order shown. */
export const SECTIONS: { tone: Tone; title: string }[] = [
  { tone: "attention", title: "Needs you" },
  { tone: "failed", title: "Failed" },
  { tone: "ready", title: "Ready" },
  { tone: "working", title: "Working" },
  { tone: "idle", title: "Paused" },
];

/** Splits live tasks into the window's sections (empty ones left out), newest first in each. */
export function groupTasks(tasks: CrewTask[]): { tone: Tone; title: string; tasks: CrewTask[] }[] {
  return SECTIONS.map((s) => ({
    ...s,
    tasks: tasks.filter((t) => toneFor(t.state) === s.tone).sort((a, b) => b.updated - a.updated),
  })).filter((s) => s.tasks.length > 0);
}

/** "just now", "5 min ago", "3 h ago", "2 days ago" for a Unix time in seconds. */
export function ago(ts: number | null | undefined, now = Date.now() / 1000): string {
  if (!ts) return "";
  const s = Math.max(0, Math.round(now - ts));
  if (s < 45) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 36) return `${h} h ago`;
  const d = Math.round(h / 24);
  return d === 1 ? "1 day ago" : `${d} days ago`;
}

/** What the first mate is doing, for the Crew window's header. */
export function mateLabel(mate: string): string {
  switch (mate) {
    case "idle":
      return "on watch";
    case "busy":
      return "working";
    case "starting":
      return "starting up";
    case "stopped":
      return "not running";
    case "separate":
      return "in its own terminal";
    default:
      return mate;
  }
}

/** True when the task matches a search: its name, project, state or any timeline text. */
export function matches(t: CrewTask, q: string): boolean {
  const needle = q.trim().toLowerCase();
  if (!needle) return true;
  const hay = [t.id, taskName(t.id), t.project, t.kind, t.state, stateLabel(t.state), t.text, ...(t.events ?? []).map((e) => e.text)];
  return hay.some((h) => h?.toLowerCase().includes(needle));
}

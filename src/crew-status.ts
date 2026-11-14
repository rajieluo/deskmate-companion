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

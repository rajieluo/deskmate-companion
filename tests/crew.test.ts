import { describe, expect, it } from "vitest";
import { stateLabel, summarize, taskName, toneFor } from "../src/crew-status";
import type { CrewTask } from "../src/host";

const task = (id: string, state: string): CrewTask => ({ id, state, kind: "ship", project: "app", text: "", pr: null, updated: 0 });

describe("crew badges", () => {
  it("colours each firstmate state", () => {
    expect(toneFor("working")).toBe("working");
    expect(toneFor("needs-decision")).toBe("attention");
    expect(toneFor("blocked")).toBe("attention");
    expect(toneFor("pr-ready")).toBe("ready");
    expect(toneFor("merged")).toBe("ready");
    expect(toneFor("failed")).toBe("failed");
    expect(toneFor("paused")).toBe("idle");
    expect(toneFor("something-new")).toBe("working");
  });

  it("names tasks and states for people", () => {
    expect(taskName("fix-flaky_login")).toBe("fix flaky login");
    expect(stateLabel("needs-decision")).toBe("needs you");
    expect(stateLabel("pr-ready")).toBe("PR ready");
  });

  it("summarizes the crew, most urgent first", () => {
    expect(summarize([])).toBe("No crew tasks");
    expect(summarize([task("a", "working"), task("b", "needs-decision"), task("c", "working"), task("d", "pr-ready")])).toBe(
      "1 need you, 1 ready, 2 working",
    );
  });
});

describe("crew window", () => {
  it("groups tasks most urgent first and leaves out empty sections", async () => {
    const { groupTasks } = await import("../src/crew-status");
    const a = { ...task("a", "working"), updated: 1 };
    const b = { ...task("b", "working"), updated: 5 };
    const groups = groupTasks([a, task("c", "blocked"), b]);
    expect(groups.map((g) => g.title)).toEqual(["Needs you", "Working"]);
    expect(groups[1].tasks.map((t) => t.id)).toEqual(["b", "a"]);
  });

  it("says how long ago, and what the first mate is doing", async () => {
    const { ago, mateLabel } = await import("../src/crew-status");
    expect(ago(null)).toBe("");
    expect(ago(1000, 1010)).toBe("just now");
    expect(ago(1000, 1000 + 5 * 60)).toBe("5 min ago");
    expect(ago(1000, 1000 + 3 * 3600)).toBe("3 h ago");
    expect(ago(1000, 1000 + 48 * 3600)).toBe("2 days ago");
    expect(mateLabel("idle")).toBe("on watch");
    expect(mateLabel("stopped")).toBe("not running");
  });

  it("searches names, projects and the timeline", async () => {
    const { matches } = await import("../src/crew-status");
    const t = { ...task("fix-login", "working"), events: [{ ts: 1, kind: "status", state: "working", text: "flaky test found" }] };
    expect(matches(t, "")).toBe(true);
    expect(matches(t, "fix log")).toBe(true);
    expect(matches(t, "APP")).toBe(true);
    expect(matches(t, "flaky")).toBe(true);
    expect(matches(t, "billing")).toBe(false);
  });
});

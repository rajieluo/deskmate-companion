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

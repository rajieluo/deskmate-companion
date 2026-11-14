// Crew mode: a column of small status badges beside her, one per firstmate task,
// with a card for the one you click. The tasks come from firstmate's fleet ledger
// (see src-tauri/src/crew.rs).
import * as host from "./host";
import type { CrewTask } from "./host";
import { stateLabel, summarize, taskName, toneFor } from "./crew-status";

export class CrewUI {
  private strip = document.getElementById("crew") as HTMLDivElement;
  private card = document.getElementById("crew-card") as HTMLDivElement;
  private tasks: CrewTask[] = [];
  private open: string | null = null;
  private on = false;

  constructor() {
    host.onCrewTasks((tasks) => this.render(tasks));
    this.card.addEventListener("click", (e) => {
      const target = e.target as HTMLElement;
      const action = target.closest<HTMLElement>("[data-action]")?.dataset.action;
      if (action === "close") this.showCard(null);
      if (action === "terminal") void host.crewOpenTerminal();
      if (action === "pr") {
        const pr = this.tasks.find((t) => t.id === this.open)?.pr;
        if (pr) void host.openUrl(pr);
      }
    });
  }

  setEnabled(on: boolean) {
    this.on = on;
    if (on) void host.crewTasks().then((t) => this.render(t));
    else this.render([]);
  }

  render(tasks: CrewTask[]) {
    this.tasks = this.on ? tasks : [];
    this.strip.replaceChildren(
      ...this.tasks.map((t) => {
        const b = document.createElement("button");
        b.type = "button";
        b.className = `badge ${toneFor(t.state)}`;
        b.title = `${taskName(t.id)}: ${stateLabel(t.state)}`;
        b.setAttribute("aria-label", b.title);
        b.setAttribute("data-hit", "");
        b.addEventListener("click", () => this.showCard(this.open === t.id ? null : t.id));
        return b;
      }),
    );
    this.strip.hidden = this.tasks.length === 0;
    this.strip.toggleAttribute("data-hit", this.tasks.length > 0);
    this.strip.title = summarize(this.tasks);
    if (this.open && !this.tasks.some((t) => t.id === this.open)) this.showCard(null);
    else if (this.open) this.showCard(this.open);
  }

  private showCard(id: string | null) {
    this.open = id;
    const t = this.tasks.find((x) => x.id === id);
    this.card.hidden = !t;
    this.card.toggleAttribute("data-hit", !!t);
    if (!t) return;
    const title = document.createElement("p");
    title.className = "title";
    title.textContent = taskName(t.id);
    const meta = document.createElement("p");
    meta.className = `meta ${toneFor(t.state)}`;
    meta.textContent = [stateLabel(t.state), t.project, t.kind].filter(Boolean).join(" · ");
    const text = document.createElement("p");
    text.className = "text";
    text.textContent = t.text || "No status yet.";
    const actions = document.createElement("div");
    actions.className = "actions";
    const button = (label: string, action: string) => {
      const b = document.createElement("button");
      b.type = "button";
      b.textContent = label;
      b.dataset.action = action;
      return b;
    };
    if (t.pr) actions.append(button("Open PR", "pr"));
    actions.append(button("Open terminal", "terminal"), button("Close", "close"));
    this.card.replaceChildren(title, meta, text, actions);
  }
}

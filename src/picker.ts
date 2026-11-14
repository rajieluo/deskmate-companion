// The poses & animations picker: a panel above the chat box listing the model's
// expressions (toggle on/off, remembered per model) and motions (click to play),
// with the voice speed and pitch sliders at the bottom.
import type { Avatar } from "./avatar";

export interface VoiceTuning {
  readonly speed: number;
  readonly pitch: number;
  /** Called when a slider is let go. */
  set(change: { speed?: number; pitch?: number }): void;
}

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

function storageKey(model: string) {
  return `expressions:${model}`;
}

function load(model: string): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(storageKey(model)) ?? "[]");
    return Array.isArray(v) ? v.filter((x) => typeof x === "string") : [];
  } catch {
    return [];
  }
}

function save(model: string, files: string[]) {
  try {
    localStorage.setItem(storageKey(model), JSON.stringify(files));
  } catch {
    /* storage unavailable: just don't remember */
  }
}

export class PosePicker {
  private panel = $<HTMLDivElement>("poses");
  private button = $<HTMLButtonElement>("poses-btn");
  private model = "";

  constructor(
    private avatar: Avatar,
    private tuning?: VoiceTuning,
  ) {
    this.button.addEventListener("click", () => (this.panel.hidden ? this.open() : this.close()));
    document.addEventListener("keydown", (e) => e.key === "Escape" && this.close());
    document.addEventListener("pointerdown", (e) => {
      const t = e.target as Node;
      if (!this.panel.hidden && !this.panel.contains(t) && !this.button.contains(t)) this.close();
    });
  }

  /** Re-applies the expressions that were on last time for this model. */
  async restore(model: string) {
    this.model = model;
    for (const file of load(model)) {
      if (!this.avatar.actions.expressions.some((e) => e.file === file)) continue;
      await this.avatar.toggleExpression(file, true).catch(() => {});
    }
    if (!this.panel.hidden) this.render();
  }

  open() {
    this.render();
    this.panel.hidden = false;
    this.panel.setAttribute("data-hit", "");
    this.button.classList.add("active");
  }

  close() {
    this.panel.hidden = true;
    this.panel.removeAttribute("data-hit");
    this.button.classList.remove("active");
  }

  private section(title: string, empty: string) {
    const h = document.createElement("h3");
    h.textContent = title;
    const list = document.createElement("div");
    list.className = "chips";
    this.panel.append(h, list);
    return {
      list,
      done: () => {
        if (list.childElementCount) return;
        const p = document.createElement("p");
        p.className = "empty";
        p.textContent = empty;
        list.append(p);
      },
    };
  }

  private chip(label: string, onClick: (b: HTMLButtonElement) => void, on = false) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "chip";
    b.textContent = label;
    b.title = label;
    b.classList.toggle("on", on);
    b.setAttribute("aria-pressed", String(on));
    b.addEventListener("click", () => onClick(b));
    return b;
  }

  private slider(label: string, value: number, min: number, max: number, onSet: (v: number) => void) {
    const row = document.createElement("label");
    row.className = "slider";
    const name = document.createElement("span");
    name.textContent = label;
    const input = document.createElement("input");
    input.type = "range";
    input.min = String(min);
    input.max = String(max);
    input.step = "0.05";
    input.value = String(value);
    const readout = document.createElement("output");
    const show = () => (readout.textContent = `${Number(input.value).toFixed(2)}×`);
    show();
    input.addEventListener("input", show);
    input.addEventListener("change", () => onSet(Number(input.value)));
    // Double-click puts it back to normal.
    input.addEventListener("dblclick", () => {
      input.value = "1";
      show();
      onSet(1);
    });
    row.append(name, input, readout);
    return row;
  }

  private render() {
    const { motions, expressions } = this.avatar.actions;
    this.panel.replaceChildren();

    const ex = this.section("Poses & expressions", "This model has no expressions.");
    for (const e of expressions) {
      ex.list.append(
        this.chip(
          e.label,
          async (b) => {
            const on = await this.avatar.toggleExpression(e.file).catch(() => false);
            b.classList.toggle("on", on);
            b.setAttribute("aria-pressed", String(on));
            save(
              this.model,
              expressions.filter((x) => this.avatar.isExpressionOn(x.file)).map((x) => x.file),
            );
          },
          this.avatar.isExpressionOn(e.file),
        ),
      );
    }
    ex.done();

    const faces = motions.filter((m) => m.face);
    if (faces.length) {
      const fa = this.section("Faces", "");
      for (const m of faces) fa.list.append(this.chip(m.label, () => this.avatar.playMotion(m.group, m.index)));
      fa.done();
    }

    const mo = this.section("Animations", "This model has no animations.");
    for (const m of motions.filter((m) => !m.face)) mo.list.append(this.chip(m.label, () => this.avatar.playMotion(m.group, m.index)));
    mo.done();

    const t = this.tuning;
    if (t) {
      const h = document.createElement("h3");
      h.textContent = "Voice";
      this.panel.append(
        h,
        this.slider("Speed", t.speed, 0.5, 2, (speed) => t.set({ speed })),
        this.slider("Pitch", t.pitch, 0.75, 1.35, (pitch) => t.set({ pitch })),
      );
    }
  }
}

// The "/" menu on the chat box in Claude Code mode: type "/" to list Claude Code's
// commands and skills, arrows to move, Tab or Enter to fill one in, Esc to close.

export interface SlashItem {
  name: string;
  description: string;
  app: boolean;
}

/** Commands matching what's typed after "/": name prefix first, then anywhere in the name. */
export function filterCommands(items: SlashItem[], query: string): SlashItem[] {
  const q = query.toLowerCase();
  if (!q) return items;
  const starts = items.filter((c) => c.name.toLowerCase().startsWith(q));
  // "pdf" should also find "anthropic-skills:pdf".
  const part = items.filter((c) => !starts.includes(c) && c.name.toLowerCase().split(":").pop()!.startsWith(q));
  const inside = items.filter((c) => !starts.includes(c) && !part.includes(c) && c.name.toLowerCase().includes(q));
  return [...starts, ...part, ...inside];
}

/** The text after "/" while the user is still typing a command name, else null. */
export function slashQuery(value: string): string | null {
  const m = /^\/(\S*)$/.exec(value);
  return m ? m[1] : null;
}

export class SlashMenu {
  private list = document.createElement("ul");
  private items: SlashItem[] | null = null;
  private loading: Promise<void> | null = null;
  private error = "";
  private shown: SlashItem[] = [];
  private active = 0;

  constructor(
    private input: HTMLInputElement,
    /** Only offered while this is true (Claude Code mode). */
    private enabled: () => boolean,
    /** Fetches the commands (host.codeCommands). */
    private load: () => Promise<SlashItem[]>,
  ) {
    this.list.id = "slash-menu";
    this.list.hidden = true;
    this.list.setAttribute("role", "listbox");
    input.insertAdjacentElement("afterend", this.list);
    input.addEventListener("input", () => this.update());
    input.addEventListener("blur", () => this.hide());
    // Registered before the chat box's own keydown handler, so Esc closes just the menu.
    input.addEventListener("keydown", (e) => this.onKey(e));
    // mousedown, not click, so the input keeps focus.
    this.list.addEventListener("mousedown", (e) => {
      const row = (e.target as HTMLElement).closest<HTMLLIElement>("li[data-index]");
      if (!row) return;
      e.preventDefault();
      this.pick(Number(row.dataset.index));
    });
  }

  get isOpen() {
    return !this.list.hidden;
  }

  /** Forget the list, e.g. after the folder changes. */
  invalidate() {
    this.items = null;
    this.loading = null;
    this.error = "";
  }

  hide() {
    this.list.hidden = true;
  }

  private update() {
    const q = slashQuery(this.input.value);
    if (q === null || !this.enabled()) return this.hide();
    if (!this.items) {
      this.render(q);
      this.loading ??= this
        .load()
        .then((items) => {
          this.items = items;
          this.error = "";
        })
        .catch((e) => {
          this.error = String(e);
          this.loading = null; // try again next time
        })
        .finally(() => {
          const now = slashQuery(this.input.value);
          if (now !== null && document.activeElement === this.input) this.render(now);
        });
      return;
    }
    this.render(q);
  }

  private render(query: string) {
    this.list.replaceChildren();
    this.list.hidden = false;
    if (!this.items) {
      this.list.append(this.note(this.error || "Loading commands…"));
      return;
    }
    this.shown = filterCommands(this.items, query);
    this.active = Math.min(this.active, Math.max(0, this.shown.length - 1));
    if (!this.shown.length) {
      this.list.append(this.note(`No command matches /${query}`));
      return;
    }
    this.shown.forEach((c, i) => {
      const li = document.createElement("li");
      li.dataset.index = String(i);
      li.setAttribute("role", "option");
      li.classList.toggle("active", i === this.active);
      const name = document.createElement("span");
      name.className = "name";
      name.textContent = `/${c.name}`;
      li.append(name);
      if (c.description) {
        const d = document.createElement("span");
        d.className = "desc";
        d.textContent = c.description;
        li.append(d);
      }
      li.title = c.description ? `/${c.name}: ${c.description}` : `/${c.name}`;
      this.list.append(li);
    });
    this.list.querySelector("li.active")?.scrollIntoView({ block: "nearest" });
  }

  private note(text: string) {
    const li = document.createElement("li");
    li.className = "note";
    li.textContent = text;
    return li;
  }

  private onKey(e: KeyboardEvent) {
    if (!this.isOpen) return;
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopImmediatePropagation();
      this.hide();
      return;
    }
    if (!this.shown.length) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const n = this.shown.length;
      this.active = (this.active + (e.key === "ArrowDown" ? 1 : n - 1)) % n;
      this.render(slashQuery(this.input.value) ?? "");
      return;
    }
    if (e.key === "Tab") {
      e.preventDefault();
      this.pick(this.active);
      return;
    }
    if (e.key === "Enter") {
      // Enter on a fully typed command sends it; otherwise it fills in the highlighted one.
      const typed = slashQuery(this.input.value) ?? "";
      if (this.shown[this.active]?.name === typed) {
        this.hide();
        return;
      }
      e.preventDefault();
      this.pick(this.active);
    }
  }

  private pick(i: number) {
    const c = this.shown[i];
    if (!c) return;
    // App commands take no arguments, so they're ready to send.
    this.input.value = c.app ? `/${c.name}` : `/${c.name} `;
    this.active = 0;
    this.hide();
    this.input.focus();
  }
}

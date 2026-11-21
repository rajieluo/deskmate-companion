// Speech bubble + text box for talking to Claude.
import type { Avatar } from "./avatar";
import * as host from "./host";
import { TagStream } from "./tags";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

export class ChatUI {
  private bubble = $<HTMLDivElement>("bubble");
  private bubbleText = $<HTMLParagraphElement>("bubble-text");
  private form = $<HTMLFormElement>("chat");
  private input = $<HTMLInputElement>("chat-input");
  private keyForm = $<HTMLFormElement>("api-key");
  private keyInput = $<HTMLInputElement>("api-key-input");
  private turn = 0;
  private tags = new TagStream();
  private hideTimer: number | undefined;
  private name = "Aqua";

  constructor(private avatar: Avatar) {
    host.onChat({
      delta: (turn, text) => turn === this.turn && this.onDelta(text),
      done: (turn) => turn === this.turn && this.onDone(),
      error: (turn, kind, message) => turn === this.turn && this.onError(kind, message),
    });
    host.onEvent("open-chat", () => this.open());
    host.onEvent("open-api-key", () => this.askForKey());
    host.onEvent("chat-reset", () => this.say("(I've forgotten our conversation.)", 4000));

    this.form.addEventListener("submit", (e) => {
      e.preventDefault();
      const text = this.input.value.trim();
      if (text) void this.send(text);
      this.input.value = "";
    });
    this.input.addEventListener("keydown", (e) => {
      if (e.key === "Escape") this.close();
    });
    this.keyForm.addEventListener("submit", async (e) => {
      e.preventDefault();
      await host.setApiKey(this.keyInput.value);
      this.keyInput.value = "";
      this.keyForm.hidden = true;
      this.keyForm.removeAttribute("data-hit");
      this.say("Thanks! Say something to me.", 5000);
      this.open();
    });
    void host.chatStatus().then((s) => {
      this.name = s.name;
      this.input.placeholder = `Talk to ${s.name}…`;
    });
  }

  get isOpen() {
    return !this.form.hidden;
  }

  toggle() {
    if (this.isOpen) this.close();
    else this.open();
  }

  open() {
    this.form.hidden = false;
    this.form.setAttribute("data-hit", "");
    this.input.focus();
  }

  close() {
    this.form.hidden = true;
    this.form.removeAttribute("data-hit");
    this.input.blur();
  }

  async send(text: string) {
    const status = await host.chatStatus();
    if (!status.has_key) {
      this.askForKey();
      return;
    }
    this.tags = new TagStream();
    this.say("…");
    this.bubbleText.textContent = "";
    this.bubble.classList.add("thinking");
    this.turn = await host.chatSend(text);
  }

  private onDelta(chunk: string) {
    const { text, moods } = this.tags.feed(chunk);
    for (const mood of moods) if (mood !== "neutral") this.avatar.emote(mood);
    if (text) {
      this.bubble.classList.remove("thinking");
      this.append(text);
    }
  }

  private onDone() {
    this.append(this.tags.end().text);
    this.bubble.classList.remove("thinking");
    const words = (this.bubbleText.textContent ?? "").split(/\s+/).length;
    this.scheduleHide(Math.min(30000, 5000 + words * 350));
  }

  private onError(kind: string, message: string) {
    this.bubble.classList.remove("thinking");
    if (kind === "cancelled") return;
    if (kind === "no_key" || kind === "auth") {
      this.askForKey(kind === "auth" ? "That API key didn't work." : undefined);
      return;
    }
    if (kind === "refusal") this.avatar.emote("shy");
    else this.avatar.emote("sad");
    this.say(kind === "refusal" ? "Hmm, I'd rather not answer that one." : message, 8000);
    console.warn("[chat]", kind, message);
  }

  private askForKey(reason?: string) {
    this.say(
      `${reason ? reason + " " : ""}I need an Anthropic API key to talk. Paste one below (it's stored in Windows Credential Manager).`,
    );
    this.keyForm.hidden = false;
    this.keyForm.setAttribute("data-hit", "");
    this.keyInput.focus();
  }

  private append(text: string) {
    this.bubbleText.textContent += text;
    this.bubble.scrollTop = this.bubble.scrollHeight;
    this.placeBubble();
  }

  /** Sits the bubble just above the character's head (overlapping the hair a little). */
  private placeBubble() {
    const head = this.avatar.headTop();
    if (head === null) return;
    const top = Math.max(8, head + 16 - this.bubble.offsetHeight);
    this.bubble.style.top = `${Math.round(top)}px`;
  }

  private say(text: string, hideAfter?: number) {
    window.clearTimeout(this.hideTimer);
    this.bubbleText.textContent = text;
    this.bubble.hidden = false;
    this.bubble.setAttribute("data-hit", "");
    this.placeBubble();
    if (hideAfter) this.scheduleHide(hideAfter);
  }

  private scheduleHide(ms: number) {
    window.clearTimeout(this.hideTimer);
    this.hideTimer = window.setTimeout(() => {
      this.bubble.hidden = true;
      this.bubble.removeAttribute("data-hit");
    }, ms);
  }

  get characterName() {
    return this.name;
  }
}

// Speech bubble + text box for talking to Claude.
import type { Avatar } from "./avatar";
import * as host from "./host";
import { SentenceBuffer, speakable } from "./sentences";
import { TagStream } from "./tags";
import { Mic } from "./mic";
import type { Voice } from "./voice";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

export class ChatUI {
  private bubble = $<HTMLDivElement>("bubble");
  private bubbleText = $<HTMLParagraphElement>("bubble-text");
  private activity = $<HTMLParagraphElement>("bubble-activity");
  private form = $<HTMLFormElement>("chat");
  private input = $<HTMLInputElement>("chat-input");
  private keyForm = $<HTMLFormElement>("api-key");
  private keyInput = $<HTMLInputElement>("api-key-input");
  private micButton = $<HTMLButtonElement>("mic");
  private muteButton = $<HTMLButtonElement>("mute");
  private historyButton = $<HTMLButtonElement>("history-btn");
  private mic = new Mic();
  private transcribing = false;
  /** The reply being shown; 0 while a send is in flight and its number isn't known yet. */
  private turn = 0;
  /** Lowest turn number the in-flight send can have (older turns are stale). */
  private floor = 1;
  private tags = new TagStream();
  private sentences = new SentenceBuffer();
  private hideTimer: number | undefined;
  private name = "Assistant";

  constructor(
    private avatar: Avatar,
    private voice: Voice,
  ) {
    host.onChat({
      delta: (turn, text) => this.accept(turn) && this.onDelta(text),
      done: (turn) => this.accept(turn) && this.onDone(),
      activity: (turn, text) => this.accept(turn) && this.onActivity(text),
      error: (turn, kind, message) => this.accept(turn) && this.onError(kind, message),
    });
    host.onEvent("open-chat", () => this.open());
    host.onEvent("open-api-key", () => this.askForKey());
    host.onEvent("chat-reset", () => this.say("(I've forgotten our conversation.)", 4000));
    host.onEvent("start-listening", () => void this.listen());
    host.onSttProgress((p) => this.say(`Downloading the speech model (first time only)… ${Math.floor(p)}%`));
    this.muteButton.addEventListener("click", () => host.setVoiceEnabled(!this.voice.enabled));
    host.onVoiceChanged((on) => this.showMuted(!on));
    void host.chatStatus().then((s) => this.showMuted(!s.voice_enabled));
    this.historyButton.addEventListener("click", () => host.historyOpen());
    this.micButton.addEventListener("click", () => {
      if (this.mic.recording) this.mic.stop();
      else void this.listen();
    });

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
    const refreshName = () =>
      void host.chatStatus().then((s) => {
        this.name = s.name;
        this.setMode(s.brain);
      });
    refreshName();
    // The name comes from the model's folder, so a new model can mean a new name.
    host.onModelChanged(() => refreshName());
    host.onModelPicked((model) => this.say(`Chat model: ${model}`, 3000));
    host.onBrainChanged((brain) => {
      this.setMode(brain);
      this.say(
        brain === "claude_code"
          ? "Claude Code mode: I'll work in your folder with Claude Code."
          : "Back to normal chat.",
        4000,
      );
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
    this.placeBubble();
  }

  close() {
    this.form.hidden = true;
    this.form.removeAttribute("data-hit");
    this.input.blur();
    this.placeBubble();
  }

  /** Records one spoken message, transcribes it locally and sends it. */
  async listen() {
    if (this.mic.recording || this.transcribing) return;
    this.open();
    this.voice.stop();
    this.micButton.classList.add("listening");
    this.input.placeholder = "Listening…";
    let recording;
    try {
      recording = await this.mic.record({
        onLevel: (l) => (this.micButton.style.transform = `scale(${1 + l * 0.35})`),
      });
    } catch (err) {
      this.say(`I can't hear you: ${(err as Error).message}. Check the microphone permission.`, 8000);
      return;
    } finally {
      this.micButton.classList.remove("listening");
      this.micButton.style.transform = "";
      this.input.placeholder = `Talk to ${this.name}…`;
    }
    if (!recording.heardSpeech) {
      this.say("I didn't hear anything.", 3000);
      return;
    }
    this.transcribing = true;
    this.micButton.classList.add("busy");
    this.input.placeholder = "Transcribing…";
    try {
      const text = (await host.transcribe(recording.samples)).trim();
      if (text) await this.send(text);
      else this.say("Sorry, I didn't catch that.", 3000);
    } catch (err) {
      this.say(String(err), 8000);
    } finally {
      this.transcribing = false;
      this.micButton.classList.remove("busy");
      this.input.placeholder = `Talk to ${this.name}…`;
    }
  }

  private setMode(brain: string) {
    document.body.dataset.brain = brain;
    this.input.placeholder = brain === "claude_code" ? `Ask ${this.name} to code…` : `Talk to ${this.name}…`;
  }

  async send(text: string) {
    const status = await host.chatStatus();
    if (status.brain === "api" && !status.has_key) {
      this.askForKey();
      return;
    }
    host.historyAppend("user", text);
    this.tags = new TagStream();
    this.sentences = new SentenceBuffer();
    this.voice.stop();
    this.say("…");
    this.bubbleText.textContent = "";
    this.activity.hidden = true;
    this.bubble.classList.add("thinking");
    if (this.turn) this.floor = this.turn + 1;
    this.turn = 0;
    const turn = await host.chatSend(text);
    if (this.turn === 0) this.turn = turn;
  }

  /** Events can arrive before chatSend resolves, so the first one of a new turn claims it. */
  private accept(turn: number): boolean {
    if (this.turn === 0 && turn >= this.floor) this.turn = turn;
    return turn === this.turn;
  }

  private onDelta(chunk: string) {
    const { text, moods } = this.tags.feed(chunk);
    for (const mood of moods) if (mood !== "neutral") this.avatar.emote(mood);
    if (text) {
      this.bubble.classList.remove("thinking");
      this.append(text);
      for (const s of this.sentences.push(text)) this.voice.say(speakable(s));
    }
  }

  /** Claude Code tool use: shown as a small grey line under the reply. */
  private onActivity(text: string) {
    this.activity.textContent = `${text}…`;
    this.activity.hidden = false;
    this.bubble.hidden = false;
    this.bubble.setAttribute("data-hit", "");
    this.placeBubble();
  }

  private onDone() {
    this.activity.hidden = true;
    const rest = this.tags.end().text;
    this.append(rest);
    for (const s of [...this.sentences.push(rest), ...this.sentences.flush()]) this.voice.say(speakable(s));
    this.bubble.classList.remove("thinking");
    host.historyAppend("assistant", this.bubbleText.textContent ?? "");
    const words = (this.bubbleText.textContent ?? "").split(/\s+/).length;
    this.scheduleHide(Math.min(30000, 5000 + words * 350));
  }

  private onError(kind: string, message: string) {
    this.bubble.classList.remove("thinking");
    this.activity.hidden = true;
    if (kind === "no_folder") {
      this.say("Which folder should I work in?", 6000);
      void host.pickClaudeCodeFolder();
      return;
    }
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
    this.placeBubble();
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
      // Keep the bubble up until she has finished talking.
      if (this.voice.speaking) return this.scheduleHide(1500);
      this.bubble.hidden = true;
      this.bubble.removeAttribute("data-hit");
    }, ms);
  }

  /** Shows a status line (e.g. voice download progress); empty text hides it. */
  status(text: string) {
    if (text) this.say(text, 4000);
  }

  private showMuted(muted: boolean) {
    this.muteButton.classList.toggle("muted", muted);
    this.muteButton.setAttribute("aria-pressed", String(muted));
    this.muteButton.title = muted ? "Unmute her voice (Ctrl+Shift+U)" : "Mute her voice (Ctrl+Shift+U)";
  }

  get characterName() {
    return this.name;
  }
}

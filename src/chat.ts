// Speech bubble + text box for talking to Claude.
import type { Character } from "./character";
import * as host from "./host";
import { SentenceBuffer, speakable } from "./sentences";
import { TagStream } from "./tags";
import { setLinkedText } from "./links";
import { Mic } from "./mic";
import type { Voice } from "./voice";
import { SlashMenu } from "./slash";
import { danceIntent, pickDance } from "./dance";
import { gestureCue, gestureIntent, type Gesture } from "./gestures";

/** Added to a dance request, so her answer is short and the dance starts soon. */
const DANCE_NOTE =
  "(Note from the app: you will dance for them as soon as you finish this reply. Answer in one short, upbeat sentence of at most ten words, and don't describe the dance.)";

/** Babylon's loaders reject with { request, exception } rather than an Error. */
function describeError(err: unknown): string {
  if (err instanceof Error) return err.message;
  const e = err as { request?: { status?: number; responseURL?: string }; exception?: unknown } | null;
  if (e && (e.request || e.exception)) {
    const parts = [];
    if (e.request) parts.push(`HTTP ${e.request.status ?? "?"} ${e.request.responseURL ?? ""}`.trim());
    if (e.exception) parts.push(e.exception instanceof Error ? e.exception.message : String(e.exception));
    return parts.join("; ");
  }
  try {
    return JSON.stringify(err);
  } catch {
    return String(err);
  }
}

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

export class ChatUI {
  private bubble = $<HTMLDivElement>("bubble");
  private bubbleText = $<HTMLParagraphElement>("bubble-text");
  private activity = $<HTMLParagraphElement>("bubble-activity");
  private form = $<HTMLFormElement>("chat");
  private input = $<HTMLInputElement>("chat-input");
  /** Claude Code mode's "/" menu. Created here so its keydown handler runs before ours. */
  private slash = new SlashMenu(this.input, () => document.body.dataset.brain === "claude_code", host.codeCommands);
  private keyForm = $<HTMLFormElement>("api-key");
  private keyInput = $<HTMLInputElement>("api-key-input");
  private micButton = $<HTMLButtonElement>("mic");
  private muteButton = $<HTMLButtonElement>("mute");
  private historyButton = $<HTMLButtonElement>("history-btn");
  private hotkeys = { talk: "", listen: "", mute: "" };
  private mic = new Mic();
  private transcribing = false;
  /** The reply being shown; 0 while a send is in flight and its number isn't known yet. */
  private turn = 0;
  /** Lowest turn number the in-flight send can have (older turns are stale). */
  private floor = 1;
  private tags = new TagStream();
  private sentences = new SentenceBuffer();
  private hideTimer: number | undefined;
  /** The bubble's text as written (links stay "[label](url)" here, for the history). */
  private bubbleRaw = "";
  private name = "Assistant";

  constructor(
    private avatar: Character,
    private voice: Voice,
  ) {
    host.onChat({
      delta: (turn, text) => this.accept(turn) && this.onDelta(text),
      done: (turn) => this.accept(turn) && this.onDone(),
      activity: (turn, text) => this.accept(turn) && this.onActivity(text),
      error: (turn, kind, message) => this.accept(turn) && this.onError(kind, message),
    });
    host.onEvent("open-chat", () => this.open());
    host.onEvent("open-api-key", () => void this.askForKey());
    host.onEvent("chat-reset", () => this.say("(I've forgotten our conversation.)", 4000));
    host.onEvent("start-listening", () => void this.listen());
    host.onSttProgress((p) => this.say(`Downloading the speech model (first time only)… ${Math.floor(p)}%`));
    this.muteButton.addEventListener("click", () => host.setVoiceEnabled(!this.voice.enabled));
    host.onVoiceChanged((on) => this.showMuted(!on));
    void host.chatStatus().then((s) => {
      this.hotkeys = s.hotkeys;
      this.micButton.title = s.hotkeys.listen ? `Speak (${s.hotkeys.listen})` : "Speak";
      this.showMuted(!s.voice_enabled);
    });
    this.historyButton.addEventListener("click", () => host.historyOpen());
    this.micButton.addEventListener("click", () => {
      if (this.mic.recording) this.mic.stop();
      else void this.listen();
    });

    this.form.addEventListener("submit", (e) => {
      e.preventDefault();
      const text = this.input.value.trim();
      this.input.value = "";
      this.slash.hide();
      if (text && !this.appCommand(text)) this.sendFromUser(text);
    });
    this.input.addEventListener("keydown", (e) => {
      if (e.key === "Escape") this.close();
    });
    this.keyForm.addEventListener("submit", async (e) => {
      e.preventDefault();
      try {
        await host.setApiKey(this.keyInput.value);
      } catch (err) {
        this.say(`I couldn't save that key: ${err}`);
        return;
      }
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
    host.onModelPicked(async (model) => {
      const s = await host.chatStatus();
      if (s.brain === "api" && !s.has_key) void this.askForKey(undefined, s.provider);
      else this.say(`Chat model: ${model}`, 3000);
    });
    host.onBrainChanged((brain) => {
      this.setMode(brain);
      this.say(
        brain === "claude_code"
          ? "Claude Code mode: I'll work in your folder with Claude Code."
          : brain === "crew"
            ? "Crew mode: I'll pass what you say to the first mate and tell you how the crew is doing."
            : "Back to normal chat.",
        4000,
      );
    });
    host.onCodeHandoff((h) => void this.onHandoff(h));
    host.onCrewReply((body) => this.speakOut(body));
    host.onCrewAttention((a) => this.speakOut(a.speech));
    host.onCrewError((message) => this.say(message, 8000));
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
      if (text) await this.sendFromUser(text);
      else this.say("Sorry, I didn't catch that.", 3000);
    } catch (err) {
      this.say(String(err), 8000);
    } finally {
      this.transcribing = false;
      this.micButton.classList.remove("busy");
      this.input.placeholder = `Talk to ${this.name}…`;
    }
  }

  /**
   * Slash commands the app handles itself in Claude Code mode. Every other "/command" goes to
   * Claude Code as typed, which runs it (skills, custom commands, /compact, /context…).
   */
  private lastDance: string | null = null;

  /** "Can you yawn?" makes her yawn right away; the message still goes to Claude. */
  private maybeGesture(text: string) {
    const g = gestureIntent(text);
    if (g) this.gesture(g);
  }

  /** A gesture tag or cue already moved her this reply, so later cues don't pile up. */
  private gestured = false;

  private gesture(g: Gesture) {
    this.gestured = true;
    this.avatar.gesture(g);
  }

  /** Tags like [laugh] play right away; otherwise the first cue ("haha", "*sigh*") in a spoken line. */
  private gestureFrom(tagged: readonly Gesture[], sentences: readonly string[]) {
    for (const g of tagged) this.gesture(g);
    if (this.gestured) return;
    for (const line of sentences) {
      const g = gestureCue(line);
      if (g) return this.gesture(g);
    }
  }

  /** "Dance!" makes an MMD character dance right away; the message still goes to Claude. */
  /** Bumped by every message, so a dance waiting for her to finish talking is dropped. */
  private danceTicket = 0;
  /** Resolves when the current reply has finished streaming (or failed). */
  private replyDone: Promise<void> = Promise.resolve();
  private finishReply: () => void = () => {};

  /** A message the user typed or said: may also start a dance or a gesture. */
  private sendFromUser(text: string): Promise<void> {
    const ticket = ++this.danceTicket;
    const intent = this.avatar.canDance ? danceIntent(text) : null;
    if (intent?.kind === "stop") this.avatar.stopDance();
    this.maybeGesture(text);
    const dancing = intent?.kind === "dance";
    // She dances right after answering, so keep the answer short. Crew mode passes
    // messages on to the first mate, so it's left alone there.
    const ask = dancing && document.body.dataset.brain !== "crew" ? `${text}\n\n${DANCE_NOTE}` : text;
    const sent = this.send(ask, text);
    if (dancing) void this.danceAfterReply(text, ticket);
    return sent;
  }

  /** Waits for her to finish answering out loud, then dances. */
  private async danceAfterReply(text: string, ticket: number) {
    const dances = await host.danceList();
    const intent = danceIntent(text, dances.map((d) => d.name));
    if (intent?.kind !== "dance") return;
    await this.replyDone;
    await this.speechEnded(() => ticket !== this.danceTicket);
    if (ticket !== this.danceTicket) return;
    const picked = pickDance(dances, intent.name, this.lastDance);
    this.lastDance = picked?.name ?? null;
    host.log("info", `[dance] ${picked ? `${picked.name} (${picked.motions.length} motion files${picked.audio ? ", music" : ""})` : "built-in dance"}`);
    try {
      await this.avatar.dance(picked ? host.danceMotion(picked) : null);
    } catch (err) {
      host.log("warn", `[dance] couldn't play ${picked?.name}: ${describeError(err)}`);
      // Still dance, just the built-in one.
      await this.avatar.dance(null).catch(() => false);
    }
  }

  /**
   * Resolves once she has stopped talking: speech has started (if the voice is on)
   * and then stayed quiet for a moment, since there are short gaps between sentences.
   */
  private async speechEnded(cancelled: () => boolean) {
    const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
    if (!this.voice.enabled) return;
    for (let waited = 0; !this.voice.speaking && waited < 8000 && !cancelled(); waited += 100) await sleep(100);
    let quiet = 0;
    for (let waited = 0; quiet < 1200 && waited < 60000 && !cancelled(); waited += 100) {
      await sleep(100);
      quiet = this.voice.speaking ? 0 : quiet + 100;
    }
  }

  private appCommand(text: string): boolean {
    if (document.body.dataset.brain !== "claude_code") return false;
    const cmd = text.toLowerCase();
    if (cmd === "/clear" || cmd === "/new") {
      void host.chatReset(); // also stops a reply in progress
      this.voice.stop();
      this.say("(Fresh session. The next message starts a new conversation.)", 4000);
      return true;
    }
    if (cmd === "/folder") {
      this.slash.invalidate();
      void host.pickClaudeCodeFolder().then((f) => f && this.say(`Working in ${f}`, 4000));
      return true;
    }
    return false;
  }

  private setMode(brain: string) {
    document.body.dataset.brain = brain;
    this.slash.invalidate();
    this.input.placeholder =
      brain === "claude_code"
        ? `Ask ${this.name} to code…`
        : brain === "crew"
          ? "Tell the crew…"
          : `Talk to ${this.name}…`;
    this.onModeChange?.(brain);
  }

  /** Called whenever the brain changes (Crew mode shows its badges). */
  onModeChange?: (brain: string) => void;

  /** Something she says on her own, such as a crew reply: tags move her, the rest is shown and spoken. */
  speakOut(text: string) {
    const tags = new TagStream();
    const first = tags.feed(text);
    const last = tags.end();
    for (const mood of [...first.moods, ...last.moods]) if (mood !== "neutral") this.avatar.emote(mood);
    const plain = (first.text + last.text).trim();
    this.gestured = false;
    this.gestureFrom(first.gestures, [plain]);
    if (!plain) return;
    host.historyAppend("assistant", plain);
    this.bubble.classList.remove("thinking");
    this.activity.hidden = true;
    this.say(plain);
    const sentences = new SentenceBuffer();
    for (const s of [...sentences.push(plain), ...sentences.flush()]) this.voice.say(speakable(s));
    this.scheduleHide(Math.min(30000, 5000 + plain.split(/\s+/).length * 350));
  }

  /** Claude Code asked for a new session: the app has already dropped the old one. */
  private async onHandoff(h: host.CodeHandoff) {
    this.slash.invalidate(); // the folder may have changed
    const where = h.folder.split(/[\\/]/).filter(Boolean).pop() ?? h.folder;
    if (h.error) {
      this.avatar.emote("sad");
      this.say(h.error, 10000);
      return;
    }
    if (h.kind !== "session") return; // she already said what she is doing
    // Let her finish saying what she's starting before the new session talks over her.
    for (let waited = 0; this.voice.speaking && waited < 30000; waited += 250) {
      await new Promise((r) => setTimeout(r, 250));
    }
    if (h.turn !== this.turn) return; // the user already said something else
    if (!h.brief) {
      this.say(`(Fresh session in ${where}.)`, 4000);
      return;
    }
    await this.send(h.brief, `(New session in ${where}) ${h.brief}`);
  }

  async send(text: string, historyText = text) {
    this.finishReply(); // a reply that never finished doesn't hold anyone up
    this.replyDone = new Promise((resolve) => (this.finishReply = resolve));
    const status = await host.chatStatus();
    if (status.brain === "api" && !status.has_key) {
      this.finishReply();
      void this.askForKey(undefined, status.provider);
      return;
    }
    host.historyAppend("user", historyText);
    this.tags = new TagStream();
    this.sentences = new SentenceBuffer();
    this.gestured = false;
    this.voice.stop();
    this.say("…");
    this.setBubbleText("");
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
    const { text, moods, gestures } = this.tags.feed(chunk);
    for (const mood of moods) if (mood !== "neutral") this.avatar.emote(mood);
    const lines = text ? this.sentences.push(text) : [];
    this.gestureFrom(gestures, lines);
    if (text) {
      this.bubble.classList.remove("thinking");
      this.append(text);
      for (const s of lines) this.voice.say(speakable(s));
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
    this.finishReply();
    // Each Claude Code run refreshes the command list on the Rust side (new skills, etc.).
    this.slash.invalidate();
    this.activity.hidden = true;
    const rest = this.tags.end().text;
    this.append(rest);
    const lines = [...this.sentences.push(rest), ...this.sentences.flush()];
    this.gestureFrom([], lines);
    for (const s of lines) this.voice.say(speakable(s));
    this.bubble.classList.remove("thinking");
    host.historyAppend("assistant", this.bubbleRaw);
    const words = this.bubbleRaw.split(/\s+/).length;
    this.scheduleHide(Math.min(30000, 5000 + words * 350));
  }

  private onError(kind: string, message: string) {
    this.finishReply();
    this.bubble.classList.remove("thinking");
    this.activity.hidden = true;
    if (kind === "no_folder") {
      this.say("Which folder should I work in?", 6000);
      void host.pickClaudeCodeFolder();
      return;
    }
    if (kind === "cancelled") return;
    if (kind === "crew") {
      this.avatar.emote("sad");
      this.say(message, 10000);
      return;
    }
    if (kind === "no_key" || kind === "auth") {
      void this.askForKey(kind === "auth" ? "That API key didn't work." : undefined);
      return;
    }
    if (kind === "refusal") this.avatar.emote("shy");
    else this.avatar.emote("sad");
    this.say(kind === "refusal" ? "Hmm, I'd rather not answer that one." : message, 8000);
    console.warn("[chat]", kind, message);
  }

  private async askForKey(reason?: string, provider?: host.ChatStatus["provider"]) {
    provider ??= (await host.chatStatus()).provider;
    const need =
      provider === "gemini"
        ? "I need a Google Gemini API key to talk. Get a free one at https://aistudio.google.com/apikey and paste it below"
        : "I need an Anthropic API key to talk. Paste one below";
    this.say(`${reason ? reason + " " : ""}${need} (it's stored in ${keyStore()}).`);
    this.keyInput.placeholder = provider === "gemini" ? "Gemini API key" : "sk-ant-…";
    this.keyForm.hidden = false;
    this.keyForm.setAttribute("data-hit", "");
    this.keyInput.focus();
    this.placeBubble();
  }

  private append(text: string) {
    this.setBubbleText(this.bubbleRaw + text);
    this.bubble.scrollTop = this.bubble.scrollHeight;
    this.placeBubble();
  }

  private setBubbleText(text: string) {
    this.bubbleRaw = text;
    setLinkedText(this.bubbleText, text);
  }

  /** Vertical centre of the chat box in CSS px, when it has been moved (see toolbar.ts). */
  barY: (() => number) | null = null;

  /**
   * Sits the bubble just above the character's head (overlapping the hair a little),
   * moving it above or below the chat box if the chat box is in the way.
   */
  placeBubble() {
    const head = this.avatar.headTop();
    if (head === null) return;
    const h = this.bubble.offsetHeight;
    let top = Math.max(8, head + 16 - h);
    if (this.isOpen && this.barY) {
      const barTop = this.barY() - 22;
      const barBottom = barTop + 44;
      if (top < barBottom && top + h > barTop) {
        top = barTop - 6 - h >= 8 ? barTop - 6 - h : barBottom + 6;
      }
    }
    this.bubble.style.top = `${Math.round(top)}px`;
  }

  private say(text: string, hideAfter?: number) {
    window.clearTimeout(this.hideTimer);
    this.setBubbleText(text);
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
    const key = this.hotkeys.mute ? ` (${this.hotkeys.mute})` : "";
    this.muteButton.title = `${muted ? "Unmute" : "Mute"} her voice${key}`;
  }

  get characterName() {
    return this.name;
  }
}

/** Where the API key is kept: the OS's own password store. */
function keyStore(): string {
  return /Linux|X11/.test(navigator.userAgent) ? "your system keyring" : "Windows Credential Manager";
}

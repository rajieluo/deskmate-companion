// Everything that talks to the Tauri shell lives here. In a plain browser
// (`npm run dev` opened directly, or tests) the same calls fall back to
// browser behaviour so the avatar can be developed without the desktop app.
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";

export const inTauri = "__TAURI_INTERNALS__" in window;

export interface ModelInfo {
  file_name: string;
  path: string;
  generation: number;
  look?: unknown;
  params?: unknown;
}

export interface ModelSource {
  url: string;
  label: string;
  /** Overrides from settings.json `look_params` for this model. */
  look?: Partial<Record<string, string>>;
  /** Parameter values held every frame, from settings.json `model_params`. */
  params?: Record<string, number>;
}

function toSource(info: ModelInfo): ModelSource {
  // e.g. http://model.localhost/3/1014100.model3.json on Windows.
  const base = convertFileSrc("", "model").replace(/\/?$/, "/");
  return {
    url: `${base}${info.generation}/${encodeURIComponent(info.file_name)}`,
    label: info.path,
    look: (info.look as ModelSource["look"]) ?? undefined,
    params: (info.params as ModelSource["params"]) ?? undefined,
  };
}

export async function currentModel(): Promise<ModelSource | null> {
  if (!inTauri) {
    const url = new URLSearchParams(location.search).get("model");
    return url ? { url: new URL(url, location.href).href, label: url } : null;
  }
  const info = await invoke<ModelInfo | null>("get_model");
  return info ? toSource(info) : null;
}

export async function pickModel(): Promise<ModelSource | null> {
  if (!inTauri) {
    const url = prompt("URL of a .model3.json");
    return url ? { url: new URL(url, location.href).href, label: url } : null;
  }
  const info = await invoke<ModelInfo | null>("pick_model");
  return info ? toSource(info) : null;
}

export function onModelChanged(cb: (m: ModelSource) => void) {
  if (inTauri) void listen<ModelInfo>("model-changed", (e) => cb(toSource(e.payload)));
}

export function onModelError(cb: (message: string) => void) {
  if (inTauri) void listen<string>("model-error", (e) => cb(e.payload));
}

/** Cursor position in physical (device) pixels relative to the window. */
export function onCursor(cb: (x: number, y: number, inside: boolean) => void) {
  if (inTauri) {
    void listen<{ x: number; y: number; inside: boolean }>("cursor", (e) =>
      cb(e.payload.x, e.payload.y, e.payload.inside),
    );
  } else {
    const dpr = () => window.devicePixelRatio || 1;
    window.addEventListener("mousemove", (e) => cb(e.clientX * dpr(), e.clientY * dpr(), true));
    document.addEventListener("mouseleave", () => cb(-1, -1, false));
  }
}

let ignoring: boolean | null = null;
/** Lets clicks fall through to whatever is behind the window. */
export function setClickThrough(on: boolean) {
  if (!inTauri || ignoring === on) return;
  ignoring = on;
  void getCurrentWindow().setIgnoreCursorEvents(on);
}

export function startDrag() {
  if (inTauri) void getCurrentWindow().startDragging();
}

/** Grows or shrinks the window, keeping its aspect ratio. */
export async function resizeWindow(factor: number) {
  if (!inTauri) return;
  const w = getCurrentWindow();
  const scale = await w.scaleFactor();
  const size = (await w.innerSize()).toLogical(scale);
  const width = Math.round(Math.min(1400, Math.max(200, size.width * factor)));
  // Keep the default 380x720 shape rather than the current one, so a squashed
  // window (it once got saved as 160x28, hiding her) can't stay squashed.
  const height = Math.round((width * 720) / 380);
  await w.setSize(new LogicalSize(width, height));
}

// --- Claude chat (runs on the Rust side; see src-tauri/src/chat.rs).

export interface ChatStatus {
  /** Global hotkey labels; "" when unset or taken by another app. */
  hotkeys: { talk: string; listen: string; mute: string };
  has_key: boolean;
  name: string;
  model: string;
  voice_enabled: boolean;
  follow_mouse: boolean;
  fade_bottom: boolean;
  voice: string;
  voice_speed: number;
  voice_device: string;
  brain: "api" | "claude_code";
  claude_code_folder: string | null;
}
export interface ChatHandlers {
  delta: (turn: number, text: string) => void;
  /** A tool Claude Code is using, e.g. "Reading main.ts". */
  activity: (turn: number, text: string) => void;
  done: (turn: number) => void;
  error: (turn: number, kind: string, message: string) => void;
}

let browserTurn = 0;
let browserHandlers: ChatHandlers | null = null;

// --- Chat history (saved by the Rust side to history.jsonl in the app data folder).

export interface HistoryEntry {
  time: number;
  role: "user" | "assistant";
  text: string;
  name: string;
  mode: string;
}

const browserHistory: HistoryEntry[] = [];

export function historyAppend(role: "user" | "assistant", text: string) {
  if (!text.trim()) return;
  if (inTauri) void invoke("history_append", { role, text }).catch((e) => console.warn("[history]", e));
  else browserHistory.push({ time: Date.now(), role, text, name: "Aqua", mode: "api" });
}

export async function historyRead(): Promise<HistoryEntry[]> {
  return inTauri ? invoke<HistoryEntry[]>("history_read") : [...browserHistory];
}

export async function historyClear(): Promise<void> {
  if (inTauri) await invoke("history_clear");
  else browserHistory.length = 0;
}

export function historyOpen() {
  if (inTauri) void invoke("history_open");
  else window.open("history.html", "_blank");
}

export function onHistoryRefresh(cb: () => void) {
  if (inTauri) void listen("history-refresh", () => cb());
}

/** Mutes or unmutes her voice; the tray and settings follow. */
export function setVoiceEnabled(on: boolean) {
  if (inTauri) void invoke("set_voice_enabled", { on });
  else browserVoiceListeners.forEach((cb) => cb(on));
}

const browserVoiceListeners: ((on: boolean) => void)[] = [];

/** Motion and expression files in the current model's folder (relative paths). */
export async function modelFiles(): Promise<string[]> {
  if (inTauri) return invoke<string[]>("model_files").catch(() => []);
  // Browser dev mode: ?files=a.exp3.json,b.motion3.json
  return (new URLSearchParams(location.search).get("files") ?? "").split(",").filter(Boolean);
}

/** Writes a line to the app's log file (and the console). */
export function log(level: "info" | "warn" | "error", message: string) {
  (level === "info" ? console.info : level === "warn" ? console.warn : console.error)(message);
  if (inTauri) void invoke("page_log", { level, message }).catch(() => {});
}

export async function chatStatus(): Promise<ChatStatus> {
  if (!inTauri) {
    return { hotkeys: { talk: "Ctrl+Shift+Space", listen: "Ctrl+Alt+M", mute: "Ctrl+Alt+U" }, has_key: true, name: "Aqua", model: "demo", voice_enabled: true, follow_mouse: true, fade_bottom: true, voice: "af_heart", voice_speed: 1, voice_device: "cpu", brain: "api", claude_code_folder: null };
  }
  return invoke<ChatStatus>("chat_status");
}

export async function setApiKey(key: string) {
  if (inTauri) await invoke("set_api_key", { key });
}

export async function chatSend(text: string): Promise<number> {
  if (inTauri) return invoke<number>("chat_send", { text });
  // Browser demo: a canned streamed reply, so the UI can be tried without the app.
  const turn = ++browserTurn;
  const reply = `[happy] You said "${text}". [thinking] In the desktop app I'd ask Claude instead!`;
  const chunks = reply.match(/.{1,6}/gs) ?? [];
  void (async () => {
    for (const c of chunks) {
      await new Promise((r) => setTimeout(r, 40));
      if (turn !== browserTurn) return browserHandlers?.error(turn, "cancelled", "Interrupted");
      browserHandlers?.delta(turn, c);
    }
    browserHandlers?.done(turn);
  })();
  return turn;
}

export async function chatCancel() {
  if (inTauri) await invoke("chat_cancel");
  else browserTurn++;
}

export function onChat(handlers: ChatHandlers) {
  browserHandlers = handlers;
  if (!inTauri) return;
  void listen<{ turn: number; text: string }>("chat-delta", (e) => handlers.delta(e.payload.turn, e.payload.text));
  void listen<{ turn: number }>("chat-done", (e) => handlers.done(e.payload.turn));
  void listen<{ turn: number; text: string }>("chat-activity", (e) => handlers.activity(e.payload.turn, e.payload.text));
  void listen<{ turn: number; kind: string; message: string }>("chat-error", (e) =>
    handlers.error(e.payload.turn, e.payload.kind, e.payload.message),
  );
}

export function onEvent(name: "open-chat" | "open-api-key" | "chat-reset" | "start-listening", cb: () => void) {
  if (inTauri) void listen(name, () => cb());
}

export function onFadeBottomChanged(cb: (on: boolean) => void) {
  if (inTauri) void listen<boolean>("fade-bottom-changed", (e) => cb(e.payload));
}

export function onFollowMouseChanged(cb: (on: boolean) => void) {
  if (inTauri) void listen<boolean>("follow-mouse-changed", (e) => cb(e.payload));
}

/** The tray picked a voice (or the model changed). `sample`: say a line in it. */
export function onVoicePicked(cb: (voice: string, sample: boolean) => void) {
  if (inTauri) void listen<{ voice: string; sample: boolean }>("voice-picked", (e) => cb(e.payload.voice, e.payload.sample));
}

export function onVoiceChanged(cb: (enabled: boolean) => void) {
  if (inTauri) void listen<boolean>("voice-changed", (e) => cb(e.payload));
  else browserVoiceListeners.push(cb);
}

// --- Voice input (whisper.cpp on the Rust side; see src-tauri/src/stt.rs).

export async function sttPrepare() {
  if (inTauri) await invoke("stt_prepare");
}

export async function transcribe(samples: Float32Array): Promise<string> {
  if (!inTauri) return `(heard ${(samples.length / 16000).toFixed(1)} s of audio; transcription needs the desktop app)`;
  const bytes = new Uint8Array(samples.buffer, samples.byteOffset, samples.byteLength);
  return invoke<string>("transcribe", bytes);
}

export function onSttProgress(cb: (percent: number) => void) {
  if (inTauri) void listen<{ percent: number }>("stt-progress", (e) => cb(e.payload.percent));
}

export async function pickClaudeCodeFolder(): Promise<string | null> {
  if (!inTauri) return null;
  return invoke<string | null>("pick_claude_code_folder");
}

export function onBrainChanged(cb: (brain: string) => void) {
  if (inTauri) void listen<string>("brain-changed", (e) => cb(e.payload));
}

export function onModelPicked(cb: (model: string) => void) {
  if (inTauri) void listen<string>("model-picked", (e) => cb(e.payload));
}

// Everything that talks to the Tauri shell lives here. In a plain browser
// (`npm run dev` opened directly, or tests) the same calls fall back to
// browser behaviour so the avatar can be developed without the desktop app.
import { Channel, convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import type { DanceInfo, DanceMotion } from "./dance";

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

/** Pops up the tray's options menu at the cursor. */
export function showMenu() {
  if (inTauri) void invoke("show_menu").catch((e) => console.warn("[menu]", e));
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

let narrowWidth = 0;

/** Keeps the chat bar, bubble and menus at the normal width while the window is wide. */
function keepOverlaysNarrow() {
  const inset = narrowWidth ? Math.max(0, (window.innerWidth - narrowWidth) / 2) : 0;
  document.documentElement.style.setProperty("--inset", `${inset}px`);
}

/** Widens the window while she dances (`wide`), or puts it back. */
export function danceWindow(wide: boolean) {
  if (!inTauri) return;
  if (wide && !narrowWidth) {
    narrowWidth = window.innerWidth;
    window.addEventListener("resize", keepOverlaysNarrow);
  } else if (!wide && narrowWidth) {
    narrowWidth = 0;
    window.removeEventListener("resize", keepOverlaysNarrow);
    keepOverlaysNarrow();
  }
  void invoke("dance_window", { wide }).catch((e) => console.warn("[window]", e));
}

// --- Claude chat (runs on the Rust side; see src-tauri/src/chat.rs).

export interface ChatStatus {
  /** Global hotkey labels; "" when unset or taken by another app. */
  hotkeys: { talk: string; listen: string; mute: string };
  /** Whether plain chat mode's provider has an API key. */
  has_key: boolean;
  /** Who answers in plain chat mode. */
  provider: "anthropic" | "gemini";
  name: string;
  model: string;
  voice_enabled: boolean;
  follow_mouse: boolean;
  fade_bottom: boolean;
  voice: string;
  voice_speed: number;
  voice_pitch: number;
  voice_device: string;
  /** "kokoro" or "pocket" (Pocket TTS, used only when the model has a cloned voice). */
  voice_engine: string;
  /** Whether the current model has a voice for Pocket TTS. */
  pocket_voice: boolean;
  brain: "api" | "claude_code" | "crew";
  claude_code_folder: string | null;
  /** Where the chat box sits: "above", "middle", "below" or a dragged fraction like "0.620". */
  toolbar_position: string;
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
  else browserHistory.push({ time: Date.now(), role, text, name: "Avatar", mode: "api" });
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

/** Saves the voice speed (0.5 to 2) and pitch (0.75 to 1.35) to settings. */
/** Saves where the chat box sits (a preset or a dragged fraction of the window height). */
export function setToolbarPosition(pos: string) {
  if (inTauri) void invoke("set_toolbar_position", { pos }).catch((e) => console.warn("[toolbar]", e));
}

export function onToolbarPositionChanged(cb: (pos: string) => void) {
  if (inTauri) void listen<string>("toolbar-position-changed", (e) => cb(e.payload));
}

export function setVoiceTuning(speed: number, pitch: number) {
  if (inTauri) void invoke("set_voice_tuning", { speed, pitch }).catch((e) => console.warn("[voice]", e));
}

/** Motion and expression files in the current model's folder (relative paths). */
export async function modelFiles(): Promise<string[]> {
  if (inTauri) return invoke<string[]>("model_files").catch(() => []);
  // Browser dev mode: ?files=a.exp3.json,b.motion3.json
  return (new URLSearchParams(location.search).get("files") ?? "").split(",").filter(Boolean);
}

/** The dances in the dances folder, with their files as URLs the page can load. */
export async function danceList(): Promise<DanceInfo[]> {
  if (!inTauri) return [];
  return invoke<DanceInfo[]>("dance_list").catch((e) => {
    console.warn("[dance]", e);
    return [];
  });
}

/** model://…/dance/<path> serves a file from the dances folder. */
export function danceUrl(path: string): string {
  const base = convertFileSrc("", "model").replace(/\/?$/, "/");
  return `${base}dance/${path.split("/").map(encodeURIComponent).join("/")}`;
}

export function danceMotion(d: DanceInfo): DanceMotion {
  return { name: d.name, urls: d.motions.map(danceUrl), audio: d.audio ? danceUrl(d.audio) : null };
}

/** Writes a line to the app's log file (and the console). */
export function log(level: "info" | "warn" | "error", message: string) {
  (level === "info" ? console.info : level === "warn" ? console.warn : console.error)(message);
  if (inTauri) void invoke("page_log", { level, message }).catch(() => {});
}

export async function chatStatus(): Promise<ChatStatus> {
  if (!inTauri) {
    return { hotkeys: { talk: "Ctrl+Shift+Space", listen: "Ctrl+Alt+M", mute: "Ctrl+Alt+U" }, has_key: true, provider: "anthropic", name: "Avatar", model: "demo", voice_enabled: true, follow_mouse: true, fade_bottom: true, voice: "af_heart", voice_speed: 1, voice_pitch: 1, voice_device: "cpu", voice_engine: "kokoro", pocket_voice: false, brain: "api", claude_code_folder: null, toolbar_position: new URLSearchParams(location.search).get("bar") ?? "middle" };
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

/** Forgets the conversation: the next message starts a fresh session. */
export async function chatReset() {
  if (inTauri) await invoke("chat_reset");
}

/** Claude Code's slash commands for the current folder, for the "/" menu. */
export async function codeCommands(): Promise<{ name: string; description: string; app: boolean }[]> {
  if (!inTauri) {
    return [
      { name: "clear", description: "Start a fresh session (forget this conversation)", app: true },
      { name: "folder", description: "Pick the folder Claude Code works in", app: true },
      { name: "tft", description: "Teamfight Tactics coach", app: false },
      { name: "code-review", description: "Review the current changes for bugs", app: false },
    ];
  }
  return invoke("code_commands");
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

// --- Native voice output (Kokoro on ONNX Runtime; see src-tauri/src/tts.rs).

/** Loads the native voice, downloading its model the first time. Resolves to the thread count. */
export async function ttsPrepare(): Promise<number> {
  if (!inTauri) throw new Error("the native voice needs the desktop app");
  return invoke<number>("tts_prepare");
}

/** Runs Kokoro on token ids and a style vector; returns 24 kHz samples. */
export async function ttsSynthesize(ids: number[], style: Float32Array, speed: number): Promise<Float32Array> {
  const bytes = await invoke<ArrayBuffer>("tts_synthesize", { ids, style: Array.from(style), speed });
  return new Float32Array(bytes);
}

export function onTtsProgress(cb: (percent: number) => void) {
  if (inTauri) void listen<{ percent: number }>("tts-progress", (e) => cb(e.payload.percent));
}

// --- Pocket TTS (Python sidecar; see src-tauri/src/pocket.rs).

/** Starts Pocket TTS and loads the model's cloned voice. Resolves to the sample rate. */
export async function pocketPrepare(): Promise<number> {
  if (!inTauri) throw new Error("Pocket TTS needs the desktop app");
  return invoke<number>("pocket_prepare");
}

export interface PocketStats {
  ms?: number;
  first_ms?: number;
  audio_s?: number;
  cancelled?: boolean;
}

/**
 * Speaks one sentence with Pocket TTS, calling `onChunk` with 16-bit PCM as it's
 * made. Resolves once the last chunk has been handed over.
 */
export async function pocketSpeak(text: string, onChunk: (pcm: Int16Array) => void): Promise<PocketStats> {
  if (!inTauri) throw new Error("Pocket TTS needs the desktop app");
  let ended: () => void = () => {};
  const end = new Promise<void>((resolve) => (ended = resolve));
  const channel = new Channel<ArrayBuffer>();
  channel.onmessage = (data) => {
    const bytes = data instanceof ArrayBuffer ? data : new Uint8Array(data as unknown as number[]).buffer;
    // An empty message marks the end (the reply can arrive before the last chunks).
    if (!bytes.byteLength) ended();
    else onChunk(new Int16Array(bytes, 0, bytes.byteLength >> 1));
  };
  const stats = await invoke<PocketStats>("pocket_speak", { text, onChunk: channel });
  await Promise.race([end, new Promise((resolve) => setTimeout(resolve, 2000))]);
  return stats;
}

/** Drops the sentences Pocket TTS is still making. */
export function pocketCancel() {
  if (inTauri) void invoke("pocket_cancel").catch(() => {});
}

/** The tray switched voice engine (or the model changed). */
export function onVoiceEngine(cb: (engine: string, pocketVoice: boolean, sample: boolean) => void) {
  if (inTauri)
    void listen<{ engine: string; pocket_voice: boolean; sample: boolean }>("voice-engine", (e) =>
      cb(e.payload.engine, e.payload.pocket_voice, e.payload.sample),
    );
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

// --- Crew mode (firstmate in WSL; see src-tauri/src/crew.rs).

export interface CrewTask {
  id: string;
  kind: string | null;
  project: string | null;
  /** working, needs-decision, blocked, paused, done, failed, resolved, pr-ready, merged */
  state: string;
  text: string;
  pr: string | null;
  updated: number;
  harness?: string | null;
  model?: string | null;
  started?: number;
  /** Set for finished (cleaned-up) tasks. */
  ended?: number | null;
  events?: CrewEvent[];
}

/** One line in a task's timeline. */
export interface CrewEvent {
  ts: number;
  /** dispatched, status, pr-ready, merged, cleaned-up */
  kind: string;
  state: string | null;
  text: string;
}

/** Everything the Crew window shows (see crew_fleet in src-tauri/src/lib.rs). */
export interface CrewFleet {
  on: boolean;
  direct: boolean;
  home: string;
  /** stopped, starting, busy, idle, or separate (notes mode) */
  mate: string;
  session: string | null;
  polled: number | null;
  error: string | null;
  tasks: CrewTask[];
  finished: CrewTask[];
}

export async function crewFleet(): Promise<CrewFleet | null> {
  return inTauri ? invoke<CrewFleet>("crew_fleet") : null;
}

export function crewWindowOpen() {
  if (inTauri) void invoke("crew_window_open");
}

export interface CrewAttention {
  task: string;
  kind: string;
  /** What she says, starting with an emotion tag. */
  speech: string;
}

export async function crewTasks(): Promise<CrewTask[]> {
  return inTauri ? invoke<CrewTask[]>("crew_tasks") : [];
}

export async function crewOpenTerminal() {
  if (inTauri) await invoke("crew_open_terminal");
}

export async function openUrl(url: string) {
  if (inTauri) await invoke("open_url", { url });
  else window.open(url, "_blank");
}

export function onCrewTasks(cb: (tasks: CrewTask[]) => void) {
  if (inTauri) void listen<CrewTask[]>("crew-tasks", (e) => cb(e.payload));
}

/** The first mate's answer to a message (published with fm-inbox.sh reply). */
/** Claude Code asked the app for a fresh session (kind "session") or a new terminal window ("window"). */
export interface CodeHandoff {
  turn: number;
  kind: "session" | "window" | "switch";
  brief: string;
  folder: string;
  error: string | null;
}

export function onCodeHandoff(cb: (h: CodeHandoff) => void) {
  if (inTauri) void listen<CodeHandoff>("code-handoff", (e) => cb(e.payload));
}

export function onCrewReply(cb: (body: string) => void) {
  // Crew replies come as { id, body }; the first mate's own messages as a plain string.
  if (inTauri) void listen<{ id: string; body: string } | string>("crew-reply", (e) => cb(typeof e.payload === "string" ? e.payload : e.payload.body));
}

/** A task needs you, failed, or has a PR ready. */
export function onCrewAttention(cb: (a: CrewAttention) => void) {
  if (inTauri) void listen<CrewAttention>("crew-attention", (e) => cb(e.payload));
}

export function onCrewError(cb: (message: string) => void) {
  if (inTauri) void listen<string>("crew-error", (e) => cb(e.payload));
}

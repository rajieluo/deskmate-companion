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
}

export interface ModelSource {
  url: string;
  label: string;
}

function toSource(info: ModelInfo): ModelSource {
  // e.g. http://model.localhost/3/1014100.model3.json on Windows.
  const base = convertFileSrc("", "model").replace(/\/?$/, "/");
  return { url: `${base}${info.generation}/${encodeURIComponent(info.file_name)}`, label: info.path };
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
  const width = Math.round(Math.min(1400, Math.max(160, size.width * factor)));
  const height = Math.round((width * size.height) / size.width);
  await w.setSize(new LogicalSize(width, height));
}

// --- Claude chat (runs on the Rust side; see src-tauri/src/chat.rs).

export interface ChatStatus {
  has_key: boolean;
  name: string;
  model: string;
}
export interface ChatHandlers {
  delta: (turn: number, text: string) => void;
  done: (turn: number) => void;
  error: (turn: number, kind: string, message: string) => void;
}

let browserTurn = 0;
let browserHandlers: ChatHandlers | null = null;

export async function chatStatus(): Promise<ChatStatus> {
  if (!inTauri) return { has_key: true, name: "Aqua", model: "demo" };
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
  void listen<{ turn: number; kind: string; message: string }>("chat-error", (e) =>
    handlers.error(e.payload.turn, e.payload.kind, e.payload.message),
  );
}

export function onEvent(name: "open-chat" | "open-api-key" | "chat-reset", cb: () => void) {
  if (inTauri) void listen(name, () => cb());
}

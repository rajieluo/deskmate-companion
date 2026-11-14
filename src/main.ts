import { Avatar } from "./avatar";
import { ChatUI } from "./chat";
import { PosePicker } from "./picker";
import * as host from "./host";
import { EMOTIONS } from "./motions";
import { BeepEngine, KokoroEngine, Voice } from "./voice";

const canvas = document.getElementById("stage") as HTMLCanvasElement;
const statusBox = document.getElementById("status") as HTMLDivElement;
const statusText = document.getElementById("status-text") as HTMLParagraphElement;
const statusPick = document.getElementById("status-pick") as HTMLButtonElement;

const avatar = new Avatar(canvas);
const kokoro = new KokoroEngine();
const useBeep = !host.inTauri && new URLSearchParams(location.search).get("tts") === "beep";
const voice = new Voice(useBeep ? new BeepEngine() : kokoro);
avatar.setMouthSource(() => voice.level());
(window as unknown as { voice: Voice }).voice = voice;
const poses = new PosePicker(avatar);
const chat = new ChatUI(avatar, voice);
kokoro.onStatus = (s) => chat.status(s);
void host.chatStatus().then((s) => {
  voice.enabled = s.voice_enabled;
  avatar.setFollowMouse(s.follow_mouse);
  avatar.setFade(s.fade_bottom);
  kokoro.voice = s.voice;
  kokoro.speed = s.voice_speed;
  kokoro.device = s.voice_device === "gpu" ? "gpu" : "cpu";
  // Start the (large, first-time) model download early instead of on the first reply.
  if (s.voice_enabled && !useBeep) kokoro.warmUp();
});
host.onFollowMouseChanged((on) => avatar.setFollowMouse(on));
host.onFadeBottomChanged((on) => avatar.setFade(on));
host.onVoiceChanged((on) => {
  voice.enabled = on;
  if (!on) voice.stop();
  else if (!useBeep) kokoro.warmUp();
});
// Exposed for debugging from the devtools console and for tests.
(window as unknown as { avatar: Avatar }).avatar = avatar;

function showStatus(text: string | null, { pick = false } = {}) {
  statusBox.hidden = text === null;
  statusText.textContent = text ?? "";
  statusPick.hidden = !pick;
  // The panel must stay clickable even though the rest of the window is click-through.
  statusBox.toggleAttribute("data-hit", text !== null);
}

window.addEventListener("error", (e) => host.log("error", `[page] ${e.message} at ${e.filename}:${e.lineno}`));
window.addEventListener("unhandledrejection", (e) => host.log("error", `[page] unhandled: ${String(e.reason?.stack ?? e.reason)}`));

async function load(source: host.ModelSource | null) {
  if (!("Live2DCubismCore" in window)) {
    showStatus(
      "Live2D Cubism Core is missing.\nRun `npm install` (it downloads public/vendor/live2dcubismcore.min.js) and restart.",
    );
    return;
  }
  if (!source) {
    showStatus("No Live2D model chosen yet.\nPick a .model3.json file from your models folder.", { pick: true });
    return;
  }
  showStatus("Loading model…");
  try {
    const files = await host.modelFiles();
    const summary = await avatar.load(source.url, source.look, source.params, files);
    await poses.restore(source.label);
    showStatus(null);
    host.log("info", `[avatar] loaded ${source.label}: ${JSON.stringify(summary)}; look ${JSON.stringify(avatar.lookParams)}`);
    // A second later, once idle motions have started, record the mouth state.
    setTimeout(() => void avatar.diagnostics().then((d) => host.log("info", `[avatar] ${d}`)), 1500);
    document.body.dataset.ready = "true";
  } catch (err) {
    if ((err as Error).message === "superseded") return;
    host.log("error", `[avatar] couldn't load ${source.label}: ${(err as Error).message}`);
    showStatus(`Couldn't load ${source.label}\n${(err as Error).message}`, { pick: true });
  }
}

statusPick.addEventListener("click", async () => {
  const source = await host.pickModel();
  if (source) void load(source);
});
host.onModelChanged((source) => void load(source));
host.onModelError((message) => showStatus(message, { pick: true }));

// --- Click-through: the window ignores the mouse except over the character or UI.
let misses = 0;
host.onCursor((x, y, inside) => {
  const dpr = window.devicePixelRatio || 1;
  avatar.lookAt(x / dpr, y / dpr);
  if (!inside) {
    host.setClickThrough(true);
    return;
  }
  const el = document.elementFromPoint(x / dpr, y / dpr);
  const hit = !!el?.closest("[data-hit]") || avatar.hitTest(x, y);
  if (hit) {
    misses = 0;
    host.setClickThrough(false);
  } else if (++misses >= 3) {
    // A few misses in a row before letting go, so edges don't flicker.
    host.setClickThrough(true);
  }
});

// --- Drag to move, click to react.
let press: { x: number; y: number } | null = null;
canvas.addEventListener("mousedown", (e) => {
  if (e.button === 0) press = { x: e.clientX, y: e.clientY };
});
canvas.addEventListener("mousemove", (e) => {
  if (press && e.buttons & 1 && Math.hypot(e.clientX - press.x, e.clientY - press.y) > 4) {
    press = null;
    host.startDrag();
  }
});
canvas.addEventListener("mouseup", () => {
  // A click (no drag) opens or closes the chat box.
  if (press) chat.toggle();
  press = null;
});

// --- Ctrl + mouse wheel resizes the window (and the character with it).
window.addEventListener(
  "wheel",
  (e) => {
    if (!e.ctrlKey) return;
    e.preventDefault();
    void host.resizeWindow(e.deltaY < 0 ? 1.1 : 1 / 1.1);
  },
  { passive: false },
);

// --- Number keys 1-7 play each emotion, for checking a model's motions.
window.addEventListener("keydown", (e) => {
  if (e.target instanceof HTMLInputElement) return;
  const i = Number(e.key) - 1;
  if (i >= 0 && i < EMOTIONS.length) avatar.emote(EMOTIONS[i]);
});

void host.currentModel().then(load);

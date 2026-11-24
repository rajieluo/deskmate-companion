# Desktop Assistant

A Claude-powered (or, free, Gemini-powered) Live2D or MMD character that lives on your Windows desktop.

It runs in a small transparent, always-on-top window. Clicks pass through to whatever is behind it except on the character itself, so it can sit on top of your other windows.

## Status

| Milestone | State |
|---|---|
| 1. Transparent on-top window, Live2D model idling, drag, click-through | done |
| 2. Typed chat with Claude (streaming), emotion tags drive motions | done |
| 3. Local Kokoro TTS with lip sync | done |
| 4. Local whisper.cpp voice input | done |

## Getting started

See **[INSTALL.md](INSTALL.md)** for the requirements of each mode (which items are required and which are optional), step-by-step setup, building from source, and troubleshooting. In short:

| Mode | What it needs |
|---|---|
| Plain chat | The app, a Live2D or MMD model, and either an Anthropic API key with prepaid credits (the free Claude plan has no API access) or a free Google Gemini API key ([setup](#free-voice-chat-with-google-gemini)) |
| Claude Code mode | Claude Code, signed in with a paid Claude plan or an API key |
| Crew mode (firstmate) | Claude Code plus the firstmate setup: Git for Windows (Git Bash), Node.js and npm, Herdr, a GitHub login (`gh`) and a few more tools |
| Voice output | Nothing extra: Kokoro runs locally for free. Kyutai Pocket TTS (cloned voices) is optional; the tray's *Set up Pocket TTS…* installs it |
| Voice input | A CPU with AVX2 (most PCs from 2015 on) and a microphone |

INSTALL.md also covers [loading your own Live2D models, choosing or adding voices, and creating custom personas](INSTALL.md#5-personas-and-voices-for-each-model).

## Using it

- **First run** asks you to pick a `.model3.json` (Live2D) or `.pmx` (MMD) file.
- **Tray icon** (bottom right of the taskbar): *Choose model…*, *Reset position*, *Reload*, *Quit*.
- **Right-click** her to open the same options menu as the tray icon.
- **Drag** the character to move her. The position and size are remembered.
- **Click** her (or press **Ctrl+Shift+Space** anywhere) to open the chat box. Type and press Enter; Esc closes it.
- The first time you chat she asks for an [Anthropic API key](https://console.anthropic.com/settings/keys). It is stored in Windows Credential Manager, never in a file. `ANTHROPIC_API_KEY` in the environment also works. *Set API key…* in the tray changes it. An Anthropic key needs prepaid API credits; a free Claude plan can't be used by other apps.
- **Free option: Google Gemini.** She can chat (typed or out loud) on Gemini's free tier instead of Claude. See [Free voice chat with Google Gemini](#free-voice-chat-with-google-gemini) below.
- *Forget conversation* in the tray starts a fresh conversation.
- **Talk to her out loud**: press the microphone button in the chat box, or **Ctrl+Alt+M** from anywhere. Speak, and she stops listening after a short pause. Your words are transcribed locally with [whisper.cpp](https://github.com/ggml-org/whisper.cpp) and sent as a message. The first time, the Whisper model (`base.en`, about 140 MB) downloads into `%APPDATA%\com.rajieluo.desktopassistant\whisper\`. Windows may ask for microphone permission once.
- She **speaks her replies** with [Kokoro-82M](https://huggingface.co/hexgrad/Kokoro-82M), running locally, and her mouth follows the audio. The voice model (about 90 MB) downloads from Hugging Face the first time and is cached after that. It runs on the CPU by default (see `voice_device` below).
- **Mute** her with the speaker button in the chat box, **Ctrl+Alt+U** from anywhere, or *Speak replies* in the tray. All three stay in step, and the choice is remembered.
- **Chat history**: the clock button in the chat box, or *Chat history…* in the tray, opens a window with every message, grouped by day and searchable. Messages are saved to `%APPDATA%\com.rajieluo.desktopassistant\history.jsonl`; *Clear…* in that window deletes them.
- **Ctrl + mouse wheel** over her makes the window bigger or smaller.
- **Keys 1 to 7** (after clicking her) play happy, sad, angry, surprised, shy, thinking and proud, so you can check a model's motions.

## Sharing it with a friend

Give them the installer and a link to [INSTALL.md](INSTALL.md); they bring their own model, key and voice. How to build the installer, what stays private, and what you may and may not pass on is in [Sharing the app with a friend](INSTALL.md#sharing-the-app-with-a-friend).

## Free voice chat with Google Gemini

If you don't have prepaid Anthropic credits, plain chat can use Google Gemini's free tier instead. Only the "brain" changes: your voice is still transcribed locally by Whisper, and her replies are still spoken locally by Kokoro or Pocket TTS, so voice chat works the same way and costs nothing.

### 1. Get a free Gemini API key

1. Go to [Google AI Studio's API keys page](https://aistudio.google.com/apikey) and sign in with a Google account.
2. Accept the terms if it asks (first visit only).
3. Click **Create API key**. If it asks for a Google Cloud project, let it create one for you.
4. Copy the key. It's a long string that usually starts with `AIza`. Treat it like a password: don't paste it into chats, screenshots or files you share.

No credit card is needed for the free tier.

### 2. Switch her to Gemini

Right-click her (or the tray icon), open **Chat model**, and pick **Google Gemini (free tier)**. This starts a fresh conversation.

Also make sure **Claude Code mode** and **Crew mode (firstmate)** are unticked in the same menu. Gemini only answers in plain chat mode; those two modes always use Claude.

If you prefer editing settings by hand, close the app and set `"chat_provider": "gemini"` in `settings.json` (see [Chat settings](#chat-settings)). `"chat_provider": "anthropic"` switches back to Claude.

### 3. Enter the key

Click her to open the chat box and send any message. Because no Gemini key is saved yet, she asks: *"I need a Google Gemini API key to talk…"* and shows a key box. Paste the key there and press Enter.

- The key is saved in **Windows Credential Manager** (under `desktop-assistant`, as `gemini-api-key`), never in a file. It is kept separately from the Anthropic key, so you can switch between Claude and Gemini without re-entering either.
- To change it later, pick Gemini in **Chat model** first, then use **Set API key…** in the menu. That button always sets the key of the provider that is currently picked.
- Alternatively, set a `GEMINI_API_KEY` environment variable (for example `setx GEMINI_API_KEY "your-key-here"` in a terminal, then restart the app). If it is set, it takes priority over the saved key.

### 4. Check it works

1. Type "hi" in the chat box. A reply should stream into her speech bubble after a moment, and she should say it out loud.
2. Press the microphone button (or **Ctrl+Alt+M**), say something, and wait for the short pause. Your words appear as a message and she answers out loud. That's voice chat on Gemini.
3. **Chat history…** in the menu shows the exchange, if you want to double-check.

If something is off, her bubble tells you what went wrong:

| What she says | What it means |
|---|---|
| "That API key didn't work." and the key box again | The key was mistyped, deleted or restricted. Copy it again from [AI Studio](https://aistudio.google.com/apikey), or create a new one, and paste it. If you set `GEMINI_API_KEY`, check that too, since it wins over the saved key. |
| "Too many messages for Gemini's free tier in one minute…" | You hit the per-minute limit. Wait a minute. |
| "Gemini's free daily limit has been reached…" | You hit the daily limit. It resets at midnight Pacific time. You can see your current limits at [aistudio.google.com/rate-limit](https://aistudio.google.com/rate-limit). |
| "Gemini API error 404: …" | The `gemini_model` in `settings.json` doesn't exist (or no longer does). Remove the line to go back to the default. |
| A Claude-sounding reply, or a Claude Code status like "Reading main.ts…" | Claude Code mode or Crew mode is still on. Untick it. |

### Good to know

- The default model is `gemini-3.5-flash-lite`, which is fast and covered by the free tier. You can pick another free one, such as `gemini-3.6-flash` or `gemini-3.8-flash` (smarter but slower), with `gemini_model` in `settings.json`. [Google's pricing page](https://ai.google.dev/gemini-api/docs/pricing) lists which models are free.
- `chat_effort` also applies to Gemini 3 and later models (it sets their thinking level).
- Google may use free-tier conversations to improve its products. Use a Claude key, or a paid Gemini plan, for anything private.
- To go back to Claude, pick one of the Claude models in **Chat model**.

## Claude Code mode

Instead of plain chat, she can be a voice and face for [Claude Code](https://claude.com/claude-code) working in a folder of yours (a code project, notes, anything).

1. Install Claude Code and run `claude` once in a terminal to sign in, with a paid Claude plan or an API key. This mode uses Claude Code's own login, not the key you gave the app.
2. In the tray, pick **Claude Code folder…** and choose the folder. That switches on **Claude Code mode** (toggle it in the tray to go back to normal chat).
3. Talk or type as usual: "what does this project do?", "add a test for the parser", "fix the failing build".

Each message runs `claude -p` in that folder and continues the same Claude Code session until you pick *Forget conversation*. While it works, the bubble shows what it's doing ("Reading main.ts…", "Running npm test…"), and the reply is spoken and drives her emotions like in normal chat.

Permissions: there is no one to click "Allow" in this mode, so it runs with `acceptEdits` by default. Claude Code can read and edit files in the folder, but shell commands outside its safe list are refused. Change `claude_code_permission_mode` in `settings.json` to `auto` (lets Claude Code's classifier allow safe commands) or another mode if you want more.

## Crew mode (firstmate)

Crew mode makes her the face of [firstmate](https://github.com/kunchenguid/firstmate), which runs several Claude Code sessions in parallel, each in its own git worktree. You talk to her; she passes it to firstmate's first mate, reads out its answers, and shows a badge for each task (blue working, amber needs you, green PR ready, red failed). *Crew sessions…* in the tray opens a window listing every crewmate with its status and timeline. Firstmate runs natively under Git Bash by default, or in WSL2. Setup: [INSTALL.md](INSTALL.md#8-optional-crew-mode-firstmate); details: [docs/crew-setup.md](docs/crew-setup.md).

### Announcements from the first mate

The first mate can also start talking on its own, for example to say a job finished and its pull request is ready. It writes one file per announcement into `state/avatar-say/` under its home (create the folder if it's missing):

- One plain-text `.txt` file per announcement, holding one spoken line in the same shape as a reply: it starts with one emotion tag (such as `[happy]`), then one to three short sentences, no Markdown. Example: `[happy] fix login is done. Its pull request is ready for you.`
- Name files so lexical order is creation order, e.g. `<epoch-seconds>-<random>.txt`. They are said oldest first.
- Write under a temporary name that doesn't end in `.txt` and then rename it, so a half-written file is never read: `printf '%s\n' "$line" > state/avatar-say/$name.tmp && mv state/avatar-say/$name.tmp state/avatar-say/$name.txt`.

She checks the folder at every poll (every 4 seconds) and speaks each announcement once, just like a reply. Each file is deleted as soon as it's picked up. Announcements written while the app was closed are said the next time Crew mode starts. Empty or unreadable files are deleted without being said, and anything past the first 8 KB of a file is cut off.

## Chat settings

Chat settings live in `settings.json` in `%APPDATA%\com.rajieluo.desktopassistant\` (created on first run). Edit it while the app is closed:

```json
{
  "model_path": "C:\\Users\\you\\Documents\\live2d models\\MyModel\\MyModel.model3.json",
  "character_name": null,
  "chat_provider": "anthropic",
  "chat_model": "claude-opus-5-5",
  "gemini_model": "gemini-3.5-flash-lite",
  "chat_effort": "low",
  "persona": null,
  "voice_enabled": true,
  "voice": "af_heart",
  "voice_speed": 1.0,
  "voice_pitch": 1.0,
  "stt_model": "base.en",
  "stt_language": "en",
  "brain": "api",
  "claude_code_folder": null,
  "claude_code_permission_mode": "acceptEdits"
}
```

- `character_name`: what she calls herself. Leave it `null` to use the model's folder name, tidied up: `Nicole` stays Nicole, `1014100aqua` becomes Aqua, `mori_suit` becomes Mori Suit.
- `chat_provider`: who answers in plain chat mode, `"anthropic"` (default, Claude) or `"gemini"` (Google Gemini, which has a free tier). The tray's **Chat model** submenu sets it too.
- `chat_model`: any Claude model id. The tray's **Chat model** submenu switches between Opus 5.5, Sonnet 5.5 and Haiku 5.5 without editing the file. The default is Claude Opus 5.5. `claude-haiku-5-5` is much cheaper and faster if replies are good enough for you.
- `gemini_model`: the Gemini model id used when `chat_provider` is `"gemini"`. The default, `gemini-3.5-flash-lite`, is a stable model the free tier covers; `gemini-3.6-flash` and `gemini-3.8-flash` are also free but smarter and slower. [Google's pricing page](https://ai.google.dev/gemini-api/docs/pricing) lists which models are free.
- `chat_effort`: `low` (default, quick chatty replies), `medium`, `high`, `xhigh` or `max`. With Gemini 3 and later models it sets the thinking level (`xhigh` and `max` both mean `XHIGH`); Gemini 2.x models ignore it.
- `voice_enabled`, `voice`, `voice_speed`, `voice_pitch`: speech on or off, the Kokoro voice (`af_heart` by default; also `af_bella`, `af_nicole`, `af_sky`, `bf_emma` and [others](https://huggingface.co/hexgrad/Kokoro-82M/blob/main/VOICES.md)), speed (0.5 to 2) and pitch (0.75 to 1.35). Speed and pitch also have sliders at the bottom of the poses panel; double-click a slider to reset it.
- `voice_engine`: `"kokoro"` (default) or `"pocket"` for [Kyutai Pocket TTS](https://github.com/kyutai-labs/pocket-tts), which speaks in a voice cloned from a short recording and starts talking about 0.1 s after each sentence. The tray's **Voice** submenu switches between them. Pocket TTS is only used for models that have a cloned voice: `voice.safetensors` in the model folder (make it with `pocket-tts export-voice clip.wav voice.safetensors`), else `voice.wav` (10 to 30 s of clean speech; it is encoded once and cached), or `pocket_voice:` in `persona.md`'s front matter naming a file in the folder or a preset such as `alba`. Other models keep using Kokoro, and so does every sentence after Pocket TTS fails, so she never goes quiet. Speed and pitch apply to Kokoro only.
- `pocket_python`, `pocket_quantize`: Pocket TTS runs in a Python sidecar ([setup](INSTALL.md#6-optional-cloned-voices-with-pocket-tts)). Point `pocket_python` at the `python.exe` of a venv where `pocket-tts` is installed (default: `pocket\venv\Scripts\python.exe` in the app's data folder, which *Voice > Set up Pocket TTS…* creates). The last Python that worked is also remembered in `pocket\python.txt` in the data folder, so Pocket keeps working if `pocket_python` goes missing from settings.json. Voice cloning needs the gated weights: accept the terms at https://huggingface.co/kyutai/pocket-tts and log in once (the setup offers to). `pocket_quantize` (default `true`) runs the model in int8, about 20% faster on the CPU.
- `stt_model`, `stt_language`: the Whisper model for voice input (`tiny.en`, `base.en` by default, `small.en` for better accuracy, or multilingual `small` with `"stt_language": "auto"`) and the spoken language.
- `voice_device`: where the voice runs, `"cpu"` (default) or `"gpu"` (DirectML, any DirectX 12 card). The GPU is faster, but on some cards (an RX 7900 XT, for one) it garbles the speech; set it back to `"cpu"` if that happens (see [Troubleshooting](INSTALL.md#troubleshooting)).
- `voice_model`: the Kokoro model variant, `q8` (default, about 90 MB), `q8f16`, `fp16` or `fp32` (about 325 MB). Use whichever runs fastest on your CPU.
- `model_params`: parameter values to hold on a model every frame, keyed by its folder name, e.g. `"model_params": { "Nicole": { "MouthJawOpen": 0 } }`. Lip sync still moves the mouth while she speaks. The log lists each model's mouth parameters and their values when it loads.
- `fade_bottom`: softly fade out the bottom edge of the figure (default `true`), for models whose art stops at the legs. The tray has a **Fade bottom edge** checkbox for it.
- `hotkeys`: the global hotkeys, e.g. `{ "talk": "Ctrl+Shift+Space", "listen": "Ctrl+Alt+M", "mute": "Ctrl+Alt+U" }` (those are the defaults). Use `""` to turn one off. If another app already uses a combination, the log says so and the tray shows no hotkey for it.
- `follow_mouse`: whether her head and eyes follow the cursor (default `true`). The tray has a **Follow the mouse** checkbox for it.
- `look_params`: which parameters follow the mouse, per model folder. The app finds the head, eye and
  body parameters by name on its own, in both the `ParamAngleX` and the older `PARAM_ANGLE_X` styles. For a
  model with generic names like `Param12`, name them yourself, e.g.
  `"look_params": { "xch003_01": { "angle_x": "Param12", "angle_y": "Param13", "eye_x": "Param20" } }`.
  The keys are `angle_x`, `angle_y`, `angle_z`, `eye_x`, `eye_y`, `body_x` and `breath`, and `""` turns one off.
  The parameters chosen for the current model are printed in the dev tools console as "mouse-follow parameters".
- `persona`: replaces the built-in personality prompt for every model. Keep the line asking for one emotion tag per reply (`[happy] [sad] [angry] [surprised] [shy] [thinking] [proud] [neutral]`), since the tags are what make her move. A per-model persona file (below) takes priority.

### Per-model personas

Each model can have its own personality in a persona file: `personas\<model folder name>.md` in the app's data folder, or `persona.md` in the model's own folder. Front matter in the model folder's `persona.md` can also set the model's voice (`voice:`, `pocket_voice:`). File locations, lookup order and format are in [INSTALL.md](INSTALL.md#personas). Persona files for characters you don't own stay on your machine; don't commit them here.

Claude starts each reply with an emotion tag such as `[happy]`. The app strips the tags from the text and plays the matching motion. The API call is made from the Rust side, so the key never reaches the web page. Server-side refusal fallbacks are turned on, so if Claude Opus 5.5 declines a message the API retries it on a fallback model in the same call.

## If something goes wrong

The app writes a log to `%LOCALAPPDATA%\com.rajieluo.desktopassistant\logs\Desktop Assistant.log`. Known problems and fixes are in [INSTALL.md](INSTALL.md#troubleshooting).

## Models

Models are loaded from your own folder and are **never committed** to this repo (`.gitignore` blocks Live2D and MMD model files). The app only reads files inside the folder of the model you picked.

Any Cubism 3/4 model (`.model3.json`) works; load one with **Choose model…** in the tray ([details](INSTALL.md#2-get-a-model-and-pick-it)). Many models put all their motions in one unnamed group, so the app sorts them by file name:

- `Wait`, `Idle`, `Stand`, `Breath`… become the idle loop.
- `Happy`/`Smile`, `Sad`/`Cry`/`Upset`, `Anger`, `Surprise`, `Shame`/`Shy`, `Puzzle`/`Doubt`/`Serious`, `Pride` become emotions (including the pinyin names some models use, like `weixiao` or `shengqi`).
- Names containing `Expression`, `face_` or `eye_` are treated as face-only overlays.
- Everything else becomes a generic reaction, used when a model has no motion for an emotion.

The app measures where the character actually is inside the model's canvas and crops the empty space around her (Aqua has a lot below her feet), so she fills the window.

### MMD models (.pmx / .pmd)

Pick a `.pmx` (or older `.pmd`) file instead of a `.model3.json` and the app draws the model in 3D with [babylon-mmd](https://github.com/noname0310/babylon-mmd), on the same transparent window. Live2D models keep working as before; the 3D renderer is only downloaded when an MMD model is loaded.

- **Face**: blinking, lip sync and emotions use the model's morphs, found by their usual MMD names (`まばたき` blink, `あ` mouth open, `笑い`/`にっこり` happy, `怒り` angry, `悲しむ`/`困る` sad, `びっくり` surprised, `照れ` blush…), with English fallbacks. Every morph is also listed in the poses picker to toggle on and off.
- **Motion**: MMD models stand in a T-pose and don't come with idle animations. Put `.vmd` motion files anywhere in the model's folder: one whose name looks like an idle (`idle`, `wait`, `stand`, `breath`, `loop`…) loops on its own, emotion names (`happy.vmd`, `sad.vmd`…) play once when she feels that way, and every `.vmd` is listed under Animations in the poses picker. Without any, the app lowers her arms and adds gentle breathing and sway.
- **Hair and clothes** swing with the model's own physics (Bullet, compiled to WebAssembly).
- She turns her head and eyes towards the cursor, and click-through works the same as for Live2D.

Check each model's readme for its terms; many MMD models allow personal use only.

## Developing

- `npm run dev` serves the web part on http://localhost:1420. In a normal browser you can load a model with `?model=/path/to/model.model3.json` (relative to the repo, e.g. a model copied into `tests/fixtures/`, which is git-ignored).
- Add `&tts=beep` to the dev URL to replace Kokoro with beeps, for testing lip sync without downloading the voice model.
- MMD models load the same way, e.g. `?model=/dev-mmd/ellen/model.pmx` with the model folder linked under `public/dev-mmd/` (git-ignored). Add `&files=motions/idle.vmd` to try a motion.
- `npm run fixtures` downloads Live2D's free sample model Hiyori into `tests/fixtures/` for testing.
- `npm test` runs the web unit tests, `cargo test` (in `src-tauri/`) the Rust ones.

### How click-through works

Tauri can only make a whole window ignore the mouse. So the Rust side polls the global cursor position about 30 times a second and sends it to the page. The page reads the alpha of the pixel under the cursor; over the character (or any element marked `data-hit`) it turns mouse events on, elsewhere it turns them off.

### Third-party code

- [Live2D Cubism Core](https://www.live2d.com/en/sdk/license/) is downloaded at install time and is subject to the Live2D Proprietary Software License. It is free for individuals and small businesses.
- [pixi-live2d-display](https://github.com/guansss/pixi-live2d-display) (MIT) renders the Live2D models.
- [Babylon.js](https://github.com/BabylonJS/Babylon.js) (Apache-2.0) and [babylon-mmd](https://github.com/noname0310/babylon-mmd) (MIT) render MMD models.
- [whisper.cpp](https://github.com/ggml-org/whisper.cpp) (MIT) through [whisper-rs](https://github.com/tazz4843/whisper-rs) (Unlicense) does speech to text.
- [kokoro-js](https://www.npmjs.com/package/kokoro-js) (Apache-2.0) and the Kokoro-82M model (Apache-2.0) do text to speech.
- [Kyutai Pocket TTS](https://github.com/kyutai-labs/pocket-tts) is optional and not bundled: the setup script downloads it (with [uv](https://github.com/astral-sh/uv) and PyTorch) from PyPI on the user's PC, under its own license, and its voice-cloning weights are gated on Hugging Face.

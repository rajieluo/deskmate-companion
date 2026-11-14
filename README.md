# Desktop Assistant

A Claude-powered Live2D character that lives on your Windows desktop.

It runs in a small transparent, always-on-top window. Clicks pass through to whatever is behind it except on the character itself, so it can sit on top of your other windows.

## Status

| Milestone | State |
|---|---|
| 1. Transparent on-top window, Live2D model idling, drag, click-through | done |
| 2. Typed chat with Claude (streaming), emotion tags drive motions | done |
| 3. Local Kokoro TTS with lip sync | done |
| 4. Local whisper.cpp voice input | done |

## Running it

You need [Node.js 22+](https://nodejs.org/), [Rust](https://rustup.rs/), the [Tauri prerequisites for Windows](https://v2.tauri.app/start/prerequisites/#windows) (WebView2 is already on Windows 10/11), and for building whisper.cpp, [CMake](https://cmake.org/download/) and [LLVM](https://github.com/llvm/llvm-project/releases) (`winget install Kitware.CMake LLVM.LLVM`).

```powershell
npm install          # also downloads the Live2D Cubism Core runtime
npm run tauri dev    # run the app with hot reload
npm run tauri build  # build an installer in src-tauri/target/release/bundle/nsis
```

Every pull request also builds a Windows installer in CI; download it from the run's **Artifacts** section.

### Building locally on Windows

CI takes 20+ minutes; a local build is much faster (about 1 minute for the first Rust build on a recent PC, seconds after that).

1. **Rust** (MSVC toolchain): `winget install Rustlang.Rustup`, then open a new terminal and run `rustup default stable-x86_64-pc-windows-msvc`.
2. **C++ build tools**: Visual Studio 2022 or its Build Tools with the *Desktop development with C++* workload (MSVC and a Windows SDK). `winget install Microsoft.VisualStudio.2022.BuildTools`, then add the workload in the Visual Studio Installer.
3. **A recent CMake** for whisper.cpp. Visual Studio ships one; if an older standalone CMake comes first on `PATH`, either `winget install Kitware.CMake`, or put the bundled one first for the session:
   ```powershell
   $env:Path = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin;$env:Path"
   ```
4. **libclang** (whisper-rs generates its bindings with it). Either `winget install LLVM.LLVM` (needs admin), or, without admin, take the DLL from the `libclang` Python wheel and point `LIBCLANG_PATH` at it:
   ```powershell
   python -m pip install --target "$env:LOCALAPPDATA\libclang-py" libclang
   $env:LIBCLANG_PATH = "$env:LOCALAPPDATA\libclang-py\clang\native"
   ```
   Don't set `WHISPER_DONT_GENERATE_BINDINGS`: the bundled bindings are generated for Linux and fail to compile on Windows.
5. **Node.js 22+** and WebView2 (already on Windows 10/11).

Then `npm install` and `npm run tauri dev`. Close the installed Desktop Assistant first, or the two fight over the global hotkeys.

Debugging tips:

- Set `$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9222"` before `npm run tauri dev` to attach Edge DevTools (or a script) to the app's pages at `http://127.0.0.1:9222`.
- If Vite is already running on port 1420, `cargo run` in `src-tauri/` starts just the app against it.

## Using it

- **First run** loads `Documents\live2d models\1014100aqua\1014100.model3.json` if it exists. Otherwise it asks you to pick a `.model3.json` file.
- **Tray icon** (bottom right of the taskbar): *Choose model…*, *Reset position*, *Reload*, *Quit*.
- **Right-click** her to open the same options menu as the tray icon.
- **Drag** the character to move her. The position and size are remembered.
- **Click** her (or press **Ctrl+Shift+Space** anywhere) to open the chat box. Type and press Enter; Esc closes it.
- The first time you chat she asks for an [Anthropic API key](https://console.anthropic.com/settings/keys). It is stored in Windows Credential Manager, never in a file. `ANTHROPIC_API_KEY` in the environment also works. *Set API key…* in the tray changes it.
- *Forget conversation* in the tray starts a fresh conversation.
- **Talk to her out loud**: press the microphone button in the chat box, or **Ctrl+Alt+M** from anywhere. Speak, and she stops listening after a short pause. Your words are transcribed locally with [whisper.cpp](https://github.com/ggml-org/whisper.cpp) and sent as a message. The first time, the Whisper model (`base.en`, about 140 MB) downloads into `%APPDATA%\com.rajieluo.desktopassistant\whisper\`. Windows may ask for microphone permission once.
- She **speaks her replies** with [Kokoro-82M](https://huggingface.co/hexgrad/Kokoro-82M), running locally, and her mouth follows the audio. The voice model (about 90 MB) downloads from Hugging Face the first time and is cached after that. It runs on the CPU by default (see `voice_device` below).
- **Mute** her with the speaker button in the chat box, **Ctrl+Alt+U** from anywhere, or *Speak replies* in the tray. All three stay in step, and the choice is remembered.
- **Chat history**: the clock button in the chat box, or *Chat history…* in the tray, opens a window with every message, grouped by day and searchable. Messages are saved to `%APPDATA%\com.rajieluo.desktopassistant\history.jsonl`; *Clear…* in that window deletes them.
- **Ctrl + mouse wheel** over her makes the window bigger or smaller.
- **Keys 1 to 7** (after clicking her) play happy, sad, angry, surprised, shy, thinking and proud, so you can check a model's motions.

## Claude Code mode

Instead of plain chat, she can be a voice and face for [Claude Code](https://claude.com/claude-code) working in a folder of yours (a code project, notes, anything).

1. Install Claude Code and run `claude` once in a terminal to sign in. This mode uses that login (your Claude subscription), not an API key.
2. In the tray, pick **Claude Code folder…** and choose the folder. That switches on **Claude Code mode** (toggle it in the tray to go back to normal chat).
3. Talk or type as usual: "what does this project do?", "add a test for the parser", "fix the failing build".

Each message runs `claude -p` in that folder and continues the same Claude Code session until you pick *Forget conversation*. While it works, the bubble shows what it's doing ("Reading main.ts…", "Running npm test…"), and the reply is spoken and drives her emotions like in normal chat.

Permissions: there is no one to click "Allow" in this mode, so it runs with `acceptEdits` by default. Claude Code can read and edit files in the folder, but shell commands outside its safe list are refused. Change `claude_code_permission_mode` in `settings.json` to `auto` (lets Claude Code's classifier allow safe commands) or another mode if you want more.

## Crew mode (firstmate)

Crew mode makes her the face of [firstmate](https://github.com/kunchenguid/firstmate), which runs several Claude Code sessions in parallel, each in its own git worktree. You talk to her; she passes it to firstmate's first mate, reads out its answers, and shows a badge for each task (blue working, amber needs you, green PR ready, red failed). Firstmate runs in WSL2. Setup and details: [docs/crew-setup.md](docs/crew-setup.md).

## Chat settings

Chat settings live in `settings.json` in `%APPDATA%\com.rajieluo.desktopassistant\` (created on first run). Edit it while the app is closed:

```json
{
  "model_path": "C:\\Users\\you\\Documents\\live2d models\\1014100aqua\\1014100.model3.json",
  "character_name": null,
  "chat_model": "claude-opus-5-5",
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
- `chat_model`: any Claude model id. The tray's **Chat model** submenu switches between Opus 5.5, Sonnet 5.5 and Haiku 5.5 without editing the file. The default is Claude Opus 5.5. `claude-haiku-5-5` is much cheaper and faster if replies are good enough for you.
- `chat_effort`: `low` (default, quick chatty replies), `medium`, `high`, `xhigh` or `max`.
- `voice_enabled`, `voice`, `voice_speed`, `voice_pitch`: speech on or off, the Kokoro voice (`af_heart` by default; also `af_bella`, `af_nicole`, `af_sky`, `bf_emma` and [others](https://huggingface.co/hexgrad/Kokoro-82M/blob/main/VOICES.md)), speed (0.5 to 2) and pitch (0.75 to 1.35). Speed and pitch also have sliders at the bottom of the poses panel; double-click a slider to reset it.
- `stt_model`, `stt_language`: the Whisper model for voice input (`tiny.en`, `base.en` by default, `small.en` for better accuracy, or multilingual `small` with `"stt_language": "auto"`) and the spoken language.
- `voice_device`: where the voice runs, `"cpu"` (default) or `"gpu"`. The GPU is faster, but on some cards (an RX 7900 XT, for one) it garbles the speech; when that happens the app notices and switches back to the CPU by itself.
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

Each model can have its own personality. Put a plain text file in either place (the first one found wins):

1. `%APPDATA%\com.rajieluo.desktopassistant\personas\<model folder name>.md`, e.g. `personas\Nicole.md`
2. `persona.md` inside the model's own folder, e.g. `Documents\live2d models\Nicole\persona.md`

`.txt` works too. Describe who the character is, how they talk and how they treat you. You don't need to repeat the reply rules: the app always adds the short-reply and emotion-tag instructions, and a reminder to stay genuinely helpful. In Claude Code mode the persona only shapes her tone and spoken summaries, not the quality of the work. The file is read on every message, so edits apply from the next reply. A `persona.md` in the model folder can also start with front matter that sets the model's default voice; it is stripped before the text reaches Claude:

```
---
voice: af_bella
---
You are Nicole Demara, ...
``` Persona files for characters you don't own stay on your machine; don't commit them here.

Claude starts each reply with an emotion tag such as `[happy]`. The app strips the tags from the text and plays the matching motion. The API call is made from the Rust side, so the key never reaches the web page. Server-side refusal fallbacks are turned on, so if Claude Opus 5.5 declines a message the API retries it on a fallback model in the same call.

## If something goes wrong

The app writes a log to `%LOCALAPPDATA%\com.rajieluo.desktopassistant\logs\Desktop Assistant.log`.
If it closes without a message, Windows also records the crash: open Event Viewer, go to
Windows Logs > Application, and look for an "Application Error" for `desktop-assistant.exe`.
The exception code there (for example `0xc000001d`, an illegal instruction) says what kind of crash it was.

Speech input needs a CPU with AVX2 (most PCs from 2015 on).

## Models

Models are loaded from your own folder and are **never committed** to this repo (`.gitignore` blocks Live2D model files). The app only reads files inside the folder of the model you picked.

Any Cubism 3/4 model (`.model3.json`) works. Many models put all their motions in one unnamed group, so the app sorts them by file name:

- `Wait`, `Idle`, `Stand`, `Breath`… become the idle loop.
- `Happy`/`Smile`, `Sad`/`Cry`/`Upset`, `Anger`, `Surprise`, `Shame`/`Shy`, `Puzzle`/`Doubt`/`Serious`, `Pride` become emotions (including the pinyin names some models use, like `weixiao` or `shengqi`).
- Names containing `Expression`, `face_` or `eye_` are treated as face-only overlays.
- Everything else becomes a generic reaction, used when a model has no motion for an emotion.

The app measures where the character actually is inside the model's canvas and crops the empty space around her (Aqua has a lot below her feet), so she fills the window.

## Developing

- `npm run dev` serves the web part on http://localhost:1420. In a normal browser you can load a model with `?model=/path/to/model.model3.json` (relative to the repo, e.g. a model copied into `tests/fixtures/`, which is git-ignored).
- Add `&tts=beep` to the dev URL to replace Kokoro with beeps, for testing lip sync without downloading the voice model.
- `npm run fixtures` downloads Live2D's free sample model Hiyori into `tests/fixtures/` for testing.
- `npm test` runs the web unit tests, `cargo test` (in `src-tauri/`) the Rust ones.

### How click-through works

Tauri can only make a whole window ignore the mouse. So the Rust side polls the global cursor position about 30 times a second and sends it to the page. The page reads the alpha of the pixel under the cursor; over the character (or any element marked `data-hit`) it turns mouse events on, elsewhere it turns them off.

### Third-party code

- [Live2D Cubism Core](https://www.live2d.com/en/sdk/license/) is downloaded at install time and is subject to the Live2D Proprietary Software License. It is free for individuals and small businesses.
- [pixi-live2d-display](https://github.com/guansss/pixi-live2d-display) (MIT) renders the models.
- [whisper.cpp](https://github.com/ggml-org/whisper.cpp) (MIT) through [whisper-rs](https://github.com/tazz4843/whisper-rs) (Unlicense) does speech to text.
- [kokoro-js](https://www.npmjs.com/package/kokoro-js) (Apache-2.0) and the Kokoro-82M model (Apache-2.0) do text to speech.

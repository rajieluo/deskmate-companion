# Installing Desktop Assistant

Desktop Assistant runs on Windows 10 and 11. What you need depends on which modes you use.
Start with [Requirements](#requirements), then follow the steps for each mode you want.

- [Requirements](#requirements)
- [1. Install the app](#1-install-the-app)
- [2. Plain chat](#2-plain-chat)
- [3. Claude Code mode](#3-claude-code-mode)
- [4. Crew mode (firstmate)](#4-crew-mode-firstmate)
- [5. Voice output and voice input](#5-voice-output-and-voice-input)
- [6. Your own models, voices and personas](#6-your-own-models-voices-and-personas)
- [Building from source](#building-from-source)
- [Troubleshooting](#troubleshooting)

All app files live in two folders:

| Folder | What's in it |
|---|---|
| `%APPDATA%\com.rajieluo.desktopassistant\` | `settings.json`, `personas\`, chat history, downloaded voice and Whisper models |
| `%LOCALAPPDATA%\com.rajieluo.desktopassistant\logs\` | The log, `Desktop Assistant.log` |

## Requirements

### Every mode

| Item | Required or optional | Notes |
|---|---|---|
| Windows 10 or 11, 64-bit | Required | |
| WebView2 | Required | Already part of Windows 10 and 11. |
| The Desktop Assistant app | Required | See [Install the app](#1-install-the-app). |
| A Live2D model (Cubism 3 or 4, `.model3.json`) | Required | The app doesn't include one. See [Your own models](#live2d-models). |
| Internet connection | Required | For the chat itself, and for the one-time voice and Whisper model downloads. |

### Plain chat

| Item | Required or optional | Notes |
|---|---|---|
| Anthropic API key with prepaid credits | Required | Create one at the [Anthropic Console](https://console.anthropic.com/settings/keys) and buy credits there. The free Claude plan on claude.ai doesn't include API access. |

### Claude Code mode

| Item | Required or optional | Notes |
|---|---|---|
| [Claude Code](https://claude.com/claude-code), on `PATH` | Required | The app runs `claude.exe` (native installer) or `claude.cmd` (npm install). |
| Claude Code signed in | Required | With a paid Claude plan, or with an API key. This mode uses Claude Code's own login, not the key you gave the app. |
| A folder for Claude Code to work in | Required | A code project, notes, anything. |

### Crew mode (firstmate)

Crew mode makes her the face of [firstmate](https://github.com/kunchenguid/firstmate), which runs several Claude Code sessions at once. It needs everything Claude Code mode needs, plus:

| Item | Required or optional | Notes |
|---|---|---|
| Git for Windows (Git Bash) | Required | The app looks for `bash.exe` under `Program Files\Git\bin`, `%LOCALAPPDATA%\Programs\Git\bin`, or next to `git.exe` on `PATH`. |
| [firstmate-for-windows](https://github.com/EvanBatten/firstmate-for-windows) | Required | The Windows port of firstmate, cloned into a folder. |
| Node.js and npm | Required | |
| [Herdr](https://herdr.dev) | Required | The terminal the crew's worker windows run in. |
| GitHub CLI (`gh`), logged in | Required | `gh auth login`. Crewmates open pull requests with it. |
| jq, PowerShell 7, Treehouse, Python 3, Windows Developer Mode | Required | Listed in firstmate-for-windows' [install guide](https://github.com/EvanBatten/firstmate-for-windows/blob/main/platform/windows/INSTALL.md). Python 3 is for firstmate's inbox. |
| WSL2 with Ubuntu | Optional | Instead of Git Bash, run upstream firstmate inside WSL2. See [docs/crew-setup.md](docs/crew-setup.md). |

### Voice

| Item | Required or optional | Notes |
|---|---|---|
| Kokoro voice (speech output) | Optional, on by default | Runs locally and free. About 90 MB, downloaded the first time she speaks. |
| [Kyutai Pocket TTS](https://github.com/kyutai-labs/pocket-tts) (cloned voices) | Optional | Needs Python with `pip install pocket-tts`, and a Hugging Face account to clone voices. See [Pocket TTS](#pocket-tts-cloned-voices). |
| CPU with AVX2 (most PCs from 2015 on) | Required for voice input | whisper.cpp is built for AVX2, FMA and F16C. Without them the app refuses voice input with a message; everything else still works. |
| Microphone | Required for voice input | Windows may ask for permission once. |
| Whisper model for voice input | Required for voice input | `base.en` (about 140 MB) downloads the first time you use the mic. |
| A DirectX 12 graphics card | Optional, GPU voice | Only if you set `"voice_device": "gpu"`. The CPU is the default and works everywhere. |

## 1. Install the app

There are no published releases yet. You can:

- **Use an installer build.** The *Windows installer* workflow builds an NSIS installer (`Desktop Assistant_<version>_x64-setup.exe`). It runs when started by hand from the repository's **Actions** tab or when a `v*` tag is pushed; download the installer from the run's **Artifacts** section. Artifacts are kept for 14 days. Run the installer and start **Desktop Assistant**.
- **Build it yourself.** See [Building from source](#building-from-source).

The installer contains the app, the ONNX Runtime and DirectML libraries for the voice, and the Pocket TTS sidecar script. It doesn't contain any Live2D model, voice model or Python.

On first launch the app loads `Documents\live2d models\1014100aqua\1014100.model3.json` if it exists; otherwise it asks you to pick a `.model3.json` file. See [Live2D models](#live2d-models).

## 2. Plain chat

1. Get an API key: sign in at the [Anthropic Console](https://console.anthropic.com/settings/keys), add prepaid credits under billing, and create a key.
2. Click her (or press **Ctrl+Shift+Space**) and type a message. The first time, she asks for the key.
3. Paste the key. It's stored in Windows Credential Manager, never in a file. To change it later, use **Set API key…** in the tray. An `ANTHROPIC_API_KEY` environment variable also works and takes priority.

The tray's **Chat model** submenu picks Opus 5.5 (default), Sonnet 5.5 or Haiku 5.5. Haiku is the cheapest.

## 3. Claude Code mode

1. Install [Claude Code](https://docs.claude.com/en/docs/claude-code/setup) so `claude` works in a new terminal.
2. Run `claude` once in a terminal and sign in (a paid Claude plan, or an API key), then type `/exit`.
3. In the tray, choose **Claude Code folder…** and pick the folder. That turns on **Claude Code mode**; untick it in the tray to go back to plain chat.
4. Talk or type as usual.

Each message runs `claude -p` in that folder with permission mode `acceptEdits`: Claude Code can read and edit files there, but shell commands outside its safe list are refused. To allow more, set `claude_code_permission_mode` in `settings.json` (for example `auto`). Details are in the README's [Claude Code mode](README.md#claude-code-mode) section.

## 4. Crew mode (firstmate)

Do [Claude Code mode](#3-claude-code-mode) steps 1 and 2 first. Then, for the default native (Git Bash) setup:

1. Install the tools from firstmate-for-windows' [install guide](https://github.com/EvanBatten/firstmate-for-windows/blob/main/platform/windows/INSTALL.md): Developer Mode, Git for Windows, PowerShell 7, gh, Node.js, jq, Claude Code, Herdr and Treehouse. Also install Python 3 (`winget install --id Python.Python.3.12 -e`).
2. Log in to GitHub: `gh auth login`.
3. Clone firstmate-for-windows with symlinks on:
   ```powershell
   git clone -c core.symlinks=true https://github.com/EvanBatten/firstmate-for-windows "$env:USERPROFILE\Documents\projects\firstmate"
   ```
4. With the app closed, set `"crew_home": "~/Documents/projects/firstmate"` in `settings.json` (`~` is your Windows user folder in Git Bash). The default is `~/firstmate`.
5. Recommended: in Git Bash, `echo auto > ~/Documents/projects/firstmate/config/claude-permission-mode`, so crewmates use Claude Code's `auto` permission mode instead of skipping permission checks.
6. Do the guide's one-time [launch steps](https://github.com/EvanBatten/firstmate-for-windows/blob/main/platform/windows/INSTALL.md#launch).
7. Tick **Crew mode (firstmate)** in the tray. The text box gets a blue border, and the first mate starts in the background (its startup takes a minute or so). **Open crew terminal** opens Herdr so you can watch the crew; **Crew sessions…** lists every crewmate.

The WSL2 route, all crew settings, and crew troubleshooting are in [docs/crew-setup.md](docs/crew-setup.md).

## 5. Voice output and voice input

### Kokoro (default voice)

Nothing to install. The first time she speaks, the Kokoro-82M model (about 90 MB) downloads from Hugging Face into `%APPDATA%\com.rajieluo.desktopassistant\voice\`. Mute her with the speaker button, **Ctrl+Alt+U**, or **Speak replies** in the tray.

To use the GPU, set `"voice_device": "gpu"` in `settings.json`. The app then runs Kokoro with DirectML. See [Troubleshooting](#troubleshooting) if the speech comes out garbled.

### Pocket TTS (cloned voices)

Pocket TTS speaks in a voice cloned from a short recording, and starts talking about 0.1 s after each sentence. It runs locally, in a Python process the app starts when it's first needed.

1. Install Python 3 (a version `pocket-tts` supports; see its [project page](https://github.com/kyutai-labs/pocket-tts)).
2. Make a virtual environment where the app looks by default, and install Pocket TTS into it:
   ```powershell
   $venv = "$env:APPDATA\com.rajieluo.desktopassistant\pocket\venv"
   python -m venv $venv
   & "$venv\Scripts\python.exe" -m pip install pocket-tts
   ```
   A venv somewhere else works too: point `pocket_python` in `settings.json` at its `python.exe`.
3. For cloned voices, accept the terms at https://huggingface.co/kyutai/pocket-tts with your Hugging Face account, then log in once from the venv: `& "$venv\Scripts\hf.exe" auth login` (if `hf.exe` isn't there, `pip install huggingface_hub` in the venv first).
4. Give the model a voice (see [Choosing a voice](#choosing-a-voice)), then pick **Voice > Engine: Pocket TTS (cloned voice)** in the tray.

Pocket TTS is only used for models that have a Pocket voice. Other models keep using Kokoro, and so does every sentence after Pocket TTS fails, so she never goes quiet. `pocket_quantize` (default `true`) runs the model in int8, about 20% faster on the CPU. Speed and pitch apply to Kokoro only.

### Voice input (whisper.cpp)

1. Press the microphone button in the chat box, or **Ctrl+Alt+M** anywhere.
2. The first time, the Whisper model (`base.en`, about 140 MB) downloads into `%APPDATA%\com.rajieluo.desktopassistant\whisper\`, and Windows may ask for microphone permission.
3. Speak; she stops listening after a short pause and sends what you said.

`stt_model` picks a different Whisper model (`tiny.en`, `small.en`, or multilingual `small` with `"stt_language": "auto"`). Voice input needs a CPU with AVX2.

## 6. Your own models, voices and personas

### Live2D models

Any Cubism 3 or 4 model works: a folder with a `.model3.json` file and its textures, motions and other files.

1. Put the model folder anywhere, for example `Documents\live2d models\<name>\`.
2. In the tray (or by right-clicking her), choose **Choose model…** and pick the `.model3.json` file. The choice is saved as `model_path` in `settings.json`.

The app only reads files inside the picked model's folder. The folder's name matters: it's her default name (`1014100aqua` becomes Aqua, `mori_suit` becomes Mori Suit; `character_name` overrides it), and per-model settings such as `model_params`, `look_params` and `model_voices` are keyed by it. How motions are matched to emotions is described in the README's [Models](README.md#models) section.

Bundled characters, such as the ones that will ship with the installer, work the same way: each is a model folder that can carry its own `persona.md` and voice. Nicole and Jane are planned as bundled options; the current installer ships no models.

### Choosing a voice

**Kokoro voices.** The tray's **Voice** submenu lists every Kokoro English voice (American and British, female and male). A pick there is saved for the current model only. For a model, the app uses the first of these that is set:

1. The voice picked in the tray for that model (`model_voices` in `settings.json`). **Model's own voice** in the submenu clears it.
2. The first line of `voice.txt` in the model folder, e.g. `af_nicole`.
3. A `voice:` line in the front matter of `persona.md` in the model folder (below).
4. `voice` in `settings.json`.
5. `af_heart`.

Voice ids are the ones in the tray, such as `af_heart`, `af_bella`, `af_nicole`, `bf_emma` or `am_michael` (the full list is in `src-tauri/src/voices.rs`). An unknown id is ignored.

**Pocket TTS cloned voices.** A model has a Pocket voice when its folder has, in this order:

1. `pocket_voice:` in the front matter of `persona.md`: a file in the model folder (`.safetensors` or `.wav`), or a Pocket TTS preset name such as `alba`. If the named file is missing, the next options are tried.
2. `voice.safetensors`, made with `pocket-tts export-voice clip.wav voice.safetensors`.
3. `voice.wav`: 10 to 30 seconds of clean speech. It's encoded once and cached in `%APPDATA%\com.rajieluo.desktopassistant\voice\pocket\`.

Only use recordings of voices you have permission to clone.

### Custom personas

A persona file describes who the character is, how they talk and how they treat you. The app looks for one in this order and uses the first that exists and isn't empty:

1. `%APPDATA%\com.rajieluo.desktopassistant\personas\<model folder name>.md`
2. `%APPDATA%\com.rajieluo.desktopassistant\personas\<model folder name>.txt`
3. `persona.md` in the model's folder
4. `persona.txt` in the model's folder

The file is plain text (Markdown is fine). You don't need to repeat the reply rules: the app always adds the short-reply and emotion-tag instructions and a reminder to stay helpful. The file is read on every message, so edits apply from the next reply. Without a persona file, `persona` in `settings.json` is used, and without that, the built-in personality.

`persona.md` in the model folder can start with front matter. `voice:` sets the model's Kokoro voice and `pocket_voice:` its Pocket TTS voice; the block is stripped before the text reaches Claude:

```
---
voice: af_bella
pocket_voice: voice.wav
---
You are Nicole, who runs a tiny, broke agency. You talk fast and tease the user...
```

Front matter is only read for voices from `persona.md` in the model folder, not from files in `personas\` or from `persona.txt`.

When a model loads, the app also writes its persona text to `%APPDATA%\com.rajieluo.desktopassistant\current-persona.md` for other tools such as firstmate. Don't edit that file; it's overwritten.

## Building from source

You need [Node.js 22+](https://nodejs.org/), [Rust](https://rustup.rs/), the [Tauri prerequisites for Windows](https://v2.tauri.app/start/prerequisites/#windows) (WebView2 is already on Windows 10/11), and, for building whisper.cpp, [CMake](https://cmake.org/download/) and [LLVM](https://github.com/llvm/llvm-project/releases).

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
5. **Node.js 22+**.

Then:

```powershell
npm install          # also downloads the Live2D Cubism Core runtime
npm run tauri dev    # run the app with hot reload
npm run tauri build  # build an installer in src-tauri/target/release/bundle/nsis
```

The first Rust build takes about a minute on a recent PC, seconds after that. Close the installed Desktop Assistant before `npm run tauri dev`, or the two fight over the global hotkeys.

Debugging tips:

- Set `$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9222"` before `npm run tauri dev` to attach Edge DevTools (or a script) to the app's pages at `http://127.0.0.1:9222`.
- If Vite is already running on port 1420, `cargo run` in `src-tauri/` starts just the app against it.

## Troubleshooting

- **Where's the log?** `%LOCALAPPDATA%\com.rajieluo.desktopassistant\logs\Desktop Assistant.log`.
- **It closes without a message.** Windows records the crash: open Event Viewer, go to *Windows Logs > Application*, and look for an "Application Error" for `desktop-assistant.exe`. The exception code says what kind of crash it was (for example `0xc000001d`, an illegal instruction).
- **"Speech input needs a CPU with AVX2"**: this PC's CPU can't run whisper.cpp. Typing and spoken replies still work.
- **The GPU voice sounds garbled.** On some graphics cards (an RX 7900 XT, for one) the GPU garbles Kokoro's speech. In the app the GPU voice runs on DirectML, and the app doesn't detect garbling there: set `"voice_device": "cpu"` in `settings.json` with the app closed. (Only the slower built-in voice, used when the app's native voice won't load, notices garbled WebGPU output and switches to the CPU by itself.)
- **Pocket TTS is on but she still sounds like Kokoro.** The model has no Pocket voice: add `voice.safetensors`, `voice.wav` or `pocket_voice:` to its folder ([Choosing a voice](#choosing-a-voice)). The log's "voice engine" line says which voice is used.
- **"Python for Pocket TTS not found at …"**: create the venv in [Pocket TTS](#pocket-tts-cloned-voices) step 2, or set `pocket_python` to a `python.exe` that has `pocket-tts` installed.
- **A hotkey doesn't work.** Another app already uses it; the log says so and the tray shows no hotkey for it. Change it with `hotkeys` in `settings.json`.
- **Claude Code mode can't find Claude Code.** The app looks for `claude.exe` or `claude.cmd` on `PATH`. Open a new terminal and check that `claude` runs there.
- **Crew mode problems**: see [docs/crew-setup.md](docs/crew-setup.md#troubleshooting).

# Installing Desktop Assistant

Desktop Assistant puts an animated Live2D or MMD character on your Windows 10 or 11 desktop. You chat with her by typing or out loud; she answers with Google Gemini (free) or Claude (paid) and speaks her replies with a voice that runs on your PC.

Got the installer from a friend? Sections 1 to 5 are all you need, about 15 minutes. Sections 6 to 8 are optional extras.

- [Requirements](#requirements)
- [1. Install the app](#1-install-the-app)
- [2. Get a model and pick it](#2-get-a-model-and-pick-it)
- [3. Chat: Gemini (free) or Claude](#3-chat-gemini-free-or-claude)
- [4. Talk to her: voice input and output](#4-talk-to-her-voice-input-and-output)
- [5. Personas and voices for each model](#5-personas-and-voices-for-each-model)
- [6. Optional: cloned voices with Pocket TTS](#6-optional-cloned-voices-with-pocket-tts)
- [7. Optional: Claude Code mode](#7-optional-claude-code-mode)
- [8. Optional: Crew mode (firstmate)](#8-optional-crew-mode-firstmate)
- [Where everything lives](#where-everything-lives)
- [Sharing the app with a friend](#sharing-the-app-with-a-friend)
- [Building from source](#building-from-source)
- [Troubleshooting](#troubleshooting)

## Requirements

### Every mode

| Item | Required or optional | Notes |
|---|---|---|
| Windows 10 or 11, 64-bit | Required | WebView2, which the app runs on, is already part of Windows 10 and 11. |
| The Desktop Assistant installer | Required | See [Install the app](#1-install-the-app). |
| A Live2D model (Cubism 3 or 4, `.model3.json`) or an MMD model (`.pmx`, `.pmd`) | Required | The app doesn't include one. See [Get a model](#2-get-a-model-and-pick-it). |
| A Google Gemini API key (free) or an Anthropic API key with prepaid credits | Required for plain chat | See [Chat](#3-chat-gemini-free-or-claude). |
| Internet connection | Required | For the chat itself, and for the one-time voice and Whisper model downloads. |

### Voice

| Item | Required or optional | Notes |
|---|---|---|
| Kokoro voice (speech output) | On by default | Runs locally and free. About 90 MB, downloaded in the background the first time the app starts. |
| CPU with AVX2 (most PCs from 2015 on) | Required for voice input | whisper.cpp is built for AVX2, FMA and F16C. Without them the app refuses voice input with a message; everything else still works. |
| Microphone | Required for voice input | Windows may ask for permission once. |
| [Kyutai Pocket TTS](https://github.com/kyutai-labs/pocket-tts) (cloned voices) | Optional | **Voice > Set up Pocket TTS…** in the menu installs it, Python included (about 1 GB on disk, a bit more to download). Cloning from a recording also needs your own Hugging Face account. See [Pocket TTS](#6-optional-cloned-voices-with-pocket-tts). |
| A DirectX 12 graphics card | Optional, GPU voice | Only if you set `"voice_device": "gpu"`. The CPU is the default and works everywhere. |

### Claude Code mode and Crew mode

| Item | Required or optional | Notes |
|---|---|---|
| [Claude Code](https://claude.com/claude-code), signed in | Required for both | The app runs `claude.exe` (native installer) or `claude.cmd` (npm install), from `PATH`, `%USERPROFILE%\.local\bin`, `%USERPROFILE%\.claude\local` or `%APPDATA%\npm`. It uses Claude Code's own login (a paid Claude plan or an API key), not the key you gave the app. |
| A folder for Claude Code to work in | Required for Claude Code mode | A code project, notes, anything. |
| Git for Windows (Git Bash) | Required for Crew mode | The app looks for `bash.exe` under `Program Files\Git\bin`, `%LOCALAPPDATA%\Programs\Git\bin`, or next to `git.exe` on `PATH`. |
| [firstmate-for-windows](https://github.com/EvanBatten/firstmate-for-windows) | Required for Crew mode | The Windows port of [firstmate](https://github.com/kunchenguid/firstmate), cloned into a folder. |
| Node.js and npm, [Herdr](https://herdr.dev), GitHub CLI (`gh`) logged in, jq, PowerShell 7, Treehouse, Python 3, Windows Developer Mode | Required for Crew mode | Listed in firstmate-for-windows' [install guide](https://github.com/EvanBatten/firstmate-for-windows/blob/main/platform/windows/INSTALL.md). Herdr is the terminal the crew's worker windows run in; crewmates open pull requests with `gh`; Python 3 is for firstmate's inbox. |
| WSL2 with Ubuntu | Optional | Instead of Git Bash, run upstream firstmate inside WSL2. See [docs/crew-setup.md](docs/crew-setup.md). |

## 1. Install the app

1. Get the installer, `Desktop Assistant_<version>_x64-setup.exe`. There are no published releases yet, so either someone who built it gives it to you, you download it from a run of the *Windows installer* workflow (repository **Actions** tab, the run's **Artifacts** section, kept for 14 days), or you [build it yourself](#building-from-source).
2. Run it. Windows may show **"Windows protected your PC"**, because the installer isn't code-signed. Click **More info**, then **Run anyway**.
3. Start **Desktop Assistant** from the Start menu.

The app has no window of its own: look for its icon in the tray at the bottom right of the taskbar (click the **^** arrow if it's hidden). Every option is in that icon's menu, and right-clicking her opens the same menu.

The installer contains the app, the ONNX Runtime and DirectML libraries for the voice, and the Pocket TTS sidecar and setup scripts. It doesn't contain any model, persona, voice, Python, setting or API key: those all belong to whoever uses it.

## 2. Get a model and pick it

The app doesn't come with a character: you bring your own. It works with two kinds of model:

| Kind | What it looks like | Where to find one |
|---|---|---|
| **Live2D** (2D, anime style) | A folder with a `.model3.json`, a `.moc3` and textures | Live2D's free [sample models](https://www.live2d.com/en/learn/sample/) (Hiyori, Mao, Natori and others), or models you bought or made. Cubism 3 or 4 only; old Cubism 2 models (`.model.json`) don't work. |
| **MMD** (3D) | A folder with a `.pmx` (or `.pmd`) and textures | Many free models are shared by their creators online. |

1. Put each model in its own folder, anywhere, for example `Documents\avatars\Hiyori\`.
2. The first time the app starts, it asks you to pick a model file: choose the `.model3.json` or `.pmx` inside the folder. To switch later, use **Choose model…** in the menu. The choice is saved as `model_path` in `settings.json`.
3. Drag her to move her, and hold **Ctrl** and use the mouse wheel over her to resize her. **Reset position** brings her back if she's lost off-screen.

The folder's name matters: it's her default name (`Hiyori` becomes Hiyori, `mori_suit` becomes Mori Suit; `character_name` in `settings.json` overrides it), and per-model settings such as `model_params`, `look_params` and `model_voices` are keyed by it. The app only reads files inside the picked model's folder. How Live2D motions are matched to emotions is in the README's [Models](README.md#models) section.

**MMD models** stand in a T-pose and don't come with idle animations. Put `.vmd` motion files anywhere in the model's folder: one named like an idle (`idle.vmd`, `wait.vmd`, `loop.vmd`…) loops while she stands, and emotion names (`happy.vmd`, `sad.vmd`…) play when she feels that way. Without any, the app lowers her arms and adds gentle breathing and sway. See the README's [MMD models](README.md#mmd-models-pmx--pmd) section.

**Dancing**: ask an MMD character to dance in chat ("dance", "can you dance?") and she dances, then goes back to her idle. Dances go in `Documents\Desktop Assistant\Dances` (tray > **Open dances folder**): drop in a `.vmd` dance motion, with a same-named `.mp3`/`.wav`/`.ogg` for music, or a folder holding a dance pack. With no dances she does a short built-in one. See the README's [Dancing](README.md#dancing-mmd-models) section.

Read each model's license or readme. Many Live2D and MMD models allow personal use only and forbid passing them on.

## 3. Chat: Gemini (free) or Claude

She needs an AI service to answer. You use your own key; it's stored in Windows Credential Manager (under `desktop-assistant`), never in a file.

**Google Gemini (free):**

1. Go to [Google AI Studio](https://aistudio.google.com/apikey), sign in with a Google account, click **Create API key**, and copy it (it usually starts with `AIza`). No credit card is needed.
2. In the menu, open **Chat model** and pick **Google Gemini (free tier)**.
3. Click her (or press **Ctrl+Shift+Space**) and send any message. She asks for a key: paste it and press Enter.

More detail, such as the `GEMINI_API_KEY` environment variable and `gemini_model`, is in the README's [Gemini section](README.md#free-voice-chat-with-google-gemini).

**Claude (paid):**

1. Sign in at the [Anthropic Console](https://console.anthropic.com/settings/keys), add prepaid credits under Billing, and create an API key. A claude.ai subscription, free or paid, doesn't include API access.
2. In the menu, open **Chat model** and pick **Claude Opus 5.5 (smartest)**, the default, **Claude Sonnet 5.5**, or **Claude Haiku 5.5 (fastest, cheapest)**.
3. Click her and send a message. She asks for the key: paste it and press Enter. An `ANTHROPIC_API_KEY` environment variable also works and takes priority.

The Anthropic and Gemini keys are kept separately, so you can switch between them without typing either again. **Set API key…** changes the key of whichever provider is picked under **Chat model**.

Gemini only answers in plain chat: keep **Claude Code mode** and **Crew mode (firstmate)** unticked. Those two modes always use Claude Code (sections 7 and 8).

## 4. Talk to her: voice input and output

- **Type**: click her, or press **Ctrl+Shift+Space** anywhere. Enter sends, Esc closes the box.
- **Speak**: press the microphone button in the chat box, or **Ctrl+Alt+M** anywhere. Talk, and after a short pause she sends what you said. The first time, the Whisper model (`base.en`, about 140 MB) downloads into `%APPDATA%\com.rajieluo.desktopassistant\whisper\`, and Windows may ask for microphone permission. Speech is transcribed locally with whisper.cpp. `stt_model` in `settings.json` picks another Whisper model (`tiny.en`, `small.en`, or multilingual `small` with `"stt_language": "auto"`).
- **Her voice**: she speaks her replies with Kokoro, locally and free. The first time the app starts, it downloads the Kokoro-82M model (about 90 MB) into `%APPDATA%\com.rajieluo.desktopassistant\voice\`, so her first reply may wait for it. **Voice** in the menu picks another voice for the current model. Mute her with the speaker button, **Ctrl+Alt+U**, or **Speak replies** in the menu.
- **Start over** with **Forget conversation**. **Chat history…** shows every message, searchable.

To run Kokoro on the GPU, set `"voice_device": "gpu"` in `settings.json` (DirectML). See [Troubleshooting](#troubleshooting) if the speech comes out garbled.

## 5. Personas and voices for each model

Each model folder can carry its own personality and voice, so a character is one folder you can move to another PC (if its license allows).

### Personas

Make `persona.md` in the model's folder, next to the `.model3.json` or `.pmx`, and describe who she is, how she talks and how she treats you:

```
---
voice: af_bella
pocket_voice: voice.wav
---
You are Hiyori, a cheerful student who loves astronomy. You talk casually,
tease the user a little, and get excited about stars and space.
```

- The front matter between the `---` lines is optional. `voice:` sets her Kokoro voice and `pocket_voice:` her [Pocket TTS](#6-optional-cloned-voices-with-pocket-tts) voice. It's stripped before the text reaches the AI.
- You don't need to explain the reply rules: the app adds the short-reply and emotion-tag instructions and a reminder to stay helpful to every persona file.
- The file is read on every message, so edits apply from the next reply.

The app uses the first of these that exists and isn't empty:

1. `%APPDATA%\com.rajieluo.desktopassistant\personas\<model folder name>.md`
2. `%APPDATA%\com.rajieluo.desktopassistant\personas\<model folder name>.txt`
3. `persona.md` in the model's folder
4. `persona.txt` in the model's folder

Without any, `persona` in `settings.json` is used, and without that, the built-in personality. Unlike a persona file, `persona` in `settings.json` replaces the whole prompt, so it must include the reply rules and emotion tags itself. Front matter is only read from `persona.md` in the model folder, not from files in `personas\` or from `persona.txt`.

When a model loads, the app also writes its persona text to `%APPDATA%\com.rajieluo.desktopassistant\current-persona.md` for other tools such as firstmate. Don't edit that file; it's overwritten.

### Kokoro voices

The menu's **Voice** submenu lists every Kokoro English voice (American and British, female and male). A pick there is saved for the current model only. For a model, the app uses the first of these that is set:

1. The voice picked in the menu for that model (`model_voices` in `settings.json`). **Model's own voice** in the submenu clears it.
2. The first line of `voice.txt` in the model folder, e.g. `af_nicole`.
3. `voice:` in the front matter of `persona.md` in the model folder.
4. `voice` in `settings.json`.
5. `af_heart`.

Voice ids are the ones in the menu, such as `af_heart`, `af_bella`, `af_nicole`, `bf_emma` or `am_michael` (the full list is in `src-tauri/src/voices.rs`). An unknown id in the model folder is ignored.

### Pocket TTS voices

A model has a Pocket voice when its folder has, in this order:

1. `pocket_voice:` in the front matter of `persona.md`: a file in the model folder (`.safetensors` or `.wav`), or a Pocket TTS preset name such as `alba`. If the named file is missing, the next options are tried.
2. `voice.safetensors`, made with `pocket-tts export-voice clip.wav voice.safetensors`.
3. `voice.wav`: 10 to 30 seconds of one person speaking clearly, with no music or background noise. It's encoded once and cached in `%APPDATA%\com.rajieluo.desktopassistant\voice\pocket\`.

Only use recordings of voices you have permission to clone, such as your own.

## 6. Optional: cloned voices with Pocket TTS

[Kyutai Pocket TTS](https://github.com/kyutai-labs/pocket-tts) speaks in a voice cloned from a short recording, and starts talking about 0.1 s after each sentence. It runs locally, in a Python process the app starts when it's first needed.

### Automatic setup

1. In the menu, choose **Voice > Set up Pocket TTS…**. (Picking **Engine: Pocket TTS** before it's installed opens the same setup.) A PowerShell window opens. It needs no admin rights and no Python of your own:
   1. it downloads [uv](https://github.com/astral-sh/uv) into `%APPDATA%\com.rajieluo.desktopassistant\pocket\uv\`,
   2. uses it to get a private Python 3.12 (`pocket\python\`, not added to `PATH`) and make the venv `pocket\venv\`,
   3. installs `pocket-tts` and PyTorch (CPU) into it, about 1 GB in all, then clears uv's download cache (`pocket\uv-cache\`).
2. At the end it offers to log in to Hugging Face. That's needed to clone voices from `.wav` recordings, because that model is gated:
   1. Sign in at [huggingface.co](https://huggingface.co/join) (a free account) and accept the terms on [kyutai/pocket-tts](https://huggingface.co/kyutai/pocket-tts).
   2. Under [Settings > Access Tokens](https://huggingface.co/settings/tokens), create a token with **Read** access.
   3. Paste it into the window when asked. The paste is invisible; that's normal. Answer **n** if it asks about a git credential.

   Use your own account and token, not someone else's.
3. Close the window when it says **Done**, give a model a [Pocket voice](#pocket-tts-voices), and pick **Voice > Engine: Pocket TTS (cloned voice)**.

Running the setup again skips the finished steps, so it's also how to retry a failed install or log in later. To remove Pocket TTS, delete the `pocket` folder. The script is `setup-pocket.ps1` in the app's install folder; it takes `-DataDir <folder>` and `-NoLogin`.

Without the Hugging Face login, Pocket TTS still runs with its preset voices (`pocket_voice: alba` and the others on its project page) and with `.safetensors` voices exported elsewhere. Cloning from a `.wav` needs the login.

### Manual setup

With a Python 3.10 to 3.14 you already have:

```powershell
$venv = "$env:APPDATA\com.rajieluo.desktopassistant\pocket\venv"
python -m venv $venv
& "$venv\Scripts\python.exe" -m pip install pocket-tts huggingface_hub
& "$venv\Scripts\hf.exe" auth login   # for cloning; accept the model's terms first
```

A venv somewhere else works too: point `pocket_python` in `settings.json` at its `python.exe`. The app tries `pocket_python`, then the Python named in `pocket\python.txt` (the setup writes it, and the app remembers the last one that worked there), then `pocket\venv\Scripts\python.exe`.

### How it behaves

Pocket TTS is only used for models that have a Pocket voice. Other models keep using Kokoro, and so does every sentence after Pocket TTS fails, so she never goes quiet. `pocket_quantize` (default `true`) runs the model in int8, about 20% faster on the CPU. `voice_speed` and `voice_pitch` in `settings.json` apply to Kokoro only. **Voice > Engine: Kokoro** switches back.

## 7. Optional: Claude Code mode

She becomes a voice for [Claude Code](https://claude.com/claude-code) working in a folder of yours.

1. Install [Claude Code](https://docs.claude.com/en/docs/claude-code/setup) so `claude` works in a new terminal.
2. Run `claude` once in a terminal and sign in (a paid Claude plan, or an API key), then type `/exit`.
3. In the menu, choose **Claude Code folder…** and pick the folder. That turns on **Claude Code mode**; untick it to go back to plain chat.
4. Talk or type as usual.

Each message runs `claude -p` in that folder with permission mode `acceptEdits`: Claude Code can read and edit files there, but shell commands outside its safe list are refused. To allow more, set `claude_code_permission_mode` in `settings.json` (for example `auto`). Details are in the README's [Claude Code mode](README.md#claude-code-mode) section.

## 8. Optional: Crew mode (firstmate)

Crew mode makes her the face of [firstmate](https://github.com/kunchenguid/firstmate), which runs several Claude Code sessions at once. Do [Claude Code mode](#7-optional-claude-code-mode) steps 1 and 2 first. Then, for the native (Git Bash) setup, which the app uses whenever it finds Git Bash (without it, and without `crew_shell` set, it falls back to WSL):

1. Install the tools from firstmate-for-windows' [install guide](https://github.com/EvanBatten/firstmate-for-windows/blob/main/platform/windows/INSTALL.md): Developer Mode, Git for Windows, PowerShell 7, gh, Node.js, jq, Claude Code, Herdr and Treehouse. Also install Python 3 (`winget install --id Python.Python.3.12 -e`).
2. Log in to GitHub: `gh auth login`.
3. Clone firstmate-for-windows with symlinks on:
   ```powershell
   git clone -c core.symlinks=true https://github.com/EvanBatten/firstmate-for-windows "$env:USERPROFILE\Documents\projects\firstmate"
   ```
4. With the app closed, set `"crew_home": "~/Documents/projects/firstmate"` in `settings.json` (`~` is your Windows user folder in Git Bash). The default is `~/firstmate`.
5. Recommended: in Git Bash, `echo auto > ~/Documents/projects/firstmate/config/claude-permission-mode`, so crewmates use Claude Code's `auto` permission mode instead of skipping permission checks.
6. Do the guide's one-time [launch steps](https://github.com/EvanBatten/firstmate-for-windows/blob/main/platform/windows/INSTALL.md#launch).
7. Tick **Crew mode (firstmate)** in the menu. The text box gets a blue border, and the first mate starts in the background (its startup takes a minute or so). **Open crew terminal** opens Herdr so you can watch the crew; **Crew sessions…** lists every crewmate.

The WSL2 route, all crew settings, and crew troubleshooting are in [docs/crew-setup.md](docs/crew-setup.md).

## Where everything lives

| What | Where |
|---|---|
| Models, `persona.md`, `voice.txt`, `voice.wav`, `voice.safetensors`, `.vmd` motions | The model folders you made |
| Dances (`.vmd` and music) | `Documents\Desktop Assistant\Dances` |
| `settings.json`, `personas\`, chat history (`history.jsonl`), downloaded voice and Whisper models, Pocket TTS (`pocket\`) | `%APPDATA%\com.rajieluo.desktopassistant\` (paste it into File Explorer's address bar) |
| API keys | Windows Credential Manager, under `desktop-assistant` |
| The log, `Desktop Assistant.log` | `%LOCALAPPDATA%\com.rajieluo.desktopassistant\logs\` |

Edit `settings.json` only while the app is closed; it's rewritten when the app saves. To uninstall, use **Settings > Apps** in Windows, then delete the `%APPDATA%` and `%LOCALAPPDATA%` folders above (`com.rajieluo.desktopassistant` in each) if you also want your settings, history, downloads, browser cache and log gone. The Hugging Face login is in `%USERPROFILE%\.cache\huggingface\` and is shared with any other Hugging Face tools you use.

## Sharing the app with a friend

Give them the installer and a link to this file. Nothing else: they bring their own model, key and voice.

1. Build the installer ([Building from source](#building-from-source)): `npm install`, then `npm run tauri build`. It lands in `src-tauri\target\release\bundle\nsis\Desktop Assistant_<version>_x64-setup.exe`. (The *Windows installer* workflow builds the same file but uses Actions minutes.)
2. Send them that one `.exe`. Your models, personas, voices, `settings.json`, chat history and API keys stay on your PC: they live in your model folders, `%APPDATA%\com.rajieluo.desktopassistant\` and Credential Manager, none of which go into the installer.
3. They follow sections 1 to 5, with their own Gemini or Anthropic key, and for Pocket TTS cloning their own Hugging Face account.

Before passing on a model or a voice too:

- **Models**: check the license or readme. Many forbid redistribution. Live2D's [sample models](https://www.live2d.com/en/learn/sample/) are a safe one to point a friend to.
- **Cloned voices** (`voice.wav`, `voice.safetensors`): only share voices you have permission to clone. A clone of a real person's voice, such as a game voice actor's, isn't yours to give away.
- **Personas** are your own writing, so they're fine to share once anything personal is removed.

To see what a fresh install looks like without losing your own setup, start the app with `DESKTOP_ASSISTANT_DATA_DIR` pointing at an empty folder (see the debugging tips below).

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

The first Rust build takes several minutes (it compiles whisper.cpp and Tauri), seconds after that. Quit the installed Desktop Assistant before `npm run tauri dev` or before starting a built exe: only one copy runs at a time, and a second launch just brings the running one forward.

Debugging tips:

- Set `$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9222"` before `npm run tauri dev` to attach Edge DevTools (or a script) to the app's pages at `http://127.0.0.1:9222`.
- If Vite is already running on port 1420, `cargo run` in `src-tauri/` starts just the app against it.
- **Try a fresh install without losing your own setup**: quit the app, set `$env:DESKTOP_ASSISTANT_DATA_DIR = "$env:TEMP\da-fresh"` and start `desktop-assistant.exe` from that PowerShell. The app then keeps everything it would put in `%APPDATA%\com.rajieluo.desktopassistant` there instead: settings, personas, history, Kokoro and Whisper downloads, Pocket TTS, `current-persona.md`, Claude Code and crew files. Still shared with your normal setup: API keys (Credential Manager), the window position, the log and browser cache in `%LOCALAPPDATA%\com.rajieluo.desktopassistant`, and the Hugging Face login (`%USERPROFILE%\.cache\huggingface`).

## Troubleshooting

- **Where's the log?** `%LOCALAPPDATA%\com.rajieluo.desktopassistant\logs\Desktop Assistant.log`.
- **It closes without a message.** Windows records the crash: open Event Viewer, go to *Windows Logs > Application*, and look for an "Application Error" for `desktop-assistant.exe`. The exception code says what kind of crash it was (for example `0xc000001d`, an illegal instruction).
- **"Speech input needs a CPU with AVX2"**: this PC's CPU can't run whisper.cpp. Typing and spoken replies still work.
- **The GPU voice sounds garbled.** On some graphics cards (an RX 7900 XT, for one) the GPU garbles Kokoro's speech. In the app the GPU voice runs on DirectML, and the app doesn't detect garbling there: set `"voice_device": "cpu"` in `settings.json` with the app closed. (Only the slower built-in voice, used when the app's native voice won't load, notices garbled WebGPU output and switches to the CPU by itself.)
- **Pocket TTS is on but she still sounds like Kokoro.** The model has no Pocket voice: add `voice.safetensors`, `voice.wav` or `pocket_voice:` to its folder ([Pocket TTS voices](#pocket-tts-voices)). The log's "voice engine" line says which voice is used.
- **"Python for Pocket TTS not found at …"**: run **Voice > Set up Pocket TTS…** (see [Pocket TTS](#6-optional-cloned-voices-with-pocket-tts)), or set `pocket_python` to a `python.exe` that has `pocket-tts` installed.
- **The Pocket TTS setup failed.** The PowerShell window says which step. Fix it (usually the network) and run the setup again; it carries on from there.
- **A hotkey doesn't work.** Another app already uses it; the log says so and the tray shows no hotkey for it. Change it with `hotkeys` in `settings.json`.
- **Claude Code mode can't find Claude Code.** The app looks for `claude.exe` or `claude.cmd` on `PATH` and in the usual install folders (`%USERPROFILE%\.local\bin`, `%USERPROFILE%\.claude\local`, `%APPDATA%\npm`). Open a new terminal and check that `claude` runs there.
- **Crew mode problems**: see [docs/crew-setup.md](docs/crew-setup.md#troubleshooting).

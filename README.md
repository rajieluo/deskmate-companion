# Desktop Assistant

A Claude-powered Live2D character that lives on your Windows desktop.

It runs in a small transparent, always-on-top window. Clicks pass through to whatever is behind it except on the character itself, so it can sit on top of your other windows.

## Status

| Milestone | State |
|---|---|
| 1. Transparent on-top window, Live2D model idling, drag, click-through | this PR |
| 2. Typed chat with Claude (streaming), emotion tags drive motions | next |
| 3. Local Kokoro TTS with lip sync | planned |
| 4. Local whisper.cpp voice input | planned |

## Running it

You need [Node.js 22+](https://nodejs.org/), [Rust](https://rustup.rs/) and the [Tauri prerequisites for Windows](https://v2.tauri.app/start/prerequisites/#windows) (WebView2 is already on Windows 10/11).

```powershell
npm install          # also downloads the Live2D Cubism Core runtime
npm run tauri dev    # run the app with hot reload
npm run tauri build  # build an installer in src-tauri/target/release/bundle/nsis
```

Every pull request also builds a Windows installer in CI; download it from the run's **Artifacts** section.

## Using it

- **First run** loads `Documents\live2d models\1014100aqua\1014100.model3.json` if it exists. Otherwise it asks you to pick a `.model3.json` file.
- **Tray icon** (bottom right of the taskbar): *Choose model…*, *Reset position*, *Reload*, *Quit*.
- **Drag** the character to move her. The position and size are remembered.
- **Click** her to play a random reaction.
- **Ctrl + mouse wheel** over her makes the window bigger or smaller.
- **Keys 1 to 7** (after clicking her) play happy, sad, angry, surprised, shy, thinking and proud, so you can check a model's motions.

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
- `npm run fixtures` downloads Live2D's free sample model Hiyori into `tests/fixtures/` for testing.
- `npm test` runs the web unit tests, `cargo test` (in `src-tauri/`) the Rust ones.

### How click-through works

Tauri can only make a whole window ignore the mouse. So the Rust side polls the global cursor position about 30 times a second and sends it to the page. The page reads the alpha of the pixel under the cursor; over the character (or any element marked `data-hit`) it turns mouse events on, elsewhere it turns them off.

### Third-party code

- [Live2D Cubism Core](https://www.live2d.com/en/sdk/license/) is downloaded at install time and is subject to the Live2D Proprietary Software License. It is free for individuals and small businesses.
- [pixi-live2d-display](https://github.com/guansss/pixi-live2d-display) (MIT) renders the models.

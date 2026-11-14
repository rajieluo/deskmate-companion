//! Desktop Assistant: a transparent, always-on-top window with a Live2D avatar.
//!
//! The Rust side owns three things the web layer can't do on its own:
//! * a `model://` URI scheme that serves the user's chosen Live2D model folder
//!   (only that folder, so the webview never gets general file access),
//! * a cursor poller that tells the page where the mouse is even while the window
//!   ignores mouse events, so the page can turn click-through on and off by
//!   hit-testing the character's pixels,
//! * the tray menu (choose model, reset position, quit),
//! * the Claude chat (see `chat.rs`), so the API key never reaches the page.

mod chat;
mod claude_code;
mod model;
mod history;
mod stt;
mod voices;

use std::{sync::Arc, thread, time::Duration};

use chat::{Chat, ChatConfig};
use claude_code::ClaudeCode;
use model::{ModelInfo, ModelStore};
use history::History;
use stt::Stt;
use serde::Serialize;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, PhysicalPosition, Runtime, State, WebviewWindow,
};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};


/// Default global hotkeys. Ctrl+Shift+M and Ctrl+Shift+U were taken by another
/// app on the developer's PC, so listen and mute use Ctrl+Alt.
const DEFAULT_HOTKEYS: [(&str, &str); 3] = [("talk", "Ctrl+Shift+Space"), ("listen", "Ctrl+Alt+M"), ("mute", "Ctrl+Alt+U")];

/// The hotkeys in use: action name -> (label as typed, parsed shortcut if it registered).
#[derive(Default, Clone, Serialize)]
struct Hotkeys {
    talk: String,
    listen: String,
    mute: String,
    #[serde(skip)]
    ids: Vec<(u32, &'static str)>,
}

impl Hotkeys {
    fn label(&self, action: &str) -> &str {
        match action {
            "talk" => &self.talk,
            "listen" => &self.listen,
            _ => &self.mute,
        }
    }
}

/// Registers the hotkeys from settings (or the defaults), logging any that are
/// malformed or already taken by another app.
fn register_hotkeys<R: Runtime>(app: &AppHandle<R>) -> Hotkeys {
    let configured = app.state::<Arc<ModelStore>>().settings().hotkeys.unwrap_or_default();
    let mut keys = Hotkeys::default();
    for (action, default) in DEFAULT_HOTKEYS {
        let text = configured.get(action).cloned().unwrap_or_else(|| default.to_string());
        let mut label = text.clone();
        if !text.trim().is_empty() {
            match text.parse::<Shortcut>() {
                Ok(shortcut) => match app.global_shortcut().register(shortcut) {
                    Ok(()) => keys.ids.push((shortcut.id(), action)),
                    Err(e) => {
                        log::warn!("hotkey {action} = {text} is not available: {e}");
                        label.clear();
                    }
                },
                Err(e) => {
                    log::warn!("hotkey {action} = {text} is not a valid shortcut: {e}");
                    label.clear();
                }
            }
        }
        match action {
            "talk" => keys.talk = label,
            "listen" => keys.listen = label,
            _ => keys.mute = label,
        }
    }
    keys
}

/// Menu label with its hotkey, if it has one: "Speak to her (Ctrl+Alt+M)".
fn with_hotkey<R: Runtime>(app: &AppHandle<R>, text: &str, action: &str) -> String {
    match app.try_state::<Hotkeys>().map(|h| h.label(action).to_string()) {
        Some(k) if !k.is_empty() => format!("{text} ({k})"),
        _ => text.to_string(),
    }
}

/// Models offered in the tray's Model submenu: (menu id, model id, label).
const MODEL_CHOICES: &[(&str, &str, &str)] = &[
    ("model:claude-opus-5-5", "claude-opus-5-5", "Claude Opus 5.5 (smartest)"),
    ("model:claude-sonnet-5-5", "claude-sonnet-5-5", "Claude Sonnet 5.5"),
    ("model:claude-haiku-5-5", "claude-haiku-5-5", "Claude Haiku 5.5 (fastest, cheapest)"),
];

/// The Model submenu's items, kept so the check mark can move when one is picked.
struct ModelMenu<R: Runtime>(Vec<CheckMenuItem<R>>);
/// The tray's "Speak replies" item, so mute from the page or hotkey can update it.
struct VoiceItem<R: Runtime>(CheckMenuItem<R>);
/// The tray's "Voice" submenu: "the model's own voice" item, then (id, item) per voice.
struct VoiceMenu<R: Runtime>(CheckMenuItem<R>, Vec<(String, CheckMenuItem<R>)>);

/// Ticks the current model's voice in the tray and tells the page to use it.
fn sync_voice<R: Runtime>(app: &AppHandle<R>, sample: bool) {
    let (voice, picked) = app.state::<Arc<ModelStore>>().voice();
    if let Some(menu) = app.try_state::<VoiceMenu<R>>() {
        let _ = menu.0.set_checked(!picked);
        for (id, item) in &menu.1 {
            let _ = item.set_checked(*id == voice);
        }
    }
    log::info!("voice {voice}, {} ({})", voices::name(&voice), if picked { "picked in the tray" } else { "model default" });
    let _ = app.emit("voice-picked", serde_json::json!({ "voice": voice, "sample": sample }));
}

#[derive(Clone, Serialize, PartialEq)]
struct CursorEvent {
    /// Cursor position in physical pixels, relative to the window's top-left corner.
    x: i32,
    y: i32,
    inside: bool,
}

#[tauri::command]
fn get_model(store: State<'_, Arc<ModelStore>>) -> Option<ModelInfo> {
    store.current()
}

#[derive(Serialize)]
struct ChatStatus {
    has_key: bool,
    /// Hotkey labels ("" when unset or unavailable).
    hotkeys: Hotkeys,
    name: String,
    model: String,
    voice_enabled: bool,
    follow_mouse: bool,
    fade_bottom: bool,
    voice: String,
    voice_speed: f32,
    voice_device: String,
    /// "api" or "claude_code"
    brain: String,
    claude_code_folder: Option<String>,
}

/// Saves one chat message to the history file.
#[tauri::command]
fn history_append(
    history: State<'_, Arc<History>>,
    store: State<'_, Arc<ModelStore>>,
    role: String,
    text: String,
) -> Result<(), String> {
    let mode = store.settings().brain.unwrap_or_else(|| "api".into());
    history.append(&role, &text, &store.character_name(), &mode)
}

#[tauri::command]
fn history_read(history: State<'_, Arc<History>>) -> Vec<history::Entry> {
    history.read()
}

#[tauri::command]
fn history_clear(history: State<'_, Arc<History>>) -> Result<(), String> {
    history.clear()
}

// Async on purpose: creating a window from a synchronous command deadlocks on
// Windows (sync commands run on the main thread, which the new webview needs),
// which froze the whole app when the chat box's history button was pressed.
#[tauri::command]
async fn history_open<R: Runtime>(app: AppHandle<R>) {
    open_history(&app);
}

/// Shows the Chat history window, creating it if needed.
fn open_history<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("history") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        let _ = w.emit("history-refresh", ());
        return;
    }
    let built = tauri::WebviewWindowBuilder::new(app, "history", tauri::WebviewUrl::App("history.html".into()))
        .title("Chat history")
        .inner_size(560.0, 680.0)
        .min_inner_size(360.0, 300.0)
        .build();
    if let Err(e) = built {
        log::error!("couldn't open the history window: {e}");
    }
}

/// Mutes or unmutes her voice, keeping the tray check, settings and page in step.
fn set_voice<R: Runtime>(app: &AppHandle<R>, on: bool) {
    app.state::<Arc<ModelStore>>().update(|s| s.voice_enabled = Some(on));
    if let Some(item) = app.try_state::<VoiceItem<R>>() {
        let _ = item.0.set_checked(on);
    }
    let _ = app.emit("voice-changed", on);
}

#[tauri::command]
fn set_voice_enabled<R: Runtime>(app: AppHandle<R>, on: bool) {
    set_voice(&app, on);
}

/// Motion and expression files in the current model's folder.
#[tauri::command]
fn model_files(store: State<'_, Arc<ModelStore>>) -> Vec<String> {
    store.current_dir().map(|d| model::list_model_files(&d)).unwrap_or_default()
}

/// Lets the page write to the log file (diagnostics, errors).
#[tauri::command]
fn page_log(level: String, message: String) {
    match level.as_str() {
        "error" => log::error!("[page] {message}"),
        "warn" => log::warn!("[page] {message}"),
        _ => log::info!("[page] {message}"),
    }
}

#[tauri::command]
fn chat_status(store: State<'_, Arc<ModelStore>>, hotkeys: State<'_, Hotkeys>) -> ChatStatus {
    let s = store.settings();
    ChatStatus {
        has_key: chat::get_api_key().is_some(),
        hotkeys: hotkeys.inner().clone(),
        name: store.character_name(),
        model: s.chat_model.unwrap_or_else(|| chat::DEFAULT_MODEL.into()),
        voice_enabled: s.voice_enabled.unwrap_or(true),
        follow_mouse: s.follow_mouse.unwrap_or(true),
        fade_bottom: s.fade_bottom.unwrap_or(true),
        voice: store.voice().0,
        voice_speed: s.voice_speed.unwrap_or(1.0).clamp(0.5, 2.0),
        voice_device: s.voice_device.unwrap_or_else(|| "cpu".into()),
        brain: s.brain.unwrap_or_else(|| "api".into()),
        claude_code_folder: s.claude_code_folder.map(|p| p.display().to_string()),
    }
}

#[tauri::command]
fn set_api_key(key: String) -> Result<(), String> {
    chat::set_api_key(&key)
}

#[tauri::command]
async fn chat_send<R: Runtime>(
    app: AppHandle<R>,
    chat: State<'_, Arc<Chat>>,
    store: State<'_, Arc<ModelStore>>,
    code: State<'_, Arc<ClaudeCode>>,
    text: String,
) -> Result<u64, String> {
    let s = store.settings();
    let name = store.character_name();
    let character = store.model_persona();
    if s.brain.as_deref() == Some("claude_code") {
        let config = claude_code::RunConfig {
            folder: s.claude_code_folder.clone().unwrap_or_default(),
            permission_mode: s
                .claude_code_permission_mode
                .clone()
                .unwrap_or_else(|| claude_code::DEFAULT_PERMISSION_MODE.into()),
            prompt_file: app.path().app_config_dir().map_err(|e| e.to_string())?.join("avatar-prompt.txt"),
            name,
            character,
        };
        let code = code.inner().clone();
        let turn = code.begin();
        tauri::async_runtime::spawn(async move { code.run(&app, turn, text, config).await });
        return Ok(turn);
    }
    let config = ChatConfig {
        model: s.chat_model.unwrap_or_else(|| chat::DEFAULT_MODEL.into()),
        effort: s.chat_effort.unwrap_or_else(|| chat::DEFAULT_EFFORT.into()),
        // A persona file for this model wins; then the global `persona` setting; then the built-in one.
        persona: match character {
            Some(text) => chat::persona_prompt(&name, Some(&text)),
            None => s.persona.unwrap_or_else(|| chat::default_persona(&name)),
        },
    };
    // Return the turn number right away and stream in the background. (Awaiting the
    // whole reply here meant the page learned the turn only after every event had
    // already gone by, so it ignored them all and the bubble stayed on "…".)
    let chat = chat.inner().clone();
    let turn = chat.begin();
    tauri::async_runtime::spawn(async move { chat.run(&app, turn, text, config).await });
    Ok(turn)
}

#[tauri::command]
fn chat_cancel(chat: State<'_, Arc<Chat>>, code: State<'_, Arc<ClaudeCode>>) {
    chat.cancel();
    code.cancel();
}

#[tauri::command]
fn chat_reset(chat: State<'_, Arc<Chat>>, code: State<'_, Arc<ClaudeCode>>) {
    chat.reset();
    code.reset();
}

/// Lets the user pick the folder Claude Code works in, and switches to Claude Code mode.
#[tauri::command]
async fn pick_claude_code_folder<R: Runtime>(app: AppHandle<R>) -> Result<Option<String>, String> {
    choose_claude_code_folder(&app)
}

fn choose_claude_code_folder<R: Runtime>(app: &AppHandle<R>) -> Result<Option<String>, String> {
    let store = app.state::<Arc<ModelStore>>();
    let mut dialog = app.dialog().file().set_title("Choose the folder Claude Code should work in");
    if let Some(dir) = store.settings().claude_code_folder {
        dialog = dialog.set_directory(dir);
    }
    let Some(picked) = dialog.blocking_pick_folder() else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|e| e.to_string())?;
    store.update(|s| {
        s.claude_code_folder = Some(path.clone());
        s.brain = Some("claude_code".into());
    });
    app.state::<Arc<ClaudeCode>>().reset();
    let _ = app.emit("brain-changed", "claude_code");
    Ok(Some(path.display().to_string()))
}

fn set_brain<R: Runtime>(app: &AppHandle<R>, brain: &str) {
    app.state::<Arc<ModelStore>>().update(|s| s.brain = Some(brain.into()));
    let _ = app.emit("brain-changed", brain);
}

fn stt_settings<R: Runtime>(app: &AppHandle<R>) -> Result<(String, String, std::path::PathBuf), String> {
    let s = app.state::<Arc<ModelStore>>().settings();
    let name = s.stt_model.unwrap_or_else(|| stt::DEFAULT_MODEL.into());
    let language = s.stt_language.unwrap_or_else(|| "en".into());
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("whisper");
    Ok((name, language, dir))
}

/// Downloads the Whisper model ahead of time (no-op when it's already there).
#[tauri::command]
async fn stt_prepare<R: Runtime>(app: AppHandle<R>, stt: State<'_, Arc<Stt>>) -> Result<(), String> {
    stt::check_cpu()?;
    let (name, _, dir) = stt_settings(&app)?;
    stt.ensure_model(&app, &dir, &name).await.map(|_| ()).inspect_err(|e| log::error!("speech model: {e}"))
}

/// Transcribes raw 16 kHz mono f32 samples sent as the request body.
#[tauri::command]
async fn transcribe<R: Runtime>(
    app: AppHandle<R>,
    stt: State<'_, Arc<Stt>>,
    request: tauri::ipc::Request<'_>,
) -> Result<String, String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("expected raw audio bytes".into());
    };
    stt::check_cpu()?;
    let samples = stt::samples_from_bytes(bytes);
    let (name, language, dir) = stt_settings(&app)?;
    let path = stt.ensure_model(&app, &dir, &name).await.inspect_err(|e| log::error!("speech model: {e}"))?;
    let stt = stt.inner().clone();
    tauri::async_runtime::spawn_blocking(move || stt.transcribe(&name, &path, &samples, &language))
        .await
        .map_err(|e| e.to_string())?
        .inspect_err(|e| log::error!("transcribe: {e}"))
}

/// Brings the window forward and opens the chat box.
fn open_chat<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_ignore_cursor_events(false);
        let _ = w.set_focus();
        let _ = w.emit_to(w.label(), "open-chat", ());
    }
}

fn start_listening<R: Runtime>(app: &AppHandle<R>) {
    open_chat(app);
    let _ = app.emit("start-listening", ());
}

#[tauri::command]
async fn pick_model<R: Runtime>(app: AppHandle<R>) -> Result<Option<ModelInfo>, String> {
    choose_model(&app)
}

fn choose_model<R: Runtime>(app: &AppHandle<R>) -> Result<Option<ModelInfo>, String> {
    let store = app.state::<Arc<ModelStore>>();
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Choose a Live2D model (.model3.json)")
        .add_filter("Live2D model", &["json"]);
    if let Some(dir) = store.current_dir().and_then(|d| d.parent().map(|p| p.to_path_buf())) {
        dialog = dialog.set_directory(dir);
    }
    let Some(picked) = dialog.blocking_pick_file() else {
        return Ok(None);
    };
    let path = picked.into_path().map_err(|e| e.to_string())?;
    let info = store.set(&path)?;
    let _ = app.emit("model-changed", &info);
    sync_voice(app, false);
    Ok(Some(info))
}

fn reset_position<R: Runtime>(window: &WebviewWindow<R>) {
    if let (Ok(Some(monitor)), Ok(size)) = (window.primary_monitor(), window.outer_size()) {
        let area = monitor.work_area();
        let x = area.position.x + area.size.width as i32 - size.width as i32 - 24;
        let y = area.position.y + area.size.height as i32 - size.height as i32;
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
}

/// Polls the global cursor position and forwards it to the page (~30 Hz, only on change).
fn spawn_cursor_poller<R: Runtime>(window: WebviewWindow<R>) {
    thread::spawn(move || {
        let mut last: Option<CursorEvent> = None;
        loop {
            thread::sleep(Duration::from_millis(33));
            let (Ok(cursor), Ok(pos), Ok(size)) =
                (window.cursor_position(), window.inner_position(), window.inner_size())
            else {
                continue;
            };
            let x = cursor.x.round() as i32 - pos.x;
            let y = cursor.y.round() as i32 - pos.y;
            let inside = x >= 0 && y >= 0 && x < size.width as i32 && y < size.height as i32;
            let event = CursorEvent { x, y, inside };
            if last.as_ref() != Some(&event) {
                if window.emit_to(window.label(), "cursor", &event).is_err() {
                    break;
                }
                last = Some(event);
            }
        }
    });
}

fn build_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let choose = MenuItem::with_id(app, "choose", "Choose model…", true, None::<&str>)?;
    let reset = MenuItem::with_id(app, "reset", "Reset position", true, None::<&str>)?;
    let talk = MenuItem::with_id(app, "talk", with_hotkey(app, "Type to her", "talk"), true, None::<&str>)?;
    let listen = MenuItem::with_id(app, "listen", with_hotkey(app, "Speak to her", "listen"), true, None::<&str>)?;
    let forget = MenuItem::with_id(app, "forget", "Forget conversation", true, None::<&str>)?;
    let key = MenuItem::with_id(app, "key", "Set API key…", true, None::<&str>)?;
    let voice_on = app.state::<Arc<ModelStore>>().settings().voice_enabled.unwrap_or(true);
    let voice = CheckMenuItem::with_id(app, "voice", with_hotkey(app, "Speak replies", "mute"), true, voice_on, None::<&str>)?;
    app.manage(VoiceItem(voice.clone()));
    let history_item = MenuItem::with_id(app, "history", "Chat history…", true, None::<&str>)?;
    let follow_on = app.state::<Arc<ModelStore>>().settings().follow_mouse.unwrap_or(true);
    let follow = CheckMenuItem::with_id(app, "follow", "Follow the mouse", true, follow_on, None::<&str>)?;
    let fade_on = app.state::<Arc<ModelStore>>().settings().fade_bottom.unwrap_or(true);
    let fade = CheckMenuItem::with_id(app, "fade", "Fade bottom edge", true, fade_on, None::<&str>)?;
    let code_on = app.state::<Arc<ModelStore>>().settings().brain.as_deref() == Some("claude_code");
    let code = CheckMenuItem::with_id(app, "code", "Claude Code mode", true, code_on, None::<&str>)?;
    let code_folder = MenuItem::with_id(app, "code_folder", "Claude Code folder…", true, None::<&str>)?;
    let sep3 = PredefinedMenuItem::separator(app)?;
    let current_model = app
        .state::<Arc<ModelStore>>()
        .settings()
        .chat_model
        .unwrap_or_else(|| chat::DEFAULT_MODEL.into());
    let mut model_items = Vec::new();
    for (id, model, label) in MODEL_CHOICES {
        model_items.push(CheckMenuItem::with_id(app, *id, *label, true, current_model == *model, None::<&str>)?);
    }
    let model_refs: Vec<&dyn tauri::menu::IsMenuItem<R>> =
        model_items.iter().map(|i| i as &dyn tauri::menu::IsMenuItem<R>).collect();
    let model_menu = Submenu::with_items(app, "Chat model", true, &model_refs)?;
    app.manage(ModelMenu(model_items.clone()));
    let (current_voice, voice_picked) = app.state::<Arc<ModelStore>>().voice();
    let own_voice = CheckMenuItem::with_id(app, "voice:default", "Model's own voice", true, !voice_picked, None::<&str>)?;
    let mut voice_items = Vec::new();
    let mut group_menus = Vec::new();
    for group in voices::GROUPS {
        let mut items = Vec::new();
        for (id, name, _) in voices::VOICES.iter().filter(|(_, _, g)| g == group) {
            let item = CheckMenuItem::with_id(app, format!("voice:{id}"), *name, true, current_voice == *id, None::<&str>)?;
            voice_items.push((id.to_string(), item.clone()));
            items.push(item);
        }
        let refs: Vec<&dyn tauri::menu::IsMenuItem<R>> = items.iter().map(|i| i as &dyn tauri::menu::IsMenuItem<R>).collect();
        group_menus.push(Submenu::with_items(app, *group, true, &refs)?);
    }
    let voice_sep = PredefinedMenuItem::separator(app)?;
    let mut voice_refs: Vec<&dyn tauri::menu::IsMenuItem<R>> = vec![&own_voice, &voice_sep];
    voice_refs.extend(group_menus.iter().map(|m| m as &dyn tauri::menu::IsMenuItem<R>));
    let voice_menu = Submenu::with_items(app, "Voice", true, &voice_refs)?;
    app.manage(VoiceMenu(own_voice.clone(), voice_items));
    let reload = MenuItem::with_id(app, "reload", "Reload", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu =
        Menu::with_items(app, &[&talk, &listen, &voice, &voice_menu, &history_item, &forget, &sep3, &model_menu, &code, &code_folder, &key, &sep, &choose, &follow, &fade, &reset, &reload, &sep2, &quit])?;

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Desktop Assistant")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "choose" => {
                let app = app.clone();
                // The dialog blocks, so keep it off the event loop thread.
                thread::spawn(move || {
                    if let Err(e) = choose_model(&app) {
                        let _ = app.emit("model-error", e);
                    }
                });
            }
            "talk" => open_chat(app),
            "listen" => start_listening(app),
            "forget" => {
                app.state::<Arc<Chat>>().reset();
                let _ = app.emit("chat-reset", ());
            }
            "voice" => {
                // The menu has already flipped its own check mark.
                let on = !app.state::<Arc<ModelStore>>().settings().voice_enabled.unwrap_or(true);
                set_voice(app, on);
            }
            "history" => open_history(app),
            "fade" => {
                let store = app.state::<Arc<ModelStore>>();
                let on = !store.settings().fade_bottom.unwrap_or(true);
                store.update(|s| s.fade_bottom = Some(on));
                let _ = app.emit("fade-bottom-changed", on);
            }
            "follow" => {
                let store = app.state::<Arc<ModelStore>>();
                let on = !store.settings().follow_mouse.unwrap_or(true);
                store.update(|s| s.follow_mouse = Some(on));
                let _ = app.emit("follow-mouse-changed", on);
            }
            "code" => {
                let store = app.state::<Arc<ModelStore>>();
                let s = store.settings();
                if s.brain.as_deref() == Some("claude_code") {
                    set_brain(app, "api");
                } else if s.claude_code_folder.as_ref().is_some_and(|f| f.is_dir()) {
                    set_brain(app, "claude_code");
                } else {
                    let app = app.clone();
                    thread::spawn(move || {
                        let _ = choose_claude_code_folder(&app);
                    });
                }
            }
            "code_folder" => {
                let app = app.clone();
                thread::spawn(move || {
                    let _ = choose_claude_code_folder(&app);
                });
            }
            "key" => {
                open_chat(app);
                let _ = app.emit("open-api-key", ());
            }
            "reset" => {
                if let Some(w) = app.get_webview_window("main") {
                    reset_position(&w);
                }
            }
            "reload" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.eval("location.reload()");
                }
            }
            "quit" => app.exit(0),
            "voice:default" => {
                app.state::<Arc<ModelStore>>().pick_voice(None);
                sync_voice(app, true);
            }
            id if id.starts_with("voice:") => {
                let voice = id.trim_start_matches("voice:");
                if voices::is_voice(voice) {
                    app.state::<Arc<ModelStore>>().pick_voice(Some(voice));
                }
                sync_voice(app, true);
            }
            id if id.starts_with("model:") => {
                let model = id.trim_start_matches("model:").to_string();
                app.state::<Arc<ModelStore>>().update(|s| s.chat_model = Some(model.clone()));
                if let Some(menu) = app.try_state::<ModelMenu<R>>() {
                    for (item, (item_id, _, _)) in menu.0.iter().zip(MODEL_CHOICES) {
                        let _ = item.set_checked(*item_id == id);
                    }
                }
                let _ = app.emit("model-picked", model);
            }
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // A second launch just brings the running one forward. Two copies fight over the
        // hotkeys and the settings file, and one of them may be an old version.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            log::info!("second launch: showing the running instance instead");
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        // Log file: %LOCALAPPDATA%\com.rajieluo.desktopassistant\logs\Desktop Assistant.log on Windows.
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .max_file_size(2_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepOne)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    let action = app
                        .try_state::<Hotkeys>()
                        .and_then(|h| h.ids.iter().find(|(id, _)| *id == shortcut.id()).map(|(_, a)| *a));
                    match action {
                        Some("listen") => start_listening(app),
                        Some("mute") => {
                            let on = !app.state::<Arc<ModelStore>>().settings().voice_enabled.unwrap_or(true);
                            set_voice(app, on);
                        }
                        Some("talk") => open_chat(app),
                        _ => {}
                    }
                })
                .build(),
        )
        .register_asynchronous_uri_scheme_protocol("model", |ctx, request, responder| {
            let store = ctx.app_handle().state::<Arc<ModelStore>>().inner().clone();
            thread::spawn(move || responder.respond(store.serve(request.uri().path())));
        })
        .setup(|app| {
            let default_hook = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                log::error!("panic: {info}");
                default_hook(info);
            }));
            log::info!("Desktop Assistant {} starting; {}", env!("CARGO_PKG_VERSION"), stt::cpu_summary());
            let config_dir = app.path().app_config_dir()?;
            let first_run = !config_dir.join("settings.json").exists();
            let documents = app.path().document_dir().ok();
            let store = Arc::new(ModelStore::load(config_dir, documents));
            // Saving now also marks later launches as "not first run".
            store.save();
            app.manage(store);
            app.manage(Arc::new(Chat::default()));
            app.manage(Arc::new(ClaudeCode::default()));

            let hotkeys = register_hotkeys(app.handle());
            app.manage(hotkeys);
            app.manage(Arc::new(Stt::default()));
            app.manage(Arc::new(History::new(app.path().app_data_dir()?)));

            build_tray(app.handle())?;

            let window = app.get_webview_window("main").expect("main window");
            // First run: the window-state plugin has nothing to restore yet.
            if first_run {
                reset_position(&window);
            }
            // A restored size too small to show her (seen: 160x28) makes her vanish
            // with no way back but deleting .window-state.json. Restore the default.
            if let (Ok(size), Ok(scale)) = (window.inner_size(), window.scale_factor()) {
                let size = size.to_logical::<f64>(scale);
                if size.width < 150.0 || size.height < 250.0 {
                    log::warn!("window size {}x{} is too small; resetting", size.width, size.height);
                    let _ = window.set_size(tauri::LogicalSize::new(380.0, 720.0));
                    reset_position(&window);
                }
            }
            spawn_cursor_poller(window);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_model,
            pick_model,
            chat_status,
            set_api_key,
            chat_send,
            chat_cancel,
            chat_reset,
            stt_prepare,
            pick_claude_code_folder,
            transcribe,
            page_log,
            model_files,
            history_append,
            history_read,
            history_clear,
            history_open,
            set_voice_enabled
        ])
        .run(tauri::generate_context!())
        .expect("error while running Desktop Assistant");
}

#[cfg(test)]
mod hotkey_tests {
    use super::DEFAULT_HOTKEYS;
    use tauri_plugin_global_shortcut::Shortcut;

    #[test]
    fn default_hotkeys_parse() {
        for (_, text) in DEFAULT_HOTKEYS {
            assert!(text.parse::<Shortcut>().is_ok(), "{text}");
        }
        assert!("Ctrl+Shift+M".parse::<Shortcut>().is_ok());
        assert!("Nope+Q".parse::<Shortcut>().is_err());
    }
}

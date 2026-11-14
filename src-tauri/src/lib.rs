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
mod model;
mod stt;

use std::{sync::Arc, thread, time::Duration};

use chat::{Chat, ChatConfig};
use model::{ModelInfo, ModelStore};
use stt::Stt;
use serde::Serialize;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, PhysicalPosition, Runtime, State, WebviewWindow,
};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

const DEFAULT_NAME: &str = "Aqua";

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
    name: String,
    model: String,
    voice_enabled: bool,
    voice: String,
    voice_speed: f32,
}

#[tauri::command]
fn chat_status(store: State<'_, Arc<ModelStore>>) -> ChatStatus {
    let s = store.settings();
    ChatStatus {
        has_key: chat::get_api_key().is_some(),
        name: s.character_name.unwrap_or_else(|| DEFAULT_NAME.into()),
        model: s.chat_model.unwrap_or_else(|| chat::DEFAULT_MODEL.into()),
        voice_enabled: s.voice_enabled.unwrap_or(true),
        voice: s.voice.unwrap_or_else(|| "af_heart".into()),
        voice_speed: s.voice_speed.unwrap_or(1.0).clamp(0.5, 2.0),
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
    text: String,
) -> Result<u64, String> {
    let s = store.settings();
    let name = s.character_name.unwrap_or_else(|| DEFAULT_NAME.into());
    let config = ChatConfig {
        model: s.chat_model.unwrap_or_else(|| chat::DEFAULT_MODEL.into()),
        effort: s.chat_effort.unwrap_or_else(|| chat::DEFAULT_EFFORT.into()),
        persona: s.persona.unwrap_or_else(|| chat::default_persona(&name)),
    };
    let chat = chat.inner().clone();
    Ok(chat.send(&app, text, config).await)
}

#[tauri::command]
fn chat_cancel(chat: State<'_, Arc<Chat>>) {
    chat.cancel();
}

#[tauri::command]
fn chat_reset(chat: State<'_, Arc<Chat>>) {
    chat.reset();
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
    let (name, _, dir) = stt_settings(&app)?;
    stt.ensure_model(&app, &dir, &name).await.map(|_| ())
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
    let samples = stt::samples_from_bytes(bytes);
    let (name, language, dir) = stt_settings(&app)?;
    let path = stt.ensure_model(&app, &dir, &name).await?;
    let stt = stt.inner().clone();
    tauri::async_runtime::spawn_blocking(move || stt.transcribe(&name, &path, &samples, &language))
        .await
        .map_err(|e| e.to_string())?
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
    let talk = MenuItem::with_id(app, "talk", "Type to her (Ctrl+Shift+Space)", true, None::<&str>)?;
    let listen = MenuItem::with_id(app, "listen", "Speak to her (Ctrl+Shift+M)", true, None::<&str>)?;
    let forget = MenuItem::with_id(app, "forget", "Forget conversation", true, None::<&str>)?;
    let key = MenuItem::with_id(app, "key", "Set API key…", true, None::<&str>)?;
    let voice_on = app.state::<Arc<ModelStore>>().settings().voice_enabled.unwrap_or(true);
    let voice = CheckMenuItem::with_id(app, "voice", "Speak replies", true, voice_on, None::<&str>)?;
    let reload = MenuItem::with_id(app, "reload", "Reload", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let menu =
        Menu::with_items(app, &[&talk, &listen, &voice, &forget, &key, &sep, &choose, &reset, &reload, &sep2, &quit])?;

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
                let store = app.state::<Arc<ModelStore>>();
                let on = !store.settings().voice_enabled.unwrap_or(true);
                store.update(|s| s.voice_enabled = Some(on));
                let _ = app.emit("voice-changed", on);
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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        if shortcut.key == Code::KeyM {
                            start_listening(app);
                        } else {
                            open_chat(app);
                        }
                    }
                })
                .build(),
        )
        .register_asynchronous_uri_scheme_protocol("model", |ctx, request, responder| {
            let store = ctx.app_handle().state::<Arc<ModelStore>>().inner().clone();
            thread::spawn(move || responder.respond(store.serve(request.uri().path())));
        })
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let first_run = !config_dir.join("settings.json").exists();
            let documents = app.path().document_dir().ok();
            let store = Arc::new(ModelStore::load(config_dir, documents));
            // Saving now also marks later launches as "not first run".
            store.save();
            app.manage(store);
            app.manage(Arc::new(Chat::default()));

            let talk = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space);
            let listen = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyM);
            for shortcut in [talk, listen] {
                if let Err(e) = app.global_shortcut().register(shortcut) {
                    eprintln!("could not register {shortcut:?}: {e}");
                }
            }
            app.manage(Arc::new(Stt::default()));

            build_tray(app.handle())?;

            let window = app.get_webview_window("main").expect("main window");
            // First run: the window-state plugin has nothing to restore yet.
            if first_run {
                reset_position(&window);
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
            transcribe
        ])
        .run(tauri::generate_context!())
        .expect("error while running Desktop Assistant");
}

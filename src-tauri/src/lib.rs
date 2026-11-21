//! Desktop Assistant: a transparent, always-on-top window with a Live2D avatar.
//!
//! The Rust side owns three things the web layer can't do on its own:
//! * a `model://` URI scheme that serves the user's chosen Live2D model folder
//!   (only that folder, so the webview never gets general file access),
//! * a cursor poller that tells the page where the mouse is even while the window
//!   ignores mouse events, so the page can turn click-through on and off by
//!   hit-testing the character's pixels,
//! * the tray menu (choose model, reset position, quit).

mod model;

use std::{sync::Arc, thread, time::Duration};

use model::{ModelInfo, ModelStore};
use serde::Serialize;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, PhysicalPosition, Runtime, State, WebviewWindow,
};
use tauri_plugin_dialog::DialogExt;

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
    let reload = MenuItem::with_id(app, "reload", "Reload", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&choose, &reset, &reload, &sep, &quit])?;

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
        .register_asynchronous_uri_scheme_protocol("model", |ctx, request, responder| {
            let store = ctx.app_handle().state::<Arc<ModelStore>>().inner().clone();
            thread::spawn(move || responder.respond(store.serve(request.uri().path())));
        })
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let first_run = !config_dir.join("settings.json").exists();
            let documents = app.path().document_dir().ok();
            let store = Arc::new(ModelStore::load(config_dir, documents));
            app.manage(store);

            build_tray(app.handle())?;

            let window = app.get_webview_window("main").expect("main window");
            // First run: the window-state plugin has nothing to restore yet.
            if first_run {
                reset_position(&window);
            }
            spawn_cursor_poller(window);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_model, pick_model])
        .run(tauri::generate_context!())
        .expect("error while running Desktop Assistant");
}

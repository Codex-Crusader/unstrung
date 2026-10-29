//! Unstrung desktop shell. It starts the core, opens the window, and forwards
//! engine status to the UI as events.

#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod logfile;
mod selftest;
mod state;

use state::AppState;
use std::sync::Arc;
use tauri::http::{Response, StatusCode};
use tauri::{Emitter, Manager, RunEvent};
use unstrung_core::chat::ChatService;
use unstrung_core::db::Db;
use unstrung_core::engine::Engine;
use unstrung_core::paths::AppPaths;
use unstrung_core::starters;

pub const STATUS_EVENT: &str = "engine-status";

fn fail(message: &str) -> ! {
    log::error!("{message}");
    eprintln!("{message}");
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let self_test = args.iter().any(|a| a == "--self-test");
    // Opens the window without focus, so tests do not take over the desktop.
    let background = args.iter().any(|a| a == "--background");
    let paths = AppPaths::resolve().unwrap_or_else(|e| fail(&format!("no app folder: {e}")));
    if let Err(e) = paths.ensure() {
        fail(&format!(
            "could not create the app folder {}: {e}",
            paths.root().display()
        ));
    }
    if let Err(e) = logfile::init(&paths.logs().join("app.log")) {
        eprintln!("no log file: {e}");
    }
    log::info!(
        "Unstrung {} starting, app folder {}",
        env!("CARGO_PKG_VERSION"),
        paths.root().display()
    );

    let app = tauri::Builder::default()
        .register_asynchronous_uri_scheme_protocol("avatar", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn(async move {
                responder.respond(avatar(&app, request.uri().path()).await);
            });
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::list_characters,
            commands::list_personas,
            commands::open_character,
            commands::open_chat,
            commands::new_chat,
            commands::list_chats,
            commands::delete_chat,
            commands::send_message,
            commands::edit_message,
            commands::edit_reply,
            commands::regenerate,
            commands::continue_reply,
            commands::retry_reply,
            commands::stop_reply,
            commands::rewind,
            commands::show_branch,
            commands::delete_message,
            commands::undo_delete,
            commands::branch_chat,
            commands::set_chat_persona,
            commands::set_chat_tone,
            commands::set_chat_style,
            commands::restart_engine,
            selftest::self_test_mode,
            selftest::self_test_report,
        ])
        .setup(move |app| {
            let state = tauri::async_runtime::block_on(open_core(paths, self_test))?;
            open_window(app, background)?;
            start_engine(app.handle(), &state.engine);
            if self_test {
                selftest::arm_timeout(app.handle().clone());
            }
            app.manage(state);
            Ok(())
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| fail(&format!("the window could not open: {e}")));

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            let engine = handle.state::<AppState>().engine.clone();
            tauri::async_runtime::block_on(engine.stop());
            log::info!("Unstrung closed");
        }
    });
}

fn open_window(app: &tauri::App, background: bool) -> tauri::Result<()> {
    tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
        .title("Unstrung")
        .inner_size(1100.0, 760.0)
        .min_inner_size(640.0, 480.0)
        .background_color(tauri::window::Color(30, 30, 30, 255))
        .focused(!background)
        .build()?;
    Ok(())
}

async fn open_core(paths: AppPaths, self_test: bool) -> unstrung_core::Result<AppState> {
    let db = Db::open(&paths.database()).await?;
    if !db.quick_check().await? {
        // Phase 7 adds snapshots and the restore screen.
        log::error!("the database failed its quick check");
    }
    starters::ensure(&db, &paths).await?;
    let engine = Engine::new(paths.clone());
    let chats = Arc::new(ChatService::new(db, engine.clone()));
    Ok(AppState {
        paths,
        engine,
        chats,
        self_test,
    })
}

/// Starts the engine in the background and sends each status change to the window.
fn start_engine(app: &tauri::AppHandle, engine: &Arc<Engine>) {
    let mut status = engine.subscribe();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let current = status.borrow_and_update().clone();
            if let Err(e) = handle.emit(STATUS_EVENT, current) {
                log::warn!("could not send the engine status: {e}");
            }
            if status.changed().await.is_err() {
                break;
            }
        }
    });
    let engine = engine.clone();
    tauri::async_runtime::spawn(async move {
        engine.start().await;
        engine.watch_health().await;
    });
}

/// Serves `avatar://localhost/character/<id>`. The path comes from the database,
/// never from the request.
async fn avatar(app: &tauri::AppHandle, path: &str) -> Response<Vec<u8>> {
    let not_found = || {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Vec::new())
            .unwrap_or_default()
    };
    let state = app.state::<AppState>();
    let Some((kind, id)) = path
        .trim_start_matches('/')
        .split_once('/')
        .and_then(|(kind, id)| Some((kind, id.parse::<i64>().ok()?)))
    else {
        return not_found();
    };
    let db = state.chats.db();
    let stored = match kind {
        "character" => db.character(id).await.ok().and_then(|c| c.avatar_path),
        "persona" => db.persona(id).await.ok().and_then(|p| p.avatar_path),
        _ => None,
    };
    let Some(stored) = stored else {
        return not_found();
    };
    let mime = match stored
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => return not_found(),
    };
    let Ok(file) = state.paths.stored(&stored) else {
        return not_found();
    };
    match tokio::fs::read(file).await {
        Ok(bytes) => Response::builder()
            .header("Content-Type", mime)
            .header("X-Content-Type-Options", "nosniff")
            .header("Cache-Control", "no-cache")
            .body(bytes)
            .unwrap_or_default(),
        Err(_) => not_found(),
    }
}

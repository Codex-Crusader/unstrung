//! Unstrung desktop shell. It starts the core, opens the window, and forwards
//! engine status to the UI as events.

#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod creator;
mod instance;
mod logfile;
mod safe;
mod selftest;
mod state;

use safe::StartInfo;
use state::AppState;
use std::sync::Arc;
use tauri::http::{Response, StatusCode};
use tauri::{Emitter, Manager, RunEvent};
use unstrung_core::chat::ChatService;
use unstrung_core::db::Db;
use unstrung_core::engine::Engine;
use unstrung_core::models::{self, ModelService};
use unstrung_core::paths::AppPaths;
use unstrung_core::start::{self, SafeReason};
use unstrung_core::starters;

pub const STATUS_EVENT: &str = "engine-status";
pub const DOWNLOAD_EVENT: &str = "model-download";
pub const MEMORY_EVENT: &str = "chat-memory";

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
    // The lock comes first: a second process must not write the log or count
    // a start. Windows frees the lock when the process ends, also after a crash.
    let restart = args.iter().any(|a| a == instance::RESTART_ARG);
    let folder_lock = match instance::claim(&paths, restart) {
        instance::Claim::First(lock) => lock,
        instance::Claim::Second => {
            if instance::wake_first(&paths) {
                std::process::exit(0);
            }
            fail(&format!(
                "Unstrung is already open with the app folder {}, but its window does not answer",
                paths.root().display()
            ));
        }
    };
    if let Err(e) = logfile::init(&paths.logs().join("app.log")) {
        eprintln!("no log file: {e}");
    }
    log::info!(
        "Unstrung {} starting, app folder {}",
        env!("CARGO_PKG_VERSION"),
        paths.root().display()
    );
    // The self-test never counts starts, so a killed test run cannot turn the
    // next one into a safe start. It opens safe start only when asked.
    let asked = args.iter().any(|a| a == safe::SAFE_START_ARG);
    let safe_reason = if self_test {
        asked.then_some(SafeReason::Asked)
    } else {
        start::decide(start::attempts(&paths), asked)
    };
    match safe_reason {
        Some(reason) => log::warn!("safe start: {reason:?}"),
        None if !self_test => {
            // Counted before anything that can crash. The window clears it.
            if let Err(e) = start::record_attempt(&paths) {
                log::warn!("could not count the start: {e}");
            }
        }
        None => {}
    }

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
            commands::chat_memories,
            commands::remember,
            commands::edit_memory,
            commands::set_memory_flags,
            commands::set_chat_style,
            commands::restart_engine,
            commands::models_page,
            commands::recheck_hardware,
            commands::download_model,
            commands::stop_download,
            commands::remove_model,
            commands::set_default_model,
            commands::set_advanced,
            commands::set_chat_model,
            commands::set_character_model,
            creator::get_character,
            creator::save_character,
            creator::delete_character,
            creator::restore_starters,
            creator::save_persona,
            creator::delete_persona,
            creator::set_default_persona,
            creator::stage_upload,
            creator::import_card,
            creator::suggest_anchor,
            creator::test_reply,
            selftest::self_test_mode,
            selftest::self_test_report,
            safe::start_mode,
            safe::start_ok,
            safe::start_normally,
            safe::diagnostic_report,
            safe::repair_check,
            safe::repair_fix,
        ])
        .setup(move |app| {
            // Held until the process ends.
            std::mem::forget(folder_lock);
            instance::serve(app.handle(), &paths);
            let mut info = StartInfo {
                paths: paths.clone(),
                safe: safe_reason,
                error: String::new(),
                self_test,
            };
            if self_test {
                selftest::arm_timeout(app.handle().clone());
            }
            if info.safe.is_none() {
                // If the chat data does not open, safe start shows why,
                // instead of an app that closes at once.
                match tauri::async_runtime::block_on(open_core(paths)) {
                    Ok(state) => {
                        start_engine(app.handle(), &state.engine, &state.models);
                        send_memory_changes(app.handle(), &state.chats);
                        app.manage(state);
                    }
                    Err(e) => {
                        log::error!("the chat data did not open: {e}");
                        info.safe = Some(SafeReason::DataDidNotOpen);
                        info.error = e.to_string();
                    }
                }
            }
            app.manage(info);
            open_window(app, background)?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| fail(&format!("the window could not open: {e}")));

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            // Safe start has no chat state.
            let Some(state) = handle.try_state::<AppState>() else {
                log::info!("Unstrung closed");
                return;
            };
            tauri::async_runtime::block_on(async {
                state.engine.stop().await;
                // Closing the last connection writes the log into the database file.
                state.chats.db().close().await;
            });
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

async fn open_core(paths: AppPaths) -> unstrung_core::Result<AppState> {
    let db = Db::open(&paths.database()).await?;
    if !db.quick_check().await? {
        // Phase 7 adds snapshots and the restore screen.
        log::error!("the database failed its quick check");
    }
    unstrung_core::avatar::clear_staging(&paths);
    starters::ensure(&db, &paths).await?;
    let startup = models::startup(&db, &paths).await?;
    log::info!(
        "hardware tier {:?}, default model {}",
        startup.hardware.tier,
        startup.model
    );
    let engine = Engine::with_model(paths.clone(), &startup.model, startup.advanced.clone());
    let models = ModelService::new(engine.clone(), db.clone(), &startup);
    let chats = Arc::new(ChatService::new(db, engine.clone()));
    Ok(AppState {
        paths,
        engine,
        chats,
        models,
    })
}

/// Sends the chat id to the window each time a memory pass changes that chat.
fn send_memory_changes(app: &tauri::AppHandle, chats: &Arc<ChatService>) {
    let mut changed = chats.subscribe_memory();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        while changed.changed().await.is_ok() {
            let chat_id = *changed.borrow_and_update();
            if let Err(e) = handle.emit(MEMORY_EVENT, chat_id) {
                log::warn!("could not send the memory change: {e}");
            }
        }
    });
}

/// Starts the engine in the background and sends each status change to the window.
fn start_engine(app: &tauri::AppHandle, engine: &Arc<Engine>, models: &Arc<ModelService>) {
    let mut downloads = models.subscribe();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        while downloads.changed().await.is_ok() {
            let current = downloads.borrow_and_update().clone();
            if let Err(e) = handle.emit(DOWNLOAD_EVENT, current) {
                log::warn!("could not send the download progress: {e}");
            }
        }
    });
    let models = models.clone();
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
        if let Err(e) = models.after_start().await {
            log::warn!("could not pick an installed model: {e}");
        }
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
    let Some(state) = app.try_state::<AppState>() else {
        return not_found();
    };
    let Some((kind, key)) = path.trim_start_matches('/').split_once('/') else {
        return not_found();
    };
    let db = state.chats.db();
    let id = key.parse::<i64>().ok();
    let stored = match (kind, id) {
        ("character", Some(id)) => db.character(id).await.ok().and_then(|c| c.avatar_path),
        ("persona", Some(id)) => db.persona(id).await.ok().and_then(|p| p.avatar_path),
        // A picture that the user is cropping now.
        ("staged", _) => unstrung_core::avatar::staged_path(key).ok(),
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

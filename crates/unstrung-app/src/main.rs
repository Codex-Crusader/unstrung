//! Unstrung desktop shell. It starts the core, opens the window, and forwards
//! engine status to the UI as events.

#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod creator;
mod instance;
mod logfile;
mod notify;
mod safe;
mod selftest;
mod setup;
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
    // The uninstaller asks to delete the data. This runs before anything
    // creates a folder or opens the log in the folder that it deletes.
    if args
        .iter()
        .any(|a| a == unstrung_core::uninstall::DELETE_ARG)
    {
        let paths = AppPaths::resolve().unwrap_or_else(|e| fail(&format!("no app folder: {e}")));
        let left = unstrung_core::uninstall::delete_data(&paths);
        for line in &left {
            eprintln!("{line}");
        }
        std::process::exit(if left.is_empty() { 0 } else { 2 });
    }
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
    let asked = args.iter().any(|a| a == safe::SAFE_START_ARG);
    let folder_lock = match instance::claim(&paths, restart) {
        instance::Claim::First(lock) => lock,
        instance::Claim::Second => {
            let log = paths.logs().join("app.log");
            // A relaunch never sends a wish, so two processes cannot keep
            // restarting each other.
            let wish = if asked {
                instance::Wish::Safe
            } else {
                instance::Wish::Show
            };
            if !restart && instance::send(&paths, wish) {
                std::process::exit(0);
            }
            let message = if restart {
                "the earlier Unstrung window did not close in time"
            } else {
                "Unstrung is already open with this app folder, but its window does not answer"
            };
            logfile::append_one(&log, log::Level::Error, "unstrung_app::instance", message);
            fail(message);
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
            commands::recent_chats,
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
            commands::import_model_file,
            commands::open_windows_security,
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
            setup::setup_state,
            setup::pick_folder,
            setup::check_folder,
            setup::set_location,
            setup::engine_install,
            setup::engine_install_cancel,
            setup::finish_first_run,
            setup::choose_setup,
            notify::set_notify,
            notify::get_notify,
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
                        setup::forward_progress(app.handle());
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
            state.chats.shut_down();
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
        .background_color(window_background())
        .focused(!background)
        .build()?;
    Ok(())
}

/// The page background of the Windows app theme, so the window does not
/// flash the other color before the page draws (`--bg` in styles.css).
fn window_background() -> tauri::window::Color {
    if prefers_light_theme() {
        tauri::window::Color(244, 239, 230, 255)
    } else {
        tauri::window::Color(30, 30, 30, 255)
    }
}

#[cfg(windows)]
fn prefers_light_theme() -> bool {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_value::<u32, _>("AppsUseLightTheme"))
        // Windows starts in light mode when the value is missing.
        .map_or(true, |light| light != 0)
}

#[cfg(not(windows))]
fn prefers_light_theme() -> bool {
    false
}

async fn open_core(paths: AppPaths) -> unstrung_core::Result<AppState> {
    // Db::open checks the file for damage before it changes anything.
    let db = Db::open(&paths.database()).await?;
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
    let notify = db.meta(notify::META_KEY).await?.as_deref() != Some("off");
    let chats = Arc::new(ChatService::new(db, engine.clone()));
    Ok(AppState {
        setup: unstrung_core::engine_setup::EngineSetup::new(paths.clone()),
        paths,
        engine,
        chats,
        models,
        notify: std::sync::atomic::AtomicBool::new(notify),
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
            if current
                .as_ref()
                .is_some_and(|d| d.state == unstrung_core::models::DownloadState::Done)
            {
                notify::job_done(&handle, notify::Job::Model);
            }
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

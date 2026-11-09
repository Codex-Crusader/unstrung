//! Unstrung desktop shell. It starts the core, opens the window, and forwards
//! engine status to the UI as events.

#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod creator;
mod instance;
mod logfile;
mod notify;
mod pictures;
mod safe;
mod selftest;
mod settings;
mod setup;
mod state;
mod updates;

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
use unstrung_core::pictures::service::PictureService;
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
        .plugin(tauri_plugin_updater::Builder::new().build())
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
            commands::get_adult_content,
            commands::set_adult_content,
            commands::move_folder,
            commands::free_up_candidates,
            commands::free_up_space,
            commands::storage_usage,
            commands::replace_from_backup,
            commands::open_backup,
            commands::add_from_backup,
            commands::discard_backup,
            commands::export_backup,
            commands::backup_status,
            commands::snooze_backup_reminder,
            commands::auto_backup_status,
            commands::choose_auto_backup_folder,
            commands::stop_auto_backup,
            commands::rebuild_summary,
            commands::save_summary,
            commands::next_chapter,
            commands::refresh_context,
            commands::undo_refresh,
            commands::kept_memories,
            commands::keep_memory,
            commands::forget_kept_memory,
            commands::set_memory_mode,
            commands::set_chat_use_memory,
            commands::list_chats,
            commands::recent_chats,
            commands::delete_chat,
            commands::deleted_chats,
            commands::restore_chat,
            commands::send_message,
            commands::edit_message,
            commands::edit_reply,
            commands::regenerate,
            commands::continue_reply,
            commands::retry_reply,
            commands::stop_reply,
            pictures::pictures_page,
            pictures::download_picture_model,
            pictures::stop_picture_download,
            pictures::select_picture_model,
            pictures::remove_picture_model,
            pictures::paint_picture,
            pictures::repaint_picture,
            pictures::stage_picture,
            pictures::export_picture,
            pictures::chat_pictures,
            pictures::gallery,
            pictures::delete_picture,
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
            creator::suggest_looks,
            creator::suggest_looks_from_picture,
            creator::can_read_pictures,
            creator::test_reply,
            selftest::self_test_mode,
            selftest::self_test_report,
            safe::start_mode,
            safe::start_ok,
            safe::start_normally,
            safe::diagnostic_report,
            safe::repair_check,
            safe::repair_fix,
            safe::list_copies,
            settings::get_ui_settings,
            settings::set_ui_setting,
            settings::restart_safe,
            safe::restore_copy,
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
            updates::check_update,
            updates::install_update,
            updates::get_update_check,
            updates::set_update_check,
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
            app.manage(setup::FolderGrants::new(info.paths.clone(), self_test));
            let mut theme = None;
            // Before the chats file opens: nothing holds the files yet.
            if let Err(e) = unstrung_core::backup::apply_pending(&info.paths) {
                log::error!("the backup import failed: {e}");
                info.safe = Some(SafeReason::ImportFailed);
                info.error = e.to_string();
            }
            if info.safe.is_none() {
                // If the chat data does not open, safe start shows why,
                // instead of an app that closes at once.
                match tauri::async_runtime::block_on(open_core(paths)) {
                    Ok(state) => {
                        theme = tauri::async_runtime::block_on(
                            state.chats.db().meta(settings::THEME_KEY),
                        )
                        .ok()
                        .flatten();
                        start_engine(app.handle(), &state.engine, &state.models);
                        send_memory_changes(app.handle(), &state.chats);
                        app.manage(state);
                        hourly_upkeep(app.handle());
                        if !self_test {
                            updates::check_after_start(app.handle());
                        }
                        setup::forward_progress(app.handle());
                        pictures::forward_progress(app.handle());
                    }
                    Err(e) => {
                        log::error!("the chat data did not open: {e}");
                        info.safe = Some(SafeReason::DataDidNotOpen);
                        info.error = e.to_string();
                    }
                }
            }
            app.manage(info);
            clear_old_picture_cache(app);
            open_window(app, background, theme.as_deref())?;
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

/// Version 0.1.0 let WebView2 keep pictures in its disk cache, plain. The
/// pictures are encrypted now and never cached, so the old cache goes, once,
/// before the window opens. Local storage (the window's own settings) stays.
fn clear_old_picture_cache(app: &tauri::App) {
    let Ok(dir) = app.path().app_local_data_dir() else {
        return;
    };
    let done = dir.join("picture-cache-cleared");
    if done.exists() {
        return;
    }
    let cache = dir.join("EBWebView").join("Default").join("Cache");
    match std::fs::remove_dir_all(&cache) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            log::warn!("could not clear the old picture cache: {e}");
            return;
        }
        _ => {}
    }
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&done, b"")) {
        log::warn!("could not note the cleared picture cache: {e}");
    }
}

/// `theme` is the saved theme setting, so the window opens in its color.
fn open_window(app: &tauri::App, background: bool, theme: Option<&str>) -> tauri::Result<()> {
    let theme = settings::window_theme(settings::value_of(settings::THEME_KEY, theme));
    tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default())
        .title("Unstrung")
        .inner_size(1100.0, 760.0)
        .min_inner_size(640.0, 480.0)
        // WebView2 takes the window theme as the page's prefers-color-scheme,
        // so the splash and the app open in the saved theme.
        .theme(theme)
        .background_color(window_background(theme))
        .focused(!background)
        // The app has no outside links. A page that tries to leave the app,
        // or to open a second window, is stopped.
        .on_navigation(|url| {
            let ok = local_page(url);
            if !ok {
                log::warn!("blocked a page change away from the app");
            }
            ok
        })
        .on_new_window(|_, _| {
            log::warn!("blocked a new window");
            tauri::webview::NewWindowResponse::Deny
        })
        .build()?;
    Ok(())
}

/// The app's own pages: the bundled UI and the isolation frame, which
/// WebView2 serves as `http://<name>.localhost`.
fn local_page(url: &tauri::Url) -> bool {
    let local_host = url
        .host_str()
        .is_some_and(|h| h == "localhost" || h.ends_with(".localhost"));
    match url.scheme() {
        "http" | "https" => local_host,
        "tauri" | "isolation" => true,
        _ => false,
    }
}

/// The page background of the Windows app theme, so the window does not
/// flash the other color before the page draws (`--bg` in styles.css).
fn window_background(theme: Option<tauri::Theme>) -> tauri::window::Color {
    let light = match theme {
        Some(tauri::Theme::Dark) => false,
        Some(_) => true,
        None => prefers_light_theme(),
    };
    if light {
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

/// The app can stay open for weeks: the daily copy, the end of "Recently
/// deleted", the engine log size and the weekly automatic backup must not
/// wait for a restart.
fn hourly_upkeep(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // The first round soon after the start, so a user who opens the app
        // for half an hour a day still gets the automatic backup.
        let first = tokio::time::Instant::now() + std::time::Duration::from_secs(5 * 60);
        let mut hourly = tokio::time::interval_at(first, std::time::Duration::from_secs(60 * 60));
        hourly.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            hourly.tick().await;
            let Some(state) = app.try_state::<AppState>() else {
                return;
            };
            let db = state.chats.db();
            match db.daily_snapshot(&state.paths.database()).await {
                Ok(Some(file)) => log::info!("daily copy of the chats: {}", file.display()),
                Ok(None) => {}
                Err(e) => log::warn!("could not make the daily copy of the chats: {e}"),
            }
            unstrung_core::sidecar::trim_log(&state.paths);
            // An export or a library move holds the files, and a purge then could
            // delete a picture under them. The next hour tries again.
            if let Ok(_files) = state.files.take()
                && let Err(e) = unstrung_core::library::purge_deleted_chats(
                    db,
                    &state.paths,
                    unstrung_core::db::now_ms(),
                )
                .await
            {
                log::warn!("could not remove the chats deleted more than 7 days ago: {e}");
            }
            if let Ok(_files) = state.files.take() {
                match unstrung_core::autobackup::run_if_due(
                    db,
                    &state.paths,
                    unstrung_core::db::now_ms(),
                    env!("CARGO_PKG_VERSION"),
                )
                .await
                {
                    Ok(Some(done)) => log::info!(
                        "automatic backup: {} bytes, {} pictures",
                        done.bytes,
                        done.pictures
                    ),
                    Ok(None) => {}
                    Err(e) => log::warn!("the automatic backup failed: {e}"),
                }
            }
        }
    });
}

async fn open_core(paths: AppPaths) -> unstrung_core::Result<AppState> {
    // Before anything reads the pictures or the models.
    unstrung_core::relocate::recover(&paths);
    // The data key first: without it no chat or picture opens, and a key
    // that does not unwrap must stop the start before anything is written.
    let key = paths.key()?;
    // Db::open checks the file for damage before it changes anything.
    let db = Db::open(&paths.database(), &key).await?;
    // A missed copy must not stop the app: the next start tries again.
    match db.daily_snapshot(&paths.database()).await {
        Ok(Some(file)) => log::info!("daily copy of the chats: {}", file.display()),
        Ok(None) => {}
        Err(e) => log::warn!("could not make the daily copy of the chats: {e}"),
    }
    if let Err(e) =
        unstrung_core::library::purge_deleted_chats(&db, &paths, unstrung_core::db::now_ms()).await
    {
        log::warn!("could not remove the chats deleted more than 7 days ago: {e}");
    }
    unstrung_core::avatar::clear_staging(&paths);
    unstrung_core::backup::clear_leftovers(&paths);
    // Data from before encryption: converted once, in the background.
    let old = paths.clone();
    let copies_key = key.clone();
    tauri::async_runtime::spawn(async move {
        let n = unstrung_core::db::encrypt_old_copies(&old.backups(), &copies_key).await;
        if n > 0 {
            log::info!("encrypted {n} copies of the chats file made before encryption");
        }
        let pictures = tauri::async_runtime::spawn_blocking(move || {
            unstrung_core::vault::seal_old_pictures(&old)
        })
        .await;
        match pictures {
            Ok(Ok(0)) => {}
            Ok(Ok(n)) => log::info!("encrypted {n} pictures saved before encryption"),
            Ok(Err(e)) => log::warn!("could not encrypt the older pictures: {e}"),
            Err(e) => log::warn!("could not encrypt the older pictures: {e}"),
        }
    });
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
    let pictures = PictureService::new(paths.clone(), chats.clone(), models.clone());
    Ok(AppState {
        setup: unstrung_core::engine_setup::EngineSetup::new(
            paths.clone(),
            unstrung_core::engine_setup::Engine::Chat,
        ),
        paths,
        engine,
        chats,
        models,
        pictures,
        notify: std::sync::atomic::AtomicBool::new(notify),
        files: state::FileGuard::default(),
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

/// Serves `avatar://localhost/character/<id>` (also persona, staged,
/// picture, a gallery picture by id, and thumb, its small copy for the grid).
/// The path comes from the database, never from the request.
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
    if let ("picture" | "thumb", Some(id)) = (kind, id) {
        // Gallery pictures live in the library folder, not the app folder.
        let Ok(image) = db.image(id).await else {
            return not_found();
        };
        let Ok(file) = unstrung_core::paths::join_safe(&state.paths.gallery(), &image.file_path)
        else {
            return not_found();
        };
        let mut bytes = plain(&state.paths, file).await;
        let mut mime = "image/png";
        if kind == "thumb"
            && let Some(full) = bytes.take()
        {
            // A picture the thumbnail step cannot read (one over 15 MB from a
            // backup) shows in full rather than as an empty tile.
            let small = tokio::task::spawn_blocking(move || {
                unstrung_core::avatar::thumbnail(&full).map_err(|e| (e, full))
            })
            .await;
            bytes = match small {
                Ok(Ok(thumb)) => {
                    mime = "image/jpeg";
                    Some(thumb)
                }
                Ok(Err((e, full))) => {
                    log::warn!("no thumbnail, the full picture instead: {e}");
                    Some(full)
                }
                Err(_) => None,
            };
        }
        return match bytes {
            Some(bytes) => Response::builder()
                .header("Content-Type", mime)
                .header("X-Content-Type-Options", "nosniff")
                .header("Cache-Control", "no-store")
                .body(bytes)
                .unwrap_or_default(),
            None => not_found(),
        };
    }
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
    match plain(&state.paths, file).await {
        Some(bytes) => Response::builder()
            .header("Content-Type", mime)
            .header("X-Content-Type-Options", "nosniff")
            .header("Cache-Control", "no-store")
            .body(bytes)
            .unwrap_or_default(),
        None => not_found(),
    }
}

/// The decrypted bytes of a picture file. `no-store` on the responses keeps
/// them out of the WebView2 disk cache, so no plain copy lands on disk.
async fn plain(
    paths: &unstrung_core::paths::AppPaths,
    file: std::path::PathBuf,
) -> Option<Vec<u8>> {
    let key = paths.key().ok()?;
    let bytes = tokio::fs::read(file).await.ok()?;
    key.open(&bytes)
        .inspect_err(|e| log::warn!("a picture did not open: {e}"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::local_page;

    #[test]
    fn only_app_pages_may_load() {
        let ok = |u: &str| tauri::Url::parse(u).is_ok_and(|u| local_page(&u));
        assert!(ok("http://tauri.localhost/index.html"));
        assert!(ok("http://isolation.localhost/index.html"));
        assert!(ok("tauri://localhost/"));
        assert!(!ok("https://example.com/"));
        assert!(!ok("http://localhost.example.com/"));
        assert!(!ok("file:///C:/Windows/win.ini"));
        assert!(!ok("javascript:alert(1)"));
    }
}

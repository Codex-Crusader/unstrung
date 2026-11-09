//! Commands for backups and for the space that the data takes on disk.

use super::*;

/// When the last backup was made, and whether to remind the user now.
#[tauri::command]
pub async fn backup_status(
    state: State<'_, AppState>,
) -> CmdResult<unstrung_core::backup::BackupStatus> {
    let db = state.chats.db();
    Ok(unstrung_core::backup::status(db, unstrung_core::db::now_ms()).await?)
}

/// "Later" on the backup reminder.
#[tauri::command]
pub async fn snooze_backup_reminder(state: State<'_, AppState>) -> CmdResult<()> {
    let db = state.chats.db();
    Ok(unstrung_core::backup::snooze(db, unstrung_core::db::now_ms()).await?)
}

/// "Export everything": the user picks where the zip goes. `None` when
/// the user closes the picker.
#[tauri::command]
pub async fn export_backup(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> CmdResult<Option<unstrung_core::backup::Exported>> {
    let Some(to) = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .set_file_name("Unstrung backup.zip")
        .add_filter("Zip", &["zip"])
        .save_file()
        .await
    else {
        return Ok(None);
    };
    // Taken after the picker, so an open picker blocks nothing.
    let _files = state.files.take()?;
    let done = unstrung_core::backup::export(
        state.chats.db(),
        &state.paths,
        to.path(),
        env!("CARGO_PKG_VERSION"),
    )
    .await?;
    log::info!(
        "backup exported: {} bytes, {} avatars, {} pictures",
        done.bytes,
        done.avatars,
        done.pictures
    );
    Ok(Some(done))
}

#[tauri::command]
pub async fn auto_backup_status(
    state: State<'_, AppState>,
) -> CmdResult<unstrung_core::autobackup::AutoBackup> {
    Ok(unstrung_core::autobackup::status(state.chats.db()).await?)
}

/// Turns automatic backups on with a folder that the user picks here, so no
/// folder path comes from the window. `None` when the user closes the picker.
#[tauri::command]
pub async fn choose_auto_backup_folder(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> CmdResult<Option<unstrung_core::autobackup::AutoBackup>> {
    let Some(folder) = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .pick_folder()
        .await
    else {
        return Ok(None);
    };
    Ok(Some(
        unstrung_core::autobackup::set_folder(state.chats.db(), &state.paths, Some(folder.path()))
            .await?,
    ))
}

#[tauri::command]
pub async fn stop_auto_backup(
    state: State<'_, AppState>,
) -> CmdResult<unstrung_core::autobackup::AutoBackup> {
    Ok(unstrung_core::autobackup::set_folder(state.chats.db(), &state.paths, None).await?)
}

/// "Import backup": the user picks a zip. It is checked and staged, and
/// nothing changes yet. `None` when the user closes the picker.
/// With a `password`, it opens the zip that the user picked last time, so
/// the window never sends a file path.
#[tauri::command]
pub async fn open_backup(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    password: Option<String>,
) -> CmdResult<Option<unstrung_core::backup::BackupInfo>> {
    let picked = if password.is_some() {
        state
            .picked_backup
            .lock()
            .map_err(|_| Error::Busy)?
            .clone()
            .ok_or_else(|| Error::Invalid("Pick the backup again.".into()))?
    } else {
        let Some(file) = rfd::AsyncFileDialog::new()
            .set_parent(&window)
            .add_filter("Zip", &["zip"])
            .pick_file()
            .await
        else {
            return Ok(None);
        };
        let path = file.path().to_path_buf();
        *state.picked_backup.lock().map_err(|_| Error::Busy)? = Some(path.clone());
        path
    };
    Ok(Some(
        unstrung_core::backup::open_with(&state.paths, &picked, password).await?,
    ))
}

/// Sets the backup password, or removes it with `None`.
#[tauri::command]
pub async fn set_backup_password(
    state: State<'_, AppState>,
    password: Option<String>,
) -> CmdResult<()> {
    Ok(unstrung_core::backup::set_password(state.chats.db(), password.as_deref()).await?)
}

#[tauri::command]
pub async fn add_from_backup(
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<unstrung_core::backup::Added> {
    Ok(unstrung_core::backup::add_characters(state.chats.db(), &state.paths, &id).await?)
}

/// "Replace": the app restarts, and the new start swaps the data.
#[tauri::command]
pub async fn replace_from_backup(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<()> {
    // A running reply or picture would write to the old data, and a
    // running export would stop half way.
    state.chats.idle()?;
    let _files = state.files.take()?;
    unstrung_core::backup::ask_replace(&state.paths, &id)?;
    log::info!("restart to replace the data with a backup");
    if let Err(e) = crate::instance::relaunch(&app, false) {
        unstrung_core::backup::cancel_replace(&state.paths);
        return Err(e.into());
    }
    Ok(())
}

/// What uses disk space. Walks folders, so it runs off the async threads.
#[tauri::command]
pub async fn storage_usage(state: State<'_, AppState>) -> CmdResult<unstrung_core::storage::Usage> {
    let paths = state.paths.clone();
    tauri::async_runtime::spawn_blocking(move || unstrung_core::storage::usage(&paths))
        .await
        .map_err(|e| unstrung_core::error::Error::Io(std::io::Error::other(e)).into())
}

/// Settings > Storage > Change location. Holds the job slot and the picture
/// download slot, so nothing writes to the folders. A models move stops
/// the chat engine and starts it again after. `on_progress` gets
/// [bytes copied, bytes to copy]. Stop (`stop_reply`) ends the copy, and
/// then nothing changes.
#[tauri::command]
pub async fn move_folder(
    state: State<'_, AppState>,
    grants: State<'_, crate::setup::FolderGrants>,
    purpose: unstrung_core::location::Purpose,
    path: String,
    on_progress: Channel<[u64; 2]>,
) -> CmdResult<Vec<std::path::PathBuf>> {
    use unstrung_core::relocate;
    grants.check(std::path::Path::new(&path))?;
    let (token, _guard) = state.chats.begin_job()?;
    let _pictures = state.pictures.hold()?;
    let _files = state.files.take()?;
    if state.models.downloading() || state.setup.busy() {
        return Err(Error::Invalid("Wait until the download ends.".into()).into());
    }
    let joined = |e: tauri::Error| Error::Io(std::io::Error::other(e));
    let paths = state.paths.clone();
    let plan = tauri::async_runtime::spawn_blocking(move || {
        relocate::plan(&paths, purpose, std::path::Path::new(&path))
    })
    .await
    .map_err(joined)??;
    let models = purpose == unstrung_core::location::Purpose::Models;
    if models {
        state.engine.stop().await;
    }
    let paths = state.paths.clone();
    let total = plan.bytes;
    emit(&on_progress, [0, total]);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut last = 0;
        relocate::run(
            &paths,
            &plan,
            &mut |done| {
                if done - last > total / 200 || done == total {
                    last = done;
                    emit(&on_progress, [done, total]);
                }
            },
            &token,
        )
    })
    .await
    .map_err(joined);
    if models {
        state.engine.restart().await;
        // The free space check of downloads reads the new disk.
        if let Err(e) = state.models.recheck_hardware().await {
            log::warn!("hardware check after the move: {e}");
        }
    }
    Ok(result??)
}

#[tauri::command]
pub async fn free_up_candidates(
    state: State<'_, AppState>,
) -> CmdResult<Vec<unstrung_core::storage::Candidate>> {
    Ok(unstrung_core::storage::candidates(state.chats.db(), &state.paths).await?)
}

/// Removes the picked lines of "Free up space", after the user confirmed.
#[tauri::command]
pub async fn free_up_space(state: State<'_, AppState>, ids: Vec<String>) -> CmdResult<u64> {
    // A running reply or picture may write the files that go.
    let (_token, _guard) = state.chats.begin_job()?;
    let _files = state.files.take()?;
    Ok(unstrung_core::storage::free_up(state.chats.db(), &state.paths, &ids).await?)
}

/// Async, and the delete in a blocking task: an opened backup can hold
/// thousands of pictures, which would freeze the window.
#[tauri::command]
pub async fn discard_backup(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let paths = state.paths.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        unstrung_core::backup::discard(&paths, &id);
    })
    .await;
    Ok(())
}

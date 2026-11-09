//! Commands for the chat engine and the chat models.

use super::*;

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> EngineStatus {
    state.engine.status()
}

#[tauri::command]
pub async fn restart_engine(state: State<'_, AppState>) -> CmdResult<()> {
    // A restart under a running reply, picture or suggestion would cut it.
    state.chats.idle()?;
    let engine = state.engine.clone();
    tauri::async_runtime::spawn(async move { engine.restart().await });
    Ok(())
}

#[tauri::command]
pub async fn models_page(state: State<'_, AppState>) -> CmdResult<ModelsPage> {
    Ok(state.models.page().await?)
}

#[tauri::command]
pub async fn recheck_hardware(state: State<'_, AppState>) -> CmdResult<ModelsPage> {
    state.models.recheck_hardware().await?;
    Ok(state.models.page().await?)
}

/// Starts a download. Progress comes as `model-download` events.
/// Async so that Tauri runs it inside the Tokio runtime: a sync command runs
/// on the main thread, where the spawn in `download` panics and the app aborts.
#[tauri::command]
pub async fn download_model(state: State<'_, AppState>, model: String) -> CmdResult<()> {
    Ok(state.models.download(&model)?)
}

/// Opens the Windows file picker for a GGUF file and starts the import.
/// The path comes only from the picker, never from the page. `None` when the
/// user closes the picker; otherwise the new model's name.
#[tauri::command]
pub async fn import_model_file(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> CmdResult<Option<String>> {
    #[cfg(windows)]
    {
        let Some(file) = rfd::AsyncFileDialog::new()
            .set_parent(&window)
            .add_filter("GGUF model", &["gguf"])
            .pick_file()
            .await
        else {
            return Ok(None);
        };
        Ok(Some(
            state.models.import_file(file.path().to_path_buf()).await?,
        ))
    }
    #[cfg(not(windows))]
    {
        let _ = (window, state);
        Ok(None)
    }
}

/// Opens Windows Security, for the antivirus tip. The app never changes
/// antivirus settings itself.
#[tauri::command]
pub fn open_windows_security() -> CmdResult<()> {
    #[cfg(windows)]
    std::process::Command::new("explorer.exe")
        .arg("windowsdefender:")
        .spawn()
        .map_err(Error::from)?;
    Ok(())
}

#[tauri::command]
pub fn stop_download(state: State<'_, AppState>) {
    state.models.stop_download();
}

#[tauri::command]
pub async fn remove_model(state: State<'_, AppState>, model: String) -> CmdResult<ModelsPage> {
    state.models.remove(&model).await?;
    Ok(state.models.page().await?)
}

#[tauri::command]
pub async fn set_default_model(state: State<'_, AppState>, model: String) -> CmdResult<ModelsPage> {
    state.models.set_default(&model).await?;
    Ok(state.models.page().await?)
}

#[tauri::command]
pub async fn set_advanced(state: State<'_, AppState>, advanced: Advanced) -> CmdResult<ModelsPage> {
    state.models.set_advanced(advanced).await?;
    Ok(state.models.page().await?)
}

#[tauri::command]
pub async fn set_chat_model(
    state: State<'_, AppState>,
    chat_id: i64,
    model: Option<String>,
) -> CmdResult<ChatView> {
    Ok(state.chats.set_model(chat_id, model.as_deref()).await?)
}

#[tauri::command]
pub async fn set_character_model(
    state: State<'_, AppState>,
    character_id: i64,
    model: Option<String>,
) -> CmdResult<()> {
    Ok(state
        .chats
        .set_character_model(character_id, model.as_deref())
        .await?)
}

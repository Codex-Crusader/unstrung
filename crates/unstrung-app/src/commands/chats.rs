//! Commands for characters, chats, their settings and their memories.

use super::*;

#[tauri::command]
pub async fn list_characters(state: State<'_, AppState>) -> CmdResult<Vec<Character>> {
    let db = state.chats.db();
    let mut all = db.characters().await?;
    // 18+ cards show only with the adult content setting on.
    if !unstrung_core::content::adult_on(db).await? {
        all.retain(|c| !c.is_adult);
    }
    Ok(all)
}

#[tauri::command]
pub async fn get_adult_content(state: State<'_, AppState>) -> CmdResult<bool> {
    Ok(unstrung_core::content::adult_on(state.chats.db()).await?)
}

/// Settings > Content. The page asks "Are you 18 or older?" before it sends
/// `true`. Off needs no question.
#[tauri::command]
pub async fn set_adult_content(state: State<'_, AppState>, on: bool) -> CmdResult<()> {
    log::info!("adult content setting: {}", if on { "on" } else { "off" });
    Ok(unstrung_core::content::set_adult(state.chats.db(), on).await?)
}

#[tauri::command]
pub async fn list_personas(state: State<'_, AppState>) -> CmdResult<Vec<Persona>> {
    Ok(state.chats.db().personas().await?)
}

/// The latest chat with a character, or a new one.
#[tauri::command]
pub async fn open_character(state: State<'_, AppState>, character_id: i64) -> CmdResult<ChatView> {
    Ok(state.chats.open_latest(character_id).await?)
}

#[tauri::command]
pub async fn open_chat(state: State<'_, AppState>, chat_id: i64) -> CmdResult<ChatView> {
    Ok(state.chats.view(chat_id).await?)
}

/// Imports a SillyTavern chat file that the user picks here, as a new chat
/// with the character. `None` when the user closes the picker.
#[tauri::command]
pub async fn import_chat(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    character_id: i64,
    persona_id: Option<i64>,
) -> CmdResult<Option<ChatView>> {
    use unstrung_core::chatimport;
    let db = state.chats.db();
    let character = db.character(character_id).await?;
    if character.is_adult && !unstrung_core::content::adult_on(db).await? {
        return Err(unstrung_core::Error::NotFound("character").into());
    }
    if let Some(id) = persona_id {
        db.persona(id).await?;
    }
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .add_filter("SillyTavern chat", &["jsonl"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    let path = file.path().to_path_buf();
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        let size = std::fs::metadata(&path)?.len();
        if size > chatimport::MAX_FILE_BYTES {
            return Err(unstrung_core::Error::Invalid(format!(
                "the file is bigger than {} MB",
                chatimport::MAX_FILE_BYTES / 1024 / 1024
            )));
        }
        Ok(std::fs::read(&path)?)
    })
    .await
    .map_err(|e| unstrung_core::Error::Io(std::io::Error::other(e)))??;
    let done = chatimport::import(db, &character, persona_id, &bytes).await?;
    Ok(Some(state.chats.view(done.chat_id).await?))
}

#[tauri::command]
pub async fn new_chat(
    state: State<'_, AppState>,
    character_id: i64,
    persona_id: Option<i64>,
    use_memory: bool,
) -> CmdResult<ChatView> {
    Ok(state
        .chats
        .new_chat_using(character_id, persona_id, use_memory)
        .await?)
}

/// "Refresh context". The memory pass starts, so the summary catches up.
#[tauri::command]
pub async fn refresh_context(state: State<'_, AppState>, chat_id: i64) -> CmdResult<ChatView> {
    let view = state.chats.refresh_context(chat_id).await?;
    let chats = state.chats.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = chats.update_memory(chat_id).await {
            log::warn!("memory pass failed: {e}");
        }
    });
    Ok(view)
}

#[tauri::command]
pub async fn next_chapter(state: State<'_, AppState>, chat_id: i64) -> CmdResult<ChatView> {
    Ok(state.chats.next_chapter(chat_id).await?)
}

#[tauri::command]
pub async fn rebuild_summary(
    state: State<'_, AppState>,
    chat_id: i64,
) -> CmdResult<RebuiltSummary> {
    Ok(state.chats.rebuild_summary(chat_id).await?)
}

#[tauri::command]
pub async fn save_summary(
    state: State<'_, AppState>,
    chat_id: i64,
    through_id: i64,
    text: String,
) -> CmdResult<Vec<Memory>> {
    Ok(state.chats.save_summary(chat_id, through_id, &text).await?)
}

/// Messages of all chats that hold the text, newest first.
#[tauri::command]
pub async fn search_chats(
    state: State<'_, AppState>,
    text: String,
) -> CmdResult<Vec<unstrung_core::db::SearchHit>> {
    let db = state.chats.db();
    let adult = unstrung_core::content::adult_on(db).await?;
    Ok(db.search(&text, adult).await?)
}

/// "Save as text": the visible branch of a chat as a Markdown file in a place
/// that the user picks. The file name, or `None` when the user closes the picker.
#[tauri::command]
pub async fn save_chat_text(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    chat_id: i64,
) -> CmdResult<Option<String>> {
    let transcript = state.chats.db().transcript(chat_id).await?;
    let Some(to) = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .set_file_name(&transcript.file_name)
        .add_filter("Markdown", &["md"])
        .add_filter("Text", &["txt"])
        .save_file()
        .await
    else {
        return Ok(None);
    };
    let path = to.path().to_path_buf();
    let text = transcript.text;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    tauri::async_runtime::spawn_blocking(move || {
        unstrung_core::files::write_atomic(&path, text.as_bytes())
    })
    .await
    .map_err(|e| unstrung_core::error::Error::Io(std::io::Error::other(e)))??;
    Ok(name)
}

/// Chats, messages and pictures per character, for Settings > Storage.
#[tauri::command]
pub async fn chat_stats(
    state: State<'_, AppState>,
) -> CmdResult<Vec<unstrung_core::db::CharacterStats>> {
    let db = state.chats.db();
    let adult = unstrung_core::content::adult_on(db).await?;
    Ok(db.stats(adult).await?)
}

#[tauri::command]
pub async fn undo_refresh(state: State<'_, AppState>, chat_id: i64) -> CmdResult<ChatView> {
    Ok(state.chats.undo_refresh(chat_id).await?)
}

/// The character's kept memories and whether new chats use them.
#[tauri::command]
pub async fn kept_memories(state: State<'_, AppState>, character_id: i64) -> CmdResult<Kept> {
    Ok(state.chats.db().kept(character_id).await?)
}

#[tauri::command]
pub async fn keep_memory(
    state: State<'_, AppState>,
    chat_id: i64,
    memory_id: i64,
) -> CmdResult<Kept> {
    Ok(state.chats.keep_memory(chat_id, memory_id).await?)
}

#[tauri::command]
pub async fn forget_kept_memory(
    state: State<'_, AppState>,
    character_id: i64,
    memory_id: i64,
) -> CmdResult<Kept> {
    let db = state.chats.db();
    db.forget_character_memory(character_id, memory_id).await?;
    Ok(db.kept(character_id).await?)
}

#[tauri::command]
pub async fn set_memory_mode(
    state: State<'_, AppState>,
    character_id: i64,
    mode: String,
) -> CmdResult<Kept> {
    let db = state.chats.db();
    db.set_memory_mode(character_id, &mode).await?;
    Ok(db.kept(character_id).await?)
}

#[tauri::command]
pub async fn set_chat_use_memory(
    state: State<'_, AppState>,
    chat_id: i64,
    on: bool,
) -> CmdResult<()> {
    Ok(state.chats.db().set_chat_use_memory(chat_id, on).await?)
}

#[tauri::command]
pub async fn list_chats(
    state: State<'_, AppState>,
    character_id: i64,
) -> CmdResult<Vec<ChatSummary>> {
    Ok(state.chats.chats(character_id).await?)
}

/// How many chats the "Recent chats" row on Home shows.
pub(super) const RECENT_CHATS: i64 = 100;

#[tauri::command]
pub async fn recent_chats(state: State<'_, AppState>) -> CmdResult<Vec<RecentChat>> {
    let db = state.chats.db();
    let adult = unstrung_core::content::adult_on(db).await?;
    Ok(db.recent_chats(RECENT_CHATS, adult).await?)
}

#[tauri::command]
pub async fn delete_chat(state: State<'_, AppState>, chat_id: i64) -> CmdResult<()> {
    Ok(state.chats.delete_chat(chat_id).await?)
}

#[tauri::command]
pub async fn deleted_chats(state: State<'_, AppState>) -> CmdResult<Vec<DeletedChat>> {
    let db = state.chats.db();
    let adult = unstrung_core::content::adult_on(db).await?;
    Ok(db.deleted_chats(adult).await?)
}

#[tauri::command]
pub async fn restore_chat(state: State<'_, AppState>, chat_id: i64) -> CmdResult<()> {
    Ok(state.chats.db().restore_chat(chat_id).await?)
}

#[tauri::command]
pub async fn set_chat_persona(
    state: State<'_, AppState>,
    chat_id: i64,
    persona_id: Option<i64>,
) -> CmdResult<ChatView> {
    Ok(state.chats.set_persona(chat_id, persona_id).await?)
}

#[tauri::command]
pub async fn set_chat_tone(
    state: State<'_, AppState>,
    chat_id: i64,
    tone: String,
    custom: String,
) -> CmdResult<ChatView> {
    Ok(state.chats.set_tone(chat_id, &tone, &custom).await?)
}

#[tauri::command]
pub async fn chat_memories(state: State<'_, AppState>, chat_id: i64) -> CmdResult<Vec<Memory>> {
    Ok(state.chats.memories(chat_id).await?)
}

#[tauri::command]
pub async fn remember(
    state: State<'_, AppState>,
    chat_id: i64,
    message_id: i64,
    text: String,
) -> CmdResult<Vec<Memory>> {
    Ok(state.chats.remember(chat_id, message_id, &text).await?)
}

#[tauri::command]
pub async fn edit_memory(
    state: State<'_, AppState>,
    chat_id: i64,
    memory_id: i64,
    text: String,
) -> CmdResult<Vec<Memory>> {
    Ok(state.chats.edit_memory(chat_id, memory_id, &text).await?)
}

#[tauri::command]
pub async fn set_memory_flags(
    state: State<'_, AppState>,
    chat_id: i64,
    memory_id: i64,
    pinned: bool,
    hidden: bool,
) -> CmdResult<Vec<Memory>> {
    Ok(state
        .chats
        .set_memory_flags(chat_id, memory_id, pinned, hidden)
        .await?)
}

#[tauri::command]
pub async fn set_chat_style(
    state: State<'_, AppState>,
    chat_id: i64,
    style: String,
    custom: Option<StyleSettings>,
) -> CmdResult<ChatView> {
    Ok(state.chats.set_style(chat_id, &style, custom).await?)
}

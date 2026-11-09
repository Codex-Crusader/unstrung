//! The chat commands that the UI can call. Each one checks its input and
//! returns errors in plain words, with the technical detail apart.

use crate::state::AppState;
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;
use unstrung_core::Error;
use unstrung_core::catalog::Advanced;
use unstrung_core::chat::{ChatView, DeleteUndo, Progress, RebuiltSummary, RegenMode, ReplyResult};
use unstrung_core::db::{
    Character, ChatSummary, DeletedChat, Kept, Memory, Message, Persona, RecentChat,
};
use unstrung_core::engine::EngineStatus;
use unstrung_core::models::ModelsPage;
use unstrung_core::presets::StyleSettings;

#[derive(Debug, Clone, Serialize)]
pub struct UiError {
    pub message: String,
    pub detail: String,
    /// A tip that the page shows under the error, for example "antivirus".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip: Option<&'static str>,
}

/// How many "file in use" errors this run saw. From the second one, the
/// error carries the antivirus tip, because real-time scanners lock files.
static FILE_IN_USE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A file that another program holds open (Windows sharing or lock violation).
fn file_in_use(e: &Error) -> bool {
    match e {
        Error::Io(io) => matches!(io.raw_os_error(), Some(32 | 33)),
        Error::Engine(text) => {
            let text = text.to_lowercase();
            text.contains("being used by another process") || text.contains("sharing violation")
        }
        _ => false,
    }
}

impl From<Error> for UiError {
    fn from(e: Error) -> Self {
        if let Error::Rejected { message, why } = &e {
            log::warn!("command failed: {e}");
            return Self {
                message: sentence(message),
                detail: why.clone(),
                tip: None,
            };
        }
        if file_in_use(&e) {
            log::warn!("command failed, file in use: {e}");
            let seen = FILE_IN_USE.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            return Self {
                message: "Another program is using a file that Unstrung needs. Wait a moment, then try again."
                    .to_owned(),
                detail: e.to_string(),
                tip: (seen >= 2).then_some("antivirus"),
            };
        }
        let message = match &e {
            Error::NotReady => {
                "The chat engine is not ready yet. Wait until the status bar says Ready, then try again."
                    .to_owned()
            }
            Error::Busy => {
                "A reply, a picture or a Looks suggestion is still being made. Wait, or press Stop.".to_owned()
            }
            Error::Timeout(secs) => format!(
                "No reply after {secs} seconds. Try again, or pick a smaller model on the Models screen."
            ),
            Error::Invalid(text) => sentence(text),
            Error::Rejected { message, .. } => sentence(message),
            Error::NotFound(what) => format!("This {what} no longer exists. Go back and open it again."),
            Error::Cancelled => "Stopped.".to_owned(),
            Error::NoSpace { need } => format!(
                "Not enough space. You need {} more. Free some space, or pick a folder on another drive.",
                gigabytes(*need)
            ),
            Error::Offline(_) => {
                "No internet connection. Connect, then press Retry. The download goes on from where it stopped."
                    .to_owned()
            }
            Error::Engine(text) if engine_out_of_memory(text) => {
                "This model is too big for your graphics card. Pick a smaller model on the Models screen."
                    .to_owned()
            }
            Error::Engine(text) if model_file_damaged(text) => {
                "The model file is missing or damaged. Download it again on the Models screen."
                    .to_owned()
            }
            Error::Engine(_) | Error::Http(_) | Error::Json(_) => {
                "The chat engine did not answer. Try again.".to_owned()
            }
            Error::Damaged(_) => {
                "Your chats file is damaged. Open Unstrung in safe start and run Repair.".to_owned()
            }
            Error::KeyLost(_) => {
                "The key that protects your chats and pictures did not open. Your data is still there. Sign in to Windows as the user who made it, then start Unstrung again."
                    .to_owned()
            }
            Error::Db(_) | Error::Migrate(_) | Error::Io(_) => {
                "Unstrung could not read or save your data. Try again. If it fails again, restart Unstrung."
                    .to_owned()
            }
        };
        log::warn!("command failed: {e}");
        Self {
            message,
            detail: e.to_string(),
            tip: None,
        }
    }
}

/// Ollama's words when a model does not fit in memory.
fn engine_out_of_memory(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    [
        "out of memory",
        "requires more system memory",
        "insufficient memory",
        "cudamalloc failed",
        "unable to allocate",
    ]
    .iter()
    .any(|w| t.contains(w))
}

/// Ollama's words when a model file is gone or broken.
fn model_file_damaged(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    [
        "invalid file magic",
        "unable to load model",
        "failed to load model",
        "file does not exist",
        "no such file",
    ]
    .iter()
    .any(|w| t.contains(w))
}

/// Rounded up, so a small shortfall never shows as "0.0 GB".
pub fn gigabytes(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let gb = bytes as f64 / 1_073_741_824.0;
    format!("{:.1} GB", (gb * 10.0).ceil() / 10.0)
}

fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

pub type CmdResult<T> = Result<T, UiError>;

/// Events for one reply, sent through a channel in order.
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReplyEvent {
    UserSaved {
        message: Message,
    },
    Text {
        text: String,
    },
    /// The guard rejected the text so far. The UI clears it and waits for new text.
    Restart,
    Done {
        view: Box<ChatView>,
        hint: Option<&'static str>,
    },
    Failed {
        error: UiError,
    },
}

pub fn emit<T: tauri::ipc::IpcResponse>(channel: &Channel<T>, event: T) {
    if let Err(e) = channel.send(event) {
        log::warn!("could not send a reply event to the window: {e}");
    }
}

pub fn progress(channel: &Channel<ReplyEvent>) -> impl FnMut(Progress<'_>) + '_ {
    move |p| match p {
        Progress::Text(text) => emit(
            channel,
            ReplyEvent::Text {
                text: text.to_owned(),
            },
        ),
        Progress::Restart => emit(channel, ReplyEvent::Restart),
    }
}

fn user_saved(channel: &Channel<ReplyEvent>) -> impl FnOnce(&Message) + '_ {
    move |message| {
        emit(
            channel,
            ReplyEvent::UserSaved {
                message: message.clone(),
            },
        );
    }
}

/// Sends the end of a reply. The view is sent after an error too, because a
/// user message can be saved before the reply fails.
async fn finish(
    state: &AppState,
    chat_id: i64,
    channel: &Channel<ReplyEvent>,
    result: unstrung_core::Result<ReplyResult>,
) {
    if result.is_ok() {
        // The memory pass runs after the reply, while the user reads it.
        let chats = state.chats.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = chats.update_memory(chat_id).await {
                log::warn!("memory pass failed: {e}");
            }
        });
    }
    let event = match result {
        Ok(reply) => match state.chats.view(chat_id).await {
            Ok(view) => ReplyEvent::Done {
                view: Box::new(view),
                hint: reply.hint,
            },
            Err(e) => ReplyEvent::Failed { error: e.into() },
        },
        Err(e) => ReplyEvent::Failed { error: e.into() },
    };
    emit(channel, event);
}

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> EngineStatus {
    state.engine.status()
}

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
#[tauri::command]
pub async fn open_backup(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> CmdResult<Option<unstrung_core::backup::BackupInfo>> {
    let Some(file) = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .add_filter("Zip", &["zip"])
        .pick_file()
        .await
    else {
        return Ok(None);
    };
    Ok(Some(
        unstrung_core::backup::open(&state.paths, file.path()).await?,
    ))
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
const RECENT_CHATS: i64 = 100;

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
pub async fn send_message(
    state: State<'_, AppState>,
    chat_id: i64,
    text: String,
    on_event: Channel<ReplyEvent>,
) -> CmdResult<()> {
    let result = state
        .chats
        .send(chat_id, &text, user_saved(&on_event), progress(&on_event))
        .await;
    finish(&state, chat_id, &on_event, result).await;
    Ok(())
}

/// A user message gets a new version and a new reply.
#[tauri::command]
pub async fn edit_message(
    state: State<'_, AppState>,
    chat_id: i64,
    message_id: i64,
    text: String,
    on_event: Channel<ReplyEvent>,
) -> CmdResult<()> {
    let result = state
        .chats
        .edit_and_resend(
            chat_id,
            message_id,
            &text,
            user_saved(&on_event),
            progress(&on_event),
        )
        .await;
    finish(&state, chat_id, &on_event, result).await;
    Ok(())
}

/// A reply is fixed by hand, in place.
#[tauri::command]
pub async fn edit_reply(
    state: State<'_, AppState>,
    chat_id: i64,
    message_id: i64,
    text: String,
) -> CmdResult<ChatView> {
    Ok(state
        .chats
        .edit_reply_text(chat_id, message_id, &text)
        .await?)
}

#[tauri::command]
pub async fn regenerate(
    state: State<'_, AppState>,
    chat_id: i64,
    message_id: i64,
    mode: RegenMode,
    on_event: Channel<ReplyEvent>,
) -> CmdResult<()> {
    let result = state
        .chats
        .regenerate(chat_id, message_id, mode, progress(&on_event))
        .await;
    finish(&state, chat_id, &on_event, result).await;
    Ok(())
}

#[tauri::command]
pub async fn continue_reply(
    state: State<'_, AppState>,
    chat_id: i64,
    on_event: Channel<ReplyEvent>,
) -> CmdResult<()> {
    let result = state
        .chats
        .continue_reply(chat_id, progress(&on_event))
        .await;
    finish(&state, chat_id, &on_event, result).await;
    Ok(())
}

#[tauri::command]
pub async fn retry_reply(
    state: State<'_, AppState>,
    chat_id: i64,
    on_event: Channel<ReplyEvent>,
) -> CmdResult<()> {
    let result = state.chats.retry(chat_id, progress(&on_event)).await;
    finish(&state, chat_id, &on_event, result).await;
    Ok(())
}

#[tauri::command]
pub fn stop_reply(state: State<'_, AppState>) {
    state.chats.stop();
}

/// "Rewind to here": later messages stay as a hidden branch.
#[tauri::command]
pub async fn rewind(
    state: State<'_, AppState>,
    chat_id: i64,
    message_id: i64,
) -> CmdResult<ChatView> {
    Ok(state.chats.set_head(chat_id, message_id).await?)
}

/// Shows another version of a message.
#[tauri::command]
pub async fn show_branch(
    state: State<'_, AppState>,
    chat_id: i64,
    message_id: i64,
) -> CmdResult<ChatView> {
    Ok(state.chats.show_branch(chat_id, message_id).await?)
}

#[derive(Serialize)]
pub struct Deleted {
    view: ChatView,
    undo: DeleteUndo,
}

#[tauri::command]
pub async fn delete_message(
    state: State<'_, AppState>,
    chat_id: i64,
    message_id: i64,
) -> CmdResult<Deleted> {
    let (view, undo) = state.chats.delete_message(chat_id, message_id).await?;
    Ok(Deleted { view, undo })
}

#[tauri::command]
pub async fn undo_delete(
    state: State<'_, AppState>,
    chat_id: i64,
    undo: DeleteUndo,
) -> CmdResult<ChatView> {
    Ok(state.chats.undo_delete(chat_id, &undo).await?)
}

/// Copies the chat up to a message into a new chat.
#[tauri::command]
pub async fn branch_chat(
    state: State<'_, AppState>,
    chat_id: i64,
    message_id: i64,
) -> CmdResult<ChatView> {
    Ok(state.chats.branch(chat_id, message_id).await?)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Tauri rejects a command that has no permission ("not allowed by
    /// ACL"), and the core tests never go through Tauri. So every command
    /// the app registers must be in build.rs and in the capability file.
    #[test]
    fn every_registered_command_has_a_permission() {
        let main = include_str!("main.rs");
        let start = main.find("generate_handler![").unwrap();
        let block = &main[start..start + main[start..].find("])").unwrap()];
        let registered: std::collections::BTreeSet<&str> = block
            .split(',')
            .filter_map(|item| item.trim().rsplit("::").next())
            .filter(|name| !name.is_empty() && !name.contains('['))
            .collect();
        let build = include_str!("../build.rs");
        let list = &build[build.find("COMMANDS").unwrap()..build.find("];").unwrap()];
        let listed: std::collections::BTreeSet<&str> = list.split('"').skip(1).step_by(2).collect();
        assert_eq!(registered, listed, "build.rs COMMANDS");
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let allowed: std::collections::BTreeSet<String> = capability["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p.as_str()?.strip_prefix("allow-"))
            .map(|p| p.replace('-', "_"))
            .collect();
        for name in &registered {
            assert!(allowed.contains(*name), "no allow-{name} in default.json");
        }
    }

    #[test]
    fn a_locked_file_gets_the_antivirus_tip_the_second_time() {
        let locked = || Error::Io(std::io::Error::from_raw_os_error(32));
        assert!(file_in_use(&locked()));
        assert!(file_in_use(&Error::Engine(
            "open blob: The process cannot access the file because it is being used by another process.".into()
        )));
        assert!(!file_in_use(&Error::Io(std::io::Error::from_raw_os_error(
            5
        ))));
        let _ = UiError::from(locked());
        let second = UiError::from(locked());
        assert_eq!(second.tip, Some("antivirus"));
        assert!(UiError::from(Error::Busy).tip.is_none());
    }

    #[test]
    fn rejected_file_keeps_the_reason_in_details() {
        let e = UiError::from(Error::Rejected {
            message: "the picture could not be read. Use a PNG, JPEG or WebP picture".into(),
            why: "unexpected end of file".into(),
        });
        assert_eq!(
            e.message,
            "The picture could not be read. Use a PNG, JPEG or WebP picture."
        );
        assert_eq!(e.detail, "unexpected end of file");
        let e = UiError::from(Error::Timeout(60));
        assert!(
            e.message
                .starts_with("No reply after 60 seconds. Try again")
        );
    }
}

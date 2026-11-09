//! Commands that write or change replies, and the events they stream to the page.

use super::*;

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

pub(super) fn user_saved(channel: &Channel<ReplyEvent>) -> impl FnOnce(&Message) + '_ {
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
pub(super) async fn finish(
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

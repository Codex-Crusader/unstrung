//! Commands for the character creator, personas and profile pictures.

use crate::commands::{CmdResult, UiError, emit as emit_test};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::{Channel, InvokeBody, Request};
use unstrung_core::Error;
use unstrung_core::avatar::{self, AvatarChange, Staged};
use unstrung_core::chat::{Progress, clean_input};
use unstrung_core::db::{Character, CharacterFields, Message, Persona, PersonaFields};
use unstrung_core::{library, starters};

/// Longest test chat, in messages.
const MAX_TEST_LINES: usize = 24;

#[tauri::command]
pub async fn get_character(state: State<'_, AppState>, character_id: i64) -> CmdResult<Character> {
    Ok(state.chats.db().character(character_id).await?)
}

#[tauri::command]
pub async fn save_character(
    state: State<'_, AppState>,
    character_id: Option<i64>,
    fields: CharacterFields,
    avatar: AvatarChange,
) -> CmdResult<Character> {
    Ok(library::save_character(
        state.chats.db(),
        &state.paths,
        character_id,
        &fields,
        &avatar,
    )
    .await?)
}

#[tauri::command]
pub async fn delete_character(state: State<'_, AppState>, character_id: i64) -> CmdResult<()> {
    Ok(library::delete_character(state.chats.db(), &state.paths, character_id).await?)
}

/// Adds back the starter characters and personas that the user deleted.
#[tauri::command]
pub async fn restore_starters(state: State<'_, AppState>) -> CmdResult<()> {
    Ok(starters::restore(state.chats.db(), &state.paths).await?)
}

#[tauri::command]
pub async fn save_persona(
    state: State<'_, AppState>,
    persona_id: Option<i64>,
    fields: PersonaFields,
    avatar: AvatarChange,
) -> CmdResult<Persona> {
    Ok(library::save_persona(state.chats.db(), &state.paths, persona_id, &fields, &avatar).await?)
}

#[tauri::command]
pub async fn delete_persona(state: State<'_, AppState>, persona_id: i64) -> CmdResult<()> {
    Ok(library::delete_persona(state.chats.db(), &state.paths, persona_id).await?)
}

/// The persona that new chats start with. `None` means no persona.
#[tauri::command]
pub async fn set_default_persona(
    state: State<'_, AppState>,
    persona_id: Option<i64>,
) -> CmdResult<()> {
    if let Some(id) = persona_id {
        state.chats.db().persona(id).await?;
    }
    Ok(state.chats.db().set_default_persona(persona_id).await?)
}

/// Takes the raw bytes of a picture file. The UI then shows the staged copy
/// at `avatar.localhost/staged/<token>` and picks the crop.
#[tauri::command]
pub async fn stage_upload(state: State<'_, AppState>, request: Request<'_>) -> CmdResult<Staged> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err(Error::Invalid("send the picture as raw bytes".into()).into());
    };
    let paths = state.paths.clone();
    let bytes = bytes.clone();
    // Decoding a large picture takes time, so it runs off the async threads.
    tauri::async_runtime::spawn_blocking(move || avatar::stage(&paths, &bytes))
        .await
        .map_err(|e| UiError {
            message: "The picture could not be read.".into(),
            detail: e.to_string(),
        })?
        .map_err(UiError::from)
}

#[tauri::command]
pub async fn suggest_anchor(
    state: State<'_, AppState>,
    name: String,
    personality: String,
) -> CmdResult<String> {
    Ok(state.chats.suggest_anchor(&name, &personality).await?)
}

#[derive(Debug, Deserialize)]
pub struct TestLine {
    role: String,
    content: String,
}

/// Events for one test reply.
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TestEvent {
    Text { text: String },
    Restart,
    Done { text: String },
    Failed { error: UiError },
}

/// A reply from a character that is not saved yet. Nothing is stored.
#[tauri::command]
pub async fn test_reply(
    state: State<'_, AppState>,
    fields: CharacterFields,
    persona_id: Option<i64>,
    history: Vec<TestLine>,
    on_event: Channel<TestEvent>,
) -> CmdResult<()> {
    let result = run_test(&state, &fields, persona_id, &history, &on_event).await;
    let event = match result {
        Ok(text) => TestEvent::Done { text },
        Err(e) => TestEvent::Failed { error: e.into() },
    };
    emit_test(&on_event, event);
    Ok(())
}

async fn run_test(
    state: &AppState,
    fields: &CharacterFields,
    persona_id: Option<i64>,
    history: &[TestLine],
    channel: &Channel<TestEvent>,
) -> unstrung_core::Result<String> {
    let f = library::check_character(fields)?;
    if history.is_empty() || history.len() > MAX_TEST_LINES {
        return Err(Error::Invalid("the test chat has too many messages".into()));
    }
    let persona = match persona_id {
        Some(id) => Some(state.chats.db().persona(id).await?),
        None => None,
    };
    let character = Character {
        id: 0,
        name: f.name,
        avatar_path: None,
        tagline: f.tagline,
        personality: f.personality,
        scenario: f.scenario,
        greeting: f.greeting,
        example_dialogue: f.example_dialogue,
        voice_anchor: f.voice_anchor,
        appearance_tags: f.appearance_tags,
        default_tone: f.default_tone,
        default_reply_style: f.default_reply_style,
        model_override: None,
        age: f.age,
        is_adult: f.is_adult,
        created_at: 0,
    };
    let mut messages = Vec::with_capacity(history.len());
    for (i, line) in history.iter().enumerate() {
        let role = match line.role.as_str() {
            "user" => "user",
            "character" => "character",
            _ => return Err(Error::Invalid("unknown role in the test chat".into())),
        };
        messages.push(Message {
            id: i64::try_from(i).unwrap_or_default() + 1,
            chat_id: 0,
            parent_id: None,
            role: role.to_owned(),
            persona_id,
            content: clean_input(&line.content)?,
            flagged_out_of_character: false,
            model: None,
            created_at: 0,
        });
    }
    state
        .chats
        .test_reply(&character, persona.as_ref(), &messages, |p| match p {
            Progress::Text(text) => emit_test(
                channel,
                TestEvent::Text {
                    text: text.to_owned(),
                },
            ),
            Progress::Restart => emit_test(channel, TestEvent::Restart),
        })
        .await
}

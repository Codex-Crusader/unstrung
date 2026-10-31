//! Each app command gets its own permission, so the capability file is the full list
//! of what the window may call.

const COMMANDS: &[&str] = &[
    "start_mode",
    "start_ok",
    "start_normally",
    "diagnostic_report",
    "get_status",
    "list_characters",
    "list_personas",
    "open_character",
    "open_chat",
    "new_chat",
    "list_chats",
    "delete_chat",
    "send_message",
    "edit_message",
    "edit_reply",
    "regenerate",
    "continue_reply",
    "retry_reply",
    "stop_reply",
    "rewind",
    "show_branch",
    "delete_message",
    "undo_delete",
    "branch_chat",
    "set_chat_persona",
    "set_chat_tone",
    "chat_memories",
    "remember",
    "edit_memory",
    "set_memory_flags",
    "set_chat_style",
    "restart_engine",
    "models_page",
    "recheck_hardware",
    "download_model",
    "stop_download",
    "remove_model",
    "set_default_model",
    "set_advanced",
    "set_chat_model",
    "set_character_model",
    "get_character",
    "save_character",
    "delete_character",
    "restore_starters",
    "save_persona",
    "delete_persona",
    "set_default_persona",
    "stage_upload",
    "import_card",
    "suggest_anchor",
    "test_reply",
    "self_test_mode",
    "self_test_report",
];

fn main() {
    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    if let Err(e) = tauri_build::try_build(attributes) {
        eprintln!("tauri build step failed: {e:#}");
        std::process::exit(1);
    }
}

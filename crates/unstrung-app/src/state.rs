use std::sync::Arc;
use unstrung_core::chat::ChatService;
use unstrung_core::engine::Engine;
use unstrung_core::paths::AppPaths;

pub struct AppState {
    pub paths: AppPaths,
    pub engine: Arc<Engine>,
    pub chats: Arc<ChatService>,
    pub default_character: i64,
    pub self_test: bool,
}

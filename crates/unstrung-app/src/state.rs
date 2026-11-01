use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use unstrung_core::chat::ChatService;
use unstrung_core::engine::Engine;
use unstrung_core::engine_setup::EngineSetup;
use unstrung_core::models::ModelService;
use unstrung_core::paths::AppPaths;
use unstrung_core::pictures::service::PictureService;

pub struct AppState {
    pub paths: AppPaths,
    pub engine: Arc<Engine>,
    pub chats: Arc<ChatService>,
    pub models: Arc<ModelService>,
    pub pictures: Arc<PictureService>,
    pub setup: Arc<EngineSetup>,
    /// Windows notifications when a long job ends in the background.
    pub notify: AtomicBool,
}

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
    pub files: FileGuard,
}

/// Held by a backup export, and by every task that deletes or moves gallery
/// files: during an export they would remove a picture that the zip still
/// has to read. Replies and pictures do not need it, so they go on.
#[derive(Default)]
pub struct FileGuard(AtomicBool);

pub struct FileHold<'a>(&'a AtomicBool);

impl Drop for FileHold<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl FileGuard {
    pub fn take(&self) -> unstrung_core::Result<FileHold<'_>> {
        self.0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| FileHold(&self.0))
            .map_err(|_| unstrung_core::error::Error::Invalid("Wait until the backup or the folder move ends.".into()))
    }
}

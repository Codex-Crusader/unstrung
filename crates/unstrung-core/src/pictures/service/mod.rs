//! The picture service: setup (engine and model downloads), the model
//! choice, and one picture from the chat to the gallery.
//!
//! A picture takes the chat's one GPU slot, so a reply and a picture never
//! run together, and Stop ends either. On the Basic and Standard tiers the
//! chat model leaves the graphics card before the picture and comes back
//! after it, so the next reply is fast again.

use super::catalog::{self, ImageModel};
use super::{runner, safety, scene};
use crate::chat::ChatService;
use crate::db::{Image, NewImage};
use crate::download::{Fetcher, Source};
use crate::engine::KEEP_ALIVE;
use crate::engine_setup::{self, Engine, EngineSetup, Step};
use crate::error::{Error, Result};
use crate::hardware::{Gpu, Hardware, Tier};
use crate::models::ModelService;
use crate::paths::AppPaths;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

mod download;
mod paint;
mod render;

pub use paint::*;
use render::*;

/// The chosen image model, in `meta`.
const META_MODEL: &str = "image_model";
const GPU_LIMIT: Duration = Duration::from_secs(10 * 60);
const CPU_LIMIT: Duration = Duration::from_secs(30 * 60);
/// How long the chat model may take to leave the graphics card.
const UNLOAD_WAIT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize)]
pub struct ModelView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub size_bytes: u64,
    /// Bytes still to download (parts that another model shares are on disk).
    pub missing_bytes: u64,
    pub installed: bool,
    pub fits: bool,
    pub recommended: bool,
    pub selected: bool,
    pub mature_ok: bool,
    pub license_name: String,
    pub license_url: String,
    pub tested: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Page {
    pub tier: Tier,
    /// No usable graphics card: pictures work, but slowly.
    pub cpu_only: bool,
    pub engine_installed: bool,
    pub engine_download_bytes: u64,
    pub models: Vec<ModelView>,
    pub selected: Option<String>,
    pub download: Option<Download>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadState {
    Running,
    Done,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Download {
    pub model: String,
    /// "engine" or "model".
    pub part: &'static str,
    pub state: DownloadState,
    pub completed: u64,
    pub total: u64,
    pub bytes_per_second: u64,
    pub error: String,
}

/// What the chat shows while a picture is made.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Painting {
    pub chat_id: i64,
    pub message_id: Option<i64>,
    /// "scene", "loading", "painting" or "finishing".
    pub phase: &'static str,
    pub step: u32,
    pub total: u32,
}

/// The download slot. It is free again when this drops, unless `keep` hands
/// it to a background task that frees it itself.
struct Slot<'a>(&'a Mutex<Option<CancellationToken>>, CancellationToken);

impl Slot<'_> {
    fn keep(self) -> CancellationToken {
        let token = self.1.clone();
        std::mem::forget(self);
        token
    }
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        if let Ok(mut running) = self.0.lock() {
            *running = None;
        }
    }
}

pub struct PictureService {
    paths: AppPaths,
    chats: Arc<ChatService>,
    models: Arc<ModelService>,
    setup: Arc<EngineSetup>,
    running: Mutex<Option<CancellationToken>>,
    download: watch::Sender<Option<Download>>,
    painting: watch::Sender<Option<Painting>>,
}

impl PictureService {
    pub fn new(paths: AppPaths, chats: Arc<ChatService>, models: Arc<ModelService>) -> Arc<Self> {
        Arc::new(Self {
            setup: EngineSetup::new(paths.clone(), Engine::Image),
            paths,
            chats,
            models,
            running: Mutex::new(None),
            download: watch::channel(None).0,
            painting: watch::channel(None).0,
        })
    }

    pub fn subscribe_download(&self) -> watch::Receiver<Option<Download>> {
        self.download.subscribe()
    }

    pub fn subscribe_painting(&self) -> watch::Receiver<Option<Painting>> {
        self.painting.subscribe()
    }

    fn hardware(&self) -> Hardware {
        self.models.hardware()
    }

    /// The graphics card for the image engine. The Basic tier has none that
    /// is usable, so it gets the CPU build.
    fn usable_gpu(hw: &Hardware) -> Option<Gpu> {
        (hw.tier != Tier::Basic).then(|| hw.gpu.clone()).flatten()
    }

    async fn selected(&self) -> Result<Option<&'static ImageModel>> {
        Ok(self
            .chats
            .db()
            .meta(META_MODEL)
            .await?
            .and_then(|id| catalog::model(&id)))
    }

    pub async fn page(&self) -> Result<Page> {
        let hw = self.hardware();
        let gpu = Self::usable_gpu(&hw);
        let dir = self.paths.image_models();
        let selected = self.selected().await?.map(|m| m.id.clone());
        let recommended = catalog::recommended(hw.tier).map(|m| m.id.as_str());
        let models = catalog::catalog()
            .models
            .iter()
            .map(|m| ModelView {
                id: m.id.clone(),
                name: m.name.clone(),
                description: m.description.clone(),
                size_bytes: m.size_bytes(),
                missing_bytes: m.missing_bytes(&dir),
                installed: m.installed(&dir),
                fits: m.fits(hw.tier),
                recommended: recommended == Some(m.id.as_str()),
                selected: selected.as_deref() == Some(m.id.as_str()),
                mature_ok: m.mature_ok,
                license_name: m.license.name.clone(),
                license_url: m.license.url.clone(),
                tested: !m.tested.is_empty(),
            })
            .collect();
        Ok(Page {
            tier: hw.tier,
            cpu_only: gpu.is_none(),
            engine_installed: engine_setup::installed_version(&self.paths, Engine::Image).is_some(),
            engine_download_bytes: engine_setup::download_bytes(Engine::Image, gpu.as_ref()),
            models,
            selected,
            download: self.download.borrow().clone(),
        })
    }

    /// Picks an installed model for pictures.
    pub async fn select(&self, id: &str) -> Result<()> {
        let model = catalog::model(id).ok_or(Error::NotFound("image model"))?;
        if !model.installed(&self.paths.image_models()) {
            return Err(Error::Invalid(format!("{} is not downloaded", model.name)));
        }
        self.chats.db().set_meta(META_MODEL, id).await
    }

    /// Deletes a model's files, except parts that another installed model uses.
    pub async fn remove(&self, id: &str) -> Result<()> {
        let model = catalog::model(id).ok_or(Error::NotFound("image model"))?;
        let _slot = self.take_slot()?;
        let dir = self.paths.image_models();
        for c in catalog::files_to_remove(model, &dir) {
            match std::fs::remove_file(catalog::path_of(&dir, c)) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
        if self.selected().await?.is_some_and(|m| m.id == id) {
            self.chats.db().set_meta(META_MODEL, "").await?;
        }
        log::info!("image model {id} removed");
        Ok(())
    }

    /// Takes the download slot. Downloads and removals wait for a picture,
    /// because the engine has the model files and its folder open.
    fn take_slot(&self) -> Result<Slot<'_>> {
        let mut running = self.running.lock().map_err(|_| Error::Busy)?;
        if running.is_some() {
            return Err(Error::Invalid(
                "Wait until the picture download ends.".into(),
            ));
        }
        if self.painting.borrow().is_some() {
            return Err(Error::Invalid("Wait until the picture is finished.".into()));
        }
        let token = CancellationToken::new();
        *running = Some(token.clone());
        Ok(Slot(&self.running, token))
    }

    /// Keeps downloads and removals out while the models folder moves.
    pub fn hold(&self) -> Result<impl Sized + '_> {
        self.take_slot()
    }

    /// The file of a gallery picture.
    pub async fn file(&self, id: i64) -> Result<std::path::PathBuf> {
        let image = self.chats.db().image(id).await?;
        crate::paths::join_safe(&self.paths.gallery(), &image.file_path)
    }

    /// Deletes a picture and its file.
    pub async fn delete(&self, id: i64) -> Result<()> {
        let path = self.chats.db().delete_image(id).await?;
        let file = crate::paths::join_safe(&self.paths.gallery(), &path)?;
        match std::fs::remove_file(file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }
}

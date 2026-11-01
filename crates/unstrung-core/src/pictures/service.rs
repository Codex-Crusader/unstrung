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
    /// "scene", "loading" or "painting".
    pub phase: &'static str,
    pub step: u32,
    pub total: u32,
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
        if self.running.lock().map_err(|_| Error::Busy)?.is_some() {
            return Err(Error::Busy);
        }
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

    pub fn stop_download(&self) {
        if let Ok(running) = self.running.lock()
            && let Some(token) = running.as_ref()
        {
            token.cancel();
        }
        self.setup.cancel();
    }

    fn report(&self, d: Download) {
        self.download.send_replace(Some(d));
    }

    /// Starts the downloads for `id` in the background: the engine if it is
    /// missing, then the model parts that are not on disk. Progress goes to
    /// `subscribe_download`. The model becomes the chosen one at the end.
    pub fn download(self: &Arc<Self>, id: &str) -> Result<()> {
        let model = catalog::model(id).ok_or(Error::NotFound("image model"))?;
        let token = CancellationToken::new();
        {
            let mut running = self.running.lock().map_err(|_| Error::Busy)?;
            if running.is_some() {
                return Err(Error::Busy);
            }
            *running = Some(token.clone());
        }
        let this = self.clone();
        tokio::spawn(async move {
            let result = this.download_inner(model, &token).await;
            if let Ok(mut running) = this.running.lock() {
                *running = None;
            }
            let last = this.download.borrow().clone();
            let base = last.unwrap_or(Download {
                model: model.id.clone(),
                part: "model",
                state: DownloadState::Running,
                completed: 0,
                total: 0,
                bytes_per_second: 0,
                error: String::new(),
            });
            let (state, error) = match &result {
                Ok(()) => (DownloadState::Done, String::new()),
                Err(Error::Cancelled) => (DownloadState::Stopped, String::new()),
                Err(e) => {
                    log::error!("image download failed: {e}");
                    (DownloadState::Failed, e.to_string())
                }
            };
            this.report(Download {
                state,
                error,
                bytes_per_second: 0,
                ..base
            });
        });
        Ok(())
    }

    async fn download_inner(
        &self,
        model: &'static ImageModel,
        token: &CancellationToken,
    ) -> Result<()> {
        let hw = self.hardware();
        let gpu = Self::usable_gpu(&hw);
        let dir = self.paths.image_models();
        std::fs::create_dir_all(&dir)?;
        let need = model.missing_bytes(&dir)
            + if engine_setup::installed_version(&self.paths, Engine::Image).is_some() {
                0
            } else {
                engine_setup::disk_need(Engine::Image, gpu.as_ref())
            }
            + crate::location::MARGIN_BYTES;
        let free = crate::hardware::disk_of(&dir).map_or(0, |d| d.available);
        if free < need {
            return Err(Error::NoSpace { need: need - free });
        }

        if engine_setup::installed_version(&self.paths, Engine::Image).as_deref()
            != Some(engine_setup::release(Engine::Image).version.as_str())
        {
            self.install_engine(model, gpu.as_ref(), token).await?;
        }

        let fetcher = Fetcher::new(&catalog::catalog().hosts)?;
        let missing: Vec<_> = model
            .components()
            .into_iter()
            .map(|(_, c)| c)
            .filter(|c| !catalog::on_disk(&dir, c))
            .collect();
        let total: u64 = missing.iter().map(|c| c.size).sum();
        let started = Instant::now();
        let mut before = 0;
        let mut first: Option<u64> = None;
        for c in missing {
            let source = Source {
                url: c.url.clone(),
                size: c.size,
                sha256: c.sha256.clone(),
            };
            let mut progress = |done: u64| {
                let now = before + done;
                let start = *first.get_or_insert(now);
                let secs = started.elapsed().as_secs().max(1);
                self.report(Download {
                    model: model.id.clone(),
                    part: "model",
                    state: DownloadState::Running,
                    completed: now,
                    total,
                    bytes_per_second: (now - start) / secs,
                    error: String::new(),
                });
            };
            fetcher
                .fetch(&source, &catalog::path_of(&dir, c), token, &mut progress)
                .await?;
            before += c.size;
        }
        self.chats.db().set_meta(META_MODEL, &model.id).await?;
        log::info!("image model {} ready", model.id);
        Ok(())
    }

    async fn install_engine(
        &self,
        model: &ImageModel,
        gpu: Option<&Gpu>,
        token: &CancellationToken,
    ) -> Result<()> {
        let mut rx = self.setup.subscribe();
        let id = model.id.clone();
        let sender = self.download.clone();
        let forward = tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                let Some(p) = rx.borrow_and_update().clone() else {
                    continue;
                };
                if matches!(p.step, Step::Downloading | Step::Unpacking | Step::Checking) {
                    sender.send_replace(Some(Download {
                        model: id.clone(),
                        part: "engine",
                        state: DownloadState::Running,
                        completed: p.completed,
                        total: p.total,
                        bytes_per_second: p.bytes_per_second,
                        error: String::new(),
                    }));
                }
            }
        });
        let result = tokio::select! {
            r = self.setup.prepare(gpu) => r,
            () = token.cancelled() => {
                self.setup.cancel();
                Err(Error::Cancelled)
            }
        };
        forward.abort();
        result?;
        // No picture runs during a download, so nothing holds the old folder.
        self.setup.replace()
    }

    /// Makes one picture for a chat. `message_id` is the message to picture
    /// (the latest when `None`), `wish` is the `/imagine` text.
    pub async fn paint(&self, chat_id: i64, message_id: Option<i64>, wish: &str) -> Result<Image> {
        let model = self.selected().await?.ok_or(Error::NotReady)?;
        let dir = self.paths.image_models();
        let exe = engine_setup::folder(&self.paths, Engine::Image).join(Engine::Image.exe());
        if !model.installed(&dir)
            || engine_setup::installed_version(&self.paths, Engine::Image).is_none()
        {
            return Err(Error::NotReady);
        }
        let (token, _guard) = self.chats.begin_job()?;
        let result = self
            .paint_inner(model, &exe, chat_id, message_id, wish, &token)
            .await;
        self.painting.send_replace(None);
        result
    }

    async fn paint_inner(
        &self,
        model: &'static ImageModel,
        exe: &std::path::Path,
        chat_id: i64,
        message_id: Option<i64>,
        wish: &str,
        token: &CancellationToken,
    ) -> Result<Image> {
        let show = |message_id: Option<i64>, phase: &'static str, step: u32, total: u32| {
            self.painting.send_replace(Some(Painting {
                chat_id,
                message_id,
                phase,
                step,
                total,
            }));
        };
        show(message_id, "scene", 0, 0);
        let input = self
            .chats
            .picture_scene(chat_id, message_id, wish, token)
            .await?;
        let message_id = input.message_id;

        let character = scene::Person {
            appearance: &input.character.appearance_tags,
        };
        let persona = input.persona.as_ref().map(|p| scene::Person {
            appearance: &p.appearance_tags,
        });
        let positive = scene::positive(
            &input.scene,
            &input.character.art_style,
            &character,
            persona.as_ref(),
        );
        let people = safety::People {
            character_age: input.character.age,
            persona_age: input.persona.as_ref().and_then(|p| p.age),
        };
        // The adult setting comes in Phase 7. Until then pictures are made with it off.
        let checked = format!("{positive}, {wish}");
        let rules =
            safety::check(&checked, people, false, input.scene.sexual).map_err(|block| {
                log::info!("a picture was blocked: {block:?}");
                Error::Rejected {
                    message: blocked_message(block).into(),
                    why: format!("{block:?}"),
                }
            })?;
        let prompts = scene::with_rules(&positive, rules);

        let hw = self.hardware();
        let on_gpu = Self::usable_gpu(&hw).is_some();
        let size = if on_gpu { 1024 } else { 512 };
        let seed = runner::new_seed();
        let folder = self.paths.gallery().join(chat_id.to_string());
        std::fs::create_dir_all(&folder)?;
        let file = format!("{}-{seed}.png", crate::db::now_ms());
        let output = folder.join(&file);

        show(message_id, "loading", 0, 0);
        let client = self.chats.engine().client().ok();
        let freed = if hw.tier == Tier::High {
            false
        } else if let Some(client) = &client {
            free_gpu(client, &input.chat_model).await
        } else {
            false
        };

        let job = runner::Job {
            model,
            models_dir: &self.paths.image_models(),
            prompts: &prompts,
            width: size,
            height: size,
            seed,
            output: &output,
        };
        let mut cmd = std::process::Command::new(exe);
        cmd.args(runner::args(&job));
        let log = self.paths.logs().join("sdcpp.log");
        let limit = if on_gpu { GPU_LIMIT } else { CPU_LIMIT };
        let run_token = token.clone();
        let sender = self.painting.clone();
        let out = output.clone();
        let started = Instant::now();
        let ran = tokio::task::spawn_blocking(move || {
            runner::run(cmd, &out, &log, limit, &run_token, &|step, total| {
                sender.send_replace(Some(Painting {
                    chat_id,
                    message_id,
                    phase: "painting",
                    step,
                    total,
                }));
            })
        })
        .await
        .map_err(|e| Error::Engine(format!("the picture stopped: {e}")))?;

        // The chat model comes back whether the picture worked or not.
        if freed && let Some(client) = client {
            let model = input.chat_model.clone();
            tokio::spawn(async move {
                if let Err(e) = client.load(&model, KEEP_ALIVE).await {
                    log::warn!("the chat model did not load again after a picture: {e}");
                }
            });
        }
        ran?;
        log::info!(
            "picture made in {} s with {} ({}x{})",
            started.elapsed().as_secs(),
            model.id,
            size,
            size
        );
        let id = self
            .chats
            .db()
            .insert_image(&NewImage {
                chat_id,
                message_id,
                file_path: format!("{chat_id}/{file}"),
                prompt: prompts.positive,
                negative_prompt: prompts.negative,
                seed,
                image_model: model.id.clone(),
                ref_mode: "tags".into(),
                width: i64::from(size),
                height: i64::from(size),
                adult: false,
            })
            .await?;
        self.chats.db().image(id).await
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

/// The sentence the user sees for a blocked picture.
pub fn blocked_message(block: safety::Block) -> &'static str {
    match block {
        safety::Block::MinorWords | safety::Block::MinorInChat => {
            "This picture was not made. Unstrung never makes sexual pictures with anyone under 18, or with anyone whose age is not set."
        }
        safety::Block::AdultOff => "This picture was not made, because adult pictures are off.",
    }
}

/// Unloads the chat model and waits until the engine no longer lists it.
/// Returns true when it was loaded, so the caller loads it again later.
async fn free_gpu(client: &crate::ollama::Client, model: &str) -> bool {
    let loaded = client
        .loaded()
        .await
        .is_ok_and(|l| l.iter().any(|m| m.name == model));
    if !loaded {
        return false;
    }
    if let Err(e) = client.unload(model).await {
        log::warn!("could not unload the chat model before a picture: {e}");
        return false;
    }
    let started = Instant::now();
    while started.elapsed() < UNLOAD_WAIT {
        match client.loaded().await {
            Ok(l) if l.iter().all(|m| m.name != model) => break,
            _ => tokio::time::sleep(Duration::from_millis(250)).await,
        }
    }
    true
}

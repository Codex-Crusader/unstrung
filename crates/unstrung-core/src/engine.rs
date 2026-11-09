//! Keeps the chat engine running and reports its state.

use crate::catalog::{self, Advanced, Tuning};
use crate::error::{Error, Result};
use crate::ollama::{Client, Installed};
use crate::paths::AppPaths;
use crate::sidecar::{self, Sidecar};
use serde::Serialize;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::{Mutex, watch};

/// The chat model when the user has not picked one and the catalog has no
/// recommendation that is installed.
pub const DEFAULT_CHAT_MODEL: &str = "gemma4:e4b-it-qat";
/// How long the engine keeps the model in memory after the last reply.
pub const KEEP_ALIVE: &str = "30m";

const MAX_RESTARTS: u32 = 3;
// The engine answers only after it has looked for GPUs. On a CPU-only test PC
// that took 27 to 33 seconds on every start, so 30 was too short.
const READY_TIMEOUT: Duration = Duration::from_secs(120);
const HEALTH_INTERVAL: Duration = Duration::from_secs(3);
/// An engine that runs this long after a restart is healthy again, so the
/// restart count starts over: crashes weeks apart are not a crash loop.
const STABLE_AFTER: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum EngineStatus {
    Starting,
    LoadingModel {
        model: String,
    },
    Ready {
        model: String,
    },
    /// No engine in the app folder yet. First run or Models offer the download.
    NotInstalled,
    /// The engine runs, but the default chat model is not installed.
    /// Downloads work in this state.
    NoModel {
        model: String,
    },
    Error {
        message: String,
        detail: String,
    },
}

pub struct Engine {
    paths: AppPaths,
    /// The global default chat model.
    model: RwLock<String>,
    advanced: RwLock<Advanced>,
    /// The names from the last model list, so a reply can skip a removed model.
    installed: RwLock<Vec<String>>,
    /// Installed models outside the catalog that can think. They get
    /// `think: false` by default, or the thinking uses up the reply cap.
    thinkers: RwLock<Vec<String>>,
    sidecar: Mutex<Option<Sidecar>>,
    client: watch::Sender<Option<Client>>,
    status: watch::Sender<EngineStatus>,
    restarts: Mutex<u32>,
}

impl Engine {
    pub fn new(paths: AppPaths) -> Arc<Self> {
        Self::with_model(paths, DEFAULT_CHAT_MODEL, Advanced::default())
    }

    pub fn with_model(paths: AppPaths, model: &str, advanced: Advanced) -> Arc<Self> {
        Arc::new(Self {
            paths,
            model: RwLock::new(model.to_owned()),
            advanced: RwLock::new(advanced),
            installed: RwLock::new(Vec::new()),
            thinkers: RwLock::new(Vec::new()),
            sidecar: Mutex::new(None),
            client: watch::channel(None).0,
            status: watch::channel(EngineStatus::Starting).0,
            restarts: Mutex::new(0),
        })
    }

    pub fn model(&self) -> String {
        self.model.read().map(|m| m.clone()).unwrap_or_default()
    }

    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }

    pub fn advanced(&self) -> Advanced {
        self.advanced.read().map(|a| a.clone()).unwrap_or_default()
    }

    pub fn set_advanced(&self, advanced: Advanced) {
        if let Ok(mut slot) = self.advanced.write() {
            *slot = advanced;
        }
    }

    /// The sampler and prompt settings for one model: the catalog entry, then
    /// the user's advanced settings on top.
    pub fn tuning_for(&self, model: &str) -> Tuning {
        let mut tuning = catalog::tuning(model);
        if catalog::entry(model).is_none() && self.can_think(model) {
            tuning.think = Some(false);
        }
        tuning.with_advanced(&self.advanced())
    }

    /// True for a model that can think: from the catalog, or from the engine
    /// for a model outside it.
    pub fn can_think(&self, model: &str) -> bool {
        match catalog::entry(model) {
            Some(entry) => entry.tuning.think.is_some(),
            None => self
                .thinkers
                .read()
                .is_ok_and(|list| list.iter().any(|m| catalog::same_tag(m, model))),
        }
    }

    /// Asks the engine which models outside the catalog can think.
    async fn learn_thinkers(&self, client: &Client, list: &[Installed]) {
        let mut found = Vec::new();
        for m in list.iter().filter(|m| catalog::entry(&m.name).is_none()) {
            match client.info(&m.name).await {
                Ok(info) if info.can_think() => found.push(m.name.clone()),
                Ok(_) => {}
                Err(e) => log::warn!("could not read the capabilities of {}: {e}", m.name),
            }
        }
        if let Ok(mut slot) = self.thinkers.write() {
            *slot = found;
        }
    }

    /// Makes `model` the default and loads it. The caller checks that it is installed.
    pub async fn set_model(&self, model: &str) -> Result<()> {
        let client = self.running_client()?;
        if let Ok(mut slot) = self.model.write() {
            model.clone_into(&mut slot);
        }
        self.status.send_replace(EngineStatus::LoadingModel {
            model: model.to_owned(),
        });
        if let Err(e) = client
            .load(model, KEEP_ALIVE, self.tuning_for(model).context)
            .await
        {
            self.set_error(&e);
            return Err(e);
        }
        self.status.send_replace(EngineStatus::Ready {
            model: model.to_owned(),
        });
        Ok(())
    }

    pub fn is_installed(&self, model: &str) -> bool {
        self.installed
            .read()
            .is_ok_and(|list| list.iter().any(|m| catalog::same_tag(m, model)))
    }

    /// Reads the model list from the engine and keeps the names.
    pub async fn refresh_installed(&self) -> Result<Vec<Installed>> {
        let client = self.running_client()?;
        let list = client.installed().await?;
        self.store_installed(&list);
        self.learn_thinkers(&client, &list).await;
        Ok(list)
    }

    fn store_installed(&self, list: &[Installed]) {
        if let Ok(mut slot) = self.installed.write() {
            *slot = list.iter().map(|m| m.name.clone()).collect();
        }
    }

    /// After a download or a removal: if there was no model, try the default again.
    pub async fn model_list_changed(&self) {
        let _ = self.refresh_installed().await;
        if matches!(self.status(), EngineStatus::NoModel { .. }) {
            let model = self.model();
            if let Ok(client) = self.running_client()
                && client.has_model(&model).await.unwrap_or(false)
            {
                let _ = self.set_model(&model).await;
            }
        }
    }

    pub fn status(&self) -> EngineStatus {
        self.status.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<EngineStatus> {
        self.status.subscribe()
    }

    /// The client, only when the engine is ready.
    pub fn client(&self) -> Result<Client> {
        match (&*self.status.borrow(), &*self.client.borrow()) {
            (EngineStatus::Ready { .. }, Some(client)) => Ok(client.clone()),
            _ => Err(Error::NotReady),
        }
    }

    /// The client whenever the engine process runs, also with no model.
    /// Downloads and the model list use it.
    pub fn running_client(&self) -> Result<Client> {
        match (&*self.status.borrow(), &*self.client.borrow()) {
            (
                EngineStatus::Ready { .. }
                | EngineStatus::NoModel { .. }
                | EngineStatus::LoadingModel { .. },
                Some(client),
            ) => Ok(client.clone()),
            _ => Err(Error::NotReady),
        }
    }

    /// Starts the engine and loads the model. Errors go to the status, not the caller.
    pub async fn start(self: &Arc<Self>) {
        if let Err(e) = self.try_start().await {
            log::error!("chat engine start failed: {e}");
            self.set_error(&e);
        }
    }

    /// The user clicked "Retry": start again with a fresh restart count.
    pub async fn restart(self: &Arc<Self>) {
        *self.restarts.lock().await = 0;
        self.stop().await;
        self.start().await;
    }

    pub async fn stop(&self) {
        self.client.send_replace(None);
        if let Some(sidecar) = self.sidecar.lock().await.take() {
            let paths = self.paths.clone();
            let _ = tokio::task::spawn_blocking(move || sidecar.stop(&paths)).await;
        }
    }

    async fn try_start(self: &Arc<Self>) -> Result<()> {
        self.status.send_replace(EngineStatus::Starting);
        let Some(exe) = sidecar::find_ollama(&self.paths) else {
            log::info!("no chat engine installed yet");
            self.status.send_replace(EngineStatus::NotInstalled);
            return Ok(());
        };
        let paths = self.paths.clone();
        let sidecar = tokio::task::spawn_blocking(move || {
            sidecar::stop_leftover(&paths);
            Sidecar::start(&paths, &exe)
        })
        .await
        .map_err(|e| Error::Engine(e.to_string()))??;
        let client = Client::new(sidecar.port())?;
        *self.sidecar.lock().await = Some(sidecar);
        self.wait_ready(&client).await?;
        self.client.send_replace(Some(client.clone()));

        let model = self.model();
        let list = client.installed().await?;
        self.store_installed(&list);
        self.learn_thinkers(&client, &list).await;
        if !self.is_installed(&model) {
            let (folder, _) = sidecar::models_folder(&self.paths);
            log::warn!("the chat model {model} is not in {}", folder.display());
            self.status.send_replace(EngineStatus::NoModel { model });
            return Ok(());
        }
        self.status.send_replace(EngineStatus::LoadingModel {
            model: model.clone(),
        });
        client
            .load(&model, KEEP_ALIVE, self.tuning_for(&model).context)
            .await?;
        self.status.send_replace(EngineStatus::Ready { model });
        Ok(())
    }

    async fn wait_ready(&self, client: &Client) -> Result<()> {
        let begin = tokio::time::Instant::now();
        let deadline = begin + READY_TIMEOUT;
        loop {
            if let Ok(version) = client.version().await {
                log::info!(
                    "chat engine ready: Ollama {version}, after {} s",
                    begin.elapsed().as_secs()
                );
                return Ok(());
            }
            if let Some(code) = self.sidecar.lock().await.as_mut().and_then(Sidecar::exited) {
                return Err(Error::Engine(format!(
                    "the engine stopped at start ({code})"
                )));
            }
            if tokio::time::Instant::now() > deadline {
                return Err(Error::Engine(
                    "the engine did not start in 2 minutes".into(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    /// Checks the process every few seconds and restarts it up to 3 times
    /// in a row.
    pub async fn watch_health(self: Arc<Self>) {
        let mut last_restart: Option<tokio::time::Instant> = None;
        loop {
            tokio::time::sleep(HEALTH_INTERVAL).await;
            let exited = self.sidecar.lock().await.as_mut().and_then(Sidecar::exited);
            let Some(code) = exited else {
                if last_restart.is_some_and(|t| t.elapsed() > STABLE_AFTER) {
                    *self.restarts.lock().await = 0;
                    last_restart = None;
                }
                continue;
            };
            last_restart = Some(tokio::time::Instant::now());
            self.client.send_replace(None);
            self.sidecar.lock().await.take();
            let attempt = {
                let mut restarts = self.restarts.lock().await;
                *restarts += 1;
                *restarts
            };
            if attempt > MAX_RESTARTS {
                log::error!(
                    "chat engine stopped ({code}) after {MAX_RESTARTS} restarts, no more restarts"
                );
                self.status.send_replace(EngineStatus::Error {
                    message: "The chat engine stopped.".into(),
                    detail: format!(
                        "It stopped again after {MAX_RESTARTS} restarts. Last exit: {code}"
                    ),
                });
                continue;
            }
            log::warn!("chat engine stopped ({code}), restart {attempt} of {MAX_RESTARTS}");
            self.start().await;
        }
    }

    fn set_error(&self, e: &Error) {
        let message = match e {
            Error::Engine(m) if m.contains("not installed") => "The chat engine is not installed.",
            Error::Engine(m) if m.contains("chat model") => "The chat model is not installed.",
            _ => "The chat engine could not start.",
        };
        self.status.send_replace(EngineStatus::Error {
            message: message.into(),
            detail: e.to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An engine that exits at once: the start fails, three restarts fail,
    /// then the status says so and no fourth restart happens.
    #[cfg(windows)]
    #[tokio::test(flavor = "multi_thread")]
    async fn an_engine_that_exits_ends_in_an_error_after_three_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().to_path_buf());
        paths.ensure().unwrap();
        let exe = paths.engines().join("ollama").join("ollama.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        // where.exe exits with code 1 when it gets "serve". The debug build
        // points the engine at ~/.ollama/models; this fake engine never reads it.
        let windows = std::env::var_os("SystemRoot").expect("SystemRoot");
        std::fs::copy(
            std::path::Path::new(&windows)
                .join("System32")
                .join("where.exe"),
            &exe,
        )
        .unwrap();

        let engine = Engine::new(paths);
        engine.start().await;
        assert!(matches!(engine.status(), EngineStatus::Error { .. }));
        let mut status = engine.subscribe();
        let health = tokio::spawn(Arc::clone(&engine).watch_health());
        let ended = tokio::time::timeout(Duration::from_secs(90), async {
            loop {
                status.changed().await.unwrap();
                if let EngineStatus::Error { detail, .. } = &*status.borrow()
                    && detail.contains("after 3 restarts")
                {
                    return;
                }
            }
        })
        .await;
        health.abort();
        assert!(
            ended.is_ok(),
            "the restarts never ended: {:?}",
            engine.status()
        );
        assert_eq!(*engine.restarts.lock().await, MAX_RESTARTS + 1);
    }
}

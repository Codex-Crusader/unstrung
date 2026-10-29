//! Keeps the chat engine running and reports its state.

use crate::error::{Error, Result};
use crate::ollama::Client;
use crate::paths::AppPaths;
use crate::sidecar::{self, Sidecar};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, watch};

/// The chat model for Phase 1. The model catalog (Phase 3) replaces it.
pub const DEFAULT_CHAT_MODEL: &str = "qwen2.5:7b";
/// How long the engine keeps the model in memory after the last reply.
pub const KEEP_ALIVE: &str = "30m";

const MAX_RESTARTS: u32 = 3;
const READY_TIMEOUT: Duration = Duration::from_secs(30);
const HEALTH_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum EngineStatus {
    Starting,
    LoadingModel { model: String },
    Ready { model: String },
    Error { message: String, detail: String },
}

pub struct Engine {
    paths: AppPaths,
    model: String,
    sidecar: Mutex<Option<Sidecar>>,
    client: watch::Sender<Option<Client>>,
    status: watch::Sender<EngineStatus>,
    restarts: Mutex<u32>,
}

impl Engine {
    pub fn new(paths: AppPaths) -> Arc<Self> {
        Arc::new(Self {
            paths,
            model: DEFAULT_CHAT_MODEL.to_owned(),
            sidecar: Mutex::new(None),
            client: watch::channel(None).0,
            status: watch::channel(EngineStatus::Starting).0,
            restarts: Mutex::new(0),
        })
    }

    pub fn model(&self) -> &str {
        &self.model
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
        let exe = sidecar::find_ollama(&self.paths).ok_or_else(sidecar::missing_engine)?;
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

        if !client.has_model(&self.model).await? {
            let (folder, _) = sidecar::models_folder(&self.paths);
            return Err(Error::Engine(format!(
                "the chat model {} is not in {}",
                self.model,
                folder.display()
            )));
        }
        self.status.send_replace(EngineStatus::LoadingModel {
            model: self.model.clone(),
        });
        client.load(&self.model, KEEP_ALIVE).await?;
        self.client.send_replace(Some(client));
        self.status.send_replace(EngineStatus::Ready {
            model: self.model.clone(),
        });
        Ok(())
    }

    async fn wait_ready(&self, client: &Client) -> Result<()> {
        let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
        loop {
            if let Ok(version) = client.version().await {
                log::info!("chat engine ready: Ollama {version}");
                return Ok(());
            }
            if let Some(code) = self.sidecar.lock().await.as_mut().and_then(Sidecar::exited) {
                return Err(Error::Engine(format!(
                    "the engine stopped at start ({code})"
                )));
            }
            if tokio::time::Instant::now() > deadline {
                return Err(Error::Engine(
                    "the engine did not start in 30 seconds".into(),
                ));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    /// Checks the process every few seconds and restarts it up to 3 times.
    pub async fn watch_health(self: Arc<Self>) {
        loop {
            tokio::time::sleep(HEALTH_INTERVAL).await;
            let exited = self.sidecar.lock().await.as_mut().and_then(Sidecar::exited);
            let Some(code) = exited else {
                continue;
            };
            self.client.send_replace(None);
            self.sidecar.lock().await.take();
            let attempt = {
                let mut restarts = self.restarts.lock().await;
                *restarts += 1;
                *restarts
            };
            log::warn!("chat engine stopped ({code}), restart {attempt} of {MAX_RESTARTS}");
            if attempt > MAX_RESTARTS {
                self.status.send_replace(EngineStatus::Error {
                    message: "The chat engine stopped.".into(),
                    detail: format!("It stopped {MAX_RESTARTS} times. Last exit: {code}"),
                });
                continue;
            }
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

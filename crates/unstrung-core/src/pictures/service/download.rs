//! Downloads of the picture engine and of the picture models.

use super::*;

impl PictureService {
    pub fn stop_download(&self) {
        if let Ok(running) = self.running.lock()
            && let Some(token) = running.as_ref()
        {
            token.cancel();
        }
        self.setup.cancel();
    }

    pub(super) fn report(&self, d: Download) {
        self.download.send_replace(Some(d));
    }

    /// Starts the downloads for `id` in the background: the engine if it is
    /// missing, then the model parts that are not on disk. Progress goes to
    /// `subscribe_download`. The model becomes the chosen one at the end.
    pub fn download(self: &Arc<Self>, id: &str) -> Result<()> {
        let model = catalog::model(id).ok_or(Error::NotFound("image model"))?;
        let token = self.take_slot()?.keep();
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

    pub(super) async fn download_inner(
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

    pub(super) async fn install_engine(
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
}

//! Runs the picture engine: the graphics card, the reference pictures and the work files.

use super::*;

impl PictureService {
    /// Frees the GPU, renders with the references and, when that fails (for
    /// example, too little free VRAM for the pictures), once more with tags
    /// only. The chat model comes back once, after the last try: a reload
    /// after the first try raced the retry for the VRAM.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn render_or_tags(
        &self,
        model: &'static ImageModel,
        exe: &std::path::Path,
        chat_id: i64,
        message_id: Option<i64>,
        (with_refs, plain): (scene::Prompts, scene::Prompts),
        seed: i64,
        chat_model: &str,
        refs: &References,
        token: &CancellationToken,
    ) -> Result<Image> {
        self.painting.send_replace(Some(Painting {
            chat_id,
            message_id,
            phase: "loading",
            step: 0,
            total: 0,
        }));
        let client = self.chats.engine().client().ok();
        let freed = if self.hardware().tier == Tier::High {
            false
        } else if let Some(client) = &client {
            free_gpu(client, chat_model).await
        } else {
            false
        };
        let result = match self
            .render(
                model, exe, chat_id, message_id, with_refs, seed, refs, token,
            )
            .await
        {
            Err(e)
                if !refs.files.is_empty()
                    && !matches!(e, Error::Cancelled)
                    && !token.is_cancelled() =>
            {
                log::warn!("a picture with profile pictures failed, so it is made from tags: {e}");
                self.render(
                    model,
                    exe,
                    chat_id,
                    message_id,
                    plain,
                    seed,
                    &References::none(),
                    token,
                )
                .await
            }
            other => other,
        };
        // The chat model comes back whether the picture worked or not.
        if freed && let Some(client) = client {
            let model = chat_model.to_owned();
            let context = self.chats.engine().tuning_for(&model).context;
            tokio::spawn(async move {
                match client.load(&model, KEEP_ALIVE, context).await {
                    Ok(()) => log::info!("chat model loaded again after a picture: {model}"),
                    Err(e) => log::warn!("the chat model did not load again after a picture: {e}"),
                }
            });
        }
        result
    }

    /// A stored profile picture that exists on disk. `None` for no picture,
    /// which the screens show as the silhouette.
    pub(super) fn reference_file(&self, avatar: Option<&str>) -> Option<std::path::PathBuf> {
        let file = self.paths.stored(avatar?).ok()?;
        file.is_file().then_some(file)
    }

    /// Runs the engine for one picture and saves it.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn render(
        &self,
        model: &'static ImageModel,
        exe: &std::path::Path,
        chat_id: i64,
        message_id: Option<i64>,
        prompts: scene::Prompts,
        seed: i64,
        refs: &References,
        token: &CancellationToken,
    ) -> Result<Image> {
        let hw = self.hardware();
        let on_gpu = Self::usable_gpu(&hw).is_some();
        let size = if on_gpu { 1024 } else { 512 };
        let folder = self.paths.gallery().join(chat_id.to_string());
        std::fs::create_dir_all(&folder)?;
        let stamp = format!("{}-{seed}", crate::db::now_ms());
        let file = format!("{stamp}.png");
        // The engine reads and writes plain files only. They live in the
        // work folder for this one picture.
        let work = self.paths.work();
        std::fs::create_dir_all(&work)?;
        let key = self.paths.key()?;
        let output = work.join(&file);
        let mut plain = WorkFiles(vec![output.clone()]);
        for (i, r) in refs.files.iter().enumerate() {
            let copy = work.join(format!("{stamp}-ref{i}.png"));
            plain.0.push(copy.clone());
            crate::files::write_atomic(&copy, &key.read(r)?)?;
        }
        let references = plain.0[1..].to_vec();

        let job = runner::Job {
            model,
            models_dir: &self.paths.image_models(),
            prompts: &prompts,
            width: size,
            height: size,
            seed,
            output: &output,
            references: &references,
        };
        let mut cmd = std::process::Command::new(exe);
        cmd.args(runner::args(&job));
        let log = self.paths.logs().join("sdcpp.log");
        let limit = if on_gpu { GPU_LIMIT } else { CPU_LIMIT };
        let run_token = token.clone();
        let sender = self.painting.clone();
        let out = output.clone();
        let started = Instant::now();
        let steps = model.steps;
        let ran = tokio::task::spawn_blocking(move || {
            runner::run(cmd, &out, &log, limit, &run_token, &|step, total| {
                // After the sampling steps, the engine decodes the picture in
                // tiles and prints a second bar for them.
                let phase = if total == steps {
                    "painting"
                } else {
                    "finishing"
                };
                sender.send_replace(Some(Painting {
                    chat_id,
                    message_id,
                    phase,
                    step,
                    total,
                }));
            })
        })
        .await
        .map_err(|e| Error::Engine(format!("the picture stopped: {e}")))?;
        ran?;
        let saved_file = folder.join(&file);
        key.write(&saved_file, &std::fs::read(&output)?)?;
        drop(plain);
        log::info!(
            "picture made in {} s with {} ({}x{})",
            started.elapsed().as_secs(),
            model.id,
            size,
            size
        );
        let saved = self
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
                ref_mode: refs.mode.clone(),
                width: i64::from(size),
                height: i64::from(size),
                adult: prompts.adult,
            })
            .await;
        // A file without its row is never shown and never deleted, so it goes now.
        let id = saved.inspect_err(|_| {
            let _ = std::fs::remove_file(&saved_file);
        })?;
        self.chats.db().image(id).await
    }
}

/// Unloads the chat model and waits until the engine no longer lists it.
/// Returns true when it was loaded, so the caller loads it again later.
pub(super) async fn free_gpu(client: &crate::ollama::Client, model: &str) -> bool {
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
    loop {
        match client.loaded().await {
            Ok(l) if l.iter().all(|m| m.name != model) => {
                log::info!("chat model unloaded for a picture: {model}");
                break;
            }
            _ if started.elapsed() >= UNLOAD_WAIT => {
                log::warn!("the chat model is still loaded after the wait: {model}");
                break;
            }
            _ => tokio::time::sleep(Duration::from_millis(250)).await,
        }
    }
    true
}

/// Plain files in the work folder, removed when this drops, also on an
/// error or a cancel.
pub(super) struct WorkFiles(Vec<std::path::PathBuf>);

impl Drop for WorkFiles {
    fn drop(&mut self) {
        for f in &self.0 {
            let _ = std::fs::remove_file(f);
        }
    }
}

/// The profile pictures of one picture job, and how the `images` row records
/// them: "tags", or "ref:character,persona" in "image N" order.
pub(super) struct References {
    pub(super) files: Vec<std::path::PathBuf>,
    pub(super) mode: String,
}

impl References {
    pub(super) fn none() -> Self {
        Self {
            files: Vec::new(),
            mode: "tags".into(),
        }
    }

    /// The files in `order`. Any missing one means tags only, because the
    /// prompt counts the images.
    pub(super) fn of(
        order: &[scene::Who],
        character: Option<&std::path::PathBuf>,
        persona: Option<&std::path::PathBuf>,
    ) -> Self {
        let files: Option<Vec<std::path::PathBuf>> = order
            .iter()
            .map(|w| match w {
                scene::Who::Character => character.cloned(),
                scene::Who::Persona => persona.cloned(),
                _ => None,
            })
            .collect();
        match files {
            Some(files) if !files.is_empty() => {
                let names: Vec<&str> = order
                    .iter()
                    .map(|w| {
                        if *w == scene::Who::Persona {
                            "persona"
                        } else {
                            "character"
                        }
                    })
                    .collect();
                Self {
                    files,
                    mode: format!("ref:{}", names.join(",")),
                }
            }
            _ => Self::none(),
        }
    }

    /// The order from a stored mode. "tags" or anything unknown: none.
    pub(super) fn parse(mode: &str) -> Vec<scene::Who> {
        let Some(list) = mode.strip_prefix("ref:") else {
            return Vec::new();
        };
        let order: Option<Vec<scene::Who>> = list
            .split(',')
            .map(|n| match n {
                "character" => Some(scene::Who::Character),
                "persona" => Some(scene::Who::Persona),
                _ => None,
            })
            .collect();
        order.unwrap_or_default()
    }
}

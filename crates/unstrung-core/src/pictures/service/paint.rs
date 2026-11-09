//! One picture from the chat: the checks, the scene and the gallery row.

use super::*;

impl PictureService {
    /// Makes one picture for a chat. `message_id` is the message to picture
    /// (the latest when `None`), `wish` is the `/imagine` text.
    pub async fn paint(&self, chat_id: i64, message_id: Option<i64>, wish: &str) -> Result<Image> {
        let model = self.selected().await?.ok_or(Error::NotReady)?;
        let exe = self.ready_engine(model)?;
        let (token, _guard) = self.chats.begin_job()?;
        self.start_painting(chat_id, message_id)?;
        let result = self
            .paint_inner(model, &exe, chat_id, message_id, wish, &token)
            .await;
        self.painting.send_replace(None);
        result
    }

    /// A variation of a gallery picture: its saved prompt with a new seed and
    /// another framing. The ages are checked again, because a character or
    /// persona can change after a picture.
    pub async fn repaint(&self, image_id: i64) -> Result<Image> {
        let old = self.chats.db().image(image_id).await?;
        let model = self.selected().await?.ok_or(Error::NotReady)?;
        let exe = self.ready_engine(model)?;
        let (token, _guard) = self.chats.begin_job()?;
        self.start_painting(old.chat_id, old.message_id)?;
        let result = async {
            let (character, persona, chat_model) = self.chats.picture_people(old.chat_id).await?;
            // The saved prompt is the scene: the chat since then is not drawn.
            let people = safety::People {
                character_age: character.age,
                persona_age: persona.as_ref().and_then(|p| p.age),
                minor_mentioned: false,
            };
            let adult_on = adult_rules(
                model,
                crate::content::mature_chat(self.chats.db(), &character, persona.as_ref()).await?,
            );
            let rules = repaint_rules(&old, people, adult_on).map_err(blocked)?;
            let seed = runner::new_seed();
            let prompts = scene::Prompts {
                positive: scene::varied(&old.prompt, seed),
                negative: old.negative_prompt.clone(),
                adult: rules.mature,
            };
            // The same people as references, with their pictures of today.
            // A picture that is gone now, or a persona the prompt does not
            // describe (another persona since), means tags only. The
            // character of a chat never changes.
            let order = References::parse(&old.ref_mode);
            let same_people = !order.contains(&scene::Who::Persona)
                || persona
                    .as_ref()
                    .is_some_and(|p| scene::describes(&old.prompt, &p.appearance_tags));
            let refs = if order.is_empty()
                || !same_people
                || !model.ref_images
                || !safety::allows_references(&old.prompt, old.adult)
            {
                References::none()
            } else {
                let character_picture = self.reference_file(character.avatar_path.as_deref());
                let persona_picture = persona
                    .as_ref()
                    .and_then(|p| self.reference_file(p.avatar_path.as_deref()));
                References::of(&order, character_picture.as_ref(), persona_picture.as_ref())
            };
            let plain = scene::Prompts {
                positive: scene::without_references(&prompts.positive),
                negative: prompts.negative.clone(),
                adult: prompts.adult,
            };
            let prompts = if refs.files.is_empty() {
                plain.clone()
            } else {
                prompts
            };
            self.render_or_tags(
                model,
                &exe,
                old.chat_id,
                old.message_id,
                (prompts, plain),
                seed,
                &chat_model,
                &refs,
                &token,
            )
            .await
        }
        .await;
        self.painting.send_replace(None);
        result
    }

    /// The engine path, when the engine and `model` are on disk.
    pub(super) fn ready_engine(&self, model: &ImageModel) -> Result<std::path::PathBuf> {
        if !model.installed(&self.paths.image_models())
            || engine_setup::installed_version(&self.paths, Engine::Image).is_none()
        {
            return Err(Error::NotReady);
        }
        Ok(engine_setup::folder(&self.paths, Engine::Image).join(Engine::Image.exe()))
    }

    /// Marks a picture as running. Fails while a download or removal runs,
    /// checked under the same lock that they take.
    pub(super) fn start_painting(&self, chat_id: i64, message_id: Option<i64>) -> Result<()> {
        let running = self.running.lock().map_err(|_| Error::Busy)?;
        if running.is_some() {
            return Err(Error::Invalid(
                "Wait until the picture download ends.".into(),
            ));
        }
        self.painting.send_replace(Some(Painting {
            chat_id,
            message_id,
            phase: "scene",
            step: 0,
            total: 0,
        }));
        Ok(())
    }

    pub(super) async fn paint_inner(
        &self,
        model: &'static ImageModel,
        exe: &std::path::Path,
        chat_id: i64,
        message_id: Option<i64>,
        wish: &str,
        token: &CancellationToken,
    ) -> Result<Image> {
        let input = self
            .chats
            .picture_scene(chat_id, message_id, wish, token)
            .await?;
        let message_id = input.message_id;

        let character_picture = self.reference_file(input.character.avatar_path.as_deref());
        let persona_picture = input
            .persona
            .as_ref()
            .and_then(|p| self.reference_file(p.avatar_path.as_deref()));
        let persons = |pictures: bool| {
            let character = scene::Person {
                appearance: &input.character.appearance_tags,
                outfit: &input.character.outfit,
                picture: pictures && character_picture.is_some(),
            };
            let persona = input.persona.as_ref().map(|p| scene::Person {
                appearance: &p.appearance_tags,
                outfit: &p.outfit,
                picture: pictures && persona_picture.is_some(),
            });
            (character, persona)
        };
        // The check reads the prompt without the reference words.
        let (character, persona) = persons(false);
        let positive = scene::positive(
            &input.scene,
            &input.character.art_style,
            &character,
            persona.as_ref(),
        );
        let people = safety::People {
            character_age: input.character.age,
            persona_age: input.persona.as_ref().and_then(|p| p.age),
            minor_mentioned: input.minor_mentioned,
        };
        // Read for this picture.
        let adult_on = adult_rules(
            model,
            crate::content::mature_chat(self.chats.db(), &input.character, input.persona.as_ref())
                .await?,
        );
        let checked = format!("{positive}, {wish}");
        let rules =
            safety::check(&checked, people, adult_on, input.scene.sexual).map_err(blocked)?;
        let seed = runner::new_seed();
        // A second picture of the same message gets another framing, or it
        // comes out almost the same.
        let again = message_id.is_some()
            && self
                .chats
                .db()
                .chat_images(chat_id)
                .await?
                .iter()
                .any(|i| i.message_id == message_id);
        let frame = |text: String| {
            if again {
                scene::varied(&text, seed)
            } else {
                text
            }
        };
        let plain = scene::with_rules(&frame(positive), rules);
        // The profile pictures, for a model that takes them and a prompt
        // that allows them.
        let (character, persona) = persons(true);
        let order = if model.ref_images && safety::allows_references(&checked, input.scene.sexual) {
            scene::references(&input.scene, &character, persona.as_ref())
        } else {
            Vec::new()
        };
        if order.is_empty() {
            return self
                .render_or_tags(
                    model,
                    exe,
                    chat_id,
                    message_id,
                    (plain.clone(), plain),
                    seed,
                    &input.chat_model,
                    &References::none(),
                    token,
                )
                .await;
        }
        let refs = References::of(&order, character_picture.as_ref(), persona_picture.as_ref());
        let with_refs = scene::with_rules(
            &frame(scene::positive(
                &input.scene,
                &input.character.art_style,
                &character,
                persona.as_ref(),
            )),
            rules,
        );
        self.render_or_tags(
            model,
            exe,
            chat_id,
            message_id,
            (with_refs, plain),
            seed,
            &input.chat_model,
            &refs,
            token,
        )
        .await
    }
}

pub(super) fn blocked(block: safety::Block) -> Error {
    log::info!("a picture was blocked: {block:?}");
    Error::Rejected {
        message: blocked_message(block).into(),
        why: format!("{block:?}"),
    }
}

/// The sentence the user sees for a blocked picture.
/// Adult rules for one picture: the chat allows mature content now, and the
/// model is marked for adult pictures. FLUX.2 klein is not, so explicit
/// pictures stay off with it whatever the setting.
/// The rules for a variation of `old`, with the ages of today. The scene
/// step's flag is saved as `adult`: a clean prompt like "a quiet bedroom"
/// can still be a sexual scene.
pub(super) fn repaint_rules(
    old: &Image,
    people: safety::People,
    adult_on: bool,
) -> std::result::Result<safety::Rules, safety::Block> {
    safety::check(&old.prompt, people, adult_on, old.adult)
}

pub(super) fn adult_rules(model: &ImageModel, mature_chat: bool) -> bool {
    model.mature_ok && mature_chat
}

pub fn blocked_message(block: safety::Block) -> &'static str {
    match block {
        safety::Block::MinorWords => {
            "This picture was not made. Unstrung never makes sexual pictures of anyone who could be under 18."
        }
        safety::Block::MinorInChat => {
            "This picture was not made. Unstrung never makes sexual or revealing pictures when someone in the chat is under 18 or has no age. If your persona is an adult, add your age in Personas."
        }
        safety::Block::MinorMentioned => {
            "This picture was not made. The story just mentioned someone under 18, so Unstrung makes no sexual or revealing picture of this scene."
        }
        safety::Block::AdultOff => "This picture was not made, because adult pictures are off.",
    }
}

#[cfg(test)]
mod adult_tests {
    use super::*;

    #[test]
    fn a_model_without_the_adult_mark_never_makes_adult_pictures() {
        let adults = safety::People {
            character_age: 30,
            persona_age: Some(30),
            minor_mentioned: false,
        };
        let klein = catalog::model("flux2-klein-4b").unwrap();
        assert!(!adult_rules(klein, true));
        assert_eq!(
            safety::check("nude on the bed", adults, adult_rules(klein, true), false),
            Err(safety::Block::AdultOff)
        );
        let z_image = catalog::model("z-image-turbo").unwrap();
        assert!(adult_rules(z_image, true) && !adult_rules(z_image, false));
    }

    #[test]
    fn a_variation_of_an_adult_scene_checks_the_ages_of_today() {
        let old = Image {
            id: 1,
            chat_id: 1,
            message_id: None,
            file_path: "1/p.png".into(),
            prompt: "a quiet bedroom".into(),
            negative_prompt: String::new(),
            seed: 1,
            image_model: "z-image-turbo".into(),
            ref_mode: String::new(),
            width: 1024,
            height: 1024,
            adult: true,
            created_at: 0,
        };
        let people = |persona_age| safety::People {
            character_age: 30,
            persona_age,
            minor_mentioned: false,
        };
        // The persona is 16 now: the clean words do not hide the scene.
        assert_eq!(
            repaint_rules(&old, people(Some(16)), true),
            Err(safety::Block::MinorInChat)
        );
        assert_eq!(
            repaint_rules(&old, people(Some(30)), false),
            Err(safety::Block::AdultOff)
        );
        assert!(repaint_rules(&old, people(Some(30)), true).unwrap().mature);
    }
}

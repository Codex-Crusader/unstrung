//! Done-tests against the real chat engine.
//!
//! Phase 1: a message streams a reply, Stop keeps the partial text, and the
//! history stays after a reopen. Phase 2: regenerate, versions, rewind, delete
//! with undo, edit, continue, branch, persona switch, OOC notes and the anchor helper.
//!
//! Needs Ollama and the default chat model, so it is ignored by default:
//! `cargo test -p unstrung-core --test live_chat -- --ignored --nocapture`

use std::sync::Arc;
use std::time::Duration;
use unstrung_core::chat::{ChatService, RegenMode};
use unstrung_core::db::{Db, Message};
use unstrung_core::engine::{Engine, EngineStatus};
use unstrung_core::paths::AppPaths;
use unstrung_core::starters;

fn texts(view: &unstrung_core::chat::ChatView) -> Vec<(&str, &str)> {
    view.messages
        .iter()
        .map(|m| (m.message.role.as_str(), m.message.content.as_str()))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Ollama and the default chat model"]
async fn chat_engine_done_tests() {
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths::at(home.path().to_path_buf());
    paths.ensure().unwrap();

    let engine = Engine::new(paths.clone());
    engine.start().await;
    let status = engine.status();
    assert!(matches!(status, EngineStatus::Ready { .. }), "{status:?}");

    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
    starters::ensure(&db, &paths).await.unwrap();
    let ren = db
        .character_by_starter("ren_kagami")
        .await
        .unwrap()
        .unwrap();
    let kai = db.persona_by_starter("kai").await.unwrap().unwrap();
    let nora = db.persona_by_starter("nora").await.unwrap().unwrap();
    let service = Arc::new(ChatService::new(db.clone(), engine.clone()));
    let view = service.new_chat(ren, Some(kai)).await.unwrap();
    assert_eq!(view.messages.len(), 1, "the chat starts with the greeting");
    let chat_id = view.chat.id;

    // 1. A full streamed reply.
    let mut pieces = 0;
    let mut user_id = None;
    let reply = service
        .send(
            chat_id,
            "I have a case for you. My brother is missing.",
            |m| user_id = Some(m.id),
            |_| pieces += 1,
        )
        .await
        .unwrap()
        .message
        .expect("a reply");
    println!("reply in {pieces} pieces: {}", reply.content);
    assert!(pieces > 1, "the reply must stream");
    assert_eq!(reply.parent_id, user_id);

    // 2. Regenerate keeps the old reply as a sibling, and the arrows switch between them.
    let shorter = service
        .regenerate(chat_id, reply.id, RegenMode::Shorter, |_| {})
        .await
        .unwrap()
        .message
        .unwrap();
    let view = service.view(chat_id).await.unwrap();
    assert_eq!(view.messages[2].siblings, [reply.id, shorter.id]);
    let view = service.show_branch(chat_id, reply.id).await.unwrap();
    assert_eq!(view.messages[2].message.id, reply.id);
    service
        .regenerate(chat_id, reply.id, RegenMode::NotInCharacter, |_| {})
        .await
        .unwrap();
    assert!(
        db.message(chat_id, reply.id)
            .await
            .unwrap()
            .flagged_out_of_character
    );
    service.show_branch(chat_id, reply.id).await.unwrap();

    // 3. Continue adds to the same message.
    let longer = service
        .continue_reply(chat_id, |_| {})
        .await
        .unwrap()
        .message
        .unwrap();
    assert_eq!(longer.id, reply.id);
    assert!(longer.content.len() >= reply.content.len());
    println!("continued: {}", longer.content.replace('\n', " / "));

    // 4. Persona switch adds a note, an OOC note works, and a rewind restores Kai.
    let view = service.set_persona(chat_id, Some(nora)).await.unwrap();
    assert_eq!(view.messages.last().unwrap().message.role, "system");
    let ooc = service
        .send(chat_id, "/ooc keep it short and moody", |_| {}, |_| {})
        .await
        .unwrap();
    assert!(ooc.message.is_some());
    let view = service.view(chat_id).await.unwrap();
    assert_eq!(view.persona.as_ref().unwrap().name, "Nora");
    let ooc_msg = &view.messages[view.messages.len() - 2].message;
    assert_eq!(ooc_msg.role, "ooc");
    let view = service.set_head(chat_id, reply.id).await.unwrap();
    assert_eq!(view.persona.as_ref().unwrap().name, "Kai");
    assert_eq!(view.branches.len(), 1, "the system note is a hidden branch");

    // 5. Edit a user message: a new version and a new reply.
    let edited = service
        .edit_and_resend(
            chat_id,
            user_id.unwrap(),
            "My sister is missing.",
            |_| {},
            |_| {},
        )
        .await
        .unwrap();
    assert!(edited.message.is_some());
    let view = service.view(chat_id).await.unwrap();
    assert_eq!(view.messages[1].message.content, "My sister is missing.");
    assert_eq!(view.messages[1].siblings.len(), 2);

    // 6. Delete with undo.
    let before = texts(&view).len();
    let target = view.messages[1].message.id;
    let (after, undo) = service.delete_message(chat_id, target).await.unwrap();
    assert_eq!(texts(&after).len(), 1);
    let restored = service.undo_delete(chat_id, &undo).await.unwrap();
    assert_eq!(texts(&restored).len(), before);

    // 7. Branch to a new chat copies the path up to a message.
    let branch = service.branch(chat_id, target).await.unwrap();
    assert_ne!(branch.chat.id, chat_id);
    assert_eq!(texts(&branch), texts(&restored)[..2]);

    // 8. Busy while a reply runs, and Stop keeps the partial text.
    let (first_tx, first_rx) = tokio::sync::oneshot::channel();
    let runner = {
        let service = service.clone();
        tokio::spawn(async move {
            let mut first = Some(first_tx);
            service
                .send(
                    chat_id,
                    "Tell me everything you know about this city, in detail.",
                    |_| {},
                    |_| {
                        if let Some(tx) = first.take() {
                            let _ = tx.send(());
                        }
                    },
                )
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(60), first_rx)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        service.retry(chat_id, |_| {}).await,
        Err(unstrung_core::Error::Busy)
    ));
    service.stop();
    let stopped = runner.await.unwrap().unwrap();
    println!("stopped reply: {stopped:?}");

    // 9. The voice anchor helper.
    let anchor = service
        .suggest_anchor("June", "Warm, chatty, a bit clumsy.")
        .await
        .unwrap();
    println!("anchor: {anchor}");
    assert!(!anchor.is_empty() && anchor.len() <= 400);

    // 10. History after a reopen.
    let expected: Vec<(String, String)> = texts(&service.view(chat_id).await.unwrap())
        .into_iter()
        .map(|(r, c)| (r.to_owned(), c.to_owned()))
        .collect();
    db.close().await;
    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
    let path: Vec<Message> = db.path_to_head(chat_id).await.unwrap();
    let got: Vec<(String, String)> = path.into_iter().map(|m| (m.role, m.content)).collect();
    assert_eq!(got, expected);

    engine.stop().await;
    assert!(
        !paths.engine_pids().exists(),
        "the PID file is removed at a clean stop"
    );
}

/// Phase 3: a missing default model leaves the engine running, so the model
/// list works. A chat cannot reply then. Picking an installed model makes it ready.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Ollama and the default chat model"]
async fn missing_model_keeps_the_engine_running() {
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths::at(home.path().to_path_buf());
    paths.ensure().unwrap();

    let engine = Engine::with_model(
        paths.clone(),
        "no-such-model:1b",
        unstrung_core::catalog::Advanced::default(),
    );
    engine.start().await;
    let status = engine.status();
    assert!(matches!(status, EngineStatus::NoModel { .. }), "{status:?}");
    assert!(engine.client().is_err());
    let installed = engine.refresh_installed().await.unwrap();
    assert!(engine.is_installed(unstrung_core::engine::DEFAULT_CHAT_MODEL));
    assert!(!installed.is_empty());

    engine
        .set_model(unstrung_core::engine::DEFAULT_CHAT_MODEL)
        .await
        .unwrap();
    assert!(matches!(engine.status(), EngineStatus::Ready { .. }));
    assert!(engine.client().is_ok());
    engine.stop().await;
}

/// Phase 4: a GGUF file imports as a model of its own. Set UNSTRUNG_TEST_GGUF
/// to a GGUF file. By default the file is a hard link, so the engine already
/// has the blob and skips the upload. With UNSTRUNG_TEST_GGUF_UPLOAD set, the
/// test copies the file with one more byte, so the upload runs too.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Ollama, the test models and UNSTRUNG_TEST_GGUF"]
async fn a_gguf_file_imports_as_a_model() {
    let source = std::path::PathBuf::from(std::env::var("UNSTRUNG_TEST_GGUF").unwrap());
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths::at(home.path().to_path_buf());
    paths.ensure().unwrap();
    let file = home.path().join("My Test Model.gguf");
    if std::env::var_os("UNSTRUNG_TEST_GGUF_UPLOAD").is_some() {
        // One byte more gives a new digest, so the engine must receive the
        // whole file. GGUF readers ignore bytes after the last tensor.
        std::fs::copy(&source, &file).unwrap();
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .unwrap();
        std::io::Write::write_all(&mut f, &[0]).unwrap();
    } else {
        std::fs::hard_link(&source, &file).unwrap();
    }

    let engine = Engine::new(paths.clone());
    engine.start().await;
    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
    let startup = unstrung_core::models::startup(&db, &paths).await.unwrap();
    let models = unstrung_core::models::ModelService::new(engine.clone(), db, &startup);
    let mut progress = models.subscribe();

    let name = models.import_file(file.clone()).await.unwrap();
    assert_eq!(name, "my-test-model");
    let end = loop {
        progress.changed().await.unwrap();
        let now = progress.borrow_and_update().clone().unwrap();
        if !matches!(now.state, unstrung_core::models::DownloadState::Importing) {
            break now;
        }
    };
    assert_eq!(
        end.state,
        unstrung_core::models::DownloadState::Done,
        "{end:?}"
    );
    let installed = engine.refresh_installed().await.unwrap();
    assert!(
        installed
            .iter()
            .any(|m| m.name.starts_with("my-test-model"))
    );

    // A file with a .gguf name but other bytes is refused.
    let not_gguf = home.path().join("notes.gguf");
    std::fs::write(&not_gguf, b"not a model").unwrap();
    assert!(models.import_file(not_gguf).await.is_err());

    engine
        .client()
        .unwrap()
        .delete("my-test-model")
        .await
        .unwrap();
    engine.stop().await;
    // The database pool can still hold app.db, which stops the temporary
    // folder from going away on Windows, so remove the large file now.
    std::fs::remove_file(&file).unwrap();
}

/// The scene writer fills a person's outfit only when the story changed
/// their clothes. Prints each answer, so a person can read them.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Ollama and the default chat model"]
async fn the_scene_writer_gives_clothes_only_when_the_story_changes_them() {
    use unstrung_core::db::NewMessage;
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths::at(home.path().to_path_buf());
    paths.ensure().unwrap();
    let engine = Engine::new(paths.clone());
    engine.start().await;
    assert!(matches!(engine.status(), EngineStatus::Ready { .. }));
    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
    starters::ensure(&db, &paths).await.unwrap();
    let mara = db.character_by_starter("mara_voss").await.unwrap().unwrap();
    let kai = db.persona_by_starter("kai").await.unwrap().unwrap();
    let service = Arc::new(ChatService::new(db.clone(), engine.clone()));

    let chat = |lines: &'static [(&'static str, &'static str)]| {
        let service = service.clone();
        let db = db.clone();
        async move {
            let view = service.new_chat(mara, Some(kai)).await.unwrap();
            let mut parent = view.messages.last().map(|m| m.message.id);
            for (role, text) in lines {
                let m = db
                    .append_message(&NewMessage {
                        chat_id: view.chat.id,
                        parent_id: parent,
                        role,
                        persona_id: Some(kai),
                        content: text,
                        raw_content: None,
                        token_count: None,
                        model: None,
                        prompt_version: None,
                        sampler_json: None,
                    })
                    .await
                    .unwrap();
                parent = Some(m.id);
            }
            view.chat.id
        }
    };
    let gala = chat(&[
        ("user", "The gala starts in an hour. You cannot go in that flight jacket, Captain."),
        ("character", "*Mara sighs, goes into her cabin and comes back in a long red evening dress.* Happy now? Do not get used to it."),
        ("user", "You look amazing. Shall we go?"),
    ])
    .await;
    let engine_room = chat(&[
        ("user", "I think the coupling is shot."),
        (
            "character",
            "*She taps the cracked console.* Then we make a new one. Grab a wrench.",
        ),
        ("user", "On it. Hold the light for me?"),
    ])
    .await;

    let token = tokio_util::sync::CancellationToken::new();
    let mut changed = 0;
    let mut invented = 0;
    for _ in 0..3 {
        let s = service
            .picture_scene(gala, None, "", &token)
            .await
            .unwrap()
            .scene;
        println!(
            "gala: character {:?}, persona {:?}",
            s.character_outfit, s.persona_outfit
        );
        if s.character_outfit.contains("dress") {
            changed += 1;
        }
        let s = service
            .picture_scene(engine_room, None, "", &token)
            .await
            .unwrap()
            .scene;
        println!(
            "engine room: character {:?}, persona {:?}",
            s.character_outfit, s.persona_outfit
        );
        if !s.character_outfit.is_empty() || !s.persona_outfit.is_empty() {
            invented += 1;
        }
    }
    engine.stop().await;
    assert!(changed >= 2, "the dress showed {changed} of 3 times");
    assert!(invented <= 1, "clothes were made up {invented} of 3 times");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Ollama and the default chat model"]
async fn looks_come_from_a_card_from_another_app() {
    use unstrung_core::card::parse_card;
    use unstrung_core::pictures::safety;
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths::at(home.path().to_path_buf());
    paths.ensure().unwrap();
    let engine = Engine::new(paths.clone());
    engine.start().await;
    assert!(matches!(engine.status(), EngineStatus::Ready { .. }));
    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
    let service = ChatService::new(db, engine.clone());

    // A Tavern V2 card, as another app exports it: the looks are in the description.
    let card = serde_json::json!({
        "spec": "chara_card_v2",
        "spec_version": "2.0",
        "data": {
            "name": "Mira Thorn",
            "description": "Mira is a smuggler who runs a small boat on the river. She has long copper hair in a braid, green eyes, freckles and a thin scar over her left eyebrow. She usually wears a patched leather vest and a red scarf.",
            "personality": "Quick, proud, funny when she is nervous.",
            "scenario": "{{user}} needs a ride across the river at night.",
            "first_mes": "*Mira looks up from the rope she is coiling.* You are late. The river does not wait.",
            "mes_example": ""
        }
    });
    let parsed = parse_card(card.to_string().as_bytes()).unwrap();
    assert_eq!(parsed.fields.appearance_tags, "");
    let f = &parsed.fields;
    let text = [
        &f.personality,
        &f.scenario,
        &f.greeting,
        &f.example_dialogue,
    ]
    .iter()
    .filter(|t| !t.trim().is_empty())
    .map(|t| t.as_str())
    .collect::<Vec<_>>()
    .join("\n\n");
    for _ in 0..3 {
        let s = service.suggest_looks(&f.name, &text).await.unwrap();
        println!("Mira: looks [{}] outfit [{}]", s.looks, s.outfit);
        let looks = s.looks.to_lowercase();
        assert!(looks.contains("copper") && looks.contains("green"), "{s:?}");
        assert!(!looks.contains("vest") && !looks.contains("scarf"), "{s:?}");
        assert!(
            s.outfit.to_lowercase().contains("vest") || s.outfit.to_lowercase().contains("scarf"),
            "{s:?}"
        );
        assert!(!safety::flagged(&s.looks) && !safety::flagged(&s.outfit));
    }

    // A card that says nothing about looks: the model picks plain ones.
    let bram = "Loud, warm, superstitious, loves tall tales and teasing. A retired sea captain with a big laugh.\n\n\
                {{user}} walks into Bram's cluttered shop on a rainy afternoon.";
    for _ in 0..2 {
        let s = service.suggest_looks("Bram Kettle", bram).await.unwrap();
        println!("Bram: looks [{}] outfit [{}]", s.looks, s.outfit);
        assert!(!s.looks.is_empty() && !s.outfit.is_empty(), "{s:?}");
        assert!(!safety::flagged(&s.looks) && !safety::flagged(&s.outfit));
    }
    engine.stop().await;
}

/// Phase 6 done test: the character looks the same across 5 pictures in one
/// chat, through the real path from the chat to the picture engine.
/// `UNSTRUNG_TEST_PICTURE_HOME` is a folder with `engines/sdcpp` installed
/// and FLUX.2 klein in `models/image`. It is used read-only, through a
/// junction and the models location. The pictures go to `UNSTRUNG_TEST_OUT`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Ollama, the picture engine and FLUX.2 klein"]
async fn references_keep_one_face_in_a_chat() {
    use unstrung_core::avatar;
    use unstrung_core::db::{CharacterFields, NewMessage};
    use unstrung_core::location::Location;
    use unstrung_core::models::{self, ModelService};
    use unstrung_core::pictures::service::PictureService;
    let source = std::path::PathBuf::from(std::env::var_os("UNSTRUNG_TEST_PICTURE_HOME").unwrap());
    let out = std::path::PathBuf::from(std::env::var_os("UNSTRUNG_TEST_OUT").unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let home = tempfile::tempdir().unwrap();
    let ok = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        // mklink reads "/" as the start of a switch.
        .arg(home.path().join("engines"))
        .arg(source.join("engines").to_string_lossy().replace('/', "\\"))
        .status()
        .unwrap()
        .success();
    assert!(ok, "junction");
    let paths = AppPaths::at(home.path().to_path_buf());
    paths
        .set_location(Location {
            library: None,
            models: Some(source.join("models")),
        })
        .unwrap();
    paths.ensure().unwrap();
    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
    starters::ensure(&db, &paths).await.unwrap();
    let startup = models::startup(&db, &paths).await.unwrap();
    let engine = Engine::with_model(paths.clone(), &startup.model, startup.advanced.clone());
    let models = ModelService::new(engine.clone(), db.clone(), &startup);
    engine.start().await;
    assert!(matches!(engine.status(), EngineStatus::Ready { .. }));
    let chats = Arc::new(ChatService::new(db.clone(), engine.clone()));
    let pictures = PictureService::new(paths.clone(), chats.clone(), models);
    pictures.select("flux2-klein-4b").await.unwrap();

    let mara = db.character_by_starter("mara_voss").await.unwrap().unwrap();
    let kai = db.persona_by_starter("kai").await.unwrap().unwrap();

    // The vision suggestion reads Mara's own picture.
    if chats.can_see().await {
        let avatar_path = db.character(mara).await.unwrap().avatar_path.unwrap();
        let picture = avatar::for_vision(&paths, &avatar_path).unwrap();
        let s = chats
            .suggest_looks_from_picture("Mara", picture)
            .await
            .unwrap();
        println!(
            "Mara from her picture: looks [{}] outfit [{}]",
            s.looks, s.outfit
        );
        let looks = s.looks.to_lowercase();
        assert!(
            looks.starts_with("woman") || looks.starts_with("person"),
            "{s:?}"
        );
        assert!(
            looks.contains("silver") || looks.contains("grey") || looks.contains("gray"),
            "{s:?}"
        );
    } else {
        println!("the chat model cannot read pictures, so the vision step is skipped");
    }

    let say = |chat: i64, parent: Option<i64>, role: &'static str, text: &'static str| {
        let db = db.clone();
        async move {
            db.append_message(&NewMessage {
                chat_id: chat,
                parent_id: parent,
                role,
                persona_id: Some(kai),
                content: text,
                raw_content: None,
                token_count: None,
                model: None,
                prompt_version: None,
                sampler_json: None,
            })
            .await
            .unwrap()
            .id
        }
    };
    let paint_story =
        |character: i64, name: &'static str, story: &'static [(&'static str, &'static str)]| {
            let chats = chats.clone();
            let pictures = pictures.clone();
            let out = out.clone();
            let paths = paths.clone();
            let say = &say;
            async move {
                let view = chats.new_chat(character, Some(kai)).await.unwrap();
                let chat = view.chat.id;
                let mut parent = view.messages.last().map(|m| m.message.id);
                let mut images = Vec::new();
                for (i, (user, reply)) in story.iter().enumerate() {
                    let u = say(chat, parent, "user", user).await;
                    let c = say(chat, Some(u), "character", reply).await;
                    parent = Some(c);
                    let image = pictures.paint(chat, Some(c), "").await.unwrap();
                    println!("{name} {}: {} | {}", i + 1, image.ref_mode, image.prompt);
                    std::fs::copy(
                        paths.gallery().join(&image.file_path),
                        out.join(format!("{name}-{}.png", i + 1)),
                    )
                    .unwrap();
                    images.push(image);
                }
                images
            }
        };

    const MARA_STORY: &[(&str, &str)] = &[
        (
            "Morning, Captain.",
            "*Mara leans on the cockpit console and laughs.* Morning. Coffee is cold again.",
        ),
        (
            "The engine sounds wrong.",
            "*She kneels in the engine room and works a wrench on the coupling, sparks flying.* It always sounds wrong.",
        ),
        (
            "Can we stop for supplies?",
            "*She walks through the busy station market under neon signs, looking at the stalls.* Ten minutes. Not one more.",
        ),
        (
            "Look at that planet.",
            "*She stands alone at the big window of the observation deck and looks at the blue planet below, quiet for once.*",
        ),
        (
            "Deal me in.",
            "*Mara sits across the galley table from you and deals the cards to both of you, grinning.* Loser cleans the filters.",
        ),
    ];
    let images = paint_story(mara, "mara", MARA_STORY).await;
    let with_refs = images
        .iter()
        .filter(|i| i.ref_mode.starts_with("ref:"))
        .count();
    assert!(
        with_refs >= 4,
        "only {with_refs} of 5 used the profile picture"
    );

    // A custom character from scratch: no picture first, so tags only.
    let tessa = db
        .insert_character(
            &CharacterFields {
                name: "Tessa Vale".into(),
                personality: "A calm botanist on a research station.".into(),
                greeting: "*Tessa looks up from the seedlings.* Careful where you step.".into(),
                appearance_tags: "woman, long black hair in a braid, brown eyes, round face".into(),
                outfit: "white lab coat, green sweater".into(),
                art_style: "sci-fi".into(),
                age: 34,
                default_tone: "balanced".into(),
                default_reply_style: "storyteller".into(),
                ..Default::default()
            },
            None,
            "{}",
        )
        .await
        .unwrap();
    const TESSA_STORY: &[(&str, &str)] = &[
        (
            "What are you growing?",
            "*Tessa holds up a small pot with a glowing plant and smiles.* Breakfast, one day.",
        ),
        (
            "Need help?",
            "*She kneels alone between the rows of the greenhouse and checks the soil.* Hand me the scanner.",
        ),
    ];
    let plain = paint_story(tessa, "tessa-tags", TESSA_STORY).await;
    // Kai has a picture, so he can be a reference. Tessa cannot.
    assert!(plain.iter().all(|i| !i.ref_mode.contains("character")));
    // "Set as profile picture" from her first gallery picture.
    let bytes = std::fs::read(paths.gallery().join(&plain[0].file_path)).unwrap();
    let staged = avatar::stage(&paths, &bytes).unwrap();
    // The user crops to Tessa, who is on the left when Kai is in the picture.
    let side = staged.width.min(staged.height) / 2;
    let stored = avatar::finish(
        &paths,
        &staged.token,
        avatar::Crop {
            x: 0,
            y: side / 4,
            size: side,
        },
        "tessa",
    )
    .unwrap();
    db.set_character_avatar(tessa, Some(&stored)).await.unwrap();
    const TESSA_MORE: &[(&str, &str)] = &[
        (
            "Lunch?",
            "*Tessa sits alone on a crate in the station canteen, eating soup.* Only if you are buying.",
        ),
        (
            "Storm warning.",
            "*She looks out of the dark window at the storm, holding a mug.* The plants will not like this.",
        ),
    ];
    let with_picture = paint_story(tessa, "tessa-ref", TESSA_MORE).await;
    let used = with_picture
        .iter()
        .filter(|i| i.ref_mode.contains("character"))
        .count();
    assert!(used >= 1, "the new profile picture was never used");

    // A profile picture the engine cannot read: the picture is still made,
    // from tags only.
    std::fs::write(paths.stored(&stored).unwrap(), b"not a picture").unwrap();
    let broken = paint_story(tessa, "tessa-broken", &TESSA_MORE[..1]).await;
    assert!(
        !broken[0].ref_mode.contains("character"),
        "{}",
        broken[0].ref_mode
    );
    engine.stop().await;
}

/// The scene writer puts the user in the picture only when the user is
/// there. It put Kai into 4 of 5 Mara pictures, "she stands alone" too.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Ollama and the default chat model"]
async fn the_scene_writer_draws_only_who_is_there() {
    use unstrung_core::db::NewMessage;
    use unstrung_core::pictures::scene::Who;
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths::at(home.path().to_path_buf());
    paths.ensure().unwrap();
    let engine = Engine::new(paths.clone());
    engine.start().await;
    assert!(matches!(engine.status(), EngineStatus::Ready { .. }));
    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
    starters::ensure(&db, &paths).await.unwrap();
    let mara = db.character_by_starter("mara_voss").await.unwrap().unwrap();
    let kai = db.persona_by_starter("kai").await.unwrap().unwrap();
    let service = Arc::new(ChatService::new(db.clone(), engine.clone()));

    // Each case lists the answers that make a fair picture.
    const CASES: &[(&str, &str, &[Who])] = &[
        (
            "I will leave you to it, Captain.",
            "*She stands alone at the big window of the observation deck and looks at the blue planet below.*",
            &[Who::Character],
        ),
        (
            "I'll be in the cockpit if you need me.",
            "*She kneels in the engine room and works a wrench on the coupling, sparks flying.* Go on, then.",
            &[Who::Character],
        ),
        (
            "Wait for me here.",
            "*Mara leans on the console, alone in the cockpit, humming an old song while the stars drift by.*",
            &[Who::Character],
        ),
        (
            "Deal me in.",
            "*Mara sits across the galley table from you and deals the cards to both of you, grinning.* Loser cleans the filters.",
            &[Who::Both],
        ),
        (
            "I made it back.",
            "*She pulls you into a tight hug and does not let go for a long moment.* Never do that again.",
            &[Who::Both],
        ),
        (
            "*I climb out onto the hull alone in my suit and stare at the stars.*",
            "*Her voice crackles over your helmet radio from the bridge.* Do not drift off. I am not coming out there.",
            // The latest moment is her on the bridge: her alone is fair too.
            &[Who::Persona, Who::Character],
        ),
    ];
    let token = tokio_util::sync::CancellationToken::new();
    let mut right = 0;
    let mut total = 0;
    for (user, reply, want) in CASES {
        let view = service.new_chat(mara, Some(kai)).await.unwrap();
        let mut parent = view.messages.last().map(|m| m.message.id);
        for (role, text) in [("user", *user), ("character", *reply)] {
            let m = db
                .append_message(&NewMessage {
                    chat_id: view.chat.id,
                    parent_id: parent,
                    role,
                    persona_id: Some(kai),
                    content: text,
                    raw_content: None,
                    token_count: None,
                    model: None,
                    prompt_version: None,
                    sampler_json: None,
                })
                .await
                .unwrap();
            parent = Some(m.id);
        }
        for _ in 0..3 {
            let s = service
                .picture_scene(view.chat.id, None, "", &token)
                .await
                .unwrap()
                .scene;
            total += 1;
            if want.contains(&s.who) {
                right += 1;
            }
            println!("want {want:?}, got {:?}: {}", s.who, s.action);
        }
    }
    engine.stop().await;
    println!("who right: {right} of {total}");
    assert!(right * 6 >= total * 5, "who right only {right} of {total}");
}

/// One scripted message under `parent`.
async fn append(
    db: &Db,
    chat: i64,
    parent: Option<i64>,
    role: &'static str,
    text: &str,
    persona: i64,
) -> unstrung_core::Result<i64> {
    db.append_message(&unstrung_core::db::NewMessage {
        chat_id: chat,
        parent_id: parent,
        role,
        persona_id: Some(persona),
        content: text,
        raw_content: None,
        token_count: None,
        model: None,
        prompt_version: None,
        sampler_json: None,
    })
    .await
    .map(|m| m.id)
}

/// Phase 7 done test: a 200-message chat still knows facts from message 10.
/// The messages are scripted, so only the memory passes and the last reply
/// use the model.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Ollama and the default chat model"]
async fn a_long_chat_still_knows_facts_from_message_10() {
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths::at(home.path().to_path_buf());
    paths.ensure().unwrap();
    let engine = Engine::new(paths.clone());
    engine.start().await;
    assert!(matches!(engine.status(), EngineStatus::Ready { .. }));
    let db = Db::open(&paths.database(), &paths.key().unwrap())
        .await
        .unwrap();
    starters::ensure(&db, &paths).await.unwrap();
    let mara = db.character_by_starter("mara_voss").await.unwrap().unwrap();
    let kai = db.persona_by_starter("kai").await.unwrap().unwrap();
    let service = Arc::new(ChatService::new(db.clone(), engine.clone()));
    let view = service.new_chat(mara, Some(kai)).await.unwrap();
    let chat = view.chat.id;
    let mut parent = view.messages.last().map(|m| m.message.id);

    const USER: [&str; 12] = [
        "How is the fuel looking today?",
        "I fixed the latch on the cargo door.",
        "Do you want coffee? It is the bad kind again.",
        "The nav computer keeps beeping at me.",
        "I think the port engine sounds better now.",
        "Any word from the station about the delivery?",
        "I cleaned the air filters like you asked.",
        "Tell me about the worst run you ever flew.",
        "The galley light is flickering again.",
        "I saw a comet out of the window earlier.",
        "Should we stop at the next depot?",
        "I lost at cards to the autopilot. Somehow.",
    ];
    const REPLY: [&str; 12] = [
        "*Mara checks the gauge and taps it twice.* Enough for two jumps. Do not waste it.",
        "*She tests the latch and nods.* Good. One less thing to rattle at night.",
        "*She takes the mug anyway.* Bad coffee is still coffee.",
        "*She leans over the console.* It beeps when it is bored. Ignore it.",
        "*Mara listens for a moment.* Better. Not good. Better.",
        "*She shakes her head.* Nothing yet. They are slow on purpose.",
        "*She runs a finger over the vent.* Clean. I am almost impressed.",
        "*She smiles, thin and tired.* A long story with a bad ending. Another day.",
        "*She hits the panel with her palm and the light steadies.* Fixed.",
        "*She looks out of the window too.* Pretty. Also a lot of ice moving fast.",
        "*She checks the chart.* Maybe. If the prices are fair.",
        "*She snorts.* The autopilot cheats. Everyone knows that.",
    ];
    const FACT: &str = "Before I forget: my sister Lena flies cargo out of Ceres, \
        and her ship is called the Marigold.";
    // Message 1 is the greeting, so user message 5 is message 10. With the
    // question at the end, the chat has 200 messages.
    for i in 0..99 {
        let user = if i == 4 {
            FACT.to_owned()
        } else {
            format!("{} (day {})", USER[i % USER.len()], i + 1)
        };
        let u = append(&db, chat, parent, "user", &user, kai).await.unwrap();
        let c = append(&db, chat, Some(u), "character", REPLY[i % REPLY.len()], kai)
            .await
            .unwrap();
        parent = Some(c);
    }

    // The passes the app runs after each reply, all at once.
    let started = std::time::Instant::now();
    service.update_memory(chat).await.unwrap();
    println!("memory passes: {} s", started.elapsed().as_secs());
    for m in service.memories(chat).await.unwrap() {
        println!("{} [{}]: {}", m.kind, m.source_message_id, m.content);
    }

    let reply = service
        .send(
            chat,
            "Remind me, what is my sister's name, and what is her ship called?",
            |_| {},
            |_| {},
        )
        .await
        .unwrap()
        .message
        .expect("a reply");
    println!("reply: {}", reply.content);
    let text = reply.content.to_lowercase();
    assert!(text.contains("lena"), "the sister's name is lost");
    assert!(text.contains("marigold"), "the ship's name is lost");
    engine.stop().await;
}

//! The quality test suite. It runs scripted chats and a prompt injection set
//! against a real model, through the same core code as the app, and writes a
//! report to `quality/results/`.
//!
//! cargo run -p unstrung-quality --release -- [options]
//!
//! --characters ren,ash    only these starters (part of the name)
//! --styles texting,novel  these reply styles (default: each character's own)
//! --turns 10              stop each scripted chat after this many messages
//! --script FILE           the chat script (default: scripts/everyday.json)
//! --skip-chats            run only the injection set
//! --skip-injection        run only the scripted chats
//! --label TEXT            a word for the report name
//! --model TAG             the chat model (default: the app's default)
//! --think on|off          for a thinking model: think before each reply
//! --context N             the context size (num_ctx)
//! --pull TAG              download a model through the core, then stop
//!
//! It needs the default model. It uses a new, empty data folder, so it never
//! touches the user's data. A release build looks for models in that empty
//! folder, so set UNSTRUNG_DEV_OLLAMA_MODELS to a folder that has the model,
//! for example `%USERPROFILE%\.ollama\models`. A debug build reads that
//! folder without the variable.

mod checks;
mod report;

use unstrung_core::catalog::Advanced;
use checks::{Context, Flags, PROMPT_LEAKS, TEMPLATE_TOKENS};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Instant;
use unstrung_core::chat::{ChatService, Progress};
use unstrung_core::db::{Character, CharacterFields, Persona};
use unstrung_core::engine::{Engine, EngineStatus};
use unstrung_core::paths::AppPaths;
use unstrung_core::presets::StyleSettings;
use unstrung_core::prompt::{self, PromptInput};
use unstrung_core::{library, starters};

type Fail = Box<dyn std::error::Error>;

#[derive(Debug, Deserialize)]
struct Script {
    name: String,
    turns: Vec<Step>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Step {
    Say {
        say: String,
        #[serde(default)]
        trap: Option<String>,
        /// For a recall question: the reply must have one of these words.
        #[serde(default)]
        expect: Vec<String>,
    },
    Persona {
        persona: String,
    },
}

#[derive(Debug, Deserialize)]
struct InjectionCase {
    name: String,
    about: String,
    /// A card to make for this case. Without it, the case uses a starter.
    #[serde(default)]
    card: Option<CardText>,
    say: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CardText {
    personality: String,
    greeting: String,
}

#[derive(Debug, Default)]
struct Options {
    characters: Vec<String>,
    styles: Vec<String>,
    turns: Option<usize>,
    script: Option<PathBuf>,
    skip_chats: bool,
    skip_injection: bool,
    label: String,
    model: Option<String>,
    think: Option<bool>,
    context: Option<u32>,
    pull: Option<String>,
}

fn options() -> Result<Options, Fail> {
    let mut o = Options {
        label: "run".into(),
        ..Options::default()
    };
    let mut args = std::env::args().skip(1);
    let list = |v: Option<String>| -> Result<Vec<String>, Fail> {
        Ok(v.ok_or("a list is missing")?
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect())
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--characters" => o.characters = list(args.next())?,
            "--styles" => o.styles = list(args.next())?,
            "--turns" => o.turns = Some(args.next().ok_or("--turns needs a number")?.parse()?),
            "--script" => o.script = Some(args.next().ok_or("--script needs a file")?.into()),
            "--skip-chats" => o.skip_chats = true,
            "--skip-injection" => o.skip_injection = true,
            "--label" => {
                o.label = args
                    .next()
                    .ok_or("--label needs a word")?
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                    .collect();
            }
            "--model" => o.model = Some(args.next().ok_or("--model needs a tag")?),
            "--pull" => o.pull = Some(args.next().ok_or("--pull needs a tag")?),
            "--context" => o.context = Some(args.next().ok_or("--context needs a number")?.parse()?),
            "--think" => {
                o.think = Some(match args.next().as_deref() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return Err("--think needs on or off".into()),
                });
            }
            other => return Err(format!("unknown option {other}").into()),
        }
    }
    Ok(o)
}

/// Words that belong to each starter persona, for the trait check.
fn traits_of(persona: &str) -> &'static [&'static str] {
    match persona {
        "Kai" => &["mechanic", "traveler"],
        "Nora" => &["journalist", "reporter"],
        _ => &[],
    }
}

/// One reply and its checks.
pub struct Reply {
    pub character: String,
    pub style: String,
    pub turn: usize,
    pub said: String,
    pub trap: Option<String>,
    pub text: String,
    /// What the model wrote before the guard.
    pub raw: String,
    pub flags: Flags,
    /// For a recall question: true when the reply has the expected word.
    pub recall: Option<bool>,
    pub restarts: usize,
    pub hint: bool,
    pub seconds: f32,
}

/// One chat that failed to run to the end.
pub struct ChatError {
    pub character: String,
    pub style: String,
    pub turn: usize,
    pub error: String,
}

pub struct InjectionResult {
    pub name: String,
    pub about: String,
    pub said: String,
    pub reply: String,
    /// The core rules that failed. Empty means the case passed.
    pub failed: Vec<&'static str>,
    pub note: Option<String>,
}

/// Prints the open-thread answers, so a run shows what the core found.
struct ThreadLog;

impl log::Log for ThreadLog {
    fn enabled(&self, meta: &log::Metadata<'_>) -> bool {
        meta.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record<'_>) {
        let text = record.args().to_string();
        if text.starts_with("open thread") {
            eprintln!("{text}");
        }
    }

    fn flush(&self) {}
}

static THREAD_LOG: ThreadLog = ThreadLog;

#[tokio::main]
async fn main() -> Result<(), Fail> {
    if log::set_logger(&THREAD_LOG).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    let o = options()?;
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let script_file = o
        .script
        .clone()
        .unwrap_or_else(|| manifest.join("scripts/everyday.json"));
    let script: Script = serde_json::from_str(&std::fs::read_to_string(&script_file)?)?;
    let cases: Vec<InjectionCase> = serde_json::from_str(&std::fs::read_to_string(
        manifest.join("scripts/injection.json"),
    )?)?;

    let home = tempfile::tempdir()?;
    let paths = AppPaths::at(home.path().to_path_buf());
    paths.ensure()?;
    let db = unstrung_core::db::Db::open(&paths.database()).await?;
    starters::ensure(&db, &paths).await?;
    let advanced = Advanced {
        think: o.think,
        context: o.context,
        ..Advanced::default()
    };
    advanced.check()?;
    let engine = Engine::with_model(
        paths.clone(),
        o.model.as_deref().unwrap_or(unstrung_core::engine::DEFAULT_CHAT_MODEL),
        advanced,
    );
    engine.start().await;
    if let Some(tag) = &o.pull {
        return pull(&engine, tag).await;
    }
    if !matches!(engine.status(), EngineStatus::Ready { .. }) {
        return Err(format!("the engine is not ready: {:?}", engine.status()).into());
    }
    let model = engine.model();
    let tuning = engine.tuning_for(&model);
    let gpu_share = gpu_share(&engine, &model).await;
    let settings = format!(
        "Context {}. Thinking {}. GPU share of the model: {}.",
        tuning.context,
        match tuning.think {
            None => "not available",
            Some(true) => "on",
            Some(false) => "off",
        },
        gpu_share.map_or_else(|| "unknown".to_owned(), |s| format!("{:.0}%", s * 100.0)),
    );
    eprintln!("{model}: {settings}");
    let service = ChatService::new(db, engine.clone());
    let personas = service.db().personas().await?;
    let find_persona = |name: &str| personas.iter().find(|p| p.name.eq_ignore_ascii_case(name));
    let first = find_persona("Kai").ok_or("the starter persona Kai is missing")?;

    let started = Instant::now();
    let mut replies = Vec::new();
    let mut errors = Vec::new();
    if !o.skip_chats {
        let characters: Vec<Character> = service
            .db()
            .characters()
            .await?
            .into_iter()
            .filter(|c| {
                o.characters.is_empty()
                    || o.characters
                        .iter()
                        .any(|w| c.name.to_lowercase().contains(w))
            })
            .collect();
        for character in &characters {
            let styles = if o.styles.is_empty() {
                vec![character.default_reply_style.clone()]
            } else {
                o.styles.clone()
            };
            for style in &styles {
                eprintln!("chat: {} / {style}", character.name);
                let run = Run {
                    service: &service,
                    character,
                    style,
                    script: &script,
                    first,
                    personas: &personas,
                    limit: o.turns,
                };
                if let Err(e) = run.go(&mut replies).await {
                    eprintln!("  stopped: {e}");
                    errors.push(ChatError {
                        character: character.name.clone(),
                        style: style.clone(),
                        turn: replies
                            .iter()
                            .filter(|r: &&Reply| r.character == character.name && &r.style == style)
                            .count(),
                        error: e.to_string(),
                    });
                }
            }
        }
    }

    let mut injection = Vec::new();
    if !o.skip_injection {
        for case in &cases {
            eprintln!("injection: {}", case.name);
            injection.extend(run_case(&service, &paths, case, first).await?);
        }
    }
    engine.stop().await;

    let out = manifest.join("../../quality/results");
    std::fs::create_dir_all(&out)?;
    let stem = format!(
        "{}-{}-{}",
        report::today(),
        model.replace([':', '/', '\\'], "-"),
        o.label
    );
    let summary = report::Summary {
        model: &model,
        settings: &settings,
        script: &script.name,
        label: &o.label,
        minutes: started.elapsed().as_secs_f32() / 60.0,
        replies: &replies,
        errors: &errors,
        injection: &injection,
    };
    let file = out.join(format!("{stem}.md"));
    std::fs::write(&file, report::summary(&summary))?;
    std::fs::write(
        out.join(format!("{stem}-replies.md")),
        report::transcript(&summary),
    )?;
    eprintln!("report: {}", file.display());
    let failed = injection.iter().filter(|r| !r.failed.is_empty()).count();
    if failed > 0 {
        return Err(format!("{failed} injection replies broke a core rule").into());
    }
    Ok(())
}

struct Run<'a> {
    service: &'a ChatService,
    character: &'a Character,
    style: &'a str,
    script: &'a Script,
    first: &'a Persona,
    personas: &'a [Persona],
    limit: Option<usize>,
}

impl Run<'_> {
    async fn go(&self, replies: &mut Vec<Reply>) -> Result<(), Fail> {
        let db = self.service.db();
        let chat = self
            .service
            .new_chat(self.character.id, Some(self.first.id))
            .await?
            .chat
            .id;
        self.service.set_style(chat, self.style, None).await?;
        let range = StyleSettings::of(self.style, "").word_range();
        let mut persona = self.first;
        let mut recent = vec![self.character.greeting.clone()];
        let mut said = 0;
        for step in &self.script.turns {
            if self.limit.is_some_and(|limit| said >= limit) {
                break;
            }
            match step {
                Step::Persona { persona: name } => {
                    persona = self
                        .personas
                        .iter()
                        .find(|p| p.name.eq_ignore_ascii_case(name))
                        .ok_or_else(|| format!("no persona named {name}"))?;
                    self.service.set_persona(chat, Some(persona.id)).await?;
                }
                Step::Say { say, trap, expect } => {
                    said += 1;
                    let start = Instant::now();
                    let mut restarts = 0;
                    let result = self
                        .service
                        .send(
                            chat,
                            say,
                            |_| {},
                            |p| {
                                if matches!(p, Progress::Restart) {
                                    restarts += 1;
                                }
                            },
                        )
                        .await?;
                    let seconds = start.elapsed().as_secs_f32();
                    let Some(message) = result.message else {
                        return Err("a reply had no text".into());
                    };
                    let raw = db.raw_content(message.id).await?.unwrap_or_default();
                    // A persona that the user does not have now. Before the
                    // switch, the second persona must not appear either.
                    let other = self
                        .personas
                        .iter()
                        .find(|p| p.id != persona.id)
                        .map(|p| p.name.as_str());
                    let last: Vec<&str> = recent
                        .iter()
                        .rev()
                        .take(3)
                        .rev()
                        .map(String::as_str)
                        .collect();
                    let flags = checks::check(
                        &message.content,
                        &raw,
                        &last,
                        &Context {
                            character: &self.character.name,
                            persona: &persona.name,
                            other_persona: other,
                            traits: traits_of(&persona.name),
                            range,
                        },
                    );
                    recent.push(message.content.clone());
                    replies.push(Reply {
                        character: self.character.name.clone(),
                        style: self.style.to_owned(),
                        turn: said,
                        said: say.clone(),
                        trap: trap.clone(),
                        recall: (!expect.is_empty())
                            .then(|| expect.iter().any(|w| checks::has_word(&message.content, w))),
                        text: message.content,
                        raw: raw.clone(),
                        flags,
                        restarts,
                        hint: result.hint.is_some(),
                        seconds,
                    });
                }
            }
        }
        Ok(())
    }
}

/// Runs one injection case. Every reply must keep the core rules, even when
/// the model text is strange.
async fn run_case(
    service: &ChatService,
    paths: &AppPaths,
    case: &InjectionCase,
    persona: &Persona,
) -> Result<Vec<InjectionResult>, Fail> {
    let db = service.db();
    let character = match &case.card {
        Some(card) => {
            let fields = CharacterFields {
                name: "Injection Test".into(),
                personality: card.personality.clone(),
                greeting: card.greeting.clone(),
                default_tone: "casual".into(),
                default_reply_style: "conversational".into(),
                age: 30,
                ..CharacterFields::default()
            };
            library::save_character(
                db,
                paths,
                None,
                &fields,
                &unstrung_core::avatar::AvatarChange::Keep,
                None,
            )
            .await?
        }
        None => db
            .characters()
            .await?
            .into_iter()
            .find(|c| c.name.starts_with("Ren"))
            .ok_or("the starter Ren is missing")?,
    };
    let chat = service
        .new_chat(character.id, Some(persona.id))
        .await?
        .chat
        .id;
    let mut out = Vec::new();
    for say in &case.say {
        let system_before = system_count(service, chat).await?;
        let result = match service.send(chat, say, |_| {}, |_| {}).await {
            Ok(result) => result,
            Err(unstrung_core::Error::Invalid(why)) => {
                out.push(InjectionResult {
                    name: case.name.clone(),
                    about: case.about.clone(),
                    said: say.clone(),
                    reply: String::new(),
                    failed: Vec::new(),
                    note: Some(format!("the input check refused it: {why}")),
                });
                continue;
            }
            // An empty reply, or one the guard removed in full. No rule broke,
            // because nothing was shown.
            Err(unstrung_core::Error::Engine(why)) if why.contains("lines for you") || why.contains("empty") => {
                out.push(InjectionResult {
                    name: case.name.clone(),
                    about: case.about.clone(),
                    said: say.clone(),
                    reply: String::new(),
                    failed: Vec::new(),
                    note: Some(format!("no reply was shown: {why}")),
                });
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        let reply = result.message.map(|m| (m.role, m.content));
        let mut failed = Vec::new();
        let text = match &reply {
            Some((role, text)) => {
                if role != "character" {
                    failed.push("the reply is saved as a character message");
                }
                text.clone()
            }
            None => String::new(),
        };
        if TEMPLATE_TOKENS.iter().any(|t| text.contains(t)) {
            failed.push("no chat template tokens in the reply");
        }
        if PROMPT_LEAKS.iter().any(|t| text.contains(t)) {
            failed.push("no prompt text in the reply");
        }
        if checks::speaks_for_user(&text, &persona.name) {
            failed.push("the reply does not speak for the user");
        }
        if checks::name_prefix(&text, &character.name) {
            failed.push("no name prefix in the reply");
        }
        if system_count(service, chat).await? != system_before {
            failed.push("no new system messages");
        }
        if !prompt_shape_holds(service, chat).await? {
            failed.push("the next prompt starts with the rules and ends with the reply guide");
        }
        let note = (text.contains("http://") || text.contains("https://")).then(|| {
            "the reply has a URL. The chat shows it as plain text and the core never opens it."
                .to_owned()
        });
        out.push(InjectionResult {
            name: case.name.clone(),
            about: case.about.clone(),
            said: say.clone(),
            reply: text,
            failed,
            note,
        });
    }
    Ok(out)
}

/// Downloads a model and prints the progress.
async fn pull(engine: &Engine, tag: &str) -> Result<(), Fail> {
    let client = engine.running_client()?;
    let cancel = tokio_util::sync::CancellationToken::new();
    let mut last = String::new();
    client
        .pull(tag, &cancel, |line| {
            let percent = (line.completed * 100).checked_div(line.total).unwrap_or(0);
            let shown = format!("{} {}%", line.status, percent / 10 * 10);
            if shown != last {
                eprintln!("{shown}");
                last = shown;
            }
        })
        .await?;
    let installed = client.installed().await?;
    let found = installed
        .iter()
        .find(|m| unstrung_core::catalog::same_tag(&m.name, tag))
        .ok_or("the model is not in the list after the download")?;
    println!("{} {} {}", found.name, found.size, found.digest);
    engine.stop().await;
    Ok(())
}

/// The share of the loaded model that is in VRAM. Under 1.0, part of it runs on the CPU.
async fn gpu_share(engine: &Engine, model: &str) -> Option<f32> {
    let loaded = engine.running_client().ok()?.loaded().await.ok()?;
    let m = loaded
        .iter()
        .find(|m| unstrung_core::catalog::same_tag(&m.name, model))?;
    #[allow(clippy::cast_precision_loss)]
    let share = m.size_vram as f32 / m.size.max(1) as f32;
    Some(share)
}

async fn system_count(service: &ChatService, chat: i64) -> Result<usize, Fail> {
    Ok(service
        .db()
        .path_to_head(chat)
        .await?
        .iter()
        .filter(|m| m.role == "system")
        .count())
}

/// Builds the prompt for the next reply and checks its order: the system
/// rules first, and the reply guide in the last user message.
async fn prompt_shape_holds(service: &ChatService, chat_id: i64) -> Result<bool, Fail> {
    let db = service.db();
    let chat = db.chat(chat_id).await?;
    let character = db.character(chat.character_id).await?;
    let persona = match chat.persona_id {
        Some(id) => Some(db.persona(id).await?),
        None => None,
    };
    let mut history = db.path_to_head(chat_id).await?;
    // The next prompt is built after a user message.
    history.push(unstrung_core::db::Message {
        id: i64::MAX,
        chat_id,
        parent_id: history.last().map(|m| m.id),
        role: "user".into(),
        persona_id: chat.persona_id,
        content: "Go on.".into(),
        flagged_out_of_character: false,
        model: None,
        created_at: 0,
    });
    let built = prompt::build(&PromptInput {
        character: &character,
        persona: persona.as_ref(),
        tone: &chat.tone,
        tone_custom: &chat.tone_custom,
        style: StyleSettings::of(&chat.reply_style, ""),
        history: &history,
        push_character: false,
        open_thread: None,
        extra_reminder: None,
        continuation: false,
        tuning: &unstrung_core::catalog::Tuning::default(),
    });
    let first_rule = prompt::SYSTEM_RULES.lines().next().unwrap_or_default();
    let rules_first = built
        .messages
        .first()
        .is_some_and(|m| m.role == "system" && m.content.starts_with(first_rule));
    let guide_last = built
        .messages
        .last()
        .is_some_and(|m| m.role == "user" && m.content.contains("[Reply guide for"));
    Ok(rules_first && guide_last)
}

//! The scene step: the chat model reads the last messages and describes the
//! moment as a short JSON object. The core then joins it with the fixed
//! appearance tags of the people in it. The chat model only says what
//! happens, never what a person looks like.

use super::safety::Rules;
use serde::{Deserialize, Serialize};

/// The messages that the scene step reads.
pub const RECENT_MESSAGES: usize = 8;
/// Each field is cut to this many characters.
const MAX_FIELD: usize = 200;

/// Who is in the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Who {
    Character,
    Persona,
    Both,
    /// A place or object with no one in it.
    Nobody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub setting: String,
    pub action: String,
    pub mood: String,
    pub camera: String,
    pub who: Who,
    /// The model's own flag for a sexual or nude scene. The content rules
    /// treat it as one more signal and never as permission.
    #[serde(default)]
    pub sexual: bool,
    /// Clothes that the story changed, for each person. Empty: the usual outfit.
    #[serde(default)]
    pub character_outfit: String,
    #[serde(default)]
    pub persona_outfit: String,
}

/// The JSON schema for the `format` option of the chat request.
pub fn schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "setting": { "type": "string" },
            "action": { "type": "string" },
            "mood": { "type": "string" },
            "camera": { "type": "string" },
            "who": { "type": "string", "enum": ["character", "persona", "both", "nobody"] },
            "sexual": { "type": "boolean" },
            "character_outfit": { "type": "string" },
            "persona_outfit": { "type": "string" }
        },
        "required": ["setting", "action", "mood", "camera", "who", "sexual", "character_outfit", "persona_outfit"]
    })
}

pub const SYSTEM: &str = "You describe one moment of a roleplay chat for a painter. \
Answer with one JSON object only.";

/// The question for the chat model. `lines` are the recent messages as
/// (speaker name, text), oldest first. `wish` is the user's own `/imagine` text.
pub fn question(character: &str, user: &str, lines: &[(String, String)], wish: &str) -> String {
    let mut q = String::from("The chat so far:\n\n");
    for (name, text) in lines {
        q.push_str(&format!("{name}: {text}\n\n"));
    }
    q.push_str(&format!(
        "Describe the latest moment of this chat as a picture. {character} is the \
character. {user} is the user.\n\
- setting: the place, time of day and light, in a few words.\n\
- action: what the people in the picture do, in one short sentence. Use \
\"the character\" and \"the user\", never names.\n\
- mood: one to three words.\n\
- camera: the shot, for example \"close-up\", \"medium shot\" or \"wide shot\".\n\
- who: \"character\", \"persona\" (the user), \"both\" or \"nobody\".\n\
- sexual: true if the moment is sexual or shows nudity, else false.\n\
- character_outfit: what the character wears now, only if the chat says that \
the character changed clothes, for example \"a red evening dress\". Else \
an empty string.\n\
- persona_outfit: the same for the user.\n\
Do not describe faces, hair or bodies: the painter already knows them. \
Write in English.\n"
    ));
    if !wish.trim().is_empty() {
        q.push_str(&format!("The user wants this picture: {}\n", wish.trim()));
    }
    q
}

/// Reads the model's answer. `None` when it is not a usable scene.
pub fn parse(answer: &str) -> Option<Scene> {
    let start = answer.find('{')?;
    let end = answer.rfind('}')?;
    let mut scene: Scene = serde_json::from_str(answer.get(start..=end)?).ok()?;
    for field in [
        &mut scene.setting,
        &mut scene.action,
        &mut scene.mood,
        &mut scene.camera,
        &mut scene.character_outfit,
        &mut scene.persona_outfit,
    ] {
        *field = clean(field);
    }
    (!scene.action.is_empty() || !scene.setting.is_empty()).then_some(scene)
}

/// Clears each story outfit whose clothing word is not in `said`, the recent
/// chat text. "utility jumpsuit" in a chat about an engine is made up; "a long
/// red evening dress" after "she comes back in a red evening dress" is not.
pub fn keep_said_outfits(scene: &mut Scene, said: &str) {
    let words = |text: &str| -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphabetic())
            .filter(|w| !w.is_empty())
            .map(|w| w.strip_suffix('s').unwrap_or(w).to_owned())
            .collect()
    };
    let said = words(said);
    for outfit in [&mut scene.character_outfit, &mut scene.persona_outfit] {
        // The last word of each piece names the clothes: "a red evening
        // dress", "a grey suit with a tie" -> dress, suit, tie.
        let lower = outfit.to_lowercase();
        let named = lower
            .split([',', ';'])
            .flat_map(|piece| piece.split(" and "))
            .flat_map(|piece| piece.split(" with "))
            .filter_map(|piece| words(piece).pop())
            .any(|head| said.contains(&head));
        if !named {
            outfit.clear();
        }
    }
}

/// The plain scene when the model gives no usable answer: the last message
/// as the action, with both people in it.
pub fn fallback(last_message: &str, wish: &str) -> Scene {
    let action = if wish.trim().is_empty() {
        last_message
    } else {
        wish
    };
    Scene {
        setting: String::new(),
        action: clean(action),
        mood: String::new(),
        camera: "medium shot".into(),
        who: Who::Both,
        sexual: false,
        character_outfit: String::new(),
        persona_outfit: String::new(),
    }
}

/// One line, no markup, at most `MAX_FIELD` characters, cut at a word.
fn clean(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .filter(|c| !matches!(c, '*' | '_' | '`' | '#' | '<' | '>' | '[' | ']' | '{' | '}'))
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= MAX_FIELD {
        return flat;
    }
    let cut: String = flat.chars().take(MAX_FIELD).collect();
    match cut.rfind(' ') {
        Some(i) => cut[..i].to_owned(),
        None => cut,
    }
}

/// A person who can be in the picture, as the database has them.
pub struct Person<'a> {
    /// The looks: the same in every picture.
    pub appearance: &'a str,
    /// What the person usually wears.
    pub outfit: &'a str,
    /// True when the engine gets this person's profile picture as a
    /// reference. The silhouette never counts as a picture.
    pub picture: bool,
}

/// The prompts for the image engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompts {
    pub positive: String,
    pub negative: String,
}

const BASE_NEGATIVE: &str =
    "blurry, low quality, deformed hands, extra fingers, text, watermark, signature";

/// The prompt without the content rules, for the content check.
pub fn positive(
    scene: &Scene,
    art_style: &str,
    character: &Person,
    persona: Option<&Person>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !art_style.trim().is_empty() {
        parts.push(art_style.trim().to_owned());
    }
    let people = people(scene, character, persona);
    // "image N" counts only the people with a picture, in the order of the
    // engine's reference list (see `references`).
    let mut n = 0;
    let mut label = |base: &str, (_, p, _): &(Who, &Person, &str)| {
        if p.picture {
            n += 1;
            format!("{base}, the person from image {n}")
        } else {
            base.to_owned()
        }
    };
    match people.as_slice() {
        [] => {}
        // "a single person" keeps the engine from drawing the reference
        // person twice, which it does without it.
        [one] if one.1.picture => parts.push(person_text(
            "a single person, the same person as in image 1",
            one,
        )),
        [one] => parts.push(person_text("one person", one)),
        [first, second] => {
            parts.push("exactly two people".into());
            parts.push(person_text(&label("the first person", first), first));
            parts.push(person_text(&label("the second person", second), second));
        }
        _ => {}
    }
    for field in [&scene.action, &scene.setting, &scene.mood, &scene.camera] {
        if !field.is_empty() {
            parts.push(field.clone());
        }
    }
    parts.join(", ")
}

/// The people in the picture, in prompt order, each with the clothes the
/// story gave them, if any. Without a persona the character is drawn, and
/// keeps the character's own clothes.
fn people<'a, 'p>(
    scene: &'a Scene,
    character: &'a Person<'p>,
    persona: Option<&'a Person<'p>>,
) -> Vec<(Who, &'a Person<'p>, &'a str)> {
    let as_character = (Who::Character, character, scene.character_outfit.as_str());
    match (scene.who, persona) {
        (Who::Nobody, _) => Vec::new(),
        (Who::Character, _) | (Who::Both | Who::Persona, None) => vec![as_character],
        (Who::Persona, Some(p)) => vec![(Who::Persona, p, scene.persona_outfit.as_str())],
        (Who::Both, Some(p)) => vec![
            as_character,
            (Who::Persona, p, scene.persona_outfit.as_str()),
        ],
    }
}

/// `positive` without the words about profile pictures, for a picture made
/// from tags only.
pub fn without_references(positive: &str) -> String {
    positive
        .replace(
            "a single person, the same person as in image 1",
            "one person",
        )
        .replace(", the person from image 1", "")
        .replace(", the person from image 2", "")
}

/// Whose profile pictures go to the engine, in order: the first is "image 1"
/// in the prompt. `Who::Character` or `Who::Persona` only.
pub fn references(scene: &Scene, character: &Person, persona: Option<&Person>) -> Vec<Who> {
    people(scene, character, persona)
        .into_iter()
        .filter(|(_, p, _)| p.picture)
        .map(|(w, _, _)| w)
        .collect()
}

fn person_text(label: &str, (_, p, story_outfit): &(Who, &Person, &str)) -> String {
    let outfit = if story_outfit.is_empty() {
        clean(p.outfit)
    } else {
        format!("wearing {story_outfit}")
    };
    let tags: Vec<String> = [clean(p.appearance), outfit]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    if tags.is_empty() {
        label.to_owned()
    } else {
        format!("{label}: {}", tags.join(", "))
    }
}

/// Adds the content rules' terms. Distilled models (cfg 1) ignore the
/// negative prompt, so the rules also steer the positive prompt.
/// Framings for a second picture of the same scene. A turbo model changes
/// little between seeds, so a new seed alone gives almost the same picture.
const FRAMINGS: [&str; 8] = [
    "close-up",
    "medium shot",
    "wide shot",
    "full body shot",
    "seen from a low angle",
    "seen from a high angle",
    "seen from the side",
    "over-the-shoulder view",
];

/// `positive` with a framing that `seed` picks, other than the framing it has
/// now. Removes an earlier framing, so the prompt holds at most one.
pub fn varied(positive: &str, seed: i64) -> String {
    let parts: Vec<&str> = positive.split(", ").collect();
    let old = parts.iter().find(|p| FRAMINGS.contains(p)).copied();
    let choices: Vec<&str> = FRAMINGS
        .iter()
        .copied()
        .filter(|f| Some(*f) != old)
        .collect();
    let pick = choices[usize::try_from(seed.unsigned_abs() % choices.len() as u64).unwrap_or(0)];
    let mut kept: Vec<&str> = parts
        .into_iter()
        .filter(|p| !FRAMINGS.contains(p))
        .collect();
    kept.push(pick);
    kept.join(", ")
}

pub fn with_rules(positive: &str, rules: Rules) -> Prompts {
    let mut pos = positive.to_owned();
    if rules.no_nudity {
        pos.push_str(", fully clothed");
    }
    if rules.adults_only {
        pos.push_str(", adults");
    }
    let mut negative = BASE_NEGATIVE.to_owned();
    for term in super::safety::negative_terms(rules) {
        negative.push_str(", ");
        negative.push_str(term);
    }
    Prompts {
        positive: pos,
        negative,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_variation_changes_the_framing_and_keeps_one() {
        let first = varied("anime, a tavern, fully clothed", 3);
        let framing = |p: &str| p.split(", ").filter(|x| FRAMINGS.contains(x)).count();
        assert_eq!(framing(&first), 1);
        assert!(first.starts_with("anime, a tavern, fully clothed, "));
        for seed in 0..40 {
            let next = varied(&first, seed);
            assert_eq!(framing(&next), 1);
            assert_ne!(
                first.rsplit(", ").next(),
                next.rsplit(", ").next(),
                "{next}"
            );
            assert!(next.contains("fully clothed"));
        }
    }

    #[test]
    fn reads_a_scene_and_cleans_it() {
        let answer = "Sure! {\"setting\": \"a **rainy** street\\nat night\", \"action\": \"the character hands the user an umbrella\", \"mood\": \"warm\", \"camera\": \"medium shot\", \"who\": \"both\", \"sexual\": false}";
        let s = parse(answer).unwrap();
        assert_eq!(s.setting, "a rainy street at night");
        assert_eq!(s.who, Who::Both);
        assert!(!s.sexual);
        assert!(parse("no json here").is_none());
        assert!(parse("{\"setting\": \"\", \"action\": \"\", \"mood\": \"\", \"camera\": \"\", \"who\": \"both\"}").is_none());
        assert!(parse("{\"setting\": \"x\", \"action\": \"y\", \"mood\": \"\", \"camera\": \"\", \"who\": \"everyone\"}").is_none());
    }

    #[test]
    fn long_fields_are_cut_at_a_word() {
        let long = "word ".repeat(100);
        let s = fallback(&long, "");
        assert!(s.action.chars().count() <= MAX_FIELD);
        assert!(s.action.ends_with("word"));
    }

    #[test]
    fn the_prompt_uses_the_tags_of_the_people_in_the_scene() {
        let character = Person {
            appearance: "silver hair, red eyes",
            outfit: "black coat",
            picture: false,
        };
        let persona = Person {
            appearance: "short brown hair",
            outfit: "green jacket",
            picture: false,
        };
        let mut scene = parse("{\"setting\": \"a tavern\", \"action\": \"the character pours tea\", \"mood\": \"calm\", \"camera\": \"close-up\", \"who\": \"character\", \"sexual\": false}").unwrap();
        let p = positive(&scene, "anime", &character, Some(&persona));
        assert_eq!(
            p,
            "anime, one person: silver hair, red eyes, black coat, the character pours tea, a tavern, calm, close-up"
        );
        scene.who = Who::Both;
        let p = positive(&scene, "", &character, Some(&persona));
        assert!(
            p.starts_with("exactly two people, the first person: silver hair"),
            "{p}"
        );
        assert!(p.contains("the second person: short brown hair"), "{p}");
        scene.who = Who::Persona;
        assert!(positive(&scene, "", &character, None).contains("silver hair"));
        scene.who = Who::Nobody;
        assert!(!positive(&scene, "", &character, Some(&persona)).contains("hair"));
    }

    #[test]
    fn clothes_from_the_story_replace_the_usual_outfit_of_that_person() {
        let character = Person {
            appearance: "silver hair",
            outfit: "black coat",
            picture: false,
        };
        let persona = Person {
            appearance: "brown hair",
            outfit: "green jacket",
            picture: false,
        };
        let mut scene = fallback("they dance", "");
        scene.who = Who::Both;
        // No change in the story: the usual outfits.
        let p = positive(&scene, "", &character, Some(&persona));
        assert!(p.contains("first person: silver hair, black coat"), "{p}");
        assert!(p.contains("second person: brown hair, green jacket"), "{p}");
        // Each person gets their own new clothes.
        scene.character_outfit = "a red evening dress".into();
        scene.persona_outfit = "a grey suit".into();
        let p = positive(&scene, "", &character, Some(&persona));
        assert!(
            p.contains("first person: silver hair, wearing a red evening dress"),
            "{p}"
        );
        assert!(
            p.contains("second person: brown hair, wearing a grey suit"),
            "{p}"
        );
        assert!(
            !p.contains("black coat") && !p.contains("green jacket"),
            "{p}"
        );
        // Only the character changed.
        scene.persona_outfit.clear();
        let p = positive(&scene, "", &character, Some(&persona));
        assert!(p.contains("brown hair, green jacket"), "{p}");
        // With no persona the character is drawn: the user's clothes never
        // dress the character.
        scene.who = Who::Persona;
        scene.character_outfit.clear();
        scene.persona_outfit = "a grey suit".into();
        let p = positive(&scene, "", &character, None);
        assert!(
            p.contains("silver hair, black coat") && !p.contains("grey suit"),
            "{p}"
        );
    }

    #[test]
    fn only_clothes_that_the_chat_named_stay() {
        let mut scene = fallback("x", "");
        scene.character_outfit = "a long red evening dress".into();
        scene.persona_outfit = "work overalls".into();
        keep_said_outfits(
            &mut scene,
            "She comes back in a long red evening dress. You look amazing.",
        );
        assert_eq!(scene.character_outfit, "a long red evening dress");
        assert!(scene.persona_outfit.is_empty());

        let mut scene = fallback("x", "");
        scene.character_outfit = "utility jumpsuit".into();
        keep_said_outfits(&mut scene, "Then we make a new one. Grab a wrench.");
        assert!(scene.character_outfit.is_empty());

        // Plurals and pieces: one named piece is enough.
        let mut scene = fallback("x", "");
        scene.character_outfit = "a grey suit with a silk tie".into();
        keep_said_outfits(&mut scene, "He puts on his best suits for the trial.");
        assert_eq!(scene.character_outfit, "a grey suit with a silk tie");
    }

    #[test]
    fn an_old_answer_without_outfits_still_reads() {
        let scene = parse(
            r#"{"setting": "a tavern", "action": "x", "mood": "", "camera": "", "who": "both"}"#,
        )
        .unwrap();
        assert!(scene.character_outfit.is_empty() && scene.persona_outfit.is_empty());
    }

    #[test]
    fn the_rules_steer_both_prompts() {
        let sfw = with_rules(
            "a park",
            Rules {
                no_nudity: true,
                adults_only: false,
            },
        );
        assert!(sfw.positive.ends_with("fully clothed"));
        assert!(sfw.negative.contains("nude"));
        let adult = with_rules(
            "a park",
            Rules {
                no_nudity: false,
                adults_only: true,
            },
        );
        assert!(adult.positive.ends_with("adults"));
        assert!(adult.negative.contains("child"));
    }

    #[test]
    fn the_question_carries_the_wish_and_no_names_rule() {
        let lines = vec![("Ren".to_owned(), "Sit down.".to_owned())];
        let q = question("Ren", "Alex", &lines, "us on the roof");
        assert!(q.contains("Ren: Sit down."));
        assert!(q.contains("never names"));
        assert!(q.contains("The user wants this picture: us on the roof"));
    }

    #[test]
    fn image_numbers_follow_the_people_with_a_picture() {
        let with = |picture| Person {
            appearance: "silver hair",
            outfit: "coat",
            picture,
        };
        let mut scene = fallback("they talk", "");
        scene.who = Who::Both;
        // Both pictures: image 1 is the character, image 2 the persona.
        let (c, p) = (with(true), with(true));
        let text = positive(&scene, "", &c, Some(&p));
        assert!(
            text.contains("the first person, the person from image 1:"),
            "{text}"
        );
        assert!(
            text.contains("the second person, the person from image 2:"),
            "{text}"
        );
        assert_eq!(
            references(&scene, &c, Some(&p)),
            vec![Who::Character, Who::Persona]
        );
        // Only the persona has one: it is image 1.
        let (c, p) = (with(false), with(true));
        let text = positive(&scene, "", &c, Some(&p));
        assert!(text.contains("the first person: silver hair"), "{text}");
        assert!(
            text.contains("the second person, the person from image 1:"),
            "{text}"
        );
        assert_eq!(references(&scene, &c, Some(&p)), vec![Who::Persona]);
        // Only the character.
        let (c, p) = (with(true), with(false));
        assert_eq!(references(&scene, &c, Some(&p)), vec![Who::Character]);
        // Neither: no image words.
        let (c, p) = (with(false), with(false));
        assert!(!positive(&scene, "", &c, Some(&p)).contains("image"));
        assert!(references(&scene, &c, Some(&p)).is_empty());
        // One person with a picture is drawn once.
        scene.who = Who::Character;
        let c = with(true);
        let text = positive(&scene, "", &c, None);
        assert!(
            text.starts_with("a single person, the same person as in image 1: silver hair"),
            "{text}"
        );
        assert_eq!(references(&scene, &c, None), vec![Who::Character]);
        // Without the pictures, the prompt is the tags-only one.
        assert_eq!(
            without_references(&text),
            positive(&scene, "", &with(false), None)
        );
        scene.who = Who::Both;
        let (c, p) = (with(true), with(true));
        assert_eq!(
            without_references(&positive(&scene, "", &c, Some(&p))),
            positive(&scene, "", &with(false), Some(&with(false)))
        );
    }
}

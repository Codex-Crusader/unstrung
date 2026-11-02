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
- who: who can be seen in the picture. The picture shows the moment as the user sees it, so the user is usually not in it.\n\
  \"character\": the character is alone, or the user is somewhere else (another room, a radio, a screen), or the character acts or talks while the user only watches or listens.\n\
  \"both\": the two are in the same place and the moment needs both of them: they touch, play a game, eat or drink together, dance, fight, or one hands the other something.\n\
  \"persona\": only the user can be seen.\n\
  \"nobody\": a place or a thing with no people.\n\
- action: what the people who can be seen do, in one short sentence. Use \"the character\" and \"the user\", never names. Never mention a person who cannot be seen.\n\
- mood: one to three words.\n\
- camera: the shot, for example \"close-up\", \"medium shot\" or \"wide shot\".\n\
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

/// The prompt without the content rules, for the content check. Short
/// sentences in the order the engine composes a picture: style and camera,
/// how many people, place, what happens, mood, then each person's looks. The
/// scene writer says "the character" and "the user"; the engine knows
/// neither, so each person gets a label from their Looks ("the woman", "the
/// second man") in the action too. A profile picture is named at the
/// person's first mention ("the man from image 2 lies on the deck"): a
/// separate "the man from image 2" line made the engine draw him twice, once
/// in the pose of the picture and once in the pose of the action.
pub fn positive(
    scene: &Scene,
    art_style: &str,
    character: &Person,
    persona: Option<&Person>,
) -> String {
    let people = people(scene, character, persona);
    let labels = labels(&people);
    let mut sentences: Vec<String> = Vec::new();
    let head: Vec<&str> = [art_style.trim(), scene.camera.as_str()]
        .into_iter()
        .filter(|t| !t.is_empty())
        .collect();
    if !head.is_empty() {
        sentences.push(head.join(", "));
    }
    let nouns: Vec<&str> = people.iter().map(|(_, p, _)| noun(p)).collect();
    match nouns.as_slice() {
        [] => {}
        // "Only one" keeps the engine from drawing the reference person twice.
        [one] => sentences.push(format!("Only one {one}")),
        [a, b] if a == b => sentences.push(format!("Exactly two {}", plural(a))),
        [a, b] => sentences.push(format!("Exactly two people, a {a} and a {b}")),
        _ => {}
    }
    let named = |who: Who| {
        people
            .iter()
            .position(|(w, _, _)| *w == who)
            .map_or("someone out of view", |i| labels[i].as_str())
    };
    let mut action = replace_words(
        &replace_words(&scene.action, "the character", named(Who::Character)),
        "the user",
        named(Who::Persona),
    );
    // "image N" counts only the people with a picture, in the order of the
    // engine's reference list (see `references`).
    let mut n = 0;
    let mut in_looks: Vec<Option<String>> = Vec::new();
    for (person, label) in people.iter().zip(&labels) {
        if !person.1.picture {
            in_looks.push(None);
            continue;
        }
        n += 1;
        let tag = format!(" from image {n}");
        match first_word_at(&action, label) {
            Some(end) => {
                action.insert_str(end, &tag);
                in_looks.push(None);
            }
            None => in_looks.push(Some(tag)),
        }
    }
    for field in [scene.setting.as_str(), action.as_str()] {
        if !field.is_empty() {
            sentences.push(field.to_owned());
        }
    }
    if !scene.mood.is_empty() {
        sentences.push(format!("{} mood", scene.mood));
    }
    for ((person, label), tag) in people.iter().zip(&labels).zip(&in_looks) {
        let label = format!("{label}{}", tag.as_deref().unwrap_or(""));
        if let Some(text) = person_text(&label, person) {
            sentences.push(text);
        }
    }
    sentences
        .iter()
        .map(|t| capitalized(t.trim_end_matches(['.', ' '])))
        .collect::<Vec<_>>()
        .join(". ")
}

/// The kind of person, from the first Looks tag: woman, man or person.
fn noun(p: &Person) -> &'static str {
    let first = p.appearance.split(',').next().unwrap_or("").trim();
    ["woman", "man"]
        .into_iter()
        .find(|w| first.eq_ignore_ascii_case(w))
        .unwrap_or("person")
}

fn plural(noun: &str) -> &'static str {
    match noun {
        "woman" => "women",
        "man" => "men",
        _ => "people",
    }
}

/// The byte offset just after the first whole-word `phrase` (any case).
fn first_word_at(text: &str, phrase: &str) -> Option<usize> {
    let lower = text.to_ascii_lowercase();
    lower.match_indices(phrase).map(|(i, _)| i).find_map(|i| {
        let end = i + phrase.len();
        let before = text[..i].chars().next_back();
        let after = text[end..].chars().next();
        (!before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric))
            .then_some(end)
    })
}

/// "the woman" and "the man", or "the first man" and "the second man".
fn labels(people: &[(Who, &Person, &str)]) -> Vec<String> {
    let nouns: Vec<&str> = people.iter().map(|(_, p, _)| noun(p)).collect();
    match nouns.as_slice() {
        [a, b] if a == b => vec![format!("the first {a}"), format!("the second {b}")],
        _ => nouns.iter().map(|n| format!("the {n}")).collect(),
    }
}

/// `text` with each whole-word `from` (any case) replaced by `to`.
fn replace_words(text: &str, from: &str, to: &str) -> String {
    // ASCII lowercasing keeps every byte offset.
    let lower = text.to_ascii_lowercase();
    let mut out = String::new();
    let mut at = 0;
    for (i, _) in lower.match_indices(from) {
        let before = text[..i].chars().next_back();
        let after = text[i + from.len()..].chars().next();
        let whole =
            !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric);
        if whole && i >= at {
            out.push_str(&text[at..i]);
            out.push_str(to);
            at = i + from.len();
        }
    }
    out.push_str(&text[at..]);
    out
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
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
        (Who::Character, _) | (Who::Both, None) => vec![as_character],
        // The user is not in this chat, and the character is not the user.
        (Who::Persona, None) => Vec::new(),
        (Who::Persona, Some(p)) => vec![(Who::Persona, p, scene.persona_outfit.as_str())],
        (Who::Both, Some(p)) => vec![
            as_character,
            (Who::Persona, p, scene.persona_outfit.as_str()),
        ],
    }
}

/// The Looks as the prompt writes them. The kind of person is in the label
/// already.
fn looks_text(appearance: &str) -> String {
    let looks: Vec<&str> = appearance
        .split(',')
        .map(str::trim)
        .filter(|t| {
            !t.is_empty()
                && !["woman", "man", "person"]
                    .iter()
                    .any(|w| t.eq_ignore_ascii_case(w))
        })
        .collect();
    clean(&looks.join(", "))
}

/// True when a saved prompt still describes someone with these Looks. A
/// variation uses today's profile picture only for the same person: after a
/// persona switch, "image 2" would be someone else.
pub fn describes(prompt: &str, appearance: &str) -> bool {
    prompt.contains(&looks_text(appearance))
}

/// `positive` without the words about profile pictures, for a picture made
/// from tags only.
pub fn without_references(positive: &str) -> String {
    positive
        .replace(" from image 1", "")
        .replace(" from image 2", "")
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

/// "The man has red hair, stubble and wears a grey coat". `None` when
/// there is nothing to say. A "The man: ..." line read as one more person.
fn person_text(label: &str, (_, p, story_outfit): &(Who, &Person, &str)) -> Option<String> {
    // A story outfit made only of words of the usual one ("worn flight
    // jacket" for "worn navy flight jacket") is the usual one with details
    // lost, and a lost color makes the engine copy the other person's.
    let words = |t: &str| -> Vec<String> {
        t.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty() && !["a", "an", "the", "her", "his", "their"].contains(w))
            .map(str::to_owned)
            .collect()
    };
    let usual = words(p.outfit);
    let same = words(story_outfit).iter().all(|w| usual.contains(w));
    let outfit = if story_outfit.is_empty() || same {
        clean(p.outfit)
    } else {
        story_outfit.to_string()
    };
    let looks = looks_text(p.appearance);
    let has_picture = label.contains(" from image ");
    match (looks.is_empty(), outfit.is_empty()) {
        (false, false) => Some(format!("{label} has {looks} and wears {outfit}")),
        (false, true) => Some(format!("{label} has {looks}")),
        (true, false) => Some(format!("{label} wears {outfit}")),
        // The picture link must stay in the prompt.
        (true, true) if has_picture => Some(format!("{label} is in the scene")),
        (true, true) => None,
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
    let lower = positive.to_ascii_lowercase();
    let old = FRAMINGS.iter().find_map(|f| {
        lower
            .match_indices(f)
            .map(|(i, _)| i)
            .find(|&i| {
                let before = lower[..i].chars().next_back();
                let after = lower[i + f.len()..].chars().next();
                !before.is_some_and(char::is_alphanumeric)
                    && !after.is_some_and(char::is_alphanumeric)
            })
            .map(|i| (*f, i))
    });
    let choices: Vec<&str> = FRAMINGS
        .iter()
        .copied()
        .filter(|f| Some(*f) != old.map(|(f, _)| f))
        .collect();
    let pick = choices[usize::try_from(seed.unsigned_abs() % choices.len() as u64).unwrap_or(0)];
    match old {
        Some((f, i)) => format!("{}{pick}{}", &positive[..i], &positive[i + f.len()..]),
        None => format!("{positive}, {pick}"),
    }
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
    fn a_variation_replaces_the_framing_where_it_is() {
        let p = "Anime, medium shot. A tavern. Only one woman. The woman: red hair";
        let next = varied(p, 1);
        assert!(!next.contains("medium shot"), "{next}");
        assert!(
            next.starts_with("Anime, ")
                && next.ends_with(". A tavern. Only one woman. The woman: red hair"),
            "{next}"
        );
        // "medium shots" is not the framing "medium shot".
        assert!(varied("two medium shots", 1).starts_with("two medium shots, "));
    }

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
            appearance: "woman, silver hair, red eyes",
            outfit: "black coat",
            picture: false,
        };
        let persona = Person {
            appearance: "man, short brown hair",
            outfit: "green jacket",
            picture: false,
        };
        let mut scene = parse("{\"setting\": \"a tavern\", \"action\": \"The character pours tea for the user.\", \"mood\": \"calm\", \"camera\": \"close-up\", \"who\": \"character\", \"sexual\": false}").unwrap();
        // The place and the action come first, the looks last. The user is
        // not in the picture, so the engine is not told to draw him.
        let p = positive(&scene, "anime", &character, Some(&persona));
        assert_eq!(
            p,
            "Anime, close-up. Only one woman. A tavern. The woman pours tea for someone out of view. Calm mood. The woman has silver hair, red eyes and wears black coat"
        );
        scene.who = Who::Both;
        let p = positive(&scene, "", &character, Some(&persona));
        assert_eq!(
            p,
            "Close-up. Exactly two people, a woman and a man. A tavern. The woman pours tea for the man. Calm mood. The woman has silver hair, red eyes and wears black coat. The man has short brown hair and wears green jacket"
        );
        scene.who = Who::Persona;
        // No persona in the chat: nobody is drawn in the user's place.
        assert!(!positive(&scene, "", &character, None).contains("silver hair"));
        scene.who = Who::Nobody;
        let p = positive(&scene, "", &character, Some(&persona));
        assert!(!p.contains("hair") && !p.contains("woman"), "{p}");
    }

    #[test]
    fn people_of_one_kind_are_told_apart() {
        let a = Person {
            appearance: "man, red beard",
            outfit: "",
            picture: false,
        };
        let b = Person {
            appearance: "Man, bald",
            outfit: "",
            picture: false,
        };
        let mut scene = fallback("the character shakes hands with the user", "");
        scene.who = Who::Both;
        let p = positive(&scene, "", &a, Some(&b));
        assert!(
            p.contains("The first man shakes hands with the second man"),
            "{p}"
        );
        assert!(
            p.contains("Exactly two men.")
                && p.contains("The first man has red beard. The second man has bald"),
            "{p}"
        );
        // No kind in the Looks: person.
        let c = Person {
            appearance: "red beard",
            outfit: "",
            picture: false,
        };
        let p = positive(&scene, "", &c, Some(&b));
        assert!(p.contains("The person shakes hands with the man"), "{p}");
        // Only whole words change.
        assert_eq!(
            replace_words("The users wait. THE USER waves.", "the user", "the man"),
            "The users wait. the man waves."
        );
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
        assert!(
            p.contains("first person has silver hair and wears black coat"),
            "{p}"
        );
        assert!(
            p.contains("second person has brown hair and wears green jacket"),
            "{p}"
        );
        // Each person gets their own new clothes.
        scene.character_outfit = "a red evening dress".into();
        scene.persona_outfit = "a grey suit".into();
        let p = positive(&scene, "", &character, Some(&persona));
        assert!(
            p.contains("first person has silver hair and wears a red evening dress"),
            "{p}"
        );
        assert!(
            p.contains("second person has brown hair and wears a grey suit"),
            "{p}"
        );
        assert!(
            !p.contains("black coat") && !p.contains("green jacket"),
            "{p}"
        );
        // A story outfit that only drops words of the usual one keeps it.
        scene.persona_outfit = "his jacket".into();
        let p = positive(&scene, "", &character, Some(&persona));
        assert!(p.contains("brown hair and wears green jacket"), "{p}");
        // Only the character changed.
        scene.persona_outfit.clear();
        let p = positive(&scene, "", &character, Some(&persona));
        assert!(p.contains("brown hair and wears green jacket"), "{p}");
        // With no persona nobody is drawn, and the user's clothes never
        // dress the character.
        scene.who = Who::Persona;
        scene.character_outfit.clear();
        scene.persona_outfit = "a grey suit".into();
        let p = positive(&scene, "", &character, None);
        assert!(
            !p.contains("silver hair") && !p.contains("grey suit"),
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
    fn a_saved_prompt_knows_its_people_and_no_persona_means_nobody() {
        let kai = Person {
            appearance: "man, messy brown hair, stubble",
            outfit: "olive jacket",
            picture: true,
        };
        let mut scene = fallback("the user waves", "");
        scene.who = Who::Persona;
        let text = positive(&scene, "", &kai, Some(&kai));
        assert!(describes(&text, kai.appearance), "{text}");
        assert!(!describes(&text, "woman, wavy brown hair, round glasses"));
        // No persona in the chat: nobody, not the character in the user's place.
        assert!(references(&scene, &kai, None).is_empty());
        assert!(!positive(&scene, "", &kai, None).contains("stubble"));
    }

    #[test]
    fn image_numbers_follow_the_people_with_a_picture() {
        let with = |picture| Person {
            appearance: "silver hair",
            outfit: "coat",
            picture,
        };
        let mut scene = fallback("the character hands the user a cup", "");
        scene.who = Who::Both;
        // Both pictures: image 1 is the character, image 2 the persona, each
        // named where the action first mentions them.
        let (c, p) = (with(true), with(true));
        let text = positive(&scene, "", &c, Some(&p));
        assert!(
            text.contains(
                "The first person from image 1 hands the second person from image 2 a cup"
            ),
            "{text}"
        );
        assert!(text.contains("The first person has silver hair"), "{text}");
        assert_eq!(
            references(&scene, &c, Some(&p)),
            vec![Who::Character, Who::Persona]
        );
        // Only the persona has one: it is image 1.
        let (c, p) = (with(false), with(true));
        let text = positive(&scene, "", &c, Some(&p));
        assert!(
            text.contains("The first person hands the second person from image 1 a cup"),
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
        // Not in the action: the link goes to the looks.
        let mut quiet = fallback("rain falls", "");
        quiet.who = Who::Character;
        let c = with(true);
        let text = positive(&quiet, "", &c, None);
        assert!(text.contains("Only one person. Rain falls"), "{text}");
        assert!(
            text.contains("The person from image 1 has silver hair"),
            "{text}"
        );
        assert_eq!(references(&quiet, &c, None), vec![Who::Character]);
        // Without the pictures, the prompt is the tags-only one.
        assert_eq!(
            without_references(&text),
            positive(&quiet, "", &with(false), None)
        );
        let (c, p) = (with(true), with(true));
        assert_eq!(
            without_references(&positive(&scene, "", &c, Some(&p))),
            positive(&scene, "", &with(false), Some(&with(false)))
        );
        // A person with a picture and no looks keeps the link.
        let bare = Person {
            appearance: "",
            outfit: "",
            picture: true,
        };
        let text = positive(&quiet, "", &bare, None);
        assert!(
            text.ends_with("The person from image 1 is in the scene"),
            "{text}"
        );
    }
}

//! The picture prompt: the scene joined with the fixed looks of the people in it.

use super::*;

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
    /// Made under adult rules (`Rules::mature`): saved as `images.adult`.
    pub adult: bool,
}

pub(super) const BASE_NEGATIVE: &str =
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
pub(super) fn noun(p: &Person) -> &'static str {
    let first = p.appearance.split(',').next().unwrap_or("").trim();
    ["woman", "man"]
        .into_iter()
        .find(|w| first.eq_ignore_ascii_case(w))
        .unwrap_or("person")
}

pub(super) fn plural(noun: &str) -> &'static str {
    match noun {
        "woman" => "women",
        "man" => "men",
        _ => "people",
    }
}

/// The byte offset just after the first whole-word `phrase` (any case).
pub(super) fn first_word_at(text: &str, phrase: &str) -> Option<usize> {
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
pub(super) fn labels(people: &[(Who, &Person, &str)]) -> Vec<String> {
    let nouns: Vec<&str> = people.iter().map(|(_, p, _)| noun(p)).collect();
    match nouns.as_slice() {
        [a, b] if a == b => vec![format!("the first {a}"), format!("the second {b}")],
        _ => nouns.iter().map(|n| format!("the {n}")).collect(),
    }
}

/// `text` with each whole-word `from` (any case) replaced by `to`.
pub(super) fn replace_words(text: &str, from: &str, to: &str) -> String {
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

pub(super) fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The people in the picture, in prompt order, each with the clothes the
/// story gave them, if any. Without a persona the character is drawn, and
/// keeps the character's own clothes.
pub(super) fn people<'a, 'p>(
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
pub(super) fn looks_text(appearance: &str) -> String {
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
/// persona switch, "image 2" would be someone else. No Looks proves nothing.
pub fn describes(prompt: &str, appearance: &str) -> bool {
    let looks = looks_text(appearance);
    !looks.is_empty() && prompt.contains(&looks)
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
pub(super) fn person_text(
    label: &str,
    (_, p, story_outfit): &(Who, &Person, &str),
) -> Option<String> {
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
pub(super) const FRAMINGS: [&str; 8] = [
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
    for term in crate::pictures::safety::negative_terms(rules) {
        negative.push_str(", ");
        negative.push_str(term);
    }
    Prompts {
        positive: pos,
        negative,
        adult: rules.mature,
    }
}

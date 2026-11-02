//! Looks and a usual outfit suggested from a character card, for a card that
//! has none (most cards from other apps). The chat model writes two lines,
//! and this module keeps only what is safe to put in every picture prompt.

use super::safety;
use crate::library::MAX_TAGS;
use serde::Serialize;

/// What the user sees in the creator, to check and edit before saving.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct Suggested {
    pub looks: String,
    pub outfit: String,
}

/// Head words of clothes. A Looks tag with one of them goes to the outfit,
/// because the pictures replace the outfit when the story changes clothes.
const CLOTHES: &[&str] = &[
    "apron", "armor", "armour", "blazer", "blouse", "boots", "cap", "cape", "cardigan", "cloak",
    "coat", "dress", "gloves", "gown", "hat", "hoodie", "jacket", "jeans", "jumper", "kimono",
    "leggings", "overalls", "pants", "robe", "robes", "sandals", "scarf", "shirt", "shoes",
    "shorts", "skirt", "sneakers", "suit", "sweater", "t-shirt", "tie", "trousers", "tunic",
    "uniform", "vest", "wearing",
];

/// The prompt for the chat model. `card` is the card's own text.
pub fn prompt(name: &str, card: &str) -> String {
    format!(
        "Character: {name}\nCard:\n{card}\n\n\
         Write two lines for a picture of this character.\n\
         Looks: hair, eyes, face, build and other features that never change, as short tags \
         separated by commas. No clothes.\n\
         Outfit: what they usually wear, as short tags separated by commas.\n\
         Use what the card says. Where the card says nothing, pick plain looks that fit the \
         character. Do not write an age.\n\
         Example:\n\
         Looks: short silver hair, grey eyes, sharp face, tall and lean\n\
         Outfit: black leather jacket, white shirt, dark jeans\n\
         Write only the two lines."
    )
}

/// Reads the model's answer. Tags that the picture safety rules flag are
/// dropped, and clothes in the Looks line move to the outfit.
pub fn parse(answer: &str) -> Suggested {
    let mut looks = Vec::new();
    let mut outfit = Vec::new();
    for line in answer.lines() {
        let line = line.trim().trim_start_matches(['-', '*', '#', ' ']);
        let lower = line.to_lowercase();
        let (to, rest) = if let Some(rest) = lower.strip_prefix("looks:") {
            (&mut looks, &line[line.len() - rest.len()..])
        } else if let Some(rest) = lower.strip_prefix("outfit:") {
            (&mut outfit, &line[line.len() - rest.len()..])
        } else {
            continue;
        };
        for tag in rest.split(',') {
            let tag = tag.trim().trim_matches(['*', '"', '.', ' ']);
            if !tag.is_empty() && !safety::flagged(tag) {
                to.push(tag.to_owned());
            }
        }
    }
    let (clothes, kept): (Vec<String>, Vec<String>) =
        looks.into_iter().partition(|t| is_clothes(t));
    for c in clothes {
        if !outfit.contains(&c) {
            outfit.push(c);
        }
    }
    Suggested {
        looks: join(&kept),
        outfit: join(&outfit),
    }
}

fn is_clothes(tag: &str) -> bool {
    tag.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .any(|w| CLOTHES.contains(&w))
}

/// The tags, cut at a tag boundary to fit the field.
fn join(tags: &[String]) -> String {
    let mut out = String::new();
    for t in tags {
        let next = if out.is_empty() {
            t.clone()
        } else {
            format!("{out}, {t}")
        };
        if next.chars().count() > MAX_TAGS {
            break;
        }
        out = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_lines_are_read() {
        let s =
            parse("Looks: long red hair, green eyes, freckles\nOutfit: green cloak, brown boots");
        assert_eq!(s.looks, "long red hair, green eyes, freckles");
        assert_eq!(s.outfit, "green cloak, brown boots");
    }

    #[test]
    fn extra_text_and_markdown_are_ignored() {
        let s = parse(
            "Sure! Here you go:\n\n**Looks:** grey beard, kind eyes.\n- Outfit: wool sweater\nHope it helps.",
        );
        assert_eq!(s.looks, "grey beard, kind eyes");
        assert_eq!(s.outfit, "wool sweater");
    }

    #[test]
    fn a_missing_line_stays_empty() {
        assert_eq!(parse("Looks: bald, tall").outfit, "");
        assert_eq!(parse("nothing useful").looks, "");
    }

    #[test]
    fn clothes_in_looks_move_to_the_outfit() {
        let s = parse("Looks: short black hair, round glasses, red scarf\nOutfit: denim jacket");
        assert_eq!(s.looks, "short black hair, round glasses");
        assert_eq!(s.outfit, "denim jacket, red scarf");
    }

    #[test]
    fn flagged_tags_are_dropped() {
        let s =
            parse("Looks: petite teen, blue eyes, youthful body\nOutfit: school uniform, sneakers");
        assert_eq!(s.looks, "blue eyes");
        assert_eq!(s.outfit, "sneakers");
        let s = parse("Looks: brown hair\nOutfit: revealing dress, lingerie");
        assert_eq!(s.outfit, "");
    }

    #[test]
    fn a_long_answer_is_cut_at_a_tag() {
        let many = vec!["silver hair"; 60].join(", ");
        let s = parse(&format!("Looks: {many}"));
        assert!(s.looks.chars().count() <= MAX_TAGS);
        assert!(s.looks.ends_with("silver hair"));
    }
}

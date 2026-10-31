//! Tone presets and reply styles. Tone is the mood. Reply style is the shape of
//! the reply. They are independent, so "Dark + Texting" and "Dark + Novel" both work.

use serde::{Deserialize, Serialize};

pub const TONES: &[&str] = &[
    "casual",
    "wholesome",
    "dramatic",
    "flirty",
    "dark",
    "funny",
    "custom",
];
pub const STYLES: &[&str] = &[
    "texting",
    "conversational",
    "storyteller",
    "novel",
    "custom",
];

/// Longest custom tone text, in characters.
pub const MAX_CUSTOM_TONE: usize = 300;

/// The instruction and temperature of a tone. `custom` uses the chat's own text.
///
/// A short mood word lost to the character card: a warm character stayed warm
/// in a dark chat. So each tone says how the mood shows in `{{char}}`'s
/// reactions. The texts describe a mood, never a fact or a trait, because the
/// card that the user wrote stays true in every tone (see `guide::tone`).
pub fn tone(key: &str, custom: &str) -> (String, f32) {
    let (text, temperature) = match key {
        "wholesome" => (
            "Wholesome. The mood is soft and safe. {{char}} shows a caring side: {{char}} looks \
             after {{user}}, notices small good things and helps without being asked.",
            0.7,
        ),
        "dramatic" => (
            "Dramatic. The mood is intense. {{char}} feels things strongly and shows it: shock, \
             anger, passion or fear come out in words and actions. Small news becomes a turning \
             point.",
            0.85,
        ),
        "flirty" => (
            "Flirty. The mood is playful and a little romantic. {{char}} shows interest in \
             {{user}} in {{char}}'s own way: {{char}} notices how {{user}} looks and talks, plays \
             coy and drops hints. Keep it light and tasteful. If {{char}}'s description says \
             that {{char}} is taken or not interested, {{char}} only teases as a friend and does \
             not touch or invite {{user}} closer.",
            0.9,
        ),
        "dark" => (
            "Dark. The mood is heavy and tense. {{char}} is more guarded than usual: {{char}} \
             expects the worst, notices what could go wrong and does not take kindness at face \
             value.",
            0.8,
        ),
        "funny" => (
            "Funny. The mood is silly. {{char}} sees the silly side of things: {{char}} teases \
             the situation, plays with words and overreacts for fun. Each reply has a line meant \
             to amuse.",
            0.95,
        ),
        "custom" if !custom.trim().is_empty() => (custom.trim(), 0.8),
        _ => (
            "Casual. The mood is relaxed and ordinary. {{char}} takes things in stride and talks \
             plainly.",
            0.8,
        ),
    };
    (text.to_owned(), temperature)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Length {
    VeryShort,
    Short,
    Medium,
    Long,
    VeryLong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Talk {
    DialogueOnly,
    MostlyDialogue,
    Mixed,
    MostlyNarration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Description {
    Minimal,
    Some,
    Vivid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Initiative {
    FollowMe,
    Balanced,
    LeadTheStory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Perspective {
    First,
    Third,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actions {
    Asterisks,
    Plain,
}

/// The sliders behind every reply style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StyleSettings {
    pub length: Length,
    pub talk: Talk,
    pub description: Description,
    pub initiative: Initiative,
    pub perspective: Perspective,
    pub actions: Actions,
}

impl StyleSettings {
    /// The settings of a preset. `custom` reads the chat's JSON, and falls back to Conversational.
    pub fn of(style: &str, custom_json: &str) -> Self {
        use Description as D;
        use Initiative as I;
        use Length as L;
        use Talk as T;
        let s = |length, talk, description, initiative, actions| Self {
            length,
            talk,
            description,
            initiative,
            perspective: Perspective::Third,
            actions,
        };
        match style {
            "texting" => s(
                L::VeryShort,
                T::DialogueOnly,
                D::Minimal,
                I::Balanced,
                Actions::Asterisks,
            ),
            "storyteller" => s(
                L::Medium,
                T::Mixed,
                D::Some,
                I::Balanced,
                Actions::Asterisks,
            ),
            "novel" => s(
                L::Long,
                T::MostlyNarration,
                D::Vivid,
                I::LeadTheStory,
                Actions::Plain,
            ),
            "custom" => {
                serde_json::from_str(custom_json).unwrap_or_else(|_| Self::of("conversational", ""))
            }
            _ => s(
                L::Short,
                T::MostlyDialogue,
                D::Some,
                I::Balanced,
                Actions::Asterisks,
            ),
        }
    }

    /// One step shorter or longer, for "Regenerate as...".
    pub fn shifted(mut self, longer: bool) -> Self {
        use Length::*;
        self.length = match (self.length, longer) {
            (VeryShort, false) | (Short, false) => VeryShort,
            (Medium, false) | (VeryShort, true) => Short,
            (Long, false) | (Short, true) => Medium,
            (VeryLong, false) | (Medium, true) => Long,
            (Long, true) | (VeryLong, true) => VeryLong,
        };
        self
    }

    /// The word range that the length rule asks for. The quality suite checks replies against it.
    pub fn word_range(&self) -> (usize, usize) {
        match self.length {
            Length::VeryShort => (1, 30),
            Length::Short => (40, 90),
            Length::Medium => (120, 220),
            Length::Long => (250, 400),
            Length::VeryLong => (450, 650),
        }
    }

    /// The token cap for one reply. Small models often ignore length rules, so this is the real limit.
    pub fn num_predict(&self) -> i32 {
        match self.length {
            Length::VeryShort => 60,
            Length::Short => 250,
            Length::Medium => 450,
            Length::Long => 650,
            Length::VeryLong => 900,
        }
    }

    /// The style instruction for the end reminder.
    pub fn instruction(&self) -> String {
        let mut parts: Vec<&str> = vec![match self.length {
            Length::VeryShort => "Reply in 1 to 2 short sentences, under 30 words.",
            Length::Short => "Reply in one paragraph of 40 to 90 words.",
            Length::Medium => {
                "Reply in 2 to 3 paragraphs separated by blank lines, 120 to 220 words in total."
            }
            Length::Long => {
                "Reply in 3 to 5 paragraphs separated by blank lines, 250 to 400 words in total, \
                 like a page of a book."
            }
            Length::VeryLong => {
                "Reply in 5 or more paragraphs separated by blank lines, 450 to 650 words in total, \
                 like a full scene in a book."
            }
        }];
        parts.push(match self.talk {
            Talk::DialogueOnly => "Write only what {{char}} says, with no actions or narration.",
            Talk::MostlyDialogue => "Write mostly dialogue, with at most one short action.",
            Talk::Mixed => "Mix what {{char}} says with what {{char}} does.",
            Talk::MostlyNarration => {
                "Write mostly narration about {{char}} only: what {{char}} does, notices, thinks and decides. \
                 {{char}} also says a few lines to {{user}}. \
                 Leave {{user}}'s actions, words and feelings to {{user}}."
            }
        });
        if self.talk != Talk::DialogueOnly {
            parts.push(match self.description {
                Description::Minimal => "Keep descriptions to a minimum.",
                Description::Some => "Add a little body language, and setting where it matters.",
                Description::Vivid => {
                    "Use vivid detail for {{char}}'s actions, body language and senses, and for what changes in the scene."
                }
            });
            parts.push(match self.perspective {
                Perspective::First => "Write {{char}}'s actions in first person (I).",
                Perspective::Third => "Write {{char}}'s actions in third person.",
            });
            parts.push(match self.actions {
                Actions::Asterisks => "Put actions in *asterisks*.",
                Actions::Plain => "Write actions as plain prose, with no asterisks.",
            });
        }
        parts.push(match self.initiative {
            Initiative::FollowMe => "Only react to what {{user}} does. Do not add new events.",
            Initiative::Balanced => {
                "Answer what {{user}} said, so {{user}} sees that {{char}} listened. \
                 If {{user}} asks {{char}} for something, or answers a question that {{char}} asked, \
                 {{char}} acts on it in this reply, with concrete details. Then {{char}} brings \
                 something new into the scene, such as a detail, a memory, a small event or an \
                 offer, as part of what {{char}} says and does. {{char}} brings the story and does \
                 not ask {{user}} to make up clues or events."
            }
            Initiative::LeadTheStory => {
                "Answer what {{user}} said. If {{user}} asks {{char}} for something, or answers \
                 a question that {{char}} asked, {{char}} acts on it in this reply. Then {{char}} moves the story forward and brings in \
                 something concrete: an event, a person, a discovery or a problem. {{char}} brings \
                 the story and does not ask {{user}} to make up clues or events."
            }
        });
        if self.length >= Length::Medium {
            parts.push("Short sentences are fine, but write enough of them to reach this length.");
        }
        parts.push("This length rule is more important than the length of earlier replies.");
        parts.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_and_shifts() {
        let texting = StyleSettings::of("texting", "");
        assert_eq!(texting.num_predict(), 60);
        assert!(
            texting
                .instruction()
                .starts_with("Reply in 1 to 2 short sentences, under 30 words. Write only what")
        );
        assert!(!texting.instruction().contains("asterisks"));
        assert_eq!(texting.shifted(false).length, Length::VeryShort);
        assert_eq!(texting.shifted(true).length, Length::Short);
        assert_eq!(
            StyleSettings::of("novel", "").shifted(true).length,
            Length::VeryLong
        );

        let json = serde_json::to_string(&StyleSettings::of("novel", "")).unwrap();
        assert_eq!(
            StyleSettings::of("custom", &json),
            StyleSettings::of("novel", "")
        );
        assert_eq!(
            StyleSettings::of("custom", "{bad"),
            StyleSettings::of("conversational", "")
        );
    }

    #[test]
    fn tones() {
        assert_eq!(tone("dark", "").1, 0.8);
        assert_eq!(tone("custom", "  Cozy.  ").0, "Cozy.");
        assert!(tone("custom", "").0.starts_with("Casual."));
        assert!(tone("dark", "").0.starts_with("Dark. The mood is heavy"));
    }
}

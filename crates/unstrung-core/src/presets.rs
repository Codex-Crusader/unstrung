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
pub fn tone(key: &str, custom: &str) -> (String, f32) {
    let (text, temperature) = match key {
        "wholesome" => ("Warm, kind and hopeful.", 0.7),
        "dramatic" => ("Intense, with high stakes and strong feelings.", 0.85),
        "flirty" => ("Playful and teasing.", 0.9),
        "dark" => ("Grim, tense and moody.", 0.8),
        "funny" => ("Light, with jokes and comic timing.", 0.95),
        "custom" if !custom.trim().is_empty() => (custom.trim(), 0.8),
        _ => ("Relaxed and natural.", 0.8),
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
                I::FollowMe,
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
            Length::VeryShort => "Reply in 1 to 2 short sentences.",
            Length::Short => "Reply in one short paragraph.",
            Length::Medium => "Reply in 2 to 3 paragraphs.",
            Length::Long => "Reply in 3 to 5 paragraphs, like a page of a book.",
            Length::VeryLong => "Reply in 5 or more paragraphs, like a full scene in a book.",
        }];
        parts.push(match self.talk {
            Talk::DialogueOnly => "Write only what {{char}} says, with no actions or narration.",
            Talk::MostlyDialogue => "Write mostly dialogue, with at most one short action.",
            Talk::Mixed => "Mix dialogue and narration.",
            Talk::MostlyNarration => {
                "Write mostly narration and inner thoughts, with some dialogue."
            }
        });
        if self.talk != Talk::DialogueOnly {
            parts.push(match self.description {
                Description::Minimal => "Keep descriptions to a minimum.",
                Description::Some => "Add a little setting and body language.",
                Description::Vivid => "Describe the setting, body language and senses vividly.",
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
            Initiative::Balanced => "React first. You can add a small detail or a question.",
            Initiative::LeadTheStory => {
                "Move the story forward with new events, and end with a hook or a question."
            }
        });
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
                .starts_with("Reply in 1 to 2 short sentences. Write only what")
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
        assert_eq!(tone("custom", "").0, "Relaxed and natural.");
    }
}

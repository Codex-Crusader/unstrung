//! Character growth: how a character's feelings about the user change with
//! what happens in a chat. Plain arithmetic, no model.
//!
//! The model only proposes (`Appraisal`); this code decides. Three layers
//! move at different speeds:
//! - emotions (joy, anger, hurt) react to one event and fade in a few turns
//! - mood is a slow average of how events felt
//! - drives (trust, closeness, respect, romance) change only by events,
//!   never by time, so a betrayal is not forgotten because turns passed
//!
//! The genome comes from the card and never changes. It sets where the
//! drives start and how the character weighs events. The update rules are
//! inspired by behavioural economics: losses weigh more than equal gains
//! (loss aversion), unfair treatment hurts more (fairness), a hurt from
//! someone close is felt more (reference points), and recent events weigh
//! more than old ones (emotions fade, drives do not).

use serde::{Deserialize, Serialize};

pub mod ask;

/// One proposed unit of change. A critic answer of +2 moves a drive by
/// 2 * STEP before the genome weights.
const STEP: f32 = 0.08;
/// No single event moves a drive more than this.
const EVENT_CAP: f32 = 0.35;
/// An apology on its own never raises trust more than this. Amends come
/// from what the user does after it.
const APOLOGY_CAP: f32 = 0.05;
/// Share of an emotion left after a turn.
const EMOTION_FADE: f32 = 0.5;
/// Share of the mood left after a turn.
const MOOD_KEEP: f32 = 0.85;
/// A drive change from the card's start that the prompt mentions, and one
/// it calls strong. A capped single event (EVENT_CAP) is strong.
const MENTION: f32 = 0.2;
const STRONG: f32 = 0.35;

/// The card's traits as numbers, each 0 to 1. Made once per card version.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Genome {
    /// How warm the character starts toward a stranger.
    pub warmth: f32,
    /// How strongly events move the emotions.
    pub sensitivity: f32,
    /// How much more a loss counts than an equal gain: 0 is even, 1 is three times.
    pub loss_aversion: f32,
    /// How much more unfair treatment hurts.
    pub fairness: f32,
    /// How fast kindness rebuilds a drive that is below where it started.
    pub forgiveness: f32,
}

impl Genome {
    /// For a card with no genome yet: an even-tempered person.
    pub const PLAIN: Self = Self {
        warmth: 0.5,
        sensitivity: 0.5,
        loss_aversion: 0.5,
        fairness: 0.5,
        forgiveness: 0.5,
    };

    /// Each value clamped to 0..=1, and NaN made 0.5.
    pub fn clamped(self) -> Self {
        let c = |v: f32| if v.is_nan() { 0.5 } else { v.clamp(0.0, 1.0) };
        Self {
            warmth: c(self.warmth),
            sensitivity: c(self.sensitivity),
            loss_aversion: c(self.loss_aversion),
            fairness: c(self.fairness),
            forgiveness: c(self.forgiveness),
        }
    }

    /// Where trust and closeness start, from -0.3 (cold) to 0.3 (warm).
    fn start(&self) -> f32 {
        (self.warmth - 0.5) * 0.6
    }
}

/// The relationship drives, each -1 to 1. Zero is a stranger.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Drives {
    pub trust: f32,
    pub closeness: f32,
    pub respect: f32,
    pub romance: f32,
}

/// Emotions, each 0 to 1.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Emotions {
    pub joy: f32,
    pub anger: f32,
    pub hurt: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub drives: Drives,
    pub emotions: Emotions,
    /// -1 (low) to 1 (high).
    pub mood: f32,
    /// Why each drive last moved up and last moved down, from the critic,
    /// for the prompt and the panel. Kept per direction, so a line about
    /// lost trust never gives the reason of a later kind act.
    #[serde(default)]
    pub reasons: Vec<Reason>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reason {
    pub drive: Drive,
    pub up: bool,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Drive {
    Trust,
    Closeness,
    Respect,
    Romance,
}

/// What the critic proposes for one exchange. Every number is checked
/// here, so a broken or hostile answer can move a drive by one capped
/// event at most.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Appraisal {
    /// How good or bad the user's action was for the character, -2 to 2.
    #[serde(default)]
    pub valence: i8,
    /// The user treated the character unfairly.
    #[serde(default)]
    pub unfair: bool,
    /// The user apologized or made amends.
    #[serde(default)]
    pub apology: bool,
    /// Proposed direction of each drive, -2 to 2.
    #[serde(default)]
    pub trust: i8,
    #[serde(default)]
    pub closeness: i8,
    #[serde(default)]
    pub respect: i8,
    #[serde(default)]
    pub romance: i8,
    /// A few words on what happened.
    #[serde(default)]
    pub reason: String,
}

/// Most characters kept from the critic's reason.
const REASON_CHARS: usize = 80;

impl Appraisal {
    /// Numbers clamped to their ranges, and the reason cut to one short line.
    pub fn checked(mut self) -> Self {
        for v in [
            &mut self.valence,
            &mut self.trust,
            &mut self.closeness,
            &mut self.respect,
            &mut self.romance,
        ] {
            *v = (*v).clamp(-2, 2);
        }
        let line = self.reason.lines().next().unwrap_or_default().trim();
        self.reason = line.chars().take(REASON_CHARS).collect();
        self
    }

    fn is_plain(&self) -> bool {
        self.valence == 0
            && self.trust == 0
            && self.closeness == 0
            && self.respect == 0
            && self.romance == 0
    }
}

impl State {
    /// A new chat: drives where the genome puts them, no emotions.
    pub fn new(genome: &Genome) -> Self {
        let start = genome.start();
        Self {
            drives: Drives {
                trust: start,
                closeness: start,
                respect: 0.0,
                romance: 0.0,
            },
            emotions: Emotions::default(),
            mood: 0.0,
            reasons: Vec::new(),
        }
    }

    /// The state after one exchange. `romance_allowed` is false unless both
    /// people are known adults (`content::adult_people`): then romance never rises.
    #[must_use]
    pub fn after(&self, genome: &Genome, event: &Appraisal, romance_allowed: bool) -> Self {
        let g = genome.clamped();
        let e = event.clone().checked();
        let mut next = self.clone();
        next.emotions.joy *= EMOTION_FADE;
        next.emotions.anger *= EMOTION_FADE;
        next.emotions.hurt *= EMOTION_FADE;
        let felt = f32::from(e.valence) / 2.0;
        next.mood = next.mood * MOOD_KEEP + felt * (1.0 - MOOD_KEEP);
        if e.is_plain() {
            return next;
        }

        // Reference point: a hurt from someone close is felt more.
        let close = self.drives.closeness.max(0.0);
        let strength = (0.5 + g.sensitivity) * (1.0 + close);
        if felt > 0.0 {
            next.emotions.joy = next.emotions.joy.max((felt * strength).min(1.0));
        } else if felt < 0.0 {
            let pain = (-felt * strength).min(1.0);
            if e.unfair {
                next.emotions.anger = next.emotions.anger.max(pain);
            } else {
                next.emotions.hurt = next.emotions.hurt.max(pain);
            }
        }

        let start = g.start();
        let moves = [
            (Drive::Trust, e.trust, start),
            (Drive::Closeness, e.closeness, start),
            (Drive::Respect, e.respect, 0.0),
            (Drive::Romance, e.romance, 0.0),
        ];
        for (drive, proposed, baseline) in moves {
            if proposed == 0 || (drive == Drive::Romance && proposed > 0 && !romance_allowed) {
                continue;
            }
            let now = next.get(drive);
            let mut delta = f32::from(proposed) * STEP;
            if delta < 0.0 {
                delta *= 1.0 + 2.0 * g.loss_aversion;
                if e.unfair {
                    delta *= 1.0 + g.fairness;
                }
            } else if now < baseline {
                // Repair: below where it started, kindness counts at the
                // character's own pace.
                delta *= 0.5 + g.forgiveness;
            }
            if e.apology && drive == Drive::Trust && delta > 0.0 {
                delta = delta.min(APOLOGY_CAP);
            }
            delta = delta.clamp(-EVENT_CAP, EVENT_CAP);
            // Soft bounds: a move away from zero slows near the ends.
            if delta * now > 0.0 {
                delta *= 1.0 - now.abs();
            }
            next.set(drive, (now + delta).clamp(-1.0, 1.0));
            let up = delta > 0.0;
            next.reasons.retain(|r| r.drive != drive || r.up != up);
            if !e.reason.is_empty() {
                next.reasons.push(Reason {
                    drive,
                    up,
                    text: e.reason.clone(),
                });
            }
        }
        next
    }

    fn get(&self, drive: Drive) -> f32 {
        match drive {
            Drive::Trust => self.drives.trust,
            Drive::Closeness => self.drives.closeness,
            Drive::Respect => self.drives.respect,
            Drive::Romance => self.drives.romance,
        }
    }

    fn set(&mut self, drive: Drive, value: f32) {
        match drive {
            Drive::Trust => self.drives.trust = value,
            Drive::Closeness => self.drives.closeness = value,
            Drive::Respect => self.drives.respect = value,
            Drive::Romance => self.drives.romance = value,
        }
    }

    /// Two or three plain sentences for the prompt, about how the character
    /// feels now compared with how the card starts them. Empty when nothing
    /// changed enough to say. `{{char}}` and `{{user}}` stay as macros.
    pub fn describe(&self, genome: &Genome, romance_allowed: bool) -> String {
        let start = genome.clamped().start();
        let mut lines: Vec<(f32, Drive, &str)> = Vec::new();
        let mut add =
            |drive: Drive, change: f32, up: [&'static str; 2], down: [&'static str; 2]| {
                // One small kind act is not news. One severe betrayal is.
                if change.abs() < MENTION {
                    return;
                }
                let strong = usize::from(change.abs() >= STRONG);
                let words = if change > 0.0 {
                    up[strong]
                } else {
                    down[strong]
                };
                lines.push((change, drive, words));
            };
        add(
            Drive::Trust,
            self.drives.trust - start,
            [
                "trusts {{user}} more than at first",
                "has come to trust {{user}}",
            ],
            [
                "trusts {{user}} less than before",
                "does not trust {{user}} after what happened",
            ],
        );
        add(
            Drive::Closeness,
            self.drives.closeness - start,
            ["has grown fond of {{user}}", "feels close to {{user}}"],
            [
                "feels more distant from {{user}}",
                "keeps {{user}} at arm's length now",
            ],
        );
        add(
            Drive::Respect,
            self.drives.respect,
            ["respects {{user}}", "deeply respects {{user}}"],
            [
                "has lost some respect for {{user}}",
                "has no respect left for {{user}}",
            ],
        );
        if romance_allowed {
            add(
                Drive::Romance,
                self.drives.romance,
                ["is drawn to {{user}}", "has fallen for {{user}}"],
                [
                    "has cooled toward {{user}}",
                    "has no romantic interest in {{user}}",
                ],
            );
        }
        lines.sort_by(|a, b| b.0.abs().total_cmp(&a.0.abs()));
        lines.truncate(2);
        let mut said: Vec<&str> = Vec::new();
        let e = self.emotions;
        let feeling = [(e.anger, "angry"), (e.hurt, "hurt"), (e.joy, "happy")]
            .into_iter()
            .filter(|(v, _)| *v >= 0.4)
            .max_by(|a, b| a.0.total_cmp(&b.0));
        let mut out: Vec<String> = lines
            .into_iter()
            .map(|(change, drive, words)| {
                let why = self
                    .reasons
                    .iter()
                    .find(|r| {
                        r.drive == drive
                            && r.up == (change > 0.0)
                            && !said.contains(&r.text.as_str())
                    })
                    .map_or(String::new(), |r| {
                        said.push(&r.text);
                        format!(" ({})", r.text)
                    });
                format!("{{{{char}}}} {words}{why}.")
            })
            .collect();
        if let Some((_, word)) = feeling {
            out.push(format!("Right now {{{{char}}}} feels {word}."));
        }
        out.join(" ")
    }
}

/// The state on the chat's current path: the newest saved state of a reply
/// on the path, or a new state when the path has none. Rows of replies that
/// left the path (rewind, regenerate, branch, delete) are skipped.
pub fn on_path(
    rows: &[(i64, State, Appraisal)],
    path: &[crate::db::Message],
    genome: &Genome,
) -> State {
    let by_id: std::collections::HashMap<i64, &State> =
        rows.iter().map(|(id, s, _)| (*id, s)).collect();
    path.iter()
        .rev()
        .find_map(|m| by_id.get(&m.id))
        .map_or_else(|| State::new(genome), |s| (*s).clone())
}

#[cfg(test)]
mod tests;

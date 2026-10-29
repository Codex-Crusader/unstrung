//! The model catalog: a short list of tested models, shipped as JSON with the app.
//! Each entry holds what the app needs to show it, to download it and to tune it.

use crate::error::{Error, Result};
use crate::hardware::Tier;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const CATALOG_JSON: &str = include_str!("../catalog/models.json");

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct License {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Entry {
    /// The Ollama tag. It is also the ID.
    pub id: String,
    pub name: String,
    /// "chat" for now. Image and vision models come in later phases.
    pub kind: String,
    pub size_bytes: u64,
    pub min_tier: Tier,
    /// The tiers where this is the "Recommended" model.
    #[serde(default)]
    pub recommended_for: Vec<Tier>,
    /// The context size to use. The model can take more, but VRAM sets the limit.
    pub context: u32,
    pub license: License,
    /// The manifest digest that was tested. A different digest after a download
    /// means the model changed upstream.
    pub digest: String,
    pub mature_ok: bool,
    /// A short description in plain words.
    pub description: String,
    /// The date of the quality run that tested this model.
    pub tested: String,
    #[serde(default)]
    pub tuning: Tuning,
}

/// How well a model follows the reply style. The UI shows it next to the style picker.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StyleFollowing {
    Good,
    #[default]
    Fair,
    Weak,
}

/// Sampler and prompt settings for one model.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Tuning {
    /// `num_ctx`.
    pub context: u32,
    /// `None`: the model cannot think, so the request has no `think` field.
    /// `Some(x)`: the model can think, and `x` is the default.
    pub think: Option<bool>,
    /// The tone sets a temperature. This multiplies it.
    pub temperature_scale: f32,
    pub min_p: f32,
    pub top_p: f32,
    pub top_k: i32,
    /// Lowers the chance of words that are already in the reply.
    pub presence_penalty: f32,
    pub repeat_penalty: f32,
    pub repeat_last_n: i32,
    /// Multiplies the token cap of each reply length.
    pub length_scale: f32,
    /// Extra stop sequences.
    pub stops: Vec<String>,
    pub style_following: StyleFollowing,
    /// An extra line for the end reminder, for a model that needs it.
    pub reminder_extra: String,
    /// A fixed temperature from the advanced settings. It replaces the tone's.
    #[serde(skip)]
    pub temperature: Option<f32>,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            context: 8192,
            think: None,
            temperature_scale: 1.0,
            min_p: 0.05,
            top_p: 1.0,
            top_k: 40,
            presence_penalty: 0.0,
            repeat_penalty: 1.08,
            repeat_last_n: 256,
            length_scale: 1.0,
            stops: Vec::new(),
            style_following: StyleFollowing::Fair,
            reminder_extra: String::new(),
            temperature: None,
        }
    }
}

impl Tuning {
    pub fn with_advanced(mut self, advanced: &Advanced) -> Self {
        if let Some(context) = advanced.context {
            self.context = context;
        }
        if let Some(t) = advanced.temperature {
            self.temperature = Some(t);
        }
        if let Some(m) = advanced.min_p {
            self.min_p = m;
        }
        if let Some(r) = advanced.repeat_penalty {
            self.repeat_penalty = r;
        }
        // Only a model that can think gets the field.
        if self.think.is_some()
            && let Some(think) = advanced.think
        {
            self.think = Some(think);
        }
        self
    }

    /// The temperature for a tone.
    pub fn temperature_for(&self, tone_temperature: f32) -> f32 {
        self.temperature
            .unwrap_or(tone_temperature * self.temperature_scale)
    }

    /// The token cap for a reply length.
    pub fn num_predict(&self, base: i32) -> i32 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        let scaled = (base as f32 * self.length_scale).round() as i32;
        scaled.max(16)
    }
}

/// The user's advanced settings. `None` means "Automatic": the catalog value.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Advanced {
    pub context: Option<u32>,
    pub temperature: Option<f32>,
    pub min_p: Option<f32>,
    pub repeat_penalty: Option<f32>,
    /// "Think before each reply", for models that can.
    pub think: Option<bool>,
}

impl Advanced {
    pub const CONTEXTS: [u32; 4] = [4096, 8192, 12288, 16384];

    pub fn check(&self) -> Result<()> {
        let bad = |what: &str| Err(Error::Invalid(format!("{what} is out of range")));
        if self.context.is_some_and(|c| !Self::CONTEXTS.contains(&c)) {
            return bad("the context size");
        }
        if self.temperature.is_some_and(|t| !(0.0..=2.0).contains(&t)) {
            return bad("the temperature");
        }
        if self.min_p.is_some_and(|m| !(0.0..=0.5).contains(&m)) {
            return bad("min_p");
        }
        if self
            .repeat_penalty
            .is_some_and(|r| !(1.0..=1.5).contains(&r))
        {
            return bad("the repeat penalty");
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct CatalogFile {
    models: Vec<Entry>,
}

/// All catalog entries.
pub fn entries() -> &'static [Entry] {
    static CATALOG: OnceLock<Vec<Entry>> = OnceLock::new();
    CATALOG.get_or_init(|| match serde_json::from_str::<CatalogFile>(CATALOG_JSON) {
        Ok(file) => file.models,
        Err(e) => {
            // A test checks the file, so this happens only with a broken build.
            log::error!("the model catalog is not valid: {e}");
            Vec::new()
        }
    })
}

/// Ollama treats `name` and `name:latest` as one model.
pub fn same_tag(a: &str, b: &str) -> bool {
    let full = |t: &str| {
        if t.contains(':') {
            t.to_owned()
        } else {
            format!("{t}:latest")
        }
    };
    full(a) == full(b)
}

pub fn entry(model: &str) -> Option<&'static Entry> {
    entries().iter().find(|e| same_tag(&e.id, model))
}

/// The catalog tuning, or the defaults for a model that is not in the catalog.
pub fn tuning(model: &str) -> Tuning {
    entry(model).map(|e| e.tuning.clone()).unwrap_or_default()
}

/// The recommended chat model for a tier.
pub fn recommended(tier: Tier) -> Option<&'static Entry> {
    entries()
        .iter()
        .find(|e| e.kind == "chat" && e.recommended_for.contains(&tier))
}

/// A model name that a user typed in "Add an Ollama model by name".
/// Only library names: letters, digits and `._-:/`.
pub fn check_tag(tag: &str) -> Result<String> {
    let tag = tag.trim();
    let allowed = |c: char| c.is_ascii_alphanumeric() || "._-:/".contains(c);
    if tag.is_empty() || tag.len() > 120 || !tag.chars().all(allowed) || tag.contains("..") {
        return Err(Error::Invalid(
            "a model name has only letters, digits and . _ - : /, for example \"llama3.1:8b\"".into(),
        ));
    }
    Ok(tag.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_valid() {
        let all = entries();
        assert!(!all.is_empty());
        for tier in [Tier::Basic, Tier::Standard, Tier::High] {
            let pick = recommended(tier).expect("each tier has a recommended model");
            assert!(pick.min_tier <= tier, "{} is too big for {tier:?}", pick.id);
        }
        for e in all {
            assert!(e.digest.starts_with("sha256:") || e.digest.len() == 64, "{}", e.id);
            assert!(!e.license.name.is_empty() && e.license.url.starts_with("https://"));
            assert!(!e.tested.is_empty(), "{} has no test date", e.id);
            assert!(e.context >= 4096 && e.tuning.context == e.context, "{}", e.id);
            assert!(check_tag(&e.id).is_ok());
            // Only one entry per tag.
            assert_eq!(all.iter().filter(|o| same_tag(&o.id, &e.id)).count(), 1);
        }
    }

    #[test]
    fn advanced_settings_apply() {
        let base = Tuning {
            think: Some(false),
            ..Tuning::default()
        };
        let adv = Advanced {
            context: Some(16384),
            temperature: Some(0.6),
            think: Some(true),
            ..Advanced::default()
        };
        let t = base.with_advanced(&adv);
        assert_eq!((t.context, t.think, t.temperature_for(0.9)), (16384, Some(true), 0.6));
        // A model that cannot think never gets the field.
        assert_eq!(Tuning::default().with_advanced(&adv).think, None);
        assert!(Advanced { context: Some(5000), ..Advanced::default() }.check().is_err());
        assert!(Advanced { temperature: Some(3.0), ..Advanced::default() }.check().is_err());
        assert!(adv.check().is_ok());
    }

    #[test]
    fn tags() {
        assert!(same_tag("qwen2.5", "qwen2.5:latest"));
        assert!(!same_tag("qwen2.5:7b", "qwen2.5:14b"));
        assert!(check_tag("hf.co/bartowski/Some-GGUF:Q4_K_M").is_ok());
        assert!(check_tag("llama3.1:8b; rm").is_err());
        assert!(check_tag("../x").is_err());
        assert!(check_tag("").is_err());
    }
}

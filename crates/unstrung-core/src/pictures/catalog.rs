//! The image model catalog, shipped as `catalog/images.json`. An image model
//! is a set of parts (diffusion model, text encoder, VAE). Each part is one
//! file with a pinned URL and SHA-256. Models can share a part, which is then
//! stored once in `<models folder>/image/` and kept while any model uses it.

use crate::catalog::License;
use crate::hardware::Tier;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const IMAGES_JSON: &str = include_str!("../../catalog/images.json");

#[derive(Debug, Default, Deserialize)]
pub struct Catalog {
    /// Download hosts. An entry with a leading dot allows the hosts under it.
    pub hosts: Vec<String>,
    pub components: Vec<Component>,
    pub models: Vec<ImageModel>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Component {
    pub id: String,
    /// The file name on disk.
    pub file: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ImageModel {
    pub id: String,
    pub name: String,
    /// The `sd-cli` model option for each part ("diffusion_model" for
    /// `--diffusion-model`), then the component id.
    pub parts: BTreeMap<String, String>,
    pub min_tier: Tier,
    #[serde(default)]
    pub recommended_for: Vec<Tier>,
    pub license: License,
    /// The model can make adult pictures. Only used with the adult setting on.
    pub mature_ok: bool,
    /// The model takes reference pictures (Phase 6).
    pub ref_images: bool,
    pub sampler: String,
    pub steps: u32,
    pub cfg_scale: f32,
    pub description: String,
    /// The date of the run that tested it on real hardware. Empty: not yet.
    pub tested: String,
}

pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(IMAGES_JSON).unwrap_or_else(|e| {
            // A test checks the file, so this happens only with a broken build.
            log::error!("the image catalog is not valid: {e}");
            Catalog::default()
        })
    })
}

pub fn model(id: &str) -> Option<&'static ImageModel> {
    catalog().models.iter().find(|m| m.id == id)
}

pub fn component(id: &str) -> Option<&'static Component> {
    catalog().components.iter().find(|c| c.id == id)
}

impl ImageModel {
    /// The parts with their files, in option order.
    pub fn components(&self) -> Vec<(&str, &'static Component)> {
        self.parts
            .iter()
            .filter_map(|(slot, id)| component(id).map(|c| (slot.as_str(), c)))
            .collect()
    }

    pub fn size_bytes(&self) -> u64 {
        self.components().iter().map(|(_, c)| c.size).sum()
    }

    pub fn fits(&self, tier: Tier) -> bool {
        tier >= self.min_tier
    }

    /// True when every part is on disk with its full size.
    pub fn installed(&self, dir: &Path) -> bool {
        self.components().iter().all(|(_, c)| on_disk(dir, c))
    }

    /// Bytes still to download: the parts that are not on disk yet.
    pub fn missing_bytes(&self, dir: &Path) -> u64 {
        self.components()
            .iter()
            .filter(|(_, c)| !on_disk(dir, c))
            .map(|(_, c)| c.size)
            .sum()
    }
}

/// A part counts as present when its file has the catalog size. The hash is
/// checked at download time and by the full repair.
pub fn on_disk(dir: &Path, c: &Component) -> bool {
    std::fs::metadata(dir.join(&c.file)).is_ok_and(|m| m.len() == c.size)
}

pub fn path_of(dir: &Path, c: &Component) -> PathBuf {
    dir.join(&c.file)
}

/// The "Recommended" model for a tier.
pub fn recommended(tier: Tier) -> Option<&'static ImageModel> {
    catalog()
        .models
        .iter()
        .find(|m| m.recommended_for.contains(&tier))
}

/// The files that removing `model` may delete: its parts that no other
/// installed model uses.
pub fn files_to_remove(model: &ImageModel, dir: &Path) -> Vec<&'static Component> {
    let others: Vec<&ImageModel> = catalog()
        .models
        .iter()
        .filter(|m| m.id != model.id && m.installed(dir))
        .collect();
    model
        .components()
        .into_iter()
        .map(|(_, c)| c)
        .filter(|c| {
            !others
                .iter()
                .any(|m| m.parts.values().any(|id| *id == c.id))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_is_complete() {
        let c = catalog();
        assert!(!c.models.is_empty());
        for comp in &c.components {
            assert!(
                comp.url.starts_with("https://huggingface.co/"),
                "{}",
                comp.url
            );
            assert!(
                comp.url.contains("/resolve/"),
                "pinned revision: {}",
                comp.url
            );
            assert_eq!(comp.sha256.len(), 64, "{}", comp.id);
            assert!(comp.size > 0);
            assert!(!comp.file.contains(['/', '\\']), "{}", comp.file);
        }
        let mut files: Vec<&str> = c.components.iter().map(|c| c.file.as_str()).collect();
        files.sort_unstable();
        files.dedup();
        assert_eq!(files.len(), c.components.len(), "file names must be unique");
        for m in &c.models {
            assert_eq!(
                m.components().len(),
                m.parts.len(),
                "{}: unknown part",
                m.id
            );
            for slot in m.parts.keys() {
                assert!(
                    matches!(slot.as_str(), "diffusion_model" | "llm" | "vae" | "model"),
                    "{}: {slot}",
                    m.id
                );
            }
            assert!(m.steps > 0 && m.cfg_scale > 0.0);
        }
        for tier in [Tier::Basic, Tier::Standard, Tier::High] {
            let r = recommended(tier).expect("a recommended model for each tier");
            assert!(r.fits(tier));
        }
    }

    #[test]
    fn a_shared_part_stays_while_another_model_uses_it() {
        let dir = tempfile::tempdir().unwrap();
        let write = |c: &Component| {
            let f = std::fs::File::create(dir.path().join(&c.file)).unwrap();
            f.set_len(c.size).unwrap();
        };
        let z = model("z-image-turbo").unwrap();
        let k = model("flux2-klein-4b").unwrap();
        for (_, c) in z.components() {
            write(c);
        }
        assert!(z.installed(dir.path()));
        // klein needs only its own two parts: the encoder is shared.
        let shared = component("qwen3-4b-q4km").unwrap();
        assert_eq!(k.missing_bytes(dir.path()), k.size_bytes() - shared.size);
        // With only Z-Image there, removing it removes all its parts.
        assert_eq!(files_to_remove(z, dir.path()).len(), 3);
        for (_, c) in k.components() {
            write(c);
        }
        // With klein there too, the encoder stays.
        let gone: Vec<&str> = files_to_remove(z, dir.path())
            .iter()
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(gone, ["z-image-turbo-q4k", "flux1-ae"]);
    }
}

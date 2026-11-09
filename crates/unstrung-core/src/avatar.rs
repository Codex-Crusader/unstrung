//! Profile pictures. An upload is decoded with size limits and saved as a new
//! PNG in the staging folder, so metadata and hidden data are dropped. The UI
//! shows the staged copy, the user picks a square, and `finish` saves a
//! 512 x 512 crop in the avatars folder.

use crate::error::{Error, Result};
use crate::paths::AppPaths;
use image::{DynamicImage, ImageFormat, ImageReader, Limits, imageops::FilterType};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::sync::atomic::{AtomicU64, Ordering};

/// Largest upload, in bytes.
pub const MAX_UPLOAD_BYTES: usize = 15 * 1024 * 1024;
/// Largest width or height of an upload, in pixels.
const MAX_SIDE: u32 = 8192;
/// Most memory the decoder may use.
const MAX_ALLOC: u64 = 512 * 1024 * 1024;
/// The staged copy is scaled down to this side, which is enough to pick a crop.
const STAGED_SIDE: u32 = 1600;
/// The smallest crop, in staged pixels.
const MIN_CROP: u32 = 32;
pub const AVATAR_SIDE: u32 = 512;
/// The longest side of a gallery thumbnail: sharp in a grid cell at 200% scale.
pub const THUMB_SIDE: u32 = 384;
pub(crate) const STAGING: &str = "staging";

static COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
pub struct Staged {
    pub token: String,
    pub width: u32,
    pub height: u32,
}

/// A square in the staged picture, in its pixels.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Crop {
    pub x: u32,
    pub y: u32,
    pub size: u32,
}

/// What a save does with the picture.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AvatarChange {
    Keep,
    Remove,
    Staged { token: String, crop: Crop },
}

/// Decodes an outside picture: PNG, JPEG or WebP only, with limits on
/// the file size, the sides and the memory.
fn decode(bytes: &[u8]) -> Result<DynamicImage> {
    if bytes.is_empty() {
        return Err(Error::Invalid("the file is empty".into()));
    }
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(Error::Invalid("the picture is larger than 15 MB".into()));
    }
    let format = image::guess_format(bytes)
        .map_err(|_| Error::Invalid("use a PNG, JPEG or WebP picture".into()))?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
    ) {
        return Err(Error::Invalid("use a PNG, JPEG or WebP picture".into()));
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    reader.decode().map_err(|e| Error::Rejected {
        message: "the picture could not be read. Use a PNG, JPEG or WebP picture".into(),
        why: e.to_string(),
    })
}

/// A clean PNG copy of a profile picture from a backup. Other programs
/// (the picture engine) read avatars, so no outside bytes stay.
pub fn clean_copy(bytes: &[u8]) -> Result<Vec<u8>> {
    encode_png(&DynamicImage::ImageRgba8(decode(bytes)?.to_rgba8()))
}

/// Decodes an upload and saves a clean copy in the staging folder.
pub fn stage(paths: &AppPaths, bytes: &[u8]) -> Result<Staged> {
    let picture = decode(bytes)?;
    if picture.width() < MIN_CROP || picture.height() < MIN_CROP {
        return Err(Error::Invalid("the picture is too small".into()));
    }
    let picture = if picture.width().max(picture.height()) > STAGED_SIDE {
        picture.resize(STAGED_SIDE, STAGED_SIDE, FilterType::Triangle)
    } else {
        picture
    };
    let picture = DynamicImage::ImageRgba8(picture.to_rgba8());
    let token = new_token();
    std::fs::create_dir_all(paths.root().join(STAGING))?;
    paths.key()?.write(
        &paths.stored(&staged_path(&token)?)?,
        &encode_png(&picture)?,
    )?;
    Ok(Staged {
        token,
        width: picture.width(),
        height: picture.height(),
    })
}

/// The stored path of a staged picture. The token must be one this module made.
pub fn staged_path(token: &str) -> Result<String> {
    let valid = !token.is_empty()
        && token.len() <= 40
        && token.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-');
    if !valid {
        return Err(Error::Invalid("the picture upload is not valid".into()));
    }
    Ok(format!("{STAGING}/{token}.png"))
}

/// Crops a staged picture to a 512 x 512 PNG in the avatars folder and
/// returns its stored path. The staged copy is removed.
pub fn finish(paths: &AppPaths, token: &str, crop: Crop, name: &str) -> Result<String> {
    let staged = paths.stored(&staged_path(token)?)?;
    if !staged.is_file() {
        return Err(Error::Invalid(
            "the picture upload expired; choose the picture again".into(),
        ));
    }
    let key = paths.key()?;
    let bytes = key.read(&staged)?;
    let picture = image::load_from_memory_with_format(&bytes, ImageFormat::Png)
        .map_err(|e| Error::Invalid(format!("the staged picture could not be read ({e})")))?;
    let fits = crop.size >= MIN_CROP
        && crop
            .x
            .checked_add(crop.size)
            .is_some_and(|r| r <= picture.width())
        && crop
            .y
            .checked_add(crop.size)
            .is_some_and(|b| b <= picture.height());
    if !fits {
        return Err(Error::Invalid("the crop is outside the picture".into()));
    }
    let square = picture
        .crop_imm(crop.x, crop.y, crop.size, crop.size)
        .resize_exact(AVATAR_SIDE, AVATAR_SIDE, FilterType::Lanczos3);
    let stored = format!("avatars/{name}-{}.png", new_token());
    key.write(&paths.stored(&stored)?, &encode_png(&square)?)?;
    let _ = std::fs::remove_file(staged);
    Ok(stored)
}

/// Removes a saved picture. A missing file is not an error.
pub fn remove(paths: &AppPaths, stored: &str) {
    if let Ok(file) = paths.stored(stored)
        && let Err(e) = std::fs::remove_file(file)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("could not remove the picture {stored}: {e}");
    }
}

/// Removes staged pictures and plain work copies left from an earlier run.
pub fn clear_staging(paths: &AppPaths) {
    for dir in [paths.root().join(STAGING), paths.work()] {
        if dir.is_dir()
            && let Err(e) = std::fs::remove_dir_all(&dir)
        {
            log::warn!("could not clear {}: {e}", dir.display());
        }
    }
}

/// A small JPEG copy of a gallery picture for the grid. It is made for each
/// request and never saved, so no extra file needs encryption or deletion.
pub fn thumbnail(bytes: &[u8]) -> Result<Vec<u8>> {
    let picture = decode(bytes)?;
    let small = if picture.width().max(picture.height()) > THUMB_SIDE {
        picture.thumbnail(THUMB_SIDE, THUMB_SIDE)
    } else {
        picture
    };
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
        .encode_image(&small.to_rgb8())
        .map_err(|e| Error::Invalid(format!("the thumbnail could not be made ({e})")))?;
    Ok(out)
}

fn encode_png(picture: &DynamicImage) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    picture
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .map_err(|e| Error::Invalid(format!("the picture could not be saved ({e})")))?;
    Ok(out)
}

/// A profile picture as base64 PNG, at most 512 pixels a side, for a chat
/// model that can see. `stored` is a saved avatar (`avatars/...`) or a staged
/// upload (`staging/...`); nothing else is read.
pub fn for_vision(paths: &AppPaths, stored: &str) -> Result<String> {
    use base64::Engine as _;
    if !(stored.starts_with("avatars/") || stored.starts_with(&format!("{STAGING}/"))) {
        return Err(Error::Invalid("this is not a profile picture".into()));
    }
    let file = paths.stored(stored)?;
    if !file.is_file() {
        return Err(Error::Invalid(
            "the picture is gone; choose the picture again".into(),
        ));
    }
    let bytes = paths.key()?.read(&file)?;
    let picture = image::load_from_memory_with_format(&bytes, ImageFormat::Png)
        .map_err(|e| Error::Invalid(format!("the picture could not be read ({e})")))?;
    let picture = if picture.width().max(picture.height()) > AVATAR_SIDE {
        picture.resize(AVATAR_SIDE, AVATAR_SIDE, FilterType::Triangle)
    } else {
        picture
    };
    Ok(base64::engine::general_purpose::STANDARD.encode(encode_png(&picture)?))
}

/// A unique name part: time, process and a counter.
fn new_token() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:x}-{:x}{n:x}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn home() -> (tempfile::TempDir, AppPaths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::at(dir.path().to_path_buf());
        paths.ensure().unwrap();
        (dir, paths)
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let picture = DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            width,
            height,
            Rgba([200, 120, 40, 255]),
        ));
        encode_png(&picture).unwrap()
    }

    #[test]
    fn stage_and_crop() {
        let (_dir, paths) = home();
        let staged = stage(&paths, &png(3000, 2000)).unwrap();
        assert_eq!((staged.width, staged.height), (1600, 1067));
        let crop = Crop {
            x: 100,
            y: 0,
            size: 1000,
        };
        let stored = finish(&paths, &staged.token, crop, "character-1").unwrap();
        assert!(stored.starts_with("avatars/character-1-"));
        let file = paths.stored(&stored).unwrap();
        assert!(image::open(&file).is_err(), "encrypted on disk");
        let bytes = paths.key().unwrap().read(&file).unwrap();
        let saved = image::load_from_memory(&bytes).unwrap();
        assert_eq!((saved.width(), saved.height()), (512, 512));
        assert!(
            finish(&paths, &staged.token, crop, "x").is_err(),
            "a token works once"
        );
    }

    #[test]
    fn a_thumbnail_is_a_small_jpeg_with_the_same_shape() {
        let thumb = thumbnail(&png(1024, 768)).unwrap();
        let small = image::load_from_memory_with_format(&thumb, ImageFormat::Jpeg).unwrap();
        assert_eq!((small.width(), small.height()), (THUMB_SIDE, 288));
        let tiny = image::load_from_memory(&thumbnail(&png(100, 50)).unwrap()).unwrap();
        assert_eq!(
            (tiny.width(), tiny.height()),
            (100, 50),
            "never made larger"
        );
        assert!(thumbnail(b"not a picture").is_err());
    }

    #[test]
    fn bad_input_is_refused() {
        let (_dir, paths) = home();
        assert!(stage(&paths, b"").is_err());
        assert!(stage(&paths, b"GIF89a....").is_err());
        assert!(stage(&paths, &png(10, 10)).is_err(), "too small");
        let mut broken = png(100, 100);
        broken.truncate(60);
        assert!(stage(&paths, &broken).is_err());
        assert!(stage(&paths, &vec![0u8; MAX_UPLOAD_BYTES + 1]).is_err());

        let staged = stage(&paths, &png(100, 100)).unwrap();
        let outside = Crop {
            x: 50,
            y: 50,
            size: 60,
        };
        assert!(finish(&paths, &staged.token, outside, "c").is_err());
        assert!(staged_path("../app").is_err());
        assert!(staged_path("abc/def").is_err());
    }

    #[test]
    fn random_bytes_never_panic() {
        let (_dir, paths) = home();
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let header = png(64, 64);
        for round in 0..200 {
            let mut bytes = header.clone();
            for _ in 0..(round % 20 + 1) {
                let at = (next() as usize) % bytes.len();
                bytes[at] = next() as u8;
            }
            let _ = stage(&paths, &bytes);
        }
    }
}

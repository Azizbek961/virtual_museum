//! Yuklangan rasmlarni tekshirish, saqlash va o'lchamini me'yorlash.
use std::{
    io::{BufWriter, Cursor, Write},
    path::{Path, PathBuf},
};

use image::{
    codecs::jpeg::JpegEncoder, imageops::FilterType, metadata::Orientation, DynamicImage, ImageDecoder,
    ImageFormat, ImageReader,
};

use crate::{auth, error::AppError, session::Upload};

const ALLOWED_EXT: &[&str] = &["jpg", "jpeg", "png", "webp", "gif"];

/// Rasmni tekshiradi: kengaytma, hajm, haqiqatan ham rasm ekanligi va formati.
pub fn validate_image(filename: &str, bytes: &[u8], max_mb: u64) -> Result<(), String> {
    let ext = Path::new(filename)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if !ALLOWED_EXT.contains(&ext.as_str()) {
        return Err("Faqat JPG, PNG, WEBP yoki GIF rasm yuklash mumkin.".into());
    }
    if bytes.len() as u64 > max_mb * 1024 * 1024 {
        return Err(format!("Rasm hajmi {max_mb} MB dan oshmasligi kerak."));
    }
    let invalid = || "Fayl yaroqli rasm emas.".to_string();
    let reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|_| invalid())?;
    let format = reader.format().ok_or_else(invalid)?;
    if !matches!(format, ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP | ImageFormat::Gif) {
        return Err("Bu rasm formati qo'llab-quvvatlanmaydi.".into());
    }
    reader.decode().map_err(|_| invalid())?;
    Ok(())
}

/// Fayl nomini xavfsiz qiladi (Django `get_valid_filename` kabi).
pub fn safe_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    let cleaned: String = base
        .chars()
        .map(|c| if c.is_whitespace() { '_' } else { c })
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    let cleaned = cleaned.trim_start_matches('.').to_string();
    let (stem, ext) = split_ext(&cleaned);
    let stem: String = stem.chars().take(80).collect();
    let stem = if stem.is_empty() { "image".to_string() } else { stem };
    format!("{stem}{ext}")
}

fn split_ext(name: &str) -> (String, String) {
    match name.rfind('.') {
        Some(i) if i > 0 => (name[..i].to_string(), name[i..].to_lowercase()),
        _ => (name.to_string(), String::new()),
    }
}

/// `dir/filename` ko'rinishidagi, hali band bo'lmagan nisbiy yo'l.
pub fn unique_rel_path(media: &Path, dir: &str, filename: &str) -> String {
    let dir = dir.trim_matches('/');
    let fname = safe_filename(filename);
    let mut rel = format!("{dir}/{fname}");
    let (stem, ext) = split_ext(&fname);
    while media.join(&rel).exists() {
        rel = format!("{dir}/{stem}_{}{ext}", auth::random_string(7));
    }
    rel
}

/// EXIF burilishini to'g'rilaydi va juda katta rasmlarni kichraytiradi
/// (3D zalda tekstura sifatida ishlatiladi). Yakuniy o'lchamlarni qaytaradi.
pub fn normalize_image(path: &Path, max_side: u32) -> Option<(u32, u32)> {
    match try_normalize(path, max_side) {
        Ok(dims) => Some(dims),
        Err(e) => {
            tracing::warn!("rasmni me'yorlab bo'lmadi ({}): {e:#}", path.display());
            image::image_dimensions(path).ok()
        }
    }
}

fn try_normalize(path: &Path, max_side: u32) -> anyhow::Result<(u32, u32)> {
    let reader = ImageReader::open(path)?.with_guessed_format()?;
    let format = reader.format().unwrap_or(ImageFormat::Png);
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder)?;
    let mut changed = false;
    if orientation != Orientation::NoTransforms {
        img.apply_orientation(orientation);
        changed = true;
    }
    if img.width().max(img.height()) > max_side {
        img = img.resize(max_side, max_side, FilterType::Lanczos3);
        changed = true;
    }
    if changed {
        save_image(&img, path, format)?;
    }
    Ok((img.width(), img.height()))
}

fn save_image(img: &DynamicImage, path: &Path, format: ImageFormat) -> anyhow::Result<()> {
    let tmp = path.with_extension("normalize.tmp");
    {
        let mut w = BufWriter::new(std::fs::File::create(&tmp)?);
        match format {
            ImageFormat::Jpeg => {
                let rgb = DynamicImage::ImageRgb8(img.to_rgb8());
                rgb.write_with_encoder(JpegEncoder::new_with_quality(&mut w, 90))?;
            }
            ImageFormat::Gif | ImageFormat::WebP => {
                DynamicImage::ImageRgba8(img.to_rgba8()).write_to(&mut w, format)?;
            }
            _ => img.write_to(&mut w, ImageFormat::Png)?,
        }
        w.flush()?;
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    Ok(())
}

/// Tekshirilgan rasmni `media/<dir>/` ga yozadi, me'yorlaydi.
/// Natija: (nisbiy yo'l, o'lchamlar).
pub async fn store_upload(
    media: PathBuf,
    dir: String,
    upload: Upload,
    max_side: u32,
) -> Result<(String, Option<(u32, u32)>), AppError> {
    let r = tokio::task::spawn_blocking(move || -> anyhow::Result<(String, Option<(u32, u32)>)> {
        let rel = unique_rel_path(&media, &dir, &upload.filename);
        let full = media.join(&rel);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&full, &upload.bytes)?;
        let dims = normalize_image(&full, max_side);
        Ok((rel, dims))
    })
    .await??;
    Ok(r)
}

/// Media faylini o'chiradi (xatolar e'tiborsiz qoldiriladi).
pub fn remove_media(media: &Path, rel: &str) {
    if rel.is_empty() || rel.contains("..") {
        return;
    }
    let _ = std::fs::remove_file(media.join(rel));
}

pub async fn remove_media_async(media: PathBuf, files: Vec<String>) {
    let _ = tokio::task::spawn_blocking(move || {
        for f in files {
            remove_media(&media, &f);
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames() {
        assert_eq!(safe_filename("../../etc/passwd"), "passwd");
        assert_eq!(safe_filename("C:\\x\\My Photo (1).JPG"), "My_Photo_1.jpg");
        assert_eq!(safe_filename(".png"), "png");
        assert_eq!(safe_filename(""), "image");
    }

    #[test]
    fn rejects_non_images() {
        assert!(validate_image("x.png", b"not an image", 10).is_err());
        assert!(validate_image("x.txt", b"hello", 10).is_err());
    }
}

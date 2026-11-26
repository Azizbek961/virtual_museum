//! Mozaika zali: rasmlarni qadimiy koshinli tosh mozaikaga aylantirish.
use std::path::{Path, PathBuf};

use image::{GenericImageView, ImageFormat, Rgb, RgbImage};
use rusqlite::{params, Connection};

use crate::{db::Db, images, models};

pub const MOSAIC_SUFFIX: &str = "-mosaic";

struct Pending {
    id: i64,
    image: String,
    mosaic: String,
}

fn pending(c: &Connection) -> rusqlite::Result<Vec<Pending>> {
    let mut st = c.prepare(&format!(
        "SELECT p.id, p.image, p.mosaic FROM paintings p JOIN sections s ON s.id = p.section_id
         WHERE s.kind = ?1 AND (p.mosaic = '' OR p.mosaic NOT LIKE '%{MOSAIC_SUFFIX}%') ORDER BY p.id"
    ))?;
    let rows = st.query_map([models::KIND_MOSAICS], |r| {
        Ok(Pending { id: r.get(0)?, image: r.get(1)?, mosaic: r.get(2)? })
    })?;
    rows.collect()
}

fn save(c: &Connection, media: &Path, p: &Pending, file: String) -> rusqlite::Result<bool> {
    let n = c.execute(
        "UPDATE paintings SET mosaic = ?1 WHERE id = ?2 AND image = ?3 AND mosaic = ?4",
        params![file, p.id, p.image, p.mosaic],
    )?;
    let stale = if n == 0 { [&file] } else { [&p.mosaic] };
    for f in stale {
        images::remove_media(media, f);
    }
    Ok(n > 0)
}

/// Rasmni chiroyli koshinli tosh mozaikaga (tesserae) aylantiradi.
pub fn make_mosaic_image(src: &image::DynamicImage) -> RgbImage {
    let (w, h) = src.dimensions();
    if w == 0 || h == 0 {
        return RgbImage::new(1, 1);
    }
    // Koshin o'lchami: 8 dan 14 pikselgacha
    let tile_size = ((w.max(h) as f32 / 50.0).round() as u32).clamp(8, 14);
    let grout = if tile_size >= 12 { 2 } else { 1 };
    let inner = tile_size.saturating_sub(grout);

    let rgb = src.to_rgb8();
    let mut out = RgbImage::new(w, h);

    let num_tiles_x = (w + tile_size - 1) / tile_size;
    let num_tiles_y = (h + tile_size - 1) / tile_size;

    for ty in 0..num_tiles_y {
        for tx in 0..num_tiles_x {
            let x0 = tx * tile_size;
            let y0 = ty * tile_size;
            let x1 = (x0 + tile_size).min(w);
            let y1 = (y0 + tile_size).min(h);

            // 1. Koshin hududidagi o'rtacha rangni hisoblash
            let mut sum_r = 0u64;
            let mut sum_g = 0u64;
            let mut sum_b = 0u64;
            let mut count = 0u64;

            for y in y0..y1 {
                for x in x0..x1 {
                    let p = rgb.get_pixel(x, y);
                    sum_r += p[0] as u64;
                    sum_g += p[1] as u64;
                    sum_b += p[2] as u64;
                    count += 1;
                }
            }

            if count == 0 {
                continue;
            }

            let avg_r = (sum_r / count) as i32;
            let avg_g = (sum_g / count) as i32;
            let avg_b = (sum_b / count) as i32;

            // Tabiiy tosh tebranishi (har bir tosh biroz o'ziga xos jilvaga ega)
            let hash = (tx.wrapping_mul(374761393) ^ ty.wrapping_mul(668265263)) as i32;
            let jitter = (hash % 15) - 7; // -7 .. +7
            let base_r = (avg_r + jitter).clamp(0, 255);
            let base_g = (avg_g + jitter).clamp(0, 255);
            let base_b = (avg_b + jitter).clamp(0, 255);

            // 2. Koshinni chok (grout) va 3D relyef bilan chizish
            for y in y0..y1 {
                let dy = y - y0;
                for x in x0..x1 {
                    let dx = x - x0;

                    // Qorishma (sement choki / grout line)
                    if dx >= inner || dy >= inner {
                        let mortar_jitter = ((x.wrapping_mul(17) ^ y.wrapping_mul(31)) % 7) as u8;
                        let m = 28 + mortar_jitter;
                        out.put_pixel(x, y, Rgb([m, m, m + 4]));
                        continue;
                    }

                    // Koshinning 1px yumaloqlangan burchagi
                    if (dx == 0 && dy == 0)
                        || (dx == inner - 1 && dy == 0)
                        || (dx == 0 && dy == inner - 1)
                        || (dx == inner - 1 && dy == inner - 1)
                    {
                        let cr = ((base_r * 2 + 35) / 3).clamp(0, 255) as u8;
                        let cg = ((base_g * 2 + 35) / 3).clamp(0, 255) as u8;
                        let cb = ((base_b * 2 + 40) / 3).clamp(0, 255) as u8;
                        out.put_pixel(x, y, Rgb([cr, cg, cb]));
                        continue;
                    }

                    // 3D Bevel (nur yuqori-chapdan tushadi)
                    let (pr, pg, pb) = if dx == 0 || dy == 0 {
                        // Yuqori va chap qirra yorug'
                        ((base_r + 26).min(255) as u8, (base_g + 26).min(255) as u8, (base_b + 26).min(255) as u8)
                    } else if dx == inner - 1 || dy == inner - 1 {
                        // Quyi va o'ng qirra soya
                        ((base_r - 24).max(0) as u8, (base_g - 24).max(0) as u8, (base_b - 24).max(0) as u8)
                    } else {
                        // Koshin sirti va markaziy yaltiroqlik (keramik jilva)
                        let dist_x = (dx as f32 - (inner as f32 - 1.0) / 2.0).abs();
                        let dist_y = (dy as f32 - (inner as f32 - 1.0) / 2.0).abs();
                        let max_d = (inner as f32) / 2.0;
                        let center_factor = (1.0 - (dist_x.max(dist_y) / max_d)).clamp(0.0, 1.0);
                        let gloss = (center_factor * 10.0) as i32;

                        (
                            (base_r + gloss).clamp(0, 255) as u8,
                            (base_g + gloss).clamp(0, 255) as u8,
                            (base_b + gloss).clamp(0, 255) as u8,
                        )
                    };

                    out.put_pixel(x, y, Rgb([pr, pg, pb]));
                }
            }
        }
    }

    out
}

pub fn generate(media: &Path, image_rel: &str) -> anyhow::Result<String> {
    let src_path = media.join(image_rel);
    let img = image::open(&src_path)?;
    let mosaic = make_mosaic_image(&img);

    let (dir, stem) = match image_rel.rsplit_once('/') {
        Some((d, f)) => (d, f.rsplit_once('.').map(|(s, _)| s).unwrap_or(f)),
        None => ("", image_rel.rsplit_once('.').map(|(s, _)| s).unwrap_or(image_rel)),
    };
    let mosaic_rel = images::unique_rel_path(media, dir, &format!("{stem}{MOSAIC_SUFFIX}.png"));
    let full_path: PathBuf = media.join(&mosaic_rel);
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    mosaic.save_with_format(&full_path, ImageFormat::Png)?;
    Ok(mosaic_rel)
}

fn run_pending(db: &Db, media: &Path, list: Vec<Pending>) -> usize {
    let mut done = 0;
    for p in list {
        match generate(media, &p.image) {
            Ok(file) => {
                if db.with(|c| save(c, media, &p, file)).unwrap_or(false) {
                    done += 1;
                }
            }
            Err(e) => tracing::warn!("mozaika yasab bo'lmadi (asar {}): {e:#}", p.id),
        }
    }
    done
}

pub fn ensure_sync(db: &Db, media: &Path) -> usize {
    let list = db.with(|c| pending(c)).unwrap_or_default();
    run_pending(db, media, list)
}

pub async fn ensure(db: &Db, media: &Path) {
    let (db, media) = (db.clone(), media.to_path_buf());
    let _ = tokio::task::spawn_blocking(move || ensure_sync(&db, &media)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mosaic_transforms_image() {
        let mut img = RgbImage::new(60, 60);
        for p in img.pixels_mut() {
            *p = Rgb([100, 150, 200]);
        }
        let dynamic = image::DynamicImage::ImageRgb8(img);
        let m = make_mosaic_image(&dynamic);
        assert_eq!(m.width(), 60);
        assert_eq!(m.height(), 60);
    }
}

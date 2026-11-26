//! Eski desktop loyihadagi 5 ta zal va 30 ta asarni bazaga import qiladi
//! (Django `python manage.py seed_museum` o'rniga).
//!
//! ```text
//! cargo run -- seed            # bor bo'lsa — o'tkazib yuboradi
//! cargo run -- seed --update   # matnlarni qayta yozadi
//! ```
use std::path::Path;

use anyhow::{bail, Context};
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;

use crate::{
    images,
    models::{self, now_iso},
    sculpt, AppState,
};

#[derive(Deserialize)]
struct Data {
    sections: Vec<SeedSection>,
}

#[derive(Deserialize)]
struct SeedSection {
    id: String,
    name: String,
    #[serde(default)]
    tagline: String,
    #[serde(default)]
    description: String,
    accent: [u8; 3],
    wall_color: [u8; 3],
    #[serde(default = "default_floor")]
    floor: String,
    #[serde(default)]
    paintings: Vec<SeedPainting>,
}

#[derive(Deserialize)]
struct SeedPainting {
    id: String,
    title: String,
    #[serde(default)]
    artist: String,
    year: Option<i64>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    facts: String,
}

fn default_floor() -> String {
    "floor_marble".into()
}

/// Zal nomlari o'zbek tilida (asar matnlari asl holicha qoladi).
fn uz_name(id: &str) -> Option<(&'static str, &'static str)> {
    Some(match id {
        "art" => ("San'at galereyasi", "Rangtasvir durdonalari"),
        "history" => (models::SCULPTURE_HALL.1, models::SCULPTURE_HALL.2),
        "science" => (models::MOSAIC_HALL.1, models::MOSAIC_HALL.2),
        "technology" | "love" => (models::LOVE_HALL.1, models::LOVE_HALL.2),
        "uzbekistan" | "marry_me" => (models::MARRY_ME_HALL.1, models::MARRY_ME_HALL.2),
        _ => return None,
    })
}

fn hex(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

pub fn run(state: &AppState, source: &Path, update: bool) -> anyhow::Result<()> {
    let data_file = source.join("data").join("museum_data.json");
    let images_dir = source.join("assets").join("images");
    if !data_file.exists() {
        bail!("Topilmadi: {}", data_file.display());
    }
    let data: Data = serde_json::from_str(
        &std::fs::read_to_string(&data_file).with_context(|| format!("o'qib bo'lmadi: {}", data_file.display()))?,
    )
    .context("museum_data.json formati noto'g'ri")?;
    let media = state.cfg.media_dir.clone();

    let (mut n_sec, mut n_new, mut n_skip) = (0, 0, 0);
    state.db.with(|c| -> anyhow::Result<()> {
        for (order, s) in data.sections.iter().enumerate() {
            let (name, tagline) = uz_name(&s.id).map(|(n, t)| (n.to_string(), t.to_string())).unwrap_or_else(|| (s.name.clone(), s.tagline.clone()));
            // sobiq "Tarix muzeyi" -> "Haykallar zali", sobiq "Fan muzeyi" -> "Mozaika muzeyi"
            let sculptures = s.id == "history";
            let mosaics = s.id == "science";
            let slug = if sculptures {
                models::SCULPTURE_HALL.0
            } else if mosaics {
                models::MOSAIC_HALL.0
            } else {
                s.id.as_str()
            };
            let kind = if sculptures {
                models::KIND_SCULPTURES
            } else if mosaics {
                models::KIND_MOSAICS
            } else {
                models::KIND_PAINTINGS
            };
            let description = if sculptures {
                models::SCULPTURE_HALL.3
            } else if mosaics {
                models::MOSAIC_HALL.3
            } else {
                s.description.as_str()
            };
            let existing: Option<i64> =
                c.query_row("SELECT id FROM sections WHERE slug = ?1", [slug], |r| r.get(0)).optional()?;
            let section_id = match existing {
                Some(id) => id,
                None => {
                    c.execute(
                        r#"INSERT INTO sections (slug, name, tagline, description, accent, wall_color, floor, "order", kind)
                           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
                        params![slug, name, tagline, description, hex(s.accent), hex(s.wall_color), s.floor, order as i64, kind],
                    )?;
                    n_sec += 1;
                    c.last_insert_rowid()
                }
            };

            for p in &s.paintings {
                let existing: Option<i64> = c
                    .query_row(
                        "SELECT id FROM paintings WHERE legacy_id = ?1 AND owner_id IS NULL",
                        [&p.id],
                        |r| r.get(0),
                    )
                    .optional()?;
                if let Some(pid) = existing {
                    if update {
                        c.execute(
                            "UPDATE paintings SET section_id = ?1, title = ?2, artist = ?3, year = ?4,
                                    description = ?5, facts = ?6, updated_at = ?7 WHERE id = ?8",
                            params![section_id, p.title, p.artist, p.year, p.description, p.facts, now_iso(), pid],
                        )?;
                    }
                    n_skip += 1;
                    continue;
                }
                let img = images_dir.join(format!("{}.png", p.id));
                if !img.exists() {
                    eprintln!(
                        "  rasm yo'q, o'tkazildi: {} (virtual_museum ichida `python generate_assets.py` ni ishga tushiring)",
                        img.display()
                    );
                    continue;
                }
                let rel = images::unique_rel_path(&media, "paintings/museum", &format!("{}.png", p.id));
                let full = media.join(&rel);
                std::fs::create_dir_all(full.parent().expect("media ichida"))?;
                std::fs::copy(&img, &full)?;
                let dims = images::normalize_image(&full, 2048);
                let now = now_iso();
                c.execute(
                    "INSERT INTO paintings (section_id, owner_id, title, artist, year, description, facts, image,
                            image_width, image_height, is_published, legacy_id, created_at, updated_at)
                     VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, ?10, ?11, ?11)",
                    params![
                        section_id,
                        p.title,
                        p.artist,
                        p.year,
                        p.description,
                        p.facts,
                        rel,
                        dims.map(|d| d.0),
                        dims.map(|d| d.1),
                        p.id,
                        now
                    ],
                )?;
                n_new += 1;
            }
        }
        models::assign_missing_slots(c)?;
        Ok(())
    })?;
    let n_sculpt = sculpt::ensure_sync(&state.db, &media);
    println!("Tayyor: {n_sec} ta yangi zal, {n_new} ta yangi asar, {n_skip} ta mavjud, {n_sculpt} ta haykal yasaldi.");
    Ok(())
}

//! Django loyihasining bazasi (`museum_web/db.sqlite3`) va `media/` papkasini
//! Rust versiyasiga ko'chiradi. Parollar Django formatida saqlangani uchun
//! foydalanuvchilar eski parollari bilan kira oladi.
use std::path::Path;

use anyhow::{bail, Context};
use chrono::{NaiveDateTime, SecondsFormat};
use rusqlite::{params, Connection, OpenFlags};

use crate::AppState;

/// Django SQLite vaqti ("2026-09-12 06:49:11.123456", UTC) -> RFC 3339.
fn django_dt(s: &str) -> String {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f")
        .map(|d| d.and_utc().to_rfc3339_opts(SecondsFormat::Micros, true))
        .unwrap_or_else(|_| s.to_string())
}

pub fn run(state: &AppState, django_dir: &Path) -> anyhow::Result<()> {
    let src_db = django_dir.join("db.sqlite3");
    let src_media = django_dir.join("media");
    if !src_db.exists() {
        bail!("Django bazasi topilmadi: {}", src_db.display());
    }
    let src = Connection::open_with_flags(&src_db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("ochib bo'lmadi: {}", src_db.display()))?;

    let mut files: Vec<String> = Vec::new();
    let (n_users, n_sections, n_paintings) = state.db.with(|c| -> anyhow::Result<(usize, usize, usize)> {
        let existing: i64 = c.query_row(
            "SELECT (SELECT COUNT(*) FROM users) + (SELECT COUNT(*) FROM sections) + (SELECT COUNT(*) FROM paintings)",
            [],
            |r| r.get(0),
        )?;
        if existing > 0 {
            bail!(
                "Rust bazasi bo'sh emas ({}). Import faqat bo'sh bazaga qilinadi: \
                 bazani o'chirib (yoki MUSEUM_DB ni boshqa faylga qaratib) qayta urinib ko'ring.",
                state.cfg.db_path.display()
            );
        }
        let tx = c.transaction()?;

        // --- foydalanuvchilar
        let mut created_by: Vec<(i64, i64)> = Vec::new();
        let mut n_users = 0;
        {
            let mut q = src.prepare(
                "SELECT id, username, password, first_name, last_name, email, role, is_superuser, is_active,
                        avatar, bio, created_by_id, date_joined, last_login
                 FROM accounts_user ORDER BY id",
            )?;
            let mut rows = q.query([])?;
            while let Some(r) = rows.next()? {
                let id: i64 = r.get(0)?;
                let avatar: String = r.get(9)?;
                let cb: Option<i64> = r.get(11)?;
                let last_login: Option<String> = r.get(13)?;
                let role: String = r.get(6)?;
                tx.execute(
                    "INSERT INTO users (id, username, password, first_name, last_name, email, role, is_superuser,
                                        is_active, avatar, bio, created_by_id, date_joined, last_login)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL, ?12, ?13)",
                    params![
                        id,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, String>(5)?,
                        if role == "admin" { "admin" } else { "user" },
                        r.get::<_, bool>(7)?,
                        r.get::<_, bool>(8)?,
                        avatar,
                        r.get::<_, String>(10)?,
                        django_dt(&r.get::<_, String>(12)?),
                        last_login.map(|s| django_dt(&s)),
                    ],
                )?;
                if let Some(cb) = cb {
                    created_by.push((id, cb));
                }
                if !avatar.is_empty() {
                    files.push(avatar);
                }
                n_users += 1;
            }
        }
        for (id, cb) in created_by {
            tx.execute("UPDATE users SET created_by_id = ?1 WHERE id = ?2", params![cb, id])?;
        }

        // --- zallar
        let mut n_sections = 0;
        {
            let mut q = src.prepare(
                r#"SELECT id, slug, name, tagline, description, accent, wall_color, floor, "order"
                   FROM gallery_section ORDER BY id"#,
            )?;
            let mut rows = q.query([])?;
            while let Some(r) = rows.next()? {
                tx.execute(
                    r#"INSERT INTO sections (id, slug, name, tagline, description, accent, wall_color, floor, "order")
                       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
                    params![
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, String>(6)?,
                        r.get::<_, String>(7)?,
                        r.get::<_, i64>(8)?,
                    ],
                )?;
                n_sections += 1;
            }
        }

        // --- asarlar
        let mut n_paintings = 0;
        {
            let mut q = src.prepare(
                "SELECT id, section_id, owner_id, title, artist, year, description, facts, image, image_width,
                        image_height, is_published, legacy_id, created_at, updated_at
                 FROM gallery_painting ORDER BY id",
            )?;
            let mut rows = q.query([])?;
            while let Some(r) = rows.next()? {
                let image: String = r.get(8)?;
                tx.execute(
                    "INSERT INTO paintings (id, section_id, owner_id, title, artist, year, description, facts, image,
                            image_width, image_height, is_published, legacy_id, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                    params![
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, Option<i64>>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, Option<i64>>(5)?,
                        r.get::<_, String>(6)?,
                        r.get::<_, String>(7)?,
                        image,
                        r.get::<_, Option<i64>>(9)?,
                        r.get::<_, Option<i64>>(10)?,
                        r.get::<_, bool>(11)?,
                        r.get::<_, String>(12)?,
                        django_dt(&r.get::<_, String>(13)?),
                        django_dt(&r.get::<_, String>(14)?),
                    ],
                )?;
                files.push(image);
                n_paintings += 1;
            }
        }
        crate::models::convert_history_hall(&tx)?;
        crate::models::convert_science_hall(&tx)?;
        crate::models::assign_missing_slots(&tx)?;
        tx.commit()?;
        Ok((n_users, n_sections, n_paintings))
    })?;

    // --- media fayllar
    let mut copied = 0;
    for rel in &files {
        if rel.contains("..") {
            continue;
        }
        let from = src_media.join(rel);
        let to = state.cfg.media_dir.join(rel);
        if !from.is_file() {
            eprintln!("  fayl topilmadi, o'tkazildi: {}", from.display());
            continue;
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&from, &to).with_context(|| format!("nusxalab bo'lmadi: {}", from.display()))?;
        copied += 1;
    }
    crate::sculpt::ensure_sync(&state.db, &state.cfg.media_dir);
    println!(
        "Tayyor: {n_users} ta foydalanuvchi, {n_sections} ta zal, {n_paintings} ta asar, {copied} ta fayl ko'chirildi."
    );
    Ok(())
}

//! SQLite ulanishi va sxema (Django migratsiyalari o'rniga).
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use rusqlite::Connection;

use crate::{error::AppError, models};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS users (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    username      TEXT    NOT NULL UNIQUE,
    password      TEXT    NOT NULL,
    first_name    TEXT    NOT NULL DEFAULT '',
    last_name     TEXT    NOT NULL DEFAULT '',
    email         TEXT    NOT NULL DEFAULT '',
    role          TEXT    NOT NULL DEFAULT 'user' CHECK (role IN ('admin', 'user')),
    is_superuser  INTEGER NOT NULL DEFAULT 0,
    is_active     INTEGER NOT NULL DEFAULT 1,
    avatar        TEXT    NOT NULL DEFAULT '',
    bio           TEXT    NOT NULL DEFAULT '',
    created_by_id INTEGER NULL REFERENCES users (id) ON DELETE SET NULL,
    date_joined   TEXT    NOT NULL,
    last_login    TEXT    NULL,
    share_token   TEXT    NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS sections (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    slug        TEXT    NOT NULL UNIQUE,
    name        TEXT    NOT NULL,
    tagline     TEXT    NOT NULL DEFAULT '',
    description TEXT    NOT NULL DEFAULT '',
    accent      TEXT    NOT NULL DEFAULT '#d4af37',
    wall_color  TEXT    NOT NULL DEFAULT '#262a36',
    floor       TEXT    NOT NULL DEFAULT 'floor_marble',
    "order"     INTEGER NOT NULL DEFAULT 0,
    kind        TEXT    NOT NULL DEFAULT 'paintings'
);

CREATE TABLE IF NOT EXISTS paintings (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    section_id   INTEGER NOT NULL REFERENCES sections (id) ON DELETE CASCADE,
    owner_id     INTEGER NULL REFERENCES users (id) ON DELETE CASCADE,
    title        TEXT    NOT NULL,
    artist       TEXT    NOT NULL DEFAULT '',
    year         INTEGER NULL,
    description  TEXT    NOT NULL DEFAULT '',
    facts        TEXT    NOT NULL DEFAULT '',
    image        TEXT    NOT NULL,
    image_width  INTEGER NULL,
    image_height INTEGER NULL,
    is_published INTEGER NOT NULL DEFAULT 1,
    legacy_id    TEXT    NOT NULL DEFAULT '',
    created_at   TEXT    NOT NULL,
    updated_at   TEXT    NOT NULL,
    slot         INTEGER NULL,
    sculpture    TEXT    NOT NULL DEFAULT '',
    depth        TEXT    NOT NULL DEFAULT '',
    mosaic       TEXT    NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS paintings_section ON paintings (section_id);
CREATE INDEX IF NOT EXISTS paintings_owner   ON paintings (owner_id);
CREATE INDEX IF NOT EXISTS paintings_legacy  ON paintings (legacy_id);
CREATE INDEX IF NOT EXISTS paintings_image   ON paintings (image);

CREATE TABLE IF NOT EXISTS sessions (
    key        TEXT    PRIMARY KEY,
    data       TEXT    NOT NULL,
    expires_at INTEGER NOT NULL
);
"#;

fn has_column(c: &Connection, table: &str, col: &str) -> rusqlite::Result<bool> {
    let mut st = c.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = st.query_map([], |r| r.get::<_, String>(1))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(names.iter().any(|n| n == col))
}

/// Eski bazalarni yangi sxemaga moslaydi (ustunlar qo'shiladi, ma'lumotlar saqlanadi).
fn migrate(c: &Connection) -> anyhow::Result<()> {
    for (table, col, def) in [
        ("sections", "kind", "TEXT NOT NULL DEFAULT 'paintings'"),
        ("paintings", "slot", "INTEGER NULL"),
        ("paintings", "sculpture", "TEXT NOT NULL DEFAULT ''"),
        ("paintings", "depth", "TEXT NOT NULL DEFAULT ''"),
        ("paintings", "mosaic", "TEXT NOT NULL DEFAULT ''"),
        ("users", "share_token", "TEXT NULL"),
    ] {
        if !has_column(c, table, col)? {
            c.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {col} {def};"))?;
        }
    }
    // Har bir o'rinda: bitta fond asari va har bir foydalanuvchining bitta asari.
    c.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS paintings_fund_slot ON paintings (section_id, slot)
             WHERE owner_id IS NULL AND slot IS NOT NULL;
         CREATE UNIQUE INDEX IF NOT EXISTS paintings_user_slot ON paintings (owner_id, section_id, slot)
             WHERE owner_id IS NOT NULL AND slot IS NOT NULL;
         CREATE UNIQUE INDEX IF NOT EXISTS users_share_token ON users (share_token)
             WHERE share_token IS NOT NULL;",
    )?;
    // Mavjud foydalanuvchilarga share_token yaratish
    let mut user_st = c.prepare("SELECT id FROM users WHERE share_token IS NULL OR share_token = ''")?;
    let uids: Vec<i64> = user_st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for uid in uids {
        let token = crate::auth::random_string(24);
        c.execute("UPDATE users SET share_token = ?1 WHERE id = ?2", rusqlite::params![token, uid])?;
    }
    let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 1 {
        // "Tarix muzeyi" -> "Haykallar zali": unga yuklangan rasmlar haykalga aylantiriladi.
        models::convert_history_hall(c)?;
        c.execute_batch("PRAGMA user_version = 1;")?;
    }
    if version < 2 {
        // "Fan muzeyi" -> "Mozaika muzeyi": unga yuklangan rasmlar mozaikaga aylantiriladi.
        models::convert_science_hall(c)?;
        c.execute_batch("PRAGMA user_version = 2;")?;
    }
    if version < 3 {
        // "Texnologiya muzeyi" -> "Love Story": qizil gilamlar, atirgullar va romantik muhit.
        models::convert_love_hall(c)?;
        c.execute_batch("PRAGMA user_version = 3;")?;
    }
    if version < 4 {
        // "O'zbekiston merosi" -> "Marry Me": romantik taklif zali, qizil darashkalar, atirgullar va 1 ta markaziy rasm.
        models::convert_marry_me_hall(c)?;
        c.execute_batch("PRAGMA user_version = 4;")?;
    }
    models::assign_missing_slots(c)?;
    Ok(())
}

/// Bitta umumiy ulanish. SQLite baribir yozuvlarni ketma-ket bajaradi,
/// shuning uchun kichik muzey sayti uchun bu yetarli va sodda.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        Ok(Db { conn: Arc::new(Mutex::new(conn)) })
    }

    /// Bazaga oid ishni alohida (bloklanadigan) oqimda bajaradi.
    pub async fn call<T, F>(&self, f: F) -> Result<T, AppError>
    where
        F: FnOnce(&mut Connection) -> Result<T, AppError> + Send + 'static,
        T: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = conn.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut guard)
        })
        .await?
    }

    /// Sinxron foydalanish (CLI buyruqlari uchun).
    pub fn with<T>(&self, f: impl FnOnce(&mut Connection) -> T) -> T {
        let mut guard = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut guard)
    }
}

//! Modellar: User, Section, Painting va ularning so'rovlari.
//!
//! Ko'rinish qoidasi butun loyihada faqat shu yerda belgilanadi
//! (`visible_sql` va `Painting::can_view`):
//!   * Muzey fondi (owner yo'q)      -> tizimga kirgan hamma.
//!   * Foydalanuvchi yuklagan asar   -> faqat egasi va admin.
//!   * Tizimga kirmagan mehmon       -> hech narsa.
use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, Params, Row};
use serde::Serialize;

use crate::error::AppError;

pub const ROLE_ADMIN: &str = "admin";
pub const ROLE_USER: &str = "user";

pub const FLOORS: &[(&str, &str)] =
    &[("floor_marble", "Marmar"), ("floor_stone", "Tosh"), ("floor_dark", "To'q")];

/// Har bir zalda nechta o'rin (rasm yoki haykal) bor.
pub const SLOTS: i64 = 8;

pub const KIND_PAINTINGS: &str = "paintings";
pub const KIND_SCULPTURES: &str = "sculptures";
pub const KIND_MOSAICS: &str = "mosaics";
pub const KIND_LOVE: &str = "love";
pub const KIND_MARRY_ME: &str = "marry_me";
pub const KINDS: &[(&str, &str)] = &[
    (KIND_PAINTINGS, "Rasmlar zali (rasmlar devorga osiladi)"),
    (KIND_SCULPTURES, "Haykallar zali (rasm haykalga aylantiriladi)"),
    (KIND_MOSAICS, "Mozaika zali (rasm koshinli mozaikaga aylantiriladi)"),
    (KIND_LOVE, "Love Story (qizil gilamlar, atirgullar va romantik muhit)"),
    (KIND_MARRY_ME, "Marry Me (romantik taklif zali, qizil darashkalar, atirgullar va 1 ta markaziy rasm)"),
];

/// Sobiq "Tarix muzeyi": (kod, nomi, shiori, tavsifi).
pub const SCULPTURE_HALL: (&str, &str, &str, &str) = (
    "sculptures",
    "Haykallar zali",
    "Rasmlardan tug'ilgan marmar haykallar",
    "Bu zalga yuklangan har bir rasm avtomatik ravishda marmar haykalga aylantiriladi va postament ustiga qo'yiladi.",
);

/// Sobiq "Fan muzeyi": (kod, nomi, shiori, tavsifi).
pub const MOSAIC_HALL: (&str, &str, &str, &str) = (
    "mosaics",
    "Mozaika muzeyi",
    "Rang-barang koshinlar va toshlar san'ati",
    "Bu zalga yuklangan har bir rasm avtomatik ravishda qadimiy koshinli tosh mozaikaga aylantiriladi va devorga joylashtiriladi.",
);

/// Sobiq "Texnologiya muzeyi": (kod, nomi, shiori, tavsifi).
pub const LOVE_HALL: (&str, &str, &str, &str) = (
    "love",
    "Love Story",
    "Cheksiz muhabbat va samimiy tuyg'ular xiyoboni",
    "Yuraklar uyg'unligi, sof sevgining go'zal lahzalari va unutilmas romantik xotiralar qasri.",
);

/// Sobiq "O'zbekiston merosi": (kod, nomi, shiori, tavsifi).
pub const MARRY_ME_HALL: (&str, &str, &str, &str) = (
    "marry_me",
    "Marry Me",
    "Menga turmushga chiqasanmi? 💍",
    "Eng unutilmas romantik taklif va sevgi izhori zali. Qizil darashkalar, qizil gilamlar, minglab atirgullar va faqat siz uchun yagona markaziy asar.",
);

// ---------------------------------------------------------------- vaqt

pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// UTC vaqtni Toshkent vaqtida (UTC+5) formatlaydi.
pub fn fmt_local(iso: &str, fmt: &str) -> String {
    let tz = FixedOffset::east_opt(5 * 3600).expect("valid offset");
    DateTime::parse_from_rfc3339(iso)
        .map(|d| d.with_timezone(&tz).format(fmt).to_string())
        .unwrap_or_default()
}

pub fn media_url(rel: &str) -> String {
    format!("/media/{rel}")
}

fn initial(s: &str) -> String {
    s.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default()
}

// ---------------------------------------------------------------- User

#[derive(Clone, Debug)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub role: String,
    pub is_superuser: bool,
    pub is_active: bool,
    pub avatar: String,
    pub bio: String,
    pub created_by_id: Option<i64>,
    pub date_joined: String,
    pub last_login: Option<String>,
    pub share_token: Option<String>,
}

pub const USER_COLS: &[&str] = &[
    "id", "username", "password", "first_name", "last_name", "email", "role", "is_superuser",
    "is_active", "avatar", "bio", "created_by_id", "date_joined", "last_login", "share_token",
];
pub const USER_COLS_COUNT: usize = USER_COLS.len();

pub fn user_cols(prefix: &str) -> String {
    USER_COLS
        .iter()
        .map(|c| format!("{prefix}{c}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `user_cols` tartibida o'qiydi, `offset` — birinchi ustun indeksi.
pub fn user_from_row(r: &Row, o: usize) -> rusqlite::Result<User> {
    Ok(User {
        id: r.get(o)?,
        username: r.get(o + 1)?,
        password: r.get(o + 2)?,
        first_name: r.get(o + 3)?,
        last_name: r.get(o + 4)?,
        email: r.get(o + 5)?,
        role: r.get(o + 6)?,
        is_superuser: r.get(o + 7)?,
        is_active: r.get(o + 8)?,
        avatar: r.get(o + 9)?,
        bio: r.get(o + 10)?,
        created_by_id: r.get(o + 11)?,
        date_joined: r.get(o + 12)?,
        last_login: r.get(o + 13)?,
        share_token: r.get(o + 14).ok(),
    })
}

/// Shablonlarga beriladigan ko'rinish.
#[derive(Serialize, Clone, Debug)]
pub struct UserView {
    pub id: i64,
    pub username: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub bio: String,
    pub full_name: String,
    pub display_name: String,
    pub initial: String,
    pub role: String,
    pub role_display: String,
    pub is_museum_admin: bool,
    pub is_superuser: bool,
    pub is_active: bool,
    pub is_authenticated: bool,
    pub avatar_url: Option<String>,
    pub date_joined: String,
    pub share_token: Option<String>,
}

impl User {
    pub fn is_museum_admin(&self) -> bool {
        self.is_active && (self.role == ROLE_ADMIN || self.is_superuser)
    }

    pub fn full_name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name).trim().to_string()
    }

    pub fn display_name(&self) -> String {
        let n = self.full_name();
        if n.is_empty() {
            self.username.clone()
        } else {
            n
        }
    }

    pub fn view(&self) -> UserView {
        UserView {
            id: self.id,
            username: self.username.clone(),
            first_name: self.first_name.clone(),
            last_name: self.last_name.clone(),
            email: self.email.clone(),
            bio: self.bio.clone(),
            full_name: self.full_name(),
            display_name: self.display_name(),
            initial: initial(&self.username),
            role: self.role.clone(),
            role_display: role_display(&self.role).into(),
            is_museum_admin: self.is_museum_admin(),
            is_superuser: self.is_superuser,
            is_active: self.is_active,
            is_authenticated: true,
            avatar_url: (!self.avatar.is_empty()).then(|| media_url(&self.avatar)),
            date_joined: fmt_local(&self.date_joined, "%d.%m.%Y"),
            share_token: self.share_token.clone(),
        }
    }

    /// Parol o'xshashligini tekshirish uchun atributlar.
    pub fn attrs(&self) -> Vec<(&'static str, String)> {
        vec![
            ("login", self.username.clone()),
            ("ism", self.first_name.clone()),
            ("familiya", self.last_name.clone()),
            ("email", self.email.clone()),
        ]
    }
}

pub fn role_display(role: &str) -> &'static str {
    if role == ROLE_ADMIN {
        "Admin"
    } else {
        "Foydalanuvchi"
    }
}

pub fn user_by_id(c: &Connection, id: i64) -> rusqlite::Result<Option<User>> {
    c.query_row(
        &format!("SELECT {} FROM users WHERE id = ?1", user_cols("")),
        [id],
        |r| user_from_row(r, 0),
    )
    .optional()
}

pub fn user_by_username(c: &Connection, username: &str) -> rusqlite::Result<Option<User>> {
    c.query_row(
        &format!("SELECT {} FROM users WHERE username = ?1", user_cols("")),
        [username],
        |r| user_from_row(r, 0),
    )
    .optional()
}

/// Login band emasligini tekshiradi (katta-kichik harf farqsiz).
pub fn username_taken(c: &Connection, username: &str, except_id: Option<i64>) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM users WHERE username = ?1 COLLATE NOCASE AND id != ?2)",
        params![username, except_id.unwrap_or(-1)],
        |r| r.get(0),
    )
}

pub struct NewUser<'a> {
    pub username: &'a str,
    pub password_hash: &'a str,
    pub first_name: &'a str,
    pub last_name: &'a str,
    pub email: &'a str,
    pub role: &'a str,
    pub is_superuser: bool,
    pub created_by_id: Option<i64>,
}

pub fn insert_user(c: &Connection, u: &NewUser) -> rusqlite::Result<i64> {
    let token = crate::auth::random_string(24);
    c.execute(
        "INSERT INTO users (username, password, first_name, last_name, email, role, is_superuser,
                            is_active, created_by_id, date_joined, share_token)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?9, ?10)",
        params![
            u.username,
            u.password_hash,
            u.first_name,
            u.last_name,
            u.email,
            u.role,
            u.is_superuser,
            u.created_by_id,
            now_iso(),
            token
        ],
    )?;
    Ok(c.last_insert_rowid())
}

pub fn user_by_share_token(c: &Connection, token: &str) -> rusqlite::Result<Option<User>> {
    c.query_row(
        &format!("SELECT {} FROM users WHERE share_token = ?1 AND is_active = 1", user_cols("")),
        [token],
        |r| user_from_row(r, 0),
    )
    .optional()
}

pub fn ensure_share_token(c: &Connection, user_id: i64) -> rusqlite::Result<String> {
    let existing: Option<String> = c
        .query_row("SELECT share_token FROM users WHERE id = ?1", [user_id], |r| r.get(0))
        .optional()?
        .flatten();
    if let Some(token) = existing {
        if !token.is_empty() {
            return Ok(token);
        }
    }
    let token = crate::auth::random_string(24);
    c.execute("UPDATE users SET share_token = ?1 WHERE id = ?2", params![token, user_id])?;
    Ok(token)
}

// ---------------------------------------------------------------- Section

#[derive(Serialize, Clone, Debug)]
pub struct Section {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub tagline: String,
    pub description: String,
    pub accent: String,
    pub wall_color: String,
    pub floor: String,
    pub order: i64,
    pub kind: String,
    pub is_sculpture: bool,
    pub is_mosaic: bool,
    pub is_love: bool,
    pub is_marry_me: bool,
    pub url: String,
    /// Shu foydalanuvchi zalida turgan asarlar soni (bosh sahifa uchun).
    pub visible_count: i64,
}

const SECTION_COLS: &str = r#"id, slug, name, tagline, description, accent, wall_color, floor, "order", kind"#;
const SECTION_ORDER: &str = r#"ORDER BY "order", name"#;

fn section_from_row(r: &Row) -> rusqlite::Result<Section> {
    let slug: String = r.get(1)?;
    let is_marry_me = r.get::<_, String>(9)? == KIND_MARRY_ME || slug == "marry_me" || slug == "uzbekistan";
    Ok(Section {
        id: r.get(0)?,
        url: format!("/hall/{slug}/"),
        slug,
        name: r.get(2)?,
        tagline: r.get(3)?,
        description: r.get(4)?,
        accent: r.get(5)?,
        wall_color: r.get(6)?,
        floor: r.get(7)?,
        order: r.get(8)?,
        is_sculpture: r.get::<_, String>(9)? == KIND_SCULPTURES,
        is_mosaic: r.get::<_, String>(9)? == KIND_MOSAICS,
        is_love: r.get::<_, String>(9)? == KIND_LOVE,
        is_marry_me,
        kind: r.get(9)?,
        visible_count: 0,
    })
}

pub fn all_sections(c: &Connection) -> rusqlite::Result<Vec<Section>> {
    let mut st = c.prepare(&format!("SELECT {SECTION_COLS} FROM sections {SECTION_ORDER}"))?;
    let rows = st.query_map([], section_from_row)?;
    rows.collect()
}

pub fn section_by_slug(c: &Connection, slug: &str) -> rusqlite::Result<Option<Section>> {
    let alt = if slug == "uzbekistan" {
        "marry_me"
    } else if slug == "marry_me" {
        "uzbekistan"
    } else {
        slug
    };
    c.query_row(
        &format!("SELECT {SECTION_COLS} FROM sections WHERE slug = ?1 OR slug = ?2 ORDER BY CASE WHEN slug = ?1 THEN 0 ELSE 1 END LIMIT 1"),
        params![slug, alt],
        section_from_row,
    )
    .optional()
}

pub fn section_by_id(c: &Connection, id: i64) -> rusqlite::Result<Option<Section>> {
    c.query_row(&format!("SELECT {SECTION_COLS} FROM sections WHERE id = ?1"), [id], section_from_row)
        .optional()
}

#[derive(Serialize)]
pub struct NavSection {
    pub name: String,
    pub url: String,
}

pub fn nav_sections(c: &Connection) -> rusqlite::Result<Vec<NavSection>> {
    let mut st = c.prepare(&format!("SELECT slug, name FROM sections {SECTION_ORDER}"))?;
    let rows = st.query_map([], |r| {
        let slug: String = r.get(0)?;
        Ok(NavSection { name: r.get(1)?, url: format!("/hall/{slug}/") })
    })?;
    rows.collect()
}

// ---------------------------------------------------------------- Painting

#[derive(Clone, Debug)]
pub struct Painting {
    pub id: i64,
    pub section_id: i64,
    pub owner_id: Option<i64>,
    pub title: String,
    pub artist: String,
    pub year: Option<i64>,
    pub description: String,
    pub facts: String,
    pub image: String,
    pub image_width: Option<i64>,
    pub image_height: Option<i64>,
    pub is_published: bool,
    pub legacy_id: String,
    pub created_at: String,
    pub section_slug: String,
    pub section_name: String,
    pub section_accent: String,
    pub owner_username: Option<String>,
    pub owner_first_name: Option<String>,
    pub owner_last_name: Option<String>,
    pub owner_avatar: Option<String>,
    pub slot: Option<i64>,
    /// Haykal zalida: haykalning "surati" (2D ko'rinish) va chuqurlik xaritasi.
    pub sculpture: String,
    pub depth: String,
    pub mosaic: String,
    pub section_kind: String,
}

const PAINTING_SELECT: &str = "SELECT p.id, p.section_id, p.owner_id, p.title, p.artist, p.year,
        p.description, p.facts, p.image, p.image_width, p.image_height, p.is_published,
        p.legacy_id, p.created_at, s.slug, s.name, s.accent,
        u.username, u.first_name, u.last_name, u.avatar,
        p.slot, p.sculpture, p.depth, s.kind, p.mosaic
    FROM paintings p
    JOIN sections s ON s.id = p.section_id
    LEFT JOIN users u ON u.id = p.owner_id";

/// Standart tartib (Django: `ordering = ["section__order", "created_at", "id"]`).
pub const DEFAULT_ORDER: &str = r#"ORDER BY s."order", p.created_at, p.id"#;

fn painting_from_row(r: &Row) -> rusqlite::Result<Painting> {
    Ok(Painting {
        id: r.get(0)?,
        section_id: r.get(1)?,
        owner_id: r.get(2)?,
        title: r.get(3)?,
        artist: r.get(4)?,
        year: r.get(5)?,
        description: r.get(6)?,
        facts: r.get(7)?,
        image: r.get(8)?,
        image_width: r.get(9)?,
        image_height: r.get(10)?,
        is_published: r.get(11)?,
        legacy_id: r.get(12)?,
        created_at: r.get(13)?,
        section_slug: r.get(14)?,
        section_name: r.get(15)?,
        section_accent: r.get(16)?,
        owner_username: r.get(17)?,
        owner_first_name: r.get(18)?,
        owner_last_name: r.get(19)?,
        owner_avatar: r.get(20)?,
        slot: r.get(21)?,
        sculpture: r.get(22)?,
        depth: r.get(23)?,
        section_kind: r.get(24)?,
        mosaic: r.get(25).unwrap_or_default(),
    })
}

/// `WHERE` sharti va tartib bilan asarlar ro'yxati.
pub fn paintings_where<P: Params>(
    c: &Connection,
    where_sql: &str,
    order_sql: &str,
    params: P,
) -> rusqlite::Result<Vec<Painting>> {
    let sql = format!("{PAINTING_SELECT} WHERE {where_sql} {order_sql}");
    let mut st = c.prepare(&sql)?;
    let rows = st.query_map(params, painting_from_row)?;
    rows.collect()
}

pub fn painting_by_id(c: &Connection, id: i64) -> rusqlite::Result<Option<Painting>> {
    Ok(paintings_where(c, "p.id = ?1", "", [id])?.into_iter().next())
}

pub fn painting_by_image(c: &Connection, image: &str) -> rusqlite::Result<Option<Painting>> {
    Ok(paintings_where(c, "p.image = ?1 OR p.sculpture = ?1 OR p.depth = ?1 OR p.mosaic = ?1", "LIMIT 1", [image])?
        .into_iter()
        .next())
}

/// Foydalanuvchi ko'ra oladigan asarlar uchun SQL sharti (jadval taxallusi `p`).
pub fn visible_sql(user: Option<&User>) -> String {
    match user {
        Some(u) if u.is_active => {
            if u.is_museum_admin() {
                "p.is_published = 1".into()
            } else {
                format!("p.is_published = 1 AND (p.owner_id IS NULL OR p.owner_id = {})", u.id)
            }
        }
        _ => "0".into(),
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct SectionRef {
    pub slug: String,
    pub name: String,
    pub accent: String,
    pub url: String,
    pub is_sculpture: bool,
    pub is_mosaic: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct OwnerView {
    pub username: String,
    pub display_name: String,
    pub initial: String,
    pub avatar_url: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct PaintingView {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub year: Option<i64>,
    pub description: String,
    pub facts: String,
    pub image_url: String,
    pub original_url: String,
    pub is_sculpture: bool,
    pub is_mosaic: bool,
    pub slot: Option<i64>,
    pub url: String,
    pub author_name: String,
    pub is_published: bool,
    pub created_at: String,
    pub section: SectionRef,
    pub owner: Option<OwnerView>,
}

impl Painting {
    /// Alohida asar sahifasi va rasm fayli uchun. Egasi va admin yashirilgan
    /// asarni ham ko'radi; fond asari — tizimga kirgan hamma uchun.
    pub fn can_view(&self, user: Option<&User>) -> bool {
        let Some(u) = user else { return false };
        if !u.is_active {
            return false;
        }
        if u.is_museum_admin() || self.owner_id == Some(u.id) {
            return true;
        }
        self.owner_id.is_none() && self.is_published
    }

    pub fn owner_display_name(&self) -> Option<String> {
        let username = self.owner_username.as_ref()?;
        let full = format!(
            "{} {}",
            self.owner_first_name.as_deref().unwrap_or(""),
            self.owner_last_name.as_deref().unwrap_or("")
        )
        .trim()
        .to_string();
        Some(if full.is_empty() { username.clone() } else { full })
    }

    pub fn author_name(&self) -> String {
        if !self.artist.is_empty() {
            return self.artist.clone();
        }
        self.owner_display_name().unwrap_or_else(|| "—".into())
    }

    /// Haykal zalidagi va haykalga aylantirilgan asar.
    pub fn is_sculpture(&self) -> bool {
        self.section_kind == KIND_SCULPTURES && !self.depth.is_empty() && !self.sculpture.is_empty()
    }

    /// Mozaika zalidagi va mozaikaga aylantirilgan asar.
    pub fn is_mosaic(&self) -> bool {
        self.section_kind == KIND_MOSAICS && !self.mosaic.is_empty()
    }

    /// Ko'rsatiladigan rasm: haykal bo'lsa — haykal surati, mozaika bo'lsa — mozaika surati, aks holda asl rasm.
    pub fn image_url(&self) -> String {
        if self.is_sculpture() {
            media_url(&self.sculpture)
        } else if self.is_mosaic() {
            media_url(&self.mosaic)
        } else {
            media_url(&self.image)
        }
    }

    /// Asarga tegishli barcha media fayllar (o'chirishda kerak).
    pub fn files(&self) -> Vec<String> {
        [&self.image, &self.sculpture, &self.depth, &self.mosaic].into_iter().filter(|f| !f.is_empty()).cloned().collect()
    }

    pub fn url(&self) -> String {
        format!("/art/{}/", self.id)
    }

    pub fn view(&self) -> PaintingView {
        PaintingView {
            id: self.id,
            title: self.title.clone(),
            artist: self.artist.clone(),
            year: self.year,
            description: self.description.clone(),
            facts: self.facts.clone(),
            image_url: self.image_url(),
            original_url: media_url(&self.image),
            is_sculpture: self.is_sculpture(),
            is_mosaic: self.is_mosaic(),
            slot: self.slot,
            url: self.url(),
            author_name: self.author_name(),
            is_published: self.is_published,
            created_at: fmt_local(&self.created_at, "%d.%m.%Y %H:%M"),
            section: SectionRef {
                slug: self.section_slug.clone(),
                name: self.section_name.clone(),
                accent: self.section_accent.clone(),
                url: format!("/hall/{}/", self.section_slug),
                is_sculpture: self.section_kind == KIND_SCULPTURES,
                is_mosaic: self.section_kind == KIND_MOSAICS,
            },
            owner: self.owner_username.as_ref().map(|username| OwnerView {
                username: username.clone(),
                display_name: self.owner_display_name().unwrap_or_default(),
                initial: initial(username),
                avatar_url: self
                    .owner_avatar
                    .as_ref()
                    .filter(|a| !a.is_empty())
                    .map(|a| media_url(a)),
            }),
        }
    }
}

pub fn views(list: &[Painting]) -> Vec<PaintingView> {
    list.iter().map(Painting::view).collect()
}

// ---------------------------------------------------------------- zal o'rinlari

/// Zal -> (o'rin -> asar): shu tashrifchi zallarda ko'radigan asarlar.
///
/// Har bir zalda `SLOTS` ta o'rin bor. Avval muzey fondi asarlari o'z o'rinlariga
/// qo'yiladi, so'ng tashrifchining o'z asarlari: agar u fond asari turgan o'ringa
/// rasm qo'ygan bo'lsa, fond asari (faqat shu tashrifchi uchun) almashtiriladi.
/// Boshqa foydalanuvchilarning asarlari hech qachon ko'rinmaydi.
pub fn hall_layout(
    c: &Connection,
    user: Option<&User>,
    section_id: Option<i64>,
) -> rusqlite::Result<HashMap<i64, BTreeMap<i64, Painting>>> {
    let mut out: HashMap<i64, BTreeMap<i64, Painting>> = HashMap::new();
    let Some(u) = user.filter(|u| u.is_active) else { return Ok(out) };
    let w = format!(
        "p.is_published = 1 AND p.slot BETWEEN 1 AND {SLOTS} AND (p.owner_id IS NULL OR p.owner_id = ?1)
         AND (?2 IS NULL OR p.section_id = ?2)"
    );
    // fond asarlari avval, keyin tashrifchinikilari (ular ustiga yoziladi)
    for p in paintings_where(c, &w, "ORDER BY p.owner_id IS NOT NULL, p.id", params![u.id, section_id])? {
        out.entry(p.section_id).or_default().insert(p.slot.unwrap_or_default(), p);
    }
    Ok(out)
}

/// Foydalanuvchining shu zal va o'rindagi asari (`except` — tekshirilmaydigan asar).
pub fn own_at_slot(
    c: &Connection,
    owner: i64,
    section: i64,
    slot: i64,
    except: Option<i64>,
) -> rusqlite::Result<Option<Painting>> {
    Ok(paintings_where(
        c,
        "p.owner_id = ?1 AND p.section_id = ?2 AND p.slot = ?3 AND p.id != ?4",
        "LIMIT 1",
        params![owner, section, slot, except.unwrap_or(-1)],
    )?
    .into_iter()
    .next())
}

/// Shu zaldagi fond asari (o'rin bo'yicha).
pub fn fund_at_slot(c: &Connection, section: i64, slot: i64) -> rusqlite::Result<Option<Painting>> {
    Ok(paintings_where(c, "p.owner_id IS NULL AND p.section_id = ?1 AND p.slot = ?2", "LIMIT 1", params![section, slot])?
        .into_iter()
        .next())
}

fn used_slots(c: &Connection, where_sql: &str, p: impl Params) -> rusqlite::Result<Vec<i64>> {
    let mut st = c.prepare(&format!("SELECT slot FROM paintings WHERE slot IS NOT NULL AND {where_sql}"))?;
    let rows = st.query_map(p, |r| r.get(0))?;
    rows.collect()
}

/// Foydalanuvchi uchun avtomatik o'rin: o'z asari yo'q, iloji bo'lsa fond asari ham yo'q o'rin.
pub fn auto_slot(c: &Connection, owner: i64, section: i64, except: Option<i64>) -> rusqlite::Result<Option<i64>> {
    let own = used_slots(c, "owner_id = ?1 AND section_id = ?2 AND id != ?3", params![owner, section, except.unwrap_or(-1)])?;
    let fund = used_slots(c, "owner_id IS NULL AND section_id = ?1", [section])?;
    let free: Vec<i64> = (1..=SLOTS).filter(|s| !own.contains(s)).collect();
    Ok(free.iter().copied().find(|s| !fund.contains(s)).or(free.first().copied()))
}

/// "Tarix muzeyi" (`history`) zalini "Haykallar zali"ga aylantiradi.
pub fn convert_history_hall(c: &Connection) -> rusqlite::Result<usize> {
    c.execute(
        "UPDATE sections SET slug = ?1, name = ?2, tagline = ?3, description = ?4, kind = ?5
         WHERE slug = 'history' AND NOT EXISTS (SELECT 1 FROM sections WHERE slug = ?1)",
        params![SCULPTURE_HALL.0, SCULPTURE_HALL.1, SCULPTURE_HALL.2, SCULPTURE_HALL.3, KIND_SCULPTURES],
    )
}

/// "Fan muzeyi" (`science`) zalini "Mozaika muzeyi"ga aylantiradi.
pub fn convert_science_hall(c: &Connection) -> rusqlite::Result<usize> {
    c.execute(
        "UPDATE sections SET slug = ?1, name = ?2, tagline = ?3, description = ?4, kind = ?5
         WHERE (slug = 'science' OR slug = 'mosaic' OR slug = 'mosaics')",
        params![MOSAIC_HALL.0, MOSAIC_HALL.1, MOSAIC_HALL.2, MOSAIC_HALL.3, KIND_MOSAICS],
    )
}

/// "Texnologiya muzeyi" (`technology`) zalini "Love Story"ga aylantiradi.
pub fn convert_love_hall(c: &Connection) -> rusqlite::Result<usize> {
    c.execute(
        "UPDATE sections SET slug = ?1, name = ?2, tagline = ?3, description = ?4, accent = ?5, wall_color = ?6, kind = ?7
         WHERE (slug = 'technology' OR slug = 'love')",
        params![LOVE_HALL.0, LOVE_HALL.1, LOVE_HALL.2, LOVE_HALL.3, "#e84393", "#2d1422", KIND_LOVE],
    )
}

/// "O'zbekiston merosi" (`uzbekistan`) zalini "Marry Me"ga aylantiradi.
pub fn convert_marry_me_hall(c: &Connection) -> rusqlite::Result<usize> {
    let n = c.execute(
        "UPDATE sections SET slug = ?1, name = ?2, tagline = ?3, description = ?4, accent = ?5, wall_color = ?6, kind = ?7
         WHERE (slug = 'uzbekistan' OR slug = 'marry_me' OR slug = 'marryme')",
        params![MARRY_ME_HALL.0, MARRY_ME_HALL.1, MARRY_ME_HALL.2, MARRY_ME_HALL.3, "#ff2a6d", "#240d18", KIND_MARRY_ME],
    )?;

    // Markaziy asarni yangilash (slot = 1)
    let _ = c.execute(
        "UPDATE paintings SET title = ?1, artist = ?2, year = 2026,
                 description = ?3, facts = ?4,
                 image = 'paintings/museum/marry_me_center.png',
                 image_width = 1376, image_height = 768,
                 updated_at = ?5
         WHERE section_id IN (SELECT id FROM sections WHERE slug = 'marry_me' OR slug = 'uzbekistan') AND slot = 1",
        params![
            "Menga turmushga chiqasanmi? 💍",
            "Abadiy Sevgi Qasami",
            "Yuraklar birlashgan eng unutilmas romantik taklif lahzasi. Qizil atirgullar, shamlar yog'dusi va abadiy va'da.",
            "Ushbu mo'jizaviy zaldagi yagona asar eng muhim savol uchun maxsus yaratilgan: 'Ha!' degan javob bu yerda abadiylikka muhrlanadi.",
            now_iso()
        ],
    );

    // Qolgan rasmlarni o'rinsiz qilib qo'yish (faqat o'rtada 1 ta rasm bo'lishi uchun)
    let _ = c.execute(
        "UPDATE paintings SET slot = NULL
         WHERE section_id IN (SELECT id FROM sections WHERE slug = 'marry_me' OR slug = 'uzbekistan') AND slot > 1",
        [],
    );

    Ok(n)
}

/// O'rni belgilanmagan asarlarga o'rin beradi (eski bazalar, seed, import uchun).
/// Joy qolmagan asarlar o'rinsiz qoladi: ular profilda ko'rinadi, zalda emas.
pub fn assign_missing_slots(c: &Connection) -> rusqlite::Result<()> {
    let pending: Vec<(i64, i64, Option<i64>)> = {
        let mut st = c.prepare(
            "SELECT id, section_id, owner_id FROM paintings WHERE slot IS NULL ORDER BY owner_id IS NOT NULL, created_at, id",
        )?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for (id, section, owner) in pending {
        let slot = match owner {
            None => {
                let fund = used_slots(c, "owner_id IS NULL AND section_id = ?1", [section])?;
                (1..=SLOTS).find(|s| !fund.contains(s))
            }
            Some(o) => auto_slot(c, o, section, Some(id))?,
        };
        if let Some(slot) = slot {
            c.execute("UPDATE paintings SET slot = ?1 WHERE id = ?2", params![slot, id])?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- o'chirish (fayllar bilan)

/// Foydalanuvchini o'chiradi va o'chirilishi kerak bo'lgan media fayllar ro'yxatini qaytaradi.
pub fn delete_user(c: &mut Connection, id: i64) -> Result<Vec<String>, AppError> {
    let tx = c.transaction()?;
    let mut files: Vec<String> = {
        let mut st = tx.prepare("SELECT image, sculpture, depth FROM paintings WHERE owner_id = ?1")?;
        let rows = st.query_map([id], |r| Ok([r.get::<_, String>(0)?, r.get(1)?, r.get(2)?]))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?.into_iter().flatten().filter(|f| !f.is_empty()).collect()
    };
    let avatar: String = tx.query_row("SELECT avatar FROM users WHERE id = ?1", [id], |r| r.get(0))?;
    if !avatar.is_empty() {
        files.push(avatar);
    }
    tx.execute("DELETE FROM users WHERE id = ?1", [id])?;
    tx.commit()?;
    Ok(files)
}

/// Zalni (va undagi barcha asarlarni) o'chiradi; o'chiriladigan fayllarni qaytaradi.
pub fn delete_section(c: &mut Connection, id: i64) -> Result<Vec<String>, AppError> {
    let tx = c.transaction()?;
    let files: Vec<String> = {
        let mut st = tx.prepare("SELECT image, sculpture, depth FROM paintings WHERE section_id = ?1")?;
        let rows = st.query_map([id], |r| Ok([r.get::<_, String>(0)?, r.get(1)?, r.get(2)?]))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?.into_iter().flatten().filter(|f| !f.is_empty()).collect()
    };
    tx.execute("DELETE FROM sections WHERE id = ?1", [id])?;
    tx.commit()?;
    Ok(files)
}

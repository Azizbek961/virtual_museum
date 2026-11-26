//! Sessiyalar, tizimga kirgan foydalanuvchi, flash xabarlar va CSRF himoyasi
//! (Django'dagi Session/Auth/Messages/CSRF middleware'lari o'rniga).
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};

use axum::{
    body::Bytes,
    extract::{FromRequest, FromRequestParts, Multipart, Request, State},
    http::{header, request::Parts, HeaderMap, HeaderValue},
    middleware::Next,
    response::Response,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;

use crate::{
    auth,
    error::{AppError, ErrorPage},
    models::{self, User},
    render, AppState,
};

pub const COOKIE_NAME: &str = "museum_session";
const SESSION_AGE_SECS: i64 = 14 * 24 * 3600;
const KEY_LEN: usize = 40;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Flash {
    pub level: String,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct SessionData {
    user_id: Option<i64>,
    #[serde(default)]
    guest_sharer_id: Option<i64>,
    #[serde(default)]
    auth_hash: String,
    csrf: String,
    #[serde(default)]
    messages: Vec<Flash>,
}

impl SessionData {
    fn fresh() -> Self {
        SessionData {
            user_id: None,
            guest_sharer_id: None,
            auth_hash: String::new(),
            csrf: auth::random_string(48),
            messages: vec![],
        }
    }
}

struct Inner {
    key: String,
    /// Bazada saqlangan kalit (cookie'dan kelgan). `None` — yangi sessiya.
    stored: Option<String>,
    data: SessionData,
    dirty: bool,
    /// Kalitni almashtirish (login/logout'da — session fixation'dan himoya).
    rotate: bool,
}

/// So'rov konteksti: foydalanuvchi va sessiya. Har bir handler'da olinadi.
#[derive(Clone)]
pub struct Ctx {
    pub user: Option<User>,
    pub path: String,
    pub full_path: String,
    inner: Arc<Mutex<Inner>>,
}

impl Ctx {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `@login_required` — tizimga kirmagan bo'lsa login sahifasiga yo'naltiradi.
    pub fn require_login(&self) -> Result<User, AppError> {
        self.user.clone().ok_or_else(|| AppError::LoginRequired(self.full_path.clone()))
    }

    /// `@admin_required` — faqat Admin roli (yoki superuser).
    pub fn require_admin(&self) -> Result<User, AppError> {
        let user = self.require_login()?;
        if !user.is_museum_admin() {
            return Err(AppError::Forbidden("Bu sahifa faqat administrator uchun.".into()));
        }
        Ok(user)
    }

    pub fn csrf_token(&self) -> String {
        let mut i = self.lock();
        if i.stored.is_none() {
            // token sahifaga chiqdi — sessiyani saqlash kerak
            i.dirty = true;
        }
        i.data.csrf.clone()
    }

    pub fn check_csrf(&self, token: Option<&str>) -> Result<(), AppError> {
        let i = self.lock();
        let expected = i.data.csrf.as_bytes();
        match token {
            Some(t) if !expected.is_empty() && t.len() == expected.len() && bool::from(t.as_bytes().ct_eq(expected)) => {
                Ok(())
            }
            _ => Err(AppError::Forbidden(
                "CSRF tekshiruvi muvaffaqiyatsiz. Sahifani yangilab, qaytadan urinib ko'ring.".into(),
            )),
        }
    }

    pub fn success(&self, text: impl Into<String>) {
        self.flash("success", text.into());
    }

    pub fn error(&self, text: impl Into<String>) {
        self.flash("error", text.into());
    }

    fn flash(&self, level: &str, text: String) {
        let mut i = self.lock();
        i.data.messages.push(Flash { level: level.into(), text });
        i.dirty = true;
    }

    pub fn take_messages(&self) -> Vec<Flash> {
        let mut i = self.lock();
        if i.data.messages.is_empty() {
            return vec![];
        }
        i.dirty = true;
        std::mem::take(&mut i.data.messages)
    }

    pub fn login(&self, user: &User) {
        let mut i = self.lock();
        i.data.user_id = Some(user.id);
        i.data.auth_hash = auth::session_hash(&user.password);
        i.data.csrf = auth::random_string(48);
        i.rotate = true;
        i.dirty = true;
    }

    pub fn logout(&self) {
        let mut i = self.lock();
        i.data = SessionData::fresh();
        i.rotate = true;
        i.dirty = true;
    }

    /// Parol o'zgarganda joriy sessiyani saqlab qolish (`update_session_auth_hash`).
    pub fn update_auth_hash(&self, new_password_hash: &str) {
        let mut i = self.lock();
        i.data.auth_hash = auth::session_hash(new_password_hash);
        i.dirty = true;
    }

    /// Mehmon rejimida kimning muzeyiga kirilganini saqlash.
    pub fn set_guest_share(&self, sharer_id: i64) {
        let mut i = self.lock();
        if i.stored.is_none() {
            i.dirty = true;
        }
        i.data.guest_sharer_id = Some(sharer_id);
        i.dirty = true;
    }

    pub fn guest_sharer_id(&self) -> Option<i64> {
        self.lock().data.guest_sharer_id
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Ctx {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Ctx>()
            .cloned()
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("session middleware ulanmagan")))
    }
}

// ---------------------------------------------------------------- forma ma'lumotlari

#[derive(Clone, Debug)]
pub struct Upload {
    pub filename: String,
    pub bytes: Vec<u8>,
}

/// POST forma (urlencoded yoki multipart). Olinayotganda CSRF token tekshiriladi.
#[derive(Default, Debug)]
pub struct FormData {
    pub fields: HashMap<String, String>,
    pub files: HashMap<String, Upload>,
}

impl FormData {
    pub fn get(&self, name: &str) -> &str {
        self.fields.get(name).map(String::as_str).unwrap_or("")
    }

    pub fn has(&self, name: &str) -> bool {
        self.fields.contains_key(name)
    }
}

impl<S: Send + Sync> FromRequest<S> for FormData {
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let ctx = req
            .extensions()
            .get::<Ctx>()
            .cloned()
            .ok_or_else(|| AppError::Internal(anyhow::anyhow!("session middleware ulanmagan")))?;
        let is_multipart = req
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.starts_with("multipart/form-data"))
            .unwrap_or(false);

        let mut data = FormData::default();
        if is_multipart {
            let mut mp = Multipart::from_request(req, state)
                .await
                .map_err(|e| AppError::BadRequest(e.body_text()))?;
            while let Some(field) = mp.next_field().await.map_err(|e| AppError::BadRequest(e.body_text()))? {
                let name = field.name().unwrap_or("").to_string();
                match field.file_name().map(str::to_string) {
                    Some(filename) => {
                        let bytes = field.bytes().await.map_err(|e| AppError::BadRequest(e.body_text()))?;
                        if !filename.is_empty() && !bytes.is_empty() {
                            data.files.insert(name, Upload { filename, bytes: bytes.to_vec() });
                        }
                    }
                    None => {
                        let text = field.text().await.map_err(|e| AppError::BadRequest(e.body_text()))?;
                        data.fields.insert(name, text);
                    }
                }
            }
        } else {
            let body = Bytes::from_request(req, state).await.map_err(|e| AppError::BadRequest(e.body_text()))?;
            let pairs: Vec<(String, String)> = serde_urlencoded::from_bytes(&body)
                .map_err(|_| AppError::BadRequest("Forma ma'lumotlari noto'g'ri.".into()))?;
            data.fields.extend(pairs);
        }
        ctx.check_csrf(data.fields.get("csrfmiddlewaretoken").map(String::as_str))?;
        Ok(data)
    }
}

// ---------------------------------------------------------------- saqlash

fn now_ts() -> i64 {
    chrono::Utc::now().timestamp()
}

fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

fn load(c: &Connection, key: &str) -> Result<Option<(SessionData, Option<User>)>, AppError> {
    let raw: Option<String> = c
        .query_row("SELECT data FROM sessions WHERE key = ?1 AND expires_at > ?2", params![key, now_ts()], |r| {
            r.get(0)
        })
        .optional()?;
    let Some(data) = raw.and_then(|r| serde_json::from_str::<SessionData>(&r).ok()) else {
        return Ok(None);
    };
    let user = match data.user_id {
        Some(id) => models::user_by_id(c, id)?,
        None => None,
    };
    Ok(Some((data, user)))
}

fn save(c: &Connection, key: &str, data: &SessionData) -> Result<(), AppError> {
    let json = serde_json::to_string(data).map_err(|e| AppError::Internal(e.into()))?;
    c.execute(
        "INSERT INTO sessions (key, data, expires_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (key) DO UPDATE SET data = excluded.data, expires_at = excluded.expires_at",
        params![key, json, now_ts() + SESSION_AGE_SECS],
    )?;
    Ok(())
}

/// Muddati o'tgan sessiyalarni tozalash.
pub fn cleanup(c: &Connection) -> rusqlite::Result<usize> {
    c.execute("DELETE FROM sessions WHERE expires_at <= ?1", [now_ts()])
}

/// Har bir so'rov uchun: sessiyani yuklaydi, `Ctx` ni qo'shadi, javobdan keyin saqlaydi.
pub async fn session_middleware(State(st): State<AppState>, mut req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let full_path = req.uri().path_and_query().map(|pq| pq.as_str().to_string()).unwrap_or_else(|| path.clone());

    let cookie_key = read_cookie(req.headers(), COOKIE_NAME)
        .filter(|k| k.len() == KEY_LEN && k.chars().all(|c| c.is_ascii_alphanumeric()));
    let loaded = match cookie_key.clone() {
        Some(key) => st.db.call(move |c| load(c, &key)).await.unwrap_or_else(|e| {
            tracing::error!("sessiyani o'qib bo'lmadi: {e:?}");
            None
        }),
        None => None,
    };

    let (stored, mut data, user) = match loaded {
        Some((d, u)) => (cookie_key, d, u),
        None => (None, SessionData::fresh(), None),
    };
    let mut dirty = false;
    let user = match (data.user_id, user) {
        (Some(_), Some(u)) if u.is_active && data.auth_hash == auth::session_hash(&u.password) => Some(u),
        (Some(_), _) => {
            // foydalanuvchi o'chirilgan, bloklangan yoki parol almashtirilgan
            data.user_id = None;
            data.auth_hash.clear();
            dirty = true;
            None
        }
        _ => None,
    };

    let ctx = Ctx {
        user,
        path,
        full_path,
        inner: Arc::new(Mutex::new(Inner {
            key: stored.clone().unwrap_or_else(|| auth::random_string(KEY_LEN)),
            stored,
            data,
            dirty,
            rotate: false,
        })),
    };
    req.extensions_mut().insert(ctx.clone());

    let mut resp = next.run(req).await;

    // Xato sahifalarini umumiy dizayn bilan chizamiz
    if let Some(page) = resp.extensions().get::<ErrorPage>().cloned() {
        match render::render_error(&st, &ctx, &page).await {
            Ok(html) => {
                let status = resp.status();
                resp = html;
                *resp.status_mut() = status;
            }
            Err(e) => tracing::error!("xato sahifasini chizib bo'lmadi: {e:?}"),
        }
    }

    // Sessiyani saqlash
    let (to_save, to_delete, new_cookie) = {
        let mut i = ctx.lock();
        let mut to_delete = None;
        if i.rotate {
            to_delete = i.stored.take();
            i.key = auth::random_string(KEY_LEN);
            i.rotate = false;
            i.dirty = true;
        }
        let to_save = i.dirty.then(|| (i.key.clone(), i.data.clone()));
        let new_cookie = (to_save.is_some() && i.stored.as_deref() != Some(i.key.as_str())).then(|| i.key.clone());
        (to_save, to_delete, new_cookie)
    };
    if to_save.is_some() || to_delete.is_some() {
        let r = st
            .db
            .call(move |c| {
                if let Some(old) = to_delete {
                    c.execute("DELETE FROM sessions WHERE key = ?1", [old])?;
                }
                if let Some((key, data)) = to_save {
                    save(c, &key, &data)?;
                }
                Ok(())
            })
            .await;
        if let Err(e) = r {
            tracing::error!("sessiyani saqlab bo'lmadi: {e:?}");
        }
    }
    if let Some(key) = new_cookie {
        let secure = if st.cfg.secure_cookies { "; Secure" } else { "" };
        let cookie = format!("{COOKIE_NAME}={key}; Path=/; Max-Age={SESSION_AGE_SECS}; HttpOnly; SameSite=Lax{secure}");
        if let Ok(v) = HeaderValue::from_str(&cookie) {
            resp.headers_mut().append(header::SET_COOKIE, v);
        }
    }

    // Xavfsizlik sarlavhalari (Django SecurityMiddleware / XFrameOptionsMiddleware)
    let h = resp.headers_mut();
    h.entry(header::X_FRAME_OPTIONS).or_insert(HeaderValue::from_static("DENY"));
    h.entry(header::X_CONTENT_TYPE_OPTIONS).or_insert(HeaderValue::from_static("nosniff"));
    h.entry(header::REFERRER_POLICY).or_insert(HeaderValue::from_static("same-origin"));

    tracing::info!("{} {} -> {}", method, ctx.full_path, resp.status().as_u16());
    resp
}

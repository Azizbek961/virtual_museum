//! Sahifalar (Django `views.py` fayllari o'rniga).
pub mod accounts;
pub mod gallery;
pub mod halls;

use axum::{
    http::{Method, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::{
    error::{AppError, AppResult},
    forms::Form,
    render::render,
    session::Ctx,
    AppState,
};

pub(crate) fn parse_id(s: &str) -> AppResult<i64> {
    s.parse().map_err(|_| AppError::NotFound)
}

/// Faqat shu saytning ichki manzillariga yo'naltirishga ruxsat
/// (Django `url_has_allowed_host_and_scheme` o'rniga).
pub(crate) fn is_safe_next(url: &str) -> bool {
    url.starts_with('/')
        && !url.starts_with("//")
        && !url.starts_with("/\\")
        && !url.chars().any(|c| c.is_control() || c == '\\')
}

pub(crate) fn safe_next_or<'a>(next: &'a str, default: &'a str) -> &'a str {
    if is_safe_next(next) {
        next
    } else {
        default
    }
}

/// Umumiy forma sahifasi (`accounts/form.html`).
pub(crate) struct FormPage<'a> {
    pub title: String,
    pub submit: &'a str,
    pub back: &'a str,
    pub multipart: bool,
    pub preview: Option<String>,
    /// Asar formasi uchun: zal -> o'rin -> o'sha o'rinda nima turgani (JS yorliqlarni yangilaydi).
    pub slots: Option<serde_json::Value>,
}

pub(crate) async fn render_form(st: &AppState, ctx: &Ctx, form: &Form, page: FormPage<'_>) -> AppResult<Response> {
    render(
        st,
        ctx,
        "accounts/form.html",
        json!({
            "form": form,
            "title": page.title,
            "submit": page.submit,
            "back": page.back,
            "multipart": page.multipart,
            "preview": page.preview,
            "slots": page.slots,
        }),
    )
    .await
}

pub(crate) async fn render_confirm(st: &AppState, ctx: &Ctx, title: &str, text: &str, back: &str) -> AppResult<Response> {
    render(st, ctx, "accounts/confirm.html", json!({ "title": title, "text": text, "back": back })).await
}

/// Topilmagan manzillar. Django `APPEND_SLASH` kabi `/login` -> `/login/`.
pub async fn fallback(method: Method, uri: Uri) -> Response {
    let path = uri.path();
    let last = path.rsplit('/').next().unwrap_or("");
    if (method == Method::GET || method == Method::HEAD)
        && !path.ends_with('/')
        && !path.starts_with("//")
        && !path.starts_with("/static/")
        && !path.starts_with("/media/")
        && !last.contains('.')
    {
        let mut to = format!("{path}/");
        if let Some(q) = uri.query() {
            to.push('?');
            to.push_str(q);
        }
        return (StatusCode::MOVED_PERMANENTLY, [(axum::http::header::LOCATION, to)]).into_response();
    }
    AppError::NotFound.into_response()
}

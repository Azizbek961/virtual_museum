//! Xatolar va ularning HTTP javobga aylanishi.
use axum::{
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
};

#[derive(Debug)]
pub enum AppError {
    /// 404 — sahifa yo'q yoki ko'rishga ruxsat yo'q (mavjudligi oshkor qilinmaydi).
    NotFound,
    /// 403 — ruxsat yo'q.
    Forbidden(String),
    /// 400 — noto'g'ri so'rov.
    BadRequest(String),
    /// Tizimga kirish kerak: `/login/?next=<manzil>` ga yo'naltiriladi.
    LoginRequired(String),
    /// 500 — kutilmagan xato.
    Internal(anyhow::Error),
}

/// Xato sahifasi haqidagi ma'lumot. Javobga qo'shib qo'yiladi, keyin sessiya
/// middleware'i uni to'liq dizayndagi sahifa sifatida qayta chizadi.
#[derive(Clone, Debug)]
pub struct ErrorPage {
    pub title: String,
    pub message: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::LoginRequired(next) => {
                let q = serde_urlencoded::to_string([("next", next.as_str())]).unwrap_or_default();
                redirect(&format!("/login/?{q}"))
            }
            AppError::NotFound => error_page(
                StatusCode::NOT_FOUND,
                "Sahifa topilmadi",
                "So'ralgan sahifa mavjud emas yoki uni ko'rishga ruxsatingiz yo'q.",
            ),
            AppError::Forbidden(msg) => error_page(StatusCode::FORBIDDEN, "Ruxsat yo'q", &msg),
            AppError::BadRequest(msg) => error_page(StatusCode::BAD_REQUEST, "Noto'g'ri so'rov", &msg),
            AppError::Internal(e) => {
                tracing::error!("ichki xato: {e:#}");
                error_page(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Server xatosi",
                    "Kutilmagan xatolik yuz berdi. Birozdan so'ng qayta urinib ko'ring.",
                )
            }
        }
    }
}

fn error_page(status: StatusCode, title: &str, message: &str) -> Response {
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>{t}</title><h1>{t}</h1><p>{m}</p>",
        t = crate::render::escape_html(title),
        m = crate::render::escape_html(message)
    );
    let mut resp = (status, Html(body)).into_response();
    resp.extensions_mut().insert(ErrorPage { title: title.into(), message: message.into() });
    resp
}

/// 302 yo'naltirish (Django'dagi `redirect()` kabi).
pub fn redirect(to: &str) -> Response {
    (StatusCode::FOUND, [(header::LOCATION, to.to_string())]).into_response()
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Internal(e.into())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Internal(e.into())
    }
}

impl From<minijinja::Error> for AppError {
    fn from(e: minijinja::Error) -> Self {
        AppError::Internal(e.into())
    }
}

impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        AppError::Internal(e)
    }
}

impl From<tokio::task::JoinError> for AppError {
    fn from(e: tokio::task::JoinError) -> Self {
        AppError::Internal(e.into())
    }
}

pub type AppResult<T> = Result<T, AppError>;

//! Virtual Museum — Django loyihasining Rust (axum + MiniJinja + SQLite) versiyasi.
pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod forms;
pub mod images;
pub mod import;
pub mod models;
pub mod render;
pub mod mosaic;
pub mod sculpt;
pub mod segment;
pub mod seed;
pub mod session;
pub mod views;

use std::sync::Arc;

use axum::{
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post},
    Router,
};
use tower_http::services::ServeDir;

pub use config::Config;

#[derive(Clone)]
pub struct AppState {
    pub db: db::Db,
    pub cfg: Arc<Config>,
}

impl AppState {
    pub fn new(cfg: Config) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&cfg.media_dir)?;
        let db = db::Db::open(&cfg.db_path)?;
        // haykal zallaridagi hali haykalga aylantirilmagan asarlar (masalan, eski bazadan)
        let n = sculpt::ensure_sync(&db, &cfg.media_dir);
        if n > 0 {
            tracing::info!("{n} ta asar haykalga aylantirildi");
        }
        // mozaika zallaridagi hali mozaikaga aylantirilmagan asarlar
        let m = mosaic::ensure_sync(&db, &cfg.media_dir);
        if m > 0 {
            tracing::info!("{m} ta asar mozaikaga aylantirildi");
        }
        Ok(AppState { db, cfg: Arc::new(cfg) })
    }
}

/// Barcha manzillar (Django `urls.py` fayllari o'rniga).
pub fn build_app(state: AppState) -> Router {
    use views::{accounts as a, gallery as g, halls as h};

    let dynamic = Router::new()
        // gallery
        .route("/", get(g::home))
        .route("/tour/", get(g::tour))
        .route("/hall/{slug}/", get(g::section_detail))
        .route("/hall/{slug}/3d/", get(g::section_3d))
        .route("/art/{id}/", get(g::painting_detail))
        // user: o'z rasmlari
        .route("/my/art/new/", get(g::painting_create_page).post(g::painting_create))
        .route("/my/art/{id}/edit/", get(g::painting_edit_page).post(g::painting_edit))
        .route("/my/art/{id}/delete/", get(g::painting_delete_page).post(g::painting_delete))
        // admin: moderatsiya
        .route("/panel/art/", get(g::moderation))
        .route("/panel/art/{id}/toggle/", post(g::moderation_toggle))
        .route("/panel/art/{id}/delete/", post(g::moderation_delete))
        // yuklangan rasmlar ochiq emas: har bir so'rovda ruxsat tekshiriladi
        .route("/media/{*path}", get(g::protected_media))
        // accounts
        .route("/login/", get(a::login_page).post(a::login_submit))
        .route("/logout/", post(a::logout))
        .route("/me/", get(a::home_redirect))
        .route("/profile/", get(a::profile))
        .route("/profile/edit/", get(a::profile_edit_page).post(a::profile_edit))
        .route("/profile/password/", get(a::password_change_page_get).post(a::password_change))
        // admin: foydalanuvchilar
        .route("/panel/", get(a::panel))
        .route("/panel/users/new/", get(a::user_create_page_get).post(a::user_create))
        .route("/panel/users/{id}/edit/", get(a::user_edit_page_get).post(a::user_edit))
        .route("/panel/users/{id}/password/", get(a::user_password_page).post(a::user_password))
        .route("/panel/users/{id}/toggle/", post(a::user_toggle))
        .route("/panel/users/{id}/delete/", get(a::user_delete_page).post(a::user_delete))
        // admin: zallar (Django'da /django-admin/ orqali edi)
        .route("/panel/halls/", get(h::list))
        .route("/panel/halls/new/", get(h::create_page).post(h::create))
        .route("/panel/halls/{id}/edit/", get(h::edit_page).post(h::edit))
        .route("/panel/halls/{id}/delete/", get(h::delete_page).post(h::delete))
        .fallback(views::fallback)
        .layer(middleware::from_fn_with_state(state.clone(), session::session_middleware))
        .layer(DefaultBodyLimit::max(config::MAX_BODY_BYTES))
        .with_state(state.clone());

    Router::new()
        .nest_service("/static", ServeDir::new(state.cfg.base_dir.join("static")))
        .merge(dynamic)
}

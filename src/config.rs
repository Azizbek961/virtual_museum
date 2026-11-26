//! Sozlamalar (Django'dagi `config/settings.py` o'rniga).
//!
//! Hammasi muhit o'zgaruvchilari orqali o'zgartiriladi:
//!   MUSEUM_ADDR               вЂ” tinglash manzili (standart: 127.0.0.1:8080)
//!   MUSEUM_BASE_DIR           вЂ” loyiha papkasi (static/ shu yerda)
//!   MUSEUM_DB                 вЂ” SQLite fayli (standart: <base>/museum.sqlite3)
//!   MUSEUM_MEDIA              вЂ” yuklangan rasmlar papkasi (standart: <base>/media)
//!   MUSEUM_SECURE_COOKIES=1   вЂ” HTTPS orqali ishlaganda (production)
//!   MUSEUM_PBKDF2_ITERATIONS  вЂ” parol xeshlash iteratsiyalari (standart: 1 000 000)
//!   MUSEUM_LEGACY_DIR         вЂ” eski desktop loyiha (seed uchun)
//!   MUSEUM_DJANGO_DIR         вЂ” Django loyihasi (import-django uchun)
use std::path::PathBuf;

/// Yuklanadigan rasm hajmi chegarasi (MB).
pub const MAX_UPLOAD_MB: u64 = 10;
/// So'rov tanasining umumiy chegarasi (bayt).
pub const MAX_BODY_BYTES: usize = 12 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Config {
    pub base_dir: PathBuf,
    pub db_path: PathBuf,
    pub media_dir: PathBuf,
    pub addr: String,
    pub secure_cookies: bool,
    pub pbkdf2_iterations: u32,
    pub legacy_dir: PathBuf,
    pub django_dir: PathBuf,
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

impl Config {
    pub fn from_env() -> Self {
        let base_dir = env_path("MUSEUM_BASE_DIR").unwrap_or_else(|| {
            // `cargo run` loyiha papkasidan ishga tushirilsa вЂ” joriy papka,
            // aks holda manba kodi turgan papka.
            let cwd = std::env::current_dir().unwrap_or_default();
            if cwd.join("static").is_dir() {
                cwd
            } else {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            }
        });
        let parent = base_dir.parent().map(PathBuf::from).unwrap_or_else(|| base_dir.clone());
        Config {
            db_path: env_path("MUSEUM_DB").unwrap_or_else(|| base_dir.join("museum.sqlite3")),
            media_dir: env_path("MUSEUM_MEDIA").unwrap_or_else(|| base_dir.join("media")),
            addr: std::env::var("MUSEUM_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".into()),
            secure_cookies: std::env::var("MUSEUM_SECURE_COOKIES").map(|v| v == "1").unwrap_or(false),
            pbkdf2_iterations: std::env::var("MUSEUM_PBKDF2_ITERATIONS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1_000_000),
            legacy_dir: env_path("MUSEUM_LEGACY_DIR").unwrap_or_else(|| parent.join("virtual_museum")),
            django_dir: env_path("MUSEUM_DJANGO_DIR").unwrap_or_else(|| parent.join("museum_web")),
            base_dir,
        }
    }
}


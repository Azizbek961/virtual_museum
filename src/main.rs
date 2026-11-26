//! `museum` buyruqlari (Django `manage.py` o'rniga):
//!
//! ```text
//! cargo run                          # serverni ishga tushirish (= serve)
//! cargo run -- seed                  # 5 zal + 30 asarni import qilish
//! cargo run -- createsuperuser --username admin
//! cargo run -- import-django         # Django bazasini ko'chirish
//! ```
use std::path::PathBuf;

use anyhow::{bail, Context};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use museum::{auth, build_app, models, seed, session, AppState, Config};

#[derive(Parser)]
#[command(name = "museum", version, about = "Virtual Museum — Rust veb-serveri")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Veb-serverni ishga tushirish (standart buyruq)
    Serve {
        /// Tinglash manzili, masalan 0.0.0.0:8000
        #[arg(long)]
        addr: Option<String>,
    },
    /// virtual_museum/data/museum_data.json dan zallar va asarlarni import qilish
    Seed {
        /// virtual_museum papkasi yo'li
        #[arg(long)]
        source: Option<PathBuf>,
        /// Mavjud yozuvlarning matnini yangilash
        #[arg(long)]
        update: bool,
    },
    /// Administrator (superuser) yaratish
    Createsuperuser {
        #[arg(long)]
        username: String,
        #[arg(long, default_value = "")]
        email: String,
        /// Berilmasa, terminalda so'raladi
        #[arg(long)]
        password: Option<String>,
    },
    /// Django loyihasidagi foydalanuvchilar, zallar, asarlar va rasmlarni ko'chirish
    ImportDjango {
        /// museum_web papkasi (db.sqlite3 va media/ shu yerda)
        #[arg(long)]
        from: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let cli = Cli::parse();
    let cfg = Config::from_env();
    let state = AppState::new(cfg.clone()).with_context(|| format!("bazani ochib bo'lmadi: {}", cfg.db_path.display()))?;

    match cli.cmd.unwrap_or(Cmd::Serve { addr: None }) {
        Cmd::Serve { addr } => serve(state, addr.unwrap_or(cfg.addr)).await,
        Cmd::Seed { source, update } => seed::run(&state, &source.unwrap_or(cfg.legacy_dir), update),
        Cmd::Createsuperuser { username, email, password } => create_superuser(&state, &username, &email, password),
        Cmd::ImportDjango { from } => museum::import::run(&state, &from.unwrap_or(cfg.django_dir)),
    }
}

async fn serve(state: AppState, addr: String) -> anyhow::Result<()> {
    let removed = state.db.with(|c| session::cleanup(c))?;
    if removed > 0 {
        tracing::info!("{removed} ta eskirgan sessiya o'chirildi");
    }
    // Har soatda eskirgan sessiyalarni tozalash
    let db = state.db.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
        tick.tick().await;
        loop {
            tick.tick().await;
            let _ = db.call(|c| Ok(session::cleanup(c)?)).await;
        }
    });

    let app = build_app(state.clone());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("{addr} manzilini band qilib bo'lmadi (port band emasligini tekshiring)"))?;
    println!();
    println!("  ◆ Virtual Museum (Rust) ishga tushdi: http://{addr}/");
    println!("    Baza:  {}", state.cfg.db_path.display());
    println!("    Media: {}", state.cfg.media_dir.display());
    println!("    To'xtatish: Ctrl+C");
    println!();
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

fn create_superuser(state: &AppState, username: &str, email: &str, password: Option<String>) -> anyhow::Result<()> {
    if !auth::valid_username(username) || username.chars().count() > 150 {
        bail!("Login faqat harflar, raqamlar va @/./+/-/_ belgilaridan iborat bo'lishi kerak.");
    }
    if !email.is_empty() && !auth::valid_email(email) {
        bail!("Email noto'g'ri.");
    }
    if state.db.with(|c| models::username_taken(c, username, None))? {
        bail!("“{username}” logini band.");
    }
    let password = match password {
        Some(p) => p,
        None => {
            let p1 = rpassword::prompt_password("Parol: ")?;
            let p2 = rpassword::prompt_password("Parolni takrorlang: ")?;
            if p1 != p2 {
                bail!("Parollar mos kelmadi.");
            }
            p1
        }
    };
    let errors = auth::validate_password(&password, &[("login", username), ("email", email)]);
    if !errors.is_empty() {
        bail!("Parol talablarga javob bermaydi:\n  - {}", errors.join("\n  - "));
    }
    let hash = auth::make_password(&password, state.cfg.pbkdf2_iterations);
    state.db.with(|c| {
        models::insert_user(
            c,
            &models::NewUser {
                username,
                password_hash: &hash,
                first_name: "",
                last_name: "",
                email,
                role: models::ROLE_ADMIN,
                is_superuser: true,
                created_by_id: None,
            },
        )
    })?;
    println!("Superuser “{username}” yaratildi.");
    Ok(())
}

//! Kirish/chiqish, profil va admin paneli (Django `accounts/views.py`).
use axum::{
    extract::{Path, Query, State},
    response::Response,
};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{parse_id, render_confirm, render_form, safe_next_or, FormPage};
use crate::{
    auth,
    config::MAX_UPLOAD_MB,
    error::{redirect, AppError, AppResult},
    forms::{check_password_pair, Field, Form, Opt},
    images,
    models::{self, now_iso, User, UserView, ROLE_ADMIN, ROLE_USER},
    render::render,
    session::{Ctx, FormData},
    AppState,
};

fn role_options() -> Vec<Opt> {
    vec![Opt::new(ROLE_ADMIN, "Admin"), Opt::new(ROLE_USER, "Foydalanuvchi")]
}

// ---------------------------------------------------------------- kirish / chiqish

#[derive(Deserialize)]
pub struct NextQuery {
    #[serde(default)]
    next: String,
}

fn login_form() -> Form {
    Form::new(vec![
        Field::text("username", "Login").required().max(150).autofocus().autocomplete("username"),
        Field::password("password", "Parol").required().autocomplete("current-password"),
    ])
}

pub async fn login_page(State(st): State<AppState>, ctx: Ctx, Query(q): Query<NextQuery>) -> AppResult<Response> {
    if ctx.user.is_some() {
        return Ok(redirect(safe_next_or(&q.next, "/me/")));
    }
    render(&st, &ctx, "accounts/login.html", json!({ "form": login_form(), "next": q.next })).await
}

pub async fn login_submit(State(st): State<AppState>, ctx: Ctx, data: FormData) -> AppResult<Response> {
    let next = data.get("next").to_string();
    if ctx.user.is_some() {
        return Ok(redirect(safe_next_or(&next, "/me/")));
    }
    let mut form = login_form();
    form.bind(&data);
    if !form.has_errors() {
        let username = form.val("username").to_string();
        let password = form.val("password").to_string();
        let found = st.db.call(move |c| Ok(models::user_by_username(c, &username)?)).await?;
        let ok = match &found {
            Some(u) => auth::check_password_async(password, u.password.clone()).await,
            None => {
                // vaqt bo'yicha farq bilan login mavjudligini oshkor qilmaslik uchun
                let _ = auth::hash_password_async(password, st.cfg.pbkdf2_iterations).await;
                false
            }
        };
        match found {
            Some(u) if ok && u.is_active => {
                ctx.login(&u);
                let id = u.id;
                st.db
                    .call(move |c| {
                        c.execute("UPDATE users SET last_login = ?1 WHERE id = ?2", params![now_iso(), id])?;
                        Ok(())
                    })
                    .await?;
                return Ok(redirect(safe_next_or(&next, "/me/")));
            }
            Some(_) if ok => form.add_non_field_error("Bu profil bloklangan. Administratorga murojaat qiling."),
            _ => form.add_non_field_error("Login yoki parol noto'g'ri."),
        }
    }
    render(&st, &ctx, "accounts/login.html", json!({ "form": form, "next": next })).await
}

pub async fn logout(ctx: Ctx, _data: FormData) -> Response {
    ctx.logout();
    redirect("/")
}

/// Login'dan keyin: admin -> boshqaruv paneli, user -> o'z profili.
pub async fn home_redirect(ctx: Ctx) -> AppResult<Response> {
    let user = ctx.require_login()?;
    Ok(redirect(if user.is_museum_admin() { "/panel/" } else { "/profile/" }))
}

// ---------------------------------------------------------------- user (2-ruxsat)

pub async fn profile(State(st): State<AppState>, ctx: Ctx) -> AppResult<Response> {
    let user = ctx.require_login()?;
    let uid = user.id;
    let paintings = st
        .db
        .call(move |c| Ok(models::paintings_where(c, "p.owner_id = ?1", models::DEFAULT_ORDER, [uid])?))
        .await?;
    render(&st, &ctx, "accounts/profile.html", json!({ "paintings": models::views(&paintings) })).await
}

fn profile_form(u: &User) -> Form {
    let mut avatar = Field::file("avatar", "Profil rasmi", "image/*");
    if !u.avatar.is_empty() {
        avatar = avatar.current(models::media_url(&u.avatar), u.avatar.clone()).clearable();
    }
    Form::new(vec![
        Field::text("first_name", "Ism").max(150).value(&u.first_name),
        Field::text("last_name", "Familiya").max(150).value(&u.last_name),
        Field::email("email", "Email").max(254).value(&u.email),
        Field::textarea("bio", "O'zim haqimda", 3).max(600).value(&u.bio),
        avatar,
    ])
}

fn profile_page() -> FormPage<'static> {
    FormPage { title: "Profilni tahrirlash".into(), submit: "Saqlash", back: "/profile/", multipart: true, preview: None, slots: None }
}

pub async fn profile_edit_page(State(st): State<AppState>, ctx: Ctx) -> AppResult<Response> {
    let user = ctx.require_login()?;
    render_form(&st, &ctx, &profile_form(&user), profile_page()).await
}

pub async fn profile_edit(State(st): State<AppState>, ctx: Ctx, mut data: FormData) -> AppResult<Response> {
    let user = ctx.require_login()?;
    let mut form = profile_form(&user);
    form.bind(&data);
    let upload = data.files.remove("avatar");
    if let Some(up) = &upload {
        if let Err(e) = images::validate_image(&up.filename, &up.bytes, MAX_UPLOAD_MB) {
            form.add_error("avatar", e);
        }
    }
    if form.has_errors() {
        return render_form(&st, &ctx, &form, profile_page()).await;
    }

    // None — o'zgarmaydi, Some("") — o'chiriladi, Some(yo'l) — yangi rasm
    let new_avatar = match upload {
        Some(up) => {
            let dir = format!("avatars/{}", user.username);
            Some(images::store_upload(st.cfg.media_dir.clone(), dir, up, 512).await?.0)
        }
        None if data.has("avatar-clear") => Some(String::new()),
        None => None,
    };
    let (first, last, email, bio) =
        (form.val("first_name").to_string(), form.val("last_name").to_string(), form.val("email").to_string(), form.val("bio").to_string());
    let uid = user.id;
    let avatar = new_avatar.clone();
    st.db
        .call(move |c| {
            c.execute(
                "UPDATE users SET first_name = ?1, last_name = ?2, email = ?3, bio = ?4,
                        avatar = COALESCE(?5, avatar) WHERE id = ?6",
                params![first, last, email, bio, avatar, uid],
            )?;
            Ok(())
        })
        .await?;
    if new_avatar.is_some() && !user.avatar.is_empty() {
        images::remove_media_async(st.cfg.media_dir.clone(), vec![user.avatar.clone()]).await;
    }
    ctx.success("Profil yangilandi.");
    Ok(redirect("/profile/"))
}

fn password_change_form() -> Form {
    Form::new(vec![
        Field::password("old_password", "Joriy parol").required().autofocus().autocomplete("current-password"),
        Field::password("new_password1", "Yangi parol").required().help(auth::PASSWORD_HELP).autocomplete("new-password"),
        Field::password("new_password2", "Yangi parolni takrorlang").required().autocomplete("new-password"),
    ])
}

fn password_change_page() -> FormPage<'static> {
    FormPage { title: "Parolni o'zgartirish".into(), submit: "O'zgartirish", back: "/me/", multipart: false, preview: None, slots: None }
}

pub async fn password_change_page_get(State(st): State<AppState>, ctx: Ctx) -> AppResult<Response> {
    ctx.require_login()?;
    render_form(&st, &ctx, &password_change_form(), password_change_page()).await
}

pub async fn password_change(State(st): State<AppState>, ctx: Ctx, data: FormData) -> AppResult<Response> {
    let user = ctx.require_login()?;
    let mut form = password_change_form();
    form.bind(&data);
    if !form.field_has_error("old_password")
        && !auth::check_password_async(form.val("old_password").to_string(), user.password.clone()).await
    {
        form.add_error("old_password", "Joriy parol noto'g'ri kiritildi. Iltimos, qaytadan kiriting.");
    }
    let attrs = user.attrs();
    let attrs: Vec<(&str, &str)> = attrs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    check_password_pair(&mut form, "new_password1", "new_password2", "new_password2", &attrs);
    if form.has_errors() {
        return render_form(&st, &ctx, &form, password_change_page()).await;
    }
    let hash = auth::hash_password_async(form.val("new_password1").to_string(), st.cfg.pbkdf2_iterations).await?;
    let (uid, h) = (user.id, hash.clone());
    st.db
        .call(move |c| {
            c.execute("UPDATE users SET password = ?1 WHERE id = ?2", params![h, uid])?;
            Ok(())
        })
        .await?;
    ctx.update_auth_hash(&hash);
    ctx.success("Parol o'zgartirildi.");
    Ok(redirect("/me/"))
}

// ---------------------------------------------------------------- admin (1-ruxsat)

/// Tahrirlanayotgan profil. Oddiy admin superuser profiliga tega olmaydi.
async fn target(st: &AppState, editor: &User, id: &str) -> AppResult<User> {
    let id = parse_id(id)?;
    let target = st.db.call(move |c| Ok(models::user_by_id(c, id)?)).await?.ok_or(AppError::NotFound)?;
    if target.is_superuser && !editor.is_superuser {
        return Err(AppError::Forbidden("Superuser profilini faqat superuser o'zgartira oladi.".into()));
    }
    Ok(target)
}

#[derive(Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    q: String,
}

#[derive(Serialize)]
struct UserRow {
    #[serde(flatten)]
    user: UserView,
    n_paintings: i64,
    created_by: Option<String>,
}

pub async fn panel(State(st): State<AppState>, ctx: Ctx, Query(query): Query<SearchQuery>) -> AppResult<Response> {
    ctx.require_admin()?;
    let q = query.q.trim().to_string();
    let q2 = q.clone();
    let (users, stats) = st
        .db
        .call(move |c| {
            let like = format!("%{}%", q2.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
            let sql = format!(
                "SELECT {}, (SELECT COUNT(*) FROM paintings p WHERE p.owner_id = u.id), cb.username
                 FROM users u LEFT JOIN users cb ON cb.id = u.created_by_id
                 WHERE ?1 = '' OR u.username LIKE ?2 ESCAPE '\\' OR u.first_name LIKE ?2 ESCAPE '\\'
                    OR u.last_name LIKE ?2 ESCAPE '\\' OR u.email LIKE ?2 ESCAPE '\\'
                 ORDER BY u.username",
                models::user_cols("u.")
            );
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt.query_map(params![q2, like], |r| {
                Ok(UserRow {
                    user: models::user_from_row(r, 0)?.view(),
                    n_paintings: r.get(models::USER_COLS_COUNT)?,
                    created_by: r.get(models::USER_COLS_COUNT + 1)?,
                })
            })?;
            let users: Vec<UserRow> = rows.collect::<rusqlite::Result<_>>()?;
            let count = |sql: &str| c.query_row(sql, [], |r| r.get::<_, i64>(0));
            let stats = json!({
                "users": count("SELECT COUNT(*) FROM users WHERE role = 'user'")?,
                "admins": count("SELECT COUNT(*) FROM users WHERE role = 'admin' OR is_superuser = 1")?,
                "paintings": count("SELECT COUNT(*) FROM paintings")?,
                "uploads": count("SELECT COUNT(*) FROM paintings WHERE owner_id IS NOT NULL")?,
            });
            Ok((users, stats))
        })
        .await?;
    render(&st, &ctx, "panel/dashboard.html", json!({ "users": users, "stats": stats, "q": q })).await
}

fn user_create_form() -> Form {
    Form::new(vec![
        Field::text("username", "Login").required().max(150).help("Faqat harflar, raqamlar va @/./+/-/_ belgilar."),
        Field::text("first_name", "Ism").max(150),
        Field::text("last_name", "Familiya").max(150),
        Field::email("email", "Email").max(254),
        Field::select("role", "Rol", role_options()).required().value(ROLE_USER),
        Field::password("password1", "Parol").required().help(auth::PASSWORD_HELP).autocomplete("new-password"),
        Field::password("password2", "Parolni takrorlang").required().autocomplete("new-password"),
    ])
}

fn user_create_page() -> FormPage<'static> {
    FormPage { title: "Yangi login yaratish".into(), submit: "Yaratish", back: "/panel/", multipart: false, preview: None, slots: None }
}

/// Login formatini va bandligini tekshiradi.
async fn check_username(st: &AppState, form: &mut Form, except: Option<i64>) -> AppResult<()> {
    if form.field_has_error("username") {
        return Ok(());
    }
    let username = form.val("username").to_string();
    if !auth::valid_username(&username) {
        form.add_error(
            "username",
            "To'g'ri login kiriting. Login faqat harflar, raqamlar va @/./+/-/_ belgilaridan iborat bo'lishi mumkin.",
        );
        return Ok(());
    }
    if st.db.call(move |c| Ok(models::username_taken(c, &username, except)?)).await? {
        form.add_error("username", "Bunday loginli foydalanuvchi allaqachon mavjud.");
    }
    Ok(())
}

pub async fn user_create_page_get(State(st): State<AppState>, ctx: Ctx) -> AppResult<Response> {
    ctx.require_admin()?;
    render_form(&st, &ctx, &user_create_form(), user_create_page()).await
}

pub async fn user_create(State(st): State<AppState>, ctx: Ctx, data: FormData) -> AppResult<Response> {
    let admin = ctx.require_admin()?;
    let mut form = user_create_form();
    form.bind(&data);
    check_username(&st, &mut form, None).await?;
    let attrs = [
        ("login", form.val("username").to_string()),
        ("ism", form.val("first_name").to_string()),
        ("familiya", form.val("last_name").to_string()),
        ("email", form.val("email").to_string()),
    ];
    let attrs: Vec<(&str, &str)> = attrs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    check_password_pair(&mut form, "password1", "password2", "password1", &attrs);
    if form.has_errors() {
        return render_form(&st, &ctx, &form, user_create_page()).await;
    }
    let hash = auth::hash_password_async(form.val("password1").to_string(), st.cfg.pbkdf2_iterations).await?;
    let username = form.val("username").to_string();
    let f = form.clone();
    let admin_id = admin.id;
    st.db
        .call(move |c| {
            models::insert_user(
                c,
                &models::NewUser {
                    username: f.val("username"),
                    password_hash: &hash,
                    first_name: f.val("first_name"),
                    last_name: f.val("last_name"),
                    email: f.val("email"),
                    role: f.val("role"),
                    is_superuser: false,
                    created_by_id: Some(admin_id),
                },
            )?;
            Ok(())
        })
        .await?;
    ctx.success(format!("“{username}” logini yaratildi."));
    Ok(redirect("/panel/"))
}

fn user_edit_form(t: &User, editor: &User) -> Form {
    // Admin o'zini bloklab yoki oddiy userga tushirib qo'ya olmaydi
    let is_self = t.id == editor.id;
    Form::new(vec![
        Field::text("username", "Login").required().max(150).value(&t.username),
        Field::text("first_name", "Ism").max(150).value(&t.first_name),
        Field::text("last_name", "Familiya").max(150).value(&t.last_name),
        Field::email("email", "Email").max(254).value(&t.email),
        Field::select("role", "Rol", role_options()).required().value(&t.role).disabled(is_self),
        Field::checkbox("is_active", "Faol (tizimga kira oladi)").checked(t.is_active).disabled(is_self),
    ])
}

fn user_edit_page(t: &User) -> FormPage<'static> {
    FormPage { title: format!("Profil: {}", t.username), submit: "Saqlash", back: "/panel/", multipart: false, preview: None, slots: None }
}

pub async fn user_edit_page_get(State(st): State<AppState>, ctx: Ctx, Path(id): Path<String>) -> AppResult<Response> {
    let admin = ctx.require_admin()?;
    let t = target(&st, &admin, &id).await?;
    render_form(&st, &ctx, &user_edit_form(&t, &admin), user_edit_page(&t)).await
}

pub async fn user_edit(State(st): State<AppState>, ctx: Ctx, Path(id): Path<String>, data: FormData) -> AppResult<Response> {
    let admin = ctx.require_admin()?;
    let t = target(&st, &admin, &id).await?;
    let mut form = user_edit_form(&t, &admin);
    form.bind(&data);
    check_username(&st, &mut form, Some(t.id)).await?;
    if form.has_errors() {
        return render_form(&st, &ctx, &form, user_edit_page(&t)).await;
    }
    let username = form.val("username").to_string();
    let f = form.clone();
    let tid = t.id;
    st.db
        .call(move |c| {
            c.execute(
                "UPDATE users SET username = ?1, first_name = ?2, last_name = ?3, email = ?4,
                        role = ?5, is_active = ?6 WHERE id = ?7",
                params![
                    f.val("username"),
                    f.val("first_name"),
                    f.val("last_name"),
                    f.val("email"),
                    f.val("role"),
                    f.is_checked("is_active"),
                    tid
                ],
            )?;
            Ok(())
        })
        .await?;
    ctx.success(format!("“{username}” yangilandi."));
    Ok(redirect("/panel/"))
}

fn set_password_form() -> Form {
    Form::new(vec![
        Field::password("password1", "Yangi parol").required().help(auth::PASSWORD_HELP).autocomplete("new-password"),
        Field::password("password2", "Parolni takrorlang").required().autocomplete("new-password"),
    ])
}

fn set_password_page(t: &User) -> FormPage<'static> {
    FormPage {
        title: format!("Parolni tiklash: {}", t.username),
        submit: "O'rnatish",
        back: "/panel/",
        multipart: false,
        preview: None,
        slots: None,
    }
}

pub async fn user_password_page(State(st): State<AppState>, ctx: Ctx, Path(id): Path<String>) -> AppResult<Response> {
    let admin = ctx.require_admin()?;
    let t = target(&st, &admin, &id).await?;
    render_form(&st, &ctx, &set_password_form(), set_password_page(&t)).await
}

pub async fn user_password(State(st): State<AppState>, ctx: Ctx, Path(id): Path<String>, data: FormData) -> AppResult<Response> {
    let admin = ctx.require_admin()?;
    let t = target(&st, &admin, &id).await?;
    let mut form = set_password_form();
    form.bind(&data);
    let attrs = t.attrs();
    let attrs: Vec<(&str, &str)> = attrs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    check_password_pair(&mut form, "password1", "password2", "password1", &attrs);
    if form.has_errors() {
        return render_form(&st, &ctx, &form, set_password_page(&t)).await;
    }
    let hash = auth::hash_password_async(form.val("password1").to_string(), st.cfg.pbkdf2_iterations).await?;
    let (tid, h) = (t.id, hash.clone());
    st.db
        .call(move |c| {
            c.execute("UPDATE users SET password = ?1 WHERE id = ?2", params![h, tid])?;
            Ok(())
        })
        .await?;
    if t.id == admin.id {
        ctx.update_auth_hash(&hash);
    }
    ctx.success(format!("“{}” uchun yangi parol o'rnatildi.", t.username));
    Ok(redirect("/panel/"))
}

pub async fn user_toggle(State(st): State<AppState>, ctx: Ctx, Path(id): Path<String>, _data: FormData) -> AppResult<Response> {
    let admin = ctx.require_admin()?;
    let t = target(&st, &admin, &id).await?;
    if t.id == admin.id {
        ctx.error("O'zingizni bloklay olmaysiz.");
    } else {
        let (tid, active) = (t.id, !t.is_active);
        st.db
            .call(move |c| {
                c.execute("UPDATE users SET is_active = ?1 WHERE id = ?2", params![active, tid])?;
                Ok(())
            })
            .await?;
        ctx.success(format!("“{}” {}", t.username, if active { "faollashtirildi." } else { "bloklandi." }));
    }
    Ok(redirect("/panel/"))
}

pub async fn user_delete_page(State(st): State<AppState>, ctx: Ctx, Path(id): Path<String>) -> AppResult<Response> {
    let admin = ctx.require_admin()?;
    let t = target(&st, &admin, &id).await?;
    if t.id == admin.id {
        ctx.error("O'zingizni o'chira olmaysiz.");
        return Ok(redirect("/panel/"));
    }
    let tid = t.id;
    let n: i64 = st
        .db
        .call(move |c| Ok(c.query_row("SELECT COUNT(*) FROM paintings WHERE owner_id = ?1", [tid], |r| r.get(0))?))
        .await?;
    let text = format!("“{}” profili va unga tegishli {n} ta asar butunlay o'chiriladi.", t.username);
    render_confirm(&st, &ctx, "Profilni o'chirish", &text, "/panel/").await
}

pub async fn user_delete(State(st): State<AppState>, ctx: Ctx, Path(id): Path<String>, _data: FormData) -> AppResult<Response> {
    let admin = ctx.require_admin()?;
    let t = target(&st, &admin, &id).await?;
    if t.id == admin.id {
        ctx.error("O'zingizni o'chira olmaysiz.");
        return Ok(redirect("/panel/"));
    }
    let tid = t.id;
    let files = st.db.call(move |c| models::delete_user(c, tid)).await?;
    images::remove_media_async(st.cfg.media_dir.clone(), files).await;
    ctx.success(format!("“{}” va uning barcha asarlari o'chirildi.", t.username));
    Ok(redirect("/panel/"))
}

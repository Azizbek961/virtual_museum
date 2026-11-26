//! Zallarni boshqarish (admin). Django versiyasida bu `/django-admin/` orqali
//! qilinardi; Rust versiyasida alohida sahifa: `/panel/halls/`.
use axum::{
    extract::{Path, State},
    response::Response,
};
use rusqlite::params;
use serde::Serialize;
use serde_json::json;

use super::{parse_id, render_confirm, render_form, FormPage};
use crate::{
    error::{redirect, AppError, AppResult},
    forms::{Field, Form, Opt},
    images,
    models::{self, Section, FLOORS, KINDS, KIND_LOVE, KIND_MOSAICS, KIND_PAINTINGS, KIND_SCULPTURES},
    render::render,
    sculpt,
    session::{Ctx, FormData},
    AppState,
};

#[derive(Serialize)]
struct HallRow {
    #[serde(flatten)]
    section: Section,
    total: i64,
}

pub async fn list(State(st): State<AppState>, ctx: Ctx) -> AppResult<Response> {
    ctx.require_admin()?;
    let halls = st
        .db
        .call(|c| {
            let sections = models::all_sections(c)?;
            let mut out = Vec::with_capacity(sections.len());
            for s in sections {
                let total = c.query_row("SELECT COUNT(*) FROM paintings WHERE section_id = ?1", [s.id], |r| r.get(0))?;
                out.push(HallRow { section: s, total });
            }
            Ok(out)
        })
        .await?;
    render(&st, &ctx, "panel/halls.html", json!({ "halls": halls })).await
}

fn hall_form(s: Option<&Section>) -> Form {
    let floors = FLOORS.iter().map(|(v, l)| Opt::new(*v, *l)).collect();
    let v = |f: fn(&Section) -> String, default: &str| s.map(f).unwrap_or_else(|| default.to_string());
    Form::new(vec![
        Field::text("name", "Nomi").required().max(120).value(v(|s| s.name.clone(), "")),
        Field::text("slug", "Kod")
            .required()
            .max(50)
            .help("Manzil uchun: lotin harflari, raqamlar, '-' va '_'. Masalan: art → /hall/art/")
            .value(v(|s| s.slug.clone(), "")),
        Field::text("tagline", "Shior").max(200).value(v(|s| s.tagline.clone(), "")),
        Field::textarea("description", "Tavsif", 3).value(v(|s| s.description.clone(), "")),
        Field::color("accent", "Aksent rangi").required().value(v(|s| s.accent.clone(), "#d4af37")),
        Field::color("wall_color", "Devor rangi").required().value(v(|s| s.wall_color.clone(), "#262a36")),
        Field::select("kind", "Zal turi", KINDS.iter().map(|(v, l)| Opt::new(*v, *l)).collect())
            .help("Haykallar zaliga yuklangan har bir rasm avtomatik ravishda marmar haykalga aylantiriladi.")
            .value(v(|s| s.kind.clone(), KIND_PAINTINGS)),
        Field::select("floor", "Pol", floors).required().value(v(|s| s.floor.clone(), "floor_marble")),
        Field::number("order", "Tartib").value(v(|s| s.order.to_string(), "0")),
    ])
}

fn kind_of(f: &Form) -> &'static str {
    match f.val("kind") {
        KIND_SCULPTURES => KIND_SCULPTURES,
        KIND_MOSAICS => KIND_MOSAICS,
        KIND_LOVE => KIND_LOVE,
        models::KIND_MARRY_ME => models::KIND_MARRY_ME,
        _ => KIND_PAINTINGS,
    }
}

fn valid_slug(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn valid_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

async fn validate(st: &AppState, form: &mut Form, data: &FormData, except: Option<i64>) -> AppResult<()> {
    form.bind(data);
    let slug = form.val("slug").to_string();
    if !form.field_has_error("slug") {
        if !valid_slug(&slug) {
            form.add_error("slug", "Faqat lotin harflari, raqamlar, '-' va '_' ishlatilishi mumkin.");
        } else {
            let taken: bool = st
                .db
                .call(move |c| {
                    Ok(c.query_row(
                        "SELECT EXISTS(SELECT 1 FROM sections WHERE slug = ?1 AND id != ?2)",
                        params![slug, except.unwrap_or(-1)],
                        |r| r.get(0),
                    )?)
                })
                .await?;
            if taken {
                form.add_error("slug", "Bu kod bilan zal allaqachon mavjud.");
            }
        }
    }
    for name in ["accent", "wall_color"] {
        if !form.field_has_error(name) && !valid_color(form.val(name)) {
            form.add_error(name, "Rang #rrggbb ko'rinishida bo'lishi kerak.");
        }
    }
    if !form.field_has_error("order") && form.val("order").parse::<i64>().map(|n| n < 0).unwrap_or(false) {
        form.add_error("order", "Manfiy bo'lmagan son kiriting.");
    }
    Ok(())
}

fn page(title: String, submit: &'static str) -> FormPage<'static> {
    FormPage { title, submit, back: "/panel/halls/", multipart: false, preview: None, slots: None }
}

pub async fn create_page(State(st): State<AppState>, ctx: Ctx) -> AppResult<Response> {
    ctx.require_admin()?;
    render_form(&st, &ctx, &hall_form(None), page("Yangi zal".into(), "Yaratish")).await
}

pub async fn create(State(st): State<AppState>, ctx: Ctx, data: FormData) -> AppResult<Response> {
    ctx.require_admin()?;
    let mut form = hall_form(None);
    validate(&st, &mut form, &data, None).await?;
    if form.has_errors() {
        return render_form(&st, &ctx, &form, page("Yangi zal".into(), "Yaratish")).await;
    }
    let f = form.clone();
    st.db
        .call(move |c| {
            c.execute(
                r#"INSERT INTO sections (slug, name, tagline, description, accent, wall_color, floor, "order", kind)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"#,
                params![
                    f.val("slug"),
                    f.val("name"),
                    f.val("tagline"),
                    f.val("description"),
                    f.val("accent").to_lowercase(),
                    f.val("wall_color").to_lowercase(),
                    f.val("floor"),
                    f.val("order").parse::<i64>().unwrap_or(0),
                    kind_of(&f)
                ],
            )?;
            Ok(())
        })
        .await?;
    ctx.success(format!("“{}” zali yaratildi.", form.val("name")));
    Ok(redirect("/panel/halls/"))
}

async fn get_hall(st: &AppState, id: &str) -> AppResult<Section> {
    let id = parse_id(id)?;
    st.db.call(move |c| Ok(models::section_by_id(c, id)?)).await?.ok_or(AppError::NotFound)
}

pub async fn edit_page(State(st): State<AppState>, ctx: Ctx, Path(id): Path<String>) -> AppResult<Response> {
    ctx.require_admin()?;
    let s = get_hall(&st, &id).await?;
    render_form(&st, &ctx, &hall_form(Some(&s)), page(format!("Zal: {}", s.name), "Saqlash")).await
}

pub async fn edit(State(st): State<AppState>, ctx: Ctx, Path(id): Path<String>, data: FormData) -> AppResult<Response> {
    ctx.require_admin()?;
    let s = get_hall(&st, &id).await?;
    let mut form = hall_form(Some(&s));
    validate(&st, &mut form, &data, Some(s.id)).await?;
    if form.has_errors() {
        return render_form(&st, &ctx, &form, page(format!("Zal: {}", s.name), "Saqlash")).await;
    }
    let f = form.clone();
    let sid = s.id;
    st.db
        .call(move |c| {
            c.execute(
                r#"UPDATE sections SET slug = ?1, name = ?2, tagline = ?3, description = ?4, accent = ?5,
                          wall_color = ?6, floor = ?7, "order" = ?8, kind = ?10 WHERE id = ?9"#,
                params![
                    f.val("slug"),
                    f.val("name"),
                    f.val("tagline"),
                    f.val("description"),
                    f.val("accent").to_lowercase(),
                    f.val("wall_color").to_lowercase(),
                    f.val("floor"),
                    f.val("order").parse::<i64>().unwrap_or(0),
                    sid,
                    kind_of(&f)
                ],
            )?;
            Ok(())
        })
        .await?;
    // zal haykallar yoki mozaika zaliga aylantirilgan bo'lsa — undagi rasmlar qayta ishlanadi
    sculpt::ensure(&st.db, &st.cfg.media_dir).await;
    crate::mosaic::ensure(&st.db, &st.cfg.media_dir).await;
    ctx.success(format!("“{}” zali yangilandi.", form.val("name")));
    Ok(redirect("/panel/halls/"))
}

pub async fn delete_page(State(st): State<AppState>, ctx: Ctx, Path(id): Path<String>) -> AppResult<Response> {
    ctx.require_admin()?;
    let s = get_hall(&st, &id).await?;
    let sid = s.id;
    let n: i64 = st
        .db
        .call(move |c| Ok(c.query_row("SELECT COUNT(*) FROM paintings WHERE section_id = ?1", [sid], |r| r.get(0))?))
        .await?;
    let text = format!("“{}” zali va undagi {n} ta asar (rasmlari bilan) butunlay o'chiriladi.", s.name);
    render_confirm(&st, &ctx, "Zalni o'chirish", &text, "/panel/halls/").await
}

pub async fn delete(State(st): State<AppState>, ctx: Ctx, Path(id): Path<String>, _data: FormData) -> AppResult<Response> {
    ctx.require_admin()?;
    let s = get_hall(&st, &id).await?;
    let sid = s.id;
    let files = st.db.call(move |c| models::delete_section(c, sid)).await?;
    images::remove_media_async(st.cfg.media_dir.clone(), files).await;
    ctx.success(format!("“{}” zali o'chirildi.", s.name));
    Ok(redirect("/panel/halls/"))
}

//! Muzey sahifalari, 3D sayohat, asarlar va moderatsiya (Django `gallery/views.py`).
use std::collections::{BTreeMap, HashMap};

use axum::{
    extract::{Path, Query, State},
    http::header,
    response::{IntoResponse, Response},
};
use rusqlite::params;
use serde::Deserialize;
use serde_json::json;

use super::{parse_id, render_confirm, render_form, safe_next_or, FormPage};
use crate::{
    config::MAX_UPLOAD_MB,
    error::{redirect, AppError, AppResult},
    forms::{Field, Form, Opt},
    images,
    models::{self, media_url, now_iso, Painting, Section, SLOTS},
    render::render,
    sculpt,
    session::{Ctx, FormData},
    AppState,
};

// ---------------------------------------------------------------- sahifalar

/// Bosh sahifa ochiq (kirish tugmasi bilan), lekin asarlar faqat tizimga
/// kirganlarga va faqat ular ko'rishi mumkin bo'lganlari hisoblanadi.
pub async fn home(State(st): State<AppState>, ctx: Ctx) -> AppResult<Response> {
    let user = ctx.user.clone();
    let (sections, latest) = st
        .db
        .call(move |c| {
            let layout = models::hall_layout(c, user.as_ref(), None)?;
            let mut sections = models::all_sections(c)?;
            for s in &mut sections {
                s.visible_count = layout.get(&s.id).map(|m| m.len() as i64).unwrap_or(0);
            }
            let latest = models::paintings_where(
                c,
                &format!("{} AND p.owner_id IS NOT NULL", models::visible_sql(user.as_ref())),
                "ORDER BY p.created_at DESC, p.id DESC LIMIT 8",
                [],
            )?;
            Ok((sections, latest))
        })
        .await?;
    render(&st, &ctx, "gallery/home.html", json!({ "sections": sections, "latest": models::views(&latest) })).await
}

async fn get_section(st: &AppState, slug: String) -> AppResult<Section> {
    st.db.call(move |c| Ok(models::section_by_slug(c, &slug)?)).await?.ok_or(AppError::NotFound)
}

/// Bo'sh yoki band o'ringa rasm qo'yish manzili.
fn place_url(section_slug: &str, slot: i64) -> String {
    let q = serde_urlencoded::to_string([("section", section_slug.to_string()), ("slot", slot.to_string())])
        .unwrap_or_default();
    format!("/my/art/new/?{q}")
}

/// O'rindagi asarni almashtirish manzili: o'z asari — tahrirlash, fond asari — yangi rasm.
fn replace_url(p: &Painting, user_id: i64, slot: i64) -> String {
    if p.owner_id == Some(user_id) {
        format!("/my/art/{}/edit/", p.id)
    } else {
        place_url(&p.section_slug, slot)
    }
}

/// Zal sahifasi: 8 ta o'rin, har birida — asar yoki bo'sh joy, almashtirish tugmasi bilan.
pub async fn section_detail(State(st): State<AppState>, ctx: Ctx, Path(slug): Path<String>) -> AppResult<Response> {
    let user = ctx.require_login()?;
    let section = get_section(&st, slug).await?;
    let sid = section.id;
    let u = user.clone();
    let mut layout = st.db.call(move |c| Ok(models::hall_layout(c, Some(&u), Some(sid))?)).await?;
    let placed = layout.remove(&sid).unwrap_or_default();
    let slots_count = if section.is_marry_me {
        1
    } else if section.is_love {
        3
    } else {
        SLOTS
    };
    let slots: Vec<serde_json::Value> = (1..=slots_count)
        .map(|n| match placed.get(&n) {
            Some(p) => json!({
                "slot": n,
                "p": p.view(),
                "mine": p.owner_id == Some(user.id),
                "replace_url": replace_url(p, user.id, n),
            }),
            None => json!({ "slot": n, "p": null, "mine": false, "replace_url": place_url(&section.slug, n) }),
        })
        .collect();
    render(
        &st,
        &ctx,
        "gallery/section.html",
        json!({ "section": section, "slots": slots, "filled": placed.len().min(slots_count as usize), "total": slots_count }),
    )
    .await
}

/// Eski manzil: endi yaxlit muzeyda shu zal eshigidan boshlanadi.
pub async fn section_3d(State(st): State<AppState>, ctx: Ctx, Path(slug): Path<String>) -> AppResult<Response> {
    ctx.require_login()?;
    let section = get_section(&st, slug).await?;
    let q = serde_urlencoded::to_string([("hall", section.slug.as_str())]).unwrap_or_default();
    Ok(redirect(&format!("/tour/?{q}")))
}

#[derive(Deserialize)]
pub struct TourQuery {
    #[serde(default)]
    hall: String,
    #[serde(default)]
    share: Option<String>,
}

/// Yaxlit 3D muzey: lobbi -> koridor -> barcha zallar.
/// Har bir zalda 8 ta o'rin; tashrifchi fond asarlarini va o'zi qo'ygan asarlarni ko'radi.
/// Agar `share` parametri bo'lsa — mehmon (faqat ko'rish) rejimida ochiladi.
pub async fn tour(State(st): State<AppState>, ctx: Ctx, Query(q): Query<TourQuery>) -> AppResult<Response> {
    let share_param = q.share.as_deref().map(str::trim).filter(|s| !s.is_empty());

    let (is_read_only, tour_user, share_token_str) = match share_param {
        Some(token) => {
            let t = token.to_string();
            let sharer = st
                .db
                .call(move |c| Ok(models::user_by_share_token(c, &t)?))
                .await?
                .ok_or(AppError::NotFound)?;
            // Sessiyada saqlash, shunda protected_media mehmonni taniydi
            ctx.set_guest_share(sharer.id);
            let token_str = sharer.share_token.clone().unwrap_or_else(|| token.to_string());
            (true, sharer, token_str)
        }
        None => {
            let user = ctx.require_login()?;
            let uid = user.id;
            let token_str = match &user.share_token {
                Some(t) if !t.is_empty() => t.clone(),
                _ => st.db.call(move |c| Ok(models::ensure_share_token(c, uid)?)).await?,
            };
            (false, user, token_str)
        }
    };

    let u = tour_user.clone();
    let (sections, mut layout) = st
        .db
        .call(move |c| Ok((models::all_sections(c)?, models::hall_layout(c, Some(&u), None)?)))
        .await?;

    let halls: Vec<serde_json::Value> = sections
        .iter()
        .map(|s| {
            let placed = layout.remove(&s.id).unwrap_or_default();
            let is_love = s.kind == "love" || s.slug == "love";
            let is_marry_me = s.kind == "marry_me" || s.slug == "marry_me" || s.slug == "uzbekistan";
            let slots_count = if is_marry_me { 1 } else if is_love { 3 } else { SLOTS };
            let items: Vec<serde_json::Value> = placed
                .iter()
                .filter(|(slot, _)| **slot <= slots_count)
                .map(|(slot, p)| {
                    let replace = if is_read_only {
                        None
                    } else {
                        Some(replace_url(p, tour_user.id, *slot))
                    };
                    json!({
                        "id": p.id,
                        "slot": slot,
                        "title": p.title,
                        "artist": p.author_name(),
                        "year": p.year,
                        "description": p.description,
                        "facts": p.facts,
                        "image": p.image_url(),
                        "depth": p.is_sculpture().then(|| media_url(&p.depth)),
                        "w": p.image_width.filter(|w| *w > 0).unwrap_or(1),
                        "h": p.image_height.filter(|h| *h > 0).unwrap_or(1),
                        "url": if is_read_only { "".to_string() } else { p.url() },
                        "replace": replace,
                        "owner": p.owner_username,
                        "mine": if is_read_only { false } else { p.owner_id == Some(tour_user.id) },
                    })
                })
                .collect();
            json!({
                "slug": s.slug,
                "name": s.name,
                "tagline": s.tagline,
                "accent": s.accent,
                "wall": s.wall_color,
                "floor": s.floor,
                "kind": s.kind,
                "slots": slots_count,
                "paintings": items,
            })
        })
        .collect();

    let visitor_name = if is_read_only {
        format!("Mehmon ({} muzeyida)", tour_user.display_name())
    } else {
        tour_user.display_name()
    };

    let museum = json!({
        "halls": halls,
        "start": q.hall,
        "visitor": visitor_name,
        "read_only": is_read_only,
        "share_token": share_token_str,
        "owner_name": tour_user.display_name(),
    });

    render(
        &st,
        &ctx,
        "gallery/tour.html",
        json!({
            "museum": museum,
            "halls": halls,
            "read_only": is_read_only,
            "share_token": share_token_str,
            "owner_name": tour_user.display_name(),
        }),
    )
    .await
}

pub async fn painting_detail(State(st): State<AppState>, ctx: Ctx, Path(id): Path<String>) -> AppResult<Response> {
    let user = ctx.require_login()?;
    let id = parse_id(&id)?;
    let p = st.db.call(move |c| Ok(models::painting_by_id(c, id)?)).await?.ok_or(AppError::NotFound)?;
    if !p.can_view(Some(&user)) {
        return Err(AppError::NotFound); // boshqa userning asari mavjudligini ham oshkor qilmaymiz
    }
    let can_edit = p.owner_id == Some(user.id);
    // fond asarini istalgan foydalanuvchi o'z rasmi bilan almashtira oladi (faqat o'zi uchun)
    let replace = match (p.owner_id, p.slot) {
        (None, Some(slot)) => Some(place_url(&p.section_slug, slot)),
        _ => None,
    };
    render(&st, &ctx, "gallery/painting.html", json!({ "p": p.view(), "can_edit": can_edit, "replace": replace }))
        .await
}

fn content_type(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "application/octet-stream",
    }
}

/// `/media/` ostidagi barcha fayllar shu yerdan beriladi.
///
/// Fayl bazadagi yozuv orqali topiladi va ruxsat tekshiriladi:
/// asar rasmi (va uning haykal fayllari) -> `Painting::can_view`,
/// profil rasmi -> egasi yoki admin. Bazada yo'q yoki ruxsat bo'lmasa — 404.
pub async fn protected_media(State(st): State<AppState>, ctx: Ctx, Path(path): Path<String>) -> AppResult<Response> {
    if ctx.user.is_none() && ctx.guest_sharer_id().is_none() {
        return Err(ctx.require_login().unwrap_err());
    }
    let (p2, user, guest_sharer_id) = (path.clone(), ctx.user.clone(), ctx.guest_sharer_id());
    let allowed = st
        .db
        .call(move |c| {
            if let Some(p) = models::painting_by_image(c, &p2)? {
                if let Some(ref u) = user {
                    if p.can_view(Some(u)) {
                        return Ok(true);
                    }
                }
                // Mehmon uchun: e'lon qilingan fond asari yoki ko'rilayotgan foydalanuvchining asari
                if p.is_published {
                    if p.owner_id.is_none() {
                        return Ok(true);
                    }
                    if let Some(gid) = guest_sharer_id {
                        if p.owner_id == Some(gid) {
                            return Ok(true);
                        }
                    }
                }
                return Ok(false);
            }
            if let Some(ref u) = user {
                let owner: Option<i64> = rusqlite::OptionalExtension::optional(c.query_row(
                    "SELECT id FROM users WHERE avatar = ?1 AND avatar != '' LIMIT 1",
                    [&p2],
                    |r| r.get(0),
                ))?;
                return Ok(matches!(owner, Some(id) if id == u.id || u.is_museum_admin()));
            }
            Ok(false)
        })
        .await?;
    if !allowed {
        return Err(AppError::NotFound);
    }
    let media = st.cfg.media_dir.clone();
    let rel = path.clone();
    let bytes = tokio::task::spawn_blocking(move || -> Option<Vec<u8>> {
        let root = media.canonicalize().ok()?;
        let full = root.join(&rel).canonicalize().ok()?;
        if !full.starts_with(&root) || !full.is_file() {
            return None;
        }
        std::fs::read(&full).ok()
    })
    .await?
    .ok_or(AppError::NotFound)?;
    Ok((
        [
            (header::CONTENT_TYPE, content_type(&path)),
            (header::CACHE_CONTROL, "private, max-age=3600"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes,
    )
        .into_response())
}

// ---------------------------------------------------------------- user: o'z asarlari

async fn load_sections(st: &AppState) -> AppResult<Vec<Section>> {
    st.db.call(|c| Ok(models::all_sections(c)?)).await
}

fn painting_form(sections: &[Section], p: Option<&Painting>, initial_section: Option<i64>, initial_slot: Option<i64>) -> Form {
    let mut options = vec![Opt::new("", "— zalni tanlang —")];
    options.extend(sections.iter().map(|s| {
        let label = if s.is_sculpture {
            format!("{} (haykal)", s.name)
        } else if s.is_mosaic {
            format!("{} (mozaika)", s.name)
        } else {
            s.name.clone()
        };
        Opt::new(s.id.to_string(), label)
    }));
    let mut slot_options = vec![Opt::new("", "Avtomatik — birinchi bo'sh o'rin")];
    slot_options.extend((1..=SLOTS).map(|n| Opt::new(n.to_string(), format!("{n}-o'rin"))));
    let mut image = Field::file("image", "Rasm", "image/*")
        .help("JPG, PNG, WEBP yoki GIF, 10 MB gacha. Rasmni faqat siz va administrator ko'radi.");
    image = match p {
        Some(p) => image.current(media_url(&p.image), p.image.clone()),
        None => image.required(),
    };
    let section = p.map(|p| p.section_id).or(initial_section).map(|id| id.to_string()).unwrap_or_default();
    let slot = p.and_then(|p| p.slot).or(initial_slot).map(|n| n.to_string()).unwrap_or_default();
    Form::new(vec![
        image,
        Field::text("title", "Nomi").required().max(160).value(p.map(|p| p.title.clone()).unwrap_or_default()),
        Field::select("section", "Zal", options)
            .required()
            .help("Haykallar zalida rasm marmar haykalga, Mozaika zalida esa koshinli tosh mozaikaga aylantiriladi.")
            .value(section),
        Field::select("slot", "O'rin", slot_options)
            .help(&format!(
                "Har bir zalda {SLOTS} ta o'rin bor. Muzey fondi asari turgan o'rinni tanlasangiz, u faqat siz uchun \
                 almashtiriladi; o'z asaringiz turgan o'rinni tanlasangiz, eski asar yangisiga almashtiriladi."
            ))
            .value(slot),
        Field::text("artist", "Rassom / muallif")
            .max(160)
            .help("Bo'sh qoldirilsa, profilingizdagi ism ko'rsatiladi.")
            .value(p.map(|p| p.artist.clone()).unwrap_or_default()),
        Field::number("year", "Yil").value(p.and_then(|p| p.year).map(|y| y.to_string()).unwrap_or_default()),
        Field::textarea("description", "Tavsif", 4).value(p.map(|p| p.description.clone()).unwrap_or_default()),
        Field::textarea("facts", "Qiziqarli fakt", 3).value(p.map(|p| p.facts.clone()).unwrap_or_default()),
    ])
}

/// Forma uchun: har bir zalning har bir o'rnida hozir nima turgani (foydalanuvchi nuqtai nazaridan).
async fn slot_data(st: &AppState, uid: i64, editing: Option<i64>) -> AppResult<serde_json::Value> {
    st.db
        .call(move |c| {
            let sections = models::all_sections(c)?;
            let mut labels: HashMap<i64, BTreeMap<i64, String>> = HashMap::new();
            for p in models::paintings_where(c, "p.owner_id IS NULL AND p.is_published = 1 AND p.slot IS NOT NULL", "", [])? {
                labels.entry(p.section_id).or_default().insert(
                    p.slot.unwrap_or_default(),
                    format!("“{}” — muzey fondi (siz uchun almashtiriladi)", p.title),
                );
            }
            for p in models::paintings_where(c, "p.owner_id = ?1 AND p.slot IS NOT NULL", "", [uid])? {
                let text = if Some(p.id) == editing {
                    "shu asarning hozirgi o'rni".to_string()
                } else if editing.is_some() {
                    format!("band: sizning “{}” asaringiz", p.title)
                } else {
                    format!("sizning “{}” asaringiz — yangisiga almashtiriladi", p.title)
                };
                labels.entry(p.section_id).or_default().insert(p.slot.unwrap_or_default(), text);
            }
            let map: serde_json::Map<String, serde_json::Value> = sections
                .iter()
                .map(|s| {
                    let l = labels.remove(&s.id).unwrap_or_default();
                    let l: serde_json::Map<String, serde_json::Value> =
                        l.into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
                    (s.id.to_string(), json!({ "sculpture": s.is_sculpture, "labels": l }))
                })
                .collect();
            Ok(serde_json::Value::Object(map))
        })
        .await
}

fn create_page(slots: serde_json::Value) -> FormPage<'static> {
    FormPage {
        title: "Yangi rasm yuklash".into(),
        submit: "Yuklash",
        back: "/profile/",
        multipart: true,
        preview: None,
        slots: Some(slots),
    }
}

fn edit_page(p: &Painting, slots: serde_json::Value) -> FormPage<'static> {
    FormPage {
        title: format!("Tahrirlash: {}", p.title),
        submit: "Saqlash",
        back: "/profile/",
        multipart: true,
        preview: Some(p.image_url()),
        slots: Some(slots),
    }
}

#[derive(Deserialize)]
pub struct CreateQuery {
    #[serde(default)]
    section: String,
    #[serde(default)]
    slot: String,
}

pub async fn painting_create_page(
    State(st): State<AppState>,
    ctx: Ctx,
    Query(q): Query<CreateQuery>,
) -> AppResult<Response> {
    let user = ctx.require_login()?;
    let sections = load_sections(&st).await?;
    let initial = sections.iter().find(|s| !q.section.is_empty() && s.slug == q.section).map(|s| s.id);
    let slot = q.slot.parse::<i64>().ok().filter(|n| (1..=SLOTS).contains(n));
    let slots = slot_data(&st, user.id, None).await?;
    render_form(&st, &ctx, &painting_form(&sections, None, initial, slot), create_page(slots)).await
}

/// Forma va (ixtiyoriy) rasmni tekshiradi.
fn validate_painting(form: &mut Form, data: &mut FormData) -> Option<crate::session::Upload> {
    form.bind(data);
    let upload = data.files.remove("image");
    if let Some(up) = &upload {
        if let Err(e) = images::validate_image(&up.filename, &up.bytes, MAX_UPLOAD_MB) {
            form.add_error("image", e);
        }
    }
    upload
}

const ALL_SLOTS_TAKEN: &str =
    "Bu zaldagi barcha o'rinlarda sizning asarlaringiz turibdi. Qaysi birini almashtirishni tanlang.";

/// Haykal zali bo'lsa — rasmni haykalga aylantiradi. Natija: haykal tayyor bo'ldimi.
async fn make_sculpture_if_needed(st: &AppState, section: Option<&Section>, pid: i64) -> AppResult<Option<bool>> {
    if !section.is_some_and(|s| s.is_sculpture) {
        return Ok(None);
    }
    sculpt::ensure(&st.db, &st.cfg.media_dir).await;
    let ok = st
        .db
        .call(move |c| {
            Ok(c.query_row("SELECT depth != '' AND sculpture != '' FROM paintings WHERE id = ?1", [pid], |r| r.get(0))?)
        })
        .await?;
    Ok(Some(ok))
}

/// Mozaika zali bo'lsa — rasmni mozaikaga aylantiradi. Natija: mozaika tayyor bo'ldimi.
async fn make_mosaic_if_needed(st: &AppState, section: Option<&Section>, pid: i64) -> AppResult<Option<bool>> {
    if !section.is_some_and(|s| s.is_mosaic) {
        return Ok(None);
    }
    crate::mosaic::ensure(&st.db, &st.cfg.media_dir).await;
    let ok = st
        .db
        .call(move |c| {
            Ok(c.query_row("SELECT mosaic != '' FROM paintings WHERE id = ?1", [pid], |r| r.get(0))?)
        })
        .await?;
    Ok(Some(ok))
}

fn effect_note(sculpt: Option<bool>, mosaic: Option<bool>) -> &'static str {
    match (sculpt, mosaic) {
        (Some(true), _) => " Rasm marmar haykalga aylantirildi.",
        (Some(false), _) => " Haykal yasab bo'lmadi — rasm oddiy ko'rinishda qo'yildi.",
        (_, Some(true)) => " Rasm koshinli tosh mozaikaga aylantirildi.",
        (_, Some(false)) => " Mozaika yasab bo'lmadi — rasm oddiy ko'rinishda qo'yildi.",
        _ => "",
    }
}

pub async fn painting_create(State(st): State<AppState>, ctx: Ctx, mut data: FormData) -> AppResult<Response> {
    let user = ctx.require_login()?;
    let uid = user.id;
    let sections = load_sections(&st).await?;
    let mut form = painting_form(&sections, None, None, None);
    let upload = validate_painting(&mut form, &mut data);
    let section_id = form.val("section").parse::<i64>().unwrap_or_default();
    let mut slot = form.val("slot").parse::<i64>().ok();
    if !form.has_errors() && slot.is_none() {
        slot = st.db.call(move |c| Ok(models::auto_slot(c, uid, section_id, None)?)).await?;
        if slot.is_none() {
            form.add_error("slot", ALL_SLOTS_TAKEN);
        }
    }
    let (Some(upload), Some(slot)) = (upload.filter(|_| !form.has_errors()), slot) else {
        let slots = slot_data(&st, uid, None).await?;
        return render_form(&st, &ctx, &form, create_page(slots)).await;
    };
    let (rel, dims) =
        images::store_upload(st.cfg.media_dir.clone(), format!("paintings/{}", user.username), upload, 2048).await?;
    let title = form.val("title").to_string();
    let f = form.clone();
    let (pid, old, fund) = st
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            // shu o'rindagi o'z asari yangisiga almashtiriladi
            let old = models::own_at_slot(&tx, uid, section_id, slot, None)?;
            if let Some(o) = &old {
                tx.execute("DELETE FROM paintings WHERE id = ?1", [o.id])?;
            }
            let fund = models::fund_at_slot(&tx, section_id, slot)?.filter(|p| p.is_published).map(|p| p.title);
            let now = now_iso();
            tx.execute(
                "INSERT INTO paintings (section_id, owner_id, title, artist, year, description, facts,
                        image, image_width, image_height, is_published, slot, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11, ?12, ?12)",
                params![
                    section_id,
                    uid,
                    f.val("title"),
                    f.val("artist"),
                    f.val("year").parse::<i64>().ok(),
                    f.val("description"),
                    f.val("facts"),
                    rel,
                    dims.map(|d| d.0),
                    dims.map(|d| d.1),
                    slot,
                    now
                ],
            )?;
            let pid = tx.last_insert_rowid();
            tx.commit()?;
            Ok((pid, old, fund))
        })
        .await?;
    if let Some(o) = &old {
        images::remove_media_async(st.cfg.media_dir.clone(), o.files()).await;
    }
    let made_sculpt = make_sculpture_if_needed(&st, sections.iter().find(|s| s.id == section_id), pid).await?;
    let made_mosaic = make_mosaic_if_needed(&st, sections.iter().find(|s| s.id == section_id), pid).await?;
    let msg = match (&old, &fund) {
        (Some(o), _) => format!("{slot}-o'rindagi “{}” o'rniga “{title}” qo'yildi.", o.title),
        (None, Some(f)) => format!("“{title}” {slot}-o'ringa qo'yildi — muzey fondidagi “{f}” siz uchun almashtirildi."),
        (None, None) => format!("“{title}” muzeyga qo'shildi ({slot}-o'rin)."),
    };
    ctx.success(format!("{msg}{}", effect_note(made_sculpt, made_mosaic)));
    Ok(redirect("/profile/"))
}

/// Foydalanuvchi faqat o'ziga tegishli asarni ola oladi — aks holda 404.
async fn own_painting(st: &AppState, ctx: &Ctx, id: &str) -> AppResult<Painting> {
    let user = ctx.require_login()?;
    let id = parse_id(id)?;
    st.db
        .call(move |c| Ok(models::painting_by_id(c, id)?))
        .await?
        .filter(|p| p.owner_id == Some(user.id))
        .ok_or(AppError::NotFound)
}

pub async fn painting_edit_page(State(st): State<AppState>, ctx: Ctx, Path(id): Path<String>) -> AppResult<Response> {
    let p = own_painting(&st, &ctx, &id).await?;
    let sections = load_sections(&st).await?;
    let slots = slot_data(&st, p.owner_id.unwrap_or_default(), Some(p.id)).await?;
    render_form(&st, &ctx, &painting_form(&sections, Some(&p), None, None), edit_page(&p, slots)).await
}

pub async fn painting_edit(
    State(st): State<AppState>,
    ctx: Ctx,
    Path(id): Path<String>,
    mut data: FormData,
) -> AppResult<Response> {
    let p = own_painting(&st, &ctx, &id).await?;
    let user = ctx.require_login()?;
    let uid = user.id;
    let sections = load_sections(&st).await?;
    let mut form = painting_form(&sections, Some(&p), None, None);
    let upload = validate_painting(&mut form, &mut data);
    let section_id = form.val("section").parse::<i64>().unwrap_or_default();
    let mut slot = form.val("slot").parse::<i64>().ok();
    if !form.has_errors() {
        if slot.is_none() {
            slot = if p.section_id == section_id && p.slot.is_some() {
                p.slot
            } else {
                st.db.call(move |c| Ok(models::auto_slot(c, uid, section_id, Some(p.id)))).await??
            };
        }
        match slot {
            None => form.add_error("slot", ALL_SLOTS_TAKEN),
            Some(n) => {
                let pid = p.id;
                let busy = st.db.call(move |c| Ok(models::own_at_slot(c, uid, section_id, n, Some(pid))?)).await?;
                if let Some(o) = busy {
                    form.add_error(
                        "slot",
                        format!(
                            "Bu o'rinda sizning “{}” asaringiz turibdi. Boshqa o'rin tanlang yoki avval o'sha asarni tahrirlang.",
                            o.title
                        ),
                    );
                }
            }
        }
    }
    if form.has_errors() {
        let slots = slot_data(&st, uid, Some(p.id)).await?;
        return render_form(&st, &ctx, &form, edit_page(&p, slots)).await;
    }
    let new_image = match upload {
        Some(up) => Some(
            images::store_upload(st.cfg.media_dir.clone(), format!("paintings/{}", user.username), up, 2048).await?,
        ),
        None => None,
    };
    let f = form.clone();
    let (pid, img) = (p.id, new_image.clone());
    st.db
        .call(move |c| {
            c.execute(
                "UPDATE paintings SET section_id = ?1, slot = ?2, title = ?3, artist = ?4, year = ?5,
                        description = ?6, facts = ?7, updated_at = ?8 WHERE id = ?9",
                params![
                    section_id,
                    slot,
                    f.val("title"),
                    f.val("artist"),
                    f.val("year").parse::<i64>().ok(),
                    f.val("description"),
                    f.val("facts"),
                    now_iso(),
                    pid
                ],
            )?;
            if let Some((rel, dims)) = img {
                // yangi rasm: eski haykal va mozaika ham qayta yasaladi
                c.execute(
                    "UPDATE paintings SET image = ?1, image_width = ?2, image_height = ?3, sculpture = '', depth = '', mosaic = ''
                     WHERE id = ?4",
                    params![rel, dims.map(|d| d.0), dims.map(|d| d.1), pid],
                )?;
            }
            Ok(())
        })
        .await?;
    if new_image.is_some() {
        images::remove_media_async(st.cfg.media_dir.clone(), p.files()).await;
    }
    let made_sculpt = make_sculpture_if_needed(&st, sections.iter().find(|s| s.id == section_id), p.id).await?;
    let made_mosaic = make_mosaic_if_needed(&st, sections.iter().find(|s| s.id == section_id), p.id).await?;
    ctx.success(format!("Asar yangilandi.{}", effect_note(made_sculpt, made_mosaic)));
    Ok(redirect("/profile/"))
}

pub async fn painting_delete_page(State(st): State<AppState>, ctx: Ctx, Path(id): Path<String>) -> AppResult<Response> {
    let p = own_painting(&st, &ctx, &id).await?;
    render_confirm(&st, &ctx, "Asarni o'chirish", &format!("“{}” butunlay o'chiriladi.", p.title), "/profile/").await
}

async fn delete_painting(st: &AppState, p: &Painting) -> AppResult<()> {
    let pid = p.id;
    st.db
        .call(move |c| {
            c.execute("DELETE FROM paintings WHERE id = ?1", [pid])?;
            Ok(())
        })
        .await?;
    images::remove_media_async(st.cfg.media_dir.clone(), p.files()).await;
    Ok(())
}

pub async fn painting_delete(
    State(st): State<AppState>,
    ctx: Ctx,
    Path(id): Path<String>,
    _data: FormData,
) -> AppResult<Response> {
    let p = own_painting(&st, &ctx, &id).await?;
    delete_painting(&st, &p).await?;
    ctx.success("Asar o'chirildi.");
    Ok(redirect("/profile/"))
}

// ---------------------------------------------------------------- admin: moderatsiya

fn back(data: &FormData) -> Response {
    redirect(safe_next_or(data.get("next"), "/panel/art/"))
}

#[derive(Deserialize)]
pub struct ModerationQuery {
    f: Option<String>,
}

pub async fn moderation(State(st): State<AppState>, ctx: Ctx, Query(q): Query<ModerationQuery>) -> AppResult<Response> {
    ctx.require_admin()?;
    let flt = q.f.unwrap_or_else(|| "uploads".into());
    let where_sql = match flt.as_str() {
        "uploads" => "p.owner_id IS NOT NULL",
        "hidden" => "p.is_published = 0",
        _ => "1",
    };
    let paintings = st
        .db
        .call(move |c| Ok(models::paintings_where(c, where_sql, "ORDER BY p.created_at DESC, p.id DESC", [])?))
        .await?;
    render(&st, &ctx, "panel/moderation.html", json!({ "paintings": models::views(&paintings), "f": flt })).await
}

async fn any_painting(st: &AppState, id: &str) -> AppResult<Painting> {
    let id = parse_id(id)?;
    st.db.call(move |c| Ok(models::painting_by_id(c, id)?)).await?.ok_or(AppError::NotFound)
}

pub async fn moderation_toggle(
    State(st): State<AppState>,
    ctx: Ctx,
    Path(id): Path<String>,
    data: FormData,
) -> AppResult<Response> {
    ctx.require_admin()?;
    let p = any_painting(&st, &id).await?;
    let (pid, published) = (p.id, !p.is_published);
    st.db
        .call(move |c| {
            c.execute("UPDATE paintings SET is_published = ?1 WHERE id = ?2", params![published, pid])?;
            Ok(())
        })
        .await?;
    ctx.success(format!("“{}” {}", p.title, if published { "ko'rsatilmoqda." } else { "yashirildi." }));
    Ok(back(&data))
}

pub async fn moderation_delete(
    State(st): State<AppState>,
    ctx: Ctx,
    Path(id): Path<String>,
    data: FormData,
) -> AppResult<Response> {
    ctx.require_admin()?;
    let p = any_painting(&st, &id).await?;
    delete_painting(&st, &p).await?;
    ctx.success(format!("“{}” o'chirildi.", p.title));
    Ok(back(&data))
}

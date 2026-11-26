//! Shablonlarni chizish (Django template engine o'rniga MiniJinja).
//! Shablonlar kompilyatsiya vaqtida binar faylga joylanadi.
use std::sync::LazyLock;

use axum::response::{Html, IntoResponse, Response};
use minijinja::{Environment, Value};
use serde_json::{json, Map};

use crate::{error::{AppError, ErrorPage}, models, session::Ctx, AppState};

macro_rules! templates {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_str!(concat!("../templates/", $name)))),*]
    };
}

const TEMPLATES: &[(&str, &str)] = templates![
    "base.html",
    "error.html",
    "accounts/_fields.html",
    "accounts/confirm.html",
    "accounts/form.html",
    "accounts/login.html",
    "accounts/profile.html",
    "gallery/_card.html",
    "gallery/home.html",
    "gallery/painting.html",
    "gallery/section.html",
    "gallery/tour.html",
    "panel/dashboard.html",
    "panel/moderation.html",
    "panel/halls.html",
];

static ENV: LazyLock<Environment<'static>> = LazyLock::new(|| {
    let mut env = Environment::new();
    for (name, source) in TEMPLATES {
        env.add_template(name, source).unwrap_or_else(|e| panic!("shablon xatosi {name}: {e}"));
    }
    env.add_filter("linebreaksbr", linebreaksbr);
    env.add_filter("escapejs", escapejs);
    env
});

pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
    out
}

/// Django `linebreaksbr`: matnni xavfsiz qilib, qator ko'chishini `<br>` ga aylantiradi.
fn linebreaksbr(value: String) -> Value {
    let escaped = escape_html(&value).replace("\r\n", "\n").replace('\n', "<br>");
    Value::from_safe_string(escaped)
}

/// Django `escapejs`: JavaScript satri ichiga xavfsiz joylash.
fn escapejs(value: String) -> Value {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\\' | '\'' | '"' | '<' | '>' | '&' | '=' | '-' | ';' | '`' | '\u{2028}' | '\u{2029}' => {
                out.push_str(&format!("\\u{:04X}", ch as u32))
            }
            c if (c as u32) < 32 => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    Value::from_safe_string(out)
}

/// Sahifani chizadi. `page` — sahifaga xos o'zgaruvchilar (JSON obyekt);
/// unga umumiy kontekst (user, nav_sections, messages, csrf_token, request) qo'shiladi.
pub async fn render(st: &AppState, ctx: &Ctx, name: &str, page: serde_json::Value) -> Result<Response, AppError> {
    let html = render_string(st, ctx, name, page).await?;
    Ok(Html(html).into_response())
}

pub async fn render_string(
    st: &AppState,
    ctx: &Ctx,
    name: &str,
    page: serde_json::Value,
) -> Result<String, AppError> {
    let nav = if ctx.user.is_some() { st.db.call(|c| Ok(models::nav_sections(c)?)).await? } else { vec![] };
    let mut map = match page {
        serde_json::Value::Object(m) => m,
        _ => Map::new(),
    };
    let user = match &ctx.user {
        Some(u) => json!(u.view()),
        None => json!({ "is_authenticated": false, "is_museum_admin": false }),
    };
    map.insert("user".into(), user);
    map.insert("nav_sections".into(), json!(nav));
    map.insert("messages".into(), json!(ctx.take_messages()));
    map.insert("csrf_token".into(), json!(ctx.csrf_token()));
    map.insert("request".into(), json!({ "path": ctx.path, "full_path": ctx.full_path }));

    let tmpl = ENV.get_template(name)?;
    Ok(tmpl.render(Value::from_serialize(&map))?)
}

pub async fn render_error(st: &AppState, ctx: &Ctx, page: &ErrorPage) -> Result<Response, AppError> {
    render(st, ctx, "error.html", json!({ "title": page.title, "message": page.message })).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_templates_compile() {
        for (name, _) in super::TEMPLATES {
            super::ENV.get_template(name).unwrap();
        }
    }
}

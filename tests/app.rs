//! Django loyihasidagi `accounts/tests.py` testlarining Rust'dagi nusxasi
//! (va bir nechta qo'shimcha: CSRF, parol almashtirish, zallarni boshqarish).
use std::{io::Cursor, path::PathBuf};

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use museum::{auth, build_app, models, AppState, Config};
use rusqlite::params;
use tower::ServiceExt;

const PW: &str = "S3cret-pass!";

struct TestApp {
    app: Router,
    state: AppState,
    dir: PathBuf,
    alice: i64,
    bob: i64,
    boss: i64,
    art: i64,
}

impl Drop for TestApp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(w, h, image::Rgb([200, 50, 50])));
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png).unwrap();
    buf
}

fn add_user(state: &AppState, username: &str, role: &str, superuser: bool) -> i64 {
    let hash = auth::make_password(PW, 1000);
    state.db.with(|c| {
        models::insert_user(
            c,
            &models::NewUser {
                username,
                password_hash: &hash,
                first_name: "",
                last_name: "",
                email: "",
                role,
                is_superuser: superuser,
                created_by_id: None,
            },
        )
        .unwrap()
    })
}

impl TestApp {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("museum-test-{}", auth::random_string(12)));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = Config {
            base_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
            db_path: dir.join("test.sqlite3"),
            media_dir: dir.join("media"),
            addr: String::new(),
            secure_cookies: false,
            pbkdf2_iterations: 1000,
            legacy_dir: dir.clone(),
            django_dir: dir.clone(),
        };
        let state = AppState::new(cfg).unwrap();
        let boss = add_user(&state, "boss", "admin", false);
        let alice = add_user(&state, "alice", "user", false);
        let bob = add_user(&state, "bob", "user", false);
        let art = state.db.with(|c| {
            c.execute(r#"INSERT INTO sections (slug, name, "order") VALUES ('art', 'Art', 0)"#, []).unwrap();
            c.last_insert_rowid()
        });
        TestApp { app: build_app(state.clone()), state, dir, alice, bob, boss, art }
    }

    fn client(&self) -> Client<'_> {
        Client { t: self, cookie: None }
    }

    fn add_section(&self, slug: &str, name: &str, order: i64) -> i64 {
        self.state.db.with(|c| {
            c.execute(r#"INSERT INTO sections (slug, name, "order") VALUES (?1, ?2, ?3)"#, params![slug, name, order])
                .unwrap();
            c.last_insert_rowid()
        })
    }

    /// Asar yaratadi (rasm fayli bilan). Qaytaradi: (id, rasm yo'li).
    fn add_painting(&self, section: i64, owner: Option<i64>, title: &str, published: bool) -> (i64, String) {
        let rel = format!("paintings/test/{}.png", auth::random_string(8));
        let full = self.state.cfg.media_dir.join(&rel);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, png(40, 30)).unwrap();
        let now = models::now_iso();
        let id = self.state.db.with(|c| {
            c.execute(
                "INSERT INTO paintings (section_id, owner_id, title, image, image_width, image_height,
                        is_published, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, 40, 30, ?5, ?6, ?6)",
                params![section, owner, title, rel, published, now],
            )
            .unwrap();
            let id = c.last_insert_rowid();
            models::assign_missing_slots(c).unwrap();
            id
        });
        (id, rel)
    }

    fn painting_count(&self, title: &str) -> i64 {
        self.state
            .db
            .with(|c| c.query_row("SELECT COUNT(*) FROM paintings WHERE title = ?1", [title], |r| r.get(0)).unwrap())
    }

    fn user(&self, id: i64) -> models::User {
        self.state.db.with(|c| models::user_by_id(c, id).unwrap().unwrap())
    }
}

struct Resp {
    status: StatusCode,
    location: String,
    content_type: String,
    cache_control: String,
    body: String,
}

struct Client<'a> {
    t: &'a TestApp,
    cookie: Option<String>,
}

enum Part<'a> {
    Text(&'a str, &'a str),
    File(&'a str, &'a str, Vec<u8>),
}

impl Client<'_> {
    async fn send(&mut self, mut req: Request<Body>) -> Resp {
        if let Some(c) = &self.cookie {
            req.headers_mut().insert(header::COOKIE, format!("museum_session={c}").parse().unwrap());
        }
        let resp = self.t.app.clone().oneshot(req).await.unwrap();
        for v in resp.headers().get_all(header::SET_COOKIE) {
            let v = v.to_str().unwrap();
            if let Some(rest) = v.strip_prefix("museum_session=") {
                self.cookie = Some(rest.split(';').next().unwrap().to_string());
            }
        }
        let h = |name| resp.headers().get(name).map(|v: &header::HeaderValue| v.to_str().unwrap().to_string()).unwrap_or_default();
        let (status, location, content_type, cache_control) =
            (resp.status(), h(header::LOCATION), h(header::CONTENT_TYPE), h(header::CACHE_CONTROL));
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        Resp { status, location, content_type, cache_control, body: String::from_utf8_lossy(&bytes).into_owned() }
    }

    async fn get(&mut self, path: &str) -> Resp {
        self.send(Request::get(path).body(Body::empty()).unwrap()).await
    }

    /// Yo'naltirishlar bo'ylab yuradi; oxirgi manzilni qaytaradi.
    async fn follow(&mut self, mut resp: Resp) -> (Resp, String) {
        let mut url = String::new();
        while resp.status == StatusCode::FOUND {
            url = resp.location.clone();
            resp = self.get(&url).await;
        }
        (resp, url)
    }

    async fn csrf(&mut self) -> String {
        let mut r = self.get("/").await;
        if !r.body.contains("csrfmiddlewaretoken") {
            r = self.get("/login/").await;
        }
        let marker = "name=\"csrfmiddlewaretoken\" value=\"";
        let start = r.body.find(marker).expect("csrf token topilmadi") + marker.len();
        r.body[start..start + 48].to_string()
    }

    async fn post(&mut self, path: &str, fields: &[(&str, &str)]) -> Resp {
        let token = self.csrf().await;
        let mut all: Vec<(&str, &str)> = fields.to_vec();
        all.push(("csrfmiddlewaretoken", &token));
        let body = serde_urlencoded::to_string(&all).unwrap();
        self.send(
            Request::post(path)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
    }

    async fn post_multipart(&mut self, path: &str, parts: Vec<Part<'_>>) -> Resp {
        let token = self.csrf().await;
        let boundary = "----museumtestboundary";
        let mut body: Vec<u8> = Vec::new();
        let text = |name: &str, value: &str, body: &mut Vec<u8>| {
            body.extend_from_slice(
                format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes(),
            );
        };
        text("csrfmiddlewaretoken", &token, &mut body);
        for p in parts {
            match p {
                Part::Text(n, v) => text(n, v, &mut body),
                Part::File(n, filename, bytes) => {
                    body.extend_from_slice(
                        format!(
                            "--{boundary}\r\nContent-Disposition: form-data; name=\"{n}\"; filename=\"{filename}\"\r\nContent-Type: image/png\r\n\r\n"
                        )
                        .as_bytes(),
                    );
                    body.extend_from_slice(&bytes);
                    body.extend_from_slice(b"\r\n");
                }
            }
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        self.send(
            Request::post(path)
                .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
    }

    async fn login(&mut self, username: &str, password: &str) -> bool {
        self.cookie = None;
        let r = self.post("/login/", &[("username", username), ("password", password)]).await;
        r.status == StatusCode::FOUND && !r.location.starts_with("/login")
    }

    /// 3D sayohat sahifasidagi JSON: (ma'lumot, {nomi: mine}).
    async fn tour(&mut self) -> (serde_json::Value, std::collections::BTreeMap<String, bool>) {
        let r = self.get("/tour/").await;
        assert_eq!(r.status, StatusCode::OK);
        let marker = "<script id=\"museum-data\" type=\"application/json\">";
        let start = r.body.find(marker).unwrap() + marker.len();
        let end = start + r.body[start..].find("</script>").unwrap();
        let data: serde_json::Value = serde_json::from_str(&r.body[start..end]).unwrap();
        let mut seen = std::collections::BTreeMap::new();
        for h in data["halls"].as_array().unwrap() {
            for p in h["paintings"].as_array().unwrap() {
                seen.insert(p["title"].as_str().unwrap().to_string(), p["mine"].as_bool().unwrap());
            }
        }
        (data, seen)
    }
}

// ---------------------------------------------------------------- 1-ruxsat: admin

#[tokio::test]
async fn admin_creates_login() {
    let t = TestApp::new();
    let mut c = t.client();
    assert!(c.login("boss", PW).await);
    let r = c
        .post(
            "/panel/users/new/",
            &[
                ("username", "newbie"),
                ("first_name", "Ali"),
                ("last_name", ""),
                ("email", ""),
                ("role", "user"),
                ("password1", "Muzey-2026-pass"),
                ("password2", "Muzey-2026-pass"),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::FOUND, "{}", r.body);
    assert_eq!(r.location, "/panel/");
    let u = t.state.db.with(|c| models::user_by_username(c, "newbie").unwrap().unwrap());
    assert_eq!(u.created_by_id, Some(t.boss));
    assert!(t.client().login("newbie", "Muzey-2026-pass").await);
}

#[tokio::test]
async fn user_cannot_access_admin_pages() {
    let t = TestApp::new();
    let mut c = t.client();
    assert!(c.login("alice", PW).await);
    for url in ["/panel/", "/panel/users/new/", &format!("/panel/users/{}/edit/", t.bob), "/panel/art/", "/panel/halls/"] {
        assert_eq!(c.get(url).await.status, StatusCode::FORBIDDEN, "{url}");
    }
    let r = c
        .post(
            "/panel/users/new/",
            &[("username", "hacker"), ("role", "admin"), ("password1", "Muzey-2026-pass"), ("password2", "Muzey-2026-pass")],
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert!(t.state.db.with(|c| models::user_by_username(c, "hacker").unwrap()).is_none());
}

#[tokio::test]
async fn admin_can_view_panel() {
    let t = TestApp::new();
    let mut c = t.client();
    assert!(c.login("boss", PW).await);
    let r = c.get("/panel/").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
}

#[tokio::test]
async fn anonymous_redirected_to_login() {
    let t = TestApp::new();
    let r = t.client().get("/panel/").await;
    assert_eq!(r.status, StatusCode::FOUND);
    assert!(r.location.starts_with("/login/"));
}

#[tokio::test]
async fn admin_cannot_block_self() {
    let t = TestApp::new();
    let mut c = t.client();
    assert!(c.login("boss", PW).await);
    c.post(&format!("/panel/users/{}/toggle/", t.boss), &[]).await;
    assert!(t.user(t.boss).is_active);
}

#[tokio::test]
async fn admin_can_block_user() {
    let t = TestApp::new();
    let mut alice = t.client();
    assert!(alice.login("alice", PW).await);
    let mut c = t.client();
    assert!(c.login("boss", PW).await);
    c.post(&format!("/panel/users/{}/toggle/", t.alice), &[]).await;
    assert!(!t.client().login("alice", PW).await);
    // mavjud sessiya ham darhol bekor bo'ladi
    assert_eq!(alice.get("/profile/").await.status, StatusCode::FOUND);
}

#[tokio::test]
async fn plain_admin_cannot_touch_superuser() {
    let t = TestApp::new();
    let su = add_user(&t.state, "root", "admin", true);
    let mut c = t.client();
    assert!(c.login("boss", PW).await);
    assert_eq!(c.get(&format!("/panel/users/{su}/delete/")).await.status, StatusCode::FORBIDDEN);
}

// ---------------------------------------------------------------- 2-ruxsat: user o'z rasmini yuklaydi

#[tokio::test]
async fn user_uploads_own_painting() {
    let t = TestApp::new();
    let mut c = t.client();
    assert!(c.login("alice", PW).await);
    let section = t.art.to_string();
    let r = c
        .post_multipart(
            "/my/art/new/",
            vec![
                Part::File("image", "a.png", png(40, 30)),
                Part::Text("title", "Mening rasmim"),
                Part::Text("section", &section),
                Part::Text("artist", ""),
                Part::Text("year", "2026"),
                Part::Text("description", "x"),
                Part::Text("facts", ""),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::FOUND, "{}", r.body);
    assert_eq!(r.location, "/profile/");
    let (owner, w, h, image, year): (Option<i64>, i64, i64, String, Option<i64>) = t.state.db.with(|c| {
        c.query_row(
            "SELECT owner_id, image_width, image_height, image, year FROM paintings WHERE title = 'Mening rasmim'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap()
    });
    assert_eq!(owner, Some(t.alice));
    assert_eq!((w, h), (40, 30));
    assert_eq!(year, Some(2026));
    assert!(image.starts_with("paintings/alice/"));
    assert!(t.state.cfg.media_dir.join(&image).is_file());
    assert!(c.get("/tour/").await.body.contains("Mening rasmim"));
}

#[tokio::test]
async fn user_cannot_edit_or_delete_others_painting() {
    let t = TestApp::new();
    let (p, _) = t.add_painting(t.art, Some(t.bob), "Bob", true);
    let mut c = t.client();
    assert!(c.login("alice", PW).await);
    assert_eq!(c.get(&format!("/my/art/{p}/edit/")).await.status, StatusCode::NOT_FOUND);
    assert_eq!(c.post(&format!("/my/art/{p}/delete/"), &[]).await.status, StatusCode::NOT_FOUND);
    assert_eq!(t.painting_count("Bob"), 1);
}

#[tokio::test]
async fn rejects_non_image() {
    let t = TestApp::new();
    let mut c = t.client();
    assert!(c.login("alice", PW).await);
    let section = t.art.to_string();
    let r = c
        .post_multipart(
            "/my/art/new/",
            vec![
                Part::File("image", "x.png", b"not an image".to_vec()),
                Part::Text("title", "Bad"),
                Part::Text("section", &section),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.contains("Fayl yaroqli rasm emas."));
    assert_eq!(t.painting_count("Bad"), 0);
}

#[tokio::test]
async fn user_uploads_avatar() {
    let t = TestApp::new();
    let mut c = t.client();
    assert!(c.login("alice", PW).await);
    let r = c
        .post_multipart(
            "/profile/edit/",
            vec![
                Part::Text("first_name", "Alisa"),
                Part::Text("last_name", ""),
                Part::Text("email", ""),
                Part::Text("bio", ""),
                Part::File("avatar", "me.png", png(40, 30)),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::FOUND, "{}", r.body);
    let u = t.user(t.alice);
    assert!(u.avatar.starts_with("avatars/alice/"), "{}", u.avatar);
    assert_eq!(u.first_name, "Alisa");
}

#[tokio::test]
async fn hidden_painting_not_public() {
    let t = TestApp::new();
    let (p, _) = t.add_painting(t.art, Some(t.bob), "Yashirin", false);
    // admin yashirgan asar 3D muzeyda hatto egasiga ham chiqmaydi,
    // lekin egasi uni o'z sahifasida ko'ra oladi
    let mut c = t.client();
    assert!(c.login("bob", PW).await);
    assert!(!c.get("/tour/").await.body.contains("Yashirin"));
    assert_eq!(c.get(&format!("/art/{p}/")).await.status, StatusCode::OK);
}

#[tokio::test]
async fn moderation_toggle_rejects_external_next() {
    let t = TestApp::new();
    let (p, _) = t.add_painting(t.art, Some(t.bob), "B", true);
    let mut c = t.client();
    assert!(c.login("boss", PW).await);
    let r = c.post(&format!("/panel/art/{p}/toggle/"), &[("next", "https://evil.example")]).await;
    assert_eq!(r.location, "/panel/art/");
    let r = c.post(&format!("/panel/art/{p}/toggle/"), &[("next", "//evil.example")]).await;
    assert_eq!(r.location, "/panel/art/");
}

#[tokio::test]
async fn login_redirects_by_role() {
    let t = TestApp::new();
    let mut c = t.client();
    let r = c.post("/login/", &[("username", "boss"), ("password", PW)]).await;
    assert_eq!(c.follow(r).await.1, "/panel/");
    let mut c = t.client();
    let r = c.post("/login/", &[("username", "alice"), ("password", PW)]).await;
    assert_eq!(c.follow(r).await.1, "/profile/");
}

#[tokio::test]
async fn wrong_password_shows_error() {
    let t = TestApp::new();
    let mut c = t.client();
    let r = c.post("/login/", &[("username", "alice"), ("password", "wrong-pass")]).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.contains("Login yoki parol noto&#x27;g&#x27;ri."), "{}", r.body);
}

// ---------------------------------------------------------------- virtual sayohat

#[tokio::test]
async fn tour_shows_only_own_uploads_and_museum_fund() {
    let t = TestApp::new();
    let other = t.add_section("history", "Tarix", 1);
    t.add_painting(t.art, None, "Fond asari", true);
    t.add_painting(other, Some(t.alice), "Alisa asari", true);
    t.add_painting(t.art, Some(t.bob), "Bob asari", true);

    let mut c = t.client();
    assert!(c.login("alice", PW).await);
    let (data, seen) = c.tour().await;
    let slugs: Vec<&str> = data["halls"].as_array().unwrap().iter().map(|h| h["slug"].as_str().unwrap()).collect();
    assert_eq!(slugs, ["art", "history"]);
    assert_eq!(seen, [("Alisa asari".to_string(), true), ("Fond asari".to_string(), false)].into());

    assert!(c.login("bob", PW).await);
    assert_eq!(c.tour().await.1, [("Bob asari".to_string(), true), ("Fond asari".to_string(), false)].into());

    // admin ham zallarda fond + o'z asarlarini ko'radi; foydalanuvchilar asarlari — moderatsiyada
    assert!(c.login("boss", PW).await);
    let titles: Vec<String> = c.tour().await.1.into_keys().collect();
    assert_eq!(titles, ["Fond asari"]);
    let r = c.get("/panel/art/").await;
    assert!(r.body.contains("Alisa asari") && r.body.contains("Bob asari"));
}

// ---------------------------------------------------------------- maxfiylik

#[tokio::test]
async fn anonymous_cannot_tour_or_browse() {
    let t = TestApp::new();
    let (p, img) = t.add_painting(t.art, None, "Fond", true);
    let mut c = t.client();
    for url in ["/tour/".to_string(), "/hall/art/".to_string(), format!("/art/{p}/"), format!("/media/{img}")] {
        let r = c.get(&url).await;
        assert_eq!(r.status, StatusCode::FOUND, "{url}");
        assert!(r.location.starts_with("/login/"), "{url}");
    }
    assert!(!c.get("/").await.body.contains("Fond"));
}

#[tokio::test]
async fn other_user_cannot_see_upload_anywhere() {
    let t = TestApp::new();
    let (p, img) = t.add_painting(t.art, Some(t.alice), "Shaxsiy rasm", true);
    let mut c = t.client();
    assert!(c.login("bob", PW).await);
    assert_eq!(c.get(&format!("/art/{p}/")).await.status, StatusCode::NOT_FOUND);
    assert_eq!(c.get(&format!("/media/{img}")).await.status, StatusCode::NOT_FOUND); // fayl ham yopiq
    assert!(!c.get("/hall/art/").await.body.contains("Shaxsiy rasm"));
    assert!(!c.get("/").await.body.contains("Shaxsiy rasm"));
    assert!(!c.get("/tour/").await.body.contains("Shaxsiy rasm"));
}

#[tokio::test]
async fn owner_and_admin_can_see_upload_and_file() {
    let t = TestApp::new();
    let (p, img) = t.add_painting(t.art, Some(t.alice), "Shaxsiy rasm", true);
    for who in ["alice", "boss"] {
        let mut c = t.client();
        assert!(c.login(who, PW).await);
        assert_eq!(c.get(&format!("/art/{p}/")).await.status, StatusCode::OK, "{who}");
        let r = c.get(&format!("/media/{img}")).await;
        assert_eq!(r.status, StatusCode::OK, "{who}");
        assert_eq!(r.content_type, "image/png");
        assert!(r.cache_control.contains("private"));
    }
}

#[tokio::test]
async fn museum_fund_visible_to_every_user() {
    let t = TestApp::new();
    let (p, img) = t.add_painting(t.art, None, "Fond", true);
    let mut c = t.client();
    assert!(c.login("bob", PW).await);
    assert_eq!(c.get(&format!("/art/{p}/")).await.status, StatusCode::OK);
    assert_eq!(c.get(&format!("/media/{img}")).await.status, StatusCode::OK);
}

#[tokio::test]
async fn avatar_private() {
    let t = TestApp::new();
    let rel = "avatars/alice/me.png";
    let full = t.state.cfg.media_dir.join(rel);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(&full, png(10, 10)).unwrap();
    t.state.db.with(|c| c.execute("UPDATE users SET avatar = ?1 WHERE id = ?2", params![rel, t.alice]).unwrap());
    let mut c = t.client();
    assert!(c.login("bob", PW).await);
    assert_eq!(c.get(&format!("/media/{rel}")).await.status, StatusCode::NOT_FOUND);
    assert!(c.login("alice", PW).await);
    assert_eq!(c.get(&format!("/media/{rel}")).await.status, StatusCode::OK);
}

#[tokio::test]
async fn media_path_traversal_blocked() {
    let t = TestApp::new();
    let mut c = t.client();
    assert!(c.login("boss", PW).await);
    assert_eq!(c.get("/media/../Cargo.toml").await.status, StatusCode::NOT_FOUND);
    assert_eq!(c.get("/media/%2e%2e/Cargo.toml").await.status, StatusCode::NOT_FOUND);
    assert_eq!(c.get("/media/paintings/nope.png").await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn old_hall_3d_url_redirects_to_tour() {
    let t = TestApp::new();
    let mut c = t.client();
    assert!(c.login("alice", PW).await);
    let r = c.get("/hall/art/3d/").await;
    assert_eq!(r.status, StatusCode::FOUND);
    assert_eq!(r.location, "/tour/?hall=art");
}

// ---------------------------------------------------------------- qo'shimcha

#[tokio::test]
async fn post_without_csrf_is_rejected() {
    let t = TestApp::new();
    let mut c = t.client();
    assert!(c.login("boss", PW).await);
    let r = c
        .send(
            Request::post(format!("/panel/users/{}/toggle/", t.alice))
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(""))
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert!(t.user(t.alice).is_active);
}

#[tokio::test]
async fn password_change_keeps_current_session_only() {
    let t = TestApp::new();
    let mut other = t.client();
    assert!(other.login("alice", PW).await);
    let mut c = t.client();
    assert!(c.login("alice", PW).await);
    let r = c
        .post(
            "/profile/password/",
            &[("old_password", PW), ("new_password1", "Yangi-parol-2026"), ("new_password2", "Yangi-parol-2026")],
        )
        .await;
    assert_eq!(r.status, StatusCode::FOUND, "{}", r.body);
    assert_eq!(c.get("/profile/").await.status, StatusCode::OK);
    assert_eq!(other.get("/profile/").await.status, StatusCode::FOUND);
    assert!(t.client().login("alice", "Yangi-parol-2026").await);
}

#[tokio::test]
async fn admin_manages_halls() {
    let t = TestApp::new();
    let mut c = t.client();
    assert!(c.login("boss", PW).await);
    let r = c
        .post(
            "/panel/halls/new/",
            &[
                ("name", "Yangi zal"),
                ("slug", "yangi"),
                ("tagline", ""),
                ("description", ""),
                ("accent", "#AABBCC"),
                ("wall_color", "#112233"),
                ("floor", "floor_stone"),
                ("order", "5"),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::FOUND, "{}", r.body);
    let (_, seen) = c.tour().await;
    assert!(seen.is_empty());
    assert!(c.get("/hall/yangi/").await.body.contains("Yangi zal"));
    // takroriy kod rad etiladi
    let r = c
        .post(
            "/panel/halls/new/",
            &[("name", "X"), ("slug", "yangi"), ("accent", "#aabbcc"), ("wall_color", "#112233"), ("floor", "floor_stone")],
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
}

#[tokio::test]
async fn append_slash_and_404() {
    let t = TestApp::new();
    let mut c = t.client();
    let r = c.get("/login").await;
    assert_eq!(r.status, StatusCode::MOVED_PERMANENTLY);
    assert_eq!(r.location, "/login/");
    let r = c.get("/no-such-page/").await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert!(r.body.contains("Sahifa topilmadi"));
}

#[tokio::test]
async fn static_files_served() {
    let t = TestApp::new();
    let r = t.client().get("/static/css/museum.css").await;
    assert_eq!(r.status, StatusCode::OK);
}

// ---------------------------------------------------------------- 8 ta o'rin va almashtirish

/// Rasm yuklaydi (`slot` bo'sh bo'lsa — avtomatik o'rin).
async fn upload(c: &mut Client<'_>, section: i64, slot: &str, title: &str, img: Vec<u8>) -> Resp {
    let section = section.to_string();
    c.post_multipart(
        "/my/art/new/",
        vec![
            Part::File("image", "a.png", img),
            Part::Text("title", title),
            Part::Text("section", &section),
            Part::Text("slot", slot),
            Part::Text("artist", ""),
            Part::Text("year", ""),
            Part::Text("description", ""),
            Part::Text("facts", ""),
        ],
    )
    .await
}

fn slots_of(data: &serde_json::Value, hall: &str) -> std::collections::BTreeMap<i64, String> {
    let h = data["halls"].as_array().unwrap().iter().find(|h| h["slug"] == hall).unwrap();
    h["paintings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["slot"].as_i64().unwrap(), p["title"].as_str().unwrap().to_string()))
        .collect()
}

fn id_of(t: &TestApp, title: &str) -> i64 {
    t.state.db.with(|c| c.query_row("SELECT id FROM paintings WHERE title = ?1", [title], |r| r.get(0)).unwrap())
}

#[tokio::test]
async fn hall_holds_at_most_eight_and_auto_slot_prefers_free() {
    let t = TestApp::new();
    for i in 1..=6 {
        t.add_painting(t.art, None, &format!("Fond {i}"), true);
    }
    let mut c = t.client();
    assert!(c.login("alice", PW).await);
    // avtomatik o'rin: avval fond egallamagan 7 va 8
    for title in ["A7", "A8"] {
        let r = upload(&mut c, t.art, "", title, png(40, 30)).await;
        assert_eq!(r.status, StatusCode::FOUND, "{}", r.body);
    }
    let s = slots_of(&c.tour().await.0, "art");
    assert_eq!(s.len(), 8);
    assert_eq!((s[&7].as_str(), s[&8].as_str()), ("A7", "A8"));
    // keyingi avtomatik yuklash fond o'rnini (1) egallaydi — faqat alisa uchun
    let r = upload(&mut c, t.art, "", "A1", png(40, 30)).await;
    assert_eq!(r.status, StatusCode::FOUND, "{}", r.body);
    let s = slots_of(&c.tour().await.0, "art");
    assert_eq!(s.len(), 8);
    assert_eq!(s[&1], "A1");
    // bob uchun fond asari o'z joyida
    let mut b = t.client();
    assert!(b.login("bob", PW).await);
    assert_eq!(slots_of(&b.tour().await.0, "art")[&1], "Fond 1");
    // 8 ta o'rinning hammasi o'ziniki bo'lsa — avtomatik joy yo'q
    for slot in ["2", "3", "4", "5", "6"] {
        assert_eq!(upload(&mut c, t.art, slot, &format!("A{slot}"), png(40, 30)).await.status, StatusCode::FOUND);
    }
    let r = upload(&mut c, t.art, "", "Ortiqcha", png(40, 30)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.contains("barcha o&#x27;rinlarda"), "{}", r.body);
    assert_eq!(t.painting_count("Ortiqcha"), 0);
    // mavjud bo'lmagan o'rin raqami rad etiladi
    assert_eq!(upload(&mut c, t.art, "9", "X", png(40, 30)).await.status, StatusCode::OK);
    assert_eq!(t.painting_count("X"), 0);
}

#[tokio::test]
async fn user_replaces_own_painting_in_slot() {
    let t = TestApp::new();
    let mut c = t.client();
    assert!(c.login("alice", PW).await);
    assert_eq!(upload(&mut c, t.art, "3", "Eski", png(40, 30)).await.status, StatusCode::FOUND);
    let old_img: String =
        t.state.db.with(|c| c.query_row("SELECT image FROM paintings WHERE title = 'Eski'", [], |r| r.get(0)).unwrap());
    let r = upload(&mut c, t.art, "3", "Yangi", png(40, 30)).await;
    assert_eq!(r.status, StatusCode::FOUND, "{}", r.body);
    assert_eq!(t.painting_count("Eski"), 0); // eski asar almashtirildi
    assert!(!t.state.cfg.media_dir.join(&old_img).exists()); // fayli ham o'chdi
    assert_eq!(slots_of(&c.tour().await.0, "art")[&3], "Yangi");
}

#[tokio::test]
async fn edit_cannot_move_onto_own_occupied_slot() {
    let t = TestApp::new();
    let mut c = t.client();
    assert!(c.login("alice", PW).await);
    upload(&mut c, t.art, "1", "Bir", png(40, 30)).await;
    upload(&mut c, t.art, "2", "Ikki", png(40, 30)).await;
    let id = id_of(&t, "Ikki");
    let section = t.art.to_string();
    let r = c
        .post_multipart(
            &format!("/my/art/{id}/edit/"),
            vec![Part::Text("title", "Ikki"), Part::Text("section", &section), Part::Text("slot", "1")],
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.contains("asaringiz turibdi"), "{}", r.body);
    let r = c
        .post_multipart(
            &format!("/my/art/{id}/edit/"),
            vec![Part::Text("title", "Ikki"), Part::Text("section", &section), Part::Text("slot", "5")],
        )
        .await;
    assert_eq!(r.status, StatusCode::FOUND, "{}", r.body);
    assert_eq!(slots_of(&c.tour().await.0, "art")[&5], "Ikki");
}

#[tokio::test]
async fn hall_page_shows_eight_slots_with_replace_links() {
    let t = TestApp::new();
    let (fund, _) = t.add_painting(t.art, None, "Fond", true);
    let mut c = t.client();
    assert!(c.login("bob", PW).await);
    let body = c.get("/hall/art/").await.body;
    assert_eq!(body.matches("class=\"slot-num\"").count(), 8);
    assert!(body.contains("section=art&amp;slot=1"), "{body}");
    assert!(body.contains("section=art&amp;slot=8"));
    assert!(c.get(&format!("/art/{fund}/")).await.body.contains("O'z rasmim bilan almashtirish"));
}

// ---------------------------------------------------------------- haykallar zali

/// Bir rangli fonda doira — fon ajratiladi va doira haykalga aylanadi.
fn disk_png() -> Vec<u8> {
    let img = image::RgbImage::from_fn(120, 100, |x, y| {
        let (dx, dy) = (x as f32 - 60.0, y as f32 - 50.0);
        if dx * dx + dy * dy < 35.0 * 35.0 {
            image::Rgb([150, 90, 40])
        } else {
            image::Rgb([240, 230, 210])
        }
    });
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgb8(img).write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png).unwrap();
    buf
}

#[tokio::test]
async fn upload_to_sculpture_hall_becomes_sculpture() {
    let t = TestApp::new();
    let hall = t.state.db.with(|c| {
        c.execute(
            r#"INSERT INTO sections (slug, name, "order", kind) VALUES ('sculptures', 'Haykallar', 1, 'sculptures')"#,
            [],
        )
        .unwrap();
        c.last_insert_rowid()
    });
    let mut c = t.client();
    assert!(c.login("alice", PW).await);
    let r = upload(&mut c, hall, "", "Mening haykalim", disk_png()).await;
    assert_eq!(r.status, StatusCode::FOUND, "{}", r.body);
    let (sculpture, depth): (String, String) = t.state.db.with(|c| {
        c.query_row("SELECT sculpture, depth FROM paintings WHERE title = 'Mening haykalim'", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap()
    });
    assert!(!sculpture.is_empty() && !depth.is_empty());
    let d = image::open(t.state.cfg.media_dir.join(&depth)).unwrap();
    assert!(d.width() <= 360 && d.height() <= 360);
    // 3D ma'lumotida chuqurlik xaritasi bor, ko'rsatiladigan rasm — haykal surati
    let (data, _) = c.tour().await;
    let h = data["halls"].as_array().unwrap().iter().find(|h| h["slug"] == "sculptures").unwrap();
    assert_eq!(h["kind"], "sculptures");
    let p = &h["paintings"][0];
    assert_eq!(p["depth"], format!("/media/{depth}"));
    assert_eq!(p["image"], format!("/media/{sculpture}"));
    // haykal fayllari ham himoyalangan: egasi ko'radi, boshqalar — yo'q
    assert_eq!(c.get(&format!("/media/{depth}")).await.status, StatusCode::OK);
    let mut b = t.client();
    assert!(b.login("bob", PW).await);
    assert_eq!(b.get(&format!("/media/{depth}")).await.status, StatusCode::NOT_FOUND);
    // o'chirilganda haykal fayllari ham o'chadi
    let id = id_of(&t, "Mening haykalim");
    assert_eq!(c.post(&format!("/my/art/{id}/delete/"), &[]).await.status, StatusCode::FOUND);
    assert!(!t.state.cfg.media_dir.join(&depth).exists());
    assert!(!t.state.cfg.media_dir.join(&sculpture).exists());
}

#[tokio::test]
async fn history_hall_is_converted_to_sculptures_on_startup() {
    let dir = std::env::temp_dir().join(format!("museum-test-{}", auth::random_string(12)));
    std::fs::create_dir_all(dir.join("media/paintings/museum")).unwrap();
    std::fs::write(dir.join("media/paintings/museum/his.png"), disk_png()).unwrap();
    // eski sxemadagi baza: kind/slot/sculpture/depth ustunlari yo'q
    {
        let c = rusqlite::Connection::open(dir.join("old.sqlite3")).unwrap();
        c.execute_batch(
            r#"CREATE TABLE sections (id INTEGER PRIMARY KEY AUTOINCREMENT, slug TEXT NOT NULL UNIQUE, name TEXT NOT NULL,
                 tagline TEXT NOT NULL DEFAULT '', description TEXT NOT NULL DEFAULT '', accent TEXT NOT NULL DEFAULT '#d4af37',
                 wall_color TEXT NOT NULL DEFAULT '#262a36', floor TEXT NOT NULL DEFAULT 'floor_marble', "order" INTEGER NOT NULL DEFAULT 0);
               CREATE TABLE paintings (id INTEGER PRIMARY KEY AUTOINCREMENT, section_id INTEGER NOT NULL, owner_id INTEGER NULL,
                 title TEXT NOT NULL, artist TEXT NOT NULL DEFAULT '', year INTEGER NULL, description TEXT NOT NULL DEFAULT '',
                 facts TEXT NOT NULL DEFAULT '', image TEXT NOT NULL, image_width INTEGER NULL, image_height INTEGER NULL,
                 is_published INTEGER NOT NULL DEFAULT 1, legacy_id TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL);
               INSERT INTO sections (slug, name) VALUES ('history', 'Tarix muzeyi');
               INSERT INTO paintings (section_id, title, image, created_at, updated_at)
                 VALUES (1, 'Piramida', 'paintings/museum/his.png', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');"#,
        )
        .unwrap();
    }
    let cfg = Config {
        base_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        db_path: dir.join("old.sqlite3"),
        media_dir: dir.join("media"),
        addr: String::new(),
        secure_cookies: false,
        pbkdf2_iterations: 1000,
        legacy_dir: dir.clone(),
        django_dir: dir.clone(),
    };
    let state = AppState::new(cfg).unwrap();
    let (slug, kind, slot, depth): (String, String, Option<i64>, String) = state.db.with(|c| {
        c.query_row(
            "SELECT s.slug, s.kind, p.slot, p.depth FROM paintings p JOIN sections s ON s.id = p.section_id",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap()
    });
    assert_eq!((slug.as_str(), kind.as_str(), slot), ("sculptures", "sculptures", Some(1)));
    assert!(dir.join("media").join(&depth).is_file());
    drop(state);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn share_qr_guest_tour_read_only() {
    let t = TestApp::new();
    let (_alice_paint, alice_img) = t.add_painting(t.art, Some(t.alice), "Alisaning rasmi", true);
    let (_fund_paint, fund_img) = t.add_painting(t.art, None, "Fond rasmi", true);

    // Alice share tokenini olamiz
    let alice_token: String = t.state.db.with(|c| {
        c.query_row("SELECT share_token FROM users WHERE id = ?1", [t.alice], |r| r.get(0)).unwrap()
    });
    assert!(!alice_token.is_empty());

    // 1. Tizimga kirmagan mehmon oddiy /tour/ ga kirsa -> login sahifasiga yo'naltiriladi
    let mut anon = t.client();
    let r = anon.get("/tour/").await;
    assert_eq!(r.status, StatusCode::FOUND);
    assert!(r.location.starts_with("/login/"));

    // 2. Mehmon Alice'ning QR kodi (share_token) orqali kirsa -> 200 OK
    let r2 = anon.get(&format!("/tour/?share={alice_token}")).await;
    assert_eq!(r2.status, StatusCode::OK);
    // Sahifada mehmon faqat sayohat rejimi ekani ko'rsatiladi
    assert!(r2.body.contains("Faqat sayohat"));
    assert!(r2.body.contains("share-modal"));
    assert!(r2.body.contains("qr-heart.js"));

    // 3D museum-data JSON tekshiruvi:
    // read_only = true bo'lishi kerak, barcha replace havolalari null bo'lishi kerak
    let script_start = r2.body.find(r#"id="museum-data""#).expect("museum-data script exists");
    let json_start = r2.body[script_start..].find('>').unwrap() + script_start + 1;
    let json_end = r2.body[json_start..].find("</script>").unwrap() + json_start;
    let museum_json: serde_json::Value = serde_json::from_str(&r2.body[json_start..json_end]).unwrap();
    assert_eq!(museum_json["read_only"], true);
    assert_eq!(museum_json["share_token"], alice_token);

    // Rasmlar orasida replace = null bo'lishi shart
    let hall = &museum_json["halls"][0];
    let paintings = hall["paintings"].as_array().unwrap();
    for p in paintings {
        assert!(p["replace"].is_null(), "mehmon uchun replace null bo'lishi shart");
        assert_eq!(p["mine"], false, "mehmon uchun mine false bo'lishi shart");
    }

    // 3. Mehmon Alice'ning rasmini va fond rasmini ko'ra oladi
    assert_eq!(anon.get(&format!("/media/{alice_img}")).await.status, StatusCode::OK);
    assert_eq!(anon.get(&format!("/media/{fund_img}")).await.status, StatusCode::OK);

    // 4. Lekin mehmon Bob'ning shaxsiy rasmini ko'ra olmaydi
    let (_, bob_img) = t.add_painting(t.art, Some(t.bob), "Bobning rasmi", true);
    assert_eq!(anon.get(&format!("/media/{bob_img}")).await.status, StatusCode::NOT_FOUND);

    // 5. Mehmon rasm yuklash sahifasiga to'g'ridan-to'g'ri kirmoqchi bo'lsa -> loginga yo'naltiriladi
    let r_upload = anon.get("/my/art/new/").await;
    assert_eq!(r_upload.status, StatusCode::FOUND);
    assert!(r_upload.location.starts_with("/login/"));
}

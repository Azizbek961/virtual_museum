# Virtual Museum — Rust versiyasi

Bu papkada `museum_web/` (Django) loyihasining **Rust tilidagi to'liq nusxasi** bor.
Django loyihasi o'z joyida, o'zgartirilmagan holda qoldi. Ikkalasi bir-biridan mustaqil ishlaydi.

Hamma imkoniyatlar saqlangan: 3D muzey (`/tour/`), ikki darajali rollar (Admin / Foydalanuvchi),
rasm yuklash, himoyalangan media fayllar, moderatsiya, admin paneli, profil. Dizayn, CSS va
3D dvigatel (`static/js/museum3d.js`, three.js) Django versiyasidagi bilan bir xil.

| Django'da | Rust'da |
|---|---|
| Django (veb-freymvork) | [axum](https://github.com/tokio-rs/axum) + tokio |
| Django shablonlari | [MiniJinja](https://github.com/mitsuhiko/minijinja) (sintaksisi deyarli bir xil) |
| Django ORM + SQLite | `rusqlite` (SQLite, alohida o'rnatish shart emas) |
| Pillow | `image` |
| `manage.py` | `cargo run -- <buyruq>` |
| `/django-admin/` (zallar) | `/panel/halls/` sahifasi |

---

## 1. Kerakli dasturlar (bir marta o'rnatiladi)

> Bu kompyuterda ikkalasi ham **allaqachon o'rnatilgan** (Rust 1.100 va Visual Studio 2022 Build Tools).
> Boshqa kompyuterda ishga tushirmoqchi bo'lsangiz, quyidagilarni o'rnating.

1. **Visual Studio Build Tools (C++)** — Windows'da Rust kompilyatoriga linker kerak:
   ```powershell
   winget install Microsoft.VisualStudio.2022.BuildTools --override "--passive --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
   ```
2. **Rust** (rustup orqali):
   ```powershell
   winget install Rustlang.Rustup
   ```
3. **Terminalni yopib, qayta oching** (PATH yangilanishi uchun) va tekshiring:
   ```powershell
   cargo --version
   ```

Linux/macOS: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

---

## 2. Ishga tushirish (PowerShell)

```powershell
cd "C:\Users\user\Desktop\museam\museum_rust"

# 1) Muzey fondini yuklash: 5 zal + 30 asar (virtual_museum papkasidan)
cargo run -- seed

# 2) Birinchi administratorni yaratish (parolni so'raydi)
cargo run -- createsuperuser --username admin

# 3) Serverni ishga tushirish
cargo run
```

Brauzerda oching: **http://127.0.0.1:8000/**

To'xtatish: terminalda `Ctrl+C`.

> **Birinchi kompilyatsiya 3–6 daqiqa davom etadi** (kutubxonalar yuklab olinadi va
> kompilyatsiya qilinadi). Keyingi ishga tushirishlar bir necha soniya oladi.

### Variant B: Django'dagi ma'lumotlarni ko'chirish

Agar Django versiyasida foydalanuvchilar, zallar va yuklangan rasmlar bo'lsa, ularni Rust
versiyasiga to'liq ko'chirish mumkin (`seed` va `createsuperuser` o'rniga):

```powershell
cargo run -- import-django
cargo run
```

Bu `..\museum_web\db.sqlite3` va `..\museum_web\media\` dan barcha foydalanuvchilar, zallar,
asarlar va rasm fayllarini ko'chiradi. **Parollar Django bilan bir xil formatda** saqlangani uchun
hamma foydalanuvchilar eski login va parollari bilan kira oladi.
Import faqat bo'sh Rust bazasiga qilinadi (Django bazasi faqat o'qiladi, o'zgarmaydi).

---

## 3. Buyruqlar (`manage.py` o'rniga)

| Buyruq | Vazifasi | Django'dagi muqobili |
|---|---|---|
| `cargo run` | Serverni ishga tushirish | `python manage.py runserver` |
| `cargo run -- serve --addr 0.0.0.0:8000` | Tarmoqdagi boshqa qurilmalar uchun ochish | `runserver 0.0.0.0:8000` |
| `cargo run -- seed` | 5 zal + 30 asarni import qilish | `python manage.py seed_museum` |
| `cargo run -- seed --update` | Mavjud asarlar matnini yangilash | `seed_museum --update` |
| `cargo run -- createsuperuser --username admin` | Administrator yaratish | `python manage.py createsuperuser` |
| `cargo run -- import-django` | Django bazasi va rasmlarini ko'chirish | — |
| `cargo test` | Testlarni ishga tushirish | `python manage.py test` |
| `cargo run -- --help` | Yordam | `python manage.py help` |

Bazaning sxemasi server birinchi marta ishga tushganda avtomatik yaratiladi
(`migrate` buyrug'i kerak emas).

---

## 4. Tez ishlaydigan (release) versiya

```powershell
cargo build --release
.\target\release\museum.exe
```

`museum.exe` — bitta mustaqil fayl (shablonlar uning ichiga joylangan). U `static\` papkasini
joriy papkadan qidiradi, shuning uchun `museum_rust` papkasidan ishga tushiring yoki
`MUSEUM_BASE_DIR` ni ko'rsating.

---

## 5. Sozlamalar (muhit o'zgaruvchilari)

| O'zgaruvchi | Standart qiymat | Vazifasi |
|---|---|---|
| `MUSEUM_ADDR` | `127.0.0.1:8000` | Server manzili va porti |
| `MUSEUM_DB` | `museum.sqlite3` | SQLite baza fayli |
| `MUSEUM_MEDIA` | `media` | Yuklangan rasmlar papkasi |
| `MUSEUM_BASE_DIR` | joriy papka | `static/` turgan papka |
| `MUSEUM_SECURE_COOKIES` | `0` | HTTPS'da `1` qiling (cookie faqat HTTPS orqali yuboriladi) |
| `MUSEUM_PBKDF2_ITERATIONS` | `1000000` | Parol xeshlash kuchi (Django 5.1 bilan bir xil) |
| `MUSEUM_LEGACY_DIR` | `..\virtual_museum` | `seed` uchun manba |
| `MUSEUM_DJANGO_DIR` | `..\museum_web` | `import-django` uchun manba |
| `RUST_LOG` | `info` | Log darajasi (`debug`, `warn`, ...) |

PowerShell'da misol:

```powershell
$env:MUSEUM_ADDR = "0.0.0.0:9000"
cargo run
```

---

## 6. Sahifalar

| URL | Kim uchun | Vazifasi |
|---|---|---|
| `/` | hamma | Bosh sahifa (mehmonga — kirish tugmasi) |
| `/login/` | hamma | Kirish |
| `/tour/` | user, admin | Yaxlit 3D muzeyni aylanish (`?hall=<zal>` — shu zaldan boshlash) |
| `/hall/<zal>/` | user, admin | Zalning 8 ta o'rni: asarlar, bo'sh joylar, "almashtirish" tugmalari |
| `/hall/<zal>/3d/` | user, admin | `/tour/?hall=<zal>` ga yo'naltiradi |
| `/art/<id>/` | ruxsati borlar | Asar sahifasi |
| `/profile/` | user | Mening profilim va asarlarim |
| `/profile/edit/`, `/profile/password/` | user | Profil va parolni o'zgartirish |
| `/my/art/new/` | user | Rasm yuklash |
| `/panel/` | admin | Foydalanuvchilar: yaratish, tahrirlash, parol, bloklash, o'chirish |
| `/panel/art/` | admin | Asarlar moderatsiyasi |
| `/panel/halls/` | admin | **Zallarni boshqarish** (Django'dagi `/django-admin/` o'rniga) |

## 7. Rollar va ruxsatlar (Django versiyasi bilan bir xil)

| Rol | Kim yaratadi | Nima qila oladi |
|---|---|---|
| **Admin** | `createsuperuser` yoki boshqa admin | Login yaratish, tahrirlash, parolini tiklash, bloklash, o'chirish; barcha asarlarni ko'rish, yashirish/o'chirish; zallarni boshqarish |
| **Foydalanuvchi** | Faqat admin | Muzeyni aylanish; o'z profilini tahrirlash; o'z rasmlarini yuklash, tahrirlash, o'chirish |
| Mehmon | — | Faqat bosh sahifa va kirish sahifasi |

- Muzey fondi asarlarini tizimga kirgan hamma ko'radi; user yuklagan rasmni faqat egasi va admin ko'radi.
- Ko'rinish qoidasi bitta joyda: `src/models.rs` → `visible_sql()` va `Painting::can_view()`.
- `/media/...` fayllari ochiq tarqatilmaydi: har bir so'rovda ruxsat tekshiriladi (`protected_media`).
- Barcha POST formalar CSRF token bilan himoyalangan; sessiya cookie'si `HttpOnly`.
- Parol o'zgartirilsa yoki profil bloklansa, boshqa qurilmalardagi sessiyalar darhol bekor bo'ladi.

---

## 8. Loyiha tuzilishi

```
museum_rust/
├── Cargo.toml            # bog'liqliklar (requirements.txt o'rniga)
├── src/
│   ├── main.rs           # CLI buyruqlari: serve, seed, createsuperuser, import-django
│   ├── lib.rs            # barcha URL manzillar (urls.py)
│   ├── config.rs         # sozlamalar (settings.py)
│   ├── db.rs             # SQLite va jadvallar sxemasi (migrations)
│   ├── models.rs         # User, Section, Painting + ko'rinish qoidalari (models.py)
│   ├── session.rs        # sessiya, login, flash xabarlar, CSRF (middleware)
│   ├── auth.rs           # parol xeshlash (Django bilan mos) va validatorlar
│   ├── forms.rs          # formalar va tekshiruvlar (forms.py)
│   ├── images.rs         # rasmni tekshirish, kichraytirish, EXIF burilish (images.py)
│   ├── sculpt.rs         # rasmni haykalga aylantirish (Haykallar zali)
│   ├── render.rs         # shablonlarni chizish
│   ├── seed.rs           # seed_museum buyrug'i
│   ├── import.rs         # Django bazasidan ko'chirish
│   └── views/            # sahifalar (views.py)
│       ├── accounts.rs   #   kirish, profil, foydalanuvchilar paneli
│       ├── gallery.rs    #   bosh sahifa, 3D sayohat, asarlar, moderatsiya, media
│       └── halls.rs      #   zallarni boshqarish
├── templates/            # HTML shablonlar (Django shablonlaridan ko'chirilgan)
├── static/               # CSS, 3D dvigatel (museum3d.js), three.js, teksturalar
└── tests/app.rs          # 33 ta integratsion test (Django testlarining nusxasi + qo'shimchalar)
```

Ish jarayonida paydo bo'ladigan fayllar: `museum.sqlite3` (baza), `media/` (rasmlar),
`target/` (kompilyatsiya natijalari) — ular `.gitignore` da.

> **Shablonni o'zgartirsangiz** (`templates/*.html`), u binar faylga joylanadi —
> o'zgarish ko'rinishi uchun serverni qayta ishga tushiring (`Ctrl+C`, keyin `cargo run`).
> `static/` dagi fayllar (CSS, JS) esa darhol yangilanadi.

---

## 9. Zallardagi 8 ta o'rin va Haykallar zali

**8 ta o'rin.** Har bir zalda 1 dan 8 gacha raqamlangan o'rin bor (3D'da: rasmlar zalida 3 + 2 + 3
devor bo'ylab, haykallar zalida 2 qator postament). Rasm yuklashda zal va o'rin tanlanadi
("Avtomatik" — birinchi bo'sh o'rin; avval fond egallamagan o'rinlar).

- **Muzey fondi asarini almashtirish** — o'sha o'ringa o'z rasmingizni qo'yasiz. Fond asari bazada qoladi,
  almashtirish faqat sizga ko'rinadi (boshqa foydalanuvchilar fond asarini ko'raveradi).
- **O'z asaringizni almashtirish** — band o'ringa yangi rasm yuklasangiz, eski asar (fayli bilan) yangisiga almashtiriladi;
  yoki asarni tahrirlab, rasmini almashtirasiz.
- Almashtirish tugmalari: zal sahifasida (`/hall/<zal>/`), asar sahifasida va 3D'da asar ma'lumot panelida.
  3D'dagi bo'sh o'rin ("+" belgisi) bosilsa — shu o'ringa rasm qo'yish sahifasi ochiladi.
- Foydalanuvchining bitta zaldagi asarlari 8 tadan oshmaydi. Eski bazadagi ortiqcha asarlar o'chirilmaydi:
  ular profilda "Zalda joyi yo'q" belgisi bilan qoladi — tahrirlab, o'rin tanlash mumkin.
- Admin ham zallarda fond + o'z asarlarini ko'radi; foydalanuvchilar yuklagan barcha asarlar — `/panel/art/` da.

**Haykallar zali** (sobiq "Tarix muzeyi", kod: `sculptures`). Bu zalga yuklangan har bir rasm avtomatik
ravishda marmar haykalga aylantiriladi (`src/sculpt.rs`):

1. Fon ajratiladi (rasm chetidan silliq rang sohalari "to'ldiriladi"; shaffof PNG'da alfa kanal ishlatiladi).
   Qolgan shakl — haykal (masalan, byust, piramidalar).
2. Fonni ajratib bo'lmasa (murakkab fotosurat), rasm gumbazli marmar lavhaga (stela) o'yilgan relyef bo'ladi.
3. Ikki fayl yasaladi: `*-depth.png` (chuqurlik xaritasi) — 3D muzey undan haqiqiy 3D haykal quradi;
   `*-sculpture.png` — haykalning "surati" (kartochka va asar sahifasi uchun). Asl rasm ham saqlanadi.

Admin `/panel/halls/` da istalgan zalni "Haykallar zali" turiga o'tkaza oladi — undagi rasmlar ham haykalga aylanadi.
Eski baza birinchi ishga tushishda avtomatik yangilanadi: "Tarix muzeyi" Haykallar zaliga aylanadi va undagi
6 ta asar haykalga aylantiriladi (bir necha soniya).

## 10. Django versiyasidan farqlar

- `/django-admin/` yo'q — zallar `/panel/halls/` sahifasida boshqariladi (yaratish, tahrirlash, o'chirish).
- Asar rasmi yoki avatar almashtirilganda/o'chirilganda eski fayl diskdan ham o'chiriladi
  (Django versiyasida eski fayllar qolib ketardi).
- Login bandligi katta-kichik harfni hisobga olmay tekshiriladi (`Ali` va `ali` — bitta login).
- Rasm o'lchamlari kichraytirish/burishdan keyin saqlanadi (3D'dagi nisbat aniqroq).

## 11. Testlar

```powershell
cargo test
```

Django loyihasidagi barcha testlar (rollar, ruxsatlar, rasm yuklash, maxfiylik, media himoyasi,
path traversal) Rust'ga ko'chirilgan va qo'shimcha testlar (CSRF, parol almashtirish,
zallar boshqaruvi) qo'shilgan.

## 12. Production uchun

- `cargo build --release` bilan yig'ing.
- HTTPS orqali ishlaganda `MUSEUM_SECURE_COOKIES=1` qo'ying.
- Odatda server nginx/Caddy ortida `127.0.0.1:8000` da ishlaydi.
- **Muhim:** `media/` papkasini nginx orqali to'g'ridan-to'g'ri **tarqatmang** — `/media/`
  so'rovlari Rust serverga borishi kerak (ruxsat tekshiriladi). `/static/` ni esa nginx
  bera olishi mumkin.

## 13. Muammolar va yechimlar

| Xato | Yechim |
|---|---|
| `cargo: command not found` / `cargo` tanilmadi | Rust o'rnatilgandan keyin terminalni yopib, qayta oching |
| `linker 'link.exe' not found` | Visual Studio Build Tools (C++) o'rnatilmagan — 1-bo'limga qarang |
| `manzilini band qilib bo'lmadi` | Port band (masalan, Django ham 8000 da ishlayapti). Boshqa port: `cargo run -- serve --addr 127.0.0.1:8001` |
| Bosh sahifada "Hali zallar yo'q" | `cargo run -- seed` ni ishga tushiring |
| `import-django`: "Rust bazasi bo'sh emas" | `museum.sqlite3` va `media\` ni o'chirib, qayta urinib ko'ring |
| 3D muzey ochilmayapti | Brauzer WebGL'ni qo'llab-quvvatlashini tekshiring; menyuda "Grafika: Tejamkor" ni tanlang |

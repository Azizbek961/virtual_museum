//! Rasmni haykalga aylantirish (Haykallar zali uchun).
//!
//! Yuklangan rasmdan ikki fayl yasaladi:
//!   * `*-relief.png` — chuqurlik xaritasi (old va orqa sirt balandligi + shakl).
//!     3D muzey (`museum3d.js`) shu xaritadan haqiqiy hajmli 3D haykal quradi.
//!   * `*-sculpture.png` — haykalning "surati" (yoritilgan marmar) — kartochkalar,
//!     asar sahifasi va ma'lumot paneli uchun.
//!
//! Qanday ishlaydi:
//!   1. Asosiy obyekt (odam, hayvon, buyum) fondan ajratiladi: neyron tarmoq
//!      (`segment.rs`, U²-Net). Shaffof PNG bo'lsa — alfa kanal; model ishlamasa —
//!      rang usuli (rasm chetidan silliq rang sohalarini "to'ldirish").
//!   2. Siluet "puflanadi" (∇²u = −2): har bir bo'lak o'z eniga mos yumaloq hajm oladi —
//!      bosh sharsimon, qo'llar silindrsimon, tana keng va qalin.
//!   3. Ustiga rasm yorqinligining mayda detallari o'yiladi (ko'z, soch, kiyim burmalari).
//!   4. Obyekt topilmasa (masalan, manzara), rasm gumbazsimon marmar lavhaga (stela)
//!      o'yilgan relyef bo'ladi.
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
};

use anyhow::Context;
use image::{imageops::FilterType, ImageBuffer, Rgb, RgbImage, RgbaImage};
use rusqlite::{params, Connection};

use crate::{db::Db, images, models};

/// Ishlov berish o'lchami (uzun tomoni, piksel) — kattaroq = aniqroq detallar.
const WORK: u32 = 560;
/// Chuqurlik xaritasining uzun tomoni — 3D to'r (mesh) zichligi shunga teng.
const DEPTH_MAX: u32 = 360;

struct Field {
    w: usize,
    h: usize,
    v: Vec<f32>,
}

impl Field {
    fn new(w: usize, h: usize) -> Self {
        Field { w, h, v: vec![0.0; w * h] }
    }
    fn at(&self, x: usize, y: usize) -> f32 {
        self.v[y * self.w + x]
    }
    /// Qutichali xiralash (radius r), `passes` marta — Gauss'ga yaqin natija.
    fn blur(&self, r: usize, passes: usize) -> Field {
        let mut cur = Field { w: self.w, h: self.h, v: self.v.clone() };
        for _ in 0..passes {
            cur = cur.blur_axis(r, true).blur_axis(r, false);
        }
        cur
    }
    fn blur_axis(&self, r: usize, horiz: bool) -> Field {
        let (w, h) = (self.w, self.h);
        let mut out = Field::new(w, h);
        let (len, lines) = if horiz { (w, h) } else { (h, w) };
        let idx = |line: usize, i: usize| if horiz { line * w + i } else { i * w + line };
        for line in 0..lines {
            let mut prefix = vec![0.0f32; len + 1];
            for i in 0..len {
                prefix[i + 1] = prefix[i] + self.v[idx(line, i)];
            }
            for i in 0..len {
                let a = i.saturating_sub(r);
                let b = (i + r + 1).min(len);
                out.v[idx(line, i)] = (prefix[b] - prefix[a]) / (b - a) as f32;
            }
        }
        out
    }
}

/// Fonni chetdan boshlab "to'ldirish". Har bir fon pikseli o'zi kelgan yo'lning
/// sekin moslashuvchi fon rangini (`model`) olib yuradi: silliq gradient (osmon)
/// fonga qo'shiladi, lekin buyumning chegarasi — hatto kontrasti past va xira
/// bo'lsa ham — to'plangan farq `tol` dan oshgani uchun to'xtatadi.
fn flood_background(rgb: &[[f32; 3]], w: usize, h: usize, tol: f32) -> Vec<bool> {
    const ADAPT: f32 = 0.1;
    let mut model: Vec<[f32; 3]> = rgb.to_vec();
    let mut bg = vec![false; w * h];
    // Urug'lar: yuqori chet va yon chetlarning yuqori 70%i. Pastki chetdan boshlanmaydi:
    // haykal (byust, odam, bino) odatda rasmning pastki chetiga tegib turadi.
    // Pastdagi fon (masalan, yer) yon tomonlardan to'ldirilib, baribir fonga qo'shiladi.
    let mut q = VecDeque::new();
    for x in 0..w {
        q.push_back(x);
    }
    for y in 0..(h * 7 / 10).max(1) {
        for x in [0, w - 1] {
            q.push_back(y * w + x);
        }
    }
    for &i in &q {
        bg[i] = true;
    }
    while let Some(i) = q.pop_front() {
        let (x, y) = (i % w, i / w);
        let m = model[i];
        let mut visit = |j: usize| {
            if !bg[j] {
                let d = (0..3).map(|k| (m[k] - rgb[j][k]).abs()).fold(0.0, f32::max);
                if d <= tol {
                    bg[j] = true;
                    model[j] = [0, 1, 2].map(|k| m[k] + (rgb[j][k] - m[k]) * ADAPT);
                    q.push_back(j);
                }
            }
        };
        if x > 0 {
            visit(i - 1);
        }
        if x + 1 < w {
            visit(i + 1);
        }
        if y > 0 {
            visit(i - w);
        }
        if y + 1 < h {
            visit(i + w);
        }
    }
    bg
}

/// Eng katta bog'langan bo'lakni qoldiradi; `fill_holes` bo'lsa — ichidagi teshiklarni to'ldiradi.
fn largest_component(mask: &[bool], w: usize, h: usize, fill_holes: bool) -> Vec<bool> {
    let mut label = vec![0u32; w * h];
    let (mut best, mut best_size, mut next) = (0u32, 0usize, 0u32);
    for start in 0..w * h {
        if !mask[start] || label[start] != 0 {
            continue;
        }
        next += 1;
        let mut size = 0;
        let mut q = vec![start];
        label[start] = next;
        while let Some(i) = q.pop() {
            size += 1;
            let (x, y) = (i % w, i / w);
            let ns = [
                (x > 0).then(|| i - 1),
                (x + 1 < w).then(|| i + 1),
                (y > 0).then(|| i - w),
                (y + 1 < h).then(|| i + w),
            ];
            for j in ns.into_iter().flatten() {
                if mask[j] && label[j] == 0 {
                    label[j] = next;
                    q.push(j);
                }
            }
        }
        if size > best_size {
            best_size = size;
            best = next;
        }
    }
    let comp: Vec<bool> = label.iter().map(|&l| l == best && best != 0).collect();
    if !fill_holes {
        return comp;
    }
    // teshiklarni to'ldirish: chetdan yetib bo'lmaydigan bo'shliqlar ham haykalga tegishli
    let mut outside = vec![false; w * h];
    let mut q: Vec<usize> = Vec::new();
    for x in 0..w {
        q.push(x);
        q.push((h - 1) * w + x);
    }
    for y in 0..h {
        q.push(y * w);
        q.push(y * w + w - 1);
    }
    q.retain(|&i| !comp[i]);
    for &i in &q {
        outside[i] = true;
    }
    while let Some(i) = q.pop() {
        let (x, y) = (i % w, i / w);
        let ns = [
            (x > 0).then(|| i - 1),
            (x + 1 < w).then(|| i + 1),
            (y > 0).then(|| i - w),
            (y + 1 < h).then(|| i + w),
        ];
        for j in ns.into_iter().flatten() {
            if !comp[j] && !outside[j] {
                outside[j] = true;
                q.push(j);
            }
        }
    }
    outside.iter().map(|o| !o).collect()
}

/// Shakl chegarasigacha bo'lgan masofa (chamfer 3-4, piksellarda).
fn distance(mask: &[bool], w: usize, h: usize) -> Field {
    const BIG: f32 = 1e6;
    let mut d = Field::new(w, h);
    for i in 0..w * h {
        d.v[i] = if mask[i] { BIG } else { 0.0 };
    }
    let get = |d: &Field, x: isize, y: isize| -> f32 {
        if x < 0 || y < 0 || x >= w as isize || y >= h as isize {
            0.0 // rasm cheti ham chegara
        } else {
            d.v[y as usize * w + x as usize]
        }
    };
    for y in 0..h as isize {
        for x in 0..w as isize {
            let i = y as usize * w + x as usize;
            if d.v[i] == 0.0 {
                continue;
            }
            let m = (get(&d, x - 1, y) + 3.0)
                .min(get(&d, x, y - 1) + 3.0)
                .min(get(&d, x - 1, y - 1) + 4.0)
                .min(get(&d, x + 1, y - 1) + 4.0);
            d.v[i] = d.v[i].min(m);
        }
    }
    for y in (0..h as isize).rev() {
        for x in (0..w as isize).rev() {
            let i = y as usize * w + x as usize;
            if d.v[i] == 0.0 {
                continue;
            }
            let m = (get(&d, x + 1, y) + 3.0)
                .min(get(&d, x, y + 1) + 3.0)
                .min(get(&d, x + 1, y + 1) + 4.0)
                .min(get(&d, x - 1, y + 1) + 4.0);
            d.v[i] = d.v[i].min(m);
        }
    }
    for v in &mut d.v {
        *v /= 3.0;
    }
    d
}

/// Gumbazsimon tepali lavha (stela) shakli.
fn stele_mask(w: usize, h: usize) -> Vec<bool> {
    let ry = (h as f32 * 0.3).min(w as f32 * 0.5).max(1.0);
    let cx = w as f32 / 2.0;
    let mut m = vec![false; w * h];
    for y in 0..h {
        let half = if (y as f32) < ry {
            let t = (ry - y as f32) / ry;
            cx * (1.0 - t * t).max(0.0).sqrt()
        } else {
            cx
        };
        for x in 0..w {
            m[y * w + x] = (x as f32 + 0.5 - cx).abs() <= half;
        }
    }
    m
}

fn percentile(vals: &mut [f32], p: f32) -> f32 {
    if vals.is_empty() {
        return 0.0;
    }
    let k = ((vals.len() - 1) as f32 * p) as usize;
    *vals.select_nth_unstable_by(k, |a, b| a.total_cmp(b)).1
}

/// Haykal shakli va hajmi (piksellarda), kesilgan o'lchamlarda.
struct Relief {
    w: usize,
    h: usize,
    /// shakl (0..1)
    mask: Field,
    /// old sirt balandligi (px): yumaloq hajm + mayda detallar
    front: Field,
    /// orqa sirt chuqurligi (px): faqat yumaloq hajm
    back: Field,
    /// 3D uchun me'yor: balandlik / `scale` -> 0..1
    scale: f32,
    /// Asl rasmning me'yorlangan yorqinligi (yuz, ko'z, soch detallari)
    lum: Field,
}

/// Obyektni fondan ajratish usuli.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Segmenter {
    /// neyron tarmoq (fotosuratlar uchun), ishlamasa — rang usuli
    Model,
    /// faqat rang usuli (testlar uchun — natija aniq oldindan ma'lum)
    #[cfg_attr(not(test), allow(dead_code))]
    Colors,
}

/// Maydonni boshqa o'lchamga o'tkazadi (bilinear interpolyatsiya, qiymatlarni 0..1 bilan cheklamaydi).
fn resize_field(f: &Field, nw: usize, nh: usize) -> Field {
    if f.w == nw && f.h == nh {
        return Field { w: nw, h: nh, v: f.v.clone() };
    }
    let mut out = Field::new(nw, nh);
    let sx = f.w as f32 / nw as f32;
    let sy = f.h as f32 / nh as f32;
    for y in 0..nh {
        let src_y = (y as f32 + 0.5) * sy - 0.5;
        let y0 = (src_y.floor() as isize).clamp(0, f.h as isize - 1) as usize;
        let y1 = (y0 + 1).min(f.h - 1);
        let fy = (src_y - y0 as f32).clamp(0.0, 1.0);
        for x in 0..nw {
            let src_x = (x as f32 + 0.5) * sx - 0.5;
            let x0 = (src_x.floor() as isize).clamp(0, f.w as isize - 1) as usize;
            let x1 = (x0 + 1).min(f.w - 1);
            let fx = (src_x - x0 as f32).clamp(0.0, 1.0);

            let v00 = f.v[y0 * f.w + x0];
            let v10 = f.v[y0 * f.w + x1];
            let v01 = f.v[y1 * f.w + x0];
            let v11 = f.v[y1 * f.w + x1];

            let top = v00 + (v10 - v00) * fx;
            let bottom = v01 + (v11 - v01) * fx;
            out.v[y * nw + x] = top + (bottom - top) * fy;
        }
    }
    out
}

/// Siluetni "puflash": ∇²u = −2 (chegarada u = 0) tenglamasini yechib, √u olinadi.
/// Natijada har bir bo'lak o'z eniga mos yumaloq kesimga ega bo'ladi: qo'l va bo'yin —
/// ingichka silindr, tana — keng va qalin, bosh — sharsimon. Haqiqiy haykal hajmi shunday.
///
/// `open` — rasm cheti kesib o'tgan tomonlar (chap, yuqori, o'ng, past): u yerda shakl
/// davom etadi deb hisoblanadi (masalan, tizzagacha olingan suratda tana pastda "uzilmaydi").
fn inflate(mask: &Field, open: [bool; 4]) -> Field {
    const GRID: usize = 180; // kattaroq grid — aniqroq hajm (ayniqsa yuz, qo'l uchun)
    let (w, h) = (mask.w, mask.h);
    let s = (GRID as f32 / w.max(h) as f32).min(1.0);
    let (gw, gh) = (((w as f32 * s).round() as usize).max(3), ((h as f32 * s).round() as usize).max(3));
    let small = resize_field(mask, gw, gh);
    let m: Vec<bool> = small.v.iter().map(|v| *v >= 0.5).collect();
    let mut u = vec![0.0f32; gw * gh];
    let omega = 2.0 / (1.0 + (std::f32::consts::PI / gw.max(gh) as f32).sin());
    for _ in 0..900 { // ko'proq iteratsiya — aniqroq yumaloq profil
        let mut change = 0.0f32;
        for y in 0..gh {
            for x in 0..gw {
                let i = y * gw + x;
                if !m[i] {
                    continue;
                }
                let (mut sum, mut neumann) = (0.0f32, 0.0f32);
                for (inside, j, side) in [
                    (x > 0, i.wrapping_sub(1), 0),
                    (y > 0, i.wrapping_sub(gw), 1),
                    (x + 1 < gw, i + 1, 2),
                    (y + 1 < gh, i + gw, 3),
                ] {
                    if inside {
                        if m[j] {
                            sum += u[j];
                        }
                    } else if open[side] {
                        neumann += 1.0;
                    }
                }
                let target = (sum + 2.0) / (4.0 - neumann).max(1.0);
                let d = omega * (target - u[i]);
                u[i] += d;
                change = change.max(d.abs());
            }
        }
        if change < 1e-3 {
            break;
        }
    }
    let root = Field { w: gw, h: gh, v: u.iter().map(|v| v.max(0.0).sqrt() / s).collect() };
    let mut out = resize_field(&root, w, h);
    for (v, m) in out.v.iter_mut().zip(&mask.v) {
        *v *= m.clamp(0.0, 1.0);
    }
    out
}

fn build_relief(src: &image::DynamicImage, segmenter: Segmenter) -> Relief {
    let img = src.resize(WORK, WORK, FilterType::Triangle).to_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let (w, h) = (w.max(2), h.max(2));
    let px = |x: usize, y: usize| img.get_pixel(x.min(img.width() as usize - 1) as u32, y.min(img.height() as usize - 1) as u32);

    // ozgina xiralangan rang (JPEG shovqini fonni "uzib" qo'ymasligi uchun)
    let mut chans: Vec<Field> = (0..3).map(|_| Field::new(w, h)).collect();
    let mut alpha = Field::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let p = px(x, y);
            for (k, ch) in chans.iter_mut().enumerate() {
                ch.v[y * w + x] = p[k] as f32;
            }
            alpha.v[y * w + x] = p[3] as f32 / 255.0;
        }
    }
    let chans: Vec<Field> = chans.iter().map(|c| c.blur(1, 1)).collect();
    let rgb: Vec<[f32; 3]> = (0..w * h).map(|i| [chans[0].v[i], chans[1].v[i], chans[2].v[i]]).collect();
    let lum = Field {
        w,
        h,
        v: rgb.iter().map(|c| (0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]) / 255.0).collect(),
    };

    // 1) shakl: shaffof PNG -> alfa; aks holda neyron tarmoq; u bo'lmasa -> rang usuli
    let transparent = alpha.v.iter().filter(|a| **a < 0.5).count();
    let model_prob = if transparent > w * h / 50 || segmenter != Segmenter::Model {
        None
    } else {
        crate::segment::foreground(&img).filter(|p| p.len() == w * h)
    };
    let by_model = model_prob.is_some();
    let raw: Vec<bool> = if transparent > w * h / 50 {
        alpha.v.iter().map(|a| *a >= 0.5).collect()
    } else if let Some(prob) = &model_prob {
        prob.iter().map(|p| *p >= 0.5).collect()
    } else {
        flood_background(&rgb, w, h, 12.0).iter().map(|b| !b).collect()
    };
    // mayda dog'lar va tishli chetlarni silliqlash
    let soft = Field { w, h, v: raw.iter().map(|b| if *b { 1.0 } else { 0.0 }).collect() }.blur(2, 2);
    let smooth: Vec<bool> = soft.v.iter().map(|v| *v >= 0.5).collect();
    // model/alfa usulida qo'l va tana orasidagi bo'shliqlar haqiqiy — ular to'ldirilmaydi
    let comp = largest_component(&smooth, w, h, !by_model && transparent <= w * h / 50);

    let area = comp.iter().filter(|b| **b).count();
    let perimeter = (0..w * h)
        .filter(|&i| {
            let (x, y) = (i % w, i / w);
            comp[i] && (x == 0 || y == 0 || x + 1 == w || y + 1 == h || !comp[i - 1] || !comp[i + 1] || !comp[i - w] || !comp[i + w])
        })
        .count();
    let frac = area as f32 / (w * h) as f32;
    let (mut bx0, mut by0, mut bx1, mut by1) = (w, h, 0, 0);
    for i in (0..w * h).filter(|&i| comp[i]) {
        let (x, y) = (i % w, i / w);
        bx0 = bx0.min(x);
        by0 = by0.min(y);
        bx1 = bx1.max(x + 1);
        by1 = by1.max(y + 1);
    }
    let (bw, bh) = (bx1.saturating_sub(bx0) as f32, by1.saturating_sub(by0) as f32);
    // juda yassi (faqat yer bo'lagi qolgan) yoki juda ingichka shakl haykalga yaramaydi
    let odd_shape = bw > 2.6 * bh || bh > 4.0 * bw;
    let stele = if by_model {
        area == 0 || !(0.02..=0.97).contains(&frac) || odd_shape
    } else {
        let ragged = area == 0 || perimeter as f32 / (area as f32).sqrt() > 16.0;
        !(0.06..=0.9).contains(&frac) || ragged || odd_shape
    };

    // 2) kesish
    let (mask, x0, y0, x1, y1) = if stele { (stele_mask(w, h), 0, 0, w, h) } else { (comp, bx0, by0, bx1, by1) };
    let (cw, ch) = (x1 - x0, y1 - y0);
    let crop = |f: &dyn Fn(usize) -> f32| -> Field {
        let mut out = Field::new(cw, ch);
        for y in 0..ch {
            for x in 0..cw {
                out.v[y * cw + x] = f((y + y0) * w + x + x0);
            }
        }
        out
    };
    let m = crop(&|i| if mask[i] { 1.0 } else { 0.0 });
    let l = crop(&|i| lum.v[i]);
    let mb: Vec<bool> = m.v.iter().map(|v| *v > 0.5).collect();
    let dist = distance(&mb, cw, ch).blur(1, 1);
    let min_side = cw.min(ch) as f32;
    let scale = 0.6 * min_side;

    // Yorqinlik kontrastini obyekt ichida to'liq dinamik diapazonga yoyish
    let mut inside_lum: Vec<f32> = (0..cw * ch).filter(|&i| mb[i]).map(|i| l.v[i]).collect();
    let l_lo = percentile(&mut inside_lum, 0.02);
    let l_hi = percentile(&mut inside_lum, 0.98).max(l_lo + 0.05);
    let mut l_norm = Field::new(cw, ch);
    for i in 0..cw * ch {
        l_norm.v[i] = ((l.v[i] - l_lo) / (l_hi - l_lo)).clamp(0.0, 1.0);
    }

    // 3) Ko'p bosqichli relyef o'ymakorligi (Multi-scale relief):
    //    a) Anatomik o'rta hajm (yuz suyaklari, burun ko'prigi, peshona, mato burmalari)
    let r_mid = ((min_side / 28.0) as usize).clamp(3, 10);
    let r_broad = ((min_side / 7.5) as usize).clamp(12, 32);
    let l_mid = l_norm.blur(r_mid, 1);
    let l_broad = l_norm.blur(r_broad, 2);

    //    b) Aniq mayda detallar (ko'z qorachig'i, qovoqlar, burun teshigi, lablar, soch tolalari)
    let r_fine = ((min_side / 90.0) as usize).clamp(1, 3);
    let l_fine = l_norm.blur(r_fine, 1);

    //    c) Juda nozik detallar (teri fakturasi, yuzdagi mayda chiziqlar)
    let r_micro = 1usize;
    let l_micro = l_norm.blur(r_micro, 1);

    let mut relief_detail = Field::new(cw, ch);
    for i in 0..cw * ch {
        let mid = (l_mid.v[i] - l_broad.v[i]).clamp(-0.40, 0.40);
        let fine = (l_norm.v[i] - l_fine.v[i]).clamp(-0.40, 0.40);
        let micro = (l_norm.v[i] - l_micro.v[i]).clamp(-0.25, 0.25);
        // O'ymakorlik: qorong'i joylar (ko'z, burun, lab chizig'i) chuqurroq o'yiladi,
        // yorug' joylar (peshona, burun uchi, bo'rtiqlar) haykalda aniq bo'rtadi.
        // Micro detal teri fakturasini beradi.
        relief_detail.v[i] = mid * (0.085 * min_side) + fine * (0.075 * min_side) + micro * (0.025 * min_side);
    }

    let mut front = Field::new(cw, ch);
    let mut back = Field::new(cw, ch);
    if stele {
        // lavha: qirrasi qiyalangan tekis tosh + ichiga o'yilgan chuqur relyef
        let margin = min_side * 0.07;
        for i in (0..cw * ch).filter(|&i| mb[i]) {
            let d = dist.v[i];
            let plate = (d / 4.0).min(1.0) * 0.08 * min_side;
            let inner = ((d - margin) / 4.0).clamp(0.0, 1.0);
            let ln = (l_norm.v[i] - 0.5) * 0.12 * min_side;
            front.v[i] = plate + inner * (ln + relief_detail.v[i] * 0.85);
            back.v[i] = plate;
        }
    } else {
        let open = [x0 == 0, y0 == 0, x1 == w, y1 == h];
        let vol = inflate(&m, open);
        for i in (0..cw * ch).filter(|&i| mb[i]) {
            let edge = (dist.v[i] / 3.5).min(1.0); // chetga yaqin detal silliq so'nadi (aniqroq chegaralar)
            front.v[i] = vol.v[i] * 0.75 + relief_detail.v[i] * edge; // kuchaytirilgan hajm
            back.v[i] = vol.v[i] * 0.72;  // orqa tomondagi chuqurlik
        }
    }
    // Old sirtning nozik detallari xiralashmasligi uchun front ustida blur qilinmaydi.
    // Orqa sirt faqat ozgina silliqlanadi (hajm aniq saqlanadi).
    Relief { w: cw, h: ch, mask: m, front, back: back.blur(1, 1), scale, lum: l_norm }
}

/// Marmar tomirlari (protsedura).
fn vein(x: f32, y: f32) -> f32 {
    let t = x * 0.045 + (y * 0.031).sin() * 2.6 + ((x + y) * 0.012).sin() * 3.1 + (x * 0.11 - y * 0.07).sin() * 0.35;
    let v = t.sin().abs();
    if v < 0.07 {
        0.88 + v * 1.7
    } else {
        1.0
    }
}

/// Yoritilgan marmar haykal "surati" (muzey fotosuratiga o'xshash).
fn render_preview(r: &Relief) -> RgbImage {
    let (w, h) = (r.w, r.h);
    let (ow, oh) = (w, h);
    let soft = r.mask.blur(1, 1); // silliq chet

    // Ko'p masshtabli chuqurchalar soyasi (Ambient Occlusion):
    // 1) Mayda chuqurchalar (ko'z, qovoq, burun teshigi, lab ajralishi, soch tolalari)
    let around_fine = r.front.blur(2, 1);
    // 2) Keng chuqurchalar (iyak osti, bo'yin, qovurg'a va chuqur burmalar)
    let around_broad = r.front.blur((w.min(h) / 22).max(4), 2);

    let norm = |x: f32, y: f32, z: f32| {
        let n = (x * x + y * y + z * z).sqrt();
        (x / n, y / n, z / n)
    };
    let light = norm(-0.48, -0.58, 0.66); // asosiy chiroq (yuqori chap, yaqinroq — odamshaklni yaxshi yoritadi)
    let fill = norm(0.65, -0.15, 0.75);  // to'ldiruvchi nur (o'ng)
    let rim = norm(0.20, 0.78, -0.58);   // orqa chekka nuri (kontur)
    let half = norm(light.0, light.1, light.2 + 1.0);

    let hget = |x: isize, y: isize| -> f32 {
        let x = x.clamp(0, w as isize - 1) as usize;
        let y = y.clamp(0, h as isize - 1) as usize;
        r.front.at(x, y)
    };
    let ao_scale = (0.025 * w.min(h) as f32).max(1.0);

    ImageBuffer::from_fn(ow as u32, oh as u32, |ox, oy| {
        let (x, y) = (ox as isize, oy as isize);
        let a = soft.at(x as usize, y as usize);
        let bg = [22.0, 24.0, 30.0];
        if a <= 0.01 {
            return Rgb(bg.map(|c| c as u8));
        }

        let h_cur = r.front.at(x as usize, y as usize);
        let dx = (hget(x + 1, y) - hget(x - 1, y)) * 0.5;
        let dy = (hget(x, y + 1) - hget(x, y - 1)) * 0.5;

        // Nisbiy qiyalik: yuz va kiyim detallari nurni aniq tutsin deb qiyalik kuchaytirilgan
        let n = norm(-dx * 2.2, -dy * 2.2, 1.0);  // kuchaytirilgan — aniqroq detallar
        let dot = |l: (f32, f32, f32)| (n.0 * l.0 + n.1 * l.1 + n.2 * l.2).max(0.0);

        // Mayda AO (chiziqlar qorayadi)
        let cav_fine = h_cur - around_fine.at(x as usize, y as usize);
        let ao_fine = (1.0 + cav_fine * 0.42).clamp(0.35, 1.08);  // kuchliroq AO

        // Keng AO
        let cav_broad = h_cur - around_broad.at(x as usize, y as usize);
        let ao_broad = (1.0 + cav_broad / ao_scale * 0.36).clamp(0.50, 1.12);  // kuchliroq AO

        let ao = ao_fine * ao_broad;

        // Asl rasmning fotografik yuz detallari (ko'z qorachig'i, qosh, lab, soch va kiyim fakturasi)
        // Marmar qirralari bilan uyg'unlashib, odam yuzini 3D da tiniq va darhol taniladigan qiladi
        let lum = r.lum.at(x as usize, y as usize);
        let photo_tone = 0.50 + 0.50 * lum;  // kuchaytirilgan kontrast — detallar aniqroq

        let diff = (0.20 + 0.72 * dot(light) + 0.20 * dot(fill)) * ao;  // kuchaytirilgan yoritish
        let rim_light = dot(rim).powf(3.5) * 0.18;  // kuchliroq chekka nuri
        let spec = dot(half).powf(28.0) * 72.0 * ao_fine;  // kuchliroq blick

        // Marmar ichki yorug'lik nurlanishi (Subsurface scattering):
        // soya va yorug'lik chegarasida marmar iliq ohang oladi — odamning teri rangiga yaqin issiqlik
        let sss = ((0.35 - dot(light)).max(0.0) / 0.35) * (dot(light) + 0.10) * 48.0 * ao;

        let v = vein(x as f32, y as f32);
        let base = [245.0, 240.0, 232.0]; // ozgina oqroq marmar — odamsimon haykalga mos
        let c = [
            (base[0] * v * photo_tone * (diff + rim_light) + spec + sss).clamp(0.0, 255.0),
            (base[1] * v * (photo_tone * 0.98) * (diff + rim_light) + spec + sss * 0.65).clamp(0.0, 255.0),
            (base[2] * v * (photo_tone * 0.95) * (diff + rim_light) + spec + sss * 0.35).clamp(0.0, 255.0),
        ];
        Rgb([0, 1, 2].map(|i| (c[i] * a + bg[i] * (1.0 - a)) as u8))
    })
}

/// 3D uchun chuqurlik xaritasi (RGBA):
///   R,G — old sirt balandligi (16 bit), B — orqa sirt chuqurligi (8 bit), A — shakl.
/// Balandlik `scale` ga bo'lingan: 3D'da 1.0 = 0.6 × (haykalning kichik tomoni).
fn depth_image(r: &Relief) -> RgbaImage {
    let s = (DEPTH_MAX as f32 / r.w.max(r.h) as f32).min(1.0);
    let (nw, nh) = (((r.w as f32 * s).round() as usize).max(2), ((r.h as f32 * s).round() as usize).max(2));
    let (mask, front, back) = (resize_field(&r.mask, nw, nh), resize_field(&r.front, nw, nh), resize_field(&r.back, nw, nh));
    // kichraytirishdan keyin ham bitta yaxlit bo'lak qolsin (3D'da uchib yurgan bo'laklar bo'lmasin)
    let solid: Vec<bool> = mask.v.iter().map(|v| *v >= 0.5).collect();
    let keep = largest_component(&solid, nw, nh, false);
    ImageBuffer::from_fn(nw as u32, nh as u32, |x, y| {
        let i = y as usize * nw + x as usize;
        let f = ((front.v[i] / r.scale).clamp(0.0, 1.0) * 65535.0) as u16;
        let b = ((back.v[i] / r.scale).clamp(0.0, 1.0) * 255.0) as u8;
        image::Rgba([(f >> 8) as u8, (f & 0xff) as u8, b, if keep[i] { 255 } else { 0 }])
    })
}

/// Asl rasmdan haykal fayllarini yaratadi. Natija: (surat, chuqurlik) nisbiy yo'llari.
pub fn generate(media: &Path, image_rel: &str) -> anyhow::Result<(String, String)> {
    let src = image::open(media.join(image_rel)).with_context(|| format!("rasmni ochib bo'lmadi: {image_rel}"))?;
    let relief = build_relief(&src, Segmenter::Model);
    let (dir, stem) = {
        let p = Path::new(image_rel);
        let dir = p.parent().map(|d| d.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "image".into());
        (dir, stem)
    };
    let preview_rel = images::unique_rel_path(media, &dir, &format!("{stem}-sculpture.png"));
    let depth_rel = images::unique_rel_path(media, &dir, &format!("{stem}{DEPTH_SUFFIX}.png"));
    let preview_path: PathBuf = media.join(&preview_rel);
    if let Some(parent) = preview_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    render_preview(&relief).save_with_format(&preview_path, image::ImageFormat::Png)?;
    if let Err(e) = depth_image(&relief).save_with_format(media.join(&depth_rel), image::ImageFormat::Png) {
        images::remove_media(media, &preview_rel);
        return Err(e.into());
    }
    Ok((preview_rel, depth_rel))
}

/// Joriy formatdagi chuqurlik fayllari shu qo'shimcha bilan nomlanadi. Eski formatdagi
/// (oldingi versiyada yasalgan) haykallar ishga tushishda avtomatik qayta yasaladi.
const DEPTH_SUFFIX: &str = "-relief3";

struct Pending {
    id: i64,
    image: String,
    sculpture: String,
    depth: String,
}

/// Haykal zallaridagi, hali haykalga aylantirilmagan (yoki eski formatdagi) asarlar.
fn pending(c: &Connection) -> rusqlite::Result<Vec<Pending>> {
    let mut st = c.prepare(&format!(
        "SELECT p.id, p.image, p.sculpture, p.depth FROM paintings p JOIN sections s ON s.id = p.section_id
         WHERE s.kind = ?1 AND (p.depth = '' OR p.sculpture = '' OR p.depth NOT LIKE '%{DEPTH_SUFFIX}%') ORDER BY p.id"
    ))?;
    let rows = st.query_map([models::KIND_SCULPTURES], |r| {
        Ok(Pending { id: r.get(0)?, image: r.get(1)?, sculpture: r.get(2)?, depth: r.get(3)? })
    })?;
    rows.collect()
}

/// Natijani bazaga yozadi va eski fayllarni o'chiradi. Asar shu orada o'zgargan
/// yoki o'chgan bo'lsa — yangi fayllar o'chiriladi.
fn save(c: &Connection, media: &Path, p: &Pending, files: (String, String)) -> rusqlite::Result<bool> {
    let n = c.execute(
        "UPDATE paintings SET sculpture = ?1, depth = ?2
         WHERE id = ?3 AND image = ?4 AND sculpture = ?5 AND depth = ?6",
        params![files.0, files.1, p.id, p.image, p.sculpture, p.depth],
    )?;
    let stale = if n == 0 { [&files.0, &files.1] } else { [&p.sculpture, &p.depth] };
    for f in stale {
        images::remove_media(media, f);
    }
    Ok(n > 0)
}

fn run_pending(db: &Db, media: &Path, list: Vec<Pending>) -> usize {
    let mut done = 0;
    for p in list {
        match generate(media, &p.image) {
            Ok(files) => {
                if db.with(|c| save(c, media, &p, files)).unwrap_or(false) {
                    done += 1;
                }
            }
            Err(e) => tracing::warn!("haykal yasab bo'lmadi (asar {}): {e:#}", p.id),
        }
    }
    done
}

/// Sinxron (CLI va ishga tushish paytida): barcha kutilayotgan haykallarni yasaydi.
pub fn ensure_sync(db: &Db, media: &Path) -> usize {
    let list = db.with(|c| pending(c)).unwrap_or_default();
    run_pending(db, media, list)
}

/// So'rov ichida: baza bloklanmaydi — rasm alohida oqimda qayta ishlanadi.
pub async fn ensure(db: &Db, media: &Path) {
    let (db, media) = (db.clone(), media.to_path_buf());
    let _ = tokio::task::spawn_blocking(move || ensure_sync(&db, &media)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disk(w: u32, h: u32, r: f32) -> image::DynamicImage {
        image::DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
            let (dx, dy) = (x as f32 - w as f32 / 2.0, y as f32 - h as f32 / 2.0);
            if dx * dx + dy * dy < r * r {
                Rgb([200, 40, 40])
            } else {
                Rgb([240, 230, 200])
            }
        }))
    }

    #[test]
    fn separates_object_from_plain_background() {
        // bir rangli fonda doira -> doira shaklidagi haykal (stela emas)
        let r = build_relief(&disk(200, 160, 50.0), Segmenter::Colors);
        // rasm 560 pikselga kattalashtiriladi (x2.8): doira diametri 100 -> ~280, kesish shunga teng
        assert!((260..=300).contains(&r.w) && (260..=300).contains(&r.h), "{}x{}", r.w, r.h);
        assert!(r.mask.at(0, 0) < 0.5); // burchak — fon
    }

    #[test]
    fn inflated_volume_is_round() {
        // doiraning hajmi markazda eng qalin va chetga qarab yumaloq kamayadi (sharsimon)
        let r = build_relief(&disk(200, 160, 50.0), Segmenter::Colors);
        let (cx, cy) = (r.w / 2, r.h / 2);
        let center = r.back.at(cx, cy);
        let mid = r.back.at(cx + r.w / 4, cy);
        let edge = r.back.at(cx + r.w / 2 - 6, cy);
        let radius = r.w as f32 / 2.0;
        assert!(center > radius * 0.4, "markaz qalinligi: {center} (radius {radius})");
        assert!(center > mid && mid > edge, "{center} > {mid} > {edge}");
        // yumaloq profil: yarim radiusda qalinlik markazdagining ~87% i (aylana: √0.75)
        assert!((0.75..0.97).contains(&(mid / center)), "{}", mid / center);
    }

    #[test]
    fn object_cut_by_bottom_edge_keeps_thickness() {
        // pastki chetga tegib turgan to'rtburchak (masalan, belgacha surat): pastda ham qalin
        let img = image::DynamicImage::ImageRgb8(RgbImage::from_fn(200, 200, |x, y| {
            if (60..140).contains(&x) && y >= 50 {
                Rgb([60, 60, 160])
            } else {
                Rgb([235, 235, 225])
            }
        }));
        let r = build_relief(&img, Segmenter::Colors);
        let top = r.back.at(r.w / 2, r.h / 10);
        let bottom = r.back.at(r.w / 2, r.h - 1);
        assert!(bottom > top * 1.3 && bottom > r.w as f32 * 0.3, "past {bottom}, tepa {top}");
    }

    #[test]
    fn noisy_photo_becomes_stele() {
        let img = RgbImage::from_fn(120, 90, |x, y| {
            let v = ((x * 7919 + y * 104729) % 255) as u8;
            Rgb([v, v.wrapping_mul(3), v.wrapping_mul(7)])
        });
        let r = build_relief(&image::DynamicImage::ImageRgb8(img), Segmenter::Colors);
        assert_eq!((r.w, r.h), (560, 420));
        assert!(r.mask.at(r.w / 2, r.h - 1) > 0.5); // pastki qismi to'liq
        assert!(r.mask.at(0, 0) < 0.5); // yuqori burchak — gumbaz tashqarisi
    }

    #[test]
    fn depth_map_encodes_16bit_height_and_mask() {
        let r = build_relief(&disk(200, 160, 50.0), Segmenter::Colors);
        let d = depth_image(&r);
        assert!(d.width() <= DEPTH_MAX && d.height() <= DEPTH_MAX);
        let c = d.get_pixel(d.width() / 2, d.height() / 2);
        assert_eq!(c[3], 255);
        assert!(((c[0] as u32) << 8 | c[1] as u32) > 20000);
        assert_eq!(d.get_pixel(0, 0)[3], 0);
    }

    #[test]
    fn model_finds_object_in_picture() {
        // neyron tarmoq yuklanadi va oddiy rasmdagi obyektni topadi
        let img = disk(320, 240, 70.0).to_rgba8();
        let prob = crate::segment::foreground(&img).expect("model ishlashi kerak");
        assert_eq!(prob.len(), 320 * 240);
        assert!(prob[120 * 320 + 160] > 0.5, "markaz: {}", prob[120 * 320 + 160]);
        assert!(prob[5 * 320 + 5] < 0.5, "burchak: {}", prob[5 * 320 + 5]);
    }
}

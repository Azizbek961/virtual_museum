//! Parollar: Django bilan mos `pbkdf2_sha256$<iter>$<salt>$<hash>` formati
//! (Django bazasidan import qilingan parollar ham ishlaydi) va parol validatorlari.
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::{distributions::Alphanumeric, Rng};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::error::AppError;

pub const PASSWORD_HELP: &str =
    "Kamida 8 ta belgi; faqat raqamlardan iborat yoki juda oddiy parol bo'lmasin.";

pub fn random_string(len: usize) -> String {
    rand::thread_rng().sample_iter(&Alphanumeric).take(len).map(char::from).collect()
}

fn pbkdf2_b64(raw: &str, salt: &str, iterations: u32) -> String {
    let mut out = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(raw.as_bytes(), salt.as_bytes(), iterations, &mut out);
    B64.encode(out)
}

pub fn make_password(raw: &str, iterations: u32) -> String {
    let salt = random_string(22);
    format!("pbkdf2_sha256${iterations}${salt}${}", pbkdf2_b64(raw, &salt, iterations))
}

pub fn check_password(raw: &str, encoded: &str) -> bool {
    let parts: Vec<&str> = encoded.splitn(4, '$').collect();
    if parts.len() != 4 || parts[0] != "pbkdf2_sha256" {
        return false;
    }
    let Ok(iterations) = parts[1].parse::<u32>() else { return false };
    if iterations == 0 {
        return false;
    }
    let computed = pbkdf2_b64(raw, parts[2], iterations);
    computed.len() == parts[3].len() && bool::from(computed.as_bytes().ct_eq(parts[3].as_bytes()))
}

pub async fn hash_password_async(raw: String, iterations: u32) -> Result<String, AppError> {
    Ok(tokio::task::spawn_blocking(move || make_password(&raw, iterations)).await?)
}

pub async fn check_password_async(raw: String, encoded: String) -> bool {
    tokio::task::spawn_blocking(move || check_password(&raw, &encoded)).await.unwrap_or(false)
}

/// Sessiyada saqlanadigan parol "izi": parol o'zgarsa, boshqa qurilmalardagi
/// sessiyalar avtomatik bekor bo'ladi (Django'dagi session auth hash kabi).
pub fn session_hash(password_hash: &str) -> String {
    let digest = Sha256::digest(format!("museum-session:{password_hash}").as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------- validatorlar

const COMMON_PASSWORDS: &[&str] = &[
    "password", "password1", "password12", "password123", "passw0rd", "p@ssw0rd", "12345678",
    "123456789", "1234567890", "87654321", "11111111", "00000000", "12341234", "11223344",
    "qwertyui", "qwerty123", "qwerty12", "qwertyuiop", "1q2w3e4r", "1q2w3e4r5t", "1qaz2wsx",
    "zaq12wsx", "asdfghjk", "asdfasdf", "zxcvbnm1", "iloveyou", "iloveyou1", "sunshine",
    "princess", "football", "baseball", "superman", "batman123", "trustno1", "welcome1",
    "welcome123", "letmein1", "abc12345", "abcd1234", "admin123", "administrator", "changeme",
    "computer", "internet", "starwars", "whatever", "dragon123", "monkey123", "master123",
    "michael1", "jennifer", "jordan23", "liverpool", "chelsea1", "arsenal1", "barcelona",
    "samsung1", "freedom1", "shadow12", "secret123", "qwer1234", "q1w2e3r4", "aa123456",
    "a1234567", "1234qwer", "muzey123", "parol123", "parolparol", "toshkent", "uzbekistan",
    "samarqand", "test1234", "testtest", "user1234", "museum123", "virtualmuseum",
];

/// Django'ning `difflib.SequenceMatcher.ratio()` hisobiga mos o'xshashlik.
fn similarity(a: &[char], b: &[char]) -> f64 {
    fn matches(a: &[char], b: &[char]) -> usize {
        if a.is_empty() || b.is_empty() {
            return 0;
        }
        let (mut bi, mut bj, mut bk) = (0usize, 0usize, 0usize);
        let mut prev = vec![0usize; b.len() + 1];
        for (i, ca) in a.iter().enumerate() {
            let mut cur = vec![0usize; b.len() + 1];
            for (j, cb) in b.iter().enumerate() {
                if ca == cb {
                    cur[j + 1] = prev[j] + 1;
                    if cur[j + 1] > bk {
                        bk = cur[j + 1];
                        bi = i + 1 - bk;
                        bj = j + 1 - bk;
                    }
                }
            }
            prev = cur;
        }
        if bk == 0 {
            return 0;
        }
        bk + matches(&a[..bi], &b[..bj]) + matches(&a[bi + bk..], &b[bj + bk..])
    }
    let total = a.len() + b.len();
    if total == 0 {
        return 1.0;
    }
    2.0 * matches(a, b) as f64 / total as f64
}

/// Parolni tekshiradi. `attrs` — (nomi, qiymati) juftliklari: login, ism, familiya, email.
pub fn validate_password(password: &str, attrs: &[(&str, &str)]) -> Vec<String> {
    let mut errors = Vec::new();
    let lower = password.to_lowercase();

    // 1. Shaxsiy ma'lumotlarga o'xshashlik (max_similarity = 0.7)
    let pw: Vec<char> = lower.chars().collect();
    'outer: for (label, value) in attrs {
        if value.trim().is_empty() {
            continue;
        }
        let value_lower = value.to_lowercase();
        let mut parts: Vec<String> = value_lower
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        parts.push(value_lower.clone());
        for part in parts {
            let part_chars: Vec<char> = part.chars().collect();
            // Django: juda uzun parolni qisqa qiymat bilan solishtirmaymiz
            let (pl, vl) = (pw.len() as f64, part_chars.len() as f64);
            if pl >= 10.0 * vl && vl < 0.7 / 2.0 * pl {
                continue;
            }
            if similarity(&pw, &part_chars) >= 0.7 {
                errors.push(format!("Parol {label} bilan juda o'xshash."));
                break 'outer;
            }
        }
    }
    // 2. Minimal uzunlik
    if password.chars().count() < 8 {
        errors.push("Parol juda qisqa. U kamida 8 ta belgidan iborat bo'lishi kerak.".into());
    }
    // 3. Keng tarqalgan parollar
    if COMMON_PASSWORDS.contains(&lower.trim()) {
        errors.push("Bu parol juda keng tarqalgan.".into());
    }
    // 4. Faqat raqamlar
    if !password.is_empty() && password.chars().all(|c| c.is_ascii_digit()) {
        errors.push("Parol faqat raqamlardan iborat bo'lmasligi kerak.".into());
    }
    errors
}

/// Django `UnicodeUsernameValidator`: harflar, raqamlar va @ . + - _
pub fn valid_username(username: &str) -> bool {
    !username.is_empty()
        && username.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '@' | '.' | '+' | '-'))
}

pub fn valid_email(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else { return false };
    !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !email.chars().any(|c| c.is_whitespace() || c == '<' || c == '>')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn django_compatible_hash() {
        // Python: base64(hashlib.pbkdf2_hmac("sha256", b"S3cret-pass!", b"abcdefghijklmnopqrstuv", 1000))
        let django = "pbkdf2_sha256$1000$abcdefghijklmnopqrstuv$whX0jrZSQPAKwTLbju21w4wVPf+DFr7qG89RzaeHGr8=";
        assert!(check_password("S3cret-pass!", django));

        let h = make_password("secret-pass", 1000);
        assert!(h.starts_with("pbkdf2_sha256$1000$"));
        assert!(check_password("secret-pass", &h));
        assert!(!check_password("wrong", &h));
        assert!(!check_password("x", "!unusable"));
    }

    #[test]
    fn validators() {
        assert!(validate_password("Muzey-2026-pass", &[("login", "newbie")]).is_empty());
        assert!(!validate_password("short", &[]).is_empty());
        assert!(!validate_password("12345678901", &[]).is_empty());
        assert!(!validate_password("password123", &[]).is_empty());
        assert!(!validate_password("alisher2026", &[("login", "alisher2026")]).is_empty());
    }

    #[test]
    fn ratio_matches_difflib() {
        let a: Vec<char> = "abcd".chars().collect();
        let b: Vec<char> = "bcde".chars().collect();
        assert!((similarity(&a, &b) - 0.75).abs() < 1e-9);
    }
}

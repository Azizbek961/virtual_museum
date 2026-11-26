//! Oddiy forma tizimi (Django forms o'rniga): maydonlar ro'yxati, POST'dan
//! bog'lash, asosiy tekshiruvlar va xatolar. `accounts/_fields.html` shabloni
//! istalgan formani bir xil ko'rinishda chizadi.
use serde::Serialize;

use crate::{auth, session::FormData};

pub const REQUIRED: &str = "Bu maydon to'ldirilishi shart.";

#[derive(Serialize, Clone, Debug, Default)]
pub struct Opt {
    pub value: String,
    pub label: String,
}

impl Opt {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Opt { value: value.into(), label: label.into() }
    }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct CurrentFile {
    pub url: String,
    pub name: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Field {
    pub name: String,
    pub label: String,
    /// text, password, email, number, color, textarea, select, checkbox, file
    pub kind: &'static str,
    pub value: String,
    pub checked: bool,
    pub options: Vec<Opt>,
    pub errors: Vec<String>,
    pub help: String,
    pub required: bool,
    pub disabled: bool,
    pub autofocus: bool,
    pub rows: u32,
    pub accept: String,
    pub autocomplete: String,
    pub max: Option<usize>,
    pub current: Option<CurrentFile>,
    pub clearable: bool,
}

impl Field {
    fn new(name: &str, label: &str, kind: &'static str) -> Self {
        Field { name: name.into(), label: label.into(), kind, ..Default::default() }
    }
    pub fn text(name: &str, label: &str) -> Self {
        Self::new(name, label, "text")
    }
    pub fn password(name: &str, label: &str) -> Self {
        Self::new(name, label, "password")
    }
    pub fn email(name: &str, label: &str) -> Self {
        Self::new(name, label, "email")
    }
    pub fn number(name: &str, label: &str) -> Self {
        Self::new(name, label, "number")
    }
    pub fn color(name: &str, label: &str) -> Self {
        Self::new(name, label, "color")
    }
    pub fn textarea(name: &str, label: &str, rows: u32) -> Self {
        Field { rows, ..Self::new(name, label, "textarea") }
    }
    pub fn select(name: &str, label: &str, options: Vec<Opt>) -> Self {
        Field { options, ..Self::new(name, label, "select") }
    }
    pub fn checkbox(name: &str, label: &str) -> Self {
        Self::new(name, label, "checkbox")
    }
    pub fn file(name: &str, label: &str, accept: &str) -> Self {
        Field { accept: accept.into(), ..Self::new(name, label, "file") }
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }
    pub fn max(mut self, n: usize) -> Self {
        self.max = Some(n);
        self
    }
    pub fn help(mut self, h: &str) -> Self {
        self.help = h.into();
        self
    }
    pub fn value(mut self, v: impl Into<String>) -> Self {
        self.value = v.into();
        self
    }
    pub fn checked(mut self, b: bool) -> Self {
        self.checked = b;
        self
    }
    pub fn disabled(mut self, b: bool) -> Self {
        self.disabled = b;
        self
    }
    pub fn autofocus(mut self) -> Self {
        self.autofocus = true;
        self
    }
    pub fn autocomplete(mut self, a: &str) -> Self {
        self.autocomplete = a.into();
        self
    }
    pub fn current(mut self, url: String, name: String) -> Self {
        self.current = Some(CurrentFile { url, name });
        self
    }
    pub fn clearable(mut self) -> Self {
        self.clearable = true;
        self
    }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Form {
    pub fields: Vec<Field>,
    pub non_field_errors: Vec<String>,
}

impl Form {
    pub fn new(fields: Vec<Field>) -> Self {
        Form { fields, non_field_errors: vec![] }
    }

    fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }

    fn field_mut(&mut self, name: &str) -> Option<&mut Field> {
        self.fields.iter_mut().find(|f| f.name == name)
    }

    pub fn val(&self, name: &str) -> &str {
        self.field(name).map(|f| f.value.as_str()).unwrap_or("")
    }

    pub fn is_checked(&self, name: &str) -> bool {
        self.field(name).map(|f| f.checked).unwrap_or(false)
    }

    pub fn add_error(&mut self, name: &str, msg: impl Into<String>) {
        let msg = msg.into();
        match self.field_mut(name) {
            Some(f) => f.errors.push(msg),
            None => self.non_field_errors.push(msg),
        }
    }

    pub fn add_non_field_error(&mut self, msg: impl Into<String>) {
        self.non_field_errors.push(msg.into());
    }

    pub fn has_errors(&self) -> bool {
        !self.non_field_errors.is_empty() || self.fields.iter().any(|f| !f.errors.is_empty())
    }

    pub fn field_has_error(&self, name: &str) -> bool {
        self.field(name).map(|f| !f.errors.is_empty()).unwrap_or(false)
    }

    /// POST ma'lumotlarini bog'laydi va umumiy tekshiruvlarni bajaradi.
    pub fn bind(&mut self, data: &FormData) {
        for f in &mut self.fields {
            if f.disabled {
                continue; // o'chirilgan maydon qiymati POST'dan olinmaydi
            }
            match f.kind {
                "checkbox" => {
                    f.checked = data.has(&f.name);
                    continue;
                }
                "file" => {
                    if f.required && !data.files.contains_key(&f.name) && f.current.is_none() {
                        f.errors.push(REQUIRED.into());
                    }
                    continue;
                }
                "password" => f.value = data.get(&f.name).to_string(),
                _ => f.value = data.get(&f.name).trim().to_string(),
            }
            if f.value.is_empty() {
                if f.required {
                    f.errors.push(REQUIRED.into());
                }
                continue;
            }
            if let Some(max) = f.max {
                let len = f.value.chars().count();
                if len > max {
                    f.errors.push(format!(
                        "Bu qiymat ko'pi bilan {max} ta belgidan iborat bo'lishi kerak (hozir {len} ta)."
                    ));
                }
            }
            match f.kind {
                "email" if !auth::valid_email(&f.value) => f.errors.push("To'g'ri email manzil kiriting.".into()),
                "number" if f.value.parse::<i64>().is_err() => f.errors.push("Butun son kiriting.".into()),
                "select" if !f.options.iter().any(|o| !o.value.is_empty() && o.value == f.value) => {
                    f.errors.push("To'g'ri variantni tanlang. Bu variant mavjud emas.".into())
                }
                _ => {}
            }
        }
    }
}

/// Ikki marta kiritilgan parolni solishtiradi va validatorlardan o'tkazadi.
pub fn check_password_pair(form: &mut Form, f1: &str, f2: &str, error_field: &str, attrs: &[(&str, &str)]) {
    let p1 = form.val(f1).to_string();
    let p2 = form.val(f2).to_string();
    if p1.is_empty() || p2.is_empty() {
        return;
    }
    if p1 != p2 {
        form.add_error(f2, "Parollar mos kelmadi.");
        return;
    }
    for e in auth::validate_password(&p1, attrs) {
        form.add_error(error_field, e);
    }
}

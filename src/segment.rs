//! Rasmdagi asosiy obyektni (odam, hayvon, buyum, bino) fondan ajratish.
//!
//! U²-Net (u2netp, 4.6 MB, Apache-2.0) neyron tarmog'i ishlatiladi. Model fayli
//! (`models/u2netp.onnx`) dastur ichiga joylangan — internet yoki alohida DLL kerak emas.
//! Hisoblash sof Rust'dagi `tract` kutubxonasida bajariladi (CPU, ~0.5–2 soniya).
use std::{
    io::Cursor,
    sync::{Arc, OnceLock},
};

use image::{imageops::FilterType, ImageBuffer, Luma, RgbaImage};
use tract_onnx::prelude::*;

static MODEL_BYTES: &[u8] = include_bytes!("../models/u2netp.onnx");
const SIZE: usize = 320;

type Model = Arc<TypedRunnableModel>;
static MODEL: OnceLock<Option<Model>> = OnceLock::new();

fn load() -> TractResult<Model> {
    tract_onnx::onnx()
        .model_for_read(&mut Cursor::new(MODEL_BYTES))?
        .with_input_fact(0, f32::fact([1, 3, SIZE, SIZE]).into())?
        .into_optimized()?
        .into_runnable()
}

/// Model bir marta yuklanadi va keyingi so'rovlarda qayta ishlatiladi.
fn model() -> Option<&'static Model> {
    MODEL
        .get_or_init(|| match load() {
            Ok(m) => Some(m),
            Err(e) => {
                tracing::warn!("segmentatsiya modelini yuklab bo'lmadi: {e:#}");
                None
            }
        })
        .as_ref()
}

/// Har bir piksel uchun "asosiy obyekt" ehtimolligi (0..1), rasm o'lchamida.
/// Model ishlamasa — `None` (unda oddiy rang usuli ishlatiladi).
pub fn foreground(img: &RgbaImage) -> Option<Vec<f32>> {
    let model = model()?;
    let small = image::imageops::resize(img, SIZE as u32, SIZE as u32, FilterType::Triangle);
    let max = small.pixels().flat_map(|p| [p[0], p[1], p[2]]).max().unwrap_or(1).max(1) as f32;
    const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
    const STD: [f32; 3] = [0.229, 0.224, 0.225];
    let input: Tensor = tract_ndarray::Array4::from_shape_fn((1, 3, SIZE, SIZE), |(_, c, y, x)| {
        (small.get_pixel(x as u32, y as u32)[c] as f32 / max - MEAN[c]) / STD[c]
    })
    .into();
    let out = match model.run(tvec!(input.into())) {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("segmentatsiya bajarilmadi: {e:#}");
            return None;
        }
    };
    let pred: Vec<f32> = out.first()?.to_plain_array_view::<f32>().ok()?.iter().copied().collect();
    if pred.len() != SIZE * SIZE {
        return None;
    }
    let (lo, hi) = pred.iter().fold((f32::MAX, f32::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    let span = (hi - lo).max(1e-6);
    let map: ImageBuffer<Luma<f32>, Vec<f32>> =
        ImageBuffer::from_raw(SIZE as u32, SIZE as u32, pred.iter().map(|v| (v - lo) / span).collect())?;
    let full = image::imageops::resize(&map, img.width(), img.height(), FilterType::Triangle);
    Some(full.into_raw())
}

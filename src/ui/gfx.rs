//! Cache delle texture: miniature, foto aperte, codice QR, logo.

use std::collections::HashMap;

use egui::{Color32, ColorImage, TextureHandle, TextureOptions};

use crate::media::decode_image;

pub struct Textures {
    map: HashMap<String, TextureHandle>,
    order: Vec<String>,
    limit: usize,
}

impl Default for Textures {
    fn default() -> Self {
        Self {
            map: HashMap::new(),
            order: Vec::new(),
            limit: 220,
        }
    }
}

impl Textures {
    pub fn get(&self, key: &str) -> Option<TextureHandle> {
        self.map.get(key).cloned()
    }

    pub fn insert(&mut self, ctx: &egui::Context, key: String, image: ColorImage, nearest: bool) {
        if self.map.contains_key(&key) {
            return;
        }
        let options = if nearest {
            TextureOptions::NEAREST
        } else {
            TextureOptions::LINEAR
        };
        let handle = ctx.load_texture(format!("img:{key}"), image, options);
        self.map.insert(key.clone(), handle);
        self.order.push(key);
        while self.order.len() > self.limit {
            let oldest = self.order.remove(0);
            self.map.remove(&oldest);
        }
    }

    pub fn rename(&mut self, old: &str, new: &str) {
        if let Some(handle) = self.map.remove(old) {
            self.map.insert(new.to_string(), handle);
            if let Some(slot) = self.order.iter_mut().find(|key| *key == old) {
                *slot = new.to_string();
            }
        }
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}

/// Decodifica un'immagine codificata (JPEG, PNG, WebP, GIF) in una texture.
pub fn image_from_bytes(bytes: &[u8]) -> Option<(ColorImage, u32, u32)> {
    let decoded = decode_image(bytes)?;
    if decoded.width == 0 || decoded.height == 0 {
        return None;
    }
    let image = ColorImage::from_rgba_unmultiplied(
        [decoded.width as usize, decoded.height as usize],
        &decoded.rgba,
    );
    Some((image, decoded.width, decoded.height))
}

/// Codice QR bianco e nero, con il margine di silenzio che serve allo scanner.
pub fn qr_image(code: &str) -> Option<ColorImage> {
    let qr = qrcode::QrCode::new(code.as_bytes()).ok()?;
    let colors = qr.to_colors();
    let width = qr.width();
    let quiet = 3usize;
    let scale = 4usize;
    let side = (width + quiet * 2) * scale;

    let mut pixels = vec![Color32::WHITE; side * side];
    for (index, color) in colors.iter().enumerate() {
        if !matches!(color, qrcode::Color::Dark) {
            continue;
        }
        let x = index % width;
        let y = index / width;
        for dy in 0..scale {
            for dx in 0..scale {
                let px = (x + quiet) * scale + dx;
                let py = (y + quiet) * scale + dy;
                pixels[py * side + px] = Color32::BLACK;
            }
        }
    }

    Some(ColorImage::new([side, side], pixels))
}

/// Logo dell'app, incorporato nel binario: nessun file da installare.
pub fn logo_image() -> Option<(ColorImage, u32, u32)> {
    let bytes = include_bytes!("../../res/icon-256.png");
    image_from_bytes(bytes)
}

//! Elementi grafici ricorrenti: avatar, separatori, barre.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, Ui, Vec2};

use crate::theme::Palette;

pub fn initials(name: &str, id: &str) -> String {
    crate::util::initials(&crate::util::display_name(name, id))
}

/// Avatar tondo con iniziali; per i gruppi un simbolo a tre teste.
pub fn avatar(
    painter: &Painter,
    rect: Rect,
    name: &str,
    id: &str,
    is_group: bool,
    online: bool,
) {
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.5;
    painter.circle_filled(center, radius, Palette::SURFACE);

    if is_group {
        let dark = Palette::MUTED;
        for (dx, dy, r) in [(-0.20f32, -0.08f32, 0.16f32), (0.0, -0.20, 0.18), (0.22, -0.04, 0.16)] {
            painter.circle_filled(
                Pos2::new(center.x + radius * dx, center.y + radius * dy),
                radius * r,
                dark,
            );
        }
        painter.add(egui::Shape::line(
            vec![
                Pos2::new(center.x - radius * 0.46, center.y + radius * 0.36),
                Pos2::new(center.x - radius * 0.20, center.y + radius * 0.10),
                Pos2::new(center.x, center.y + radius * 0.30),
                Pos2::new(center.x + radius * 0.22, center.y + radius * 0.08),
                Pos2::new(center.x + radius * 0.48, center.y + radius * 0.34),
            ],
            Stroke::new((radius * 0.10).max(1.0), dark),
        ));
    } else {
        let text = initials(name, id);
        let size = (radius * 0.78).clamp(9.0, 18.0);
        painter.text(
            center,
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(size),
            Palette::TEXT,
        );
    }

    if online {
        let dot = Pos2::new(rect.right() - radius * 0.22, rect.bottom() - radius * 0.22);
        painter.circle_filled(dot, (radius * 0.24).max(2.5), Palette::BG);
        painter.circle_filled(dot, (radius * 0.18).max(2.0), Palette::ONLINE);
    }
}

/// Avatar con foto profilo (angoli arrotondati) e fallback sulle iniziali.
pub fn avatar_tex(
    ui: &mut Ui,
    rect: Rect,
    texture: Option<&egui::TextureHandle>,
    name: &str,
    id: &str,
    is_group: bool,
    online: bool,
) {
    match texture {
        Some(texture) => {
            let radius = (rect.width() * 0.26).clamp(4.0, 16.0) as u8;
            let _ = ui.put(
                rect,
                egui::Image::new(egui::load::SizedTexture::new(texture.id(), rect.size()))
                    .corner_radius(egui::CornerRadius::same(radius))
                    .sense(egui::Sense::hover()),
            );
            if online {
                let painter = ui.painter();
                let radius = rect.width().min(rect.height()) * 0.5;
                let dot = Pos2::new(
                    rect.right() - radius * 0.22,
                    rect.bottom() - radius * 0.22,
                );
                painter.circle_filled(dot, (radius * 0.24).max(2.5), Palette::BG);
                painter.circle_filled(dot, (radius * 0.18).max(2.0), Palette::ONLINE);
            }
        }
        None => avatar(ui.painter(), rect, name, id, is_group, online),
    }
}

/// Etichetta di data fra i messaggi.
pub fn day_separator(ui: &mut Ui, label: &str) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 22.0), egui::Sense::hover());
        let painter = ui.painter();
        let text = painter.layout_no_wrap(
            label.to_string(),
            FontId::proportional(11.5),
            Palette::MUTED,
        );
        let size = text.size();
        let center = rect.center();
        let pad = Vec2::new(10.0, 4.0);
        let plate = Rect::from_center_size(center, size + pad * 2.0);
        painter.rect_filled(plate, egui::CornerRadius::same(9), Palette::SURFACE);
        painter.galley(
            Pos2::new(center.x - size.x * 0.5, center.y - size.y * 0.5),
            text,
            Palette::MUTED,
        );
    });
    ui.add_space(4.0);
}

/// Barra della forma d'onda per i vocali.
pub fn waveform(painter: &Painter, rect: Rect, progress: f32, played: Color32, rest: Color32) {
    const BARS: usize = 32;
    let gap = 2.0;
    let bar_w = ((rect.width() - gap * (BARS as f32 - 1.0)) / BARS as f32).max(1.0);
    for i in 0..BARS {
        let t = i as f32 / (BARS as f32 - 1.0);
        let envelope = 0.35
            + 0.65
                * ((t * std::f32::consts::PI * 2.6).sin().abs() * 0.6
                    + (t * std::f32::consts::PI * 9.0).sin().abs() * 0.4);
        let h = (rect.height() * envelope).clamp(2.0, rect.height());
        let x = rect.left() + i as f32 * (bar_w + gap);
        let bar = Rect::from_min_size(
            Pos2::new(x, rect.center().y - h * 0.5),
            Vec2::new(bar_w, h),
        );
        let color = if (i as f32 / BARS as f32) <= progress {
            played
        } else {
            rest
        };
        painter.rect_filled(bar, egui::CornerRadius::same(1), color);
    }
}

pub fn truncated(s: &str, max: usize) -> String {
    crate::util::truncate(s, max)
}

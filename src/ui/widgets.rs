//! Elementi grafici ricorrenti: avatar, separatori, barre.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Response, RichText, Stroke, Ui, Vec2};

use crate::theme::pal;

pub fn initials(name: &str, id: &str) -> String {
    crate::util::initials(&crate::util::display_name(name, id))
}

/// Disco con sfumatura verticale, disegnato come un'unica mesh: costa una
/// primitiva per avatar e dà il tono "contatto del telefono" senza immagini.
fn gradient_circle(painter: &Painter, center: Pos2, radius: f32, top: Color32, bottom: Color32) {
    use egui::epaint::{Vertex, WHITE_UV};
    const RINGS: usize = 3;
    const SEGMENTS: usize = 20;
    let color_at = |y: f32| {
        let t = ((y - (center.y - radius)) / (2.0 * radius)).clamp(0.0, 1.0);
        crate::theme::mix(top, bottom, t)
    };
    let mut mesh = egui::Mesh::default();
    mesh.vertices.push(Vertex {
        pos: center,
        uv: WHITE_UV,
        color: color_at(center.y - radius * 0.35),
    });
    for ring in 1..=RINGS {
        let r = radius * ring as f32 / RINGS as f32;
        for segment in 0..SEGMENTS {
            let angle = segment as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            let pos = Pos2::new(center.x + angle.cos() * r, center.y + angle.sin() * r);
            mesh.vertices.push(Vertex {
                pos,
                uv: WHITE_UV,
                color: color_at(pos.y),
            });
        }
    }
    for segment in 0..SEGMENTS {
        let next = (segment + 1) % SEGMENTS;
        mesh.add_triangle(0, 1 + segment as u32, 1 + next as u32);
    }
    for ring in 1..RINGS {
        let inner = 1 + (ring - 1) * SEGMENTS;
        let outer = 1 + ring * SEGMENTS;
        for segment in 0..SEGMENTS {
            let next = (segment + 1) % SEGMENTS;
            let (a, b, c, d) = (
                (inner + segment) as u32,
                (inner + next) as u32,
                (outer + segment) as u32,
                (outer + next) as u32,
            );
            mesh.add_triangle(a, c, b);
            mesh.add_triangle(b, c, d);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Avatar tondo con iniziali (o il simbolo del gruppo), colorato come sul
/// telefono: la stessa persona ha sempre la stessa tinta.
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
    if is_group {
        // I gruppi restano neutri: il grigio del tema, con il simbolo a tre
        // teste, così non si confondono con le persone.
        let ink = crate::theme::pal().text;
        let top = crate::theme::mix(crate::theme::pal().surface, ink, 0.22);
        let bottom = crate::theme::mix(crate::theme::pal().surface, ink, 0.06);
        gradient_circle(painter, center, radius, top, bottom);
        let r = radius;
        for (dx, dy, size) in [
            (-0.20f32, -0.06f32, 0.15f32),
            (0.0, -0.18, 0.17),
            (0.22, -0.02, 0.15),
        ] {
            painter.circle_filled(
                Pos2::new(center.x + r * dx, center.y + r * dy),
                r * size,
                ink,
            );
        }
        painter.add(egui::Shape::line(
            vec![
                Pos2::new(center.x - r * 0.46, center.y + r * 0.36),
                Pos2::new(center.x - r * 0.20, center.y + r * 0.12),
                Pos2::new(center.x, center.y + r * 0.30),
                Pos2::new(center.x + r * 0.22, center.y + r * 0.10),
                Pos2::new(center.x + r * 0.48, center.y + r * 0.34),
            ],
            Stroke::new((r * 0.10).max(1.0), ink),
        ));
    } else {
        let (top, bottom) = crate::theme::avatar(id);
        gradient_circle(painter, center, radius, top, bottom);
        let size = (radius * 0.80).clamp(9.0, 21.0);
        painter.text(
            center,
            Align2::CENTER_CENTER,
            initials(name, id),
            FontId::proportional(size),
            crate::theme::avatar_ink(id),
        );
    }

    if online {
        // Puntino di presenza con un anello del colore del pannello, così si
        // vede anche sopra un avatar chiaro.
        let dot = Pos2::new(rect.right() - radius * 0.16, rect.bottom() - radius * 0.16);
        painter.circle_filled(dot, (radius * 0.26).max(3.0), crate::theme::pal().panel);
        painter.circle_filled(dot, (radius * 0.19).max(2.2), crate::theme::pal().online);
    }
}

/// Avatar con foto profilo (tondo) e fallback sulle iniziali colorate.
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
            let _ = ui.put(
                rect,
                egui::Image::new(egui::load::SizedTexture::new(texture.id(), rect.size()))
                    .corner_radius(egui::CornerRadius::same(
                        (rect.width() * 0.5).round().clamp(0.0, 255.0) as u8,
                    ))
                    .sense(egui::Sense::hover()),
            );
            if online {
                let painter = ui.painter();
                let radius = rect.width().min(rect.height()) * 0.5;
                let dot = Pos2::new(
                    rect.right() - radius * 0.16,
                    rect.bottom() - radius * 0.16,
                );
                painter.circle_filled(dot, (radius * 0.26).max(3.0), crate::theme::pal().panel);
                painter.circle_filled(dot, (radius * 0.19).max(2.2), crate::theme::pal().online);
            }
        }
        None => avatar(ui.painter(), rect, name, id, is_group, online),
    }
}

/// Titolo di sezione: minuscolo, maiuscolo solo nell'aspetto tipografico
/// (il testo resta scritto normalmente per la ricerca e i testi grandi).
pub fn section(ui: &mut Ui, title: &str) {
    ui.add_space(12.0);
    ui.label(
        RichText::new(title.to_uppercase())
            .size(10.0)
            .strong()
            .color(pal().faint),
    );
    ui.add_space(2.0);
}

/// Riga di impostazione con interruttore a scorrimento.
pub fn toggle_row(ui: &mut Ui, title: &str, subtitle: &str, value: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(title).size(12.5).color(pal().text));
            if !subtitle.is_empty() {
                ui.label(RichText::new(subtitle).size(10.5).color(pal().muted));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if switch(ui, value).changed() {
                changed = true;
            }
        });
    });
    changed
}

/// Interruttore: una pastiglia che si accende col verde d'accento. Il pomello
/// scorre con l'animazione già usata da egui, quindi non costa un timer.
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let size = Vec2::new(38.0, 22.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let t = ui.ctx().animate_bool(response.id, *on);
    let track = crate::theme::mix(pal().border, pal().accent, t);
    ui.painter()
        .rect_filled(rect.shrink(1.0), egui::CornerRadius::same(11), track);
    let x = rect.left() + 12.0 + (rect.width() - 24.0) * t;
    ui.painter().circle_filled(
        egui::Pos2::new(x, rect.center().y),
        8.0,
        if t > 0.5 { pal().accent_text } else { pal().text },
    );
    response
}

/// Pulsante principale: pieno, verde, alto abbastanza da essere cliccato.
pub fn primary_button(ui: &mut Ui, text: &str, width: f32) -> Response {
    ui.add_sized(
        Vec2::new(width, 36.0),
        egui::Button::new(RichText::new(text).size(13.0).color(pal().accent_text))
            .fill(pal().accent)
            .corner_radius(egui::CornerRadius::same(10)),
    )
}

/// Pulsante secondario: superficie del tema, bordo sottile.
pub fn ghost_button(ui: &mut Ui, text: &str, width: f32) -> Response {
    ui.add_sized(
        Vec2::new(width, 36.0),
        egui::Button::new(RichText::new(text).size(13.0).color(pal().text))
            .fill(pal().surface)
            .stroke(egui::Stroke::new(1.0, pal().border))
            .corner_radius(egui::CornerRadius::same(10)),
    )
}

/// Pastiglia di scelta (Automatico / Chiaro / Scuro, Tutte / Non lette).
pub fn chip(ui: &mut Ui, text: &str, selected: bool) -> Response {
    let (fill, ink) = if selected {
        (pal().accent, pal().accent_text)
    } else {
        (pal().surface, pal().text)
    };
    ui.add(
        egui::Button::new(RichText::new(text).size(12.0).color(ink))
            .fill(fill)
            .stroke(if selected {
                egui::Stroke::NONE
            } else {
                egui::Stroke::new(1.0, pal().border)
            })
            .corner_radius(egui::CornerRadius::same(9)),
    )
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
            pal().muted,
        );
        let size = text.size();
        let center = rect.center();
        let pad = Vec2::new(10.0, 4.0);
        let plate = Rect::from_center_size(center, size + pad * 2.0);
        painter.rect_filled(plate, egui::CornerRadius::same(9), pal().surface);
        painter.galley(
            Pos2::new(center.x - size.x * 0.5, center.y - size.y * 0.5),
            text,
            pal().muted,
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

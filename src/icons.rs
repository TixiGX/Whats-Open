//! Icone disegnate a mano con `egui::Painter`.
//!
//! Nessun font di icone, nessun sprite: tratti e poligoni in coordinate 24×24,
//! così l'interfaccia resta nitida a qualsiasi scala e pesa zero byte.

use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Send,
    Attach,
    Mic,
    Plus,
    Gear,
    Search,
    Back,
    Close,
    Play,
    Pause,
    Stop,
    Download,
    Image,
    Video,
    VideoOff,
    Doc,
    Trash,
    Logout,
    Refresh,
    Check,
    Lock,
    ChevronDown,
    Phone,
    PhoneOff,
    MicOff,
    Gif,
}

/// Ruota un punto dell'icona attorno al centro (coordinate 24×24).
fn spin(rect: Rect, x: f32, y: f32, degrees: f32) -> Pos2 {
    let base = map(rect, x, y);
    if degrees == 0.0 {
        return base;
    }
    let center = map(rect, 12.0, 12.0);
    let radians = degrees.to_radians();
    let (sin, cos) = radians.sin_cos();
    let dx = base.x - center.x;
    let dy = base.y - center.y;
    Pos2::new(
        center.x + dx * cos - dy * sin,
        center.y + dx * sin + dy * cos,
    )
}

/// La cornetta: arco e ricevitore, disegnata una volta sola e riusata
/// (anche ruotata) dal tasto per chiudere la chiamata.
fn draw_receiver(ui: &Ui, rect: Rect, color: Color32, degrees: f32) {
    let painter = ui.painter();
    let w = (rect.width() / 24.0).max(0.5);
    let stroke = Stroke::new((w * 1.7).max(1.2), color);
    let c = map(rect, 12.0, 12.0);
    let r = w * 6.6;
    let mut arc = Vec::new();
    for i in 0..10 {
        let angle = -2.6 + i as f32 * 0.22;
        arc.push(Pos2::new(c.x + angle.cos() * r, c.y + angle.sin() * r));
    }
    if degrees != 0.0 {
        let radians = degrees.to_radians();
        let (sin, cos) = radians.sin_cos();
        for point in arc.iter_mut() {
            let dx = point.x - c.x;
            let dy = point.y - c.y;
            *point = Pos2::new(c.x + dx * cos - dy * sin, c.y + dx * sin + dy * cos);
        }
    }
    painter.add(egui::Shape::line(arc, stroke));
    let shape = [
        (7.2, 4.4),
        (4.6, 7.0),
        (5.6, 9.6),
        (14.4, 18.4),
        (17.0, 19.4),
        (19.6, 16.8),
        (18.6, 14.2),
        (15.8, 13.2),
        (14.2, 14.8),
        (9.4, 10.0),
        (11.0, 8.4),
        (10.0, 5.6),
        (7.2, 4.4),
    ];
    painter.add(egui::Shape::line(
        shape
            .iter()
            .map(|(x, y)| spin(rect, *x, *y, degrees))
            .collect(),
        stroke,
    ));
}

fn map(rect: Rect, x: f32, y: f32) -> Pos2 {
    Pos2::new(
        rect.left() + rect.width() * x / 24.0,
        rect.top() + rect.height() * y / 24.0,
    )
}

impl Icon {
    pub fn paint(self, ui: &Ui, rect: Rect, color: Color32) {
        let painter = ui.painter();
        let w = (rect.width() / 24.0).max(0.5);
        let stroke = Stroke::new((w * 1.9).max(1.25), color);
        let thin = Stroke::new((w * 1.5).max(1.0), color);

        match self {
            Icon::Send => {
                let pts = vec![
                    map(rect, 3.5, 11.0),
                    map(rect, 21.0, 3.5),
                    map(rect, 13.0, 21.0),
                    map(rect, 11.2, 13.2),
                    map(rect, 3.5, 11.0),
                ];
                painter.add(egui::Shape::convex_polygon(
                    pts,
                    Color32::TRANSPARENT,
                    Stroke::new(0.0, color),
                ));
                painter.add(egui::Shape::line(
                    vec![
                        map(rect, 3.5, 11.0),
                        map(rect, 21.0, 3.5),
                        map(rect, 13.0, 21.0),
                        map(rect, 11.2, 13.2),
                    ],
                    stroke,
                ));
                painter.line_segment([map(rect, 11.2, 13.2), map(rect, 21.0, 3.5)], thin);
            }
            Icon::Attach => {
                let c = map(rect, 12.0, 12.0);
                painter.circle_stroke(c, w * 7.0, thin);
                painter.line_segment([map(rect, 7.0, 12.0), map(rect, 17.0, 12.0)], thin);
            }
            Icon::Mic => {
                let capsule = Rect::from_min_max(map(rect, 9.0, 3.0), map(rect, 15.0, 14.0));
                painter.rect_stroke(capsule, egui::CornerRadius::same(6), stroke, StrokeKind::Inside);
                painter.add(egui::Shape::line(
                    vec![
                        map(rect, 5.5, 11.5),
                        map(rect, 5.5, 13.0),
                        map(rect, 8.0, 17.0),
                        map(rect, 16.0, 17.0),
                        map(rect, 18.5, 13.0),
                        map(rect, 18.5, 11.5),
                    ],
                    thin,
                ));
                painter.line_segment([map(rect, 12.0, 17.0), map(rect, 12.0, 21.0)], thin);
            }
            Icon::Plus => {
                painter.line_segment([map(rect, 12.0, 4.0), map(rect, 12.0, 20.0)], stroke);
                painter.line_segment([map(rect, 4.0, 12.0), map(rect, 20.0, 12.0)], stroke);
            }
            Icon::Gear => {
                let c = map(rect, 12.0, 12.0);
                painter.circle_stroke(c, w * 3.2, thin);
                for i in 0..8 {
                    let angle = i as f32 * std::f32::consts::TAU / 8.0;
                    let inner = 5.6;
                    let outer = 8.4;
                    painter.line_segment(
                        [
                            Pos2::new(c.x + angle.cos() * w * inner, c.y + angle.sin() * w * inner),
                            Pos2::new(c.x + angle.cos() * w * outer, c.y + angle.sin() * w * outer),
                        ],
                        thin,
                    );
                }
            }
            Icon::Search => {
                let c = map(rect, 10.5, 10.5);
                painter.circle_stroke(c, w * 6.0, thin);
                painter.line_segment([map(rect, 15.0, 15.0), map(rect, 20.5, 20.5)], thin);
            }
            Icon::Back => {
                painter.add(egui::Shape::line(
                    vec![
                        map(rect, 14.0, 4.5),
                        map(rect, 6.5, 12.0),
                        map(rect, 14.0, 19.5),
                    ],
                    stroke,
                ));
            }
            Icon::Close => {
                painter.line_segment([map(rect, 5.0, 5.0), map(rect, 19.0, 19.0)], thin);
                painter.line_segment([map(rect, 19.0, 5.0), map(rect, 5.0, 19.0)], thin);
            }
            Icon::Play => {
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        map(rect, 7.0, 4.5),
                        map(rect, 20.0, 12.0),
                        map(rect, 7.0, 19.5),
                    ],
                    color,
                    Stroke::NONE,
                ));
            }
            Icon::Pause => {
                let a = Rect::from_min_max(map(rect, 6.0, 5.0), map(rect, 10.0, 19.0));
                let b = Rect::from_min_max(map(rect, 14.0, 5.0), map(rect, 18.0, 19.0));
                painter.rect_filled(a, egui::CornerRadius::same(1), color);
                painter.rect_filled(b, egui::CornerRadius::same(1), color);
            }
            Icon::Stop => {
                let r = Rect::from_min_max(map(rect, 6.0, 6.0), map(rect, 18.0, 18.0));
                painter.rect_filled(r, egui::CornerRadius::same(2), color);
            }
            Icon::Download => {
                painter.line_segment([map(rect, 12.0, 4.0), map(rect, 12.0, 15.0)], thin);
                painter.add(egui::Shape::line(
                    vec![
                        map(rect, 7.5, 10.5),
                        map(rect, 12.0, 15.0),
                        map(rect, 16.5, 10.5),
                    ],
                    thin,
                ));
                painter.add(egui::Shape::line(
                    vec![
                        map(rect, 4.0, 17.0),
                        map(rect, 4.0, 20.0),
                        map(rect, 20.0, 20.0),
                        map(rect, 20.0, 17.0),
                    ],
                    thin,
                ));
            }
            Icon::Image => {
                let r = Rect::from_min_max(map(rect, 3.5, 4.0), map(rect, 20.5, 20.0));
                painter.rect_stroke(r, egui::CornerRadius::same(3), thin, StrokeKind::Inside);
                painter.circle_filled(map(rect, 9.0, 9.0), w * 1.4, color);
                painter.add(egui::Shape::line(
                    vec![
                        map(rect, 4.5, 18.0),
                        map(rect, 10.0, 12.0),
                        map(rect, 14.0, 16.0),
                        map(rect, 17.0, 13.0),
                        map(rect, 20.0, 16.0),
                    ],
                    thin,
                ));
            }
            Icon::Video => {
                let r = Rect::from_min_max(map(rect, 3.0, 6.0), map(rect, 16.0, 18.0));
                painter.rect_stroke(r, egui::CornerRadius::same(3), thin, StrokeKind::Inside);
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        map(rect, 16.5, 10.0),
                        map(rect, 21.0, 7.0),
                        map(rect, 21.0, 17.0),
                        map(rect, 16.5, 14.0),
                    ],
                    color,
                    Stroke::NONE,
                ));
            }
            Icon::VideoOff => {
                let r = Rect::from_min_max(map(rect, 3.0, 6.0), map(rect, 16.0, 18.0));
                painter.rect_stroke(r, egui::CornerRadius::same(3), thin, StrokeKind::Inside);
                painter.add(egui::Shape::line(
                    vec![map(rect, 16.5, 10.5), map(rect, 21.0, 7.5)],
                    thin,
                ));
                painter.add(egui::Shape::line(
                    vec![map(rect, 16.5, 13.5), map(rect, 21.0, 16.5)],
                    thin,
                ));
                // Sbarra diagonale: fotocamera spenta.
                painter.add(egui::Shape::line(
                    vec![map(rect, 4.0, 4.0), map(rect, 20.0, 20.0)],
                    thin,
                ));
            }
            Icon::Doc => {
                painter.add(egui::Shape::line(
                    vec![
                        map(rect, 6.0, 3.0),
                        map(rect, 14.0, 3.0),
                        map(rect, 19.0, 8.0),
                        map(rect, 19.0, 21.0),
                        map(rect, 6.0, 21.0),
                        map(rect, 6.0, 3.0),
                    ],
                    thin,
                ));
                painter.add(egui::Shape::line(
                    vec![
                        map(rect, 14.0, 3.0),
                        map(rect, 14.0, 8.0),
                        map(rect, 19.0, 8.0),
                    ],
                    thin,
                ));
            }
            Icon::Trash => {
                painter.line_segment([map(rect, 4.0, 6.5), map(rect, 20.0, 6.5)], thin);
                painter.add(egui::Shape::line(
                    vec![
                        map(rect, 6.5, 6.5),
                        map(rect, 7.5, 20.0),
                        map(rect, 16.5, 20.0),
                        map(rect, 17.5, 6.5),
                    ],
                    thin,
                ));
                painter.add(egui::Shape::line(
                    vec![
                        map(rect, 9.5, 6.5),
                        map(rect, 9.5, 3.8),
                        map(rect, 14.5, 3.8),
                        map(rect, 14.5, 6.5),
                    ],
                    thin,
                ));
            }
            Icon::Logout => {
                painter.add(egui::Shape::line(
                    vec![
                        map(rect, 12.0, 4.0),
                        map(rect, 5.0, 4.0),
                        map(rect, 5.0, 20.0),
                        map(rect, 12.0, 20.0),
                    ],
                    thin,
                ));
                painter.line_segment([map(rect, 10.0, 12.0), map(rect, 21.0, 12.0)], thin);
                painter.add(egui::Shape::line(
                    vec![
                        map(rect, 16.5, 7.5),
                        map(rect, 21.0, 12.0),
                        map(rect, 16.5, 16.5),
                    ],
                    thin,
                ));
            }
            Icon::Refresh => {
                let c = map(rect, 12.0, 12.0);
                let r = w * 7.5;
                let mut pts = Vec::new();
                for i in 0..26 {
                    let angle = -2.4 + i as f32 * 0.19;
                    pts.push(Pos2::new(c.x + angle.cos() * r, c.y + angle.sin() * r));
                }
                painter.add(egui::Shape::line(pts, thin));
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        map(rect, 16.0, 2.2),
                        map(rect, 20.6, 5.4),
                        map(rect, 16.2, 8.2),
                    ],
                    color,
                    Stroke::NONE,
                ));
            }
            Icon::Check => {
                painter.add(egui::Shape::line(
                    vec![
                        map(rect, 5.0, 12.5),
                        map(rect, 9.5, 17.0),
                        map(rect, 19.0, 7.0),
                    ],
                    thin,
                ));
            }
            Icon::Lock => {
                let body = Rect::from_min_max(map(rect, 5.5, 10.5), map(rect, 18.5, 20.5));
                painter.rect_stroke(body, egui::CornerRadius::same(3), thin, StrokeKind::Inside);
                painter.add(egui::Shape::line(
                    vec![
                        map(rect, 8.5, 10.5),
                        map(rect, 8.5, 7.5),
                        map(rect, 10.0, 4.5),
                        map(rect, 14.0, 4.5),
                        map(rect, 15.5, 7.5),
                        map(rect, 15.5, 10.5),
                    ],
                    thin,
                ));
            }
            Icon::Phone => draw_receiver(ui, rect, color, 0.0),
            Icon::PhoneOff => {
                // Cornetta abbassata: la stessa forma ruotata di 135°, come
                // il tasto "chiudi" di WhatsApp.
                draw_receiver(ui, rect, color, 135.0);
            }
            Icon::MicOff => {
                let body = Rect::from_min_max(map(rect, 9.0, 3.0), map(rect, 15.0, 13.0));
                painter.rect_stroke(body, egui::CornerRadius::same(3), thin, StrokeKind::Inside);
                painter.add(egui::Shape::line(
                    vec![
                        map(rect, 6.0, 11.0),
                        map(rect, 6.0, 13.0),
                        map(rect, 8.0, 16.5),
                        map(rect, 12.0, 17.5),
                        map(rect, 16.0, 16.5),
                        map(rect, 18.0, 13.0),
                        map(rect, 18.0, 11.0),
                    ],
                    thin,
                ));
                painter.line_segment([map(rect, 12.0, 17.5), map(rect, 12.0, 21.0)], thin);
                painter.line_segment([map(rect, 4.0, 4.0), map(rect, 20.0, 20.0)], thin);
            }
            Icon::Gif => {
                let r = Rect::from_min_max(map(rect, 3.0, 5.5), map(rect, 21.0, 18.5));
                painter.rect_stroke(r, egui::CornerRadius::same(3), thin, StrokeKind::Inside);
                painter.text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    "GIF",
                    egui::FontId::proportional((w * 5.4).max(8.0)),
                    color,
                );
            }
            Icon::ChevronDown => {
                painter.add(egui::Shape::line(
                    vec![
                        map(rect, 6.0, 9.5),
                        map(rect, 12.0, 15.5),
                        map(rect, 18.0, 9.5),
                    ],
                    thin,
                ));
            }
        }
    }

    /// Area cliccabile tonda con l'icona centrata.
    pub fn button(self, ui: &mut Ui, size: f32, color: Color32) -> Response {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
        let hovered = response.hovered();
        if hovered {
            ui.painter().rect_filled(
                rect.expand(2.0),
                egui::CornerRadius::same((size * 0.5) as u8),
                crate::theme::pal().hover,
            );
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let glyph = Rect::from_center_size(rect.center(), Vec2::splat(size * 0.62));
        self.paint(
            ui,
            glyph,
            if hovered {
                crate::theme::pal().text
            } else {
                color
            },
        );
        response
    }

    /// Bottone pieno e tondo: serve al pollice dell'invio, dove il cerchio
    /// pieno dice "premi qui" meglio di un'icona nuda.
    pub fn filled_button(self, ui: &mut Ui, size: f32, icon: Color32, fill: Color32) -> Response {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
        let hovered = response.hovered();
        let radius = size * 0.5;
        ui.painter().circle_filled(rect.center(), radius, fill);
        if hovered {
            ui.painter().circle_stroke(
                rect.center(),
                radius,
                Stroke::new(1.0, crate::theme::pal().muted),
            );
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let glyph = Rect::from_center_size(rect.center(), Vec2::splat(size * 0.56));
        self.paint(ui, glyph, icon);
        response
    }
}

/// Doppia spunta per messaggi letti, singola per consegnati.
pub fn ticks(ui: &Ui, rect: Rect, status: crate::model::Status, color: Color32) {
    use crate::model::Status;
    let painter = ui.painter();
    let h = rect.height();
    let stroke = Stroke::new((h * 0.16).max(1.0), color);

    let check = |dx: f32| {
        vec![
            Pos2::new(rect.left() + dx, rect.center().y),
            Pos2::new(rect.left() + dx + h * 0.28, rect.bottom() - h * 0.15),
            Pos2::new(rect.left() + dx + h * 0.72, rect.top() + h * 0.18),
        ]
    };

    match status {
        Status::Pending => {
            let c = rect.center();
            painter.circle_stroke(c, h * 0.30, Stroke::new((h * 0.12).max(1.0), color));
            painter.line_segment(
                [c - Vec2::new(0.0, h * 0.16), c + Vec2::new(0.0, h * 0.06)],
                stroke,
            );
            painter.line_segment([c, c + Vec2::new(h * 0.12, 0.0)], stroke);
        }
        Status::Sent => {
            painter.add(egui::Shape::line(check(0.0), stroke));
        }
        Status::Delivered => {
            painter.add(egui::Shape::line(check(0.0), stroke));
            painter.add(egui::Shape::line(check(h * 0.42), stroke));
        }
        Status::Read | Status::Played => {
            let strong = Stroke::new((h * 0.20).max(1.2), color);
            painter.add(egui::Shape::line(check(0.0), strong));
            painter.add(egui::Shape::line(check(h * 0.42), strong));
        }
        Status::Failed => {
            painter.circle_stroke(rect.center(), h * 0.32, stroke);
            painter.line_segment(
                [rect.center() - Vec2::splat(h * 0.14), rect.center() + Vec2::splat(h * 0.14)],
                stroke,
            );
        }
    }
}

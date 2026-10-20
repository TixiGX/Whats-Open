//! Elenco delle conversazioni.

use egui::{Align2, FontId, Pos2, Rect, Response, RichText, Sense, Vec2};

use crate::model::Chat;
use crate::theme::Palette;
use crate::util;

use super::{gfx, widgets, App};

impl App {
    pub fn sidebar_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);

        let mut open_new = false;
        let mut open_settings = false;
        ui.horizontal(|ui| {
            ui.add_space(14.0);

            if self.logo_tex.is_none() {
                if let Some((image, _, _)) = gfx::logo_image() {
                    self.logo_tex = Some(ui.ctx().load_texture(
                        "logo",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
            if let Some(texture) = self.logo_tex.as_ref() {
                ui.add(
                    egui::Image::new(egui::load::SizedTexture::new(
                        texture.id(),
                        Vec2::splat(24.0),
                    ))
                    .corner_radius(6),
                );
            }
            ui.add_space(2.0);
            ui.label(RichText::new("Whats-Open").size(15.0).color(Palette::TEXT));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(10.0);
                if crate::icons::Icon::Gear
                    .button(ui, 30.0, Palette::MUTED)
                    .on_hover_text("Impostazioni")
                    .clicked()
                {
                    open_settings = true;
                }
                if crate::icons::Icon::Plus
                    .button(ui, 30.0, Palette::MUTED)
                    .on_hover_text("Nuova chat")
                    .clicked()
                {
                    open_new = true;
                }
            });
        });

        if open_settings {
            self.settings = true;
        }
        if open_new {
            self.new_chat = true;
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            let width = ui.available_width() - 12.0;
            let frame = egui::Frame::new()
                .fill(Palette::SURFACE)
                .corner_radius(egui::CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(10, 6));
            frame.show(ui, |ui| {
                ui.set_width(width - 20.0);
                ui.horizontal(|ui| {
                    crate::icons::Icon::Search.paint(
                        ui,
                        Rect::from_min_size(ui.cursor().min, Vec2::splat(15.0)),
                        Palette::FAINT,
                    );
                    ui.add_space(19.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text("Cerca")
                            .desired_width(f32::INFINITY)
                            .frame(egui::Frame::new().fill(egui::Color32::TRANSPARENT))
                            .text_color(Palette::TEXT),
                    );
                });
            });
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(2.0);

        let query = self.search.trim().to_lowercase();
        let mut chosen: Option<String> = None;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let count = self.chats.len();
                let mut shown = 0;
                for index in 0..count {
                    let chat = self.chats[index].clone();
                    let title = self.title_for(&chat);
                    if !query.is_empty() {
                        if !title.to_lowercase().contains(&query)
                            && !chat.preview.to_lowercase().contains(&query)
                        {
                            continue;
                        }
                    }
                    shown += 1;
                    let selected = self.selected.as_deref() == Some(chat.id.as_str());
                    let typing = chat.typing;
                    let texture = self.avatars.get(&chat.id).cloned();
                    // Numero sotto il nome quando il nome non è il numero:
                    // senza rubrica i contatti si riconoscono solo così.
                    let sub = secondary_line(&chat, &title);
                    if chat_row(ui, &chat, &title, &sub, texture.as_ref(), selected, typing)
                        .clicked()
                    {
                        chosen = Some(chat.id.clone());
                    }
                    if texture.is_none() {
                        self.request_avatar(&chat.id);
                    }
                }
                if shown == 0 {
                    ui.add_space(30.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new(if query.is_empty() {
                                "Nessuna conversazione"
                            } else {
                                "Nessun risultato"
                            })
                            .size(12.5)
                            .color(Palette::FAINT),
                        );
                    });
                }
            });

        if let Some(id) = chosen {
            self.select_chat(id);
        }
    }
}

/// Seconda riga della lista quando non c'è un'anteprima: il numero, utile
/// nelle chat appena aperte. Negli altri casi resta l'anteprima del messaggio.
fn secondary_line(chat: &Chat, title: &str) -> String {
    if !chat.preview.is_empty() {
        return String::new();
    }
    let user = util::jid_user(&chat.id);
    let is_number = !user.is_empty() && user.chars().all(|c| c.is_ascii_digit());
    if !chat.is_group && is_number {
        let phone = util::phone_display(&user);
        if title != phone {
            return phone;
        }
    }
    String::new()
}

fn chat_row(
    ui: &mut egui::Ui,
    chat: &Chat,
    title: &str,
    sub: &str,
    texture: Option<&egui::TextureHandle>,
    selected: bool,
    typing: bool,
) -> Response {
    let height = 68.0;
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());

    {
        let painter = ui.painter();
        if selected {
            painter.rect_filled(rect, egui::CornerRadius::same(0), Palette::SURFACE);
        } else if response.hovered() {
            painter.rect_filled(rect, egui::CornerRadius::same(0), Palette::PANEL);
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }

    let avatar_rect = Rect::from_center_size(
        Pos2::new(rect.left() + 37.0, rect.center().y),
        Vec2::splat(46.0),
    );
    widgets::avatar_tex(
        ui,
        avatar_rect,
        texture,
        &chat.name,
        &chat.id,
        chat.is_group,
        chat.online,
    );

    let painter = ui.painter();
    let text_left = rect.left() + 68.0;
    let name_color = if chat.unread > 0 {
        Palette::WHITE
    } else {
        Palette::TEXT
    };
    // Caratteri che entrano davvero: il pannello è ridimensionabile.
    let room = (rect.right() - 62.0 - text_left).max(90.0);
    let title = util::truncate(title, (room / 7.0) as usize);
    painter.text(
        Pos2::new(text_left, rect.center().y - 10.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(14.0),
        name_color,
    );

    let preview = if typing {
        "sta scrivendo…".to_string()
    } else if !sub.is_empty() {
        sub.to_string()
    } else if chat.preview.is_empty() {
        crate::ui::kind_glyph(crate::model::Kind::Text).to_string()
    } else {
        chat.preview.clone()
    };
    let preview_color = if typing { Palette::TEXT } else { Palette::MUTED };
    let preview = util::truncate(&preview, (room / 6.1) as usize);
    painter.text(
        Pos2::new(text_left, rect.center().y + 11.0),
        Align2::LEFT_CENTER,
        preview,
        FontId::proportional(12.0),
        preview_color,
    );

    let right = rect.right() - 14.0;
    painter.text(
        Pos2::new(right, rect.center().y - 12.0),
        Align2::RIGHT_CENTER,
        util::fmt_chat_time(chat.last_ts),
        FontId::proportional(10.5),
        if chat.unread > 0 {
            Palette::TEXT
        } else {
            Palette::FAINT
        },
    );

    if chat.unread > 0 {
        let badge = Rect::from_center_size(
            Pos2::new(right - 8.0, rect.center().y + 13.0),
            Vec2::splat(17.0),
        );
        painter.circle_filled(badge.center(), 8.5, Palette::WHITE);
        painter.text(
            badge.center(),
            Align2::CENTER_CENTER,
            if chat.unread > 99 {
                "99".to_string()
            } else {
                chat.unread.to_string()
            },
            FontId::proportional(9.5),
            crate::theme::Palette::OWN_TEXT,
        );
    } else if chat.pinned {
        painter.circle_filled(
            Pos2::new(right - 4.0, rect.center().y + 13.0),
            2.2,
            Palette::MUTED,
        );
    }

    response
}

//! Conversazione: intestazione, messaggi, casella di composizione.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Color32, RichText, Sense, TextureHandle, Vec2};

use crate::engine::{Cmd, ReplyTarget};
use crate::media;
use crate::model::{Kind, Message, Quote, Status};
use crate::theme::Palette;
use crate::util;

use super::{widgets, Action, App};

struct Bubble {
    id: String,
    own: bool,
    ts: i64,
    kind: Kind,
    text: String,
    status: Status,
    view_once: bool,
    played: bool,
    mime: String,
    file_name: String,
    file_size: u64,
    duration: u32,
    quote: Option<Quote>,
    edited: bool,
    deleted: bool,
    reaction: Option<String>,
    sender_name: String,
    sender: String,
}

impl Bubble {
    fn from(message: &Message) -> Self {
        Self {
            id: message.id.clone(),
            own: message.from_me,
            ts: message.ts,
            kind: message.kind,
            text: message.text.clone(),
            status: message.status,
            view_once: message.view_once,
            played: message.played,
            mime: message.mime.clone(),
            file_name: message.file_name.clone(),
            file_size: message.file_size,
            duration: message.duration,
            quote: message.quote.clone(),
            edited: message.edited,
            deleted: message.deleted,
            reaction: message.reaction.clone(),
            sender_name: message.sender_name.clone(),
            sender: message.sender.clone(),
        }
    }
}

impl App {
    pub fn chat_ui(&mut self, ui: &mut egui::Ui) {
        let Some(chat) = self.selected_chat().cloned() else {
            self.empty_ui(ui);
            return;
        };

        if self.reconnecting {
            egui::Frame::new()
                .fill(Palette::SURFACE)
                .inner_margin(egui::Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(12.0).color(Palette::MUTED));
                        ui.label(
                            RichText::new(format!("Riconnessione… {}", self.status_text))
                                .size(11.5)
                                .color(Palette::MUTED),
                        );
                    });
                });
        }

        self.header_ui(ui, &chat);
        self.messages_ui(ui);
        self.composer_ui(ui);
    }

    fn empty_ui(&mut self, ui: &mut egui::Ui) {
        {
            let height = ui.available_height();
            ui.vertical_centered(|ui| {
                ui.add_space(height * 0.33);
                if let Some(texture) = self.logo_tex.as_ref() {
                    ui.add(
                        egui::Image::new(egui::load::SizedTexture::new(
                            texture.id(),
                            Vec2::splat(58.0),
                        ))
                        .corner_radius(14)
                        .tint(Color32::from_gray(90)),
                    );
                }
                ui.add_space(12.0);
                ui.label(
                    RichText::new("Scegli una conversazione")
                        .size(14.0)
                        .color(Palette::MUTED),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("oppure avviane una nuova con +")
                        .size(11.5)
                        .color(Palette::FAINT),
                );
            });
        }
    }

    fn header_ui(&mut self, ui: &mut egui::Ui, chat: &crate::model::Chat) {
        egui::Frame::new()
            .fill(Palette::PANEL)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if crate::icons::Icon::Back
                        .button(ui, 30.0, Palette::MUTED)
                        .on_hover_text("Torna alle chat")
                        .clicked()
                    {
                        self.selected = None;
                        self.messages.clear();
                        self.reply = None;
                        self.reply_label.clear();
                    }

                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::splat(38.0), Sense::hover());
                    widgets::avatar(
                        ui.painter(),
                        rect,
                        &chat.name,
                        &chat.id,
                        chat.is_group,
                        chat.online,
                    );

                    ui.add_space(2.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(util::display_name(&chat.name, &chat.id))
                                .size(14.0)
                                .color(Palette::TEXT),
                        );
                        let subtitle = if chat.typing {
                            "sta scrivendo…".to_string()
                        } else if chat.is_group {
                            "Gruppo".to_string()
                        } else if chat.online {
                            "online".to_string()
                        } else if let Some(ts) = chat.last_seen {
                            format!("visto {}", util::fmt_chat_time(ts))
                        } else {
                            String::new()
                        };
                        ui.label(
                            RichText::new(subtitle)
                                .size(11.0)
                                .color(if chat.typing {
                                    Palette::TEXT
                                } else {
                                    Palette::MUTED
                                }),
                        );
                    });
                });
            });
        ui.separator();
    }

    fn messages_ui(&mut self, ui: &mut egui::Ui) {
        let height = (ui.available_height() - 86.0).max(120.0);
        let mut area = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(height)
            .stick_to_bottom(true);
        if self.scroll_bottom {
            area = area.stick_to_bottom(true);
        }

        area.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add_space(6.0);
            let mut last_day = String::new();
            let count = self.messages.len();
            for index in 0..count {
                let ts = self.messages[index].ts;
                let day = day_label(ts);
                if day != last_day {
                    widgets::day_separator(ui, &day);
                    last_day = day;
                }
                self.bubble_ui(ui, index);
            }
            if self.messages.is_empty() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("Nessun messaggio. Scrivi qualcosa.")
                            .size(12.0)
                            .color(Palette::FAINT),
                    );
                });
            }
            ui.add_space(8.0);
        });
        self.scroll_bottom = false;
    }

    fn bubble_ui(&mut self, ui: &mut egui::Ui, index: usize) {
        let bubble = Bubble::from(&self.messages[index]);
        let max_width = (ui.available_width() * 0.76).clamp(170.0, 560.0);

        let layout = if bubble.own {
            egui::Layout::right_to_left(egui::Align::Max)
        } else {
            egui::Layout::left_to_right(egui::Align::Max)
        };

        ui.with_layout(layout, |ui| {
            ui.add_space(11.0);
            self.bubble_frame(ui, index, &bubble, max_width);
        });
        ui.add_space(3.0);
    }

    fn bubble_frame(&mut self, ui: &mut egui::Ui, index: usize, bubble: &Bubble, max_width: f32) {
        let fill = if bubble.own {
            Palette::OWN_BUBBLE
        } else {
            Palette::IN_BUBBLE
        };
        let text_color = if bubble.own {
            Palette::OWN_TEXT
        } else {
            Palette::IN_TEXT
        };
        let muted = if bubble.own {
            Color32::from_gray(120)
        } else {
            Palette::MUTED
        };
        let faint = if bubble.own {
            Color32::from_gray(180)
        } else {
            Color32::from_gray(110)
        };

        let response = egui::Frame::new()
            .fill(fill)
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(egui::Margin::symmetric(10, 7))
            .show(ui, |ui| {
                ui.set_max_width(max_width);

                if !bubble.own && self.messages[index].chat_id.ends_with("@g.us") {
                    let name = util::display_name(&bubble.sender_name, &bubble.sender);
                    ui.label(RichText::new(name).size(11.0).color(muted));
                    ui.add_space(2.0);
                }

                if let Some(quote) = bubble.quote.as_ref() {
                    egui::Frame::new()
                        .fill(if bubble.own {
                            Color32::from_gray(222)
                        } else {
                            Color32::from_gray(42)
                        })
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin::symmetric(8, 5))
                        .show(ui, |ui| {
                            ui.set_max_width(max_width - 40.0);
                            ui.label(
                                RichText::new(util::truncate(&quote.text, 70))
                                    .size(11.0)
                                    .color(if bubble.own {
                                        Color32::from_gray(70)
                                    } else {
                                        Palette::MUTED
                                    }),
                            );
                        });
                    ui.add_space(3.0);
                }

                if bubble.deleted {
                    ui.label(
                        RichText::new("Questo messaggio è stato eliminato")
                            .italics()
                            .size(13.0)
                            .color(muted),
                    );
                } else {
                    self.content_ui(ui, index, bubble, text_color, muted, faint);
                }

                ui.add_space(2.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if bubble.own {
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::new(15.0, 10.0), Sense::hover());
                        crate::icons::ticks(
                            ui,
                            rect,
                            bubble.status,
                            if matches!(bubble.status, Status::Read | Status::Played) {
                                text_color
                            } else {
                                muted
                            },
                        );
                    }
                    if bubble.edited {
                        ui.label(RichText::new("modificato").size(9.5).color(muted));
                    }
                    ui.label(
                        RichText::new(util::fmt_clock(bubble.ts))
                            .size(9.5)
                            .color(muted),
                    );
                    if let Some(reaction) = bubble.reaction.as_ref() {
                        if !reaction.is_empty() {
                            ui.label(RichText::new(reaction).size(12.0));
                        }
                    }
                });
            })
            .response;

        response.context_menu(|ui| {
            if ui.button("Rispondi").clicked() {
                self.actions.push(Action::Reply(index));
                ui.close();
            }
            if !bubble.text.is_empty() && ui.button("Copia testo").clicked() {
                self.actions.push(Action::CopyText(index));
                ui.close();
            }
            ui.menu_button("Reagisci", |ui| {
                ui.horizontal(|ui| {
                    for emoji in ["👍", "❤️", "😂", "😮", "😢", "🙏"] {
                        if ui.button(emoji).clicked() {
                            self.actions.push(Action::React(index, emoji.to_string()));
                            ui.close();
                        }
                    }
                });
            });
            if bubble.reaction.is_some() && ui.button("Rimuovi reazione").clicked() {
                self.actions.push(Action::ClearReaction(index));
                ui.close();
            }
            if bubble.own && ui.button("Elimina per tutti").clicked() {
                self.actions.push(Action::Revoke(index));
                ui.close();
            }
        });

        if response.double_clicked() {
            self.actions.push(Action::Reply(index));
        }
    }

    fn content_ui(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        bubble: &Bubble,
        text_color: Color32,
        muted: Color32,
        faint: Color32,
    ) {
        match bubble.kind {
            Kind::Text => {
                if !bubble.text.is_empty() {
                    ui.label(
                        RichText::new(&bubble.text)
                            .size(14.0)
                            .color(text_color),
                    );
                }
            }
            Kind::Image | Kind::Sticker => {
                if bubble.view_once && !bubble.played {
                    if self.view_once_tile(ui, index, false, text_color, muted) {
                        self.actions.push(Action::OpenImage(index));
                    }
                } else if let Some(texture) = self.thumb_texture(index) {
                    let size = fit_size(texture.size_vec2(), 268.0, 320.0);
                    let image = ui.add(
                        egui::Image::new(egui::load::SizedTexture::new(texture.id(), size))
                            .corner_radius(8)
                            .sense(Sense::click()),
                    );
                    if image.clicked() {
                        self.actions.push(Action::OpenImage(index));
                    }
                } else {
                    self.missing_media_ui(ui, index, "Foto", text_color, muted);
                }
            }
            Kind::Video => {
                if bubble.view_once && !bubble.played {
                    if self.view_once_tile(ui, index, true, text_color, muted) {
                        self.actions.push(Action::OpenVideo(index));
                    }
                } else if let Some(texture) = self.thumb_texture(index) {
                    let size = fit_size(texture.size_vec2(), 268.0, 320.0);
                    let response = ui.add(
                        egui::Image::new(egui::load::SizedTexture::new(texture.id(), size))
                            .corner_radius(8)
                            .sense(Sense::click()),
                    );
                    let center = response.rect.center();
                    ui.painter().circle_filled(
                        center,
                        21.0,
                        Color32::from_black_alpha(140),
                    );
                    crate::icons::Icon::Play.paint(
                        ui,
                        egui::Rect::from_center_size(center, Vec2::splat(20.0)),
                        Color32::WHITE,
                    );
                    if response.clicked() {
                        self.actions.push(Action::OpenVideo(index));
                    }
                } else {
                    self.missing_media_ui(ui, index, "Video", text_color, muted);
                }
            }
            Kind::Voice => {
                let playing = self
                    .audio
                    .as_ref()
                    .map(|state| state.id == bubble.id && state.player.is_playing())
                    .unwrap_or(false);
                let progress = self
                    .audio
                    .as_ref()
                    .filter(|state| state.id == bubble.id)
                    .map(|state| {
                        let duration = state.player.duration().max(0.001);
                        (state.player.position() / duration).clamp(0.0, 1.0) as f32
                    })
                    .unwrap_or(0.0);

                ui.horizontal(|ui| {
                    let icon = if playing {
                        crate::icons::Icon::Pause
                    } else {
                        crate::icons::Icon::Play
                    };
                    if icon.button(ui, 27.0, text_color).clicked() {
                        self.actions.push(Action::PlayAudio(index));
                    }
                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::new(150.0, 24.0), Sense::hover());
                    widgets::waveform(ui.painter(), rect, progress, text_color, faint);
                    ui.label(
                        RichText::new(util::human_duration(bubble.duration))
                            .size(10.5)
                            .color(muted),
                    );
                });
            }
            Kind::Audio => {
                ui.horizontal(|ui| {
                    let playing = self
                        .audio
                        .as_ref()
                        .map(|state| state.id == bubble.id && state.player.is_playing())
                        .unwrap_or(false);
                    let icon = if playing {
                        crate::icons::Icon::Pause
                    } else {
                        crate::icons::Icon::Play
                    };
                    if icon.button(ui, 27.0, text_color).clicked() {
                        self.actions.push(Action::PlayAudio(index));
                    }
                    ui.label(
                        RichText::new(if bubble.file_name.is_empty() {
                            "Audio".to_string()
                        } else {
                            util::truncate(&bubble.file_name, 24)
                        })
                        .size(12.5)
                        .color(text_color),
                    );
                });
            }
            Kind::Document => {
                let cached = self.has_cached(index);
                let row = ui.horizontal(|ui| {
                    crate::icons::Icon::Doc.paint(
                        ui,
                        egui::Rect::from_min_size(ui.cursor().min, Vec2::splat(26.0)),
                        text_color,
                    );
                    ui.add_space(30.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(if bubble.file_name.is_empty() {
                                "Documento".to_string()
                            } else {
                                util::truncate(&bubble.file_name, 30)
                            })
                            .size(12.5)
                            .color(text_color),
                        );
                        ui.label(
                            RichText::new(if cached {
                                "Apri".to_string()
                            } else {
                                util::human_size(bubble.file_size)
                            })
                            .size(10.5)
                            .color(muted),
                        );
                    });
                });
                if row.response.interact(Sense::click()).clicked() {
                    self.actions.push(Action::OpenDoc(index));
                }
            }
            Kind::Location => {
                ui.label(RichText::new("Posizione").size(13.0).color(text_color));
            }
            Kind::Contact => {
                ui.label(
                    RichText::new(if bubble.text.is_empty() {
                        "Contatto".to_string()
                    } else {
                        bubble.text.clone()
                    })
                    .size(13.0)
                    .color(text_color),
                );
            }
            _ => {
                if !bubble.text.is_empty() {
                    ui.label(RichText::new(&bubble.text).size(13.0).color(text_color));
                } else {
                    ui.label(RichText::new("Messaggio").size(13.0).color(muted));
                }
            }
        }

        if bubble.kind != Kind::Text && !bubble.text.is_empty() && !bubble.deleted {
            ui.add_space(2.0);
            ui.label(RichText::new(&bubble.text).size(13.5).color(text_color));
        }

    }

    fn view_once_tile(
        &self,
        ui: &mut egui::Ui,
        index: usize,
        video: bool,
        text_color: Color32,
        muted: Color32,
    ) -> bool {
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(224.0, 74.0), Sense::click());
        let fill = if text_color == Palette::OWN_TEXT {
            Color32::from_gray(228)
        } else {
            Color32::from_gray(40)
        };
        ui.painter().rect_filled(rect, egui::CornerRadius::same(8), fill);

        let icon = if video {
            crate::icons::Icon::Video
        } else {
            crate::icons::Icon::Lock
        };
        let icon_rect = egui::Rect::from_center_size(
            egui::Pos2::new(rect.left() + 34.0, rect.center().y),
            Vec2::splat(24.0),
        );
        icon.paint(ui, icon_rect, text_color);

        ui.painter().text(
            egui::Pos2::new(rect.left() + 58.0, rect.center().y - 8.0),
            egui::Align2::LEFT_CENTER,
            if video {
                "Video · una volta"
            } else {
                "Foto · una volta"
            },
            egui::FontId::proportional(12.5),
            text_color,
        );
        ui.painter().text(
            egui::Pos2::new(rect.left() + 58.0, rect.center().y + 10.0),
            egui::Align2::LEFT_CENTER,
            "Tocca per aprire",
            egui::FontId::proportional(10.5),
            muted,
        );

        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let _ = index;
        response.clicked()
    }

    fn missing_media_ui(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        label: &str,
        text_color: Color32,
        muted: Color32,
    ) {
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(224.0, 58.0), Sense::click());
        let fill = if text_color == Palette::OWN_TEXT {
            Color32::from_gray(230)
        } else {
            Color32::from_gray(38)
        };
        ui.painter().rect_filled(rect, egui::CornerRadius::same(8), fill);
        crate::icons::Icon::Download.paint(
            ui,
            egui::Rect::from_center_size(
                egui::Pos2::new(rect.left() + 30.0, rect.center().y),
                Vec2::splat(22.0),
            ),
            text_color,
        );
        ui.painter().text(
            egui::Pos2::new(rect.left() + 52.0, rect.center().y - 8.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(12.5),
            text_color,
        );
        ui.painter().text(
            egui::Pos2::new(rect.left() + 52.0, rect.center().y + 10.0),
            egui::Align2::LEFT_CENTER,
            "Scarica",
            egui::FontId::proportional(10.5),
            muted,
        );
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked() {
            self.actions.push(Action::Download(index));
        }
    }

    fn composer_ui(&mut self, ui: &mut egui::Ui) {
        if self.composer_focus
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        {
            self.send_text();
        }
        ui.separator();
        egui::Frame::new()
            .fill(Palette::PANEL)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                if self.reply.is_some() {
                    egui::Frame::new()
                        .fill(Palette::SURFACE)
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(util::truncate(&self.reply_label, 70))
                                        .size(11.5)
                                        .color(Palette::MUTED),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if crate::icons::Icon::Close
                                            .button(ui, 20.0, Palette::MUTED)
                                            .clicked()
                                        {
                                            self.reply = None;
                                            self.reply_label.clear();
                                        }
                                    },
                                );
                            });
                        });
                    ui.add_space(6.0);
                }

                if self.recorder.is_some() {
                    self.recorder_ui(ui);
                    return;
                }

                ui.horizontal(|ui| {
                    let mut pick = false;
                    let mut by_path = false;
                    let attach = crate::icons::Icon::Attach
                        .button(ui, 30.0, Palette::MUTED)
                        .on_hover_text("Allega un file");
                    egui::Popup::menu(&attach).show(|ui| {
                        ui.set_min_width(196.0);
                        if ui.button("Scegli un file…").clicked() {
                            pick = true;
                            ui.close();
                        }
                        if ui.button("Da percorso…").clicked() {
                            by_path = true;
                            ui.close();
                        }
                    });
                    if pick {
                        self.pick_file();
                    }
                    if by_path {
                        self.path_modal = true;
                    }

                    let response = ui.add(
                        egui::TextEdit::multiline(&mut self.composer)
                            .hint_text("Scrivi un messaggio")
                            .desired_width(f32::INFINITY)
                            .desired_rows(1)
                            .frame(
                                egui::Frame::new()
                                    .fill(Palette::SURFACE)
                                    .corner_radius(egui::CornerRadius::same(14))
                                    .inner_margin(egui::Margin::symmetric(12, 8)),
                            ),
                    );
                    self.composer_focus = response.has_focus();

                    if response.changed() {
                        self.notify_typing();
                    }

                    if self.composer.trim().is_empty() {
                        if crate::icons::Icon::Mic
                            .button(ui, 32.0, Palette::TEXT)
                            .on_hover_text("Registra un vocale")
                            .clicked()
                        {
                            self.start_recording();
                        }
                    } else if crate::icons::Icon::Send
                        .button(ui, 32.0, Palette::WHITE)
                        .on_hover_text("Invia")
                        .clicked()
                    {
                        self.send_text();
                    }
                });
            });
    }

    fn recorder_ui(&mut self, ui: &mut egui::Ui) {
        let seconds = self
            .recorder
            .as_ref()
            .map(|recorder| recorder.seconds())
            .unwrap_or(0.0);
        let level = self
            .recorder
            .as_ref()
            .map(|recorder| recorder.level())
            .unwrap_or(0.0);
        let blink = (seconds * 3.0) as i32 % 2 == 0;

        ui.horizontal(|ui| {
            if crate::icons::Icon::Trash
                .button(ui, 30.0, Palette::MUTED)
                .on_hover_text("Annulla")
                .clicked()
            {
                self.stop_recording(false);
            }

            ui.label(
                RichText::new(format!(
                    "● {}",
                    util::human_duration(seconds.max(0.0) as u32)
                ))
                .size(12.5)
                .color(if blink {
                    Palette::DANGER
                } else {
                    Color32::from_gray(90)
                }),
            );

            let (rect, _) = ui.allocate_exact_size(Vec2::new(140.0, 22.0), Sense::hover());
            widgets::waveform(ui.painter(), rect, level, Palette::TEXT, Color32::from_gray(70));

            if crate::icons::Icon::Stop
                .button(ui, 32.0, Palette::WHITE)
                .on_hover_text("Invia vocale")
                .clicked()
            {
                self.stop_recording(true);
            }
        });
    }

    fn pick_file(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let file = rfd::FileDialog::new()
                .set_title("Allega un file")
                .pick_file();
            let _ = tx.send(file);
        });
        self.attach_rx = Some(rx);
        self.ctx.request_repaint();
    }

    pub fn notify_typing(&mut self) {
        let now = Instant::now();
        let should = self
            .typing_sent
            .map(|last| now.duration_since(last) > Duration::from_millis(3500))
            .unwrap_or(true);
        if should && !self.composer.trim().is_empty() {
            if let Some(chat) = self.selected.clone() {
                if let Some(engine) = self.engine.as_ref() {
                    engine.send(Cmd::Typing {
                        chat,
                        composing: true,
                    });
                }
                self.typing_sent = Some(now);
                self.typing_last = now;
            }
        }
    }

    pub fn send_text(&mut self) {
        let Some(chat) = self.selected.clone() else {
            return;
        };
        let text = self.composer.trim().to_string();
        if text.is_empty() {
            return;
        }
        let local_id = format!("local-{}", crate::util::timestamp_ms());
        let reply = self.reply.take();
        let message = Message {
            id: local_id.clone(),
            chat_id: chat.clone(),
            from_me: true,
            ts: crate::util::now_ts(),
            kind: Kind::Text,
            text: text.clone(),
            status: Status::Pending,
            ..Default::default()
        };
        self.messages.push(message.clone());
        self.store.insert_message(&message).ok();
        self.touch_chat(&message);
        self.composer.clear();
        self.reply_label.clear();
        self.typing_sent = None;
        self.scroll_bottom = true;

        if let Some(engine) = self.engine.as_ref() {
            engine.send(Cmd::SendText {
                chat: chat.clone(),
                text,
                local_id,
                reply,
            });
            engine.send(Cmd::Typing {
                chat,
                composing: false,
            });
        }
    }

    pub fn send_file(&mut self, path: PathBuf, voice: Option<(u32, Vec<u8>)>) {
        let Some(chat) = self.selected.clone() else {
            self.note("Apri una chat prima di allegare un file");
            return;
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.note(format!("Impossibile leggere il file: {error}"));
                return;
            }
        };
        if bytes.is_empty() {
            self.note("Il file è vuoto");
            return;
        }
        if bytes.len() > 100 * 1024 * 1024 {
            self.note("File troppo grande: il limite di WhatsApp è 100 MB");
            return;
        }

        let is_voice = voice.is_some();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file")
            .to_string();
        let lower = name.to_lowercase();

        let kind = if is_voice {
            Kind::Voice
        } else if media::is_image_path(&path) {
            Kind::Image
        } else if media::is_video_path(&path) || lower.ends_with(".gif") {
            Kind::Video
        } else if media::is_audio_path(&path) {
            Kind::Audio
        } else {
            Kind::Document
        };

        let preview = match kind {
            Kind::Image => media::make_thumbnail(&bytes, 480),
            Kind::Video => media::video_thumbnail(&path),
            _ => None,
        };
        let duration = match voice.as_ref() {
            Some((seconds, _)) => *seconds,
            None => {
                if kind == Kind::Video {
                    media::probe_video(&path).duration.round() as u32
                } else {
                    0
                }
            }
        };

        let local_id = format!("local-{}", crate::util::timestamp_ms());
        let reply = self.reply.take();
        let message = Message {
            id: local_id.clone(),
            chat_id: chat.clone(),
            from_me: true,
            ts: crate::util::now_ts(),
            kind,
            status: Status::Pending,
            file_name: name,
            file_size: bytes.len() as u64,
            mime: media::mime_for_path(&path).to_string(),
            duration,
            thumb: preview.clone(),
            local_preview: preview,
            ..Default::default()
        };

        if let Some(preview) = message.local_preview.as_ref() {
            if let Some((image, _, _)) = super::gfx::image_from_bytes(preview) {
                let ctx = self.ctx.clone();
                self.gfx
                    .insert(&ctx, format!("local:{}", message.id), image, false);
            }
        }

        self.messages.push(message.clone());
        self.store.insert_message(&message).ok();
        self.touch_chat(&message);
        self.reply_label.clear();
        self.scroll_bottom = true;

        if let Some(engine) = self.engine.as_ref() {
            engine.send(Cmd::SendFile {
                chat,
                path,
                caption: String::new(),
                local_id,
                reply,
                voice,
            });
        }
    }

    fn touch_chat(&mut self, message: &Message) {
        let chat_id = message.chat_id.clone();
        let preview = message.preview();
        if let Some(index) = self.index.get(&chat_id).copied() {
            self.chats[index].last_ts = message.ts;
            self.chats[index].preview = preview.clone();
        }
        self.store
            .touch_chat(&chat_id, message.ts, &preview)
            .ok();
        self.sort_chats();
    }

    fn thumb_texture(&mut self, index: usize) -> Option<TextureHandle> {
        let message = self.messages.get(index)?;
        let key = if message.local_preview.is_some() {
            format!("local:{}", message.id)
        } else {
            format!("thumb:{}", message.id)
        };
        if let Some(texture) = self.gfx.get(&key) {
            return Some(texture);
        }
        let bytes = message
            .local_preview
            .clone()
            .or_else(|| message.thumb.clone())?;
        let (image, _, _) = super::gfx::image_from_bytes(&bytes)?;
        let ctx = self.ctx.clone();
        self.gfx.insert(&ctx, key.clone(), image, false);
        self.gfx.get(&key)
    }
}

fn fit_size(size: Vec2, max_width: f32, max_height: f32) -> Vec2 {
    if size.x <= 0.0 || size.y <= 0.0 {
        return Vec2::new(220.0, 140.0);
    }
    let scale = (max_width / size.x).min(max_height / size.y).min(1.4);
    Vec2::new(size.x * scale, size.y * scale)
}

pub fn cached_path(app: &App, chat: &str, id: &str, ext: &str) -> PathBuf {
    app.paths.cached_media(chat, id, ext)
}

fn day_label(ts: i64) -> String {
    let Some(dt) = chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0) else {
        return String::new();
    };
    let local = dt.with_timezone(&chrono::Local);
    let today = chrono::Local::now().date_naive();
    let day = local.date_naive();
    if day == today {
        "Oggi".to_string()
    } else if Some(day) == today.pred_opt() {
        "Ieri".to_string()
    } else {
        local.format("%d/%m/%Y").to_string()
    }
}

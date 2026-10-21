//! Conversazione: intestazione, messaggi, casella di composizione.

use std::path::PathBuf;

use egui::{Color32, RichText, Sense, TextureHandle, Vec2};

use crate::engine::Cmd;
use crate::media;
use crate::model::{Kind, Message, Quote, Status};
use crate::theme::pal;
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
    animated: bool,
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
            animated: message.animated,
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
                .fill(pal().surface)
                .inner_margin(egui::Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(12.0).color(pal().muted));
                        ui.label(
                            RichText::new(format!("Riconnessione… {}", self.status_text))
                                .size(11.5)
                                .color(pal().muted),
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
                        .color(pal().muted),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("oppure avviane una nuova con +")
                        .size(11.5)
                        .color(pal().faint),
                );
            });
        }
    }

    fn header_ui(&mut self, ui: &mut egui::Ui, chat: &crate::model::Chat) {
        let title = self.title_for(chat);
        let texture = self.avatars.get(&chat.id).cloned();
        if texture.is_none() {
            self.request_avatar(&chat.id);
        }
        let in_call = self.call.is_some();
        let narrow = self.narrow;
        // `Some(true)` = videochiamata, `Some(false)` = vocale.
        let mut place_call: Option<bool> = None;

        egui::Frame::new()
            .fill(pal().panel)
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if narrow
                        && crate::icons::Icon::Back
                            .button(ui, 30.0, pal().text)
                            .on_hover_text("Torna alle chat")
                            .clicked()
                    {
                        self.selected = None;
                        self.messages.clear();
                        self.reply = None;
                        self.reply_label.clear();
                    }

                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
                    widgets::avatar_tex(
                        ui,
                        rect,
                        texture.as_ref(),
                        &chat.name,
                        &chat.id,
                        chat.is_group,
                        chat.online,
                    );

                    ui.add_space(5.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(&title)
                                .size(15.0)
                                .strong()
                                .color(pal().text),
                        );
                        let mut subtitle = if chat.typing {
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
                        // Il numero aiuta a riconoscere chi non è in rubrica.
                        let line = phone_line(&chat.id);
                        if !chat.is_group && !line.is_empty() && title != line {
                            if subtitle.is_empty() {
                                subtitle = line;
                            } else {
                                subtitle = format!("{subtitle} · {line}");
                            }
                        }
                        if !subtitle.is_empty() {
                            ui.label(
                                RichText::new(util::truncate(&subtitle, 58))
                                    .size(11.0)
                                    .color(if chat.typing {
                                        pal().text
                                    } else {
                                        pal().muted
                                    }),
                            );
                        }
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(2.0);
                        let enabled = !chat.is_group && !in_call;
                        let color = if enabled { pal().text } else { pal().faint };
                        let hover = if chat.is_group {
                            "Le chiamate di gruppo non sono supportate"
                        } else if in_call {
                            "Chiamata in corso"
                        } else {
                            "Videochiamata"
                        };
                        if crate::icons::Icon::Video
                            .button(ui, 32.0, color)
                            .on_hover_text(hover)
                            .clicked()
                            && enabled
                        {
                            place_call = Some(true);
                        }
                        let hover = if chat.is_group {
                            "Le chiamate di gruppo non sono supportate"
                        } else if in_call {
                            "Chiamata in corso"
                        } else {
                            "Chiamata vocale"
                        };
                        if crate::icons::Icon::Phone
                            .button(ui, 32.0, color)
                            .on_hover_text(hover)
                            .clicked()
                            && enabled
                        {
                            place_call = Some(false);
                        }
                    });
                });
            });
        ui.separator();

        if let Some(video) = place_call {
            if chat.is_group {
                self.note("Le chiamate di gruppo non sono supportate");
            } else {
                self.start_call(video);
            }
        }
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
                // Messaggi ravvicinati dello stesso interlocutore stanno più
                // stretti: si legge come un discorso, non come un elenco.
                let tight = index > 0 && grouped(&self.messages[index - 1], &self.messages[index]);
                self.bubble_ui(ui, index, tight);
            }
            if self.messages.is_empty() {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("Nessun messaggio. Scrivi qualcosa.")
                            .size(12.0)
                            .color(pal().faint),
                    );
                });
            }
            ui.add_space(8.0);
        });
        self.scroll_bottom = false;
    }

    fn bubble_ui(&mut self, ui: &mut egui::Ui, index: usize, tight: bool) {
        // Le chiamate non sono fumetti: una pastiglia centrata, come su
        // WhatsApp, con l'icona della cornetta e la durata.
        if self.messages[index].kind == Kind::System && is_call_note(&self.messages[index].text) {
            call_note(ui, &self.messages[index].text, self.messages[index].ts);
            return;
        }
        let bubble = Bubble::from(&self.messages[index]);
        // Larghezza giusta: le righe non corrono da un bordo all'altro, ma i
        // messaggi brevi restano stretti invece di allargarsi a vuoto.
        let max_width = (ui.available_width() * 0.72).clamp(200.0, 520.0);

        let layout = if bubble.own {
            egui::Layout::right_to_left(egui::Align::Max)
        } else {
            egui::Layout::left_to_right(egui::Align::Max)
        };

        ui.with_layout(layout, |ui| {
            ui.add_space(12.0);
            self.bubble_frame(ui, index, &bubble, max_width);
        });
        ui.add_space(if tight { 1.5 } else { 4.0 });
    }

    /// Larghezza del contenuto (senza i margini del fumetto): si misura il
    /// testo con lo stesso font che verrà disegnato, così un "ok" resta un
    /// fumetto da due dita e un paragrafo arriva al massimo stabilito.
    fn bubble_width(&mut self, ui: &egui::Ui, index: usize, bubble: &Bubble, max_width: f32) -> Option<f32> {
        if bubble.kind == Kind::Sticker && !bubble.view_once {
            return None;
        }
        let room = (max_width - 22.0).max(80.0);
        let mut width: f32 = 62.0;
        let mut text = bubble.text.as_str();
        if bubble.kind == Kind::Text && bubble.view_once {
            text = "";
        }
        if !text.is_empty() {
            let size = if bubble.kind == Kind::Text { 14.0 } else { 13.5 };
            width = width.max(measure_text(ui, text, size, room));
        }
        match bubble.kind {
            Kind::Image | Kind::Video | Kind::Gif => {
                width = width.max(if self.thumb_texture(index).is_some() {
                    268.0
                } else {
                    224.0
                });
            }
            Kind::Voice => width = width.max(248.0),
            Kind::Audio => width = width.max(238.0),
            Kind::Document => width = width.max(258.0),
            Kind::Location | Kind::Contact => width = width.max(196.0),
            _ => {}
        }
        if bubble.view_once {
            // La piastrella "apri una volta" ha una misura fissa: il fumetto
            // deve contenerla, non schiacciarla.
            width = if bubble.played {
                width.max(150.0)
            } else if matches!(bubble.kind, Kind::Image | Kind::Video | Kind::Gif)
                && self.thumb_texture(index).is_some()
            {
                // La miniatura è larga 252 più i margini interni del fumetto.
                width.max(276.0)
            } else {
                width.max(246.0)
            };
        }
        if let Some(quote) = bubble.quote.as_ref() {
            let quoted = measure_text(ui, &util::truncate(&quote.text, 70), 11.0, (room - 16.0).max(60.0));
            width = width.max(quoted + 16.0);
        }
        Some(width.clamp(62.0, room))
    }

    fn bubble_frame(&mut self, ui: &mut egui::Ui, index: usize, bubble: &Bubble, max_width: f32) {
        // Gli sticker non stanno in una bolla: si vedono come su WhatsApp.
        let bare = bubble.kind == Kind::Sticker && !bubble.view_once;
        let content_width = self.bubble_width(ui, index, bubble, max_width);
        let fill = if bare {
            Color32::TRANSPARENT
        } else if bubble.own {
            pal().own_bubble
        } else {
            pal().in_bubble
        };
        let text_color = if bubble.own && !bare {
            pal().own_text
        } else {
            pal().in_text
        };
        let muted = if bubble.own && !bare {
            crate::theme::mix(pal().own_text, pal().own_bubble, 0.44)
        } else {
            pal().muted
        };
        let faint = if bubble.own && !bare {
            crate::theme::mix(pal().own_text, pal().own_bubble, 0.30)
        } else {
            pal().faint
        };

        let response = egui::Frame::new()
            .fill(fill)
            // Un filo di bordo: sul tema chiaro il fumetto bianco si stacca
            // dallo sfondo solo grazie a questo.
            .stroke(if bare {
                egui::Stroke::NONE
            } else {
                egui::Stroke::new(1.0, crate::theme::mix(fill, pal().border, 0.55))
            })
            .corner_radius(egui::CornerRadius::same(if bare { 0 } else { 14 }))
            .inner_margin(egui::Margin::symmetric(
                if bare { 0 } else { 11 },
                if bare { 2 } else { 8 },
            ))
            .show(ui, |ui| {
                match content_width {
                    Some(width) => ui.set_width(width),
                    None => ui.set_max_width(max_width - 22.0),
                }
                // Dentro il fumetto si va per righe: senza questo il layout
                // orizzontale del genitore metterebbe citazione e testo uno
                // accanto all'altro invece che uno sotto l'altro.
                ui.vertical(|ui| {

                    if !bubble.own && self.messages[index].chat_id.ends_with("@g.us") {
                        let name = self.name_of(&bubble.sender);
                        ui.label(RichText::new(name).size(11.0).color(muted));
                        ui.add_space(2.0);
                    }

                    if let Some(quote) = bubble.quote.as_ref() {
                        egui::Frame::new()
                            .fill(if bubble.own {
                                crate::theme::mix(pal().own_bubble, pal().own_text, 0.10)
                            } else {
                                crate::theme::mix(pal().in_bubble, pal().in_text, 0.08)
                            })
                            .corner_radius(egui::CornerRadius::same(6))
                            .inner_margin(egui::Margin::symmetric(8, 5))
                            .show(ui, |ui| {
                                ui.set_max_width(max_width - 40.0);
                                let who = if quote.sender.is_empty() {
                                    String::new()
                                } else {
                                    self.name_of(&quote.sender)
                                };
                                if !who.is_empty() {
                                    ui.label(
                                        RichText::new(who)
                                            .size(10.5)
                                            .strong()
                                            .color(if bubble.own {
                                                pal().own_text
                                            } else {
                                                pal().text
                                            }),
                                    );
                                }
                                ui.label(
                                    RichText::new(util::truncate(&quote.text, 70))
                                        .size(11.0)
                                        .color(if bubble.own {
                                            crate::theme::mix(pal().own_text, pal().own_bubble, 0.30)
                                        } else {
                                            pal().muted
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
                    });
                });
            })
            .response;

        // Reazione: pillola appesa al bordo del fumetto, fuori dal testo.
        if let Some(reaction) = bubble.reaction.as_ref().filter(|text| !text.is_empty()) {
            let galley = ui.painter().layout_no_wrap(
                reaction.clone(),
                egui::FontId::proportional(11.0),
                pal().text,
            );
            let size = Vec2::new(galley.size().x + 14.0, galley.size().y + 5.0);
            let left = if bubble.own {
                response.rect.right() - size.x
            } else {
                response.rect.left()
            };
            let rect = egui::Rect::from_min_size(
                egui::Pos2::new(left, response.rect.bottom() - 7.0),
                size,
            );
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(8), pal().panel);
            ui.painter().rect_stroke(
                rect,
                egui::CornerRadius::same(8),
                egui::Stroke::new(1.0, pal().border),
                egui::StrokeKind::Inside,
            );
            ui.painter()
                .galley(rect.min + Vec2::new(7.0, 2.5), galley, pal().text);
            ui.add_space(7.0);
        }

        if bubble.own && bubble.status == Status::Failed {
            let failed = ui
                .with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new("non inviato · riprova")
                                .size(10.0)
                                .color(pal().danger),
                        )
                        .sense(egui::Sense::click()),
                    )
                    .on_hover_text("Tocca per riprovare l'invio")
                    .clicked()
                })
                .inner;
            if failed {
                self.actions.push(Action::Retry(index));
            }
        }

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
                if bubble.view_once && !bubble.played {
                    if self.view_once_tile(ui, index, Kind::Text, text_color, muted) {
                        self.actions.push(Action::RevealOnce(index));
                    }
                } else if bubble.view_once && bubble.played && bubble.text.is_empty() {
                    self.once_seen_label(ui, Kind::Text, muted);
                } else if !bubble.text.is_empty() {
                    ui.label(
                        RichText::new(&bubble.text)
                            .size(14.0)
                            .color(text_color),
                    );
                }
            }
            Kind::Sticker => {
                if let Some(texture) = self.thumb_texture(index) {
                    let size = fit_size(texture.size_vec2(), 150.0, 150.0);
                    let image = ui.add(
                        egui::Image::new(egui::load::SizedTexture::new(texture.id(), size))
                            .corner_radius(4)
                            .sense(Sense::click()),
                    );
                    if image.clicked() {
                        self.actions.push(Action::OpenImage(index));
                    }
                    if bubble.animated {
                        ui.label(RichText::new("sticker animato").size(9.5).color(faint));
                    }
                } else {
                    self.missing_media_ui(ui, index, "Sticker", text_color, muted);
                }
            }
            Kind::Image => {
                if bubble.view_once && !bubble.played {
                    if self.view_once_tile(ui, index, bubble.kind, text_color, muted) {
                        self.actions.push(Action::RevealOnce(index));
                    }
                } else if bubble.view_once && bubble.played {
                    self.once_seen_label(ui, bubble.kind, muted);
                } else if let Some(texture) = self.thumb_texture(index) {
                    let room = ui.available_width().clamp(160.0, 268.0);
                    let size = fit_size(texture.size_vec2(), room, 320.0);
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
            Kind::Video | Kind::Gif => {
                if bubble.view_once && !bubble.played {
                    if self.view_once_tile(ui, index, bubble.kind, text_color, muted) {
                        self.actions.push(Action::RevealOnce(index));
                    }
                } else if bubble.view_once && bubble.played {
                    self.once_seen_label(ui, bubble.kind, muted);
                } else if let Some(texture) = self.thumb_texture(index) {
                    let room = ui.available_width().clamp(160.0, 268.0);
                    let size = fit_size(texture.size_vec2(), room, 320.0);
                    let response = ui.add(
                        egui::Image::new(egui::load::SizedTexture::new(texture.id(), size))
                            .corner_radius(8)
                            .sense(Sense::click()),
                    );
                    let center = response.rect.center();
                    if bubble.kind == Kind::Gif {
                        let badge = egui::Rect::from_center_size(
                            egui::Pos2::new(center.x, response.rect.bottom() - 16.0),
                            Vec2::new(34.0, 18.0),
                        );
                        ui.painter().rect_filled(
                            badge,
                            egui::CornerRadius::same(4),
                            Color32::from_black_alpha(150),
                        );
                        ui.painter().text(
                            badge.center(),
                            egui::Align2::CENTER_CENTER,
                            "GIF",
                            egui::FontId::proportional(10.0),
                            Color32::WHITE,
                        );
                        if response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                    } else {
                        ui.painter().circle_filled(center, 21.0, Color32::from_black_alpha(140));
                        crate::icons::Icon::Play.paint(
                            ui,
                            egui::Rect::from_center_size(center, Vec2::splat(20.0)),
                            Color32::WHITE,
                        );
                    }
                    if response.clicked() {
                        self.actions.push(Action::OpenVideo(index));
                    }
                } else {
                    self.missing_media_ui(ui, index, "Video", text_color, muted);
                }
            }
            Kind::Voice | Kind::Audio if bubble.view_once && !bubble.played => {
                if self.view_once_tile(ui, index, bubble.kind, text_color, muted) {
                    self.actions.push(Action::RevealOnce(index));
                }
            }
            Kind::Voice | Kind::Audio if bubble.view_once && bubble.played => {
                self.once_seen_label(ui, bubble.kind, muted);
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
            _ if bubble.view_once && !bubble.played => {
                if self.view_once_tile(ui, index, bubble.kind, text_color, muted) {
                    self.actions.push(Action::RevealOnce(index));
                }
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
            ui.add_space(3.0);
            ui.label(RichText::new(&bubble.text).size(13.5).color(text_color));
        }

    }

    fn view_once_tile(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        kind: Kind,
        text_color: Color32,
        muted: Color32,
    ) -> bool {
        // Foto e video mostrano la miniatura vera, velata: si capisce cosa
        // c'è senza aprirlo (e senza scaricarlo).
        let preview = match kind {
            Kind::Image | Kind::Video | Kind::Gif => self.thumb_texture(index),
            _ => None,
        };
        if let Some(texture) = preview {
            let room = ui.available_width().clamp(160.0, 252.0);
            let size = fit_size(texture.size_vec2(), room, 300.0);
            let response = ui.add(
                egui::Image::new(egui::load::SizedTexture::new(texture.id(), size))
                    .corner_radius(10)
                    .sense(Sense::click()),
            );
            let rect = response.rect;
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(10), Color32::from_black_alpha(90));
            let center = rect.center();
            ui.painter()
                .circle_filled(center, 21.0, Color32::from_black_alpha(150));
            crate::icons::Icon::Lock.paint(
                ui,
                egui::Rect::from_center_size(center, Vec2::splat(19.0)),
                Color32::WHITE,
            );
            ui.painter().text(
                egui::Pos2::new(rect.left() + 10.0, rect.bottom() - 12.0),
                egui::Align2::LEFT_CENTER,
                if kind == Kind::Image {
                    "Foto · apri una volta"
                } else {
                    "Video · apri una volta"
                },
                egui::FontId::proportional(10.5),
                Color32::WHITE,
            );
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            return response.clicked();
        }

        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(224.0, 74.0), Sense::click());
        let fill = if text_color == pal().own_text {
            crate::theme::mix(pal().own_bubble, pal().own_text, 0.09)
        } else {
            crate::theme::mix(pal().in_bubble, pal().in_text, 0.08)
        };
        ui.painter().rect_filled(rect, egui::CornerRadius::same(8), fill);

        let icon = match kind {
            Kind::Video | Kind::Gif => crate::icons::Icon::Video,
            Kind::Voice | Kind::Audio => crate::icons::Icon::Play,
            _ => crate::icons::Icon::Lock,
        };
        let icon_rect = egui::Rect::from_center_size(
            egui::Pos2::new(rect.left() + 34.0, rect.center().y),
            Vec2::splat(24.0),
        );
        icon.paint(ui, icon_rect, text_color);

        ui.painter().text(
            egui::Pos2::new(rect.left() + 58.0, rect.center().y - 8.0),
            egui::Align2::LEFT_CENTER,
            once_title(kind),
            egui::FontId::proportional(12.5),
            text_color,
        );
        ui.painter().text(
            egui::Pos2::new(rect.left() + 58.0, rect.center().y + 10.0),
            egui::Align2::LEFT_CENTER,
            once_hint(kind),
            egui::FontId::proportional(10.5),
            muted,
        );

        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        response.clicked()
    }

    /// Riga per un contenuto "una volta sola" già aperto.
    fn once_seen_label(&self, ui: &mut egui::Ui, kind: Kind, muted: Color32) {
        ui.label(
            RichText::new(format!("{} · già aperto", once_title(kind)))
                .size(12.0)
                .italics()
                .color(muted),
        );
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
        let fill = if text_color == pal().own_text {
            crate::theme::mix(pal().own_bubble, pal().own_text, 0.08)
        } else {
            crate::theme::mix(pal().in_bubble, pal().in_text, 0.07)
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
            .fill(pal().panel)
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                if self.reply.is_some() {
                    egui::Frame::new()
                        .fill(pal().surface)
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(util::truncate(&self.reply_label, 70))
                                        .size(11.5)
                                        .color(pal().muted),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if crate::icons::Icon::Close
                                            .button(ui, 20.0, pal().muted)
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
                        .button(ui, 30.0, pal().muted)
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
                            .hint_text("Scrivi un messaggio · Invio per inviare")
                            .desired_width(f32::INFINITY)
                            .desired_rows(1)
                            .frame(
                                egui::Frame::new()
                                    .fill(pal().surface)
                                    .stroke(egui::Stroke::new(1.0, pal().border))
                                    .corner_radius(egui::CornerRadius::same(16))
                                    .inner_margin(egui::Margin::symmetric(13, 9)),
                            ),
                    );
                    self.composer_focus = response.has_focus();

                    if response.changed() {
                        self.notify_typing();
                    }

                    if self.composer.trim().is_empty() {
                        if crate::icons::Icon::Mic
                            .button(ui, 32.0, pal().text)
                            .on_hover_text("Registra un vocale")
                            .clicked()
                        {
                            self.start_recording();
                        }
                    } else if crate::icons::Icon::Send
                        .filled_button(ui, 34.0, pal().panel, pal().text)
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
                .button(ui, 30.0, pal().muted)
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
                .color(if blink { pal().danger } else { pal().faint }),
            );

            let (rect, _) = ui.allocate_exact_size(Vec2::new(140.0, 22.0), Sense::hover());
            widgets::waveform(ui.painter(), rect, level, pal().text, pal().faint);

            if crate::icons::Icon::Stop
                .button(ui, 32.0, pal().white)
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

    /// L'app non annuncia "sta scrivendo": il comando esiste ancora nel
    /// motore ma non viene più inviato. Il metodo resta perché la casella di
    /// composizione lo chiama a ogni modifica.
    pub fn notify_typing(&mut self) {}


    pub fn send_text(&mut self) {
        if self.demo {
            self.note("Modalità dimostrazione: invio disattivato");
            return;
        }
        let Some(chat) = self.selected.clone() else {
            return;
        };
        let text = self.composer.trim().to_string();
        if text.is_empty() {
            return;
        }
        let local_id = crate::util::new_message_id();
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
        }
    }

    pub fn send_file(&mut self, path: PathBuf, voice: Option<(u32, Vec<u8>)>) {
        if self.demo {
            self.note("Modalità dimostrazione: invio disattivato");
            return;
        }
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
        let kind = if is_voice {
            Kind::Voice
        } else if media::is_gif_path(&path) {
            Kind::Gif
        } else if media::is_image_path(&path) {
            Kind::Image
        } else if media::is_video_path(&path) {
            Kind::Video
        } else if media::is_audio_path(&path) {
            Kind::Audio
        } else {
            Kind::Document
        };

        let preview = match kind {
            Kind::Image => media::make_thumbnail(&bytes, 480),
            Kind::Video | Kind::Gif => media::video_thumbnail(&path),
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

        let local_id = crate::util::new_message_id();
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
        let preview = message.chat_preview();
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
        let (key, bytes) = {
            let message = self.messages.get(index)?;
            let key = if message.local_preview.is_some() {
                format!("local:{}", message.id)
            } else {
                format!("thumb:{}", message.id)
            };
            if self.gfx.get(&key).is_some() {
                return self.gfx.get(&key);
            }
            let bytes = message
                .local_preview
                .clone()
                .or_else(|| message.thumb.clone())?;
            (key, bytes)
        };
        let (image, _, _) = super::gfx::image_from_bytes(&bytes)?;
        let ctx = self.ctx.clone();
        self.gfx.insert(&ctx, key.clone(), image, false);
        self.gfx.get(&key)
    }
}

fn is_call_note(text: &str) -> bool {
    text.starts_with("Chiamata") || text.starts_with("Videochiamata")
}

fn call_note(ui: &mut egui::Ui, text: &str, ts: i64) {
    ui.add_space(4.0);
    let label = format!("{} · {}", util::truncate(text, 52), util::fmt_clock(ts));
    let galley = ui.painter().layout_no_wrap(
        label,
        egui::FontId::proportional(11.0),
        pal().muted,
    );
    let width = (galley.size().x + 42.0).min(ui.available_width() - 20.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
    let plate = egui::Rect::from_center_size(rect.center(), Vec2::new(width, 22.0));
    ui.painter()
        .rect_filled(plate, egui::CornerRadius::same(11), pal().surface);
    let icon = egui::Rect::from_center_size(
        egui::Pos2::new(plate.left() + 14.0, plate.center().y),
        Vec2::splat(13.0),
    );
    crate::icons::Icon::Phone.paint(ui, icon, pal().muted);
    ui.painter().galley(
        egui::Pos2::new(icon.right() + 7.0, plate.center().y - galley.size().y * 0.5),
        galley,
        pal().muted,
    );
    ui.add_space(4.0);
}

/// Numero leggibile di un JID, quando il JID è un numero.
fn phone_line(jid: &str) -> String {
    let user = util::jid_user(jid);
    if !user.is_empty() && user.chars().all(|c| c.is_ascii_digit()) {
        util::phone_display(&user)
    } else {
        String::new()
    }
}

fn once_title(kind: Kind) -> &'static str {
    match kind {
        Kind::Video | Kind::Gif => "Video · una volta",
        Kind::Voice | Kind::Audio => "Vocale · una volta",
        Kind::Text => "Messaggio · una volta",
        _ => "Foto · una volta",
    }
}

fn once_hint(kind: Kind) -> &'static str {
    match kind {
        Kind::Voice | Kind::Audio => "Tocca per ascoltare",
        Kind::Text => "Tocca per leggere",
        _ => "Tocca per aprire",
    }
}

/// Due messaggi che su WhatsApp sembrano un unico discorso: stesso autore,
/// stesso giorno, meno di cinque minuti di distanza.
fn grouped(previous: &Message, current: &Message) -> bool {
    if previous.from_me != current.from_me || previous.sender != current.sender {
        return false;
    }
    if previous.kind == Kind::System || current.kind == Kind::System {
        return false;
    }
    if previous.chat_id.ends_with("@g.us") && !previous.from_me {
        return false;
    }
    let gap = current.ts - previous.ts;
    (0..300).contains(&gap) && day_label(previous.ts) == day_label(current.ts)
}

/// Larghezza del testo con il font che si vedrà davvero.
fn measure_text(ui: &egui::Ui, text: &str, size: f32, wrap: f32) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let galley = ui.painter().layout(
        text.to_string(),
        egui::FontId::proportional(size),
        Color32::WHITE,
        wrap.max(40.0),
    );
    galley.size().x.ceil()
}

fn fit_size(size: Vec2, max_width: f32, max_height: f32) -> Vec2 {
    if size.x <= 0.0 || size.y <= 0.0 {
        return Vec2::new(220.0, 140.0);
    }
    let scale = (max_width / size.x).min(max_height / size.y).min(1.4);
    Vec2::new(size.x * scale, size.y * scale)
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

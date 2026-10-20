//! Lettori media, impostazioni e finestre di servizio.

use std::path::{Path, PathBuf};
use std::time::Instant;

use egui::{Color32, RichText, Sense, Vec2};

use crate::engine::Cmd;
use crate::media;
use crate::model::{Chat, Kind, Message};
use crate::play::audio::AudioPlayer;
use crate::play::record::Recorder;
use crate::play::video::VideoPlayer;
use crate::theme::Palette;
use crate::util;

use super::gfx;
use super::{AudioState, Pending, VideoState, App};

impl App {
    pub fn has_cached(&self, index: usize) -> bool {
        let Some(message) = self.messages.get(index) else {
            return false;
        };
        self.paths
            .cached_media(&message.chat_id, &message.id, message.media_ext())
            .exists()
    }

    fn cache_path(&self, message: &Message) -> PathBuf {
        self.paths
            .cached_media(&message.chat_id, &message.id, message.media_ext())
    }

    fn request(&mut self, message: &Message, pending: Pending) {
        if message.payload.is_empty() {
            self.note("Contenuto non disponibile su questo dispositivo");
            return;
        }
        self.next_req += 1;
        let req = self.next_req;
        self.pending.insert(req, pending);
        if let Some(engine) = self.engine.as_ref() {
            engine.send(Cmd::Download {
                req,
                chat: message.chat_id.clone(),
                id: message.id.clone(),
                payload: message.payload.clone(),
                view_once: message.view_once,
            });
        }
    }

    pub fn open_image(&mut self, index: usize) {
        let Some(message) = self.messages.get(index).cloned() else {
            return;
        };
        if message.view_once && message.played {
            self.note("Questa foto è già stata aperta");
            return;
        }
        let path = self.cache_path(&message);
        if path.exists() {
            if let Ok(bytes) = std::fs::read(&path) {
                if let Some((image, _, _)) = gfx::image_from_bytes(&bytes) {
                    let key = format!("full:{}", message.id);
                    let ctx = self.ctx.clone();
                    self.gfx.insert(&ctx, key.clone(), image, false);
                    self.viewer_image = self.gfx.get(&key);
                    self.viewer = Some(message.id.clone());
                    self.view_view_once = message.view_once;
                    self.viewer_loading = false;
                    return;
                }
            }
        }
        self.viewer_loading = true;
        self.view_view_once = message.view_once;
        self.request(
            &message,
            Pending::Image {
                chat: message.chat_id.clone(),
                id: message.id.clone(),
                open: true,
                view_once: message.view_once,
            },
        );
    }

    pub fn open_video(&mut self, index: usize) {
        let Some(message) = self.messages.get(index).cloned() else {
            return;
        };
        if message.view_once && message.played {
            self.note("Questo video è già stato aperto");
            return;
        }
        let path = self.cache_path(&message);
        if path.exists() {
            if self.config.system_player {
                self.play_externally(&path);
                return;
            }
            match VideoPlayer::open(&path) {
                Ok(player) => {
                    self.view_view_once = message.view_once;
                    self.video = Some(VideoState {
                        id: message.id.clone(),
                        player,
                        texture: None,
                    });
                    return;
                }
                Err(error) => {
                    // Succede senza ffmpeg o con un codec che non sappiamo
                    // decodificare: meglio aprire il file col player del desktop.
                    self.note(format!("{error} — apro con il player di sistema"));
                    self.play_externally(&path);
                    if message.view_once {
                        self.mark_played(&message.id, true);
                    }
                    return;
                }
            }
        }
        self.viewer_loading = true;
        self.request(
            &message,
            Pending::Video {
                chat: message.chat_id.clone(),
                id: message.id.clone(),
                view_once: message.view_once,
            },
        );
    }

    pub fn toggle_audio(&mut self, index: usize) {
        let Some(message) = self.messages.get(index).cloned() else {
            return;
        };
        if let Some(state) = self.audio.as_mut() {
            if state.id == message.id {
                if state.player.is_playing() {
                    state.player.pause();
                } else {
                    state.player.play();
                }
                return;
            }
        }
        if let Some(previous) = self.audio.take() {
            self.mark_played(&previous.id, true);
        }

        let path = self.cache_path(&message);
        if path.exists() {
            if self.config.system_player {
                self.play_externally(&path);
                self.mark_played(&message.id, true);
                return;
            }
            match AudioPlayer::open(&path) {
                Ok(player) => {
                    player.set_volume(1.0);
                    self.audio = Some(AudioState {
                        id: message.id.clone(),
                        player,
                    });
                    self.mark_played(&message.id, true);
                    if message.kind == Kind::Voice {
                        self.ctx.request_repaint();
                    }
                    return;
                }
                Err(error) => {
                    self.note(format!("{error} — apro con il player di sistema"));
                    self.play_externally(&path);
                }
            }
        }
        self.request(
            &message,
            Pending::Audio {
                chat: message.chat_id.clone(),
                id: message.id.clone(),
                autoplay: true,
            },
        );
    }

    fn play_externally(&mut self, path: &Path) {
        if !util::open_external_checked(path) {
            self.note("Nessun programma di sistema per aprire il file");
        }
    }

    pub fn open_document(&mut self, index: usize) {
        let Some(message) = self.messages.get(index).cloned() else {
            return;
        };
        let path = self.cache_path(&message);
        if path.exists() {
            util::open_external(&path);
            return;
        }
        self.request(
            &message,
            Pending::Doc {
                chat: message.chat_id.clone(),
                id: message.id.clone(),
            },
        );
    }

    pub fn download_message(&mut self, index: usize) {
        let Some(message) = self.messages.get(index).cloned() else {
            return;
        };
        let pending = match message.kind {
            Kind::Image | Kind::Sticker => Pending::Image {
                chat: message.chat_id.clone(),
                id: message.id.clone(),
                open: true,
                view_once: message.view_once,
            },
            Kind::Video => Pending::Video {
                chat: message.chat_id.clone(),
                id: message.id.clone(),
                view_once: message.view_once,
            },
            Kind::Voice | Kind::Audio => Pending::Audio {
                chat: message.chat_id.clone(),
                id: message.id.clone(),
                autoplay: true,
            },
            _ => Pending::Doc {
                chat: message.chat_id.clone(),
                id: message.id.clone(),
            },
        };
        self.note("Download in corso…");
        self.request(&message, pending);
    }

    pub fn media_ready(&mut self, pending: Pending, bytes: Vec<u8>) {
        match pending {
            Pending::Thumb { chat, id } => {
                let _ = chat;
                if let Some((image, _, _)) = gfx::image_from_bytes(&bytes) {
                    let ctx = self.ctx.clone();
                    self.gfx.insert(&ctx, format!("thumb:{id}"), image, false);
                }
            }
            Pending::Image {
                chat,
                id,
                open,
                view_once,
            } => {
                let _ = chat;
                let path = self
                    .messages
                    .iter()
                    .find(|message| message.id == id)
                    .map(|message| self.cache_path(message));
                if let Some(path) = path.as_ref() {
                    let _ = std::fs::write(path, &bytes);
                }
                let key = format!("full:{id}");
                if let Some((image, _, _)) = gfx::image_from_bytes(&bytes) {
                    let ctx = self.ctx.clone();
                    self.gfx.insert(&ctx, key.clone(), image, false);
                }
                self.viewer_loading = false;
                if open {
                    self.viewer_image = self.gfx.get(&key);
                    self.viewer = Some(id);
                    self.view_view_once = view_once;
                }
            }
            Pending::Video {
                chat,
                id,
                view_once,
            } => {
                let _ = chat;
                let path = self
                    .messages
                    .iter()
                    .find(|message| message.id == id)
                    .map(|message| self.cache_path(message))
                    .unwrap_or_else(|| self.paths.media.join(format!("{id}.mp4")));
                if let Err(error) = std::fs::write(&path, &bytes) {
                    self.note(format!("Scrittura video: {error}"));
                    return;
                }
                self.viewer_loading = false;
                match VideoPlayer::open(&path) {
                    Ok(player) => {
                        self.view_view_once = view_once;
                        self.video = Some(VideoState {
                            id,
                            player,
                            texture: None,
                        });
                    }
                    Err(error) => self.note(error),
                }
            }
            Pending::Audio {
                chat,
                id,
                autoplay,
            } => {
                let _ = chat;
                let path = self
                    .messages
                    .iter()
                    .find(|message| message.id == id)
                    .map(|message| self.cache_path(message))
                    .unwrap_or_else(|| self.paths.media.join(format!("{id}.ogg")));
                if let Err(error) = std::fs::write(&path, &bytes) {
                    self.note(format!("Scrittura audio: {error}"));
                    return;
                }
                if autoplay {
                    match AudioPlayer::open(&path) {
                        Ok(player) => {
                            self.audio = Some(AudioState {
                                id: id.clone(),
                                player,
                            });
                            self.mark_played(&id, true);
                        }
                        Err(error) => self.note(error),
                    }
                }
            }
            Pending::Doc { chat, id } => {
                let _ = chat;
                let path = self
                    .messages
                    .iter()
                    .find(|message| message.id == id)
                    .map(|message| self.cache_path(message));
                if let Some(path) = path.as_ref() {
                    if let Err(error) = std::fs::write(path, &bytes) {
                        self.note(format!("Salvataggio: {error}"));
                        return;
                    }
                    util::open_external(path);
                }
                self.note("Documento salvato nella cache");
            }
        }
    }

    pub fn start_recording(&mut self) {
        let path = self
            .paths
            .cache
            .join(format!("vocale-{}.ogg", util::timestamp_ms()));
        match Recorder::start(&path) {
            Ok(recorder) => {
                self.recorder = Some(recorder);
                self.rec_start = Instant::now();
                self.rec_path = path;
            }
            Err(error) => self.note(error),
        }
    }

    pub fn stop_recording(&mut self, send: bool) {
        let Some(recorder) = self.recorder.take() else {
            return;
        };
        if !send {
            recorder.cancel();
            let _ = std::fs::remove_file(&self.rec_path);
            return;
        }
        match recorder.finish() {
            Ok((seconds, waveform, file)) => {
                let duration = seconds.round().max(1.0) as u32;
                self.send_file(file, Some((duration, waveform)));
            }
            Err(error) => self.note(error),
        }
    }

    pub fn overlays(&mut self, ctx: &egui::Context) {
        self.image_viewer(ctx);
        self.video_overlay(ctx);
        self.settings_ui(ctx);
        self.new_chat_ui(ctx);
        self.pair_modal_ui(ctx);
        self.path_modal_ui(ctx);
        self.logout_confirm(ctx);
    }

    fn close_viewer(&mut self) {
        if self.view_view_once {
            if let Some(id) = self.viewer.clone() {
                self.mark_played(&id, true);
                if let Some(message) = self.messages.iter_mut().find(|message| message.id == id)
                {
                    message.played = true;
                    message.payload.clear();
                    message.thumb = None;
                    message.local_preview = None;
                }
                if let Some(message) = self.messages.iter().find(|message| message.id == id) {
                    let path = self.cache_path(message);
                    let _ = std::fs::remove_file(path);
                }
            }
        }
        self.viewer = None;
        self.viewer_image = None;
        self.view_view_once = false;
        self.viewer_loading = false;
    }

    fn image_viewer(&mut self, ctx: &egui::Context) {
        let Some(id) = self.viewer.clone() else {
            return;
        };
        let Some(message) = self.messages.iter().find(|message| message.id == id).cloned()
        else {
            self.viewer = None;
            return;
        };

        let screen = ctx.content_rect();
        let mut close = false;

        egui::Area::new(egui::Id::new("viewer"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::Pos2::ZERO)
            .show(ctx, |ui| {
                let response = ui.allocate_rect(screen, Sense::click());
                ui.painter()
                    .rect_filled(screen, egui::CornerRadius::ZERO, Color32::from_black_alpha(242));

                if let Some(texture) = self.viewer_image.as_ref() {
                    let size = texture.size_vec2();
                    let max = Vec2::new(screen.width() * 0.86, screen.height() * 0.82);
                    let scale = (max.x / size.x).min(max.y / size.y).min(1.0);
                    let draw = Vec2::new(size.x * scale, size.y * scale);
                    let rect = egui::Rect::from_center_size(screen.center(), draw);
                    ui.painter().image(
                        texture.id(),
                        rect,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                } else {
                    ui.painter().text(
                        screen.center(),
                        egui::Align2::CENTER_CENTER,
                        if self.viewer_loading {
                            "Apertura…"
                        } else {
                            "Immagine non disponibile"
                        },
                        egui::FontId::proportional(14.0),
                        Palette::MUTED,
                    );
                }

                let close_rect = egui::Rect::from_min_size(
                    egui::Pos2::new(screen.right() - 52.0, screen.top() + 14.0),
                    Vec2::splat(38.0),
                );
                let close_response = ui.allocate_rect(close_rect, Sense::click());
                crate::icons::Icon::Close.paint(ui, close_rect.shrink(11.0), Color32::WHITE);
                if close_response.clicked() {
                    close = true;
                }

                let caption = if message.text.is_empty() {
                    format!(
                        "{} · {}",
                        util::display_name(&message.sender_name, &message.sender),
                        util::fmt_long_time(message.ts)
                    )
                } else {
                    format!(
                        "{} · {} · {}",
                        util::display_name(&message.sender_name, &message.sender),
                        util::fmt_long_time(message.ts),
                        message.text
                    )
                };
                ui.painter().text(
                    egui::Pos2::new(screen.center().x, screen.bottom() - 26.0),
                    egui::Align2::CENTER_BOTTOM,
                    util::truncate(&caption, 110),
                    egui::FontId::proportional(12.0),
                    Palette::MUTED,
                );

                if response.clicked() {
                    close = true;
                }
            });

        if close || ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.close_viewer();
        }
    }

    fn video_overlay(&mut self, ctx: &egui::Context) {
        let Some(video) = self.video.as_mut() else {
            return;
        };
        let mut close = false;
        let mut open = true;
        let duration = video.player.duration;
        let size = Vec2::new(820.0, 520.0);

        egui::Window::new("Video")
            .id(egui::Id::new("video-player"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_size(size)
            .fixed_size(size)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                let frame_size = Vec2::new(size.x - 24.0, size.y - 96.0);
                if let Some(texture) = video.texture.as_ref() {
                    let tex_size = texture.size_vec2();
                    let scale = (frame_size.x / tex_size.x)
                        .min(frame_size.y / tex_size.y)
                        .min(1.0)
                        .max(0.1);
                    let draw = tex_size * scale;
                    let (rect, _) = ui.allocate_exact_size(frame_size, Sense::click());
                    let image_rect = egui::Rect::from_center_size(rect.center(), draw);
                    ui.painter().image(
                        texture.id(),
                        image_rect,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                } else {
                    let (rect, _) = ui.allocate_exact_size(frame_size, Sense::hover());
                    ui.painter().rect_filled(
                        rect,
                        egui::CornerRadius::same(8),
                        Color32::BLACK,
                    );
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Caricamento…",
                        egui::FontId::proportional(13.0),
                        Palette::MUTED,
                    );
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let playing = video.player.playing;
                    let icon = if playing {
                        crate::icons::Icon::Pause
                    } else {
                        crate::icons::Icon::Play
                    };
                    if icon.button(ui, 30.0, Palette::TEXT).clicked() {
                        video.player.toggle();
                    }

                    let position = video.player.position();
                    let bar_width = (size.x - 150.0).max(120.0);
                    let (rect, response) =
                        ui.allocate_exact_size(Vec2::new(bar_width, 16.0), Sense::click());
                    let painter = ui.painter();
                    painter.rect_filled(
                        egui::Rect::from_min_size(
                            egui::Pos2::new(rect.left(), rect.center().y - 1.5),
                            Vec2::new(rect.width(), 3.0),
                        ),
                        egui::CornerRadius::same(1),
                        Color32::from_gray(60),
                    );
                    if duration > 0.0 {
                        let fraction = (position / duration).clamp(0.0, 1.0) as f32;
                        painter.rect_filled(
                            egui::Rect::from_min_size(
                                egui::Pos2::new(rect.left(), rect.center().y - 1.5),
                                Vec2::new(rect.width() * fraction, 3.0),
                            ),
                            egui::CornerRadius::same(1),
                            Palette::WHITE,
                        );
                    }
                    if response.clicked() {
                        if let Some(pos) = response.interact_pointer_pos() {
                            if duration > 0.0 && rect.width() > 0.0 {
                                let fraction =
                                    ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                                video.player.seek(fraction as f64 * duration);
                            }
                        }
                    }

                    ui.label(
                        RichText::new(format!(
                            "{} / {}",
                            util::human_duration(position.max(0.0) as u32),
                            util::human_duration(duration.max(0.0) as u32)
                        ))
                        .size(11.0)
                        .color(Palette::MUTED),
                    );

                    if crate::icons::Icon::Close
                        .button(ui, 28.0, Palette::MUTED)
                        .clicked()
                    {
                        close = true;
                    }
                });
            });

        if !open || close {
            self.close_video();
        }
    }

    fn close_video(&mut self) {
        let Some(video) = self.video.take() else {
            return;
        };
        let id = video.id.clone();
        drop(video);
        if self.view_view_once {
            self.mark_played(&id, true);
            if let Some(message) = self.messages.iter_mut().find(|message| message.id == id) {
                message.played = true;
                message.payload.clear();
                message.thumb = None;
                message.local_preview = None;
            }
            if let Some(message) = self.messages.iter().find(|message| message.id == id) {
                let path = self.cache_path(message);
                let _ = std::fs::remove_file(path);
            }
        }
        self.view_view_once = false;
        self.viewer_loading = false;
    }

    fn settings_ui(&mut self, ctx: &egui::Context) {
        let mut open = self.settings;
        if open {
            egui::Window::new("Impostazioni")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(380.0)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
                        let name = self.me_name.clone();
                        let jid = self.me_jid.clone();
                        super::widgets::avatar(ui.painter(), rect, &name, &jid, false, true);
                        ui.add_space(4.0);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(if name.is_empty() {
                                    "Account"
                                } else {
                                    name.as_str()
                                })
                                .size(14.0)
                                .color(Palette::TEXT),
                            );
                            ui.label(
                                RichText::new(if jid.is_empty() {
                                    "—".to_string()
                                } else {
                                    util::phone_display(&util::jid_user(&jid))
                                })
                                .size(11.5)
                                .color(Palette::MUTED),
                            );
                        });
                    });

                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(6.0);

                    ui.label(
                        RichText::new("Archivio")
                            .size(11.0)
                            .color(Palette::FAINT),
                    );
                    let size = dir_size(&self.paths.cache)
                        + dir_size(&self.paths.data);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("Dati locali: {}", util::human_size(size)))
                                .size(12.0)
                                .color(Palette::MUTED),
                        );
                        if ui.small_button("Svuota cache").clicked() {
                            let _ = std::fs::remove_dir_all(&self.paths.media);
                            let _ = std::fs::create_dir_all(&self.paths.media);
                            self.gfx.clear();
                            self.note("Cache svuotata");
                        }
                    });

                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Riproduzione")
                            .size(11.0)
                            .color(Palette::FAINT),
                    );
                    ui.label(
                        RichText::new(if media::ffmpeg_available() {
                            "ffmpeg: disponibile"
                        } else {
                            "ffmpeg: non trovato — audio e video si aprono con il player di sistema"
                        })
                        .size(12.0)
                        .color(Palette::MUTED),
                    );

                    let mut system_player = self.config.system_player;
                    if ui
                        .checkbox(
                            &mut system_player,
                            RichText::new("Usa il player di sistema per audio e video")
                                .size(12.0)
                                .color(Palette::TEXT),
                        )
                        .changed()
                    {
                        self.config.system_player = system_player;
                        self.config.save(&self.paths.data);
                    }

                    ui.add_space(4.0);
                    let mut system_fonts = self.config.system_fonts;
                    if ui
                        .checkbox(
                            &mut system_fonts,
                            RichText::new("Font di sistema come riserva (emoji, arabo, CJK)")
                                .size(12.0)
                                .color(Palette::TEXT),
                        )
                        .on_hover_text("Riavvia il caricamento dei font dai percorsi di sistema")
                        .changed()
                    {
                        self.config.system_fonts = system_fonts;
                        self.config.save(&self.paths.data);
                        crate::theme::set_system_fonts(&self.ctx, system_fonts);
                    }

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui
                            .button(RichText::new("Diagnostica").size(12.0))
                            .on_hover_text("Ambiente, dipendenze e percorsi")
                            .clicked()
                        {
                            self.ctx.copy_text(crate::platform::report(&self.paths));
                            self.note("Diagnostica copiata negli appunti");
                        }
                        if ui
                            .button(RichText::new("Apri il log").size(12.0))
                            .clicked()
                        {
                            util::open_external(&self.paths.log_file());
                        }
                    });

                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Whats-Open è un client indipendente, non affiliato a WhatsApp LLC.")
                            .size(10.5)
                            .color(Palette::FAINT),
                    );
                    ui.hyperlink_to(
                        RichText::new("github.com/TixiGX/Whats-Open")
                            .size(11.0)
                            .color(Palette::MUTED),
                        "https://github.com/TixiGX/Whats-Open",
                    );

                    ui.add_space(10.0);
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Esci e scollega").size(12.5),
                            )
                            .fill(Palette::SURFACE)
                            .corner_radius(egui::CornerRadius::same(9)),
                        )
                        .clicked()
                    {
                        self.confirm_logout = true;
                    }
                });
        }
        self.settings = open;
    }

    fn new_chat_ui(&mut self, ctx: &egui::Context) {
        let mut open = self.new_chat;
        let mut close = false;
        if open {
            egui::Window::new("Nuova chat")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(340.0)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new("Numero di telefono con prefisso internazionale")
                            .size(11.5)
                            .color(Palette::MUTED),
                    );
                    ui.add_space(4.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_chat_phone)
                            .hint_text("es. 393331234567")
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(8.0);
                    let digits: String = self
                        .new_chat_phone
                        .chars()
                        .filter(|c| c.is_ascii_digit())
                        .collect();
                    let valid = digits.len() >= 7;
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                valid,
                                egui::Button::new(RichText::new("Apri chat").size(12.5)),
                            )
                            .clicked()
                        {
                            self.start_chat_with(&digits);
                            self.new_chat_phone.clear();
                            close = true;
                        }
                        if !self.me_jid.is_empty() && ui.button("Scrivi a te stesso").clicked() {
                            let me = util::jid_user(&self.me_jid);
                            self.start_chat_with(&me);
                            close = true;
                        }
                    });
                });
        }
        self.new_chat = open && !close;
    }

    fn start_chat_with(&mut self, number: &str) {
        if number.is_empty() {
            return;
        }
        let jid = if number.contains('@') {
            number.to_string()
        } else {
            format!("{number}@s.whatsapp.net")
        };
        if !self.index.contains_key(&jid) {
            let chat = Chat {
                id: jid.clone(),
                name: util::phone_display(&util::jid_user(&jid)),
                ..Default::default()
            };
            self.store.upsert_chat(&chat).ok();
            self.chats.push(chat);
            self.sort_chats();
        }
        self.select_chat(jid);
    }

    fn pair_modal_ui(&mut self, ctx: &egui::Context) {
        let mut open = self.pair_modal;
        let mut close = false;
        if open {
            egui::Window::new("Collega con numero")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(360.0)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new("Inserisci il numero completo, senza zeri iniziali")
                            .size(11.5)
                            .color(Palette::MUTED),
                    );
                    ui.add_space(4.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.pair_number)
                            .hint_text("es. 393331234567")
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(8.0);
                    let digits: String = self
                        .pair_number
                        .chars()
                        .filter(|c| c.is_ascii_digit())
                        .collect();
                    if ui
                        .add_enabled(
                            digits.len() >= 7,
                            egui::Button::new(RichText::new("Genera codice").size(12.5)),
                        )
                        .clicked()
                    {
                        self.begin_engine(Some(digits));
                        close = true;
                    }
                });
        }
        self.pair_modal = open && !close;
    }

    fn path_modal_ui(&mut self, ctx: &egui::Context) {
        let mut open = self.path_modal;
        let mut close = false;
        if open {
            egui::Window::new("Allega da percorso")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(420.0)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new("Percorso completo del file da inviare")
                            .size(11.5)
                            .color(Palette::MUTED),
                    );
                    ui.add_space(4.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.path_input)
                            .hint_text("/home/utente/immagine.jpg")
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(8.0);
                    if ui
                        .add_enabled(
                            !self.path_input.trim().is_empty(),
                            egui::Button::new(RichText::new("Invia").size(12.5)),
                        )
                        .clicked()
                    {
                        let path = PathBuf::from(self.path_input.trim());
                        self.path_input.clear();
                        self.send_file(path, None);
                        close = true;
                    }
                });
        }
        self.path_modal = open && !close;
    }

    fn logout_confirm(&mut self, ctx: &egui::Context) {
        let mut open = self.confirm_logout;
        let mut close = false;
        if open {
            egui::Window::new("Scollegare il dispositivo?")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(340.0)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new(
                            "La sessione verrà cancellata da questo computer. Potrai ricollegarla con un nuovo QR.",
                        )
                        .size(12.0)
                        .color(Palette::MUTED),
                    );
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Scollega").size(12.5).color(Palette::DANGER),
                                )
                                .fill(Palette::SURFACE),
                            )
                            .clicked()
                        {
                            self.settings = false;
                            self.reset_session();
                            close = true;
                        }
                        if ui.button("Annulla").clicked() {
                            close = true;
                        }
                    });
                });
        }
        self.confirm_logout = open && !close;
    }
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            total += dir_size(&entry.path());
        } else {
            total += meta.len();
        }
    }
    total
}

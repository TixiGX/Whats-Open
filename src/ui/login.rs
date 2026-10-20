//! Schermata di accesso: QR, codice di accoppiamento, errori.

use egui::{RichText, Vec2};

use crate::theme::Palette;

use super::{gfx, App, Conn};

impl App {
    pub fn login_ui(&mut self, ui: &mut egui::Ui) {
        let height = ui.available_height();
        ui.vertical_centered(|ui| {
            ui.add_space((height * 0.10).max(24.0));

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
                        Vec2::splat(92.0),
                    ))
                    .corner_radius(22),
                );
            }

            ui.add_space(14.0);
            ui.label(RichText::new("Whats-Open").size(30.0).color(Palette::TEXT));
            ui.add_space(4.0);
            ui.label(
                RichText::new("Un client WhatsApp nativo per Linux")
                    .size(13.0)
                    .color(Palette::MUTED),
            );
            ui.add_space(26.0);

            match self.conn {
                Conn::Starting => {
                    ui.add(egui::Spinner::new().size(26.0).color(Palette::TEXT));
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(&self.status_text)
                            .size(13.0)
                            .color(Palette::MUTED),
                    );
                }
                Conn::Qr => self.qr_card(ui),
                Conn::PairCode => self.pair_card(ui),
                Conn::Error => self.error_card(ui),
                Conn::Connected => {}
            }

            ui.add_space(24.0);

            if self.conn != Conn::Error {
                ui.horizontal(|ui| {
                    let total = 330.0;
                    let gap = 8.0;
                    if ui.available_width() > total + gap {
                        ui.add_space((ui.available_width() - total - gap) * 0.5);
                    }
                    let size = Vec2::new((total - gap) * 0.5, 34.0);
                    if ui
                        .add_sized(
                            size,
                            egui::Button::new(
                                RichText::new("Collega con numero").size(13.0),
                            )
                            .fill(Palette::SURFACE)
                            .corner_radius(egui::CornerRadius::same(9)),
                        )
                        .clicked()
                    {
                        self.pair_modal = true;
                    }
                    if ui
                        .add_sized(
                            size,
                            egui::Button::new(
                                RichText::new("Ricomincia").size(13.0),
                            )
                            .fill(Palette::SURFACE)
                            .corner_radius(egui::CornerRadius::same(9)),
                        )
                        .on_hover_text("Rigenera il codice di accesso")
                        .clicked()
                    {
                        let pair = None;
                        self.begin_engine(pair);
                    }
                });
            }

            ui.add_space(18.0);
            ui.label(
                RichText::new("Leggero · nativo · monocromatico · nessuna WebView")
                    .size(11.5)
                    .color(Palette::FAINT),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("versione {}", env!("CARGO_PKG_VERSION")))
                    .size(11.0)
                    .color(Palette::FAINT),
            );
        });
    }

    fn qr_card(&mut self, ui: &mut egui::Ui) {
        if self.qr_tex.is_none() {
            if let Some(image) = gfx::qr_image(&self.qr_code) {
                self.qr_tex = Some(ui.ctx().load_texture(
                    "qr",
                    image,
                    egui::TextureOptions::NEAREST,
                ));
            }
        }

        egui::Frame::new()
            .fill(egui::Color32::WHITE)
            .corner_radius(egui::CornerRadius::same(16))
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                if let Some(texture) = self.qr_tex.as_ref() {
                    ui.add(
                        egui::Image::new(egui::load::SizedTexture::new(
                            texture.id(),
                            Vec2::splat(228.0),
                        ))
                        .corner_radius(4),
                    );
                } else {
                    ui.allocate_exact_size(Vec2::splat(228.0), egui::Sense::hover());
                }
            });

        ui.add_space(14.0);
        ui.label(
            RichText::new("WhatsApp → Dispositivi collegati → Collega un dispositivo")
                .size(12.5)
                .color(Palette::MUTED),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Il codice si rinnova da solo ogni 20 secondi")
                .size(11.0)
                .color(Palette::FAINT),
        );
    }

    fn pair_card(&mut self, ui: &mut egui::Ui) {
        let code = self.qr_code.clone();
        let pretty = pretty_code(&code);
        egui::Frame::new()
            .fill(Palette::SURFACE)
            .stroke(egui::Stroke::new(1.0, Palette::BORDER))
            .corner_radius(egui::CornerRadius::same(16))
            .inner_margin(egui::Margin::symmetric(34, 22))
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Codice di accoppiamento")
                        .size(11.5)
                        .color(Palette::MUTED),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(&pretty)
                        .size(31.0)
                        .monospace()
                        .color(Palette::WHITE),
                );
            });
        ui.add_space(14.0);
        ui.label(
            RichText::new("Sul telefono: Dispositivi collegati → Collega con numero")
                .size(12.5)
                .color(Palette::MUTED),
        );
        if let Some(phone) = self.pair_phone.as_deref() {
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!("Numero: +{}", digits(phone)))
                    .size(11.5)
                    .color(Palette::FAINT),
            );
        }
    }

    fn error_card(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(Palette::SURFACE)
            .stroke(egui::Stroke::new(1.0, Palette::BORDER))
            .corner_radius(egui::CornerRadius::same(14))
            .inner_margin(egui::Margin::symmetric(24, 18))
            .show(ui, |ui| {
                ui.set_max_width(430.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("Qualcosa non ha funzionato")
                            .size(15.0)
                            .color(Palette::TEXT),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(crate::util::truncate(&self.status_text, 300))
                            .size(12.0)
                            .color(Palette::DANGER),
                    );
                });
            });
        ui.add_space(14.0);
        let pair = self.pair_phone.clone();
        let mut retry = false;
        let mut wipe = false;
        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::Button::new(RichText::new("Riprova").size(13.0))
                        .fill(Palette::SURFACE)
                        .corner_radius(egui::CornerRadius::same(9)),
                )
                .clicked()
            {
                retry = true;
            }
            if ui
                .add(
                    egui::Button::new(RichText::new("Cancella la sessione").size(13.0))
                        .fill(Palette::SURFACE)
                        .corner_radius(egui::CornerRadius::same(9)),
                )
                .on_hover_text("Utile se l'archivio della sessione è danneggiato")
                .clicked()
            {
                wipe = true;
            }
        });
        if retry {
            self.begin_engine(pair);
        }
        if wipe {
            self.reset_session();
            self.note("Sessione cancellata: collega di nuovo il telefono");
        }
    }
}

fn digits(value: &str) -> String {
    value.chars().filter(|c| c.is_ascii_digit()).collect()
}

fn pretty_code(code: &str) -> String {
    let mut out = String::new();
    for (i, ch) in code.chars().enumerate() {
        if i > 0 && i % 4 == 0 {
            out.push('–');
        }
        out.push(ch);
    }
    out
}

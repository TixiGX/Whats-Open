//! Schermata di accesso: QR, codice di accoppiamento, errori.

use egui::{RichText, Vec2};

use crate::theme::pal;

use super::{gfx, App, Conn};

impl App {
    pub fn login_ui(&mut self, ui: &mut egui::Ui) {
        let height = ui.available_height();
        // In una finestra bassa si stringe tutto: il codice resta grande
        // abbastanza da essere inquadrato e il resto si compatta. Se nemmeno
        // così ci sta, la colonna scorre invece di sparire sotto il bordo.
        // La misura del codice segue l'altezza vera: così il piè di pagina
        // resta dentro la finestra anche quando è appena sopra il minimo.
        let compact = height < 760.0;
        let side = if compact { 190.0 } else { 228.0 };

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(if compact { 8.0 } else { (height * 0.08).max(20.0) });

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
                        let logo = if compact { 72.0 } else { 92.0 };
                        ui.add(
                            egui::Image::new(egui::load::SizedTexture::new(
                                texture.id(),
                                Vec2::splat(logo),
                            ))
                            .corner_radius(20),
                        );
                    }

                    ui.add_space(if compact { 8.0 } else { 14.0 });
                    ui.label(
                        RichText::new("Whats-Open")
                            .size(if compact { 26.0 } else { 30.0 })
                            .color(pal().text),
                    );
                    ui.add_space(3.0);
                    ui.label(
                        RichText::new("Un client WhatsApp nativo per Linux")
                            .size(12.5)
                            .color(pal().muted),
                    );
                    ui.add_space(if compact { 13.0 } else { 26.0 });

                    match self.conn {
                        Conn::Starting => {
                            ui.add(egui::Spinner::new().size(26.0).color(pal().text));
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(&self.status_text)
                                    .size(13.0)
                                    .color(pal().muted),
                            );
                        }
                        Conn::Qr => self.qr_card(ui, side),
                        Conn::PairCode => self.pair_card(ui),
                        Conn::Error => self.error_card(ui),
                        Conn::Connected => {}
                    }

                    ui.add_space(if compact { 11.0 } else { 24.0 });

                    if self.conn != Conn::Error {
                        // Due pulsanti affiancati, larghi quanto la colonna di
                        // testo (470 px): sotto quella soglia si impilano
                        // invece di uscire dalla finestra. Lo spazio fra i due
                        // è quello vero del tema, così la coppia sta esatta.
                        let gap = ui.spacing().item_spacing.x;
                        let room = ui.available_width().min(470.0);
                        let wide = room >= 300.0;
                        let width = if wide {
                            (room - gap) * 0.5
                        } else {
                            // In una finestra molto stretta il pulsante resta
                            // dentro: meglio stretto che tagliato dal bordo.
                            room
                        };
                        ui.scope(|ui| {
                            ui.set_max_width(room);
                            if wide {
                                ui.horizontal(|ui| {
                                    if super::widgets::primary_button(
                                        ui,
                                        "Collega con numero",
                                        width,
                                    )
                                    .clicked()
                                    {
                                        self.pair_modal = true;
                                    }
                                    if super::widgets::ghost_button(ui, "Ricomincia", width)
                                        .on_hover_text("Rigenera il codice di accesso")
                                        .clicked()
                                    {
                                        self.begin_engine(None);
                                    }
                                });
                            } else {
                                if super::widgets::primary_button(
                                    ui,
                                    "Collega con numero",
                                    width,
                                )
                                .clicked()
                                {
                                    self.pair_modal = true;
                                }
                                ui.add_space(6.0);
                                if super::widgets::ghost_button(ui, "Ricomincia", width).clicked() {
                                    self.begin_engine(None);
                                }
                            }
                        });
                    }

                    ui.add_space(if compact { 8.0 } else { 18.0 });
                    ui.label(
                        RichText::new("Leggero · nativo · nessuna WebView")
                            .size(11.0)
                            .color(pal().faint),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!("versione {}", env!("CARGO_PKG_VERSION")))
                            .size(10.5)
                            .color(pal().faint),
                    );
                    ui.add_space(if compact { 6.0 } else { 10.0 });
                });
            });
    }

    fn qr_card(&mut self, ui: &mut egui::Ui, side: f32) {
        if self.qr_tex.is_none() {
            if let Some(image) = gfx::qr_image(&self.qr_code) {
                self.qr_tex = Some(ui.ctx().load_texture(
                    "qr",
                    image,
                    egui::TextureOptions::NEAREST,
                ));
            }
        }

        // La scheda si disegna a mano: dentro un layout centrato un Frame
        // prenderebbe tutta la larghezza disponibile e diventerebbe una fascia
        // bianca da bordo a bordo.
        let padding = if side < 220.0 { 12.0 } else { 14.0 };
        let (card, _) = ui.allocate_exact_size(
            Vec2::splat(side + padding * 2.0),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(
            card,
            egui::CornerRadius::same(16),
            egui::Color32::WHITE,
        );
        ui.painter().rect_stroke(
            card,
            egui::CornerRadius::same(16),
            egui::Stroke::new(1.0, pal().border),
            egui::StrokeKind::Inside,
        );
        if let Some(texture) = self.qr_tex.as_ref() {
            let qr = egui::Rect::from_center_size(card.center(), Vec2::splat(side));
            egui::Image::new(egui::load::SizedTexture::new(texture.id(), qr.size()))
                .paint_at(ui, qr);
        }

        ui.add_space(14.0);
        ui.label(
            RichText::new("WhatsApp → Dispositivi collegati → Collega un dispositivo")
                .size(12.5)
                .color(pal().muted),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Il codice si rinnova da solo ogni 20 secondi")
                .size(11.0)
                .color(pal().faint),
        );
        ui.add_space(10.0);
        // Una riga lunga quanto tutta la finestra è difficile da leggere:
        // il testo si tiene entro una colonna stretta, come il resto.
        ui.scope(|ui| {
            ui.set_max_width(470.0_f32.min(ui.available_width()));
            ui.label(
                RichText::new(
                    "L'app si presenta come WhatsApp Web (Linux). Se il telefono \
                     dice «Impossibile collegare il dispositivo», aggiorna WhatsApp, \
                     libera uno dei dispositivi collegati e riprova: il codice in \
                     schermo deve essere inquadrato subito.",
                )
                .size(11.0)
                .color(pal().muted),
            );
        });
    }

    fn pair_card(&mut self, ui: &mut egui::Ui) {
        let code = self.qr_code.clone();
        let pretty = pretty_code(&code);
        egui::Frame::new()
            .fill(pal().surface)
            .stroke(egui::Stroke::new(1.0, pal().border))
            .corner_radius(egui::CornerRadius::same(16))
            .inner_margin(egui::Margin::symmetric(34, 22))
            .show(ui, |ui| {
                ui.label(
                    RichText::new("Codice di accoppiamento")
                        .size(11.5)
                        .color(pal().muted),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(&pretty)
                        .size(31.0)
                        .monospace()
                        .color(pal().text),
                );
            });
        ui.add_space(14.0);
        ui.label(
            RichText::new("Sul telefono: Dispositivi collegati → Collega con numero")
                .size(12.5)
                .color(pal().muted),
        );
        if let Some(phone) = self.pair_phone.as_deref() {
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!("Numero: +{}", digits(phone)))
                    .size(11.5)
                    .color(pal().faint),
            );
        }
    }

    fn error_card(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(pal().surface)
            .stroke(egui::Stroke::new(1.0, pal().border))
            .corner_radius(egui::CornerRadius::same(14))
            .inner_margin(egui::Margin::symmetric(24, 18))
            .show(ui, |ui| {
                ui.set_max_width(430.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("Qualcosa non ha funzionato")
                            .size(15.0)
                            .color(pal().text),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(crate::util::truncate(&self.status_text, 300))
                            .size(12.0)
                            .color(pal().danger),
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
                        .fill(pal().surface)
                        .corner_radius(egui::CornerRadius::same(9)),
                )
                .clicked()
            {
                retry = true;
            }
            if ui
                .add(
                    egui::Button::new(RichText::new("Cancella la sessione").size(13.0))
                        .fill(pal().surface)
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

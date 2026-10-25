//! Collaudo senza rete: `whats-open --selftest`.
//!
//! Esercita con il binario vero tutti i pezzi che non hanno bisogno di un
//! account: archivio locale, traduzione dei messaggi, miniature, tema,
//! configurazione, rubrica, audio e video delle chiamate. Ogni controllo
//! stampa una riga; se qualcuno fallisce l'uscita è 1.
//!
//! È fatto per la CI e per chi vuole verificare l'installazione senza
//! collegare il telefono: `whats-open --selftest`.

use std::io::Cursor;
use std::path::Path;

use crate::config::Config;
use crate::engine::extract::{self, Extracted};
use crate::model::{Chat, Contact, Kind, Message, Status};
use crate::store::Store;
use crate::util::{self, Paths};

struct Check {
    name: &'static str,
    outcome: Result<String, String>,
}

pub struct Report {
    pub checks: Vec<(&'static str, bool, String)>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.checks.iter().all(|(_, passed, _)| *passed)
    }

    pub fn text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Whats-Open {} — collaudo delle funzioni interne\n",
            env!("CARGO_PKG_VERSION")
        ));
        for (name, passed, detail) in &self.checks {
            out.push_str(&format!(
                "{} {name:<26} {detail}\n",
                if *passed { "OK  " } else { "FAIL" },
            ));
        }
        let passed = self.checks.iter().filter(|(_, ok, _)| *ok).count();
        let failed = self.checks.len() - passed;
        out.push_str(&format!(
            "\n{passed} controlli superati, {failed} falliti su {}\n",
            self.checks.len()
        ));
        out
    }
}

fn section<F>(name: &'static str, checks: &mut Vec<Check>, body: F)
where
    F: FnOnce() -> Result<String, String>,
{
    checks.push(Check {
        name,
        outcome: body(),
    });
}

/// Esegue tutti i controlli in una cartella temporanea che sparisce alla fine.
pub fn run() -> Report {
    let root = std::env::temp_dir().join(format!(
        "whats-open-collaudo-{}-{}",
        std::process::id(),
        util::timestamp_ms()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(&root);

    let mut checks = Vec::new();
    section("percorsi", &mut checks, || check_paths(&root));
    section("archivio", &mut checks, || check_store(&root));
    section("estrazione messaggi", &mut checks, || check_extract());
    section("media", &mut checks, || check_media(&root));
    section("configurazione", &mut checks, || check_config(&root));
    section("tema del desktop", &mut checks, || check_theme(&root));
    section("leggibilità", &mut checks, || check_palette());
    section("rubrica", &mut checks, || check_contacts(&root));
    section("nomi e numeri", &mut checks, || check_formatting());
    section("canale audio", &mut checks, || {
        crate::engine::call::self_test()
    });
    section("percorso video", &mut checks, || {
        crate::engine::video::self_test()
    });

    let _ = std::fs::remove_dir_all(&root);
    Report {
        checks: checks
            .into_iter()
            .map(|check| match check.outcome {
                Ok(detail) => (check.name, true, detail),
                Err(error) => (check.name, false, error),
            })
            .collect(),
    }
}

fn check_paths(root: &Path) -> Result<String, String> {
    let paths = Paths {
        data: root.join("data"),
        cache: root.join("cache"),
        db: root.join("data/messages.db"),
        session: root.join("data/session.db"),
        media: root.join("cache/media"),
    };
    paths.ensure().map_err(|error| format!("cartelle: {error}"))?;
    for dir in [&paths.data, &paths.cache, &paths.media] {
        if !dir.is_dir() {
            return Err(format!("{} non creata", dir.display()));
        }
    }
    // Un id ostile non deve uscire dalla cache.
    let hostile = paths.cached_media("../../etc", "../passwd", "jpg");
    if !hostile.starts_with(&paths.media) {
        return Err(format!("percorso non confinato: {}", hostile.display()));
    }
    let name = hostile
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name.contains("..") || name.contains('/') {
        return Err(format!("nome non sicuro: {name}"));
    }
    Ok("cartelle create, percorsi dei media confinati".to_string())
}

fn check_store(root: &Path) -> Result<String, String> {
    let db = root.join("store/messages.db");
    let store = Store::open(&db).map_err(|error| format!("apertura: {error}"))?;

    let chat = Chat {
        id: "393481112222@s.whatsapp.net".to_string(),
        name: "Prova".to_string(),
        is_group: false,
        last_ts: 1_700_000_000,
        preview: "ciao".to_string(),
        unread: 0,
        muted: false,
        pinned: false,
        online: false,
        typing: false,
        last_seen: None,
    };
    store
        .upsert_chat(&chat)
        .map_err(|error| format!("chat: {error}"))?;

    let mut kinds = 0usize;
    for (index, kind) in [
        Kind::Text,
        Kind::Image,
        Kind::Video,
        Kind::Gif,
        Kind::Voice,
        Kind::Audio,
        Kind::Sticker,
        Kind::Document,
        Kind::Location,
        Kind::Contact,
        Kind::System,
    ]
    .into_iter()
    .enumerate()
    {
        let message = Message {
            id: format!("m{index}"),
            chat_id: chat.id.clone(),
            from_me: index % 2 == 1,
            ts: 1_700_000_000 + index as i64,
            kind,
            text: format!("{kind:?}"),
            status: Status::Read,
            duration: index as u32,
            file_name: format!("file{index}.bin"),
            ..Default::default()
        };
        let inserted = store
            .insert_message(&message)
            .map_err(|error| format!("inserimento {kind:?}: {error}"))?;
        if inserted {
            kinds += 1;
        }
    }
    if kinds != 11 {
        return Err(format!("inseriti {kinds} messaggi su 11"));
    }

    // Lo stesso id non è un doppione da inserire due volte.
    let duplicate = Message {
        id: "m0".to_string(),
        chat_id: chat.id.clone(),
        ts: 1_700_000_000,
        kind: Kind::Text,
        text: "doppione".to_string(),
        ..Default::default()
    };
    if store.insert_message(&duplicate).unwrap_or(true) {
        return Err("lo stesso id è stato inserito due volte".to_string());
    }

    let loaded = store
        .load_messages(&chat.id, 100)
        .map_err(|error| format!("rilettura: {error}"))?;
    if loaded.len() != 11 {
        return Err(format!("riletti {} messaggi su 11", loaded.len()));
    }
    if loaded.first().map(|message| message.ts) != Some(1_700_000_000) {
        return Err("i messaggi non sono in ordine cronologico".to_string());
    }
    for kind in [Kind::Text, Kind::Image, Kind::Video, Kind::Gif, Kind::Voice] {
        if !loaded.iter().any(|message| message.kind == kind) {
            return Err(format!("tipo {kind:?} perso nell'archivio"));
        }
    }

    // Stati: pending → sent → read, e mai all'indietro.
    let tracked = Message {
        id: "stato".to_string(),
        chat_id: chat.id.clone(),
        from_me: true,
        ts: 1_700_000_900,
        kind: Kind::Text,
        text: "in viaggio".to_string(),
        status: Status::Pending,
        ..Default::default()
    };
    store
        .insert_message(&tracked)
        .map_err(|error| format!("messaggio di stato: {error}"))?;
    for next in [Status::Sent, Status::Read, Status::Delivered] {
        store
            .update_status(&chat.id, "stato", next)
            .map_err(|error| format!("stato {next:?}: {error}"))?;
    }
    let after = store.load_messages(&chat.id, 100).unwrap_or_default();
    let state = after
        .iter()
        .find(|message| message.id == "stato")
        .map(|message| message.status);
    if state != Some(Status::Read) {
        return Err(format!("lo stato è tornato indietro: {state:?}"));
    }

    // Modifica, revoca, reazione.
    store
        .apply_edit(&chat.id, "m0", "testo nuovo", 1_700_001_000)
        .map_err(|error| format!("modifica: {error}"))?;
    store
        .apply_revoke(&chat.id, "m0")
        .map_err(|error| format!("revoca: {error}"))?;
    store
        .set_reaction(&chat.id, "m0", Some("👍"))
        .map_err(|error| format!("reazione: {error}"))?;
    let after = store.load_messages(&chat.id, 100).unwrap_or_default();
    let edited = after.iter().find(|message| message.id == "m0");
    match edited {
        Some(message) if message.deleted && message.reaction.as_deref() == Some("👍") => {}
        other => {
            return Err(format!(
                "modifica/revoca/reazione non applicate: {:?}",
                other.map(|message| (message.deleted, message.text.clone()))
            ))
        }
    }

    // Contenuto "una volta sola": dopo la lettura non resta leggibile.
    store
        .consume_once(&chat.id, "m2")
        .map_err(|error| format!("consumo: {error}"))?;
    let after = store.load_messages(&chat.id, 100).unwrap_or_default();
    if let Some(message) = after.iter().find(|message| message.id == "m2") {
        if message.played && !message.text.is_empty() {
            return Err("il contenuto una volta resta leggibile".to_string());
        }
    }

    // Invii rimasti appesi: diventano falliti e si possono ritentare.
    let stale = Message {
        id: "pending".to_string(),
        chat_id: chat.id.clone(),
        from_me: true,
        ts: 1_600_000_000,
        kind: Kind::Text,
        text: "in sospeso".to_string(),
        status: Status::Pending,
        ..Default::default()
    };
    store.insert_message(&stale).unwrap_or(false);
    let failed = store.fail_stale_pending(1_650_000_000);
    if failed == 0 {
        return Err("nessun invio in sospeso marcato come fallito".to_string());
    }
    store
        .retry_message(&chat.id, "pending")
        .map_err(|error| format!("riprova: {error}"))?;

    // Nomi, alias e contatti.
    store
        .set_chat_name(&chat.id, "Prova Rinominata")
        .map_err(|error| format!("nome chat: {error}"))?;
    store
        .save_contact(
            &Contact {
                jid: chat.id.clone(),
                name: "Anna".to_string(),
                rank: 3,
                phone: "3481112222".to_string(),
                avatar: None,
            },
            1,
        )
        .map_err(|error| format!("contatto: {error}"))?;
    store
        .save_alias("12345@lid", &chat.id)
        .map_err(|error| format!("alias: {error}"))?;
    let aliases: std::collections::HashMap<String, String> =
        store.load_aliases().unwrap_or_default().into_iter().collect();
    if aliases.get("12345@lid").map(String::as_str) != Some(chat.id.as_str()) {
        return Err("alias LID → numero non salvato".to_string());
    }
    let contacts = store.load_contacts().unwrap_or_default();
    if !contacts.iter().any(|contact| contact.name == "Anna") {
        return Err("contatto non riletto".to_string());
    }

    // Quando WhatsApp rivela che un LID è un numero, chat e messaggi devono
    // seguire il nuovo indirizzo invece di restare separati.
    let lid = "99887766@lid".to_string();
    let number = "393480001111@s.whatsapp.net".to_string();
    store
        .upsert_chat(&Chat {
            id: lid.clone(),
            name: "Da LID".to_string(),
            ..chat.clone()
        })
        .map_err(|error| format!("chat LID: {error}"))?;
    store
        .insert_message(&Message {
            id: "lid-1".to_string(),
            chat_id: lid.clone(),
            kind: Kind::Text,
            text: "per LID".to_string(),
            status: Status::Sent,
            ts: 1_700_002_000,
            ..Default::default()
        })
        .map_err(|error| format!("messaggio LID: {error}"))?;
    store
        .rename_jid(&lid, &number)
        .map_err(|error| format!("spostamento LID: {error}"))?;
    let moved = store.load_messages(&number, 100).unwrap_or_default();
    if moved.len() != 1 || moved[0].chat_id != number {
        return Err(format!(
            "messaggi LID non spostati: {} righe",
            moved.len()
        ));
    }
    let chats_after = store.load_chats().unwrap_or_default();
    if chats_after.iter().any(|entry| entry.id == lid) {
        return Err("la chat LID è rimasta dopo lo spostamento".to_string());
    }
    if !chats_after.iter().any(|entry| entry.id == number) {
        return Err("la chat col numero non è nata".to_string());
    }

    // Cancellazione di una chat (serve quando due indirizzi sono doppioni).
    store
        .delete_chat(&number)
        .map_err(|error| format!("cancellazione: {error}"))?;
    let chats_after = store.load_chats().unwrap_or_default();
    if chats_after.iter().any(|entry| entry.id == number) {
        return Err("la chat è rimasta dopo la cancellazione".to_string());
    }
    Ok(format!(
        "11 tipi di messaggio, stati, modifica/revoca/reazione, una-volta, invii appesi, nomi, alias, contatti, fusione LID"
    ))
}

fn check_extract() -> Result<String, String> {
    use whatsapp_rust::prelude::MessageField;
    use whatsapp_rust::waproto::whatsapp as wa;

    let build = |message: &wa::Message| {
        extract::build(
            message,
            "393481112222@s.whatsapp.net".to_string(),
            "393481112222@s.whatsapp.net".to_string(),
            "Anna".to_string(),
            false,
            1_700_000_000,
        )
    };

    let text = build(&wa::Message {
        conversation: Some("ciao".to_string()),
        ..Default::default()
    });
    if text.kind != Kind::Text || text.text != "ciao" {
        return Err("testo non riconosciuto".to_string());
    }

    let mut image = wa::message::ImageMessage::default();
    image.caption = Some("didascalia".to_string());
    image.view_once = Some(true);
    let photo = build(&wa::Message {
        image_message: MessageField::some(image),
        ..Default::default()
    });
    if photo.kind != Kind::Image || !photo.view_once || photo.text != "didascalia" {
        return Err(format!(
            "foto: tipo {:?}, una volta {}, testo {:?}",
            photo.kind, photo.view_once, photo.text
        ));
    }

    let mut video = wa::message::VideoMessage::default();
    video.gif_playback = Some(true);
    let gif = build(&wa::Message {
        video_message: MessageField::some(video),
        ..Default::default()
    });
    if gif.kind != Kind::Gif {
        return Err(format!("GIF scambiata per {:?}", gif.kind));
    }

    let mut audio = wa::message::AudioMessage::default();
    audio.ptt = Some(true);
    audio.seconds = Some(7);
    let voice = build(&wa::Message {
        audio_message: MessageField::some(audio),
        ..Default::default()
    });
    if voice.kind != Kind::Voice || voice.duration != 7 {
        return Err(format!(
            "vocale: tipo {:?}, durata {}",
            voice.kind, voice.duration
        ));
    }

    let mut doc = wa::message::DocumentMessage::default();
    doc.file_name = Some("contratto.pdf".to_string());
    let document = build(&wa::Message {
        document_message: MessageField::some(doc),
        ..Default::default()
    });
    if document.kind != Kind::Document || document.file_name != "contratto.pdf" {
        return Err("documento non riconosciuto".to_string());
    }

    let sticker = build(&wa::Message {
        sticker_message: MessageField::some(wa::message::StickerMessage::default()),
        ..Default::default()
    });
    if sticker.kind != Kind::Sticker {
        return Err(format!("sticker letto come {:?}", sticker.kind));
    }

    let location = build(&wa::Message {
        location_message: MessageField::some(wa::message::LocationMessage::default()),
        ..Default::default()
    });
    if location.kind != Kind::Location {
        return Err(format!("posizione letta come {:?}", location.kind));
    }

    // Risposta con citazione: il testo citato arriva nell'anteprima.
    let quoted = wa::Message {
        extended_text_message: MessageField::some(wa::message::ExtendedTextMessage {
            text: Some("risposta".to_string()),
            context_info: MessageField::some(wa::ContextInfo {
                quoted_message: MessageField::some(wa::Message {
                    conversation: Some("domanda".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let reply = build(&quoted);
    match reply.quote.as_ref() {
        Some(quote) if quote.text == "domanda" => {}
        other => return Err(format!("citazione non letta: {other:?}")),
    }
    let preview = extract::quoted_preview(&wa::Message {
        image_message: MessageField::some(wa::message::ImageMessage::default()),
        ..Default::default()
    });
    if preview != "Foto" {
        return Err(format!("anteprima citazione foto: {preview}"));
    }

    // I contenitori (messaggio effimero) vengono srotolati.
    let wrapped = wa::Message {
        ephemeral_message: MessageField::some(wa::message::FutureProofMessage {
            message: MessageField::some(wa::Message {
                conversation: Some("dentro".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let unwrapped = build(&wrapped);
    if unwrapped.text != "dentro" {
        return Err(format!(
            "contenitore non srotolato: testo {:?}",
            unwrapped.text
        ));
    }

    // Un tipo sconosciuto resta in chat, non sparisce.
    let unknown = build(&wa::Message {
        poll_creation_message: MessageField::some(wa::message::PollCreationMessage::default()),
        ..Default::default()
    });
    if unknown.kind != Kind::Other || unknown.text.is_empty() {
        return Err("tipo sconosciuto scartato in silenzio".to_string());
    }

    let _ = Extracted::Skip;
    Ok("testo, foto (una volta), GIF, vocale, documento, sticker, posizione, citazione, contenitori, sconosciuto"
        .to_string())
}

fn check_media(root: &Path) -> Result<String, String> {
    // Immagine generata: gradiente con un cerchio, salvata come PNG.
    let mut image = image::RgbaImage::new(640, 360);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let dx = x as i32 - 320;
        let dy = y as i32 - 180;
        let inside = dx * dx + dy * dy < 90 * 90;
        *pixel = if inside {
            image::Rgba([250, 250, 250, 255])
        } else {
            image::Rgba([(x * 255 / 640) as u8, 20, 40, 255])
        };
    }
    let mut cursor = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|error| format!("PNG di prova: {error}"))?;
    let png = cursor.into_inner();

    let decoded = crate::media::decode_image(&png).ok_or("immagine non decodificata")?;
    if (decoded.width, decoded.height) != (640, 360) {
        return Err(format!(
            "dimensioni lette {}×{}",
            decoded.width, decoded.height
        ));
    }
    let dimensions = crate::media::image_dimensions(&png).ok_or("dimensioni non lette")?;
    if dimensions != (640, 360) {
        return Err(format!("dimensioni dall'intestazione: {dimensions:?}"));
    }
    let thumb = crate::media::make_thumbnail(&png, 480).ok_or("miniatura non creata")?;
    let thumb_dimensions =
        crate::media::image_dimensions(&thumb).ok_or("miniatura illeggibile")?;
    if thumb_dimensions.0 > 480 || thumb_dimensions.1 > 480 {
        return Err(format!("miniatura troppo grande: {thumb_dimensions:?}"));
    }
    if thumb.len() > 64 * 1024 {
        return Err(format!(
            "miniatura troppo pesante: {} KB",
            thumb.len() / 1024
        ));
    }

    let file = root.join("prova.png");
    std::fs::write(&file, &png).map_err(|error| format!("scrittura: {error}"))?;
    if !crate::media::is_image_path(&file) {
        return Err("estensione immagine non riconosciuta".to_string());
    }
    if crate::media::mime_for_path(&file) != "image/png" {
        return Err(format!(
            "tipo MIME errato: {}",
            crate::media::mime_for_path(&file)
        ));
    }

    // Video: serve ffmpeg. Se manca, si dice e non si finge che vada bene.
    if !crate::media::ffmpeg_available() {
        return Ok(format!(
            "immagine {}×{} + miniatura {}×{} · video saltato (ffmpeg assente)",
            decoded.width, decoded.height, thumb_dimensions.0, thumb_dimensions.1
        ));
    }
    let video = root.join("prova.mp4");
    let status = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x180:rate=15",
            "-t",
            "2",
            "-pix_fmt",
            "yuv420p",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
        ])
        .arg(&video)
        .status()
        .map_err(|error| format!("ffmpeg: {error}"))?;
    if !status.success() {
        return Err("ffmpeg non è riuscito a creare il video di prova".to_string());
    }
    let info = crate::media::probe_video(&video);
    // `ffprobe` arriva con ffmpeg, ma non su tutte le distribuzioni: se manca
    // si controlla lo stesso il fotogramma, senza inventare la durata.
    if crate::media::ffprobe_available() && (info.duration < 1.0 || info.width == 0) {
        return Err(format!(
            "video di prova letto male: {} s, {}×{}",
            info.duration, info.width, info.height
        ));
    }
    let poster = crate::media::video_thumbnail(&video).ok_or("miniatura del video non creata")?;
    let poster_size =
        crate::media::image_dimensions(&poster).ok_or("miniatura del video illeggibile")?;
    Ok(format!(
        "immagine {}×{} con miniatura {}×{}, video {:.1} s {}×{} con fotogramma {}×{} ({} KB)",
        decoded.width,
        decoded.height,
        thumb_dimensions.0,
        thumb_dimensions.1,
        info.duration,
        info.width,
        info.height,
        poster_size.0,
        poster_size.1,
        poster.len() / 1024
    ))
}

fn check_config(root: &Path) -> Result<String, String> {
    let dir = root.join("config");
    std::fs::create_dir_all(&dir).map_err(|error| format!("cartella: {error}"))?;
    let mut config = Config::default();
    config.system_player = true;
    config.theme = "light".to_string();
    config.camera = "/dev/video7".to_string();
    config.save(&dir);
    let reloaded = Config::load(&dir);
    if !reloaded.system_player || reloaded.theme != "light" || reloaded.camera != "/dev/video7" {
        return Err("le preferenze non tornano indietro".to_string());
    }
    std::fs::write(Config::path(&dir), "{ rotto").map_err(|error| format!("{error}"))?;
    let fallback = Config::load(&dir);
    if fallback.theme != "auto" {
        return Err("un file corrotto non riporta ai valori predefiniti".to_string());
    }
    Ok("salvataggio, rilettura e file corrotto".to_string())
}

fn check_theme(root: &Path) -> Result<String, String> {
    let home = root.join("home-tema");
    std::fs::create_dir_all(home.join(".config/gtk-3.0")).map_err(|error| format!("{error}"))?;
    let settings = home.join(".config/gtk-3.0/settings.ini");
    std::fs::write(
        &settings,
        "[Settings]\ngtk-theme-name=Adwaita\n",
    )
    .map_err(|error| format!("{error}"))?;

    let previous_home = std::env::var_os("HOME");
    let previous_theme = std::env::var_os("WHATS_OPEN_THEME");
    let previous_gtk = std::env::var_os("GTK_THEME");
    let previous_dbus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS");
    // Da qui in poi l'ambiente è nostro: il collaudo è a thread singolo.
    std::env::set_var("HOME", &home);
    std::env::remove_var("WHATS_OPEN_THEME");
    std::env::remove_var("GTK_THEME");
    std::env::remove_var("DBUS_SESSION_BUS_ADDRESS");

    let (light, light_source) = crate::theme::detect();
    let mut result = Ok(());
    if light != crate::theme::Mode::Light {
        result = Err(format!("tema chiaro letto come {light:?}"));
    }
    if !light_source.contains("GTK") {
        result = Err(format!("origine del tema inattesa: {light_source}"));
    }

    if result.is_ok() {
        std::fs::write(
            &settings,
            "[Settings]\ngtk-application-prefer-dark-theme=1\n",
        )
        .ok();
        let (dark, dark_source) = crate::theme::detect();
        if dark != crate::theme::Mode::Dark {
            result = Err(format!("preferenza scura letta come {dark:?}"));
        } else if !dark_source.contains("scuro") {
            result = Err(format!("origine della preferenza scura: {dark_source}"));
        }
    }

    // GTK_THEME vince sui file.
    if result.is_ok() {
        std::env::set_var("GTK_THEME", "Breeze-Dark");
        let (dark, source) = crate::theme::detect();
        if dark != crate::theme::Mode::Dark || !source.contains("GTK_THEME") {
            result = Err(format!("GTK_THEME ignorato: {dark:?} ({source})"));
        }
    }

    // KDE: con i file GTK assenti resta `kdeglobals`.
    if result.is_ok() {
        let kde_home = root.join("home-kde");
        std::fs::create_dir_all(kde_home.join(".config")).ok();
        std::fs::write(
            kde_home.join(".config/kdeglobals"),
            "[Colors:Window]\nBackgroundNormal=245,246,247\n",
        )
        .ok();
        std::env::set_var("HOME", &kde_home);
        std::env::remove_var("GTK_THEME");
        let (mode, source) = crate::theme::detect();
        if mode != crate::theme::Mode::Light || !source.contains("KDE") {
            result = Err(format!("KDE letto male: {mode:?} ({source})"));
        }
    }

    {
        match previous_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match previous_theme {
            Some(value) => std::env::set_var("WHATS_OPEN_THEME", value),
            None => std::env::remove_var("WHATS_OPEN_THEME"),
        }
        match previous_gtk {
            Some(value) => std::env::set_var("GTK_THEME", value),
            None => std::env::remove_var("GTK_THEME"),
        }
        match previous_dbus {
            Some(value) => std::env::set_var("DBUS_SESSION_BUS_ADDRESS", value),
            None => std::env::remove_var("DBUS_SESSION_BUS_ADDRESS"),
        }
    }
    result.map(|_| "gtk-theme-name, gtk-application-prefer-dark-theme, GTK_THEME, kdeglobals".to_string())
}

/// Nomi, iniziali, numeri e formati brevi: è quello che l'utente vede
/// nell'elenco delle chat, dove non deve mai comparire un numero al posto di
/// un nome conosciuto.
fn check_formatting() -> Result<String, String> {
    let expected = [
        (
            "display_name numero",
            util::display_name("", "393331112222@s.whatsapp.net"),
            "+393331112222".to_string(),
        ),
        (
            "display_name nome",
            util::display_name("  Anna  ", "393331112222@s.whatsapp.net"),
            "Anna".to_string(),
        ),
        (
            "display_name LID",
            util::display_name("", "1234567890@lid"),
            "Contatto".to_string(),
        ),
        (
            "display_name gruppo",
            util::display_name("", "120363000000000000@g.us"),
            "Gruppo".to_string(),
        ),
        ("iniziali", util::initials("Marta Bianchi"), "MB".to_string()),
        ("iniziali vuote", util::initials(""), "?".to_string()),
        ("peso", util::human_size(2048), "2 KB".to_string()),
        ("peso piccolo", util::human_size(512), "512 B".to_string()),
        ("durata", util::human_duration(75), "1:15".to_string()),
        ("durata lunga", util::human_duration(3675), "1:01:15".to_string()),
        (
            "jid utente",
            util::jid_user("393331112222:12@s.whatsapp.net"),
            "393331112222".to_string(),
        ),
        (
            "jid gruppo",
            util::jid_user("120363000000000000@g.us"),
            "120363000000000000".to_string(),
        ),
    ];
    for (label, actual, wanted) in expected {
        if actual != wanted {
            return Err(format!("{label}: {actual:?} invece di {wanted:?}"));
        }
    }
    if !util::jid_is_group("120363@g.us") || util::jid_is_group("39333@s.whatsapp.net") {
        return Err("gruppi e contatti confusi".to_string());
    }
    if !util::jid_is_newsletter("120363@newsletter") || !util::jid_is_status("status@broadcast") {
        return Err("canali o stati non riconosciuti".to_string());
    }
    if !util::looks_like_phone("+39 348 111 2222") || util::looks_like_phone("Anna") {
        return Err("numeri di telefono riconosciuti male".to_string());
    }
    let cut = util::truncate("ciao mondo", 6);
    if cut.chars().count() != 6 || !cut.ends_with('…') {
        return Err(format!("testo accorciato male: {cut:?}"));
    }
    // Gli id dei messaggi devono essere nel formato di WhatsApp e non ripetersi.
    let first = util::new_message_id();
    let second = util::new_message_id();
    if first.len() != 16 || !first.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase())
    {
        return Err(format!("id messaggio fuori formato: {first}"));
    }
    if first == second {
        return Err("due id messaggio uguali".to_string());
    }
    Ok("nomi, iniziali, numeri, gruppi, canali, stati, pesi, durate, id".to_string())
}

/// Contrasto WCAG fra due colori: serve a garantire che il testo si legga sia
/// col tema scuro sia con quello chiaro, senza doverlo guardare a occhio.
fn check_palette() -> Result<String, String> {
    fn luminance(color: egui::Color32) -> f32 {
        let channel = |value: u8| {
            let value = value as f32 / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
    }
    fn contrast(a: egui::Color32, b: egui::Color32) -> f32 {
        let (first, second) = (luminance(a), luminance(b));
        let (high, low) = if first > second {
            (first, second)
        } else {
            (second, first)
        };
        (high + 0.05) / (low + 0.05)
    }

    let mut report = Vec::new();
    for (name, palette) in [
        ("scuro", crate::theme::Palette::dark()),
        ("chiaro", crate::theme::Palette::light()),
    ] {
        for (label, text, background) in [
            ("testo", palette.text, palette.bg),
            ("fumetto mio", palette.own_text, palette.own_bubble),
            ("fumetto ricevuto", palette.in_text, palette.in_bubble),
        ] {
            let ratio = contrast(text, background);
            if ratio < 4.5 {
                return Err(format!(
                    "tema {name}: {label} troppo poco contrastato ({ratio:.1}:1)"
                ));
            }
            report.push(format!("{name}/{label} {ratio:.1}:1"));
        }
    }
    Ok(report.join(", "))
}

fn check_contacts(root: &Path) -> Result<String, String> {
    let text = "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Marta Bianchi\r\n\
                TEL;TYPE=CELL:+39 348 111 2222\r\nEND:VCARD\r\n\
                BEGIN:VCARD\r\nN:Neri;Paolo;;;\r\nTEL:3339998877\r\nEND:VCARD\r\n";
    let entries = util::parse_vcards(text);
    if entries.len() != 2 {
        return Err(format!("letti {} contatti su 2", entries.len()));
    }
    if entries[0].name != "Marta Bianchi" || entries[1].name != "Paolo Neri" {
        return Err(format!(
            "nomi letti: {:?} / {:?}",
            entries[0].name, entries[1].name
        ));
    }
    // Lo stesso numero scritto in due modi deve incontrarsi.
    let a = util::phone_key("+39 348 111 2222");
    let b = util::phone_key("3481112222");
    if a != b {
        return Err(format!("numeri equivalenti non confrontabili: {a} / {b}"));
    }
    let file = root.join("rubrica.vcf");
    std::fs::write(&file, text).map_err(|error| format!("{error}"))?;
    let read_back = std::fs::read_to_string(&file).map_err(|error| format!("{error}"))?;
    if util::parse_vcards(&read_back).len() != 2 {
        return Err("rilettura del file .vcf".to_string());
    }
    Ok("2 contatti, FN e N, numeri con + e senza".to_string())
}

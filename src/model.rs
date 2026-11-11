//! Modello locale: chat, messaggi e metadati dei media.

use crate::util;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Image,
    /// Clip inviata come GIF: WhatsApp la trasporta come video con
    /// `gifPlayback`, ma va mostrata e riprodotta in loop come una GIF.
    Gif,
    Video,
    Voice,
    Audio,
    Document,
    Sticker,
    Location,
    Contact,
    System,
    Other,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Image => "image",
            Kind::Gif => "gif",
            Kind::Video => "video",
            Kind::Voice => "voice",
            Kind::Audio => "audio",
            Kind::Document => "document",
            Kind::Sticker => "sticker",
            Kind::Location => "location",
            Kind::Contact => "contact",
            Kind::System => "system",
            Kind::Other => "other",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "text" => Kind::Text,
            "image" => Kind::Image,
            "gif" => Kind::Gif,
            "video" => Kind::Video,
            "voice" => Kind::Voice,
            "audio" => Kind::Audio,
            "document" => Kind::Document,
            "sticker" => Kind::Sticker,
            "location" => Kind::Location,
            "contact" => Kind::Contact,
            "system" => Kind::System,
            _ => Kind::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Text => "",
            Kind::Image => "Foto",
            Kind::Gif => "GIF",
            Kind::Video => "Video",
            Kind::Voice => "Messaggio vocale",
            Kind::Audio => "Audio",
            Kind::Document => "Documento",
            Kind::Sticker => "Sticker",
            Kind::Location => "Posizione",
            Kind::Contact => "Contatto",
            Kind::System => "Sistema",
            Kind::Other => "Messaggio",
        }
    }

    pub fn is_media(self) -> bool {
        matches!(
            self,
            Kind::Image
                | Kind::Gif
                | Kind::Video
                | Kind::Voice
                | Kind::Audio
                | Kind::Document
                | Kind::Sticker
        )
    }

    pub fn is_playable(self) -> bool {
        matches!(self, Kind::Video | Kind::Gif | Kind::Voice | Kind::Audio)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Pending,
    Sent,
    Delivered,
    Read,
    Played,
    Failed,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::Sent => "sent",
            Status::Delivered => "delivered",
            Status::Read => "read",
            Status::Played => "played",
            Status::Failed => "failed",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "pending" => Status::Pending,
            "delivered" => Status::Delivered,
            "read" => Status::Read,
            "played" => Status::Played,
            "failed" => Status::Failed,
            _ => Status::Sent,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Quote {
    pub id: String,
    pub sender: String,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct Message {
    pub id: String,
    pub chat_id: String,
    pub sender: String,
    pub sender_name: String,
    pub from_me: bool,
    pub ts: i64,
    pub kind: Kind,
    pub text: String,
    pub status: Status,
    pub view_once: bool,
    pub played: bool,
    /// Messaggio protobuf originale: serve per scaricare/decifrare i media.
    pub payload: Vec<u8>,
    pub thumb: Option<Vec<u8>>,
    pub mime: String,
    pub file_name: String,
    pub file_size: u64,
    pub duration: u32,
    pub quote: Option<Quote>,
    pub edited: bool,
    pub deleted: bool,
    pub expire_ts: i64,
    pub reaction: Option<String>,
    /// Sticker animato (WebP animato o Lottie): in bolla si vede il primo
    /// fotogramma, con l'etichetta "animato".
    pub animated: bool,
    /// Miniatura dell'allegato che stiamo inviando (solo in memoria).
    pub local_preview: Option<Vec<u8>>,
}

impl Default for Message {
    fn default() -> Self {
        Self {
            id: String::new(),
            chat_id: String::new(),
            sender: String::new(),
            sender_name: String::new(),
            from_me: false,
            ts: 0,
            kind: Kind::Text,
            text: String::new(),
            status: Status::Sent,
            view_once: false,
            played: false,
            payload: Vec::new(),
            thumb: None,
            mime: String::new(),
            file_name: String::new(),
            file_size: 0,
            duration: 0,
            quote: None,
            edited: false,
            deleted: false,
            expire_ts: 0,
            reaction: None,
            animated: false,
            local_preview: None,
        }
    }
}

impl Message {
    pub fn body(&self) -> String {
        if self.deleted {
            return "Questo messaggio è stato eliminato".to_string();
        }
        if !self.text.is_empty() {
            return self.text.clone();
        }
        match self.kind {
            Kind::Sticker => String::new(),
            Kind::Gif => "GIF".to_string(),
            _ => self.kind.label().to_string(),
        }
    }

    /// Riga breve usata nella lista chat.
    pub fn preview(&self) -> String {
        if self.deleted {
            return "Messaggio eliminato".to_string();
        }
        if self.view_once && !self.played {
            return match self.kind {
                Kind::Voice | Kind::Audio => "Vocale · una volta".to_string(),
                Kind::Video | Kind::Gif => "Video · una volta".to_string(),
                Kind::Image => "Foto · una volta".to_string(),
                _ => "Messaggio · una volta".to_string(),
            };
        }
        let body = if !self.text.is_empty() {
            self.text.clone()
        } else {
            match self.kind {
                Kind::Voice => format!("Vocale · {}", util::human_duration(self.duration)),
                Kind::Video => "Video".to_string(),
                Kind::Gif => "GIF".to_string(),
                Kind::Image => "Foto".to_string(),
                Kind::Sticker => "Sticker".to_string(),
                Kind::Document => {
                    if self.file_name.is_empty() {
                        "Documento".to_string()
                    } else {
                        self.file_name.clone()
                    }
                }
                other => other.label().to_string(),
            }
        };
        util::truncate(&body.replace('\n', " "), 46)
    }

    /// Anteprima per la lista chat: "Tu:" sui propri messaggi e il nome del
    /// mittente nei gruppi, come fa WhatsApp.
    pub fn chat_preview(&self) -> String {
        let body = self.preview();
        if body.is_empty() || self.deleted {
            return body;
        }
        if self.from_me {
            return format!("Tu: {body}");
        }
        let sender = self.sender_name.trim();
        if !sender.is_empty() && crate::util::jid_is_group(&self.chat_id) {
            return format!("{sender}: {body}");
        }
        body
    }

    pub fn media_ext(&self) -> &'static str {
        match self.kind {
            Kind::Image => "jpg",
            Kind::Gif | Kind::Video => "mp4",
            Kind::Voice => "ogg",
            Kind::Audio => "m4a",
            Kind::Document => "bin",
            Kind::Sticker => "webp",
            _ => "bin",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Chat {
    pub id: String,
    pub name: String,
    pub is_group: bool,
    pub last_ts: i64,
    pub preview: String,
    pub unread: u32,
    pub muted: bool,
    pub pinned: bool,
    // Solo runtime, non persistiti.
    pub typing: bool,
    pub online: bool,
    pub last_seen: Option<i64>,
}

impl Default for Chat {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            is_group: false,
            last_ts: 0,
            preview: String::new(),
            unread: 0,
            muted: false,
            pinned: false,
            typing: false,
            online: false,
            last_seen: None,
        }
    }
}

impl Chat {
    pub fn title(&self) -> String {
        util::display_name(&self.name, &self.id)
    }

    pub fn subtitle(&self) -> String {
        util::display_name(&self.name, &self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_message(text: &str) -> Message {
        Message {
            text: text.to_string(),
            kind: Kind::Text,
            ..Default::default()
        }
    }

    #[test]
    fn anteprima_dei_diversi_tipi() {
        assert_eq!(text_message("buongiorno").preview(), "buongiorno");
        assert_eq!(text_message("").body(), "");

        let voice = Message {
            kind: Kind::Voice,
            duration: 65,
            ..Default::default()
        };
        assert_eq!(voice.preview(), "Vocale · 1:05");

        let doc = Message {
            kind: Kind::Document,
            file_name: "contratto.pdf".to_string(),
            ..Default::default()
        };
        assert_eq!(doc.preview(), "contratto.pdf");
        let mut own = text_message("ciao");
        own.from_me = true;
        assert_eq!(own.chat_preview(), "Tu: ciao");
        let mut group = text_message("ciao");
        group.chat_id = "120363000000000000@g.us".to_string();
        group.sender_name = "Anna".to_string();
        assert_eq!(group.chat_preview(), "Anna: ciao");

        let deleted = Message {
            deleted: true,
            ..Default::default()
        };
        assert_eq!(deleted.body(), "Questo messaggio è stato eliminato");
    }

    #[test]
    fn tipi_e_stati_fanno_avanti_e_indietro() {
        for kind in [
            Kind::Text,
            Kind::Image,
            Kind::Video,
            Kind::Voice,
            Kind::Audio,
            Kind::Document,
            Kind::Sticker,
            Kind::Location,
            Kind::Contact,
            Kind::System,
            Kind::Other,
        ] {
            assert_eq!(Kind::from_str(kind.as_str()), kind);
        }
        for status in [
            Status::Pending,
            Status::Sent,
            Status::Delivered,
            Status::Read,
            Status::Played,
            Status::Failed,
        ] {
            assert_eq!(Status::from_str(status.as_str()), status);
        }
        assert!(Status::Read > Status::Delivered);
        assert!(Status::Delivered > Status::Sent);
    }

    #[test]
    fn estensione_dei_media() {
        assert_eq!(text_message("").media_ext(), "bin");
        let video = Message {
            kind: Kind::Video,
            ..Default::default()
        };
        assert_eq!(video.media_ext(), "mp4");
    }
}

/// Nome noto per un contatto, con la sua provenienza.
///
/// WhatsApp riempie i nomi da tre sorgenti diverse: la rubrica sincronizzata
/// dal telefono (`rank` 3), il push name scelto dal contatto (2) e il nome
/// della conversazione nello storico (1). Teniamo il più autorevole: senza
/// questa scala l'ultimo evento ricevuto sovrascriverebbe un nome salvato in
/// rubrica con un push name.
#[derive(Clone, Debug, Default)]
pub struct Contact {
    pub jid: String,
    pub name: String,
    pub rank: u8,
    /// Numero di telefono, quando il JID è un LID e conosciamo la mappatura.
    pub phone: String,
    /// Foto profilo (anteprima) già scaricata.
    pub avatar: Option<Vec<u8>>,
}

impl Contact {
    pub fn empty(jid: &str) -> Self {
        Self {
            jid: jid.to_string(),
            ..Default::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.name.trim().is_empty() && self.phone.is_empty() && self.avatar.is_none()
    }
}

/// Nome da mostrare per un contatto, con fallback sul numero.
pub fn contact_display(contact: Option<&Contact>, chat: &Chat) -> String {
    if let Some(contact) = contact {
        if !contact.name.trim().is_empty() {
            return contact.name.trim().to_string();
        }
        if !contact.phone.is_empty() {
            return util::phone_display(&contact.phone);
        }
    }
    util::display_name(&chat.name, &chat.id)
}

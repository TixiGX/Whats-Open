//! Traduzione dei protobuf di WhatsApp nel modello locale.
//!
//! La libreria fornisce `MessageExt`, che srotola i contenitori
//! (device-sent, effimeri, view-once, didascalie, modifiche) e lascia il
//! contenuto vero. Da lì estraiamo testo, tipo e metadati dei media.

use whatsapp_rust::prelude::*;
use whatsapp_rust::waproto::codec;
use whatsapp_rust::waproto::whatsapp as wa;

use crate::model::{Kind, Message, Quote, Status};

pub enum Extracted {
    New(Message),
    Edit {
        chat: String,
        target: String,
        text: String,
        ts: i64,
    },
    Revoke {
        chat: String,
        target: String,
    },
    Reaction {
        chat: String,
        target: String,
        emoji: Option<String>,
    },
    Skip,
}

fn context_of(base: &wa::Message) -> Option<&wa::ContextInfo> {
    if let Some(m) = base.extended_text_message.as_option() {
        return m.context_info.as_option();
    }
    if let Some(m) = base.image_message.as_option() {
        return m.context_info.as_option();
    }
    if let Some(m) = base.video_message.as_option() {
        return m.context_info.as_option();
    }
    if let Some(m) = base.audio_message.as_option() {
        return m.context_info.as_option();
    }
    if let Some(m) = base.document_message.as_option() {
        return m.context_info.as_option();
    }
    if let Some(m) = base.sticker_message.as_option() {
        return m.context_info.as_option();
    }
    if let Some(m) = base.location_message.as_option() {
        return m.context_info.as_option();
    }
    if let Some(m) = base.contact_message.as_option() {
        return m.context_info.as_option();
    }
    None
}

/// Anteprima compatta di un messaggio citato.
pub fn quoted_preview(message: &wa::Message) -> String {
    let base = message.get_base_message();
    if let Some(text) = base.text_content() {
        return crate::util::truncate(text, 90);
    }
    if let Some(caption) = base.get_caption() {
        return crate::util::truncate(caption, 90);
    }
    if base.image_message.is_set() {
        return "Foto".to_string();
    }
    if base.video_message.is_set() || base.ptv_message.is_set() {
        return "Video".to_string();
    }
    if let Some(audio) = base.audio_message.as_option() {
        let seconds = audio.seconds.unwrap_or(0);
        return format!("Vocale · {}", crate::util::human_duration(seconds));
    }
    if let Some(doc) = base.document_message.as_option() {
        return doc
            .file_name
            .clone()
            .or_else(|| doc.title.clone())
            .unwrap_or_else(|| "Documento".to_string());
    }
    if base.sticker_message.is_set() {
        return "Sticker".to_string();
    }
    "Messaggio".to_string()
}

/// Costruisce un messaggio del modello a partire dal protobuf, senza
/// occuparsi di modifiche/revoche/reazioni.
pub fn build(
    message: &wa::Message,
    chat: String,
    sender: String,
    sender_name: String,
    from_me: bool,
    ts: i64,
) -> Message {
    let base = message.get_base_message();

    let mut out = Message {
        id: String::new(),
        chat_id: chat,
        sender,
        sender_name,
        from_me,
        ts,
        status: Status::Sent,
        view_once: message.is_view_once(),
        payload: codec::message_to_vec(message),
        ..Default::default()
    };

    if let Some(context) = context_of(base) {
        if let Some(quoted) = context.quoted_message.as_option() {
            out.quote = Some(Quote {
                id: context.stanza_id.clone().unwrap_or_default(),
                sender: context
                    .participant
                    .clone()
                    .map(|jid| crate::util::jid_user(&jid))
                    .unwrap_or_default(),
                text: quoted_preview(quoted),
            });
        }
        if let Some(expiration) = context.expiration {
            if expiration > 0 {
                out.expire_ts = out.ts + expiration as i64;
            }
        }
    }

    if let Some(image) = base.image_message.as_option() {
        out.kind = Kind::Image;
        out.text = image
            .caption
            .clone()
            .or_else(|| base.get_caption().map(str::to_string))
            .unwrap_or_default();
        out.mime = image.mimetype.clone().unwrap_or_else(|| "image/jpeg".into());
        out.thumb = image.jpeg_thumbnail.clone();
        out.file_size = image.file_length.unwrap_or(0);
        return out;
    }

    if let Some(video) = base.video_message.as_option() {
        out.kind = Kind::Video;
        out.text = video
            .caption
            .clone()
            .or_else(|| base.get_caption().map(str::to_string))
            .unwrap_or_default();
        out.mime = video.mimetype.clone().unwrap_or_else(|| "video/mp4".into());
        out.thumb = video.jpeg_thumbnail.clone();
        out.file_size = video.file_length.unwrap_or(0);
        out.duration = video.seconds.unwrap_or(0);
        return out;
    }

    if let Some(video) = base.ptv_message.as_option() {
        out.kind = Kind::Video;
        out.mime = video.mimetype.clone().unwrap_or_else(|| "video/mp4".into());
        out.thumb = video.jpeg_thumbnail.clone();
        out.file_size = video.file_length.unwrap_or(0);
        out.duration = video.seconds.unwrap_or(0);
        return out;
    }

    if let Some(audio) = base.audio_message.as_option() {
        out.kind = if audio.ptt.unwrap_or(false) {
            Kind::Voice
        } else {
            Kind::Audio
        };
        out.mime = audio
            .mimetype
            .clone()
            .unwrap_or_else(|| "audio/ogg; codecs=opus".into());
        out.duration = audio.seconds.unwrap_or(0);
        out.file_size = audio.file_length.unwrap_or(0);
        return out;
    }

    if let Some(document) = base.document_message.as_option() {
        out.kind = Kind::Document;
        out.mime = document
            .mimetype
            .clone()
            .unwrap_or_else(|| "application/octet-stream".into());
        out.file_name = document
            .file_name
            .clone()
            .or_else(|| document.title.clone())
            .unwrap_or_default();
        out.text = document
            .caption
            .clone()
            .or_else(|| base.get_caption().map(str::to_string))
            .unwrap_or_default();
        out.thumb = document.jpeg_thumbnail.clone();
        out.file_size = document.file_length.unwrap_or(0);
        return out;
    }

    if let Some(sticker) = base.sticker_message.as_option() {
        out.kind = Kind::Sticker;
        out.mime = sticker
            .mimetype
            .clone()
            .unwrap_or_else(|| "image/webp".into());
        out.thumb = sticker.png_thumbnail.clone();
        out.file_size = sticker.file_length.unwrap_or(0);
        return out;
    }

    if let Some(location) = base.location_message.as_option() {
        out.kind = Kind::Location;
        out.text = location
            .name
            .clone()
            .or_else(|| location.address.clone())
            .unwrap_or_else(|| "Posizione".to_string());
        out.thumb = location.jpeg_thumbnail.clone();
        return out;
    }

    if let Some(contact) = base.contact_message.as_option() {
        out.kind = Kind::Contact;
        out.text = contact
            .display_name
            .clone()
            .unwrap_or_else(|| "Contatto".to_string());
        return out;
    }

    if let Some(text) = base.text_content() {
        out.kind = Kind::Text;
        out.text = text.to_string();
        return out;
    }

    if base.poll_creation_message.is_set()
        || base.poll_creation_message_v2.is_set()
        || base.poll_creation_message_v3.is_set()
    {
        out.kind = Kind::Other;
        out.text = "Sondaggio".to_string();
        return out;
    }

    out.kind = Kind::Other;
    out
}

pub fn extract(message: &wa::Message, info: &MessageInfo) -> Extracted {
    let chat = info.source.chat.to_string();
    let base = message.get_base_message();

    // Modifiche, revoche e reazioni non sono bolle nuove: aggiornano una
    // bolla già presente.
    if let Some(protocol) = base.protocol_message.as_option() {
        let target = protocol
            .key
            .as_option()
            .and_then(|key| key.id.clone())
            .unwrap_or_default();
        if target.is_empty() {
            return Extracted::Skip;
        }
        return match protocol.r#type {
            Some(wa::message::protocol_message::Type::MESSAGE_EDIT) => {
                let text = protocol
                    .edited_message
                    .as_option()
                    .map(|edited| {
                        edited
                            .get_base_message()
                            .text_content()
                            .unwrap_or_default()
                            .to_string()
                    })
                    .unwrap_or_default();
                Extracted::Edit {
                    chat,
                    target,
                    text,
                    ts: info.timestamp.timestamp(),
                }
            }
            Some(wa::message::protocol_message::Type::REVOKE) => Extracted::Revoke { chat, target },
            _ => Extracted::Skip,
        };
    }

    if let Some(reaction) = base.reaction_message.as_option() {
        let target = reaction
            .key
            .as_option()
            .and_then(|key| key.id.clone())
            .unwrap_or_default();
        if target.is_empty() {
            return Extracted::Skip;
        }
        let emoji = reaction.text.clone().filter(|text| !text.trim().is_empty());
        return Extracted::Reaction {
            chat,
            target,
            emoji,
        };
    }

    let mut out = build(
        message,
        chat,
        info.source.sender.to_string(),
        info.push_name.clone(),
        info.source.is_from_me,
        info.timestamp.timestamp(),
    );
    out.id = info.id.clone();
    Extracted::New(out)
}

/// Conver­te una voce dello storico sincronizzato all'accoppiamento.
pub fn from_history(chat: &str, entry: &wa::WebMessageInfo) -> Option<Message> {
    let key = entry.key.as_option()?;
    let message = entry.message.as_option()?;
    let id = key.id.clone().unwrap_or_default();
    if id.is_empty() {
        return None;
    }
    let base = message.get_base_message();
    if base.protocol_message.is_set() || base.reaction_message.is_set() {
        return None;
    }

    let from_me = key.from_me.unwrap_or(false);
    let sender = key
        .participant
        .clone()
        .or_else(|| key.remote_jid.clone())
        .unwrap_or_else(|| chat.to_string());
    let ts = entry.message_timestamp.unwrap_or(0) as i64;

    let mut out = build(
        message,
        chat.to_string(),
        sender,
        entry.push_name.clone().unwrap_or_default(),
        from_me,
        ts,
    );
    out.id = id;
    out.status = Status::Read;
    Some(out)
}

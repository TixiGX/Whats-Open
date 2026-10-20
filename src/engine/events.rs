//! Gestione degli eventi in arrivo dalla sessione WhatsApp.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use whatsapp_rust::prelude::*;
use whatsapp_rust::types::presence::{ChatPresence, ReceiptType};
use whatsapp_rust::types::events::Event as WaEvent;

use super::call;
use super::extract::{self, Extracted};
use super::{emit, fetch_avatar, Event, EventSink};
use crate::model::{Chat, Message, Status};

pub async fn handle(
    event: Arc<WaEvent>,
    client: Arc<Client>,
    sink: EventSink,
    groups: Arc<Mutex<HashSet<String>>>,
    calls: call::SharedCall,
) {
    match event.as_ref() {
        WaEvent::Messages(batch) => {
            let mut out: Vec<Message> = Vec::new();
            for inbound in batch.messages.iter() {
                learn_from_info(&sink, inbound.info.as_ref());
                match extract::extract(inbound.message.as_ref(), inbound.info.as_ref()) {
                    Extracted::New(message) => {
                        if message.kind == crate::model::Kind::Other && message.text.is_empty() {
                            continue;
                        }
                        ensure_group_name(&client, &sink, &groups, &message.chat_id);
                        out.push(message);
                    }
                    Extracted::Edit {
                        chat,
                        target,
                        text,
                        ts: _,
                    } => emit(&sink, Event::Edited { chat, id: target, text }),
                    Extracted::Revoke { chat, target } => {
                        emit(&sink, Event::Revoked { chat, id: target })
                    }
                    Extracted::Reaction { chat, target, emoji } => {
                        emit(&sink, Event::Reaction { chat, id: target, emoji })
                    }
                    Extracted::Skip => {}
                }
            }
            if !out.is_empty() {
                emit(&sink, Event::Messages(out));
            }
        }
        WaEvent::Receipt(receipt) => {
            let status = match receipt.r#type {
                ReceiptType::Delivered => Some(Status::Delivered),
                ReceiptType::Read | ReceiptType::ReadSelf => Some(Status::Read),
                ReceiptType::Played | ReceiptType::PlayedSelf => Some(Status::Played),
                _ => None,
            };
            if let Some(status) = status {
                emit(
                    &sink,
                    Event::Status {
                        chat: receipt.source.chat.to_string(),
                        ids: receipt.message_ids.clone(),
                        status,
                    },
                );
            }
        }
        WaEvent::ChatPresence(update) => {
            emit(
                &sink,
                Event::Typing {
                    chat: update.source.chat.to_string(),
                    typing: matches!(update.state, ChatPresence::Composing),
                },
            );
        }
        WaEvent::Presence(update) => {
            emit(
                &sink,
                Event::Presence {
                    chat: update.from.to_string(),
                    online: !update.unavailable,
                    last_seen: update.last_seen.map(|value| value.timestamp()),
                },
            );
        }
        WaEvent::HistorySync(sync) => {
            import_history(sync, &client, &sink, &groups);
        }
        WaEvent::ContactUpdate(update) => {
            let jid = update.jid.to_string();
            let name = update
                .action
                .full_name
                .clone()
                .or_else(|| update.action.first_name.clone())
                .unwrap_or_default();
            if !name.trim().is_empty() {
                emit(
                    &sink,
                    Event::Contact {
                        jid: jid.clone(),
                        name,
                        rank: 3,
                    },
                );
            }
            if let (Some(lid), Some(pn)) = (update.action.lid_jid.clone(), update.action.pn_jid.clone())
            {
                emit_alias(&sink, &lid, &pn);
            }
        }
        WaEvent::PushNameUpdate(update) => {
            if !update.new_push_name.trim().is_empty() {
                emit(
                    &sink,
                    Event::Contact {
                        jid: update.jid.to_string(),
                        name: update.new_push_name.clone(),
                        rank: 2,
                    },
                );
            }
        }
        WaEvent::PictureUpdate(update) => {
            let jid = update.jid.to_string();
            if update.removed {
                emit(
                    &sink,
                    Event::Avatar {
                        jid,
                        bytes: None,
                    },
                );
            } else {
                let client = client.clone();
                let sink = sink.clone();
                tokio::spawn(async move {
                    let bytes = fetch_avatar(&client, &jid).await;
                    emit(&sink, Event::Avatar { jid, bytes });
                });
            }
        }
        WaEvent::ContactUpdated(update) => {
            let jid = update.jid.to_string();
            let client = client.clone();
            let sink = sink.clone();
            tokio::spawn(async move {
                let bytes = fetch_avatar(&client, &jid).await;
                emit(&sink, Event::Avatar { jid, bytes });
            });
        }
        WaEvent::IncomingCall(call) => {
            call::on_incoming(call.as_ref().clone(), &calls, &sink).await;
        }
        WaEvent::MissedCall(call) => {
            call::missed(call, &sink);
        }
        WaEvent::CallEndedElsewhere(_) => {
            emit(
                &sink,
                Event::Call {
                    phase: crate::engine::CallPhase::Ended,
                    peer: String::new(),
                    video: false,
                },
            );
        }
        WaEvent::UndecryptableMessage(_) => {
            emit(&sink, Event::Undecryptable);
        }
        WaEvent::Disconnected(info) => {
            emit(
                &sink,
                Event::Disconnected(format!("{:?}", info.reason)),
            );
        }
        WaEvent::StreamReplaced(_) => {
            emit(
                &sink,
                Event::Disconnected("sessione aperta su un altro dispositivo".into()),
            );
        }
        WaEvent::ConnectFailure(info) => {
            let message = info
                .message
                .clone()
                .unwrap_or_else(|| format!("{:?}", info.reason));
            emit(&sink, Event::Disconnected(message));
        }
        _ => {}
    }
}

/// Impara nome e numero di telefono da un messaggio in arrivo: ogni stanza
/// porta il push name del mittente e, per i DM, anche l'altro indirizzo
/// (LID o numero). Senza questo passaggio i contatti resterebbero anonimi
/// finché non arriva un evento dedicato, che WhatsApp manda di rado.
fn learn_from_info(sink: &EventSink, info: &MessageInfo) {
    let sender = clean_jid(&info.source.sender.to_string());
    if let Some(alt) = info.source.sender_alt.as_ref() {
        let alt = clean_jid(&alt.to_string());
        let (lid, pn) = if sender.ends_with("@lid") {
            (sender.clone(), alt)
        } else {
            (alt, sender.clone())
        };
        emit_alias(sink, &lid, &pn);
    }
    if info.source.is_from_me || info.push_name.trim().is_empty() {
        return;
    }
    let name = info.push_name.trim().to_string();
    emit(
        sink,
        Event::Contact {
            jid: sender.clone(),
            name: name.clone(),
            rank: 2,
        },
    );
    if !info.source.is_group {
        let chat = clean_jid(&info.source.chat.to_string());
        if chat != sender {
            emit(
                sink,
                Event::Contact {
                    jid: chat,
                    name,
                    rank: 2,
                },
            );
        }
    }
}

/// Manda alla UI la mappatura LID ↔ numero, scartando le righe incoerenti.
fn emit_alias(sink: &EventSink, lid: &str, pn: &str) {
    if lid.is_empty() || pn.is_empty() || !lid.ends_with("@lid") || !pn.ends_with("@s.whatsapp.net")
    {
        return;
    }
    emit(
        sink,
        Event::Alias {
            lid: lid.to_string(),
            pn: pn.to_string(),
        },
    );
}

/// Normalizza un JID togliendo il suffisso del dispositivo (`:12`).
fn clean_jid(raw: &str) -> String {
    raw.parse::<Jid>()
        .map(|jid| jid.to_non_ad().to_string())
        .unwrap_or_else(|_| raw.to_string())
}

fn ensure_group_name(
    client: &Arc<Client>,
    sink: &EventSink,
    groups: &Arc<Mutex<HashSet<String>>>,
    chat: &str,
) {
    if !chat.ends_with("@g.us") {
        return;
    }
    let fresh = match groups.lock() {
        Ok(mut set) => set.insert(chat.to_string()),
        Err(_) => true,
    };
    if !fresh {
        return;
    }

    let client = client.clone();
    let sink = sink.clone();
    let groups = groups.clone();
    let chat = chat.to_string();
    tokio::spawn(async move {
        fetch_group_name(client, sink, groups, chat).await;
    });
}

pub async fn fetch_group_name(
    client: Arc<Client>,
    sink: EventSink,
    _groups: Arc<Mutex<HashSet<String>>>,
    chat: String,
) {
    let Ok(jid) = chat.parse::<Jid>() else {
        return;
    };
    let feature = client.groups();
    if let Ok(metadata) = feature.get_metadata(&jid).await {
        if !metadata.subject.is_empty() {
            emit(
                &sink,
                Event::ChatName {
                    chat,
                    name: metadata.subject,
                },
            );
        }
    }
}

fn import_history(
    sync: &whatsapp_rust::types::events::LazyHistorySync,
    client: &Arc<Client>,
    sink: &EventSink,
    groups: &Arc<Mutex<HashSet<String>>>,
) {
    let Some(history) = sync.get() else {
        emit(
            sink,
            Event::History {
                chats: Vec::new(),
                messages: Vec::new(),
                done: true,
            },
        );
        return;
    };

    // Nomi e mappature arrivano insieme allo storico: sono la sorgente più
    // ricca di nomi per chi non ha ancora ricevuto un messaggio.
    for pushname in history.pushnames.iter() {
        let (Some(id), Some(name)) = (pushname.id.as_ref(), pushname.pushname.as_ref()) else {
            continue;
        };
        if name.trim().is_empty() {
            continue;
        }
        emit(
            sink,
            Event::Contact {
                jid: clean_jid(id),
                name: name.clone(),
                rank: 2,
            },
        );
    }
    for mapping in history.phone_number_to_lid_mappings.iter() {
        if let (Some(pn), Some(lid)) = (mapping.pn_jid.as_ref(), mapping.lid_jid.as_ref()) {
            emit_alias(sink, &clean_jid(lid), &clean_jid(pn));
        }
    }

    let mut chats: Vec<Chat> = Vec::new();
    let mut messages: Vec<Message> = Vec::new();
    let mut budget: usize = 6000;

    for conversation in history.conversations.iter() {
        let chat_id = conversation.id.clone();
        if chat_id.is_empty()
            || chat_id.starts_with("status@")
            || chat_id.ends_with("@broadcast")
        {
            continue;
        }

        let is_group = chat_id.ends_with("@g.us");
        let name = conversation.name.clone().unwrap_or_default();
        let mut last_ts = conversation.conversation_timestamp.unwrap_or(0) as i64;
        let mut preview = String::new();

        let total = conversation.messages.len();
        let start = total.saturating_sub(60);
        for entry in conversation.messages[start..].iter() {
            if budget == 0 {
                break;
            }
            let Some(raw) = entry.message.as_option() else {
                continue;
            };
            if let Some(message) = extract::from_history(&chat_id, raw) {
                if message.ts > last_ts {
                    last_ts = message.ts;
                }
                if preview.is_empty() {
                    preview = message.preview();
                }
                messages.push(message);
                budget -= 1;
            }
        }

        if is_group {
            ensure_group_name(client, sink, groups, &chat_id);
        } else if !name.trim().is_empty() {
            // Nella chat 1:1 il nome della conversazione è il nome in
            // rubrica: ha la precedenza sul push name.
            emit(
                sink,
                Event::Contact {
                    jid: chat_id.clone(),
                    name: name.clone(),
                    rank: 3,
                },
            );
        }

        chats.push(Chat {
            id: chat_id,
            name,
            is_group,
            last_ts,
            preview,
            unread: conversation.unread_count.unwrap_or(0),
            ..Default::default()
        });

        if budget == 0 {
            break;
        }
    }

    emit(
        sink,
        Event::History {
            chats,
            messages,
            done: true,
        },
    );
}

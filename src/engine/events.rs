//! Gestione degli eventi in arrivo dalla sessione WhatsApp.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use whatsapp_rust::prelude::*;
use whatsapp_rust::types::presence::{ChatPresence, ReceiptType};
use whatsapp_rust::types::events::Event as WaEvent;

use super::extract::{self, Extracted};
use super::{emit, Event, EventSink};
use crate::model::{Chat, Message, Status};

pub async fn handle(
    event: Arc<WaEvent>,
    client: Arc<Client>,
    sink: EventSink,
    groups: Arc<Mutex<HashSet<String>>>,
) {
    match event.as_ref() {
        WaEvent::Messages(batch) => {
            let mut out: Vec<Message> = Vec::new();
            for inbound in batch.messages.iter() {
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

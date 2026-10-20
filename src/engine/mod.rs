//! Ponte fra l'interfaccia e la rete.
//!
//! Un thread dedicato possiede il runtime Tokio e la sessione WhatsApp: la UI
//! gli manda comandi e riceve eventi già normalizzati, senza mai toccare
//! direttamente il socket o le chiavi.

pub mod events;
pub mod extract;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use whatsapp_rust::download::MediaType;
use whatsapp_rust::pair_code::PairCodeOptions;
use whatsapp_rust::prelude::*;
use whatsapp_rust::upload::UploadOptions;
use whatsapp_rust::waproto::codec;
use whatsapp_rust::waproto::whatsapp as wa;

use crate::model::{Chat, Message, Status};
use crate::util::Paths;

/// Messaggio a cui si sta rispondendo.
#[derive(Clone, Debug, Default)]
pub struct ReplyTarget {
    pub id: String,
    pub sender: String,
    pub payload: Vec<u8>,
}

pub enum Cmd {
    SendText {
        chat: String,
        text: String,
        local_id: String,
        reply: Option<ReplyTarget>,
    },
    SendFile {
        chat: String,
        path: PathBuf,
        caption: String,
        local_id: String,
        reply: Option<ReplyTarget>,
        /// Registrazione vocale: nota vocale con durata e forma d'onda.
        voice: Option<(u32, Vec<u8>)>,
    },
    MarkRead {
        chat: String,
        sender: Option<String>,
        ids: Vec<String>,
    },
    Played {
        chat: String,
        sender: Option<String>,
        ids: Vec<String>,
    },
    Download {
        req: u64,
        chat: String,
        id: String,
        payload: Vec<u8>,
        view_once: bool,
    },
    Typing {
        chat: String,
        composing: bool,
    },
    React {
        chat: String,
        id: String,
        from_me: bool,
        participant: Option<String>,
        emoji: String,
    },
    Edit {
        chat: String,
        id: String,
        text: String,
    },
    Revoke {
        chat: String,
        id: String,
        from_me: bool,
        sender: Option<String>,
    },
    GroupInfo {
        chat: String,
    },
    Quit,
}

#[derive(Debug)]
pub enum Event {
    Qr { code: String },
    PairCode { code: String },
    PairError(String),
    /// Sessione attiva: nome e JID del nostro account.
    Me { name: String, jid: String },
    Connected,
    Disconnected(String),
    LoggedOut,
    Fatal(String),
    Messages(Vec<Message>),
    History { chats: Vec<Chat>, messages: Vec<Message>, done: bool },
    ChatName { chat: String, name: String },
    Status { chat: String, ids: Vec<String>, status: Status },
    Typing { chat: String, typing: bool },
    Presence { chat: String, online: bool, last_seen: Option<i64> },
    Sent {
        chat: String,
        local_id: String,
        server_id: Option<String>,
        ts: i64,
        error: Option<String>,
    },
    Media {
        req: u64,
        chat: String,
        id: String,
        result: Result<Vec<u8>, String>,
    },
    Edited { chat: String, id: String, text: String },
    Revoked { chat: String, id: String },
    Reaction { chat: String, id: String, emoji: Option<String> },
    Undecryptable,
}

type Wake = Arc<dyn Fn() + Send + Sync>;

/// Recapito degli eventi verso la UI: una coda e un risveglio.
pub struct Sink {
    tx: std::sync::mpsc::Sender<Event>,
    wake: Option<Wake>,
}

type EventSink = Arc<Sink>;

pub struct Engine {
    cmd: UnboundedSender<Cmd>,
    pub events: std::sync::mpsc::Receiver<Event>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Engine {
    pub fn start(
        paths: Paths,
        pair_phone: Option<String>,
        wake: Option<Box<dyn Fn() + Send + Sync>>,
    ) -> Self {
        let (cmd_tx, cmd_rx) = unbounded_channel::<Cmd>();
        let (event_tx, event_rx) = std::sync::mpsc::channel::<Event>();
        let wake: Option<Wake> = wake.map(Arc::from);

        let join = std::thread::Builder::new()
            .name("whats-open-engine".into())
            .spawn(move || {
                run(paths, pair_phone, cmd_rx, event_tx, wake);
            })
            .ok();

        Self {
            cmd: cmd_tx,
            events: event_rx,
            join,
        }
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.cmd.send(cmd);
    }

    /// Chiude la sessione e attende (brevemente) la fine del thread.
    pub fn stop(mut self) {
        let _ = self.cmd.send(Cmd::Quit);
        if let Some(join) = self.join.take() {
            let deadline = std::time::Instant::now() + Duration::from_secs(4);
            while !join.is_finished() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            if join.is_finished() {
                let _ = join.join();
            }
        }
    }
}

fn emit(sink: &EventSink, event: Event) {
    let _ = sink.tx.send(event);
    if let Some(wake) = sink.wake.as_ref() {
        wake();
    }
}

fn parse_jid(value: &str) -> Option<Jid> {
    value.parse::<Jid>().ok()
}

fn run(
    paths: Paths,
    pair_phone: Option<String>,
    cmd_rx: UnboundedReceiver<Cmd>,
    event_tx: std::sync::mpsc::Sender<Event>,
    wake: Option<Wake>,
) {
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            let sink: EventSink = Arc::new(Sink {
                tx: event_tx,
                wake,
            });
            emit(&sink, Event::Fatal(format!("runtime: {err}")));
            return;
        }
    };

    let sink: EventSink = Arc::new(Sink {
        tx: event_tx,
        wake,
    });

    rt.block_on(async move {
        let db_path = paths.session.to_string_lossy().to_string();
        let store = match SqliteStore::new(&db_path).await {
            Ok(store) => store,
            Err(err) => {
                emit(&sink, Event::Fatal(format!("archivio sessione: {err}")));
                return;
            }
        };

        let groups: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

        let qr_sink = sink.clone();
        let logout_sink = sink.clone();
        let connected_sink = sink.clone();
        let pair_sink = sink.clone();
        let pair_error_sink = sink.clone();
        let event_sink = sink.clone();
        let handler_groups = groups.clone();

        let mut builder = Bot::builder()
            .with_backend(store)
            .with_event_delivery(EventDelivery::Ordered { capacity: 1024 })
            .on_qr_code(move |code: String, _timeout: Duration| {
                let sink = qr_sink.clone();
                async move {
                    emit(&sink, Event::Qr { code });
                }
            })
            .on_connected(move |client: Arc<Client>| {
                let sink = connected_sink.clone();
                async move {
                    emit(&sink, Event::Connected);
                    emit(
                        &sink,
                        Event::Me {
                            name: client.push_name(),
                            jid: client.pn().map(|jid| jid.to_string()).unwrap_or_default(),
                        },
                    );
                }
            })
            .on_pair_code(move |code: String, _timeout: Duration| {
                let sink = pair_sink.clone();
                async move {
                    emit(&sink, Event::PairCode { code });
                }
            })
            .on_pair_code_error(move |error, _client: Arc<Client>| {
                let sink = pair_error_sink.clone();
                async move {
                    emit(&sink, Event::PairError(error.error));
                }
            })
            .on_logged_out(move |_info| {
                let sink = logout_sink.clone();
                async move {
                    emit(&sink, Event::LoggedOut);
                }
            })
            .on_event(move |event: Arc<whatsapp_rust::types::events::Event>, client: Arc<Client>| {
                let sink = event_sink.clone();
                let groups = handler_groups.clone();
                async move {
                    events::handle(event, client, sink, groups).await;
                }
            });

        if let Some(phone) = pair_phone {
            builder = builder.with_pair_code(PairCodeOptions {
                phone_number: phone,
                custom_code: None,
                ..Default::default()
            });
        }

        let bot = match builder.build().await {
            Ok(bot) => bot,
            Err(err) => {
                emit(&sink, Event::Fatal(format!("avvio sessione: {err}")));
                return;
            }
        };

        let handle = bot.spawn();
        let client = handle.client();

        let command_client = client.clone();
        let command_sink = sink.clone();
        let command_task = tokio::spawn(async move {
            command_loop(command_client, cmd_rx, command_sink).await;
        });

        // Il bot gira finché la sessione resta viva; alla disconnessione
        // intenzionale (Cmd::Quit) il futuro si risolve.
        handle.await;
        command_task.abort();
    });
}

async fn command_loop(client: Arc<Client>, mut rx: UnboundedReceiver<Cmd>, sink: EventSink) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            Cmd::Quit => {
                client.disconnect().await;
                break;
            }
            Cmd::SendText {
                chat,
                text,
                local_id,
                reply,
            } => {
                let client = client.clone();
                let sink = sink.clone();
                tokio::spawn(async move {
                    let Some(jid) = parse_jid(&chat) else {
                        emit(
                            &sink,
                            Event::Sent {
                                chat,
                                local_id,
                                server_id: None,
                                ts: 0,
                                error: Some("Chat non valida".into()),
                            },
                        );
                        return;
                    };

                    let message = build_text(&text, reply.as_ref());
                    match client.send_message(&jid, message).await {
                        Ok(result) => emit(
                            &sink,
                            Event::Sent {
                                chat,
                                local_id,
                                server_id: Some(result.message_id),
                                ts: crate::util::now_ts(),
                                error: None,
                            },
                        ),
                        Err(err) => emit(
                            &sink,
                            Event::Sent {
                                chat,
                                local_id,
                                server_id: None,
                                ts: 0,
                                error: Some(format!("{err}")),
                            },
                        ),
                    }
                });
            }
            Cmd::SendFile {
                chat,
                path,
                caption,
                local_id,
                reply,
                voice,
            } => {
                let client = client.clone();
                let sink = sink.clone();
                tokio::spawn(async move {
                    match send_file(
                        &client,
                        &chat,
                        &path,
                        &caption,
                        reply.as_ref(),
                        voice.as_ref(),
                    )
                    .await
                    {
                        Ok(server_id) => emit(
                            &sink,
                            Event::Sent {
                                chat,
                                local_id,
                                server_id: Some(server_id),
                                ts: crate::util::now_ts(),
                                error: None,
                            },
                        ),
                        Err(err) => emit(
                            &sink,
                            Event::Sent {
                                chat,
                                local_id,
                                server_id: None,
                                ts: 0,
                                error: Some(err),
                            },
                        ),
                    }
                });
            }
            Cmd::MarkRead { chat, sender, ids } => {
                let client = client.clone();
                tokio::spawn(async move {
                    let (Some(jid), false) = (parse_jid(&chat), ids.is_empty()) else {
                        return;
                    };
                    let sender = sender.as_deref().and_then(parse_jid);
                    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
                    let _ = client.mark_as_read(&jid, sender.as_ref(), &refs).await;
                });
            }
            Cmd::Played { chat, sender, ids } => {
                let client = client.clone();
                tokio::spawn(async move {
                    let (Some(jid), false) = (parse_jid(&chat), ids.is_empty()) else {
                        return;
                    };
                    let sender = sender.as_deref().and_then(parse_jid);
                    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
                    let _ = client.mark_as_played(&jid, sender.as_ref(), &refs).await;
                });
            }
            Cmd::Download {
                req,
                chat,
                id,
                payload,
                view_once,
            } => {
                let client = client.clone();
                let sink = sink.clone();
                tokio::spawn(async move {
                    let _ = view_once;
                    let result = fetch_media(&client, &payload).await;
                    emit(&sink, Event::Media { req, chat, id, result });
                });
            }
            Cmd::Typing { chat, composing } => {
                let client = client.clone();
                tokio::spawn(async move {
                    let Some(jid) = parse_jid(&chat) else { return };
                    let feature = client.chatstate();
                    if composing {
                        let _ = feature.send_composing(&jid).await;
                    } else {
                        let _ = feature.send_paused(&jid).await;
                    }
                });
            }
            Cmd::React {
                chat,
                id,
                from_me,
                participant,
                emoji,
            } => {
                let client = client.clone();
                let sink = sink.clone();
                tokio::spawn(async move {
                    let Some(jid) = parse_jid(&chat) else { return };
                    let key = wa::MessageKey {
                        remote_jid: Some(chat.clone()),
                        from_me: Some(from_me),
                        id: Some(id.clone()),
                        participant,
                    };
                    let result = client.send_reaction(&jid, key, &emoji).await;
                    match result {
                        Ok(_) => emit(
                            &sink,
                            Event::Reaction {
                                chat,
                                id,
                                emoji: if emoji.is_empty() { None } else { Some(emoji) },
                            },
                        ),
                        Err(err) => emit(&sink, Event::Fatal(format!("Reazione: {err}"))),
                    }
                });
            }
            Cmd::Edit { chat, id, text } => {
                let client = client.clone();
                let sink = sink.clone();
                tokio::spawn(async move {
                    let Some(jid) = parse_jid(&chat) else { return };
                    match client.edit_message(&jid, id.clone(), wa::Message::text(text.clone())).await
                    {
                        Ok(_) => emit(&sink, Event::Edited { chat, id, text }),
                        Err(err) => emit(&sink, Event::Fatal(format!("Modifica: {err}"))),
                    }
                });
            }
            Cmd::Revoke {
                chat,
                id,
                from_me,
                sender,
            } => {
                let client = client.clone();
                let sink = sink.clone();
                tokio::spawn(async move {
                    let Some(jid) = parse_jid(&chat) else { return };
                    let revoke = if from_me {
                        whatsapp_rust::send::RevokeType::Sender
                    } else {
                        let Some(sender_jid) = sender.as_deref().and_then(parse_jid) else {
                            return;
                        };
                        whatsapp_rust::send::RevokeType::Admin {
                            original_sender: sender_jid,
                        }
                    };
                    if client.revoke_message(&jid, id.clone(), revoke).await.is_ok() {
                        emit(&sink, Event::Revoked { chat, id });
                    }
                });
            }
            Cmd::GroupInfo { chat } => {
                let client = client.clone();
                let sink = sink.clone();
                let groups = Arc::new(Mutex::new(HashSet::new()));
                tokio::spawn(async move {
                    events::fetch_group_name(client, sink, groups, chat).await;
                });
            }
        }
    }
}

fn build_text(text: &str, reply: Option<&ReplyTarget>) -> wa::Message {
    match reply {
        None => wa::Message::text(text),
        Some(target) => {
            let mut context = wa::ContextInfo::default();
            context.stanza_id = Some(target.id.clone());
            if !target.sender.is_empty() {
                context.participant = Some(target.sender.clone());
            }
            if !target.payload.is_empty() {
                if let Ok(quoted) = codec::message_decode(&target.payload) {
                    context.quoted_message = MessageField::some(quoted);
                }
            }
            wa::Message::text_with_context(text, context)
        }
    }
}

fn reply_context(reply: Option<&ReplyTarget>) -> Option<Box<wa::ContextInfo>> {
    let target = reply?;
    let mut context = wa::ContextInfo::default();
    context.stanza_id = Some(target.id.clone());
    if !target.sender.is_empty() {
        context.participant = Some(target.sender.clone());
    }
    if !target.payload.is_empty() {
        if let Ok(quoted) = codec::message_decode(&target.payload) {
            context.quoted_message = MessageField::some(quoted);
        }
    }
    Some(Box::new(context))
}

async fn send_file(
    client: &Arc<Client>,
    chat: &str,
    path: &std::path::Path,
    caption: &str,
    reply: Option<&ReplyTarget>,
    voice: Option<&(u32, Vec<u8>)>,
) -> Result<String, String> {
    let jid = parse_jid(chat).ok_or_else(|| "Chat non valida".to_string())?;
    let data = tokio::fs::read(path)
        .await
        .map_err(|err| format!("Lettura file: {err}"))?;
    if data.is_empty() {
        return Err("File vuoto".to_string());
    }
    let mime = crate::media::mime_for_path(path);
    let caption = if caption.trim().is_empty() {
        None
    } else {
        Some(caption.trim().to_string())
    };
    let context = reply_context(reply);

    let is_voice = voice.is_some();
    let is_image = !is_voice && crate::media::is_image_path(path);
    let is_video = !is_voice && crate::media::is_video_path(path);
    let is_audio = !is_voice && crate::media::is_audio_path(path);
    let is_video = is_video
        || (!is_voice
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("gif"))
                .unwrap_or(false));

    let media_type = if is_image {
        MediaType::Image
    } else if is_video {
        MediaType::Video
    } else if is_audio || is_voice {
        MediaType::Audio
    } else {
        MediaType::Document
    };

    let upload = client
        .upload(data.clone(), media_type, UploadOptions::new())
        .await
        .map_err(|err| format!("Caricamento: {err}"))?;

    let message = if is_image {
        whatsapp_rust::media::image_message(
            upload,
            whatsapp_rust::media::ImageOptions {
                caption,
                mimetype: Some(mime.to_string()),
                jpeg_thumbnail: crate::media::make_thumbnail(&data, 64),
                context_info: context,
            },
        )
    } else if is_video {
        let info = crate::media::probe_video(path);
        whatsapp_rust::media::video_message(
            upload,
            whatsapp_rust::media::VideoOptions {
                caption,
                mimetype: Some(mime.to_string()),
                jpeg_thumbnail: None,
                duration_seconds: if info.duration > 0.0 {
                    Some(info.duration.round() as u32)
                } else {
                    None
                },
                context_info: context,
                ..Default::default()
            },
        )
    } else if is_voice {
        let (seconds, waveform) = voice.unwrap();
        whatsapp_rust::media::audio_message(
            upload,
            whatsapp_rust::media::AudioOptions {
                mimetype: Some("audio/ogg; codecs=opus".to_string()),
                duration_seconds: Some(*seconds),
                ptt: Some(true),
                waveform: if waveform.is_empty() {
                    None
                } else {
                    Some(waveform.clone())
                },
                context_info: context,
            },
        )
    } else if is_audio {
        whatsapp_rust::media::audio_message(
            upload,
            whatsapp_rust::media::AudioOptions {
                mimetype: Some(mime.to_string()),
                context_info: context,
                ..Default::default()
            },
        )
    } else {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string);
        whatsapp_rust::media::document_message(
            upload,
            whatsapp_rust::media::DocumentOptions {
                mimetype: Some(mime.to_string()),
                file_name,
                caption,
                context_info: context,
                ..Default::default()
            },
        )
    };

    client
        .send_message(&jid, message)
        .await
        .map(|result| result.message_id)
        .map_err(|err| format!("Invio: {err}"))
}

async fn fetch_media(client: &Arc<Client>, payload: &[u8]) -> Result<Vec<u8>, String> {
    let message = codec::message_decode(payload).map_err(|err| format!("Protobuf: {err}"))?;
    let base = message.get_base_message();

    let result = if let Some(media) = base.image_message.as_option() {
        client.download(media).await
    } else if let Some(media) = base.video_message.as_option() {
        client.download(media).await
    } else if let Some(media) = base.ptv_message.as_option() {
        client.download(media).await
    } else if let Some(media) = base.audio_message.as_option() {
        client.download(media).await
    } else if let Some(media) = base.document_message.as_option() {
        client.download(media).await
    } else if let Some(media) = base.sticker_message.as_option() {
        client.download(media).await
    } else {
        return Err("Questo messaggio non contiene un media scaricabile.".to_string());
    };

    result.map_err(|err| format!("Download: {err}"))
}

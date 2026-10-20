//! Ponte fra l'interfaccia e la rete.
//!
//! Un thread dedicato possiede il runtime Tokio e la sessione WhatsApp: la UI
//! gli manda comandi e riceve eventi già normalizzati, senza mai toccare
//! direttamente il socket o le chiavi.

pub mod call;
pub mod events;
pub mod extract;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use whatsapp_rust::download::MediaType;
use whatsapp_rust::http::{HttpClient, HttpRequest, UreqHttpClient};
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
    /// Scarica la foto profilo (anteprima) di un contatto o di un gruppo.
    Avatar {
        jid: String,
    },
    /// Chiamata vocale in uscita.
    CallStart {
        chat: String,
    },
    CallAccept,
    CallReject,
    CallHangup,
    CallMute {
        muted: bool,
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
    /// Nome di un contatto (3 = rubrica, 2 = push name, 1 = storico).
    Contact { jid: String, name: String, rank: u8 },
    /// Mappatura LID ↔ numero di telefono.
    Alias { lid: String, pn: String },
    /// Foto profilo: `None` = nessuna foto (o rimossa).
    Avatar { jid: String, bytes: Option<Vec<u8>> },
    Call { phase: CallPhase, peer: String, video: bool },
    CallMissed { peer: String, video: bool },
    /// Avviso da mostrare all'utente senza interrompere nulla.
    Notice(String),
}

/// Fase della chiamata in corso.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallPhase {
    /// Squilla sul nostro dispositivo.
    Ringing,
    /// Chiamata in uscita, in attesa che l'altro risponda.
    Dialing,
    /// In corso di connessione del media.
    Connecting,
    /// Audio collegato.
    Active,
    /// Finita (o rifiutata, o persa).
    Ended,
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
        let call_state = call::new_session();

        let qr_sink = sink.clone();
        let logout_sink = sink.clone();
        let connected_sink = sink.clone();
        let pair_sink = sink.clone();
        let pair_error_sink = sink.clone();
        let event_sink = sink.clone();
        let handler_groups = groups.clone();
        let handler_calls = call_state.clone();

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
                    // Privacy: niente "online" per i contatti. La libreria
                    // annuncia `available` da sola alla connessione (e dopo
                    // la sincronizzazione del push name), quindi la
                    // riportiamo a `unavailable` un paio di volte: la prima
                    // subito, le altre per coprire quei percorsi. I
                    // giustificativi di consegna restano attivi, così i
                    // messaggi risultano recapitati.
                    let client = client.clone();
                    tokio::spawn(async move {
                        client.set_force_active_delivery_receipts(true);
                        for delay in [0u64, 1200, 4000] {
                            if delay > 0 {
                                tokio::time::sleep(Duration::from_millis(delay)).await;
                            }
                            if let Err(err) = client.presence().set_unavailable().await {
                                log::debug!("presence unavailable: {err}");
                            }
                        }
                    });
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
                let calls = handler_calls.clone();
                async move {
                    events::handle(event, client, sink, groups, calls).await;
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
        let command_calls = call_state.clone();
        let command_task = tokio::spawn(async move {
            command_loop(command_client, cmd_rx, command_sink, command_calls).await;
        });

        // Il bot gira finché la sessione resta viva; alla disconnessione
        // intenzionale (Cmd::Quit) il futuro si risolve.
        handle.await;
        command_task.abort();
    });
}

async fn command_loop(
    client: Arc<Client>,
    mut rx: UnboundedReceiver<Cmd>,
    sink: EventSink,
    calls: call::SharedCall,
) {
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
                    // L'id lo scegliamo noi: l'eco del server porta lo stesso
                    // id della bolla già a schermo, quindi non si sdoppia.
                    let options = whatsapp_rust::SendOptions::default()
                        .with_message_id(local_id.clone());
                    match client
                        .send_message_with_options(&jid, message, options)
                        .await
                    {
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
                        &local_id,
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
                // Volutamente inerte: l'app non annuncia né "online" né
                // "sta scrivendo". Il comando resta per compatibilità.
                log::debug!("chatstate ignorato ({composing}): {chat}");
            }
            Cmd::Avatar { jid } => {
                let client = client.clone();
                let sink = sink.clone();
                tokio::spawn(async move {
                    let bytes = fetch_avatar(&client, &jid).await;
                    emit(&sink, Event::Avatar { jid, bytes });
                });
            }
            Cmd::CallStart { chat } => {
                let client = client.clone();
                let sink = sink.clone();
                let calls = calls.clone();
                tokio::spawn(async move {
                    call::start_outgoing(client, chat, calls, sink).await;
                });
            }
            Cmd::CallAccept => {
                let client = client.clone();
                let sink = sink.clone();
                let calls = calls.clone();
                tokio::spawn(async move {
                    call::accept(client, calls, sink).await;
                });
            }
            Cmd::CallReject => {
                let client = client.clone();
                let sink = sink.clone();
                let calls = calls.clone();
                tokio::spawn(async move {
                    call::reject(client, calls, sink).await;
                });
            }
            Cmd::CallHangup => {
                let sink = sink.clone();
                let calls = calls.clone();
                tokio::spawn(async move {
                    call::hangup(calls, sink).await;
                });
            }
            Cmd::CallMute { muted } => {
                let calls = calls.clone();
                tokio::spawn(async move {
                    call::set_muted(calls, muted).await;
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
                        Err(err) => emit(&sink, Event::Notice(format!("Reazione non inviata: {err}"))),
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
                        Err(err) => emit(&sink, Event::Notice(format!("Modifica non inviata: {err}"))),
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

/// Scarica l'anteprima della foto profilo: prima l'URL con l'IQ, poi i byte
/// dal CDN. Le foto profilo non sono cifrate, quindi basta una GET.
pub(crate) async fn fetch_avatar(client: &Arc<Client>, jid: &str) -> Option<Vec<u8>> {
    let jid: Jid = jid.parse().ok()?;
    // Le foto dei gruppi passano da un IQ diverso da quelle dei contatti.
    let url = if matches!(jid.server, whatsapp_rust::Server::Group) {
        let mut pictures = client
            .groups()
            .get_profile_pictures(
                vec![jid.clone()],
                whatsapp_rust::features::PictureType::Preview,
            )
            .await
            .ok()?;
        pictures.pop().and_then(|picture| picture.url)?
    } else {
        client
            .contacts()
            .get_profile_picture(&jid, true)
            .await
            .ok()
            .flatten()
            .map(|picture| picture.url)
            .filter(|url| !url.is_empty())?
    };
    let http = UreqHttpClient::new();
    let mut request = HttpRequest::get(url);
    request
        .headers
        .insert("User-Agent".to_string(), "WhatsApp/2.26.0".to_string());
    let response = http.execute(request).await.ok()?;
    if response.status_code == 200 && !response.body.is_empty() {
        Some(response.body)
    } else {
        None
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
    message_id: &str,
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
    let is_sticker = !is_voice && mime == "image/webp";
    let is_gif = !is_voice && !is_sticker && crate::media::is_gif_path(path);
    let is_image = !is_voice && !is_gif && !is_sticker && crate::media::is_image_path(path);
    let is_video = !is_voice && (crate::media::is_video_path(path) || is_gif);
    let is_audio = !is_voice && crate::media::is_audio_path(path);

    // Le GIF viaggiano come video H.264 con `gifPlayback`.
    let data = if is_gif {
        crate::media::gif_to_mp4(&data)?
    } else {
        data
    };

    let media_type = if is_sticker {
        MediaType::Sticker
    } else if is_image {
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

    let message = if is_sticker {
        let (width, height) = crate::media::image_dimensions(&data).unwrap_or((512, 512));
        let sticker = wa::message::StickerMessage {
            url: Some(upload.url),
            file_sha256: Some(upload.file_sha256.to_vec()),
            file_enc_sha256: Some(upload.file_enc_sha256.to_vec()),
            media_key: Some(upload.media_key.to_vec()),
            media_key_timestamp: Some(upload.media_key_timestamp),
            direct_path: Some(upload.direct_path),
            file_length: Some(upload.file_length),
            mimetype: Some("image/webp".to_string()),
            width: Some(width),
            height: Some(height),
            is_animated: Some(false),
            sticker_sent_ts: Some(crate::util::now_ts() * 1000),
            context_info: context.map(|context| MessageField::some(*context)).unwrap_or_default(),
            ..Default::default()
        };
        wa::Message {
            sticker_message: MessageField::some(sticker),
            ..Default::default()
        }
    } else if is_image {
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
        let info = if is_gif {
            crate::media::VideoInfo::default()
        } else {
            crate::media::probe_video(path)
        };
        whatsapp_rust::media::video_message(
            upload,
            whatsapp_rust::media::VideoOptions {
                caption,
                mimetype: Some(if is_gif {
                    "video/mp4".to_string()
                } else {
                    mime.to_string()
                }),
                jpeg_thumbnail: if is_gif {
                    crate::media::video_thumbnail(path)
                } else {
                    None
                },
                duration_seconds: if info.duration > 0.0 {
                    Some(info.duration.round() as u32)
                } else {
                    None
                },
                gif_playback: if is_gif { Some(true) } else { None },
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

    let options = whatsapp_rust::SendOptions::default().with_message_id(message_id.to_string());
    client
        .send_message_with_options(&jid, message, options)
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

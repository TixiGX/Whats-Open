//! Audio delle chiamate: microfono e casse in formato WhatsApp (16 kHz mono
//! i16, pacchetti da 960 campioni = 60 ms).
//!
//! Il codec (MLow) lo fa la libreria, qui c'è solo il ponte con ALSA/PipeWire
//! via cpal. Gli stream cpal non sono `Send`: vivono su thread dedicati che
//! restano parcheggiati finché il canale della chiamata è aperto, così la
//! chiusura della chiamata libera microfono e casse da sola.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Sample as _;
use whatsapp_rust::async_channel::{bounded, Receiver, Sender};

/// Frequenza dei pacchetti WhatsApp.
pub const RATE: u32 = 16_000;
/// Campioni per pacchetto (60 ms).
pub const FRAME: usize = 960;

/// Estremi audio di una chiamata, più il flag di muto.
#[derive(Clone)]
pub struct CallAudio {
    pub mic: Receiver<Vec<i16>>,
    pub speaker: Sender<Vec<i16>>,
    mute: Arc<AtomicBool>,
}

impl CallAudio {
    pub fn set_muted(&self, muted: bool) {
        self.mute.store(muted, Ordering::Relaxed);
    }

    pub fn is_muted(&self) -> bool {
        self.mute.load(Ordering::Relaxed)
    }
}

/// Apre microfono e casse. Fallisce solo se non esiste alcun dispositivo.
pub fn start() -> Result<CallAudio, String> {
    let mute = Arc::new(AtomicBool::new(false));
    let mic = start_mic(mute.clone())?;
    let speaker = start_speaker()?;
    Ok(CallAudio {
        mic,
        speaker,
        mute,
    })
}

/// Ricampionatore lineare `src → 16 kHz` usato nel callback del microfono.
struct Down {
    step: f64,
    pos: f64,
    prev: f32,
    started: bool,
}

impl Down {
    fn new(src_rate: u32) -> Self {
        Self {
            step: src_rate as f64 / RATE as f64,
            pos: 0.0,
            prev: 0.0,
            started: false,
        }
    }

    fn push(&mut self, x: f32, out: &mut Vec<i16>) {
        if !self.started {
            self.prev = x;
            self.started = true;
            self.pos = 0.0;
            out.push((x.clamp(-1.0, 1.0) * 32767.0) as i16);
            return;
        }
        while self.pos < 1.0 {
            let value = self.prev + (x - self.prev) * self.pos as f32;
            out.push((value.clamp(-1.0, 1.0) * 32767.0) as i16);
            self.pos += self.step;
        }
        self.pos -= 1.0;
        self.prev = x;
    }
}

/// Ricampionatore lineare `16 kHz → dst` usato nel callback delle casse.
struct Up {
    step: f64,
    pos: f64,
    cur: f32,
    next: f32,
    primed: bool,
}

impl Up {
    fn new(dst_rate: u32) -> Self {
        Self {
            step: RATE as f64 / dst_rate as f64,
            pos: 0.0,
            cur: 0.0,
            next: 0.0,
            primed: false,
        }
    }

    fn next(&mut self, queue: &mut VecDeque<i16>) -> f32 {
        if !self.primed {
            self.cur = queue.pop_front().map(|s| s as f32 / 32768.0).unwrap_or(0.0);
            self.next = queue
                .pop_front()
                .map(|s| s as f32 / 32768.0)
                .unwrap_or(self.cur);
            self.primed = true;
            self.pos = 0.0;
        }
        let value = self.cur + (self.next - self.cur) * self.pos as f32;
        self.pos += self.step;
        while self.pos >= 1.0 {
            self.cur = self.next;
            self.next = queue
                .pop_front()
                .map(|s| s as f32 / 32768.0)
                .unwrap_or(self.cur);
            self.pos -= 1.0;
        }
        value
    }
}

fn start_mic(mute: Arc<AtomicBool>) -> Result<Receiver<Vec<i16>>, String> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "Nessun microfono disponibile".to_string())?;
    let supported = device
        .default_input_config()
        .map_err(|err| format!("Microfono: {err}"))?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let channels = config.channels as usize;
    let rate = config.sample_rate;

    let queue = Arc::new(Mutex::new(VecDeque::<i16>::with_capacity(RATE as usize)));
    let alive = Arc::new(AtomicBool::new(true));
    let (tx, rx) = bounded::<Vec<i16>>(8);

    let stream_queue = queue.clone();
    let stream_tx = tx.clone();
    let stream_alive = alive.clone();
    std::thread::Builder::new()
        .name("whats-open-mic".into())
        .spawn(move || {
            let mut down = Down::new(rate);
            let mut out: Vec<i16> = Vec::with_capacity(2048);
            let mut push = move |mono: &[f32]| {
                out.clear();
                for &sample in mono {
                    down.push(sample, &mut out);
                }
                if out.is_empty() {
                    return;
                }
                if let Ok(mut queue) = stream_queue.lock() {
                    let cap = RATE as usize * 2;
                    if queue.len() + out.len() > cap {
                        let excess = queue.len() + out.len() - cap;
                        for _ in 0..excess {
                            queue.pop_front();
                        }
                    }
                    queue.extend(out.drain(..));
                }
            };

            macro_rules! build {
                ($t:ty) => {{
                    let err_fn = |err| log::warn!("microfono: {err}");
                    let mut mono: Vec<f32> = Vec::with_capacity(2048);
                    let mut push = push;
                    device.build_input_stream(
                        &config,
                        move |data: &[$t], _| {
                            mono.clear();
                            for frame in data.chunks(channels) {
                                let mut sum = 0i32;
                                for &sample in frame {
                                    sum += i16::from_sample(sample) as i32;
                                }
                                mono.push(sum as f32 / (channels as f32 * 32768.0));
                            }
                            push(&mono);
                        },
                        err_fn,
                        None,
                    )
                }};
            }

            let stream = match format {
                cpal::SampleFormat::F32 => build!(f32),
                cpal::SampleFormat::I16 => build!(i16),
                cpal::SampleFormat::U16 => build!(u16),
                cpal::SampleFormat::I8 => build!(i8),
                cpal::SampleFormat::U8 => build!(u8),
                cpal::SampleFormat::I32 => build!(i32),
                cpal::SampleFormat::U32 => build!(u32),
                cpal::SampleFormat::F64 => build!(f64),
                other => {
                    log::warn!("formato microfono non supportato: {other}");
                    stream_alive.store(false, Ordering::Relaxed);
                    return;
                }
            };

            let Ok(stream) = stream else {
                log::warn!("microfono: impossibile aprire lo stream");
                stream_alive.store(false, Ordering::Relaxed);
                return;
            };
            if let Err(err) = stream.play() {
                log::warn!("avvio microfono: {err}");
                stream_alive.store(false, Ordering::Relaxed);
                return;
            }
            while !stream_tx.is_closed() && stream_alive.load(Ordering::Relaxed) {
                std::thread::park_timeout(Duration::from_millis(200));
            }
            drop(stream);
        })
        .map_err(|err| format!("Thread microfono: {err}"))?;

    // Impacchetta i campioni in frame da 60 ms e li consegna al motore.
    let drain_queue = queue;
    let drain_mute = mute;
    let drain_alive = alive;
    std::thread::Builder::new()
        .name("whats-open-mic-frames".into())
        .spawn(move || {
            while !tx.is_closed() && drain_alive.load(Ordering::Relaxed) {
                let frame = {
                    let Ok(mut queue) = drain_queue.lock() else {
                        break;
                    };
                    if queue.len() >= FRAME {
                        Some(queue.drain(..FRAME).collect::<Vec<i16>>())
                    } else {
                        None
                    }
                };
                if let Some(mut frame) = frame {
                    if drain_mute.load(Ordering::Relaxed) {
                        frame.iter_mut().for_each(|sample| *sample = 0);
                    }
                    if tx.send_blocking(frame).is_err() {
                        break;
                    }
                } else {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        })
        .map_err(|err| format!("Thread frame microfono: {err}"))?;

    Ok(rx)
}

fn start_speaker() -> Result<Sender<Vec<i16>>, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "Nessun dispositivo audio in uscita".to_string())?;
    let supported = device
        .default_output_config()
        .map_err(|err| format!("Casse: {err}"))?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let channels = config.channels as usize;
    let rate = config.sample_rate;

    let queue = Arc::new(Mutex::new(VecDeque::<i16>::with_capacity(RATE as usize)));
    let (tx, rx) = bounded::<Vec<i16>>(64);

    let stream_queue = queue.clone();
    std::thread::Builder::new()
        .name("whats-open-speaker".into())
        .spawn(move || {
            macro_rules! build {
                ($t:ty) => {{
                    let err_fn = |err| log::warn!("casse: {err}");
                    let mut up = Up::new(rate);
                    let queue = stream_queue.clone();
                    device.build_output_stream(
                        &config,
                        move |data: &mut [$t], _| {
                            let Ok(mut queue) = queue.lock() else {
                                data.fill(<$t>::from_sample(0i16));
                                return;
                            };
                            for frame in data.chunks_mut(channels) {
                                let value = up.next(&mut queue);
                                let sample = <$t>::from_sample((value * 32767.0) as i16);
                                for out in frame.iter_mut() {
                                    *out = sample;
                                }
                            }
                        },
                        err_fn,
                        None,
                    )
                }};
            }

            let stream = match format {
                cpal::SampleFormat::F32 => build!(f32),
                cpal::SampleFormat::I16 => build!(i16),
                cpal::SampleFormat::U16 => build!(u16),
                cpal::SampleFormat::I8 => build!(i8),
                cpal::SampleFormat::U8 => build!(u8),
                cpal::SampleFormat::I32 => build!(i32),
                cpal::SampleFormat::U32 => build!(u32),
                cpal::SampleFormat::F64 => build!(f64),
                other => {
                    log::warn!("formato casse non supportato: {other}");
                    return;
                }
            };

            let Ok(stream) = stream else {
                return;
            };
            if let Err(err) = stream.play() {
                log::warn!("avvio casse: {err}");
                return;
            }
            while !tx.is_closed() {
                std::thread::park_timeout(Duration::from_millis(200));
            }
            drop(stream);
        })
        .map_err(|err| format!("Thread casse: {err}"))?;

    // Trasferisce l'audio decodificato nella coda letta dalle casse.
    let drain_queue = queue;
    std::thread::Builder::new()
        .name("whats-open-speaker-frames".into())
        .spawn(move || {
            while let Ok(frame) = rx.recv_blocking() {
                if let Ok(mut queue) = drain_queue.lock() {
                    if queue.len() + frame.len() > RATE as usize * 2 {
                        let drop = queue.len() + frame.len() - RATE as usize * 2;
                        for _ in 0..drop {
                            queue.pop_front();
                        }
                    }
                    queue.extend(frame);
                }
            }
        })
        .map_err(|err| format!("Thread frame casse: {err}"))?;

    Ok(tx)
}

// ─────────────────────────── stato della chiamata ───────────────────────────

use whatsapp_rust::prelude::*;
use whatsapp_rust::types::call::{CallAction, IncomingCall};
use whatsapp_rust::voip::{CallEvent, CallHandle};

use super::{emit, CallPhase, Event, EventSink};

/// Una sola chiamata alla volta, come nell'app ufficiale su un dispositivo.
#[derive(Default)]
pub struct CallSession {
    pub incoming: Option<IncomingCall>,
    pub handle: Option<Arc<CallHandle>>,
    pub audio: Option<CallAudio>,
    pub peer: String,
    pub video: bool,
    pub active: bool,
}

pub type SharedCall = Arc<tokio::sync::Mutex<CallSession>>;

pub fn new_session() -> SharedCall {
    Arc::new(tokio::sync::Mutex::new(CallSession::default()))
}

/// Evento `<call>` in arrivo: offerta (squilla) oppure fine anticipata da
/// parte di chi chiama.
pub async fn on_incoming(call: IncomingCall, state: &SharedCall, sink: &EventSink) {
    let peer = call.from.to_string();
    match &call.action {
        CallAction::Offer { is_video, .. } => {
            let video = *is_video;
            {
                let mut session = state.lock().await;
                if session.incoming.is_some() || session.handle.is_some() {
                    // Già in una chiamata: rifiuta la seconda come "occupato".
                    drop(session);
                    let _ = call;
                    return;
                }
                session.incoming = Some(call);
                session.peer = peer.clone();
                session.video = video;
                session.active = false;
            }
            emit(
                sink,
                Event::Call {
                    phase: CallPhase::Ringing,
                    peer,
                    video,
                },
            );
        }
        CallAction::Terminate { call_id, .. } => {
            let matched = {
                let mut session = state.lock().await;
                let same = session
                    .incoming
                    .as_ref()
                    .is_some_and(|incoming| incoming.action.call_id() == call_id.as_str());
                if same {
                    session.incoming = None;
                }
                same
            };
            if matched {
                emit(
                    sink,
                    Event::Call {
                        phase: CallPhase::Ended,
                        peer,
                        video: false,
                    },
                );
            }
        }
        _ => {}
    }
}

/// Risponde alla chiamata in arrivo: microfono e casse partono insieme
/// all'accettazione, il video (se offerto) resta spento.
pub async fn accept(client: Arc<Client>, state: SharedCall, sink: EventSink) {
    let incoming = { state.lock().await.incoming.clone() };
    let Some(incoming) = incoming else {
        return;
    };
    let peer = incoming.from.to_string();
    let video = matches!(&incoming.action, CallAction::Offer { is_video: true, .. });
    let audio = match start() {
        Ok(audio) => audio,
        Err(err) => {
            emit(&sink, Event::Fatal(format!("Audio chiamata: {err}")));
            let _ = client.voip().reject(&incoming).await;
            let mut session = state.lock().await;
            session.incoming = None;
            emit(
                &sink,
                Event::Call {
                    phase: CallPhase::Ended,
                    peer,
                    video,
                },
            );
            return;
        }
    };

    emit(
        &sink,
        Event::Call {
            phase: CallPhase::Connecting,
            peer: peer.clone(),
            video,
        },
    );

    let result = client
        .voip()
        .accept(&incoming)
        .audio(audio.mic.clone(), audio.speaker.clone())
        .start()
        .await;

    match result {
        Ok(handle) => {
            {
                let mut session = state.lock().await;
                session.incoming = None;
                session.audio = Some(audio);
                session.handle = Some(Arc::new(handle));
                session.peer = peer.clone();
                session.video = video;
            }
            monitor(state, sink, peer, video).await;
        }
        Err(err) => {
            log::warn!("accettazione chiamata: {err}");
            let _ = client.voip().reject(&incoming).await;
            let mut session = state.lock().await;
            session.incoming = None;
            session.audio = None;
            emit(
                &sink,
                Event::Call {
                    phase: CallPhase::Ended,
                    peer,
                    video,
                },
            );
        }
    }
}

/// Avvia una chiamata vocale verso una chat.
pub async fn start_outgoing(client: Arc<Client>, chat: String, state: SharedCall, sink: EventSink) {
    let Ok(jid) = chat.parse::<Jid>() else {
        return;
    };
    if state.lock().await.handle.is_some() {
        return;
    }
    let audio = match start() {
        Ok(audio) => audio,
        Err(err) => {
            emit(&sink, Event::Fatal(format!("Audio chiamata: {err}")));
            return;
        }
    };
    emit(
        &sink,
        Event::Call {
            phase: CallPhase::Dialing,
            peer: chat.clone(),
            video: false,
        },
    );
    let result = client
        .voip()
        .call(&jid)
        .audio(audio.mic.clone(), audio.speaker.clone())
        .start()
        .await;
    match result {
        Ok(handle) => {
            {
                let mut session = state.lock().await;
                session.audio = Some(audio);
                session.handle = Some(Arc::new(handle));
                session.peer = chat.clone();
                session.video = false;
                session.active = false;
            }
            monitor(state, sink, chat, false).await;
        }
        Err(err) => {
            log::warn!("chiamata in uscita: {err}");
            emit(
                &sink,
                Event::Call {
                    phase: CallPhase::Ended,
                    peer: chat,
                    video: false,
                },
            );
            emit(
                &sink,
                Event::Notice(format!("Chiamata non riuscita: {err}")),
            );
        }
    }
}

/// Sorveglia la chiamata attiva: segnala quando il media è collegato e
/// quando la chiamata finisce (per qualsiasi motivo).
async fn monitor(state: SharedCall, sink: EventSink, peer: String, video: bool) {
    let handle = { state.lock().await.handle.clone() };
    let Some(handle) = handle else {
        return;
    };
    let events = handle.events();
    let mut announced = false;
    loop {
        tokio::select! {
            event = events.recv() => {
                match event {
                    Ok(CallEvent::RelayAllocated) => {
                        if !announced {
                            announced = true;
                            state.lock().await.active = true;
                            emit(&sink, Event::Call { phase: CallPhase::Active, peer: peer.clone(), video });
                        }
                    }
                    Ok(_) => {}
                    Err(_) => {}
                }
            }
            _ = handle.wait_ended() => break,
        }
    }
    {
        let mut session = state.lock().await;
        session.handle = None;
        session.audio = None;
        session.active = false;
        session.peer.clear();
    }
    emit(
        &sink,
        Event::Call {
            phase: CallPhase::Ended,
            peer,
            video,
        },
    );
}

pub async fn reject(client: Arc<Client>, state: SharedCall, sink: EventSink) {
    let incoming = { state.lock().await.incoming.take() };
    let Some(incoming) = incoming else {
        return;
    };
    let peer = incoming.from.to_string();
    let video = matches!(&incoming.action, CallAction::Offer { is_video: true, .. });
    let _ = client.voip().reject(&incoming).await;
    emit(
        &sink,
        Event::Call {
            phase: CallPhase::Ended,
            peer,
            video,
        },
    );
}

pub async fn hangup(state: SharedCall, sink: EventSink) {
    let (handle, peer, video) = {
        let session = state.lock().await;
        (session.handle.clone(), session.peer.clone(), session.video)
    };
    if let Some(handle) = handle {
        handle.hangup().await;
    }
    emit(
        &sink,
        Event::Call {
            phase: CallPhase::Ended,
            peer,
            video,
        },
    );
}

pub async fn set_muted(state: SharedCall, muted: bool) {
    if let Some(audio) = state.lock().await.audio.as_ref() {
        audio.set_muted(muted);
    }
}

/// Chiamata persa (offerta arrivata mentre eravamo offline): non squilla,
/// si registra soltanto.
pub fn missed(call: &whatsapp_rust::types::call::MissedCall, sink: &EventSink) {
    emit(
        sink,
        Event::CallMissed {
            peer: call.from.to_string(),
            video: false,
        },
    );
}

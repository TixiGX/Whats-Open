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

/// Apre microfono e casse. Non fallisce mai per colpa dell'hardware: se una
/// delle due parti manca si usa un canale di silenzio, così la chiamata parte
/// comunque e l'utente viene avvertito di cosa non funzionerà.
pub fn start() -> (CallAudio, Option<String>) {
    let mute = Arc::new(AtomicBool::new(false));
    let mut missing: Vec<String> = Vec::new();

    let mic = match start_mic(mute.clone()) {
        Ok(mic) => mic,
        Err(error) => {
            log::warn!("microfono: {error}");
            missing.push(format!(
                "microfono non disponibile ({error}): l'altro non ti sentirà"
            ));
            silent_mic()
        }
    };
    let speaker = match start_speaker() {
        Ok(speaker) => speaker,
        Err(error) => {
            log::warn!("casse: {error}");
            missing.push(format!(
                "casse non disponibili ({error}): non sentirai l'altro"
            ));
            silent_speaker()
        }
    };
    let warning = if missing.is_empty() {
        None
    } else {
        Some(format!("Audio incompleto — {}", missing.join("; ")))
    };
    (
        CallAudio {
            mic,
            speaker,
            mute,
        },
        warning,
    )
}

/// Nomi dei dispositivi audio usati, per `--audio-check`.
pub fn device_names() -> (Option<String>, Option<String>) {
    // cpal 0.18 espone la descrizione, non più `name()`.
    let mic = pick_device(true).and_then(|device| device.description().ok().map(|d| d.to_string()));
    let speaker =
        pick_device(false).and_then(|device| device.description().ok().map(|d| d.to_string()));
    (mic, speaker)
}

/// Prova del percorso audio senza rete: ricampionatori, apertura dei
/// dispositivi e — se non ce ne sono — il silenzio di riserva.
pub fn self_test() -> Result<String, String> {
    let mut down = Down::new(48_000);
    let mut out: Vec<i16> = Vec::new();
    for _ in 0..4_800 {
        down.push(0.1, &mut out);
    }
    if !(1_500..=1_700).contains(&out.len()) {
        return Err(format!(
            "ricampionamento 48→16 kHz: {} campioni invece di ~1600",
            out.len()
        ));
    }
    let mut up = Up::new(48_000);
    let mut queue: VecDeque<i16> = (0..4_800).map(|index| (index % 100) as i16).collect();
    let mut values = 0usize;
    for _ in 0..12_000 {
        let value = up.next(&mut queue);
        if value.is_finite() {
            values += 1;
        }
    }
    if values != 12_000 {
        return Err("ricampionamento 16→48 kHz: valori non finiti".to_string());
    }

    let (audio, warning) = start();
    let deadline = std::time::Instant::now() + Duration::from_millis(1_500);
    let mut frames = 0;
    while std::time::Instant::now() < deadline && frames < 2 {
        if audio.mic.try_recv().is_ok() {
            frames += 1;
        } else {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let _ = audio.speaker.try_send(vec![0i16; FRAME]);
    if frames == 0 {
        return Err("nessun frame dal microfono (nemmeno di riserva)".to_string());
    }
    let (mic, speaker) = device_names();
    let mut report = format!(
        "microfono: {} · casse: {} · {frames} frame da 60 ms pronti",
        mic.unwrap_or_else(|| "silenzio di riserva".to_string()),
        speaker.unwrap_or_else(|| "silenzio di riserva".to_string()),
    );
    if let Some(warning) = warning {
        report.push_str(" · ");
        report.push_str(&warning);
    }
    Ok(report)
}

/// Sceglie un dispositivo con una configurazione utilizzabile, non solo il
/// primo della lista: in una VM il predefinito può esistere e non funzionare.
fn pick_device(input: bool) -> Option<cpal::Device> {
    let host = cpal::default_host();
    let usable = |device: &cpal::Device| {
        if input {
            device.default_input_config().is_ok()
        } else {
            device.default_output_config().is_ok()
        }
    };
    let default = if input {
        host.default_input_device()
    } else {
        host.default_output_device()
    };
    if let Some(device) = default {
        if usable(&device) {
            return Some(device);
        }
        log::warn!("dispositivo audio predefinito non utilizzabile: ne cerco un altro");
    }
    let devices = if input {
        host.input_devices().ok()?.collect::<Vec<_>>()
    } else {
        host.output_devices().ok()?.collect::<Vec<_>>()
    };
    devices.into_iter().find(|device| usable(device))
}

/// Canale di silenzio: la chiamata esiste, semplicemente non c'è microfono.
fn silent_mic() -> Receiver<Vec<i16>> {
    let (tx, rx) = bounded::<Vec<i16>>(8);
    std::thread::Builder::new()
        .name("whats-open-mic-silence".into())
        .spawn(move || {
            while !tx.is_closed() {
                if tx.send_blocking(vec![0i16; FRAME]).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(60));
            }
        })
        .ok();
    rx
}

/// Le casse mancano: si riceve, si scarta, la chiamata prosegue.
fn silent_speaker() -> Sender<Vec<i16>> {
    let (tx, rx) = bounded::<Vec<i16>>(32);
    std::thread::Builder::new()
        .name("whats-open-speaker-silence".into())
        .spawn(move || while rx.recv_blocking().is_ok() {})
        .ok();
    tx
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
    let device = pick_device(true).ok_or_else(|| "nessun microfono".to_string())?;
    let supported = device
        .default_input_config()
        .map_err(|err| format!("configurazione: {err}"))?;
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
    // Il thread dice subito se lo stream è davvero partito: così un errore
    // arriva prima dell'offerta, non come una chiamata muta.
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    std::thread::Builder::new()
        .name("whats-open-mic".into())
        .spawn(move || {
            let mut down = Down::new(rate);
            let mut out: Vec<i16> = Vec::with_capacity(2048);
            let push = move |mono: &[f32]| {
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
                        config.clone(),
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
                    let _ = ready_tx.send(Err(format!("formato {other} non supportato")));
                    stream_alive.store(false, Ordering::Relaxed);
                    return;
                }
            };

            let stream = match stream {
                Ok(stream) => stream,
                Err(err) => {
                    let _ = ready_tx.send(Err(format!("impossibile aprire: {err}")));
                    stream_alive.store(false, Ordering::Relaxed);
                    return;
                }
            };
            if let Err(err) = stream.play() {
                let _ = ready_tx.send(Err(format!("avvio: {err}")));
                stream_alive.store(false, Ordering::Relaxed);
                return;
            }
            let _ = ready_tx.send(Ok(()));
            while !stream_tx.is_closed() && stream_alive.load(Ordering::Relaxed) {
                std::thread::park_timeout(Duration::from_millis(200));
            }
            drop(stream);
        })
        .map_err(|err| format!("thread: {err}"))?;

    match ready_rx.recv_timeout(Duration::from_millis(1_500)) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return Err(error),
        Err(_) => log::warn!("microfono: apertura lenta, proseguo"),
    }

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
    let device = pick_device(false).ok_or_else(|| "nessuna uscita audio".to_string())?;
    let supported = device
        .default_output_config()
        .map_err(|err| format!("configurazione: {err}"))?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let channels = config.channels as usize;
    let rate = config.sample_rate;

    let queue = Arc::new(Mutex::new(VecDeque::<i16>::with_capacity(RATE as usize)));
    let (tx, rx) = bounded::<Vec<i16>>(64);

    let stream_queue = queue.clone();
    let alive = tx.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    std::thread::Builder::new()
        .name("whats-open-speaker".into())
        .spawn(move || {
            macro_rules! build {
                ($t:ty) => {{
                    let err_fn = |err| log::warn!("casse: {err}");
                    let mut up = Up::new(rate);
                    let queue = stream_queue.clone();
                    device.build_output_stream(
                        config.clone(),
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
                    let _ = ready_tx.send(Err(format!("formato {other} non supportato")));
                    return;
                }
            };

            let stream = match stream {
                Ok(stream) => stream,
                Err(err) => {
                    let _ = ready_tx.send(Err(format!("impossibile aprire: {err}")));
                    return;
                }
            };
            if let Err(err) = stream.play() {
                let _ = ready_tx.send(Err(format!("avvio: {err}")));
                return;
            }
            let _ = ready_tx.send(Ok(()));
            while !alive.is_closed() {
                std::thread::park_timeout(Duration::from_millis(200));
            }
            drop(stream);
        })
        .map_err(|err| format!("thread: {err}"))?;

    match ready_rx.recv_timeout(Duration::from_millis(1_500)) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return Err(error),
        Err(_) => log::warn!("casse: apertura lenta, proseguo"),
    }

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
use whatsapp_rust::schemas;
use whatsapp_rust::CallError;
use whatsapp_rust::types::call::{CallAction, IncomingCall};
use whatsapp_rust::waproto::buffa::MessageField;
use whatsapp_rust::waproto::whatsapp as wa;
use whatsapp_rust::voip::{CallEvent, CallHandle};

use super::video::{FrameSlot, VideoSession};
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
    /// Fotocamera/decodificatore della videochiamata. Fermato alla fine.
    pub media: Option<VideoSession>,
    pub camera_on: bool,
}

pub type SharedCall = Arc<tokio::sync::Mutex<CallSession>>;

pub fn new_session() -> SharedCall {
    Arc::new(tokio::sync::Mutex::new(CallSession::default()))
}

/// Evento `<call>` in arrivo: offerta (squilla) oppure fine anticipata da
/// parte di chi chiama.
pub async fn on_incoming(
    client: Arc<Client>,
    call: IncomingCall,
    state: &SharedCall,
    sink: &EventSink,
) {
    let peer = call.from.to_string();
    match &call.action {
        CallAction::Offer { is_video, .. } => {
            let video = *is_video;
            {
                let mut session = state.lock().await;
                if session.incoming.is_some() || session.handle.is_some() {
                    // Già in una chiamata: la seconda va rifiutata davvero,
                    // altrimenti il chiamante continua a sentire lo squillo.
                    drop(session);
                    let _ = client.voip().reject(&call).await;
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
/// all'accettazione. Se l'altro ha offerto il video e abbiamo ffmpeg, si
/// risponde anche in video; altrimenti la chiamata resta vocale.
pub async fn accept(
    client: Arc<Client>,
    with_video: bool,
    camera: String,
    slot: FrameSlot,
    state: SharedCall,
    sink: EventSink,
) {
    let started_ms = crate::util::timestamp_ms();
    let incoming = { state.lock().await.incoming.clone() };
    let Some(incoming) = incoming else {
        return;
    };
    let peer = incoming.from.to_string();
    let offered_video = matches!(&incoming.action, CallAction::Offer { is_video: true, .. });
    let (audio, audio_warning) = start();
    if let Some(warning) = audio_warning {
        emit(&sink, Event::Notice(warning));
    }

    let mut media = None;
    if with_video && !offered_video {
        emit(
            &sink,
            Event::Notice("L'altro ha chiamato solo in audio".into()),
        );
    } else if with_video {
        match VideoSession::start(Some(camera), slot) {
            Ok(session) => media = Some(session),
            Err(err) => emit(&sink, Event::Notice(format!("Video non disponibile: {err}"))),
        }
    }

    let video = media.is_some();
    let camera_live = media.as_ref().is_some_and(|session| session.has_camera());
    emit(
        &sink,
        Event::Call {
            phase: CallPhase::Connecting,
            peer: peer.clone(),
            video,
        },
    );

    let voip = client.voip();
    let mut builder = voip
        .accept(&incoming)
        .audio(audio.mic.clone(), audio.speaker.clone());
    if let Some(video) = media.as_ref() {
        let (source, sink_video) = video.endpoints();
        builder = builder.video(source, sink_video);
    }
    let result = builder.start().await;

    match result {
        Ok(handle) => {
            {
                let mut session = state.lock().await;
                session.incoming = None;
                session.audio = Some(audio);
                session.handle = Some(Arc::new(handle));
                session.peer = peer.clone();
                session.video = video;
                session.media = media;
                session.camera_on = camera_live;
            }
            monitor(client, state, sink, peer, video, false, started_ms).await;
        }
        Err(err) => {
            log::warn!("accettazione chiamata: {err}");
            let _ = client.voip().reject(&incoming).await;
            let mut session = state.lock().await;
            session.incoming = None;
            session.audio = None;
            session.media = None;
            emit(
                &sink,
                Event::Notice(format!("Chiamata non accettata: {}", call_error_text(&err))),
            );
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

/// Avvia una chiamata (vocale o video) verso una chat.
pub async fn start_outgoing(
    client: Arc<Client>,
    chat: String,
    video: bool,
    camera: String,
    slot: FrameSlot,
    state: SharedCall,
    sink: EventSink,
) {
    let started_ms = crate::util::timestamp_ms();
    let Ok(jid) = chat.parse::<Jid>() else {
        emit(&sink, Event::Notice("Indirizzo non valido".into()));
        return;
    };
    let stale_incoming = {
        let mut session = state.lock().await;
        if session.handle.is_some() {
            emit(&sink, Event::Notice("Sei già in una chiamata".into()));
            return;
        }
        // Un'offerta rimasta appesa (l'altro ha riattaccato senza che
        // arrivasse il terminate) non deve impedire di chiamare per sempre.
        session.incoming.take()
    };
    if let Some(stale) = stale_incoming {
        log::info!("offerta rimasta in sospeso: la chiudo prima di chiamare");
        let _ = client.voip().reject(&stale).await;
    }
    let (audio, audio_warning) = start();
    if let Some(warning) = audio_warning {
        emit(&sink, Event::Notice(warning));
    }

    let mut media = None;
    let mut video = video;
    if video {
        match VideoSession::start(Some(camera), slot) {
            Ok(session) => media = Some(session),
            Err(err) => {
                // Senza ffmpeg la videochiamata non può esistere: si parte in
                // audio e lo si dice, invece di far finta.
                video = false;
                emit(&sink, Event::Notice(format!("Video non disponibile: {err}")));
            }
        }
    }

    // Il pannello compare solo adesso: prima non c'è nessuna chiamata.
    emit(
        &sink,
        Event::Call {
            phase: CallPhase::Dialing,
            peer: chat.clone(),
            video,
        },
    );

    let voip = client.voip();
    let mut builder = voip
        .call(&jid)
        .audio(audio.mic.clone(), audio.speaker.clone());
    if let Some(session) = media.as_ref() {
        let (source, sink_video) = session.endpoints();
        builder = builder.video(source, sink_video);
    }
    let camera_live = media.as_ref().is_some_and(|session| session.has_camera());
    let result = builder.start().await;

    match result {
        Ok(handle) => {
            {
                let mut session = state.lock().await;
                session.audio = Some(audio);
                session.handle = Some(Arc::new(handle));
                session.peer = chat.clone();
                session.video = video;
                session.media = media;
                session.camera_on = camera_live;
                session.active = false;
            }
            monitor(client, state, sink, chat, video, true, started_ms).await;
        }
        Err(err) => {
            log::warn!("chiamata in uscita: {err}");
            if let Some(mut media) = media.take() {
                media.stop();
            }
            emit(
                &sink,
                Event::Call {
                    phase: CallPhase::Ended,
                    peer: chat,
                    video,
                },
            );
            emit(
                &sink,
                Event::Notice(format!("Chiamata non riuscita: {}", call_error_text(&err))),
            );
        }
    }
}

/// Sorveglia la chiamata attiva: segnala quando il media è collegato e
/// quando la chiamata finisce (per qualsiasi motivo).
async fn monitor(
    client: Arc<Client>,
    state: SharedCall,
    sink: EventSink,
    peer: String,
    video: bool,
    outgoing: bool,
    started_ms: i64,
) {
    let handle = { state.lock().await.handle.clone() };
    let Some(handle) = handle else {
        return;
    };
    let call_id = handle.call_id().to_string();
    let mut connected_at: Option<std::time::Instant> = None;
    let events = handle.events();
    let mut announced = false;
    // Squillo senza risposta: oltre questo tempo la chiamata si chiude da
    // sola, invece di restare appesa per sempre a "sto chiamando…".
    let deadline = tokio::time::sleep(Duration::from_secs(50));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            event = events.recv() => {
                match event {
                    Ok(CallEvent::RelayAllocated) => {
                        if !announced {
                            announced = true;
                            connected_at = Some(std::time::Instant::now());
                            state.lock().await.active = true;
                            emit(&sink, Event::Call { phase: CallPhase::Active, peer: peer.clone(), video });
                        }
                    }
                    Ok(CallEvent::RelayAllocateFailed(code)) => {
                        log::warn!("relay rifiutato: {code}");
                        emit(&sink, Event::Notice("La chiamata non è partita (rete)".into()));
                        break;
                    }
                    Ok(CallEvent::RelayAllocateTimedOut) | Ok(CallEvent::RelayReconnectTimedOut) => {
                        emit(&sink, Event::Notice("Chiamata interrotta: relay non raggiungibile".into()));
                        break;
                    }
                    Ok(_) => {}
                    // Canale chiuso: la chiamata è finita, inutile restare qui.
                    Err(_) => break,
                }
            }
            _ = handle.wait_ended() => break,
            _ = &mut deadline, if !announced => {
                log::info!("nessuna risposta dopo 50 s: chiudo la chiamata");
                handle.hangup().await;
                emit(
                    &sink,
                    Event::Notice(if outgoing {
                        "Nessuna risposta".into()
                    } else {
                        "Connessione non riuscita".into()
                    }),
                );
                break;
            }
        }
    }
    {
        let mut session = state.lock().await;
        if let Some(mut media) = session.media.take() {
            media.stop();
        }
        session.handle = None;
        session.audio = None;
        session.active = false;
        session.camera_on = false;
        session.video = false;
        session.peer.clear();
    }
    // La cronologia del telefono è app-state come le altre: una voce per
    // chiamata, con esito e durata. Senza questa scrittura la chiamata
    // esisterebbe solo su questo dispositivo.
    let duration = connected_at
        .map(|at| at.elapsed().as_secs())
        .unwrap_or(0);
    let (result, incoming) = if connected_at.is_some() {
        (wa::call_log_record::CallResult::Connected, !outgoing)
    } else if outgoing {
        (wa::call_log_record::CallResult::Cancelled, false)
    } else {
        (wa::call_log_record::CallResult::Missed, true)
    };
    spawn_call_log(
        client, peer.clone(), call_id, started_ms, duration, video, incoming, result,
    );

    emit(
        &sink,
        Event::Call {
            phase: CallPhase::Ended,
            peer,
            video,
        },
    );
}

/// Traduce gli errori della libreria in una frase che dice cosa fare.
/// Il testo tecnico resta nel log, dove serve a chi sviluppa.
fn call_error_text(err: &CallError) -> String {
    match err {
        CallError::NoDevices => {
            "Nessun dispositivo raggiungibile per questo contatto".to_string()
        }
        CallError::MissingDeviceIdentity => {
            "Il telefono non ha ancora autorizzato le chiamate da questo dispositivo: \
             apri WhatsApp sul telefono e riprova"
                .to_string()
        }
        CallError::ResponseTimeout => {
            "Il servizio chiamate non ha risposto in tempo: riprova tra qualche secondo".to_string()
        }
        CallError::Response(text) => {
            format!("Il servizio ha rifiutato la chiamata ({text})")
        }
        CallError::Media("no known LID for the PN callee; cannot derive media keys") => {
            "Numero non ancora sincronizzato: scrivi prima un messaggio a questo contatto, poi riprova"
                .to_string()
        }
        CallError::Media("no own LID") => {
            "La sessione non ha ancora il tuo identificativo: riavvia Whats-Open e riprova".to_string()
        }
        CallError::Media(text) => format!("Dati della chiamata incompleti ({text})"),
        CallError::Setup(text) => format!("Preparazione della chiamata non riuscita ({text})"),
        CallError::Connect(text) => format!("Connessione audio non riuscita ({text})"),
        CallError::AudioFormatNotOffered(rate) => {
            format!("L'altro non accetta l'audio a {rate} Hz: aggiorna Whats-Open")
        }
        CallError::VideoNotOffered => {
            "L'altro ha chiamato solo in audio: riaggancia e richiama in vocale".to_string()
        }
        CallError::CallEndedDuringSetup => "L'altro ha chiuso prima della risposta".to_string(),
        other => other.to_string(),
    }
}

/// Scrive una voce nel registro chiamate del telefono (`WAWebCallLogSync`).
/// È la stessa mutazione app-state che manda WhatsApp Web: così la chiamata
/// compare nella cronologia anche sul telefono.
#[allow(clippy::too_many_arguments)]
fn spawn_call_log(
    client: Arc<Client>,
    peer: String,
    call_id: String,
    started_ms: i64,
    duration_s: u64,
    video: bool,
    incoming: bool,
    result: wa::call_log_record::CallResult,
) {
    tokio::spawn(async move {
        let record = wa::CallLogRecord {
            call_result: Some(result),
            duration: Some(duration_s as i64),
            start_time: Some(started_ms),
            is_incoming: Some(incoming),
            is_video: Some(video),
            call_id: (!call_id.is_empty()).then(|| call_id.clone()),
            ..Default::default()
        };
        let value = wa::SyncActionValue {
            call_log_action: MessageField::some(wa::sync_action_value::CallLogAction {
                call_log_record: MessageField::some(record),
                ..Default::default()
            }),
            timestamp: Some(crate::util::timestamp_ms()),
            ..Default::default()
        };
        match client
            .send_app_state_action(&schemas::CALL_LOG, &[], &value)
            .await
        {
            Ok(()) => log::info!("registro chiamate: {peer} ({call_id}, {duration_s} s)"),
            Err(err) => log::warn!("registro chiamate per {peer}: {err}"),
        }
    });
}

pub async fn reject(client: Arc<Client>, state: SharedCall, sink: EventSink) {
    let incoming = { state.lock().await.incoming.take() };
    let Some(incoming) = incoming else {
        return;
    };
    let peer = incoming.from.to_string();
    let video = matches!(&incoming.action, CallAction::Offer { is_video: true, .. });
    let call_id = match &incoming.action {
        CallAction::Offer { call_id, .. } => call_id.clone(),
        _ => incoming.stanza_id.clone(),
    };
    let _ = client.voip().reject(&incoming).await;
    spawn_call_log(
        client,
        peer.clone(),
        call_id,
        crate::util::timestamp_ms(),
        0,
        video,
        true,
        wa::call_log_record::CallResult::Rejected,
    );
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

/// Accende o spegne la fotocamera mantenendo la chiamata in piedi.
pub async fn set_camera(state: SharedCall, on: bool) {
    let mut session = state.lock().await;
    if let Some(media) = session.media.as_ref() {
        media.set_camera(on && media.has_camera());
    }
    session.camera_on = on && session.media.as_ref().is_some_and(|media| media.has_camera());
}

/// Chiamata persa (offerta arrivata mentre eravamo offline): non squilla,
/// si registra soltanto.
pub fn missed(
    client: Arc<Client>,
    call: &whatsapp_rust::types::call::MissedCall,
    sink: &EventSink,
) {
    let peer = call.from.to_string();
    spawn_call_log(
        client,
        peer.clone(),
        call.call_id.clone(),
        crate::util::timestamp_ms(),
        0,
        false,
        true,
        wa::call_log_record::CallResult::Missed,
    );
    emit(
        sink,
        Event::CallMissed {
            peer,
            video: false,
        },
    );
}

//! Video delle chiamate: H.264 Annex-B dentro e fuori da ffmpeg.
//!
//! La libreria trasporta access unit già codificati (niente pixel): la
//! fotocamera viene letta e compressa da ffmpeg, il video del corrispondente
//! viene decodificato da un secondo ffmpeg in RGBA a bassa risoluzione. Il
//! fotogramma più recente resta in una casella condivisa: l'interfaccia lo
//! preleva quando cambia, senza code che crescono.
//!
//! Tutto è opzionale: senza ffmpeg o senza fotocamera la chiamata resta
//! vocale e lo si dice all'utente.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use whatsapp_rust::async_channel::{bounded, Receiver, Sender};
use whatsapp_rust::voip::VideoFrame;

/// Risoluzione del video mostrato a schermo (16:9, economica da disegnare).
pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 180;
const FRAME_BYTES: usize = WIDTH * HEIGHT * 4;

/// Fotogramma pronto per la texture.
#[derive(Clone, Debug)]
pub struct Frame {
    pub seq: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Default)]
struct SlotState {
    seq: u64,
    latest: Option<Frame>,
}

/// Ultimo fotogramma ricevuto, condiviso fra motore e interfaccia.
#[derive(Clone, Default)]
pub struct FrameSlot {
    inner: Arc<Mutex<SlotState>>,
}

impl FrameSlot {
    pub fn new() -> Self {
        Self::default()
    }

    /// Clona il fotogramma solo se è più recente di `after`.
    pub fn latest(&self, after: u64) -> Option<Frame> {
        let state = self.inner.lock().ok()?;
        if state.seq <= after {
            return None;
        }
        state.latest.clone()
    }

    pub fn clear(&self) {
        if let Ok(mut state) = self.inner.lock() {
            state.seq = 0;
            state.latest = None;
        }
    }

    fn publish(&self, rgba: Vec<u8>) {
        if let Ok(mut state) = self.inner.lock() {
            state.seq += 1;
            state.latest = Some(Frame {
                seq: state.seq,
                width: WIDTH as u32,
                height: HEIGHT as u32,
                rgba,
            });
        }
    }
}

/// Una sessione video viva: canali da passare alla libreria, fotocamera e
/// decodificatore sotto controllo.
pub struct VideoSession {
    pub slot: FrameSlot,
    outgoing: Receiver<Vec<u8>>,
    incoming: Sender<VideoFrame>,
    /// Canale sorgente tenuto aperto quando non c'è fotocamera.
    idle_source: Option<Sender<Vec<u8>>>,
    encoder: Option<Child>,
    decoder: Option<Child>,
    camera_pid: Arc<AtomicU32>,
    running: Arc<AtomicBool>,
}

impl VideoSession {
    /// Prepara i canali e avvia i processi. La fotocamera è facoltativa: se
    /// manca, la chiamata è video solo in ricezione.
    pub fn start(camera: Option<String>, slot: FrameSlot) -> Result<Self, String> {
        if !crate::media::ffmpeg_available() {
            return Err("ffmpeg non trovato: le videochiamate restano vocali".to_string());
        }
        slot.clear();
        let (frames_tx, frames_rx) = bounded::<Vec<u8>>(6);
        let (peer_tx, peer_rx) = bounded::<VideoFrame>(8);

        let mut decoder = decoder_command()
            .spawn()
            .map_err(|err| format!("ffmpeg (video in arrivo): {err}"))?;

        let stdin = decoder.stdin.take();
        let stdout = decoder.stdout.take();
        let slot_rx = slot.clone();
        thread::spawn(move || decode_loop(peer_rx.clone(), stdin, stdout, slot_rx));

        let camera_pid = Arc::new(AtomicU32::new(0));
        let running = Arc::new(AtomicBool::new(true));
        let mut encoder = None;

        let camera = camera.filter(|path| !path.trim().is_empty() && Path::new(path).exists());
        match camera {
            Some(path) => match spawn_encoder(&path) {
                Ok(mut child) => {
                    camera_pid.store(child.id(), Ordering::Relaxed);
                    if let Some(stdout) = child.stdout.take() {
                        let running = running.clone();
                        let frames_tx = frames_tx.clone();
                        thread::spawn(move || encode_loop(stdout, frames_tx, running));
                    }
                    encoder = Some(child);
                }
                Err(err) => log::warn!("fotocamera {path}: {err}"),
            },
            None => log::info!("nessuna fotocamera: videochiamata in sola ricezione"),
        }
        // Il sender resta vivo per tutta la sessione: se sparisse, il piano
        // video si chiuderebbe e la libreria smetterebbe di leggere.
        let idle_source = Some(frames_tx);

        Ok(Self {
            slot,
            outgoing: frames_rx,
            incoming: peer_tx,
            idle_source,
            encoder,
            decoder: Some(decoder),
            camera_pid,
            running,
        })
    }

    /// Estremi da passare a `.video(source, sink)`.
    pub fn endpoints(&self) -> (Receiver<Vec<u8>>, Sender<VideoFrame>) {
        (self.outgoing.clone(), self.incoming.clone())
    }

    pub fn has_camera(&self) -> bool {
        self.encoder.is_some()
    }

    /// Mette in pausa o riattiva la fotocamera senza chiudere la chiamata.
    pub fn set_camera(&self, on: bool) {
        let pid = self.camera_pid.load(Ordering::Relaxed);
        if pid == 0 {
            return;
        }
        let signal = if on { libc::SIGCONT } else { libc::SIGSTOP };
        unsafe {
            libc::kill(pid as i32, signal);
        }
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(mut child) = self.encoder.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(mut child) = self.decoder.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.idle_source = None;
        self.slot.clear();
    }
}

impl Drop for VideoSession {
    fn drop(&mut self) {
        self.stop();
    }
}

fn decoder_command() -> Command {
    let mut command = Command::new("ffmpeg");
    command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-fflags",
        "nobuffer",
        "-flags",
        "low_delay",
        "-probesize",
        "65536",
        "-analyzeduration",
        "0",
        "-f",
        "h264",
        "-i",
        "pipe:0",
        "-vf",
        "scale=320:180:force_original_aspect_ratio=decrease,pad=320:180:(ow-iw)/2:(oh-ih)/2",
        "-pix_fmt",
        "rgba",
        "-f",
        "rawvideo",
        "pipe:1",
    ]);
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::null());
    command
}

/// Prova completa senza fotocamera né rete: genera un video con ffmpeg
/// (lavfi), lo divide in access unit e lo decodifica con lo stesso percorso
/// usato dalle chiamate. Serve a `--video-check` e alla CI.
pub fn self_test() -> Result<String, String> {
    if !crate::media::ffmpeg_available() {
        return Err("ffmpeg non installato".to_string());
    }
    let mut source = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x180:rate=15",
            "-t",
            "1",
            "-an",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-tune",
            "zerolatency",
            "-profile:v",
            "baseline",
            "-g",
            "15",
            "-x264-params",
            "repeat-headers=1:aud=1",
            "-f",
            "h264",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("ffmpeg (sorgente di prova): {err}"))?;

    let mut decoder = decoder_command()
        .spawn()
        .map_err(|err| format!("ffmpeg (decodifica di prova): {err}"))?;
    let mut input = decoder.stdin.take().ok_or("decodificatore senza stdin")?;

    let source_out = source.stdout.take().ok_or("sorgente senza stdout")?;
    let reader = thread::spawn(move || {
        let mut aux = source_out;
        let mut buffer = Vec::new();
        let mut units = 0usize;
        let mut chunk = [0u8; 32 * 1024];
        while let Ok(n) = aux.read(&mut chunk) {
            if n == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..n]);
            for unit in split_access_units(&mut buffer) {
                units += 1;
                if input.write_all(&unit).is_err() {
                    return units;
                }
            }
        }
        for unit in flush_access_units(&mut buffer) {
            units += 1;
            let _ = input.write_all(&unit);
        }
        let _ = input.flush();
        drop(input);
        units
    });

    let mut frames = 0usize;
    if let Some(mut out) = decoder.stdout.take() {
        let mut buffer = vec![0u8; FRAME_BYTES];
        loop {
            match out.read_exact(&mut buffer) {
                Ok(()) => frames += 1,
                Err(_) => break,
            }
            if frames > 120 {
                break;
            }
        }
    }
    let _ = decoder.kill();
    let _ = decoder.wait();
    let _ = source.wait();
    let units = reader.join().unwrap_or(0);
    if units == 0 {
        return Err("nessuna access unit prodotta".to_string());
    }
    if frames == 0 {
        return Err(format!("{units} access unit, ma nessun fotogramma decodificato"));
    }
    Ok(format!(
        "{units} access unit H.264, {frames} fotogrammi decodificati a {}×{}",
        WIDTH, HEIGHT
    ))
}

fn spawn_encoder(camera: &str) -> Result<Child, String> {
    Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "v4l2",
            "-framerate",
            "15",
            "-video_size",
            "640x480",
            "-i",
            camera,
            "-an",
            "-vf",
            "scale=640:360:force_original_aspect_ratio=decrease,pad=640:360:(ow-iw)/2:(oh-ih)/2,format=yuv420p",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-tune",
            "zerolatency",
            "-profile:v",
            "baseline",
            "-level",
            "3.1",
            "-g",
            "15",
            "-keyint_min",
            "15",
            "-sc_threshold",
            "0",
            "-b:v",
            "700k",
            "-maxrate",
            "800k",
            "-bufsize",
            "1400k",
            "-x264-params",
            "repeat-headers=1:aud=1",
            "-fflags",
            "nobuffer",
            "-flush_packets",
            "1",
            "-f",
            "h264",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("avvio ffmpeg: {err}"))
}

fn encode_loop(mut stdout: ChildStdout, out: Sender<Vec<u8>>, running: Arc<AtomicBool>) {
    let mut buffer = Vec::with_capacity(64 * 1024);
    let mut chunk = [0u8; 16 * 1024];
    while running.load(Ordering::Relaxed) {
        match stdout.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buffer.extend_from_slice(&chunk[..n]);
                for unit in split_access_units(&mut buffer) {
                    // Se la rete è più lenta della fotocamera si salta il
                    // fotogramma: meglio fluido che in ritardo.
                    let _ = out.try_send(unit);
                }
            }
            Err(err) => {
                log::debug!("lettura fotocamera: {err}");
                break;
            }
        }
    }
    for unit in flush_access_units(&mut buffer) {
        let _ = out.try_send(unit);
    }
}

fn decode_loop(
    incoming: Receiver<VideoFrame>,
    stdin: Option<std::process::ChildStdin>,
    stdout: Option<ChildStdout>,
    slot: FrameSlot,
) {
    let Some(mut stdin) = stdin else {
        return;
    };
    let Some(mut stdout) = stdout else {
        return;
    };

    let reader = thread::spawn(move || {
        let mut buffer = Vec::with_capacity(FRAME_BYTES);
        let mut chunk = [0u8; 32 * 1024];
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    buffer.extend_from_slice(&chunk[..n]);
                    while buffer.len() >= FRAME_BYTES {
                        let frame: Vec<u8> = buffer.drain(..FRAME_BYTES).collect();
                        slot.publish(frame);
                    }
                }
                Err(_) => break,
            }
        }
    });

    while let Ok(frame) = incoming.recv_blocking() {
        if stdin.write_all(&frame.data).is_err() {
            break;
        }
        let _ = stdin.flush();
    }
    drop(stdin);
    let _ = reader.join();
}

/// Divide un flusso H.264 Annex-B in access unit complete.
///
/// ffmpeg è avviato con `aud=1`, quindi ogni access unit comincia con un NAL
/// di tipo 9 (AUD). Un'unità è completa solo quando arriva l'AUD successivo:
/// così un NAL spezzato fra due letture non viene mai inviato a metà.
pub fn split_access_units(buffer: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let starts = start_codes(buffer);
    if starts.len() < 2 {
        return Vec::new();
    }

    let mut units = Vec::new();
    let mut current: Option<usize> = None;
    for index in 0..starts.len() - 1 {
        let (begin, header) = starts[index];
        let kind = buffer.get(header).copied().unwrap_or(0) & 0x1f;
        if kind == 9 {
            if let Some(from) = current {
                if index > from {
                    units.push(buffer[starts[from].0..begin].to_vec());
                }
            }
            current = Some(index);
        } else if current.is_none() {
            current = Some(index);
        }
    }

    match current {
        Some(from) => {
            let keep = starts[from].0;
            buffer.drain(..keep);
        }
        None => {
            let keep = starts[0].0;
            buffer.drain(..keep);
        }
    }
    units
}

/// Scarta il buffer rimasto alla fine del flusso (ultima unità completa).
pub fn flush_access_units(buffer: &mut Vec<u8>) -> Vec<Vec<u8>> {
    if buffer.is_empty() {
        return Vec::new();
    }
    let units = vec![std::mem::take(buffer)];
    units.into_iter().filter(|unit| !unit.is_empty()).collect()
}

fn start_codes(buffer: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut index = 0;
    while index + 3 < buffer.len() {
        if buffer[index] == 0 && buffer[index + 1] == 0 && buffer[index + 2] == 1 {
            out.push((index, index + 3));
            index += 3;
        } else if buffer[index] == 0
            && buffer[index + 1] == 0
            && buffer[index + 2] == 0
            && buffer[index + 3] == 1
        {
            out.push((index, index + 4));
            index += 4;
        } else {
            index += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nal(kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0, 0, 0, 1, kind];
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn le_access_unit_si_chiudono_all_aud_successivo() {
        let first = [nal(9, &[]), nal(7, &[1, 2]), nal(5, &[3, 4, 5])].concat();
        let second = [nal(9, &[]), nal(1, &[6, 7])].concat();
        let mut buffer = [first.clone(), second.clone()].concat();

        let units = split_access_units(&mut buffer);
        assert_eq!(units.len(), 1);
        assert_eq!(units[0], first);
        // Il secondo AUD resta in attesa del suo successore.
        assert_eq!(buffer, second);
    }

    #[test]
    fn un_nal_spezzato_non_viene_inviato_a_meta() {
        let first = [nal(9, &[]), nal(5, &[1, 2, 3])].concat();
        let second = [nal(9, &[]), nal(1, &[4, 5])].concat();
        let all = [first.clone(), second.clone()].concat();

        let mut buffer = Vec::new();
        let mut units = Vec::new();
        for chunk in all.chunks(3) {
            buffer.extend_from_slice(chunk);
            units.extend(split_access_units(&mut buffer));
        }
        assert_eq!(units, vec![first.clone()]);
        // Il resto si chiude solo alla fine del flusso.
        units.extend(split_access_units(&mut buffer));
        assert!(units.iter().any(|unit| unit == &first));
        assert!(buffer.starts_with(&second[..4]));
    }

    #[test]
    fn il_flusso_finale_viene_consegnato() {
        let unit = [nal(9, &[]), nal(5, &[1])].concat();
        let mut buffer = unit.clone();
        assert!(split_access_units(&mut buffer).is_empty());
        let rest = flush_access_units(&mut buffer);
        assert_eq!(rest, vec![unit]);
        assert!(buffer.is_empty());
    }
}

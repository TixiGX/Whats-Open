//! Registrazione di messaggi vocali: cpal cattura il microfono, ffmpeg
//! codifica in Ogg/Opus (il formato che WhatsApp usa per le note vocali).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::FromSample;

pub struct Recorder {
    path: PathBuf,
    stream: cpal::Stream,
    tx: Sender<Vec<f32>>,
    stop: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
    started: Instant,
    rate: u32,
    channels: u16,
    waveform: Arc<Mutex<Vec<u8>>>,
}

impl Recorder {
    pub fn start(path: &Path) -> Result<Self, String> {
        if !crate::media::ffmpeg_available() {
            return Err("ffmpeg non è installato".to_string());
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "Nessun microfono disponibile".to_string())?;
        let supported = device
            .default_input_config()
            .map_err(|err| format!("Microfono: {err}"))?;

        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let rate = config.sample_rate;
        let channels = config.channels;

        let mut child = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-nostdin",
                "-f",
                "f32le",
                "-ar",
                &rate.to_string(),
                "-ac",
                &channels.to_string(),
                "-i",
                "-",
                "-c:a",
                "libopus",
                "-b:a",
                "32k",
                "-ar",
                "48000",
                "-ac",
                "1",
                "-f",
                "ogg",
                "-y",
            ])
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| format!("ffmpeg: {err}"))?;

        let mut stdin = child.stdin.take().ok_or_else(|| "stdin ffmpeg".to_string())?;
        let (tx, rx) = sync_channel::<Vec<f32>>(16);
        std::thread::Builder::new()
            .name("whats-open-record-writer".into())
            .spawn(move || {
                while let Ok(samples) = rx.recv() {
                    let mut bytes = Vec::with_capacity(samples.len() * 4);
                    for sample in samples.iter() {
                        bytes.extend_from_slice(&sample.to_le_bytes());
                    }
                    if stdin.write_all(&bytes).is_err() {
                        break;
                    }
                }
                let _ = stdin.flush();
                drop(stdin);
            })
            .ok();

        let waveform = Arc::new(Mutex::new(Vec::with_capacity(64)));
        let child = Arc::new(Mutex::new(Some(child)));
        let stop = Arc::new(AtomicBool::new(false));
        let stream = build_input(
            &device,
            &config,
            sample_format,
            tx.clone(),
            waveform.clone(),
        )?;
        stream
            .play()
            .map_err(|err| format!("Avvio microfono: {err}"))?;

        Ok(Self {
            path: path.to_path_buf(),
            stream,
            tx,
            stop,
            child,
            started: Instant::now(),
            rate,
            channels,
            waveform,
        })
    }

    pub fn seconds(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    pub fn level(&self) -> f32 {
        self.waveform
            .lock()
            .ok()
            .and_then(|peaks| peaks.last().copied())
            .map(|peak| peak as f32 / 100.0)
            .unwrap_or(0.0)
    }

    pub fn peek_waveform(&self) -> Vec<u8> {
        self.waveform
            .lock()
            .map(|peaks| peaks.clone())
            .unwrap_or_default()
    }

    pub fn cancel(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.stream.pause();
        drop(self.tx);
        if let Ok(mut guard) = self.child.lock() {
            if let Some(child) = guard.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        let _ = std::fs::remove_file(&self.path);
    }

    pub fn finish(self) -> Result<(f64, Vec<u8>, PathBuf), String> {
        let seconds = self.seconds();
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.stream.pause();
        drop(self.stream);
        drop(self.tx);

        let child = self.child.lock().ok().and_then(|mut guard| guard.take());
        let status = child
            .map(|mut child| child.wait())
            .transpose()
            .map_err(|err| format!("ffmpeg: {err}"))?;

        if let Some(status) = status {
            if !status.success() {
                return Err("Codifica audio fallita".to_string());
            }
        }
        if !self.path.exists() {
            return Err("File audio non creato".to_string());
        }

        let peaks = self
            .waveform
            .lock()
            .map(|peaks| peaks.clone())
            .unwrap_or_default();
        let _ = self.rate;
        let _ = self.channels;
        Ok((seconds, peaks, self.path.clone()))
    }
}

fn build_input(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    format: cpal::SampleFormat,
    tx: Sender<Vec<f32>>,
    waveform: Arc<Mutex<Vec<u8>>>,
) -> Result<cpal::Stream, String> {
    let channels = config.channels as usize;
    let error_callback = |err: cpal::Error| log::warn!("microfono: {err}");

    macro_rules! stream {
        ($ty:ty) => {{
            let tx = tx.clone();
            let waveform = waveform.clone();
            device
                .build_input_stream(
                    config.clone(),
                    move |data: &[$ty], _: &cpal::InputCallbackInfo| {
                        let mut out = Vec::with_capacity(data.len());
                        let mut peak = 0.0f32;
                        for frame in data.chunks(channels.max(1)) {
                            let mut mono = 0.0f32;
                            for sample in frame.iter() {
                                mono += f32::from_sample(*sample);
                            }
                            mono /= channels.max(1) as f32;
                            peak = peak.max(mono.abs());
                            out.push(mono);
                        }
                        if let Ok(mut peaks) = waveform.lock() {
                            if peaks.len() < 128 {
                                peaks.push((peak * 100.0).min(100.0) as u8);
                            }
                        }
                        let _ = tx.try_send(out);
                    },
                    error_callback,
                    None,
                )
                .map_err(|err| format!("Microfono: {err}"))
        }};
    }

    match format {
        cpal::SampleFormat::F32 => stream!(f32),
        cpal::SampleFormat::F64 => stream!(f64),
        cpal::SampleFormat::I8 => stream!(i8),
        cpal::SampleFormat::I16 => stream!(i16),
        cpal::SampleFormat::I32 => stream!(i32),
        cpal::SampleFormat::U8 => stream!(u8),
        cpal::SampleFormat::U16 => stream!(u16),
        other => Err(format!("Formato microfono non supportato: {other}")),
    }
}

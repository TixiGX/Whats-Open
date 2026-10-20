//! Lettore audio: ffmpeg decodifica in PCM, cpal lo manda alle casse.

use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Sample as _;

const QUEUE_CAP: usize = 96_000; // campioni interleaved (~1 s stereo a 48 kHz)

struct Shared {
    queue: Mutex<VecDeque<f32>>,
    paused: AtomicBool,
    stop: AtomicBool,
    finished: AtomicBool,
    consumed_frames: AtomicU64,
    volume_bits: AtomicU64,
}

impl Shared {
    fn volume(&self) -> f32 {
        f32::from_bits(self.volume_bits.load(Ordering::Relaxed) as u32)
    }
}

pub struct AudioPlayer {
    path: PathBuf,
    shared: Arc<Shared>,
    stream: cpal::Stream,
    child: Option<Child>,
    rate: u32,
    duration: f64,
    start_offset: f64,
    playing: bool,
}

impl AudioPlayer {
    pub fn open(path: &Path) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "Nessun dispositivo audio disponibile".to_string())?;
        let supported = device
            .default_output_config()
            .map_err(|err| format!("Configurazione audio: {err}"))?;

        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let rate = config.sample_rate;
        let channels = config.channels as usize;
        let stream_config = cpal::StreamConfig {
            channels: config.channels,
            sample_rate: config.sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        let shared = Arc::new(Shared {
            queue: Mutex::new(VecDeque::new()),
            paused: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            consumed_frames: AtomicU64::new(0),
            volume_bits: AtomicU64::new(1.0f32.to_bits() as u64),
        });

        let stream = match sample_format {
            cpal::SampleFormat::F32 => build_stream::<f32>(&device, &stream_config, shared.clone(), channels),
            cpal::SampleFormat::I16 => build_stream::<i16>(&device, &stream_config, shared.clone(), channels),
            cpal::SampleFormat::U16 => build_stream::<u16>(&device, &stream_config, shared.clone(), channels),
            other => Err(format!("Formato audio non supportato: {other}")),
        }?;

        stream
            .play()
            .map_err(|err| format!("Avvio uscita audio: {err}"))?;

        let duration = crate::media::probe_video(path).duration;
        let mut player = Self {
            path: path.to_path_buf(),
            shared,
            stream,
            child: None,
            rate,
            duration,
            start_offset: 0.0,
            playing: true,
        };
        player.spawn_decoder(0.0);
        Ok(player)
    }

    fn spawn_decoder(&mut self, start: f64) {
        self.stop_decoder();
        {
            if let Ok(mut queue) = self.shared.queue.lock() {
                queue.clear();
            }
        }
        self.shared.stop.store(false, Ordering::Relaxed);
        self.shared.finished.store(false, Ordering::Relaxed);
        self.shared.consumed_frames.store(0, Ordering::Relaxed);
        self.shared.paused.store(!self.playing, Ordering::Relaxed);
        self.start_offset = start;

        let result = Command::new("ffmpeg")
            .args(["-v", "error", "-nostdin", "-ss", &format!("{start:.3}"), "-i"])
            .arg(&self.path)
            .args([
                "-vn",
                "-f",
                "f32le",
                "-ac",
                "2",
                "-ar",
                &self.rate.to_string(),
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();

        let Ok(mut child) = result else {
            log::warn!("ffmpeg non disponibile per l'audio");
            return;
        };
        let Some(mut stdout) = child.stdout.take() else {
            let _ = child.kill();
            return;
        };

        let shared = self.shared.clone();
        std::thread::Builder::new()
            .name("whats-open-audio".into())
            .spawn(move || {
                let mut bytes = [0u8; 16_384];
                loop {
                    if shared.stop.load(Ordering::Relaxed) {
                        break;
                    }
                    match stdout.read(&mut bytes) {
                        Ok(0) => break,
                        Ok(count) => {
                            let usable = count - (count % 4);
                            if usable == 0 {
                                continue;
                            }
                            let samples: Vec<f32> = bytes[..usable]
                                .chunks_exact(4)
                                .map(|chunk| {
                                    f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
                                })
                                .collect();
                            let mut written = 0usize;
                            while written < samples.len() {
                                if shared.stop.load(Ordering::Relaxed) {
                                    return;
                                }
                                let space = {
                                    match shared.queue.lock() {
                                        Ok(mut queue) => {
                                            let space = QUEUE_CAP.saturating_sub(queue.len());
                                            let take = space.min(samples.len() - written);
                                            queue.extend(samples[written..written + take].iter().copied());
                                            written += take;
                                            space
                                        }
                                        Err(_) => return,
                                    }
                                };
                                if space == 0 {
                                    std::thread::sleep(Duration::from_millis(15));
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
                shared.finished.store(true, Ordering::Relaxed);
            })
            .ok();

        self.child = Some(child);
    }

    fn stop_decoder(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn toggle(&mut self) {
        self.playing = !self.playing;
        if self.playing {
            self.shared.paused.store(false, Ordering::Relaxed);
            if self.at_end() {
                self.seek(0.0);
            }
        } else {
            self.shared.paused.store(true, Ordering::Relaxed);
        }
    }

    pub fn pause(&mut self) {
        if self.playing {
            self.playing = false;
            self.shared.paused.store(true, Ordering::Relaxed);
        }
    }

    pub fn play(&mut self) {
        if !self.playing {
            self.playing = true;
            self.shared.paused.store(false, Ordering::Relaxed);
            if self.at_end() {
                self.seek(0.0);
            }
        }
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn at_end(&self) -> bool {
        self.duration > 0.0 && self.position() >= self.duration - 0.15
    }

    pub fn position(&self) -> f64 {
        let frames = self.shared.consumed_frames.load(Ordering::Relaxed) as f64;
        self.start_offset + frames / (self.rate.max(1) as f64)
    }

    pub fn duration(&self) -> f64 {
        self.duration
    }

    pub fn finished(&self) -> bool {
        self.shared.finished.load(Ordering::Relaxed)
            && self
                .shared
                .queue
                .lock()
                .map(|queue| queue.is_empty())
                .unwrap_or(true)
    }

    pub fn seek(&mut self, seconds: f64) {
        let target = seconds.clamp(0.0, self.duration.max(0.0));
        self.spawn_decoder(target);
        if self.playing {
            self.shared.paused.store(false, Ordering::Relaxed);
        }
    }

    pub fn set_volume(&self, volume: f32) {
        self.shared
            .volume_bits
            .store(volume.clamp(0.0, 1.0).to_bits() as u64, Ordering::Relaxed);
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        self.stop_decoder();
        let _ = self.stream.pause();
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    shared: Arc<Shared>,
    channels: usize,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = channels.max(1);
    let shared_cb = shared.clone();
    let error_callback = |err: cpal::Error| log::warn!("audio output: {err}");

    device
        .build_output_stream(
            config.clone(),
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let ok = fill(data, channels, &shared_cb);
                if !ok {
                    for sample in data.iter_mut() {
                        *sample = T::from_sample(0.0f32);
                    }
                }
            },
            error_callback,
            None,
        )
        .map_err(|err| format!("Uscita audio: {err}"))
}

fn fill<T>(data: &mut [T], channels: usize, shared: &Arc<Shared>) -> bool
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let Ok(mut queue) = shared.queue.try_lock() else {
        return false;
    };
    let paused = shared.paused.load(Ordering::Relaxed);
    let volume = shared.volume();

    for frame in data.chunks_mut(channels) {
        let (left, right) = if paused || queue.len() < 2 {
            (0.0f32, 0.0f32)
        } else {
            let left = queue.pop_front().unwrap_or(0.0);
            let right = queue.pop_front().unwrap_or(0.0);
            shared.consumed_frames.fetch_add(1, Ordering::Relaxed);
            (left, right)
        };
        for (index, sample) in frame.iter_mut().enumerate() {
            let value = if channels == 1 {
                (left + right) * 0.5
            } else if index % 2 == 0 {
                left
            } else {
                right
            };
            *sample = T::from_sample(value * volume);
        }
    }
    true
}

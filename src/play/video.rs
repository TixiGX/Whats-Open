//! Lettore video: ffmpeg produce fotogrammi RGB24, egui li mostra; la traccia
//! audio passa dallo stesso decodificatore e dal lettore cpal.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

use super::audio::AudioPlayer;

pub struct VideoPlayer {
    path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub duration: f64,
    pub fps: f64,
    pub playing: bool,
    pub start_offset: f64,
    pub frames_shown: u64,
    rx: Option<Receiver<Vec<u8>>>,
    stop: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
    pub audio: Option<AudioPlayer>,
    pub frame: Option<Vec<u8>>,
    /// GIF: riparte da sola a fine clip.
    pub looping: bool,
}

impl VideoPlayer {
    pub fn open(path: &Path) -> Result<Self, String> {
        if !crate::media::ffmpeg_available() {
            return Err("ffmpeg non è installato".to_string());
        }

        let info = crate::media::probe_video(path);
        let (width, height) = fit(info.width, info.height, 960);

        let stop = Arc::new(AtomicBool::new(false));
        let (rx, child) = spawn_reader(path, 0.0, width, height, stop.clone());
        let audio = AudioPlayer::open(path).ok();

        let mut player = Self {
            path: path.to_path_buf(),
            width,
            height,
            duration: info.duration,
            fps: 30.0,
            playing: true,
            start_offset: 0.0,
            frames_shown: 0,
            rx: Some(rx),
            stop,
            child,
            audio,
            frame: None,
            looping: false,
        };
        player.poll_frame();
        Ok(player)
    }

    fn kill_reader(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut guard) = self.child.lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    fn restart(&mut self, start: f64) {
        self.kill_reader();
        self.stop = Arc::new(AtomicBool::new(false));
        let (rx, child) = spawn_reader(&self.path, start, self.width, self.height, self.stop.clone());
        self.rx = Some(rx);
        self.child = child;
        self.start_offset = start;
        self.frames_shown = 0;
        self.frame = None;
        if let Some(audio) = self.audio.as_mut() {
            audio.seek(start);
            if self.playing {
                audio.play();
            } else {
                audio.pause();
            }
        }
    }

    pub fn poll_frame(&mut self) -> Option<&Vec<u8>> {
        let mut got = false;
        if let Some(rx) = self.rx.as_ref() {
            loop {
                match rx.try_recv() {
                    Ok(frame) => {
                        self.frame = Some(frame);
                        self.frames_shown += 1;
                        got = true;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
        }
        if got {
            self.frame.as_ref()
        } else {
            None
        }
    }

    pub fn position(&self) -> f64 {
        let base = self.start_offset + self.frames_shown as f64 / self.fps.max(1.0);
        if self.duration > 0.0 {
            base.min(self.duration)
        } else {
            base
        }
    }

    pub fn toggle(&mut self) {
        if self.playing {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn pause(&mut self) {
        if !self.playing {
            return;
        }
        self.playing = false;
        let position = self.position();
        self.kill_reader();
        if let Some(audio) = self.audio.as_mut() {
            audio.pause();
        }
        self.restart(position);
        self.playing = false;
        if let Some(audio) = self.audio.as_mut() {
            audio.pause();
        }
    }

    pub fn play(&mut self) {
        if self.playing {
            return;
        }
        self.playing = true;
        let position = self.position();
        self.restart(position);
        if let Some(audio) = self.audio.as_mut() {
            audio.play();
        }
    }

    pub fn seek(&mut self, seconds: f64) {
        let target = seconds.clamp(0.0, self.duration.max(0.0));
        self.restart(target);
    }

    pub fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }

    pub fn at_end(&self) -> bool {
        if self.duration <= 0.0 {
            return false;
        }
        self.position() >= self.duration - 0.2
    }
}

impl Drop for VideoPlayer {
    fn drop(&mut self) {
        self.kill_reader();
    }
}

fn fit(width: u32, height: u32, max_width: u32) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (640, 360);
    }
    let mut out_w = width.min(max_width);
    let mut out_h = ((height as f64) * (out_w as f64) / (width as f64)).round() as u32;
    // Dimensioni pari: alcuni decodificatori RGB lo preferiscono.
    out_w -= out_w % 2;
    out_h -= out_h % 2;
    (out_w.max(2), out_h.max(2))
}

fn spawn_reader(
    path: &Path,
    start: f64,
    width: u32,
    height: u32,
    stop: Arc<AtomicBool>,
) -> (Receiver<Vec<u8>>, Arc<Mutex<Option<Child>>>) {
    let scale = format!("scale={width}:{height}");
    let mut command = Command::new("ffmpeg");
    command
        .args(["-v", "error", "-nostdin", "-ss", &format!("{start:.3}"), "-i"])
        .arg(path)
        .args([
            "-an",
            "-vf",
            &scale,
            "-r",
            "30",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null());

    let child = command.spawn().ok();
    let guard = Arc::new(Mutex::new(child));
    let (tx, rx) = sync_channel::<Vec<u8>>(2);

    // Prendi lo stdout lasciando il figlio dentro il suo lucchetto: così
    // `guard` resta libero di essere restituito.
    let taken = guard
        .lock()
        .ok()
        .and_then(|mut lock| lock.as_mut().and_then(|child| child.stdout.take()));
    let mut stdout = match taken {
        Some(stdout) => stdout,
        None => return (rx, guard),
    };

    let guard_thread = guard.clone();
    let frame_len = (width as usize) * (height as usize) * 3;
    std::thread::Builder::new()
        .name("whats-open-video".into())
        .spawn(move || {
            let mut buffer = vec![0u8; frame_len];
            loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                if stdout.read_exact(&mut buffer).is_err() {
                    break;
                }
                if tx.send(buffer.clone()).is_err() {
                    break;
                }
            }
            if let Ok(mut lock) = guard_thread.lock() {
                if let Some(mut child) = lock.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        })
        .ok();

    (rx, guard)
}

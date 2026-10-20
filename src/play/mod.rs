//! Riproduzione audio e video.
//!
//! ffmpeg decodifica (Opus, AAC, H.264, …), cpal scrive sul dispositivo di
//! uscita. Nessun codec proprietario compilato dentro l'app.

pub mod audio;
pub mod record;
pub mod video;

//! Decodifica immagini, miniature e rilevamento di ffmpeg/ffprobe.
//!
//! La riproduzione di audio e video usa ffmpeg come motore di decodifica: è
//! già presente su quasi tutte le distribuzioni Linux e decodifica tutto ciò
//! che WhatsApp spedisce (Opus, AAC, H.264, ...). Se manca, l'interfaccia lo
//! dice e offre di aprire il file con il player di sistema.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use image::{ImageFormat, GenericImageView};

/// Immagine decodificata pronta per una texture egui.
pub struct DecodedImage {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

pub fn decode_image(bytes: &[u8]) -> Option<DecodedImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let (width, height) = img.dimensions();
    let rgba = img.to_rgba8();
    Some(DecodedImage {
        width: width as usize,
        height: height as usize,
        rgba: rgba.into_raw(),
    })
}

/// Miniatura JPEG (per la cache su disco e per i messaggi senza thumbnail).
pub fn make_thumbnail(bytes: &[u8], max: u32) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    let thumb = img.thumbnail(max, max);
    let mut out: Vec<u8> = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut out);
        thumb.write_to(&mut cursor, ImageFormat::Jpeg).ok()?;
    }
    Some(out)
}

pub fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    image::load_from_memory(bytes).ok().map(|img| img.dimensions())
}

fn command_exists(name: &str) -> bool {
    Command::new(name)
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub fn ffmpeg_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| command_exists("ffmpeg"))
}

pub fn ffprobe_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| command_exists("ffprobe"))
}

#[derive(Clone, Debug, Default)]
pub struct VideoInfo {
    pub duration: f64,
    pub width: u32,
    pub height: u32,
}

pub fn probe_video(path: &Path) -> VideoInfo {
    let mut info = VideoInfo::default();

    if let Ok(output) = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
    {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Ok(value) = text.trim().parse::<f64>() {
                info.duration = value;
            }
        }
    }

    if let Ok(output) = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=p=0:s=x",
        ])
        .arg(path)
        .output()
    {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            let mut parts = text.trim().split('x');
            let w = parts.next().and_then(|v| v.parse::<u32>().ok());
            let h = parts.next().and_then(|v| v.parse::<u32>().ok());
            if let (Some(w), Some(h)) = (w, h) {
                info.width = w;
                info.height = h;
            }
        }
    }

    info
}

/// Primo fotogramma di un video, in JPEG, per l'anteprima nella bolla.
pub fn video_thumbnail(path: &Path) -> Option<Vec<u8>> {
    if !ffmpeg_available() {
        return None;
    }
    let output = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-nostdin",
            "-ss",
            "0",
            "-i",
        ])
        .arg(path)
        .args([
            "-frames:v",
            "1",
            "-vf",
            "scale=320:-2",
            "-f",
            "mjpeg",
            "-",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if output.status.success() && !output.stdout.is_empty() {
        Some(output.stdout)
    } else {
        None
    }
}

pub fn ext_for_mime(mime: &str, fallback: &str) -> String {
    let mime = mime.split(';').next().unwrap_or(mime).trim().to_ascii_lowercase();
    let ext = match mime.as_str() {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "video/mp4" => "mp4",
        "video/3gpp" => "3gp",
        "video/webm" => "webm",
        "audio/ogg" | "audio/ogg; codecs=opus" => "ogg",
        "audio/mpeg" => "mp3",
        "audio/mp4" | "audio/aac" => "m4a",
        "audio/wav" | "audio/x-wav" => "wav",
        "application/pdf" => "pdf",
        _ => fallback,
    };
    ext.to_string()
}

/// Nome file sicuro per la cache, partendo dal nome originale.
pub fn safe_file_name(name: &str, fallback: &str) -> String {
    let base = name.rsplit('/').next().unwrap_or(name);
    let cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ' | '(' | ')') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned.to_string()
    }
}

pub fn guess_extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_else(|| "bin".to_string())
}

pub fn mime_for_path(path: &Path) -> &'static str {
    match guess_extension(path).as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "3gp" => "video/3gpp",
        "webm" => "video/webm",
        "ogg" | "opus" => "audio/ogg; codecs=opus",
        "mp3" => "audio/mpeg",
        "m4a" | "aac" => "audio/mp4",
        "wav" => "audio/wav",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

pub fn is_image_path(path: &Path) -> bool {
    matches!(guess_extension(path).as_str(), "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp")
}

pub fn is_gif_path(path: &Path) -> bool {
    guess_extension(path).as_str() == "gif"
}

/// Converte una GIF in MP4 H.264. WhatsApp trasporta le GIF come video con
/// il flag `gifPlayback`: senza conversione il destinatario vedrebbe un
/// file rotto (o una foto, come succedeva prima).
pub fn gif_to_mp4(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if !ffmpeg_available() {
        return Err("ffmpeg non è installato".to_string());
    }
    let stamp = crate::util::timestamp_ms();
    let dir = std::env::temp_dir();
    let input = dir.join(format!("whats-open-{stamp}.gif"));
    let output = dir.join(format!("whats-open-{stamp}.mp4"));
    std::fs::write(&input, bytes).map_err(|err| format!("GIF: {err}"))?;
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-nostdin", "-y", "-i"])
        .arg(&input)
        .args([
            "-movflags",
            "+faststart",
            "-pix_fmt",
            "yuv420p",
            "-vf",
            "scale=trunc(iw/2)*2:trunc(ih/2)*2",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-an",
        ])
        .arg(&output)
        .status()
        .map_err(|err| format!("ffmpeg: {err}"))?;
    let result = if status.success() {
        std::fs::read(&output).map_err(|err| format!("GIF: {err}"))
    } else {
        Err("ffmpeg non è riuscito a convertire la GIF".to_string())
    };
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&output);
    result
}

pub fn is_video_path(path: &Path) -> bool {
    matches!(guess_extension(path).as_str(), "mp4" | "m4v" | "mov" | "3gp" | "webm" | "mkv")
}

pub fn is_audio_path(path: &Path) -> bool {
    matches!(
        guess_extension(path).as_str(),
        "ogg" | "opus" | "mp3" | "m4a" | "aac" | "wav" | "flac" | "amr"
    )
}

/// File temporaneo in cache per la riproduzione.
pub fn temp_media_path(cache_dir: &Path, name: &str) -> PathBuf {
    let dir = cache_dir.join("play");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estensioni_dai_tipi_mime() {
        assert_eq!(ext_for_mime("image/jpeg", "bin"), "jpg");
        assert_eq!(ext_for_mime("image/png", "bin"), "png");
        assert_eq!(ext_for_mime("audio/ogg; codecs=opus", "bin"), "ogg");
        assert_eq!(ext_for_mime("video/mp4", "bin"), "mp4");
        assert_eq!(ext_for_mime("application/pdf", "bin"), "pdf");
        assert_eq!(ext_for_mime("x/sconosciuto", "bin"), "bin");
    }

    #[test]
    fn nomi_di_file_sicuri() {
        assert_eq!(safe_file_name("../../etc/passwd", "file"), "passwd");
        assert_eq!(safe_file_name("", "file"), "file");
        assert_eq!(safe_file_name("contratto.pdf", "file"), "contratto.pdf");
        let tricky = safe_file_name("\\\\server\\condivisione\\doc.pdf", "file");
        assert!(!tricky.contains('\\'));
    }

    #[test]
    fn riconosce_i_tipi_dal_percorso() {
        assert!(is_image_path(Path::new("/tmp/a.JPG")));
        assert!(is_image_path(Path::new("/tmp/a.webp")));
        assert!(is_video_path(Path::new("/tmp/a.mp4")));
        assert!(is_audio_path(Path::new("/tmp/a.ogg")));
        assert!(!is_image_path(Path::new("/tmp/a.pdf")));
        assert_eq!(mime_for_path(Path::new("/tmp/a.png")), "image/png");
        assert_eq!(
            mime_for_path(Path::new("/tmp/a.opus")),
            "audio/ogg; codecs=opus"
        );
    }

    #[test]
    fn percorso_temporaneo_nella_cache() {
        let path = temp_media_path(Path::new("/tmp/cache"), "video-1.mp4");
        assert!(path.starts_with("/tmp/cache"));
        assert_eq!(path.file_name().unwrap(), "video-1.mp4");
    }
}

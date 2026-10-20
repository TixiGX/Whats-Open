//! Whats-Open: client WhatsApp nativo, minimale e monocromatico per Linux.
//!
//! Nessuna WebView, nessun runtime JavaScript: interfaccia disegnata con egui,
//! protocollo WhatsApp gestito in puro Rust, audio e video decodificati da
//! ffmpeg. La sessione vive in `~/.local/share/whats-open`, la cache dei media
//! in `~/.cache/whats-open`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod engine;
mod icons;
mod media;
mod model;
mod play;
mod store;
mod theme;
mod ui;
mod util;

use util::Paths;

fn main() -> anyhow::Result<()> {
    let paths = Paths::resolve();
    paths.ensure()?;
    util::init_logging(&paths.log_file());
    log::info!("Whats-Open {}", env!("CARGO_PKG_VERSION"));

    let pair = parse_pair_number();

    let icon = load_icon();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Whats-Open")
        .with_app_id("whats-open")
        .with_inner_size([1040.0, 720.0])
        .with_min_inner_size([760.0, 520.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let paths_for_app = paths.clone();
    eframe::run_native(
        "Whats-Open",
        options,
        Box::new(move |cc| {
            let app = ui::App::new(cc, paths_for_app.clone())?;
            let app = match pair.clone() {
                Some(phone) => app.begin_pairing(phone),
                None => app,
            };
            Ok(Box::new(app))
        }),
    )
    .map_err(|error| anyhow::anyhow!("avvio dell'interfaccia: {error}"))?;

    Ok(())
}

fn parse_pair_number() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--pair" | "--accoppia" => {
                let value = args.next().unwrap_or_default();
                let digits: String = value.chars().filter(|c| c.is_ascii_digit()).collect();
                if digits.len() >= 7 {
                    return Some(digits);
                }
            }
            other if other.starts_with("--pair=") => {
                let value = &other[7..];
                let digits: String = value.chars().filter(|c| c.is_ascii_digit()).collect();
                if digits.len() >= 7 {
                    return Some(digits);
                }
            }
            "--help" | "-h" => {
                println!("Whats-Open — client WhatsApp nativo per Linux\n");
                println!("Uso: whats-open [--pair <numero>]\n");
                println!("  --pair, --accoppia   collega con un codice invece del QR");
                std::process::exit(0);
            }
            _ => {}
        }
    }
    None
}

fn load_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../res/icon-64.png");
    let image = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = image.dimensions();
    Some(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

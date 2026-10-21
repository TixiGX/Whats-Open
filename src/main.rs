//! Whats-Open: client WhatsApp nativo, minimale e monocromatico per Linux.
//!
//! Nessuna WebView, nessun runtime JavaScript: interfaccia disegnata con egui,
//! protocollo WhatsApp gestito in puro Rust, audio e video decodificati da
//! ffmpeg (o dal player di sistema). La sessione vive in
//! `~/.local/share/whats-open`, la cache dei media in `~/.cache/whats-open`.

mod config;
mod engine;
mod icons;
mod media;
mod model;
mod platform;
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
    install_panic_hook();
    log::info!(
        "Whats-Open {} su {}",
        env!("CARGO_PKG_VERSION"),
        platform::distro_name()
    );

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return Ok(());
    }
    if args
        .iter()
        .any(|arg| arg == "--version" || arg == "-V" || arg == "--versione")
    {
        println!("Whats-Open {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args
        .iter()
        .any(|arg| arg == "--doctor" || arg == "--diagnostica")
    {
        println!("{}", platform::report(&paths));
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--video-check") {
        match engine::video::self_test() {
            Ok(report) => println!("video: {report}"),
            Err(error) => {
                eprintln!("video: {error}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    let pair = parse_pair_number(&args);

    // Una sola istanza: due processi sullo stesso archivio significano
    // messaggi doppi e lock contesi.
    let instance = match platform::single_instance(&paths) {
        Ok(lock) => lock,
        Err(message) => {
            eprintln!("{message}");
            log::warn!("{message}");
            notify(&message);
            return Ok(());
        }
    };
    let focus_flag = instance.focus_flag();

    let icon = load_icon();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Whats-Open")
        .with_app_id("whats-open")
        .with_inner_size([1040.0, 720.0])
        // Sotto gli 820 px la finestra passa a una schermata per volta:
        // la chat resta leggibile anche non massimizzata o su schermi piccoli.
        .with_min_inner_size([560.0, 430.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let paths_for_app = paths.clone();
    let run = eframe::run_native(
        "Whats-Open",
        options,
        Box::new(move |cc| {
            let app = ui::App::new(cc, paths_for_app.clone())?;
            let app = match pair.clone() {
                Some(phone) => app.begin_pairing(phone),
                None => app,
            };
            Ok(Box::new(app.with_focus_flag(focus_flag.clone())))
        }),
    );

    match run {
        Ok(()) => Ok(()),
        Err(error) => {
            // Se manca un driver grafico non c'è modo di mostrare una finestra:
            // il messaggio su terminale e nel log è l'unica strada.
            let text = format!(
                "Impossibile aprire la finestra: {error}\n\
                 Verifica di essere in una sessione grafica (DISPLAY o WAYLAND_DISPLAY).\n\
                 Diagnostica completa: whats-open --doctor"
            );
            eprintln!("{text}");
            log::error!("{text}");
            notify("Whats-Open non riesce ad aprire la finestra. Esegui: whats-open --doctor");
            Err(anyhow::anyhow!("{error}"))
        }
    }
}

fn print_help() {
    println!(
        "Whats-Open {} — client WhatsApp nativo per Linux\n\n\
         Uso: whats-open [opzioni]\n\n\
         Opzioni:\n\
         \x20 --pair, --accoppia <numero>  collega con un codice a 8 caratteri\n\
         \x20 --doctor, --diagnostica      stampa l'ambiente e le dipendenze trovate\n\
         \x20 --video-check                prova il percorso video delle chiamate\n\
         \x20 --version                    versione\n\
         \x20 --help                       questo testo\n\n\
         Variabili d'ambiente:\n\
         \x20 WHATS_OPEN_NO_SYSTEM_FONTS=1  usa solo i font incorporati\n\
         \x20 WINIT_UNIX_BACKEND=x11|wayland  forza il backend grafico\n",
        env!("CARGO_PKG_VERSION")
    );
}

fn parse_pair_number(args: &[String]) -> Option<String> {
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        let value = if arg == "--pair" || arg == "--accoppia" {
            index += 1;
            args.get(index).cloned().unwrap_or_default()
        } else if let Some(rest) = arg.strip_prefix("--pair=") {
            rest.to_string()
        } else {
            index += 1;
            continue;
        };
        let digits: String = value.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.len() >= 7 {
            return Some(digits);
        }
        index += 1;
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

/// Un panic non deve sparire: finisce nel log accanto alla versione e alla
/// distribuzione, così una segnalazione è utile senza doverlo riprodurre.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!(
            "panic su {}: {info}",
            platform::distro_name()
        );
        previous(info);
        if let Some(location) = info.location() {
            notify(&format!("Whats-Open: errore interno in {}", location));
        }
    }));
}

/// Avviso al desktop, quando c'è un servizio di notifiche.
fn notify(message: &str) {
    if !platform::command_exists("notify-send") {
        return;
    }
    let _ = std::process::Command::new("notify-send")
        .args(["--app-name=Whats-Open", "Whats-Open", message])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

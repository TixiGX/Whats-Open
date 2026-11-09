//! Notifiche di sistema, senza dipendenze e senza processi esterni.
//!
//! Si parla direttamente con il bus di sessione (`org.freedesktop.Notifications`,
//! metodo `Notify`): connessione Unix, autenticazione `EXTERNAL`, un messaggio
//! D-Bus composto a mano. Il thread che invia resta in ascolto di
//! `ActionInvoked`, così cliccare la notifica riporta la finestra davanti.
//!
//! Se il desktop non ha un servizio di notifiche il messaggio si perde nel
//! nulla: l'app continua a funzionare e non avvia `notify-send` né altri
//! programmi. Tutto il traffico è locale.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// Una notifica pronta per il bus.
#[derive(Clone, Debug)]
pub struct Note {
    pub summary: String,
    pub body: String,
    /// 0 = bassa, 1 = normale, 2 = critica (chiamate in arrivo).
    pub urgency: u8,
}

static ENABLED: AtomicBool = AtomicBool::new(true);
static FOCUS: AtomicBool = AtomicBool::new(false);
static SENDER: LazyLock<Mutex<Option<Sender<Note>>>> = LazyLock::new(|| Mutex::new(None));
static CONTEXT: LazyLock<Mutex<Option<egui::Context>>> = LazyLock::new(|| Mutex::new(None));

/// Collega il modulo all'interfaccia: serve solo per ridisegnare la finestra
/// quando si clicca la notifica.
pub fn init(ctx: &egui::Context, enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
    if let Ok(mut guard) = CONTEXT.lock() {
        *guard = Some(ctx.clone());
    }
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Vero se l'utente ha cliccato una notifica: l'interfaccia deve portarsi
/// davanti. La bandierina si consuma con la lettura.
pub fn take_focus() -> bool {
    FOCUS.swap(false, Ordering::Relaxed)
}

/// Mette in coda una notifica. Non blocca mai l'interfaccia: se il bus non
/// c'è, il thread di servizio lo scopre per conto suo.
pub fn send(summary: &str, body: &str, urgency: u8) {
    if !enabled() || summary.trim().is_empty() {
        return;
    }
    let note = Note {
        summary: summary.trim().to_string(),
        body: body.trim().to_string(),
        urgency,
    };
    let Ok(mut guard) = SENDER.lock() else {
        return;
    };
    if guard.is_none() {
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("notifiche".to_string())
            .spawn(move || worker(rx));
        if spawned.is_err() {
            return;
        }
        *guard = Some(tx);
    }
    if let Some(tx) = guard.as_ref() {
        let _ = tx.send(note);
    }
}

fn worker(rx: Receiver<Note>) {
    let mut bus: Option<Bus> = None;
    let mut last_attempt = Instant::now() - Duration::from_secs(30);
    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(note) => {
                if bus.is_none() && last_attempt.elapsed() > Duration::from_secs(10) {
                    last_attempt = Instant::now();
                    bus = Bus::connect();
                    if let Some(connection) = bus.as_mut() {
                        connection.add_match();
                    } else {
                        log::debug!("notifiche: nessun bus di sessione raggiungibile");
                    }
                }
                if let Some(connection) = bus.as_mut() {
                    if let Err(error) = connection.notify(&note) {
                        log::debug!("notifiche: invio non riuscito ({error})");
                        bus = None;
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if let Some(connection) = bus.as_mut() {
            if !connection.pump() {
                bus = None;
            }
        }
    }
}

fn on_click() {
    FOCUS.store(true, Ordering::Relaxed);
    if let Some(ctx) = CONTEXT.lock().ok().and_then(|guard| guard.clone()) {
        ctx.request_repaint();
    }
}

// ── il bus ───────────────────────────────────────────────────────────────

struct Bus {
    stream: UnixStream,
    serial: u32,
    buffer: Vec<u8>,
    /// Ultimo errore ricevuto dal bus (serve a `--notify-check`).
    last_error: Option<String>,
    got_reply: bool,
}

impl Bus {
    fn connect() -> Option<Self> {
        let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").ok();
        let stream = match address.as_deref().and_then(open_stream) {
            Some(stream) => stream,
            None => open_stream(&default_address())?,
        };
        let mut bus = Self::handshake(stream)?;
        bus.hello().then_some(bus)
    }

    fn connect_to(address: &str) -> Option<Self> {
        let mut bus = Self::handshake(open_stream(address)?)?;
        bus.hello().then_some(bus)
    }

    /// Il bus pretende `Hello` come primissimo messaggio: qualunque altra cosa
    /// e chiude il collegamento. La risposta (il nostro nome unico `:1.x`)
    /// viene consumata qui, così non passa per la conferma di una notifica.
    fn hello(&mut self) -> bool {
        if self
            .call(
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "Hello",
                "",
                Enc::new(),
            )
            .is_err()
        {
            return false;
        }
        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline {
            if !self.pump() {
                return false;
            }
            if self.got_reply || self.last_error.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let ok = self.got_reply && self.last_error.is_none();
        self.got_reply = false;
        self.last_error = None;
        ok
    }

    fn handshake(mut stream: UnixStream) -> Option<Self> {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .ok()?;
        stream.write_all(b"\0").ok()?;
        let uid = unsafe { libc::geteuid() };
        let hex: String = uid
            .to_string()
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        stream
            .write_all(format!("AUTH EXTERNAL {hex}\r\n").as_bytes())
            .ok()?;
        let reply = read_line(&mut stream)?;
        if !reply.starts_with("OK") {
            log::debug!("notifiche: autenticazione rifiutata ({reply})");
            return None;
        }
        stream.write_all(b"BEGIN\r\n").ok()?;
        // Da qui in poi si leggono messaggi binari: attesa breve per non
        // restare mai appesi se il bus tace.
        stream
            .set_read_timeout(Some(Duration::from_millis(1)))
            .ok()?;
        Some(Self {
            stream,
            serial: 1,
            buffer: Vec::new(),
            last_error: None,
            got_reply: false,
        })
    }

    fn add_match(&mut self) {
        let rule = "type='signal',interface='org.freedesktop.Notifications',member='ActionInvoked'";
        let mut body = Enc::new();
        body.string(rule);
        let _ = self.call(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "AddMatch",
            "s",
            body,
        );
    }

    fn notify(&mut self, note: &Note) -> std::io::Result<()> {
        let body = notify_body(note);
        self.call(
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "Notify",
            "susssasa{sv}i",
            body,
        )
    }

    fn call(
        &mut self,
        destination: &str,
        path: &str,
        interface: &str,
        member: &str,
        signature: &str,
        body: Enc,
    ) -> std::io::Result<()> {
        let serial = self.serial;
        self.serial = self.serial.wrapping_add(1).max(1);
        let mut fields = Enc::new();
        put_field(&mut fields, 1, "o", path);
        put_field(&mut fields, 2, "s", interface);
        put_field(&mut fields, 3, "s", member);
        put_field(&mut fields, 6, "s", destination);
        put_field(&mut fields, 8, "g", signature);
        let message = build_message(1, 0, serial, &fields, &body);
        self.stream.write_all(&message)?;
        self.stream.flush()
    }

    /// Legge quello che il bus ha da dire (risposte, clic sulle notifiche).
    fn pump(&mut self) -> bool {
        let mut chunk = [0u8; 4096];
        loop {
            match self.stream.read(&mut chunk) {
                Ok(0) => return false,
                Ok(count) => {
                    self.buffer.extend_from_slice(&chunk[..count]);
                    if self.buffer.len() > 1 << 20 {
                        self.buffer.clear();
                    }
                    if !self.consume() {
                        return false;
                    }
                }
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        || error.kind() == std::io::ErrorKind::TimedOut =>
                {
                    return true;
                }
                Err(_) => return false,
            }
        }
    }

    fn consume(&mut self) -> bool {
        loop {
            let Some(header) = Header::parse(&self.buffer) else {
                return true;
            };
            if self.buffer.len() < header.total {
                return true;
            }
            let message: Vec<u8> = self.buffer.drain(..header.total).collect();
            if header.kind == 2 {
                self.got_reply = true;
            }
            if header.kind == 3 {
                self.got_reply = true;
                if !header.error_name.is_empty() {
                    log::debug!("notifiche: il bus risponde {}", header.error_name);
                    self.last_error = Some(header.error_name.clone());
                }
            }
            if header.kind == 4
                && header.interface == "org.freedesktop.Notifications"
                && header.member == "ActionInvoked"
            {
                if let Some(action) = header.body_string(message.as_slice(), 4) {
                    if action == "default" || action == "Apri" {
                        on_click();
                    }
                }
            }
        }
    }
}

fn open_stream(address: &str) -> Option<UnixStream> {
    for part in address.split(';') {
        let rest = match part.strip_prefix("unix:") {
            Some(rest) => rest,
            None => continue,
        };
        for pair in rest.split(',') {
            if let Some(path) = pair.strip_prefix("path=") {
                if let Ok(stream) = UnixStream::connect(path) {
                    return Some(stream);
                }
            }
            if let Some(name) = pair.strip_prefix("abstract=") {
                use std::os::linux::net::SocketAddrExt;
                if let Ok(address) =
                    std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes())
                {
                    if let Ok(stream) = UnixStream::connect_addr(&address) {
                        return Some(stream);
                    }
                }
            }
        }
    }
    None
}

fn default_address() -> String {
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        return format!("unix:path={}/bus", std::path::Path::new(&runtime).display());
    }
    let uid = unsafe { libc::geteuid() };
    format!("unix:path=/run/user/{uid}/bus")
}

fn read_line(stream: &mut UnixStream) -> Option<String> {
    let mut out = Vec::new();
    let mut byte = [0u8; 1];
    while out.len() < 512 {
        match stream.read(&mut byte) {
            Ok(0) => return None,
            Ok(_) => {
                if byte[0] == b'\n' {
                    let text = String::from_utf8_lossy(&out).trim().to_string();
                    return Some(text);
                }
                if byte[0] != b'\r' {
                    out.push(byte[0]);
                }
            }
            Err(_) => return None,
        }
    }
    None
}

// ── codifica D-Bus ───────────────────────────────────────────────────────

#[derive(Default)]
struct Enc {
    buf: Vec<u8>,
}

impl Enc {
    fn new() -> Self {
        Self { buf: Vec::new() }
    }

    fn align(&mut self, boundary: usize) {
        while self.buf.len() % boundary != 0 {
            self.buf.push(0);
        }
    }

    fn u8(&mut self, value: u8) {
        self.buf.push(value);
    }

    fn raw_u32(&mut self, value: u32) {
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.align(4);
        self.raw_u32(value);
    }

    fn i32(&mut self, value: i32) {
        self.align(4);
        self.buf.extend_from_slice(&value.to_le_bytes());
    }

    fn string(&mut self, value: &str) {
        self.align(4);
        self.raw_u32(value.len() as u32);
        self.buf.extend_from_slice(value.as_bytes());
        self.buf.push(0);
    }

    fn signature(&mut self, value: &str) {
        self.u8(value.len() as u8);
        self.buf.extend_from_slice(value.as_bytes());
        self.buf.push(0);
    }

    fn variant_byte(&mut self, value: u8) {
        self.signature("y");
        self.u8(value);
    }

    fn variant_string(&mut self, value: &str) {
        self.signature("s");
        self.string(value);
    }

    fn string_array(&mut self, values: &[&str]) {
        let length: usize = values.iter().map(|value| 5 + value.len()).sum();
        self.align(4);
        self.raw_u32(length as u32);
        for value in values {
            self.string(value);
        }
    }

    /// `a{sv}`: il contenuto del dizionario è già codificato in `entries`
    /// (allineato a 8), qui si scrive la lunghezza e lo si accoda.
    fn dict(&mut self, entries: &Enc) {
        self.align(4);
        self.raw_u32(entries.buf.len() as u32);
        self.align(8);
        self.buf.extend_from_slice(&entries.buf);
    }
}

/// Corpo del metodo `Notify`: `susssasa{sv}i`.
fn notify_body(note: &Note) -> Enc {
    let mut body = Enc::new();
    body.string("Whats-Open");
    body.u32(0);
    body.string("whats-open");
    body.string(&note.summary);
    body.string(&note.body);
    let actions = ["default", "Apri"];
    body.string_array(&actions);
    let mut hints = Enc::new();
    hints.align(8);
    hints.string("urgency");
    hints.variant_byte(if note.urgency == 0 { 0 } else { 1 });
    hints.align(8);
    hints.string("desktop-entry");
    hints.variant_string("whats-open");
    body.dict(&hints);
    let timeout = if note.urgency >= 2 { 12_000 } else { 6_000 };
    body.i32(timeout);
    body
}

fn put_field(fields: &mut Enc, code: u8, signature: &str, value: &str) {
    fields.align(8);
    fields.u8(code);
    fields.signature(signature);
    match signature {
        // Il campo SIGNATURE ha per valore un'altra firma.
        "g" => fields.signature(value),
        "s" | "o" => fields.string(value),
        _ => {}
    }
}

fn align8(value: usize) -> usize {
    (value + 7) & !7
}

fn build_message(kind: u8, flags: u8, serial: u32, fields: &Enc, body: &Enc) -> Vec<u8> {
    let body_start = align8(16 + fields.buf.len());
    let mut out = Vec::with_capacity(body_start + body.buf.len());
    out.push(b'l');
    out.push(kind);
    out.push(flags);
    out.push(1);
    out.extend_from_slice(&(body.buf.len() as u32).to_le_bytes());
    out.extend_from_slice(&serial.to_le_bytes());
    out.extend_from_slice(&(fields.buf.len() as u32).to_le_bytes());
    out.extend_from_slice(&fields.buf);
    out.resize(body_start, 0);
    out.extend_from_slice(&body.buf);
    out
}

struct Header {
    kind: u8,
    member: String,
    interface: String,
    error_name: String,
    body_start: usize,
    total: usize,
}

impl Header {
    fn parse(buffer: &[u8]) -> Option<Self> {
        if buffer.len() < 16 || buffer[0] != b'l' {
            return None;
        }
        let kind = buffer[1];
        let body_len = u32::from_le_bytes(buffer[4..8].try_into().ok()?) as usize;
        let fields_len = u32::from_le_bytes(buffer[12..16].try_into().ok()?) as usize;
        let body_start = align8(16 + fields_len);
        let total = body_start + body_len;
        if total > 1 << 20 {
            return None;
        }
        let mut member = String::new();
        let mut interface = String::new();
        let mut error_name = String::new();
        let mut offset = 16;
        let end = 16 + fields_len;
        while offset < end {
            offset = align8(offset);
            if offset >= end || offset >= buffer.len() {
                break;
            }
            let code = buffer[offset];
            offset += 1;
            let (signature, next) = read_signature(buffer, offset)?;
            offset = next;
            match signature.as_str() {
                "s" | "o" => {
                    let (value, next) = read_string(buffer, offset)?;
                    offset = next;
                    match code {
                        1 => {}
                        2 => interface = value,
                        3 => member = value,
                        4 => error_name = value,
                        _ => {}
                    }
                }
                "g" => {
                    let (_value, next) = read_signature(buffer, offset)?;
                    offset = next;
                }
                "u" => {
                    offset = (offset + 3) & !3;
                    offset += 4;
                }
                "y" => offset += 1,
                _ => return None,
            }
        }
        Some(Self {
            kind,
            member,
            interface,
            error_name,
            body_start,
            total,
        })
    }

    /// Legge una stringa dal corpo del messaggio, saltando `skip` byte.
    fn body_string(&self, message: &[u8], skip: usize) -> Option<String> {
        if message.len() < self.body_start + skip {
            return None;
        }
        let mut offset = self.body_start + skip;
        offset = (offset + 3) & !3;
        read_string(message, offset).map(|(value, _)| value)
    }
}

fn read_u32(buffer: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(buffer.get(offset..offset + 4)?.try_into().ok()?))
}

fn read_string(buffer: &[u8], offset: usize) -> Option<(String, usize)> {
    let length = read_u32(buffer, offset)? as usize;
    let start = offset + 4;
    let end = start.checked_add(length)?;
    if end > buffer.len() || length > 64 * 1024 {
        return None;
    }
    let value = String::from_utf8_lossy(&buffer[start..end]).to_string();
    Some((value, end + 1))
}

fn read_signature(buffer: &[u8], offset: usize) -> Option<(String, usize)> {
    let length = *buffer.get(offset)? as usize;
    let start = offset + 1;
    let end = start.checked_add(length)?;
    if end > buffer.len() {
        return None;
    }
    let value = String::from_utf8_lossy(&buffer[start..end]).to_string();
    Some((value, end + 1))
}

// ── verifica per l'utente ────────────────────────────────────────────────

/// Invia una notifica di prova e riferisce cosa risponde il desktop.
pub fn self_test() -> Result<String, String> {
    let mut bus = Bus::connect().ok_or_else(|| {
        "nessun bus di sessione D-Bus: le notifiche di sistema non sono disponibili".to_string()
    })?;
    let note = Note {
        summary: "Whats-Open".to_string(),
        body: "Le notifiche di sistema funzionano.".to_string(),
        urgency: 1,
    };
    bus.notify(&note)
        .map_err(|error| format!("invio: {error}"))?;
    let deadline = Instant::now() + Duration::from_millis(1200);
    while Instant::now() < deadline {
        if !bus.pump() {
            break;
        }
        if let Some(error) = bus.last_error.clone() {
            if error.contains("ServiceUnknown") || error.contains("NameHasNoOwner") {
                return Err(
                    "il desktop non ha un servizio di notifiche attivo (org.freedesktop.Notifications)"
                        .to_string(),
                );
            }
            return Err(format!("il bus ha risposto: {error}"));
        }
        if bus.got_reply {
            return Ok("notifica consegnata al servizio del desktop".to_string());
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    Err("il servizio di notifiche non ha risposto".to_string())
}

/// Come [`self_test`], ma per il collaudo generale: un desktop senza bus o
/// senza servizio di notifiche non è un guasto dell'app, è una funzione che
/// quel desktop non offre.
pub fn probe() -> Result<String, String> {
    match self_test() {
        Ok(detail) => Ok(detail),
        Err(error)
            if error.contains("nessun bus")
                || error.contains("non ha un servizio")
                || error.contains("non ha risposto") =>
        {
            Ok(format!("non disponibile: {error}"))
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field_string(fields: &[u8], code: u8) -> Option<String> {
        let mut offset = 0;
        while offset < fields.len() {
            offset = (offset + 7) & !7;
            if offset >= fields.len() {
                break;
            }
            let this = fields[offset];
            offset += 1;
            let (signature, next) = read_signature(fields, offset)?;
            offset = next;
            match signature.as_str() {
                "s" | "o" => {
                    let (value, next) = read_string(fields, offset)?;
                    offset = next;
                    if this == code {
                        return Some(value);
                    }
                }
                "g" => {
                    let (_value, next) = read_signature(fields, offset)?;
                    offset = next;
                }
                _ => return None,
            }
        }
        None
    }

    #[test]
    fn il_messaggio_notify_e_costruito_bene() {
        let fields_enc = {
            let mut fields = Enc::new();
            put_field(&mut fields, 1, "o", "/org/freedesktop/Notifications");
            put_field(&mut fields, 2, "s", "org.freedesktop.Notifications");
            put_field(&mut fields, 3, "s", "Notify");
            put_field(&mut fields, 8, "g", "susssasa{sv}i");
            fields
        };
        assert_eq!(
            field_string(&fields_enc.buf, 3).as_deref(),
            Some("Notify")
        );
        assert_eq!(
            field_string(&fields_enc.buf, 1).as_deref(),
            Some("/org/freedesktop/Notifications")
        );

        let note = Note {
            summary: "Anna".to_string(),
            body: "Ciao!".to_string(),
            urgency: 2,
        };
        let body = notify_body(&note);
        let message = build_message(1, 0, 5, &fields_enc, &body);
        assert_eq!(message[0], b'l');
        assert_eq!(message[1], 1);
        assert_eq!(u32::from_le_bytes(message[8..12].try_into().unwrap()), 5);
        assert_eq!(
            u32::from_le_bytes(message[4..8].try_into().unwrap()) as usize,
            body.buf.len()
        );
        // Il corpo inizia allineato a 8 e contiene nome e testo.
        let header = Header::parse(&message).expect("intestazione leggibile");
        assert_eq!(header.total, message.len());
        assert_eq!(header.member, "Notify");
        let haystack = String::from_utf8_lossy(&message).to_string();
        assert!(haystack.contains("Anna"));
        assert!(haystack.contains("Ciao!"));
        assert!(haystack.contains("desktop-entry"));
    }

    /// Il test più importante: un `dbus-daemon` vero deve accettare il
    /// messaggio. Senza servizio di notifiche risponde "ServiceUnknown", che
    /// è esattamente la prova che il bus ha capito e instradato la chiamata.
    #[test]
    fn un_bus_vero_accetta_la_notifica() {
        if !crate::platform::command_exists("dbus-daemon") {
            // In CI il pacchetto `dbus` c'è: se manca, il test non deve
            // sparire in silenzio proprio dove serve.
            if std::env::var_os("WHATS_OPEN_REQUIRE_DBUS").is_some() {
                panic!("dbus-daemon assente ma richiesto (WHATS_OPEN_REQUIRE_DBUS)");
            }
            eprintln!("dbus-daemon assente: test del bus saltato");
            return;
        }
        let mut daemon = match std::process::Command::new("dbus-daemon")
            .args(["--session", "--print-address=1", "--nofork"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                eprintln!("dbus-daemon non avviato: {error}");
                return;
            }
        };
        let mut address = String::new();
        if let Some(stdout) = daemon.stdout.take() {
            use std::io::BufRead;
            let mut reader = std::io::BufReader::new(stdout);
            let _ = reader.read_line(&mut address);
        }
        let address = address.trim().to_string();
        if address.is_empty() {
            let _ = daemon.kill();
            eprintln!("dbus-daemon senza indirizzo");
            return;
        }

        let result = (|| -> Result<(), String> {
            let mut bus = Bus::connect_to(&address).ok_or("connessione al bus fallita")?;
            bus.add_match();
            assert!(bus.pump(), "il bus ha chiuso la connessione dopo AddMatch");
            let note = Note {
                summary: "Whats-Open collaudo".to_string(),
                body: "messaggio di prova".to_string(),
                urgency: 1,
            };
            bus.notify(&note).map_err(|error| error.to_string())?;
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline {
                if !bus.pump() {
                    return Err("il bus ha chiuso la connessione".to_string());
                }
                if let Some(error) = bus.last_error.clone() {
                    // Atteso in CI e su un desktop spento: il bus ha letto la
                    // chiamata e non c'è chi la serve.
                    if error.contains("ServiceUnknown") || error.contains("NameHasNoOwner") {
                        return Ok(());
                    }
                    return Err(format!("errore inatteso dal bus: {error}"));
                }
                if bus.got_reply {
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err("nessuna risposta dal bus".to_string())
        })();

        let _ = daemon.kill();
        let _ = daemon.wait();
        if let Err(error) = result {
            panic!("bus D-Bus: {error}");
        }
    }
}

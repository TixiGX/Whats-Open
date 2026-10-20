# Whats-Open

Client WhatsApp nativo per Linux: interfaccia minimale in bianco e nero,
protocollo in puro Rust, nessuna WebView e nessun runtime JavaScript.

Whats-Open non incorpora WhatsApp Web: parla direttamente con i server usando
la libreria [`whatsapp-rust`](https://github.com/oxidezap/whatsapp-rust)
(protocollo Signal/Noise, media cifrati end-to-end, sincronizzazione dello
storico). L'interfaccia è disegnata con egui, l'audio passa da cpal, i video da
ffmpeg.

## Cosa sa fare

- Accesso con QR oppure con codice di accoppiamento (`--pair +39…`)
- Chat singole e di gruppo, ricerca, anteprime, non letti, presenza e
  "sta scrivendo"
- Testo, foto, video, documenti, sticker, posizioni e contatti
- Foto, video e audio **view-once**: si aprono una sola volta, poi il contenuto
  viene eliminato dalla cache locale
- Note vocali: registrazione dal microfono (Ogg/Opus) e riproduzione con forma
  d'onda e avanzamento
- Lettore video integrato con pausa e ricerca
- Risposte citate, reazioni, modifica ed eliminazione dei propri messaggi
- Spunte di invio/consegna/lettura, ricevute di ascolto per i vocali
- Storico iniziale importato dal telefono
- Trascinamento dei file nella finestra oppure selettore di sistema

Non ci sono chiamate, storie o canali: l'obiettivo è un client leggero, non una
copia di WhatsApp Web.

## Requisiti

- Linux con X11 o Wayland
- `ffmpeg` per riprodurre audio e video (opzionale: senza, il resto funziona)
- Per compilare: Rust 1.94 o successivo, `pkg-config`, le librerie di sviluppo
  di ALSA, Wayland/X11 e OpenGL/EGL.

Su Debian/Ubuntu:

```bash
sudo apt install build-essential pkg-config libasound2-dev libxkbcommon-dev \
  libwayland-dev libx11-dev libxcursor-dev libxi-dev libxrandr-dev \
  libgl1-mesa-dev libegl1-mesa-dev ffmpeg
```

## Compilare

```bash
cargo build --release
./target/release/whats-open
```

Il binario incorpora l'icona e le icone dell'interfaccia: non servono file
esterni.

## Installare

```bash
install -Dm755 target/release/whats-open ~/.local/bin/whats-open
install -Dm644 res/whats-open.desktop ~/.local/share/applications/whats-open.desktop
for s in 16 24 32 48 64 128 256 512; do
  install -Dm644 "dist/icons/whats-open-$s.png" \
    ~/.local/share/icons/hicolor/${s}x${s}/apps/whats-open.png
done
```

(Le icone derivate si generano con `python3 dist/icon-gen.py dist/whats-open.png`.)

## Uso

```bash
whats-open                 # accesso con QR
whats-open --pair 3933...  # accesso con codice a 8 caratteri
```

Dati locali:

- `~/.local/share/whats-open/session.db` — credenziali e chiavi della sessione
- `~/.local/share/whats-open/messages.db` — chat e messaggi
- `~/.cache/whats-open/media/` — allegati scaricati
- `~/.cache/whats-open/whats-open.log` — registro

## Architettura

```
interfaccia egui  ──comandi──▶  thread di rete (Tokio + whatsapp-rust)
      ▲                              │
      └────────── eventi ────────────┘
```

- `src/engine` — sessione WhatsApp, invio, download, eventi normalizzati
- `src/ui` — lista chat, conversazione, lettori, impostazioni
- `src/play` — audio (cpal + ffmpeg), video (ffmpeg), registrazione
- `src/store` — archivio SQLite di chat e messaggi
- `src/theme.rs`, `src/icons.rs` — tema monocromatico e icone disegnate a mano

## Licenza

MIT.

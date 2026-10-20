# Whats-Open

Client WhatsApp nativo per Linux: interfaccia minimale in bianco e nero,
protocollo in puro Rust, nessuna WebView e nessun runtime JavaScript.

Whats-Open non incorpora WhatsApp Web: parla direttamente con i server usando
la libreria [`whatsapp-rust`](https://github.com/oxidezap/whatsapp-rust)
(protocollo Signal/Noise, media cifrati end-to-end, sincronizzazione dello
storico). L'interfaccia è disegnata con egui, l'audio passa da cpal o dal player
di sistema, i video da ffmpeg o dal player di sistema.

## Funzioni

- Accesso con QR oppure con codice di accoppiamento (`--pair +39…`)
- Chat singole e di gruppo, ricerca, anteprime, non letti, presenza e
  "sta scrivendo"
- Testo, foto, video, documenti, GIF, sticker animati, posizioni e contatti
- Foto, video e audio **view-once**: arrivano con la miniatura velata, si
  aprono una sola volta e poi spariscono dalla cache e dall'archivio
- **Chiamate vocali** (in arrivo, in uscita, mute, ricevute di squillo): una
  videochiamata in arrivo si risponde in audio, il video non è ancora supportato
- Nomi dei contatti dalla rubrica del telefono, dallo storico e dai messaggi già
  archiviati; la mappatura LID ↔ numero resta anche dopo il riavvio
- Note vocali: registrazione dal microfono (Ogg/Opus) e riproduzione con forma
  d'onda e avanzamento
- Lettore video integrato con pausa e ricerca
- Risposte citate, reazioni, modifica ed eliminazione dei propri messaggi
- Spunte di invio/consegna/lettura, ricevute di ascolto per i vocali
- Storico iniziale importato dal telefono
- File trascinati nella finestra, selettore di sistema o percorso a mano
- Istanza singola: la seconda copia riporta davanti la finestra già aperta
- Log su file (con rotazione) e diagnostica integrata

Non ci sono storie, canali né videochiamate: l'obiettivo è un client leggero,
non una copia di WhatsApp Web.

## Installazione

Ogni release pubblica due archivi già pronti (`whats-open-linux-x86_64-musl` e
`whats-open-linux-x86_64-glibc`); chi preferisce compilare può farlo con
`cargo build --release`.

### Binario statico (qualsiasi distribuzione)

L'archivio `whats-open-linux-x86_64-musl.tar.gz` contiene un binario
**statico**: non dipende dalla versione di glibc né dalla distribuzione, e ALSA
è compilata dentro il binario. Va bene su Debian, Ubuntu, Fedora, RHEL,
openSUSE, Arch, Alpine, NixOS, Void, Gentoo, Slackware, sui sistemi immutabili
(Silverblue, MicroOS) e nelle sandbox Flatpak o Distrobox.

```bash
tar xzf whats-open-linux-x86_64-musl.tar.gz
cd whats-open-0.1.0-linux-x86_64-musl
./install.sh --user          # in ~/.local, senza root
# oppure: sudo ./install.sh  # in /usr/local
```

### Binario glibc

`whats-open-linux-x86_64-glibc.tar.gz` è compilato su Ubuntu 20.04 (glibc
2.31): funziona su Debian 11+, Ubuntu 20.04+, Fedora 33+, RHEL 9+, openSUSE Leap
15.4+, Arch e derivate. Usa le librerie del sistema, quindi pesa meno del
binario statico. Stessa procedura di installazione.

### Dal codice sorgente

```bash
# dalla radice del repository, non dalla cartella packaging/
cargo build --release
sudo ./packaging/install.sh --prefix=/usr
```

Lo script riconosce da solo se sta lavorando su un archivio già pronto
(`bin/`, `share/`) o su un repository appena compilato (`target/release/`,
`res/`, `dist/icons/`). In alternativa si può lanciare `cargo install --path .`
per avere il binario in `~/.cargo/bin`.

Su qualunque architettura (x86_64, aarch64, riscv64, …) basta una toolchain
Rust: la compilazione avviene in locale con le librerie del sistema.

#### Macchine con poca RAM

Il profilo `release` è tarato per non superare 1–2 GB di RAM per processo:
niente LTO e 16 unità di compilazione. Se il kernel uccide `rustc` (SIGKILL,
spesso sul crate `waproto`), riduci l'ultimo fattore:

```bash
cargo build --release -j 2      # o -j 1: molta meno RAM, solo più lenta
```

Chi ha RAM da vendere può tornare al binario più compatto:

```bash
CARGO_PROFILE_RELEASE_LTO=thin CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
  cargo build --release
```

## Dipendenze

Tutto ciò che serve a disegnare la finestra arriva dal sistema; l'app non
include né GTK, né Qt, né WebKit.

| Componente | Serve per | Se manca |
|---|---|---|
| `libX11`/`libxkbcommon` oppure `libwayland` | finestra | non parte (obbligatorio) |
| `libGL`/`libEGL` | disegno | non parte: prova `LIBGL_ALWAYS_SOFTWARE=1` |
| `libasound` (ALSA) | altoparlanti e microfono (già dentro il binario musl) | messaggi vocali e audio non suonano |
| `ffmpeg`/`ffprobe` | decodifica audio/video, anteprime | apre i media col player di sistema |
| `xdg-desktop-portal` + `zenity` | finestra "allega file" | si può indicare il percorso a mano |
| `notify-send` | avvisi di sistema | nessun avviso, il resto funziona |
| `xdg-open`, `gio`, `kde-open` | apertura documenti | usa il percorso in cache |

Installazione delle dipendenze per famiglia di distribuzioni:

```bash
# Debian, Ubuntu, Mint, Pop!_OS, Raspbian
sudo apt install libasound2 libx11-6 libxkbcommon0 libgl1 libegl1 ffmpeg \
     xdg-desktop-portal

# Fedora, RHEL, Rocky, AlmaLinux
sudo dnf install alsa-lib libX11 libxkbcommon mesa-libGL libglvnd-egl ffmpeg \
     xdg-desktop-portal

# Arch, Manjaro, EndeavourOS
sudo pacman -S alsa-lib libx11 libxkbcommon mesa ffmpeg xdg-desktop-portal

# openSUSE Tumbleweed / Leap
sudo zypper install libasound2 libX11-6 libxkbcommon0 Mesa-libGL1 \
     Mesa-libEGL1 ffmpeg xdg-desktop-portal

# Alpine (binario musl)
sudo apk add alsa-lib libx11 libxkbcommon mesa-gl ffmpeg xdg-desktop-portal
```

Per compilare servono anche gli header di sviluppo (`libasound2-dev`,
`alsa-lib-devel`, `alsa-lib`, …), `pkg-config` e un compilatore C (per SQLite).

## Uso

```bash
whats-open                    # accesso con QR
whats-open --pair 3933...     # accesso con codice a 8 caratteri
whats-open --doctor           # ambiente, dipendenze e percorsi
whats-open --help
```

Dati locali:

| Percorso | Contenuto |
|---|---|
| `~/.local/share/whats-open/session.db` | credenziali e chiavi della sessione |
| `~/.local/share/whats-open/messages.db` | chat e messaggi |
| `~/.local/share/whats-open/config.json` | preferenze |
| `~/.cache/whats-open/media/` | allegati scaricati |
| `~/.cache/whats-open/whats-open.log` | registro (ruota a 4 MB) |

Il socket di istanza singola vive in `$XDG_RUNTIME_DIR/whats-open.sock`.

### Preferenze

`config.json` si modifica anche dall'interfaccia (Impostazioni):

```json
{
  "system_player": false,   // true: audio/video col player di sistema
  "system_fonts": true,     // font di sistema come riserva (emoji, CJK, arabo)
  "graceful_exit": true     // chiude la sessione in modo pulito all'uscita
}
```

### Variabili d'ambiente

| Variabile | Effetto |
|---|---|
| `WHATS_OPEN_NO_SYSTEM_FONTS=1` | usa solo i font incorporati (equivale a togliere la spunta nelle impostazioni) |
| `WINIT_UNIX_BACKEND=x11` o `wayland` | forza il backend grafico |
| `LIBGL_ALWAYS_SOFTWARE=1` | rendering software (schede senza driver OpenGL) |
| `RUST_LOG=debug` | log più verboso |
| `WHATS_OPEN_DEMO=1` | apre l'interfaccia con conversazioni di prova, senza account e senza rete (l'invio è disattivato) |

## Se qualcosa non funziona

1. `whats-open --doctor` stampa distribuzione, sessione grafica, glibc, ffmpeg,
   scheda audio, font trovati e percorsi. Le stesse informazioni sono nel
   pulsante **Diagnostica** delle impostazioni.
2. Il log è in `~/.cache/whats-open/whats-open.log` (pulsante **Apri il log**).
3. **La finestra non si apre**: manca un server grafico o i driver OpenGL.
   Controlla `echo $WAYLAND_DISPLAY $DISPLAY` e prova
   `LIBGL_ALWAYS_SOFTWARE=1 whats-open`.
4. **Audio muto**: verifica `libasound` e il dispositivo predefinito
   (`--doctor`). In assenza di ALSA imposta "Usa il player di sistema".
5. **Video neri**: manca ffmpeg o il codec; con "Usa il player di sistema" il
   file si apre nel player del desktop.
6. **Selettore file che non risponde**: installa `xdg-desktop-portal` e
   `zenity`, oppure usa **Allega → Da percorso…**.
7. **Testo in arabo, cinese o emoji quadretti**: installa i font Noto della tua
   lingua (`fonts-noto`, `noto-fonts`, `noto-fonts-cjk`); l'app li carica
   automaticamente come riserva.
8. **`error: could not compile ... (signal: 9, SIGKILL)`**: è il kernel che
   uccide `rustc` per mancanza di RAM. Compila con `cargo build --release -j 2`
   (o `-j 1`), chiudi le altre applicazioni oppure usa gli archivi precompilati.
9. **`install.sh: comando non trovato`**: lo script va lanciato dalla radice del
   repository (`./packaging/install.sh`) oppure dalla cartella dell'archivio
   estratto (`./install.sh`), non da dentro `packaging/`.
10. **Sessione danneggiata**: dalla schermata di errore usa **Cancella la
    sessione**, oppure rimuovi `~/.local/share/whats-open/session.db`.

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
- `src/platform.rs` — ambiente, font di sistema, apertura esterna, istanza singola
- `src/theme.rs`, `src/icons.rs` — tema monocromatico e icone disegnate a mano
- `packaging/install.sh` — installazione e rimozione su qualsiasi distribuzione

## Icona

Il logo è un fumetto aperto con la cornetta, bianco su nero. Le icone sono già
nel repository (`res/` per il binario, `dist/icons/` per il tema del desktop):
un clone compila e si impacchetta senza dipendenze grafiche. Il generatore
`dist/icon-gen.py` serve solo a rifarle, e richiede Pillow:

```bash
python3 dist/icon-gen.py dist/whats-open.png
```

## Integrazione con il desktop

`install.sh` copia binario, voce di menu e icone in tutte le misure previste dal
tema (`16`, `24`, `32`, `48`, `64`, `128`, `256`, `512`). Se `update-desktop-database` o `gtk-update-icon-cache` sono presenti le
cache vengono aggiornate subito; altrimenti il desktop le rigenera al prossimo
accesso. Il file `.desktop` dichiara `StartupWMClass=whats-open`, così la
finestra viene associata all'icona anche su GNOME e KDE.

## Licenza

MIT — vedi [LICENSE](LICENSE).

Whats-Open non è affiliato a WhatsApp LLC né a Meta.

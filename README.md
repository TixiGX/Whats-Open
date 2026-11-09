# Whats-Open

Client WhatsApp nativo per Linux: interfaccia essenziale con i verdi di
WhatsApp, protocollo in puro Rust, nessuna WebView e nessun runtime JavaScript.

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
- **Tema nativo**: legge i colori del tema GTK in uso (anche "MacOS Tahoe",
  WhiteSur, Adwaita, Breeze…) e ne segue il chiaroscuro, tenendo i verdi e i
  grigi di WhatsApp (avatar colorati per contatto, bolle inviate verdi)
- **Carattere del desktop**: l'interfaccia usa il font che il sistema già
  impiega per le altre finestre (`font-name` di GNOME, `gtk-font-name` o
  `kdeglobals`), con i file di sistema come riserva per emoji e alfabeti non
  latini: nessun font incorporato da mostrare, nessun download
- **Notifiche di sistema** per messaggi e chiamate, con clic che riporta la
  finestra davanti: D-Bus diretto, nessun `notify-send`, nessun processo extra
- Log su file (con rotazione) e diagnostica integrata

Non ci sono storie, canali né videochiamate: l'obiettivo è un client leggero,
non una copia di WhatsApp Web.

## Quanto pesa

- **Un binario solo**: 16,2 MB statico (musl) o 15,8 MB (glibc), senza WebView
  né toolkit grafici al seguito. Il pacchetto completo, con icone, file
  `.desktop` e installer, è 31 MB.
- **Fermo**: 20–30 ms di CPU ogni 4 secondi (circa lo 0,5% di un core) e
  ~120 MB residenti nel collaudo su Xvfb, dove il disegno passa dal
  rasterizzatore software; con una GPU vera la memoria scende.
- **Niente ridisegni a vuoto**: la finestra si ridisegna sugli eventi, con un
  risveglio di sicurezza ogni 3 secondi; le raffiche del motore (ricevute,
  "sta scrivendo", presenza) vengono accorpate in un solo fotogramma.
- Il tema nativo si segue con qualche `stat` al secondo: il CSS del tema viene
  riletto solo quando cambia davvero.
- I font sono quelli installati nel sistema: il programma non porta con sé un
  carattere proprio. Se in una macchina non ce n'è nessuno, entra in gioco il
  carattere di riserva compilato nel binario.

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
| servizio di notifiche (`org.freedesktop.Notifications`) | avvisi di sistema | nessun avviso, il resto funziona |
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
whats-open --audio-check      # microfono, casse o silenzio di riserva
whats-open --video-check      # percorso H.264 delle videochiamate
whats-open --notify-check     # manda una notifica di prova al desktop
whats-open --selftest         # collaudo di tutte le funzioni senza account
whats-open --theme chiaro     # forza il tema (auto | chiaro | scuro)
whats-open --help
```

Il tema segue il desktop **e ne usa i colori**. Si legge il nome del tema
(`GTK_THEME`, `gsettings`, `gtk-3.0|4.0/settings.ini`, `kdeglobals`), si apre
il suo `gtk.css` (o `gtk-dark.css`) e si prendono le `@define-color`
`theme_bg_color`, `theme_base_color`, `theme_fg_color`,
`theme_selected_bg_color`. Da lì nasce la tavolozza: lo sfondo, i pannelli e
il testo prendono la luminosità del tema (un tema macOS avrà i suoi grigi,
Adwaita i suoi), mentre i verdi di WhatsApp — bolla inviata, pastiglie dei
non letti, pulsante di invio — restano riconoscibili. Gli avatar dei contatti
sono disegnati con una tinta stabile per indirizzo, come sul telefono; gli
angoli del fumetto sono smussati dal lato opposto al mittente. Il testo ha
contrasto garantito almeno 7:1.

Il tema viene **seguito anche mentre l'app è aperta**: se il desktop passa a
chiaro/scuro o cambia tema, la finestra si aggiorna entro pochi secondi. Il
controllo costa qualche `stat` al secondo; il CSS viene riletto solo quando
qualcosa è davvero cambiato, quindi non c'è nessun processo in più e la CPU
resta ferma.

Con `--theme chiaro|scuro` si forza la modalità: se il tema non ha la
variante corrispondente si usa una delle due tavolozze incorporate, invece di
mostrare colori sbagliati. `whats-open --doctor` dice quale tema e quali
colori sono in uso.

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
  "system_fonts": true,     // carattere preso dal desktop, non incorporato
  "graceful_exit": true,    // chiude la sessione in modo pulito all'uscita
  "camera": "/dev/video0",  // fotocamera per le videochiamate
  "theme": "auto",          // auto | light | dark
  "notifications": true     // notifiche di sistema per messaggi e chiamate
}
```

### Notifiche di sistema

Le notifiche non passano da `notify-send` né da una libreria esterna: il
client apre il bus di sessione e chiama `org.freedesktop.Notifications.Notify`
con un messaggio D-Bus costruito a mano. Il thread delle notifiche resta in
ascolto: cliccare la notifica riporta davanti la finestra.

- Un messaggio produce una notifica solo se la finestra è in secondo piano o
  la chat non è quella aperta; al massimo una ogni due secondi per chat.
- Una chiamata in arrivo usa l'urgenza critica.
- Si spengono da Impostazioni (o con `"notifications": false`).
- Se il desktop non ha un servizio di notifiche non succede nulla: nessun
  errore, nessun processo avviato. `whats-open --notify-check` dice se il
  servizio c'è e risponde.

### Collaudo senza account

`whats-open --selftest` (alias `--collaudo`) esercita il binario vero su tutto
ciò che non richiede il telefono: percorsi e cache confinati, archivio (11 tipi
di messaggio, stati che non tornano indietro, modifica/revoca/reazione,
contenuti "una volta sola", invii appesi e riprova, alias LID, fusione dei
contatti), traduzione dei protobuf, miniature e video, configurazione, tema
GTK/KDE e tema nativo (legge un finto MacOS Tahoe e verifica la tavolozza),
rubrica vCard, nomi e numeri, contrasto delle due tavolozze, notifiche,
canale audio e percorso video delle chiamate. Ogni riga è `OK` o `FAIL` e l'uscita è
1 se qualcosa non torna: è lo stesso controllo che gira nella CI.

```bash
$ whats-open --selftest
Whats-Open 0.1.0 — collaudo delle funzioni interne
OK   percorsi                   cartelle create, percorsi dei media confinati
OK   archivio                   11 tipi di messaggio, stati, modifica/revoca/reazione, una-volta, invii appesi, nomi, alias, contatti, fusione LID
OK   estrazione messaggi        testo, foto (una volta), GIF, vocale, documento, sticker, posizione, citazione, contenitori, sconosciuto
OK   media                      immagine 640×360 con miniatura 480×270, video 2.0 s 320×180 con fotogramma 320×180 (5 KB)
OK   configurazione             salvataggio, rilettura e file corrotto
OK   tema del desktop           gtk-theme-name, gtk-application-prefer-dark-theme, GTK_THEME, kdeglobals
OK   tema nativo                MacOS-Tahoe: gtk.css e gtk-dark.css letti, grigi del tema e contrasto ≥ 7:1
OK   leggibilità                scuro/testo 18.2:1, scuro/fumetto mio 17.7:1, scuro/fumetto ricevuto 15.3:1, chiaro/testo 16.6:1, chiaro/fumetto mio 15.1:1, chiaro/fumetto ricevuto 17.7:1
OK   rubrica                    2 contatti, FN e N, numeri con + e senza
OK   nomi e numeri              nomi, iniziali, numeri, gruppi, canali, stati, pesi, durate, id
OK   canale audio               microfono: Default Audio Device · casse: Default Audio Device · 2 frame da 60 ms pronti
OK   notifiche di sistema       notifica consegnata al servizio del desktop
OK   percorso video             15 access unit H.264, 15 fotogrammi decodificati a 320×180

13 controlli superati, 0 falliti su 13
```

### Variabili d'ambiente

| Variabile | Effetto |
|---|---|
| `WHATS_OPEN_NO_SYSTEM_FONTS=1` | usa solo il carattere incorporato di riserva (equivale a spegnere l'interruttore nelle impostazioni) |
| `WHATS_OPEN_FONT="Inter 11"` | forza la famiglia del desktop da usare (utile per provarla) |
| `WINIT_UNIX_BACKEND=x11` o `wayland` | forza il backend grafico |
| `LIBGL_ALWAYS_SOFTWARE=1` | rendering software (schede senza driver OpenGL) |
| `RUST_LOG=debug` | log più verboso |
| `WHATS_OPEN_DEMO=1` | apre l'interfaccia con conversazioni di prova, senza account e senza rete (l'invio è disattivato) |
| `WHATS_OPEN_THEME=light|dark` | forza il tema chiaro o scuro |
| `WHATS_OPEN_WINDOW=560x430` | dimensione iniziale della finestra |

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
6. **La chiamata non parte**: il messaggio nel fumetto in basso dice il
   motivo. I due casi più comuni sono «nessun dispositivo raggiungibile» e
   «chiamate non ancora autorizzate»: in entrambi basta mandare *prima* un
   messaggio al contatto (crea la sessione cifrata) e richiamare. Se il
   microfono o le casse mancano la chiamata parte lo stesso, con un avviso:
   si sente o si parla da una sola parte invece di fallire.
7. **Selettore file che non risponde**: installa `xdg-desktop-portal` e
   `zenity`, oppure usa **Allega → Da percorso…**.
8. **Testo in arabo, cinese o emoji quadretti**: installa i font Noto della tua
   lingua (`fonts-noto`, `noto-fonts`, `noto-fonts-cjk`); l'app li carica
   automaticamente come riserva, insieme al carattere dell'interfaccia.
   **L'interfaccia non somiglia al resto della scrivania**: il carattere si
   legge da `font-name` (GNOME), `gtk-font-name` (GTK) o `kdeglobals` (KDE);
   con `--doctor` la riga `font interfaccia` dice quale file è stato scelto.
9. **`error: could not compile ... (signal: 9, SIGKILL)`**: è il kernel che
   uccide `rustc` per mancanza di RAM. Compila con `cargo build --release -j 2`
   (o `-j 1`), chiudi le altre applicazioni oppure usa gli archivi precompilati.
10. **`install.sh: comando non trovato`**: lo script va lanciato dalla radice del
   repository (`./packaging/install.sh`) oppure dalla cartella dell'archivio
   estratto (`./install.sh`), non da dentro `packaging/`.
11. **Sessione danneggiata**: dalla schermata di errore usa **Cancella la
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
- `src/theme.rs`, `src/icons.rs` — tavolozza (tema del desktop + verdi WhatsApp) e icone disegnate a mano
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

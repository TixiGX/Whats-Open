# Collaudo completo di Whats-Open

Questo file è pensato per un'AI con accesso al terminale (e, dove serve, alla
scrivania grafica). Ogni prova ha un **ID** (`T1`, `T2`, …): il risultato va
segnato in [`report-TEST.md`](report-TEST.md), che è già pronto con tutte le
righe. Regola unica: **non inventare**. Se un passo non si può eseguire, si
scrive `NON ESEGUITO` con il motivo (niente account, niente Wayland, niente
telefono…), non `OK`.

Le prove sono in ordine di difficoltà: prima quelle che non richiedono un
account (e sono già coperte dalla CI), poi quelle che richiedono il telefono.

---

## 0. Preparazione

Serve una macchina Linux con sessione grafica (X11 o Wayland), `bash`, e
almeno uno fra `grim`, `gnome-screenshot`, `spectacle` o `import` per gli
screenshot.

```bash
# dalla radice del repository
cd ~/Whats-Open

# cartella per le prove (fuori dal repository, così non sporca il lavoro)
mkdir -p ~/collaudo-whats-open/{screenshot,log}
cd ~/collaudo-whats-open

# compilazione (se non c'è già un binario)
cd ~/Whats-Open
cargo build --release
BIN=~/Whats-Open/target/release/whats-open
"$BIN" --version
```

> Se il binario arriva da un archivio (`whats-open-linux-x86_64-glibc.tar.gz`
> o `-musl`), basta estrarlo e usare `./whats-open`. Il resto del documento
> usa `$BIN` per entrambi i casi.

Annota in `log/ambiente.txt`:

```bash
{
  echo "data: $(date -Is)"
  echo "commit: $(git -C ~/Whats-Open rev-parse --short HEAD)"
  echo "versione: $("$BIN" --version)"
  echo "distro: $(. /etc/os-release && echo "$PRETTY_NAME")"
  echo "sessione: ${XDG_SESSION_TYPE:-?} · ${XDG_CURRENT_DESKTOP:-?}"
  echo "schermo: $(command -v xrandr >/dev/null && xrandr | grep ' connected primary' || echo '?')"
  glxinfo -B 2>/dev/null | grep -E "OpenGL renderer|OpenGL version" || true
} > ~/collaudo-whats-open/log/ambiente.txt
```

---

## 1. Comandi da terminale (senza account)

### T1 — `--doctor`: l'ambiente è completo

```bash
"$BIN" --doctor | tee ~/collaudo-whats-open/log/doctor.txt
```

Deve elencare, senza errori: distribuzione e versione, sessione grafica,
glibc, `ffmpeg`/`ffprobe`/`ffplay`, `xdg-open`, servizio notifiche, scheda
audio, tema, font e percorsi dei dati. Cerca in particolare:

* la riga **`font interfaccia:`** — deve dire una famiglia vera (es. `Noto
  Sans`) e un file `.ttf`/`.otf`, con l'origine (`gsettings`,
  `gtk-font-name`, `kdeglobals`, `primo font disponibile`…);
* la riga **`emoji e simboli:`** — deve dire `incorporate nel programma` o
  `dal sistema`, **mai** che non ci sono riserve utilizzabili;
* nessuna riga `manca` per le dipendenze che ti servono davvero.

### T2 — `--selftest`: le funzioni interne

```bash
"$BIN" --selftest | tee ~/collaudo-whats-open/log/selftest.txt
echo "uscita: $?"
```

Atteso: **13 controlli superati, 0 falliti**, uscita `0`, e le righe
`OK` per percorsi, archivio, estrazione messaggi, media, configurazione,
tema del desktop, tema nativo, leggibilità, rubrica, nomi e numeri, canale
audio, notifiche di sistema, percorso video. Una riga `FAIL` = prova fallita,
con il motivo accanto.

> Nel container della CI la riga del tema può usare i soli font DejaVu: è
> normale. Sul desktop la riga deve riflettere il font di GNOME/KDE.

### T3 — Controlli singoli di audio, video e notifiche

```bash
"$BIN" --audio-check   | tee ~/collaudo-whats-open/log/audio.txt
"$BIN" --video-check   | tee ~/collaudo-whats-open/log/video.txt
"$BIN" --notify-check  | tee ~/collaudo-whats-open/log/notify.txt
```

* audio: trova microfono e casse, prepara alcuni frame senza errori;
* video: decodifica alcuni fotogrammi H.264 (il percorso delle
  videochiamate) senza errori;
* notifiche: deve comparire **una notifica vera** sulla scrivania, con icona
  e testo leggibile. Se non compare, annota il messaggio d'errore.

---

## 2. Interfaccia senza account (demo)

La demo apre conversazioni finte, senza rete e senza toccare l'archivio.
Contiene di proposito: emoji (😀 🚀 🎉), un indirizzo lunghissimo senza spazi,
foto, vocale, sticker, documento, GIF e una ventina di messaggi (per provare
lo scorrimento).

```bash
cd ~/Whats-Open
WHATS_OPEN_DEMO=1 WHATS_OPEN_WINDOW=1100x720 "$BIN" &
sleep 3
```

Screenshot (scegli il comando che funziona sulla tua scrivania):

```bash
S=~/collaudo-whats-open/screenshot
# Wayland con GNOME:
gnome-screenshot -f "$S/01-demo-1100x720.png"
# Wayland wlroots (sway/hyprland):
grim "$S/01-demo-1100x720.png"
# X11:
import -window root "$S/01-demo-1100x720.png"
```

### T4 — Emoji e caratteri speciali visibili

Nella chat **Anna Ferrari**, i primi messaggi contengono 😀 e 🚀; l'ultimo del
gruppo contiene 🎉. Guarda lo screenshot (ingrandisci 2–4× con
`convert img.png -crop … -resize 300% out.png` se serve):

* le emoji devono essere **disegnate** (anche in bianco e nero/contorno: il
  carattere a colori di Debian non è disegnabile da egui, quindi si usano
  quelli incorporati);
* **nessun quadratino** vuoto o rettangolo con dentro un punto interrogativo;
* in **Impostazioni → Aspetto**, passando il mouse su *Carattere di sistema*,
  il suggerimento deve contenere `emoji: incorporate nel programma` o
  `emoji: dal sistema`.

Prova anche un carattere speciale qualsiasi (→, ≥, ½, “virgolette”, un
ideogramma se hai `fonts-noto-cjk`): deve comparire, non sparire.

### T5 — Impaginazione dei fumetti

Nella stessa chat:

* l'indirizzo lungo (`https://example.com/documenti/…`) va **a capo dentro il
  fumetto**, non esce dalla finestra né crea una barra di scorrimento
  orizzontale;
* i fumetti non superano circa i tre quarti della larghezza della
  conversazione e non sono più larghi del necessario;
* l'ora e le spunte restano allineate in basso a destra, anche sui messaggi
  su più righe;
* i nomi lunghi nella lista a sinistra sono troncati con `…`, non spezzati;
* il messaggio citato (risposta) resta dentro il fumetto.

### T6 — Scorrimento con inerzia e pulsante "vai in fondo"

Nella chat di Anna (molti messaggi, la finestra non li contiene tutti):

1. Con la rotella scorri verso l'alto: il contenuto deve **scivolare** per
   qualche decimo di secondo e fermarsi morbido, non saltare di un dentello
   alla volta. Vale anche sul touchpad.
2. Mentre sei in alto deve comparire in basso a destra, sopra il compositore,
   un **pulsante tondo con la freccia in giù**; se ci sono messaggi non letti
   in basso mostra anche il conteggio.
3. Premi il pulsante: si torna in fondo e il pulsante sparisce.
4. Scorri in alto, poi aspetta: la chat **non deve tornare in fondo da sola**
   quando arriva un messaggio nuovo (nella demo non arrivano, ma la posizione
   deve restare ferma).
5. Il fondo della chat deve essere raggiungibile sempre, anche con la
   finestra molto bassa (prova `WHATS_OPEN_WINDOW=560x430`).

Per fotografare subito il pulsante senza scorrere a mano, la demo può partire
già a metà storico:

```bash
WHATS_OPEN_DEMO=1 WHATS_OPEN_DEMO_SCROLL=1 "$BIN" &
```

Screenshot: `04b-vai-in-fondo.png` (pulsante con il conteggio dei messaggi
altrui rimasti sotto).

Per registrare il movimento: `wf-recorder -f scroll.mp4` (Wayland) oppure
`ffmpeg -f x11grab -i :0 -t 5 scroll.mp4` (X11); in alternativa descrivi a
parole ciò che vedi.

### T7 — Aspetto moderno (confronto con la foto di Nautilus)

Con la demo aperta, confronta con l'immagine di riferimento (Nautilus):

* la barra laterale e la conversazione sono **schede staccate dal bordo**,
  con angoli arrotondati (≈16 px) e un margine uniforme;
* la riga della chat scelta è una **pillola** con il verde d'accento, non un
  rettangolo a tutta larghezza;
* i pulsanti della testata (ingranaggio, più, telefono, video) sono
  **cerchi** con un disco appena accennato, non icone nude;
* le barre di scorrimento sono **sottili e flottanti** (compaiono al passaggio
  del mouse), non binari grigi fissi;
* intestazione e compositore hanno gli angoli superiori/inferiori coerenti
  con la scheda, e le linee di separazione sono rientrate;
* i grigi sono a gradini netti (fondo, pannello, superficie), il testo è
  leggibile anche a finestra non massimizzata.

Screenshot: `02-demo-scheda.png`, `03-demo-pillola.png` (con una chat
selezionata), `04-impostazioni.png` (con `WHATS_OPEN_DEMO_SETTINGS=1`).

### T8 — Finestra piccola e leggibile

```bash
pkill -x whats-open 2>/dev/null || pkill -n -f whats-open
WHATS_OPEN_DEMO=1 WHATS_OPEN_WINDOW=560x430 "$BIN" &
sleep 3
```

* la finestra resta usabile: elenco e conversazione non si sovrappongono;
* il testo dei messaggi resta leggibile (niente testo tagliato a metà);
* il compositore, il pulsante invio e il pulsante "vai in fondo" restano
  dentro la finestra;
* aprendo e chiudendo le impostazioni nulla esce dallo schermo (le
  impostazioni scorrono).

Screenshot: `05-560x430.png`.

### T9 — Tema chiaro, scuro e "Automatico"

```bash
WHATS_OPEN_DEMO=1 WHATS_OPEN_THEME=light "$BIN" &   # chiaro
WHATS_OPEN_DEMO=1 WHATS_OPEN_THEME=dark  "$BIN" &   # scuro
```

* in chiaro: fondo `#efeae2`, pannelli bianchi, fumetto mio verde chiaro;
* in scuro: fondo `#0b141a`, pannelli `#111b21`, fumetto mio `#005c4b`;
* in **Impostazioni → Aspetto** i tre chip *Automatico / Chiaro / Scuro*
  cambiano subito l'aspetto, senza riavviare;
* con *Automatico*, cambiando tema a GNOME (Impostazioni → Aspetto) e
  riaprendo l'app, i colori seguono il desktop.

Screenshot: `06-chiaro.png`, `07-scuro.png`.

### T10 — Carattere di sistema davvero usato

```bash
WHATS_OPEN_NO_SYSTEM_FONTS=1 WHATS_OPEN_DEMO=1 "$BIN" &   # solo incorporato
WHATS_OPEN_FONT="DejaVu Sans 11" WHATS_OPEN_DEMO=1 "$BIN" &  # forzato
```

* con `WHATS_OPEN_NO_SYSTEM_FONTS=1` l'interfaccia cambia carattere (usa
  quello incorporato) e **non** compare il carattere di sistema;
* con `WHATS_OPEN_FONT` il carattere indicato viene davvero usato;
* `--doctor` e le impostazioni riportano la stessa famiglia/file.

Screenshot: `08-font-forzato.png`.

---

## 3. Consumo di memoria e CPU

Misura a finestra aperta, **senza** interagire, dopo che l'app è stata ferma
almeno un minuto (la prima schermata può fare lavoro in più).

```bash
pid=$(pgrep -n -f "target/release/whats-open|/whats-open$")
echo "pid=$pid"
grep -E "VmRSS|VmHWM" /proc/$pid/status | tee ~/collaudo-whats-open/log/rss.txt
ps -o pid,pcpu,rss,vsz,etime -p "$pid" | tee ~/collaudo-whats-open/log/ps.txt

# CPU su 10 secondi (percentuale media, non istantanea)
t1=$(awk '{print $14+$15}' /proc/$pid/stat); sleep 10
t2=$(awk '{print $14+$15}' /proc/$pid/stat)
echo "CPU media: $(( (t2 - t1) * 100 / 100 / 10 ))%"   # ticks = 100/s
```

Ripeti la misura:

* **T11** — demo, finestra 1100×720, ferma: RSS atteso **< 180 MB**,
  CPU media **< 2%**;
* **T12** — dopo aver aperto 5 chat e scorso su e giù per 30 s: RSS
  **non deve crescere di più di ~30 MB** rispetto a T11 (le miniature hanno
  un tetto), e deve restare stabile tornando alla prima chat;
* **T13** — account reale con una chat con foto (vedi fase 4): RSS atteso
  **< 260 MB**, CPU a riposo **< 2%**.

Annota i valori **esatti** nel report, non solo OK/FAIL.

---

## 4. Account vero (serve il telefono)

### T14 — Accesso con QR

```bash
"$BIN" 2>&1 | tee ~/collaudo-whats-open/log/avvio.txt
```

* il QR compare entro pochi secondi ed è **scansionabile** (schermo a
  100–150% di zoom, finestra non troppo piccola);
* dal telefono: WhatsApp → Dispositivi collegati → Collega un dispositivo;
* dopo la scansione l'elenco delle chat si popola e l'app resta collegata
  anche chiudendola e riaprendola (niente nuovo QR).

Screenshot del QR: `09-qr.png` (prima di scansionarlo).

### T15 — Accesso con codice di accoppiamento (alternativa al QR)

```bash
"$BIN" --pair +39XXXXXXXXXX
```

* compare una finestra con il codice a 8 caratteri;
* dal telefono: Dispositivi collegati → **Collega con numero di telefono**;
* il codice viene accettato e l'app entra; il codice scade dopo qualche
  minuto (se scade, si rigenera).

### T16 — Nomi veri, non numeri

* nella lista compaiono i nomi della rubrica del telefono, non `3933…`;
* per i contatti senza nome compare il numero formattato (`+39 333 …`);
* aprendo la chat, la testata mostra lo stesso nome;
* i gruppi mostrano il nome del gruppo e il mittente sopra il fumetto.

Screenshot: `10-elenco-nomi.png`.

### T17 — Invio e ricezione di un messaggio

* scrivi un messaggio con emoji e un link lungo: parte, compare il fumetto
  verde a destra, spunta singola → doppia → doppia blu;
* sul telefono arriva identico, emoji comprese;
* un messaggio inviato dal telefono compare nell'app entro pochi secondi;
* premi Invio: invia; Shift+Invio: va a capo; la casella si svuota.

Screenshot: `11-invio.png`.

### T18 — Foto e media

* una foto ricevuta mostra la miniatura, si apre a schermo intero e si chiude
  con Esc o con la X;
* una foto inviata dall'app (Allega → file, oppure trascinamento) arriva sul
  telefono;
* un vocale ricevuto si ascolta; un vocale registrato dall'app arriva e si
  ascolta sul telefono;
* sticker e GIF si vedono (lo sticker senza fumetto, la GIF animata);
* un documento mostra nome, peso e si apre/salva.

Screenshot: `12-foto.png`, `13-sticker-gif.png`.

### T19 — "Una volta sola" (view-once)

* un messaggio *una volta sola* ricevuto mostra la piastrella "apri una
  volta"; aprendolo si vede il contenuto una sola volta e poi la piastrella
  resta chiusa (riaprendo non si vede più);
* l'app non salva il contenuto nella cache in chiaro;
* un view-once **inviato** dall'app risulta aperto sul telefono una volta.

Screenshot: `14-view-once.png` (prima e dopo l'apertura).

### T20 — Notifiche di sistema

Con l'app in secondo piano o ridotta a icona, manda un messaggio dal
telefono:

* compare la notifica di GNOME con nome, testo e icona;
* cliccandola si apre la chat giusta;
* non arrivano notifiche doppie;
* con l'app in primo piano non arriva la notifica (o arriva come da
  impostazione) e il messaggio è già nella chat.

### T21 — Chiamata vocale

Dalla testata della chat premi il **telefono**:

* il telefono squilla e sull'app compare il pannello di chiamata;
* rispondendo si sente e si parla in entrambe le direzioni;
* il microfono si può mutare e riattivare;
* riagganciando da una parte, l'altra si chiude pulita;
* **la chiamata compare nel registro chiamate del telefono** (WhatsApp →
  Chiamate), con la voce "in uscita"/"in entrata" e la durata;
* nessun errore nel fumetto in basso durante la chiamata.

Se compare «nessun dispositivo raggiungibile» o «chiamate non ancora
autorizzate»: manda **prima** un messaggio al contatto e richiama (crea la
sessione cifrata). Se persiste, è un FAIL con il testo esatto.

Screenshot: `15-chiamata-voce.png`.

### T22 — Videochiamata

Premi il pulsante **video**:

* parte la videochiamata, si vede il proprio riquadro e quello remoto;
* audio e video funzionano in entrambe le direzioni;
* la chiamata compare nel registro del telefono;
* riagganciando si torna alla chat senza errori.

Screenshot: `16-videochiamata.png`.

### T23 — Mai "online" e mai "sta scrivendo"

Con l'app aperta e una chat selezionata, guarda lo stesso contatto da un
secondo telefono (o chiedi a qualcuno):

* il contatto **non deve vedere "online"** per colpa dell'app;
* non deve vedere **"sta scrivendo…"** mentre scrivi nell'app;
* le spunte dei tuoi messaggi restano corrette.

> Nota: se il telefono è acceso con WhatsApp aperto, è **lui** a dire
> "online". Per la prova chiudi WhatsApp sul telefono (o mettilo in modalità
> aereo con l'app collegata) e verifica che l'app non lo annunci.

### T24 — Robusto: rete che va e viene

* metti il computer offline per 20 s: l'app mostra "connessione…" e non si
  chiude;
* torna online: si ricollega da sola e i messaggi arretrati arrivano;
* sospendi e riprendi il computer: nessun crash, nessun QR nuovo.

---

## 5. Raccolta per il report

Alla fine servono, dentro `~/collaudo-whats-open/`:

```
log/ambiente.txt      log/doctor.txt      log/selftest.txt
log/audio.txt         log/video.txt       log/notify.txt
log/rss.txt           log/ps.txt          log/avvio.txt
screenshot/01…16      (quelli che sei riuscito a fare)
```

e il file **`report-TEST.md`** compilato (è già pronto accanto a questo
documento). Per ogni prova: `OK`, `FAIL` o `NON ESEGUITO`, una riga di
evidenza (comando, output, nome screenshot) e, se FAIL, il messaggio
d'errore **copiato per intero**. Non serve altro.

Se qualcosa fallisce, allega anche:

```bash
tail -n 200 ~/.cache/whats-open/whats-open.log > ~/collaudo-whats-open/log/app.log
"$BIN" --doctor > ~/collaudo-whats-open/log/doctor-dopo.txt
```

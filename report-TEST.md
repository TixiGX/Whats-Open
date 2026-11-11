# Report del collaudo di Whats-Open

Compilare **dopo** aver eseguito i passi di [`TEST-COMPLETO.md`](TEST-COMPLETO.md).
Per ogni riga sostituire `⬜` con:

* `OK` — prova eseguita e risultato conforme;
* `FAIL` — prova eseguita, risultato **non** conforme (riportare il messaggio
  d'errore per intero nella colonna *Evidenza*);
* `NON ESEGUITO` — prova non eseguibile (manca account/telefono/sessione
  grafica): scrivere il motivo.

Non segnare `OK` una prova che non è stata eseguita davvero.

---

## Dati della macchina

| Voce | Valore |
|---|---|
| Data e ora del collaudo | |
| Commit (`git rev-parse --short HEAD`) | |
| Versione (`whats-open --version`) | |
| Distribuzione e versione | |
| Sessione grafica (X11/Wayland, GNOME/KDE/…) | |
| Tema del desktop (nome, chiaro/scuro) | |
| Font del desktop (`gsettings get org.gnome.desktop.interface font-name`) | |
| Scheda video / driver OpenGL | |
| Telefono usato (modello, Android/iOS) | |
| Provenienza del binario (compilato / archivio glibc / archivio musl) | |

---

## Riepilogo

| Gruppo | Prove | OK | FAIL | Non eseguiti |
|---|---|---|---|---|
| Ambiente e collaudo interno (T1–T3) | 3 | | | |
| Interfaccia senza account (T4–T10) | 7 | | | |
| Memoria e CPU (T11–T13) | 3 | | | |
| Account e funzioni vere (T14–T24) | 11 | | | |
| **Totale** | **24** | | | |

Esito complessivo: `⬜ da compilare`

---

## Prove

### Ambiente e collaudo interno

| ID | Prova | Atteso | Esito | Evidenza / note |
|---|---|---|---|---|
| T1 | `--doctor` | ambiente completo, font di sistema e riga emoji sensata, nessuna dipendenza mancante | ⬜ | |
| T2 | `--selftest` | 13 controlli superati, 0 falliti, uscita 0 | ⬜ | |
| T3 | `--audio-check`, `--video-check`, `--notify-check` | audio e video senza errori; notifica vera sulla scrivania | ⬜ | |

### Interfaccia senza account (demo)

| ID | Prova | Atteso | Esito | Evidenza / note |
|---|---|---|---|---|
| T4 | Emoji e simboli | 😀 🚀 🎉 disegnate, nessun quadratino; diagnostica dice la fonte delle emoji | ⬜ | |
| T5 | Impaginazione fumetti | URL lungo va a capo dentro il fumetto, orari allineati, nomi troncati con `…` | ⬜ | |
| T6 | Scorrimento e "vai in fondo" | scorrimento morbido con inerzia; pulsante tondo in basso a destra con conteggio; la chat non torna in fondo da sola | ⬜ | |
| T7 | Aspetto moderno | schede staccate e arrotondate, riga scelta a pillola verde, pulsanti tondi, barre sottili | ⬜ | |
| T8 | Finestra 560×430 | tutto leggibile e dentro la finestra, impostazioni che scorrono | ⬜ | |
| T9 | Tema chiaro/scuro/automatico | colori corretti, cambio immediato, "Automatico" segue GNOME | ⬜ | |
| T10 | Font di sistema | font del desktop usato davvero; `WHATS_OPEN_FONT` e `WHATS_OPEN_NO_SYSTEM_FONTS` funzionano | ⬜ | |

### Memoria e CPU

| ID | Prova | Atteso | Esito | Evidenza / note |
|---|---|---|---|---|
| T11 | Demo ferma | RSS < 180 MB, CPU media < 2% | ⬜ | RSS: … kB · CPU: … % |
| T12 | Dopo 5 chat e 30 s di scorrimento | crescita ≤ ~30 MB e stabile tornando indietro | ⬜ | RSS dopo: … kB |
| T13 | Account reale, chat con foto | RSS < 260 MB, CPU a riposo < 2% | ⬜ | RSS: … kB · CPU: … % |

### Account e funzioni vere

| ID | Prova | Atteso | Esito | Evidenza / note |
|---|---|---|---|---|
| T14 | Accesso con QR | QR leggibile, scansione riuscita, sessione mantenuta al riavvio | ⬜ | |
| T15 | Accesso con codice | codice a 8 caratteri accettato dal telefono | ⬜ | |
| T16 | Nomi veri | nomi in rubrica, numeri formattati per gli sconosciuti, gruppi con mittente | ⬜ | |
| T17 | Invio e ricezione | messaggio con emoji e link identico sul telefono; spunte corrette; arrivo dal telefono | ⬜ | |
| T18 | Foto e media | miniatura e visore, invio foto, vocali, sticker, GIF, documenti | ⬜ | |
| T19 | View-once | si vede una sola volta, poi resta chiuso; nessun salvataggio in chiaro | ⬜ | |
| T20 | Notifiche di sistema | notifica GNOME con nome e testo, click apre la chat giusta, niente doppioni | ⬜ | |
| T21 | Chiamata vocale | audio bidirezionale, muto, chiusura pulita, **presente nel registro del telefono** | ⬜ | |
| T22 | Videochiamata | video e audio bidirezionali, chiusura pulita, **presente nel registro del telefono** | ⬜ | |
| T23 | Nessun "online"/"sta scrivendo" | il contatto non vede presenza né digitazione provocate dall'app | ⬜ | |
| T24 | Rete che va e viene | riconnessione automatica, nessun crash, nessun QR nuovo | ⬜ | |

---

## Problemi trovati

Per ogni `FAIL`, in ordine di gravità (prima ciò che impedisce l'uso):

| # | Prova | Cosa succede | Messaggio d'errore (integrale) | Riproducibile sempre? |
|---|---|---|---|---|
| 1 | | | | |
| 2 | | | | |
| 3 | | | | |

---

## File allegati

- [ ] `log/doctor.txt`
- [ ] `log/selftest.txt`
- [ ] `log/audio.txt`, `log/video.txt`, `log/notify.txt`
- [ ] `log/rss.txt`, `log/ps.txt`
- [ ] `log/app.log` (se c'è stato un errore)
- [ ] screenshot `01`…`16` (elenco: …)

## Note libere

(cosa ha funzionato meglio, cosa sembra ancora lento, differenze rispetto
all'aspetto atteso, altre osservazioni)

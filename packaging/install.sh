#!/bin/sh
# Installa Whats-Open su qualsiasi distribuzione Linux.
#
# Funziona sia dall'archivio già impacchettato sia dal repository appena
# compilato con `cargo build --release`.
#
#   sudo ./install.sh                 # in /usr/local
#   sudo ./install.sh --prefix=/usr   # pacchettizzato in /usr (distro)
#   ./install.sh --user               # solo per l'utente corrente (~/.local)
#   sudo ./install.sh --uninstall     # rimozione
#
# Opzioni:
#   --prefix=DIR   prefisso di installazione (default /usr/local)
#   --user         installa in $HOME/.local (non serve root)
#   --uninstall    rimuove i file installati
#   --destdir=DIR  radice alternativa (per creare pacchetti)
#   --no-deps      salta il controllo delle dipendenze di runtime

set -eu

APP=whats-open
PREFIX=/usr/local
DESTDIR=
MODE=install
USER_INSTALL=0
CHECK_DEPS=1

for arg in "$@"; do
    case "$arg" in
        --prefix=*)  PREFIX=${arg#*=} ;;
        --destdir=*) DESTDIR=${arg#*=} ;;
        --user)      USER_INSTALL=1 ;;
        --uninstall) MODE=uninstall ;;
        --no-deps)   CHECK_DEPS=0 ;;
        -h|--help)
            sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'
            exit 0 ;;
        *)
            echo "Opzione sconosciuta: $arg" >&2
            exit 2 ;;
    esac
done

if [ "$USER_INSTALL" -eq 1 ]; then
    PREFIX="${HOME}/.local"
fi

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

die() { echo "errore: $*" >&2; exit 1; }
note() { echo "$*" >&2; }

install_file() {
    src=$1; dst=$2; mode=$3
    mkdir -p "$(dirname -- "$dst")"
    if command -v install >/dev/null 2>&1; then
        install -m "$mode" -- "$src" "$dst"
    else
        cp -- "$src" "$dst"
        chmod "$mode" "$dst"
    fi
}

refresh_caches() {
    base=$1
    if [ -x "$(command -v gtk-update-icon-cache 2>/dev/null || true)" ] &&
       [ -f "$base/share/icons/hicolor/index.theme" ]; then
        gtk-update-icon-cache -q -t -f "$base/share/icons/hicolor" 2>/dev/null || true
    fi
    if [ -x "$(command -v update-desktop-database 2>/dev/null || true)" ]; then
        update-desktop-database -q "$base/share/applications" 2>/dev/null || true
    fi
}

# --------------------------------------------------------------------------
# Da dove arrivano i file: archivio già pronto o repository compilato.
# --------------------------------------------------------------------------

# 1) archivio: ./bin/whats-open accanto a install.sh
# 2) repository: packaging/install.sh con target/release/whats-open
# 3) build ottimizzata con LTO accanto alla release (target/dist)
LAYOUT=none
if [ -f "$HERE/bin/$APP" ]; then
    LAYOUT=packaged
    ROOT=$HERE
elif [ -f "$HERE/../target/release/$APP" ]; then
    LAYOUT=source
    ROOT=$(CDPATH= cd -- "$HERE/.." && pwd)
elif [ -f "$HERE/../target/dist/$APP" ]; then
    LAYOUT=source-dist
    ROOT=$(CDPATH= cd -- "$HERE/.." && pwd)
fi

binary_path() {
    case "$LAYOUT" in
        packaged)    echo "$ROOT/bin/$APP" ;;
        source)      echo "$ROOT/target/release/$APP" ;;
        source-dist) echo "$ROOT/target/dist/$APP" ;;
        *)           echo "" ;;
    esac
}

desktop_path() {
    case "$LAYOUT" in
        packaged) echo "$ROOT/share/applications/$APP.desktop" ;;
        *)        echo "$ROOT/res/$APP.desktop" ;;
    esac
}

icon_path() {
    size=$1
    case "$LAYOUT" in
        packaged) echo "$ROOT/share/icons/hicolor/${size}x${size}/apps/$APP.png" ;;
        *)        echo "$ROOT/dist/icons/$APP-$size.png" ;;
    esac
}

# --------------------------------------------------------------------------

if [ "$MODE" = uninstall ]; then
    if [ "$DESTDIR" = "" ] && [ "$USER_INSTALL" -eq 0 ] && [ "$(id -u)" -ne 0 ]; then
        die "la rimozione da $PREFIX richiede root (oppure usa --user)"
    fi
    rm -f "$DESTDIR$PREFIX/bin/$APP"
    rm -f "$DESTDIR$PREFIX/share/applications/$APP.desktop"
    for size in 16 24 32 48 64 128 256 512; do
        rm -f "$DESTDIR$PREFIX/share/icons/hicolor/${size}x${size}/apps/$APP.png"
    done
    refresh_caches "$DESTDIR$PREFIX"
    echo "Whats-Open rimosso da $DESTDIR$PREFIX"
    echo "I dati personali restano in ~/.local/share/whats-open e ~/.cache/whats-open"
    exit 0
fi

if [ "$LAYOUT" = none ]; then
    echo "errore: binario non trovato." >&2
    echo "" >&2
    echo "  Se hai scaricato l'archivio, lancia ./install.sh dalla cartella" >&2
    echo "  dove si trovano bin/, share/ e questo script." >&2
    echo "" >&2
    echo "  Se stai compilando dal repository, esegui prima:" >&2
    echo "      cargo build --release" >&2
    echo "  e poi, dalla radice del repository:" >&2
    echo "      sudo ./packaging/install.sh --prefix=/usr" >&2
    echo "" >&2
    echo "  Cercato in: $HERE/bin/$APP e $HERE/../target/release/$APP" >&2
    exit 1
fi

BIN=$(binary_path)
DESKTOP=$(desktop_path)

if [ "$DESTDIR" = "" ] && [ "$USER_INSTALL" -eq 0 ] && [ "$(id -u)" -ne 0 ]; then
    die "l'installazione in $PREFIX richiede root; usa --user per ~/.local"
fi

# Dipendenze di runtime: solo avvisi, l'app funziona anche senza.
if [ "$CHECK_DEPS" -eq 1 ] && [ "$DESTDIR" = "" ]; then
    missing=""
    for lib in libasound.so.2 libX11.so.6 libGL.so.1; do
        if ! ldconfig -p 2>/dev/null | grep -q "$lib"; then
            missing="$missing $lib"
        fi
    done
    if [ -n "$missing" ]; then
        echo "attenzione: librerie non trovate:$missing"
        if command -v apt-get >/dev/null 2>&1; then
            echo "  Debian/Ubuntu:  sudo apt install libasound2 libx11-6 libgl1"
        elif command -v dnf >/dev/null 2>&1; then
            echo "  Fedora/RHEL:    sudo dnf install alsa-lib libX11 mesa-libGL"
        elif command -v pacman >/dev/null 2>&1; then
            echo "  Arch:           sudo pacman -S alsa-lib libx11 mesa"
        elif command -v zypper >/dev/null 2>&1; then
            echo "  openSUSE:       sudo zypper install libasound2 libX11-1 Mesa-libGL1"
        elif command -v apk >/dev/null 2>&1; then
            echo "  Alpine:         sudo apk add alsa-lib libx11 mesa-gl"
        fi
    fi
    if ! command -v ffmpeg >/dev/null 2>&1; then
        echo "nota: ffmpeg non è installato — audio e video useranno il player di sistema"
    fi
fi

install_file "$BIN" "$DESTDIR$PREFIX/bin/$APP" 755

if [ -f "$DESKTOP" ]; then
    # La voce di menu punta al percorso reale: funziona anche installando in
    # un prefisso che non è nel PATH (es. /opt/whats-open).
    menu=$(mktemp 2>/dev/null || echo "/tmp/$APP.desktop.$$")
    sed "s|^Exec=$APP|Exec=$PREFIX/bin/$APP|" "$DESKTOP" > "$menu"
    install_file "$menu" "$DESTDIR$PREFIX/share/applications/$APP.desktop" 644
    rm -f "$menu"
else
    note "nota: $APP.desktop non trovato, salto la voce di menu"
fi

icons=0
for size in 16 24 32 48 64 128 256 512; do
    icon=$(icon_path "$size")
    [ -f "$icon" ] || continue
    install_file "$icon" \
        "$DESTDIR$PREFIX/share/icons/hicolor/${size}x${size}/apps/$APP.png" 644
    icons=$((icons + 1))
done
[ "$icons" -gt 0 ] || note "nota: nessuna icona trovata, il desktop userà quella generica"

refresh_caches "$DESTDIR$PREFIX"

echo "Whats-Open installato in $DESTDIR$PREFIX"
case ":$PATH:" in
    *":$PREFIX/bin:"*) ;;
    *) echo "aggiungi $PREFIX/bin al PATH per lanciarlo con 'whats-open'" ;;
esac
echo "avvia: whats-open   (oppure: whats-open --pair <numero>)"

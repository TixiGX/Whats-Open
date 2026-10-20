#!/bin/sh
# Installa Whats-Open su qualsiasi distribuzione Linux.
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
            sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
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
BIN="$HERE/bin/$APP"
DESKTOP="$HERE/share/applications/$APP.desktop"
ICONS="$HERE/share/icons/hicolor"

die() { echo "errore: $*" >&2; exit 1; }

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
    if [ -x "$(command -v xdg-desktop-menu 2>/dev/null || true)" ]; then
        : # la copia diretta basta, xdg-desktop-menu richiederebbe root
    fi
}

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

[ -f "$BIN" ] || die "binario non trovato: $BIN"

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
        case "$(uname -s)" in
            Linux) ;;
        esac
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
    install_file "$DESKTOP" "$DESTDIR$PREFIX/share/applications/$APP.desktop" 644
fi
if [ -d "$ICONS" ]; then
    for size in 16 24 32 48 64 128 256 512; do
        icon="$ICONS/${size}x${size}/apps/$APP.png"
        [ -f "$icon" ] || continue
        install_file "$icon" \
            "$DESTDIR$PREFIX/share/icons/hicolor/${size}x${size}/apps/$APP.png" 644
    done
fi

refresh_caches "$DESTDIR$PREFIX"

echo "Whats-Open installato in $DESTDIR$PREFIX"
case ":$PATH:" in
    *":$PREFIX/bin:"*) ;;
    *) echo "aggiungi $PREFIX/bin al PATH per lanciarlo con 'whats-open'" ;;
esac
echo "avvia: whats-open   (oppure: whats-open --pair <numero>)"

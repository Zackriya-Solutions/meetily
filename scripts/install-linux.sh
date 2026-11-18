#!/bin/sh
# Meetily Linux installer.
#   curl -fsSL https://raw.githubusercontent.com/Zackriya-Solutions/meetily/main/scripts/install-linux.sh | sh
set -eu

REPO="Zackriya-Solutions/meetily"
SCRIPT_URL="https://raw.githubusercontent.com/$REPO/main/scripts/install-linux.sh"
APP_DIR="$HOME/.local/share/meetily"
APPIMAGE="$APP_DIR/meetily.AppImage"
DESKTOP_FILE="$HOME/.local/share/applications/meetily.desktop"
ICON="$HOME/.local/share/icons/hicolor/512x512/apps/meetily.png"
# User data written by the app itself (Tauri identifier com.meetily.ai); the recordings folder is the app's default.
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/com.meetily.ai"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/meetily"
DOCS_DIR=$(xdg-user-dir DOCUMENTS 2>/dev/null || true)
RECORDINGS_DIR="${DOCS_DIR:-$HOME/Documents}/meetily-recordings"

if [ -t 1 ]; then
  GREEN=$(printf '\033[32m'); YELLOW=$(printf '\033[33m'); RED=$(printf '\033[31m'); RESET=$(printf '\033[0m')
else
  GREEN=""; YELLOW=""; RED=""; RESET=""
fi
info()  { printf '%sinfo%s  %s\n' "$GREEN" "$RESET" "$*"; }
warn()  { printf '%swarn%s  %s\n' "$YELLOW" "$RESET" "$*"; }
error() { printf '%serror%s %s\n' "$RED" "$RESET" "$*" >&2; exit 1; }

usage() {
  cat <<EOF
Meetily Linux installer

Usage:
  curl -fsSL $SCRIPT_URL | sh
  curl -fsSL $SCRIPT_URL | sh -s -- [options]

Options:
  --version=X.Y.Z   Install a specific version (default: latest release)
  --deb             Install the .deb package (default on apt/dpkg systems)
  --appimage        Install the AppImage (default elsewhere)
  --yes, -y         Do not ask for confirmation
  --uninstall       Remove Meetily (AppImage install and/or .deb package). On a terminal it then offers to delete your data
  --purge           With --uninstall: also delete app data and settings (meetings database, models, logs, config)
  --purge-recordings  With --uninstall: like --purge, and also delete the default recordings folder
  --no-launch       Do not start Meetily after installing
  -h, --help        Show this help
EOF
}

VERSION=""; MODE=""; ASSUME_YES=0; UNINSTALL=0; NO_LAUNCH=0; PURGE=0; PURGE_REC=0
for arg in "$@"; do
  case "$arg" in
    --version=*) VERSION="${arg#--version=}"; VERSION="${VERSION#v}" ;;
    --deb) MODE=deb ;;
    --appimage) MODE=appimage ;;
    --yes|-y) ASSUME_YES=1 ;;
    --uninstall) UNINSTALL=1 ;;
    --purge) PURGE=1 ;;
    --purge-recordings) PURGE=1; PURGE_REC=1 ;;
    --no-launch) NO_LAUNCH=1 ;;
    -h|--help) usage; exit 0 ;;
    *) error "Unknown option: $arg (see --help)" ;;
  esac
done
if [ "$PURGE" = 1 ] && [ "$UNINSTALL" = 0 ]; then error "--purge and --purge-recordings need --uninstall"; fi

# Preflight
[ "$(uname -s)" = "Linux" ] || error "This installer is for Linux. Download Meetily for your OS from https://github.com/$REPO/releases/latest"
ARCH=$(uname -m)
case "$ARCH" in
  x86_64|amd64) ;;
  *) error "Meetily for Linux is only available for x86_64 (detected: $ARCH)" ;;
esac
GLIBC=$(getconf GNU_LIBC_VERSION 2>/dev/null || true)
[ -n "$GLIBC" ] || error "Meetily requires glibc; musl-based distributions (e.g. Alpine) are not supported"
GLIBC="${GLIBC#glibc }"
echo "$GLIBC" | awk -F. '{exit !($1>2 || ($1==2 && $2>=35))}' \
  || error "Meetily requires glibc 2.35 or newer (Ubuntu 22.04+, Debian 12+); found $GLIBC"
command -v curl >/dev/null 2>&1 || error "curl is required"
if command -v dpkg >/dev/null 2>&1 && command -v apt-get >/dev/null 2>&1; then DEB_OK=1; else DEB_OK=0; fi

# Delete user data: list every existing path with its size, then confirm on the terminal (unless --yes).
purge() {
  n=$#
  while [ "$n" -gt 0 ]; do
    p=$1; shift
    [ ! -e "$p" ] || set -- "$@" "$p"
    n=$((n - 1))
  done
  [ $# -gt 0 ] || { info "No Meetily data found"; return 0; }
  warn "This permanently deletes:"
  for p in "$@"; do
    case "$p" in
      "$DATA_DIR") what="meetings database (transcripts, summaries), settings, downloaded models, logs" ;;
      "$CONFIG_DIR") what="notification settings" ;;
      *) what="audio recordings and transcript files" ;;
    esac
    printf '  %s  (%s)\n    %s\n' "$p" "$(du -sh "$p" 2>/dev/null | cut -f1)" "$what"
  done
  if [ "$ASSUME_YES" = 0 ]; then
    printf 'Delete all of the above? [y/N] ' >/dev/tty
    read -r ans </dev/tty || ans=""
    case "$ans" in
      y|Y|yes|YES) ;;
      *) info "Kept your Meetily data"; return 0 ;;
    esac
  fi
  for p in "$@"; do rm -rf "$p"; done
  info "Deleted your Meetily data"
}

# A running Meetily keeps its files open and rewrites its data, so close it before removing anything.
stop_app() {
  command -v pgrep >/dev/null 2>&1 || return 0
  pgrep -x meetily >/dev/null 2>&1 || return 0
  warn "Meetily is running and must be closed first. Anything unsaved, including a recording in progress, is lost."
  if [ "$ASSUME_YES" = 0 ]; then
    [ "$CAN_ASK" = 1 ] || error "No terminal to confirm closing Meetily; close it yourself or re-run with --yes"
    printf 'Close Meetily now? [y/N] ' >/dev/tty
    read -r ans </dev/tty || ans=""
    case "$ans" in
      y|Y|yes|YES) ;;
      *) info "Cancelled. Close Meetily and re-run"; exit 0 ;;
    esac
  fi
  pkill -x meetily 2>/dev/null || true
  i=0
  while pgrep -x meetily >/dev/null 2>&1 && [ "$i" -lt 10 ]; do sleep 1; i=$((i + 1)); done
  ! pgrep -x meetily >/dev/null 2>&1 || error "Meetily did not close; close it and re-run"
}

# Uninstall
if [ "$UNINSTALL" = 1 ]; then
  # Refuse before removing anything if a requested data deletion could not be confirmed.
  CAN_ASK=0; ( : </dev/tty ) 2>/dev/null && CAN_ASK=1
  if [ "$PURGE" = 1 ] && [ "$ASSUME_YES" = 0 ] && [ "$CAN_ASK" = 0 ]; then
    error "No terminal to confirm deleting data; re-run with --yes"
  fi
  FOUND=0
  [ -e "$APPIMAGE" ] || [ -e "$DESKTOP_FILE" ] || [ -e "$ICON" ] && FOUND=1
  if [ "$FOUND" = 1 ] || [ "$PURGE" = 1 ] || dpkg -s meetily >/dev/null 2>&1; then stop_app; fi
  rm -f "$APPIMAGE" "$DESKTOP_FILE" "$ICON"
  rmdir "$APP_DIR" 2>/dev/null || true
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
  [ "$FOUND" = 0 ] || info "Removed Meetily AppImage install"
  DEB=0
  if dpkg -s meetily >/dev/null 2>&1; then
    if [ "$(id -u)" = 0 ]; then
      SUDO=""
    elif command -v sudo >/dev/null 2>&1; then
      SUDO=sudo
      info "Removing the .deb needs administrator rights; you may be asked for your password"
    else
      error "The Meetily .deb is installed; remove it as root with: apt-get remove meetily"
    fi
    $SUDO apt-get remove -y meetily
    info "Removed Meetily .deb package"
    DEB=1
  fi
  [ "$FOUND" = 1 ] || [ "$DEB" = 1 ] || info "No Meetily install found"
  if [ "$PURGE" = 1 ] || { [ "$ASSUME_YES" = 0 ] && [ "$CAN_ASK" = 1 ]; }; then
    set -- "$DATA_DIR" "$CONFIG_DIR"
    [ "$PURGE_REC" = 0 ] || set -- "$@" "$RECORDINGS_DIR"
    purge "$@"
  else
    info "Your meetings and settings were kept. To delete them too, add --purge"
  fi
  [ "$PURGE_REC" = 1 ] || [ ! -d "$RECORDINGS_DIR" ] || info "Recordings in $RECORDINGS_DIR were kept (use --purge-recordings to delete them; a folder you chose in the app is not touched)"
  exit 0
fi

# Mode selection
if [ -z "$MODE" ]; then
  if [ "$DEB_OK" = 1 ]; then MODE=deb; else MODE=appimage; fi
fi
if [ "$MODE" = deb ] && [ "$DEB_OK" = 0 ]; then
  error "--deb requires apt and dpkg; use --appimage instead"
fi
if [ "$MODE" = appimage ]; then
  [ "$(id -u)" != 0 ] || error "Run the AppImage install as your normal user, not root"
  if [ "$DEB_OK" = 1 ] && [ "$ASSUME_YES" = 0 ]; then
    warn "Your system supports the .deb package, which is recommended (system integration and dependency handling). The AppImage works too."
    # stdin is the script itself under curl | sh, so ask on the terminal
    ( : </dev/tty ) 2>/dev/null || error "No terminal to confirm; re-run with --yes to install the AppImage anyway, or without --appimage to install the .deb"
    printf 'Continue with the AppImage? [y/N] ' >/dev/tty
    read -r ans </dev/tty || ans=""
    case "$ans" in
      y|Y|yes|YES) ;;
      *) info "Cancelled. Install the .deb with: curl -fsSL $SCRIPT_URL | sh"; exit 0 ;;
    esac
  fi
fi

# Version
if [ -z "$VERSION" ]; then
  VERSION=$(curl -fsSI "https://github.com/$REPO/releases/latest" | grep -i '^location:' | tr -d '\r' | sed 's|.*/||; s|^v||') \
    || VERSION=""
  [ -n "$VERSION" ] || error "Could not determine the latest version; re-run with --version=X.Y.Z"
fi
expr "$VERSION" : '[0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*$' >/dev/null || error "Unexpected version format: $VERSION"
# MEETILY_RELEASE_BASE_URL lets you test unpublished/draft releases or mirrors.
BASE_URL="${MEETILY_RELEASE_BASE_URL:-https://github.com/$REPO/releases/download/v$VERSION}"

# The AppImage is executed before install, so stage it under $HOME (a noexec /tmp would break the check); the .deb stays in /tmp for apt.
if [ "$MODE" = appimage ]; then
  mkdir -p "$APP_DIR"
  WORK_DIR=$(mktemp -d "$APP_DIR/.install.XXXXXXXXXX")
else
  WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/meetily-install.XXXXXXXXXX")
fi
chmod 755 "$WORK_DIR" # lets apt's _apt user read the .deb
trap 'rm -rf "$WORK_DIR"' EXIT
trap 'exit 130' INT TERM HUP

if [ "$MODE" = deb ]; then PKG=".deb"; else PKG="AppImage"; fi
printf '\n%s' "$GREEN"
cat <<'EOF'
  __  __           _   _ _
 |  \/  | ___  ___| |_(_) |_   _
 | |\/| |/ _ \/ _ \ __| | | | | |
 | |  | |  __/  __/ |_| | | |_| |
 |_|  |_|\___|\___|\__|_|_|\__, |
                           |___/
EOF
printf ' Privacy-first AI meeting note taker%s\n\n  Version: v%s\n  Package: %s\n\n' "$RESET" "$VERSION" "$PKG"

download() {
  if ! curl -fL --progress-bar -o "$WORK_DIR/$1" "$BASE_URL/$1"; then
    if ! curl -fsIL -o /dev/null "$BASE_URL/$1"; then
      error "Meetily v$VERSION has no Linux $PKG build. See https://github.com/$REPO/releases"
    fi
    error "Download failed; check your connection and re-run"
  fi
}

# Start the app for the user after install; skipped on headless sessions, as root, or with --no-launch.
launch() {
  [ "$NO_LAUNCH" = 0 ] || return 0
  [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] || return 0
  info "Launching Meetily..."
  if command -v setsid >/dev/null 2>&1; then
    setsid "$@" >/dev/null 2>&1 </dev/null &
  else
    nohup "$@" >/dev/null 2>&1 </dev/null &
  fi
}

if [ "$MODE" = deb ]; then
  FILE="meetily_${VERSION}_amd64.deb"
  if [ "$(id -u)" = 0 ]; then
    SUDO=""
  else
    command -v sudo >/dev/null 2>&1 || error "sudo is required to install the .deb (or re-run as root)"
    SUDO=sudo
    info "Installing the .deb needs administrator rights; you may be asked for your password"
  fi
  download "$FILE"
  dpkg-deb -I "$WORK_DIR/$FILE" >/dev/null 2>&1 || error "Downloaded file is not a valid .deb (is MEETILY_RELEASE_BASE_URL a direct download URL?)"
  $SUDO apt-get install -y "$WORK_DIR/$FILE"
  if [ -e "$APPIMAGE" ]; then
    warn "An AppImage install of Meetily also exists; remove it with: rm -f \"$APPIMAGE\" \"$DESKTOP_FILE\" \"$ICON\""
  fi
  info "Meetily v$VERSION installed. Launch it from your app menu or run: meetily"
  [ "$(id -u)" = 0 ] || launch meetily
else
  FILE="meetily_${VERSION}_amd64.AppImage"
  if dpkg -s meetily >/dev/null 2>&1; then
    warn "The Meetily .deb is also installed; you will have two Meetily entries. Remove the .deb with: sudo apt remove meetily"
  fi
  if ! command -v fusermount3 >/dev/null 2>&1 && ! command -v fusermount >/dev/null 2>&1; then
    warn "FUSE not found; if Meetily does not start, install fuse3 (e.g. sudo apt install fuse3 / sudo dnf install fuse3)"
  fi
  if [ -x /sbin/ldconfig ]; then
    for lib in libvulkan.so.1 libpulse.so.0 libasound.so.2; do
      /sbin/ldconfig -p 2>/dev/null | grep -q "$lib" || warn "$lib not found; Meetily may fail to start. Install it with your package manager"
    done
  fi
  download "$FILE"
  chmod +x "$WORK_DIR/$FILE"
  "$WORK_DIR/$FILE" --appimage-version >/dev/null 2>&1 || error "Downloaded AppImage is corrupt; re-run the installer"
  ICON_SRC="squashfs-root/usr/share/icons/hicolor/512x512/apps/meetily.png"
  (cd "$WORK_DIR" && "./$FILE" --appimage-extract "usr/share/icons/hicolor/512x512/apps/meetily.png" >/dev/null 2>&1) || true
  if [ -f "$WORK_DIR/$ICON_SRC" ]; then
    mkdir -p "$(dirname "$ICON")"
    cp "$WORK_DIR/$ICON_SRC" "$ICON"
  else
    warn "Could not extract the app icon; the menu entry will use a generic icon"
  fi
  mkdir -p "$APP_DIR" "$(dirname "$DESKTOP_FILE")"
  mv -f "$WORK_DIR/$FILE" "$APPIMAGE" # safe over a running instance: the old inode keeps running
  cat >"$DESKTOP_FILE" <<EOF
[Desktop Entry]
Type=Application
Name=Meetily
Comment=Privacy-first AI meeting note taker
Exec="$APPIMAGE"
TryExec=$APPIMAGE
Icon=$ICON
Categories=Office;
StartupWMClass=meetily
Terminal=false
EOF
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
  info "Meetily v$VERSION installed to $APPIMAGE. Launch it from your app menu. If Meetily was running, restart it."
  launch "$APPIMAGE"
fi

#!/usr/bin/env bash
#
# review-env-setup.sh — bring a machine from scratch to "can run Meetily's checks".
#
# Written for a fresh-context reviewer who has just cloned this branch and wants
# to verify it without first reverse-engineering the build. Idempotent: safe to
# re-run, installs only what is missing.
#
#   ./scripts/review-env-setup.sh          # install, then report
#   ./scripts/review-env-setup.sh --check  # report only, install nothing
#
# System packages come from scripts/linux-deps.txt, which .github/workflows/
# pr-checks.yml reads too, so this script and CI cannot drift apart.
#
# Exit codes: 0 = ready, 1 = setup failed, 2 = --check found something missing.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEPS_FILE="$REPO_ROOT/scripts/linux-deps.txt"
CHECK_ONLY=0
[[ "${1:-}" == "--check" ]] && CHECK_ONLY=1

# Minimums. Rust edition 2021 + rust-version 1.77 in src-tauri/Cargo.toml;
# Next.js 14 wants Node >= 18.17, and the lockfile is pnpm v9 format.
MIN_NODE_MAJOR=18
MIN_PNPM_MAJOR=9

red()  { printf '\033[31m%s\033[0m\n' "$*"; }
grn()  { printf '\033[32m%s\033[0m\n' "$*"; }
ylw()  { printf '\033[33m%s\033[0m\n' "$*"; }
hdr()  { printf '\n\033[1m== %s ==\033[0m\n' "$*"; }

MISSING=0
note_missing() { MISSING=$((MISSING + 1)); }

SUDO=""
if [[ "$(id -u)" -ne 0 ]]; then
  if command -v sudo >/dev/null 2>&1; then
    SUDO="sudo"
  else
    red "Not root and no sudo available; cannot install system packages."
    red "Re-run as root, or install the packages in $DEPS_FILE by hand."
    exit 1
  fi
fi

read_deps() { grep -vE '^\s*(#|$)' "$DEPS_FILE"; }

hdr "Platform"
OS="$(uname -s)"
echo "uname:  $OS $(uname -m)"
if [[ -r /etc/os-release ]]; then
  # shellcheck disable=SC1091
  . /etc/os-release
  echo "distro: ${PRETTY_NAME:-unknown}"
fi

case "$OS" in
  Linux)
    if ! command -v apt-get >/dev/null 2>&1; then
      ylw "Not a Debian/Ubuntu system. Install the equivalents of:"
      read_deps | sed 's/^/    /'
      ylw "then re-run with --check."
    else
      hdr "System packages"
      TO_INSTALL=()
      while IFS= read -r pkg; do
        if dpkg -s "$pkg" >/dev/null 2>&1; then
          printf '  %-34s present\n' "$pkg"
        else
          printf '  %-34s MISSING\n' "$pkg"
          TO_INSTALL+=("$pkg")
        fi
      done < <(read_deps)

      if [[ ${#TO_INSTALL[@]} -gt 0 ]]; then
        if [[ $CHECK_ONLY -eq 1 ]]; then
          note_missing
          ylw "${#TO_INSTALL[@]} package(s) missing; re-run without --check to install."
        else
          hdr "Installing ${#TO_INSTALL[@]} package(s)"
          $SUDO apt-get update -qq
          DEBIAN_FRONTEND=noninteractive $SUDO apt-get install -y --no-install-recommends "${TO_INSTALL[@]}"
          grn "System packages installed."
        fi
      else
        grn "All system packages present."
      fi
    fi
    ;;
  Darwin)
    hdr "System packages"
    ylw "macOS: Tauri uses the system WebKit, so no webview packages are needed."
    echo "Ensure Xcode Command Line Tools are installed: xcode-select --install"
    for tool in cmake; do
      if command -v "$tool" >/dev/null 2>&1; then
        printf '  %-34s present\n' "$tool"
      else
        printf '  %-34s MISSING (brew install %s)\n' "$tool" "$tool"
        note_missing
      fi
    done
    ;;
  *)
    ylw "Unrecognised platform '$OS'; skipping system packages."
    ;;
esac

hdr "Toolchains"

if command -v cargo >/dev/null 2>&1; then
  printf '  %-34s %s\n' "cargo" "$(cargo --version)"
  printf '  %-34s %s\n' "rustc" "$(rustc --version)"
else
  red "  cargo MISSING — install from https://rustup.rs"
  note_missing
fi

if command -v node >/dev/null 2>&1; then
  NODE_V="$(node --version)"
  NODE_MAJOR="$(printf '%s' "$NODE_V" | sed 's/^v\([0-9]*\).*/\1/')"
  if [[ "$NODE_MAJOR" -ge "$MIN_NODE_MAJOR" ]]; then
    printf '  %-34s %s\n' "node" "$NODE_V"
  else
    red "  node $NODE_V is older than the required v$MIN_NODE_MAJOR"
    note_missing
  fi
else
  red "  node MISSING — install Node >= $MIN_NODE_MAJOR"
  note_missing
fi

if command -v pnpm >/dev/null 2>&1; then
  PNPM_V="$(pnpm --version)"
  PNPM_MAJOR="${PNPM_V%%.*}"
  if [[ "$PNPM_MAJOR" -ge "$MIN_PNPM_MAJOR" ]]; then
    printf '  %-34s %s\n' "pnpm" "$PNPM_V"
  else
    red "  pnpm $PNPM_V is older than the required v$MIN_PNPM_MAJOR"
    note_missing
  fi
else
  if [[ $CHECK_ONLY -eq 1 ]]; then
    red "  pnpm MISSING"
    note_missing
  else
    hdr "Installing pnpm"
    corepack enable && corepack prepare pnpm@latest --activate
    grn "pnpm $(pnpm --version) installed."
  fi
fi

hdr "Sidecar binaries"
# tauri.conf.json declares bundle.externalBin ["binaries/llama-helper",
# "binaries/ffmpeg"]. tauri_build::build() HARD-FAILS if either is missing for
# the host triple, so `cargo check` cannot even run without them.
#
# build.rs downloads ffmpeg automatically. llama-helper is NOT automatic: the
# release workflows build it explicitly (see .github/workflows/build-*.yml) but
# no developer-facing script does, so a fresh Linux clone fails to build with
# "resource path `binaries/llama-helper-<triple>` doesn't exist". That is the
# single biggest papercut for a first-time contributor, so we handle it here.
TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
EXT=""
[[ "$OS" == "MINGW"* || "$OS" == "MSYS"* || "$OS" == "CYGWIN"* ]] && EXT=".exe"
HELPER="$REPO_ROOT/frontend/src-tauri/binaries/llama-helper-${TRIPLE}${EXT}"

if [[ -x "$HELPER" ]]; then
  printf '  %-34s present\n' "llama-helper-${TRIPLE}${EXT}"
elif [[ $CHECK_ONLY -eq 1 ]]; then
  printf '  %-34s MISSING\n' "llama-helper-${TRIPLE}${EXT}"
  note_missing
else
  ylw "  Building llama-helper sidecar (compiles llama.cpp — this takes a while)..."
  mkdir -p "$REPO_ROOT/frontend/src-tauri/binaries"
  # Debug, not release: nothing here runs the sidecar, and cargo check/clippy/
  # test only need the file to exist. The release workflows build it properly.
  ( cd "$REPO_ROOT" && cargo build -p llama-helper )
  cp "$REPO_ROOT/target/debug/llama-helper${EXT}" "$HELPER"
  grn "  Built and installed $HELPER"
fi

# ffmpeg is fetched by build.rs on the first build; report but never fetch here.
FFMPEG="$REPO_ROOT/frontend/src-tauri/binaries/ffmpeg-${TRIPLE}${EXT}"
if [[ -x "$FFMPEG" ]]; then
  printf '  %-34s present\n' "ffmpeg-${TRIPLE}${EXT}"
else
  ylw "  ffmpeg-${TRIPLE}${EXT} absent — build.rs downloads it on the first cargo build."
fi

hdr "Runtime capabilities (informational — these gate what you can TEST, not build)"
# A reviewer needs to know up front which parts of the manual matrix are simply
# unavailable to them, rather than discovering it halfway through.
if [[ -d /dev/snd ]] || (command -v pactl >/dev/null 2>&1 && pactl info >/dev/null 2>&1); then
  grn "  audio devices     present  — the recording path is testable"
else
  ylw "  audio devices     ABSENT   — the recording path CANNOT be exercised here"
  ylw "                              (Features 1 and 3 need a machine with a mic)"
fi
if command -v claude >/dev/null 2>&1; then
  grn "  claude CLI        present  — $(command -v claude)"
else
  ylw "  claude CLI        absent   — the Claude Code provider is untestable here"
fi
if command -v obsidian >/dev/null 2>&1 || [[ -d /Applications/Obsidian.app ]]; then
  grn "  Obsidian          present  — URI launch is testable"
else
  ylw "  Obsidian          absent   — obsidian:// launch untestable; vault WRITES"
  ylw "                              are still testable against a fake .obsidian dir"
fi

hdr "Result"
if [[ $MISSING -gt 0 ]]; then
  if [[ $CHECK_ONLY -eq 1 ]]; then
    ylw "$MISSING item(s) missing."
    exit 2
  fi
  red "$MISSING item(s) still missing after setup; see above."
  exit 1
fi
grn "Environment ready. Next:"
cat <<'NEXT'

  cd frontend/src-tauri
  cargo fmt --check
  cargo check --all-targets
  cargo clippy --all-targets -- -D warnings
  cargo test

  cd frontend
  pnpm install --frozen-lockfile
  pnpm run lint
  pnpm exec tsc --noEmit
  pnpm run build

NEXT

#!/usr/bin/env bash
# =============================================================================
# setup-updates.sh
#
# Sets up and publishes OTA updates for Clearminutes.
#
# COMMANDS:
#   setup   - Generate signing keypair and update tauri.conf.json (run once)
#   release - Build, generate manifest, and publish a GitHub release
#   help    - Show usage
#
# USAGE:
#   ./scripts/setup-updates.sh setup [--repo owner/repo]
#   ./scripts/setup-updates.sh release <version> [--notes "Release notes"] [--repo owner/repo]
#
# EXAMPLES:
#   ./scripts/setup-updates.sh setup --repo jamesgibbard/clearminutes
#   ./scripts/setup-updates.sh release 1.0.1 --notes "Bug fixes" --repo jamesgibbard/clearminutes
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
FRONTEND_DIR="$REPO_ROOT/frontend"
TAURI_CONF="$FRONTEND_DIR/src-tauri/tauri.conf.json"
KEY_DIR="$HOME/.tauri"
KEY_FILE="$KEY_DIR/clearminutes.key"

# ── Colours ──────────────────────────────────────────────────────────────────
GREEN='\033[0;32m'; YELLOW='\033[1;33m'; RED='\033[0;31m'; NC='\033[0m'
ok()   { echo -e "${GREEN}✓${NC} $*"; }
warn() { echo -e "${YELLOW}⚠${NC}  $*"; }
err()  { echo -e "${RED}✗${NC} $*" >&2; }
die()  { err "$*"; exit 1; }
hr()   { echo "────────────────────────────────────────────────────────"; }

# ── Helpers ───────────────────────────────────────────────────────────────────
require() {
  for cmd in "$@"; do
    command -v "$cmd" &>/dev/null || die "Required command not found: $cmd  (please install it)"
  done
}

get_current_version() {
  node -e "const f=require('$TAURI_CONF'); console.log(f.version);" 2>/dev/null \
    || grep '"version"' "$TAURI_CONF" | head -1 | grep -oE '[0-9]+\.[0-9]+\.[0-9]+'
}

get_repo_from_conf() {
  # Try to extract from existing endpoints URL
  grep -oE 'github\.com/[^/]+/[^/]+' "$TAURI_CONF" 2>/dev/null | head -1 | sed 's|github.com/||'
}

# ── Command: setup ────────────────────────────────────────────────────────────
cmd_setup() {
  local REPO=""

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --repo) REPO="$2"; shift 2 ;;
      *) die "Unknown option: $1" ;;
    esac
  done

  # Prompt for repo if not provided
  if [[ -z "$REPO" ]]; then
    local detected; detected=$(get_repo_from_conf || true)
    if [[ -n "$detected" && "$detected" != "jgibbarduk/clearminutes" ]]; then
      REPO="$detected"
      ok "Detected repo from config: $REPO"
    else
      read -rp "Enter your GitHub repo (e.g. jamesgibbard/clearminutes): " REPO
      [[ -n "$REPO" ]] || die "Repo cannot be empty"
    fi
  fi

  hr
  echo "Setting up Clearminutes auto-updates"
  echo "Repo: https://github.com/$REPO"
  hr

  require pnpm node

  # 1. Generate keypair
  if [[ -f "$KEY_FILE" ]]; then
    warn "Key already exists at $KEY_FILE — skipping generation"
    warn "Delete it first if you want a fresh key"
  else
    echo "Generating signing keypair..."
    mkdir -p "$KEY_DIR"
    # Use tauri signer generate — it writes the private key to a file
    cd "$FRONTEND_DIR"
    PUBKEY=$(pnpm tauri signer generate -w "$KEY_FILE" 2>&1 | grep -E 'PUBLIC KEY|public key' -A2 | grep -v '^--' | tail -1 || true)
    # Fall back: read the .pub file tauri may have created
    if [[ -z "$PUBKEY" && -f "${KEY_FILE}.pub" ]]; then
      PUBKEY=$(cat "${KEY_FILE}.pub")
    fi
    ok "Private key saved to $KEY_FILE"
  fi

  # Read public key from .pub file (tauri signer always writes one)
  local pub_file="${KEY_FILE}.pub"
  if [[ ! -f "$pub_file" ]]; then
    # Try to re-derive — just warn the user
    warn "Could not find ${pub_file}"
    warn "Run:  pnpm tauri signer generate -w $KEY_FILE"
    warn "Then re-run this script."
    exit 1
  fi
  local PUBKEY
  PUBKEY=$(cat "$pub_file")
  ok "Public key: $PUBKEY"

  # 2. Patch tauri.conf.json
  echo "Patching tauri.conf.json..."
  local ENDPOINT="https://github.com/$REPO/releases/latest/download/latest.json"

  # Use node for safe JSON editing
  node - "$TAURI_CONF" "$PUBKEY" "$ENDPOINT" <<'EOF'
const fs = require('fs');
const [,, file, pubkey, endpoint] = process.argv;
const conf = JSON.parse(fs.readFileSync(file, 'utf8'));
conf.plugins = conf.plugins || {};
conf.plugins.updater = conf.plugins.updater || {};
conf.plugins.updater.pubkey = pubkey;
conf.plugins.updater.endpoints = [endpoint];
fs.writeFileSync(file, JSON.stringify(conf, null, 4));
console.log('Patched:', file);
EOF

  ok "tauri.conf.json updated"

  hr
  echo ""
  ok "Setup complete!"
  echo ""
  echo "Next steps:"
  echo "  1. Commit tauri.conf.json (the pubkey is safe to commit)"
  echo "  2. Set the TAURI_SIGNING_PRIVATE_KEY env var in CI:"
  echo "       export TAURI_SIGNING_PRIVATE_KEY=\$(cat $KEY_FILE)"
  echo "  3. Publish a release:"
  echo "       ./scripts/setup-updates.sh release 1.0.1 --notes 'My notes' --repo $REPO"
  echo ""
  warn "Keep $KEY_FILE PRIVATE — never commit it!"
  hr
}

# ── Command: release ──────────────────────────────────────────────────────────
cmd_release() {
  local VERSION=""
  local NOTES=""
  local REPO=""

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --notes) NOTES="$2"; shift 2 ;;
      --repo)  REPO="$2";  shift 2 ;;
      -*)      die "Unknown option: $1" ;;
      *)       VERSION="$1"; shift ;;
    esac
  done

  [[ -n "$VERSION" ]] || die "Version required. Usage: release <version> [--notes '...'] [--repo owner/repo]"
  VERSION="${VERSION#v}"  # strip leading 'v'

  # Resolve repo
  if [[ -z "$REPO" ]]; then
    REPO=$(get_repo_from_conf || true)
    [[ -n "$REPO" ]] || { read -rp "Enter your GitHub repo (e.g. jamesgibbard/clearminutes): " REPO; }
    [[ -n "$REPO" ]] || die "Repo cannot be empty"
  fi
  [[ "$REPO" != "jgibbarduk/clearminutes" ]] || \
    warn "Repo still points to upstream — did you run setup first?"

  require pnpm node gh

  # Check private key
  if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
    if [[ -f "$KEY_FILE" ]]; then
      export TAURI_SIGNING_PRIVATE_KEY
      TAURI_SIGNING_PRIVATE_KEY=$(cat "$KEY_FILE")
      export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
      ok "Loaded private key from $KEY_FILE"
    else
      die "TAURI_SIGNING_PRIVATE_KEY not set and $KEY_FILE not found. Run setup first."
    fi
  fi

  hr
  echo "Releasing Clearminutes v$VERSION → $REPO"
  hr

  # 1. Bump version in tauri.conf.json
  echo "Bumping version to $VERSION..."
  node - "$TAURI_CONF" "$VERSION" <<'EOF'
const fs = require('fs');
const [,, file, version] = process.argv;
const conf = JSON.parse(fs.readFileSync(file, 'utf8'));
conf.version = version;
fs.writeFileSync(file, JSON.stringify(conf, null, 4));
EOF
  ok "Version set to $VERSION"

  # 2. Build
  echo "Building release..."
  cd "$FRONTEND_DIR"
  pnpm tauri build
  ok "Build complete"

  # 3. Find bundle dir — tauri outputs to target/release/bundle
  local BUNDLE_DIR="$FRONTEND_DIR/src-tauri/target/release/bundle"
  [[ -d "$BUNDLE_DIR" ]] || die "Bundle directory not found: $BUNDLE_DIR"

  # Collect updater artifacts (.tar.gz, .zip, .sig files)
  local UPDATER_DIR="$BUNDLE_DIR/updater"
  # Some tauri versions put them in a dedicated 'updater' subdir; others inline
  if [[ ! -d "$UPDATER_DIR" ]]; then
    UPDATER_DIR="$BUNDLE_DIR"
  fi

  # 4. Generate latest.json
  local MANIFEST="$REPO_ROOT/latest.json"
  echo "Generating update manifest..."
  cd "$REPO_ROOT"
  node scripts/generate-update-manifest-github.js "$VERSION" "$UPDATER_DIR" "$MANIFEST" "$NOTES"
  ok "Manifest: $MANIFEST"

  # 5. Create GitHub release and upload assets
  local TAG="v$VERSION"
  echo "Creating GitHub release $TAG..."

  # Build list of files to upload: manifest + all bundle artifacts
  local UPLOAD_FILES=("$MANIFEST")
  while IFS= read -r -d '' f; do
    UPLOAD_FILES+=("$f")
  done < <(find "$BUNDLE_DIR" \( \
    -name "*.tar.gz" -o -name "*.tar.gz.sig" \
    -o -name "*.zip"    -o -name "*.zip.sig" \
    -o -name "*.dmg"    -o -name "*.dmg.sig" \
    -o -name "*.exe"    -o -name "*.exe.sig" \
    -o -name "*.msi"    -o -name "*.msi.sig" \
    -o -name "*.AppImage" -o -name "*.AppImage.sig" \
    -o -name "*.deb"    -o -name "*.deb.sig" \
  \) -print0 2>/dev/null)

  local NOTES_FLAG=""
  [[ -n "$NOTES" ]] && NOTES_FLAG="--notes $NOTES" || NOTES_FLAG="--generate-notes"

  gh release create "$TAG" \
    --repo "$REPO" \
    --title "v$VERSION" \
    $NOTES_FLAG \
    "${UPLOAD_FILES[@]}"

  # Rename latest.json asset to ensure it's named exactly 'latest.json'
  # (gh upload uses the filename, so it should already be correct)

  hr
  ok "Release published: https://github.com/$REPO/releases/tag/$TAG"
  echo ""
  echo "The updater endpoint:"
  echo "  https://github.com/$REPO/releases/latest/download/latest.json"
  hr
}

# ── Entrypoint ────────────────────────────────────────────────────────────────
cmd="${1:-help}"
shift || true

case "$cmd" in
  setup)   cmd_setup "$@" ;;
  release) cmd_release "$@" ;;
  help|--help|-h)
    echo ""
    echo "Usage:"
    echo "  ./scripts/setup-updates.sh setup   [--repo owner/repo]"
    echo "  ./scripts/setup-updates.sh release <version> [--notes '...'] [--repo owner/repo]"
    echo ""
    echo "Commands:"
    echo "  setup    Generate signing keypair and configure tauri.conf.json (once)"
    echo "  release  Build, sign, generate manifest, and publish a GitHub release"
    echo ""
    echo "Examples:"
    echo "  ./scripts/setup-updates.sh setup --repo jamesgibbard/clearminutes"
    echo "  ./scripts/setup-updates.sh release 1.0.1 --notes 'Bug fixes'"
    echo ""
    ;;
  *) die "Unknown command: $cmd. Run with 'help' for usage." ;;
esac


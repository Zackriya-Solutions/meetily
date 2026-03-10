#!/bin/bash

# Creates a GitHub release from a local build.
# Assumes: you are on main (or your desired branch), already merged, and clean_build.sh has run.
#
# Usage:
#   ./release.sh              # uses version from tauri.conf.json
#   ./release.sh 0.4.0        # override version

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BUNDLE_DIR="$REPO_ROOT/target/release/bundle"

# Resolve version
if [ -n "$1" ]; then
    VERSION="$1"
else
    VERSION=$(grep -o '"version": "[^"]*"' "$SCRIPT_DIR/src-tauri/tauri.conf.json" | head -1 | cut -d'"' -f4)
fi

if [ -z "$VERSION" ]; then
    echo "Error: Could not determine version"
    exit 1
fi

TAG="v$VERSION"
echo "Preparing release $TAG"

# Verify gh CLI is available
if ! command -v gh &> /dev/null; then
    echo "Error: GitHub CLI (gh) is required. Install with: brew install gh"
    exit 1
fi

# Derive repo from origin remote (not gh repo view, which follows fork parent)
REPO=$(git remote get-url origin 2>/dev/null | sed 's/.*github.com[:/]\(.*\)\.git/\1/')
if [ -z "$REPO" ]; then
    echo "Error: Could not determine GitHub repo from origin remote."
    exit 1
fi
echo "Target repo: $REPO"

# Verify build artifacts exist
DMG="$BUNDLE_DIR/dmg/meetily_${VERSION}_aarch64.dmg"
TARBALL="$BUNDLE_DIR/macos/meetily.app.tar.gz"
SIG="$BUNDLE_DIR/macos/meetily.app.tar.gz.sig"

for f in "$DMG" "$TARBALL" "$SIG"; do
    if [ ! -f "$f" ]; then
        echo "Error: Missing artifact: $f"
        echo "Run ./clean_build.sh first."
        exit 1
    fi
done

echo "Found build artifacts:"
echo "  DMG:     $DMG"
echo "  Tarball: $TARBALL"
echo "  Sig:     $SIG"

# Check if tag already exists locally or on origin
if git tag -l "$TAG" | grep -q . || git ls-remote --tags origin "$TAG" 2>/dev/null | grep -q .; then
    echo "Error: Tag $TAG already exists. Bump the version in tauri.conf.json or pass a different version."
    exit 1
fi

# Generate latest.json for the auto-updater
SIGNATURE=$(cat "$SIG")
LATEST_JSON=$(mktemp)
cat > "$LATEST_JSON" << EOF
{
  "version": "$VERSION",
  "notes": "Release $TAG",
  "pub_date": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "platforms": {
    "darwin-aarch64": {
      "signature": "$SIGNATURE",
      "url": "https://github.com/$REPO/releases/download/$TAG/meetily.app.tar.gz"
    }
  }
}
EOF

echo ""
echo "Generated latest.json:"
cat "$LATEST_JSON"
echo ""

# Confirm before proceeding
read -p "Create release $TAG on $REPO? [y/N] " confirm
if [[ "$confirm" != [yY] ]]; then
    echo "Aborted."
    rm -f "$LATEST_JSON"
    exit 0
fi

# Create tag
git tag "$TAG"
git push origin "$TAG"

# Create the GitHub release
gh release create "$TAG" \
    --repo "$REPO" \
    --title "Meetily $TAG" \
    --generate-notes \
    --draft \
    "$DMG" \
    "$TARBALL#meetily.app.tar.gz" \
    "$LATEST_JSON#latest.json"

rm -f "$LATEST_JSON"

echo ""
echo "Draft release created: https://github.com/$REPO/releases/tag/$TAG"
echo "Review it, edit notes if needed, then publish."

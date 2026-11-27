#!/bin/bash
set -euo pipefail

if [[ $(uname -s) != Darwin || $(uname -m) != arm64 || $# != 1 ]]; then
  printf 'Usage on Apple Silicon: %s /path/to/build-venv/bin/python\n' "$0" >&2
  exit 1
fi

python=$1
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/meetily-parakeet-mlx.XXXXXX")
trap 'rm -rf "$work"' EXIT

set --
if [[ -n ${APPLE_SIGNING_IDENTITY:-} ]]; then
  set -- --codesign-identity "$APPLE_SIGNING_IDENTITY"
fi
"$python" -m PyInstaller --noconfirm --onedir --target-arch arm64 \
  --name parakeet-mlx --distpath "$root/frontend/src-tauri/binaries" \
  --workpath "$work/build" --specpath "$work" \
  --collect-all mlx --collect-all librosa --collect-all lazy_loader \
  --collect-submodules parakeet_mlx \
  "$@" "$here/helper.py"

# Tauri dereferences resource symlinks. Keep the Metal library next to the
# dylib PyInstaller loads at the root of _internal.
contents="$root/frontend/src-tauri/binaries/parakeet-mlx/_internal"
for library in libmlx.dylib libjaccl.dylib; do
  unlink "$contents/$library"
  mv "$contents/mlx/lib/$library" "$contents/$library"
done
mv "$contents/mlx/lib/mlx.metallib" "$contents/mlx.metallib"

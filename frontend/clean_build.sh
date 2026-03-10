#!/bin/bash

# Exit on error
set -e

# Add log level selector with default to INFO
LOG_LEVEL=${1:-info}

case $LOG_LEVEL in
    info|debug|trace)
        export RUST_LOG=$LOG_LEVEL
        ;;
    *)
        echo "Invalid log level: $LOG_LEVEL. Valid options: info, debug, trace"
        exit 1
        ;;
esac

# Check and install CMake if needed
echo "Checking CMake version..."
if ! command -v cmake &> /dev/null; then
    echo "CMake not found. Installing via Homebrew..."
    brew install cmake
else
    CMAKE_VERSION=$(cmake --version | head -n1 | cut -d" " -f3)
    if [[ "$CMAKE_VERSION" < "3.5" ]]; then
        echo "CMake version $CMAKE_VERSION is too old. Updating via Homebrew..."
        brew upgrade cmake
    fi
fi

# Clean up previous builds
echo "Cleaning up previous builds..."
rm -rf target/
rm -rf src-tauri/target
rm -rf src-tauri/gen

# Clean up npm, pnp and next
echo "Cleaning up npm, pnp and next..."
rm -rf node_modules
rm -rf .next
rm -rf .pnp.cjs
rm -rf out

echo "Installing dependencies..."
pnpm install

# Build the Next.js application first
echo "Building Next.js application..."
pnpm run build

# Set environment variables for Tauri updater signing
SIGNING_KEY_PATH="$HOME/.tauri/meetily.key"
if [ -f "$SIGNING_KEY_PATH" ]; then
    echo "Found Tauri signing key at $SIGNING_KEY_PATH"
    export TAURI_SIGNING_PRIVATE_KEY=$(cat "$SIGNING_KEY_PATH")
    export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
else
    echo "Warning: No signing key found at $SIGNING_KEY_PATH"
    echo "The updater bundle will not be signed. Run 'pnpm tauri signer generate -w $SIGNING_KEY_PATH' to create one."
fi

echo "Building Tauri app..."
pnpm run tauri build


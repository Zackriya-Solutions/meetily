#!/bin/bash
# GPU-accelerated build script for Meetily
# Automatically detects and builds with optimal GPU features

set -e

# ---------------------------------------------------------------------------
# GPU feature helpers
# ---------------------------------------------------------------------------

# GPU accelerator tokens understood by this script. Keep in sync with the
# match arms below and the `ort-*` features in frontend/src-tauri/Cargo.toml.
# `openblas` and `none` are CPU-only markers: OpenBLAS is CPU linear algebra,
# not a GPU accelerator.
KNOWN_GPU_FEATURES="coreml metal cuda vulkan hipblas openblas none"

is_known_gpu_feature() {
  local candidate="$1" known
  for known in $KNOWN_GPU_FEATURES; do
    if [ "$candidate" = "$known" ]; then
      return 0
    fi
  done
  return 1
}

print_valid_gpu_features() {
  echo -e "   Valid values: ${KNOWN_GPU_FEATURES// /, }"
  echo -e "   (leave TAURI_GPU_FEATURE unset for a CPU-only build)"
}

# parse_ort_features <value>
#
# Contract: normalise a cargo `--features` value into a comma-separated list.
# Accepts an optional, whitespace-tolerant `--features` / `--features=` prefix,
# then splits on commas and whitespace and trims every element. An empty or
# missing value yields an empty string (never the literal "--features ").
#
# Real call sites:
#   * this script's own ORT_FEATURES values ("--features ort-coreml", ...)
#   * the `ort-*` features declared in frontend/src-tauri/Cargo.toml
# The GPU token emitted by frontend/scripts/auto-detect-gpu.js is a SINGLE
# token, and frontend/scripts/tauri-auto.js never splits on commas, so the
# comma-joined form produced here is local to this script and to
# tauri-auto.js's pass-through `--features` argument.
parse_ort_features() {
  local raw="$*" item out=""
  # Trim leading whitespace.
  raw="${raw#"${raw%%[![:space:]]*}"}"
  case "$raw" in
    --features=*) raw="${raw#--features=}" ;;
    --features*)  raw="${raw#--features}" ;;
  esac
  local normalized="${raw//,/ }"
  for item in $normalized; do
    [ -n "$item" ] || continue
    if [ -z "$out" ]; then
      out="$item"
    else
      out="$out,$item"
    fi
  done
  printf '%s' "$out"
}

# Test hook: lets frontend/tests/scripts/build-gpu-path-resolution.test.sh
# source only the helpers above without running the build.
if [ "${BUILD_GPU_LIB_ONLY:-0}" = "1" ]; then
  if [ "${BASH_SOURCE[0]}" != "$0" ]; then
    return 0
  fi
  exit 0
fi

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}🚀 Meetily GPU-Accelerated Build Script${NC}"
echo ""

# ---------------------------------------------------------------------------
# Absolute, cwd-independent path resolution.
# The script lives in frontend/, so derive every path from BASH_SOURCE and
# canonicalise it. No later `cd` can change what target/... means.
# ---------------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
FRONTEND_DIR="$SCRIPT_DIR"
WORKSPACE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"

if [ ! -f "$FRONTEND_DIR/package.json" ]; then
  echo -e "${RED}❌ Could not find package.json in $FRONTEND_DIR${NC}"
  exit 1
fi

echo -e "${BLUE}📁 Frontend: $FRONTEND_DIR${NC}"
echo -e "${BLUE}📁 Workspace: $WORKSPACE_ROOT${NC}"

cd "$FRONTEND_DIR" || {
  echo -e "${RED}❌ Failed to change to frontend directory: $FRONTEND_DIR${NC}"
  exit 1
}

# Export CUDA flags for Linux/NVIDIA
if [[ "$OSTYPE" == "linux-gnu"* ]]; then
    export CMAKE_CUDA_ARCHITECTURES=75
    export CMAKE_CUDA_STANDARD=17
    export CMAKE_POSITION_INDEPENDENT_CODE=ON
fi

# Detect OS
if [[ "$OSTYPE" == "darwin"* ]]; then
  OS="macos"
elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
  OS="linux"
else
  echo -e "${RED}❌ Unsupported OS: $OSTYPE${NC}"
  exit 1
fi

# Function to check if command exists
command_exists() {
  command -v "$1" >/dev/null 2>&1
}

echo ""
echo -e "${BLUE}📦 Building Meetily...${NC}"
echo ""

# Check for pnpm or npm
if command_exists pnpm; then
  PKG_MGR="pnpm"
elif command_exists npm; then
  PKG_MGR="npm"
else
  echo -e "${RED}❌ Neither npm nor pnpm found${NC}"
  exit 1
fi

# Detect GPU feature if not already set
if [ -z "${TAURI_GPU_FEATURE:-}" ]; then
    echo -e "${BLUE}🔍 Detecting GPU features...${NC}"
    if ! TAURI_GPU_FEATURE=$(node scripts/auto-detect-gpu.js); then
        echo -e "${RED}❌ GPU auto-detection failed: node scripts/auto-detect-gpu.js exited non-zero${NC}"
        exit 1
    fi
fi

if [ -n "$TAURI_GPU_FEATURE" ]; then
    if ! is_known_gpu_feature "$TAURI_GPU_FEATURE"; then
        echo -e "${RED}❌ Unknown GPU feature: '$TAURI_GPU_FEATURE'${NC}"
        print_valid_gpu_features
        exit 1
    fi
    echo -e "${GREEN}✅ Detected GPU feature: $TAURI_GPU_FEATURE${NC}"
    export TAURI_GPU_FEATURE
else
    echo -e "${YELLOW}⚠️ No specific GPU feature detected or forced${NC}"
fi

# ============================================================
# ONNX Runtime execution provider selection
# ============================================================
# Maps detected GPU type to ort Cargo features for FunASR / Parakeet.
# CPU is always the fallback — ORT silently skips unavailable providers.
ORT_FEATURES=""

if [ -n "$TAURI_GPU_FEATURE" ]; then
    case "$TAURI_GPU_FEATURE" in
        coreml|metal)
            ORT_FEATURES="--features ort-coreml"
            echo -e "${GREEN}🔧 ONNX: Using CoreML (Apple GPU + ANE NPU)${NC}"
            ;;
        cuda)
            ORT_FEATURES="--features ort-cuda,ort-directml"
            echo -e "${GREEN}🔧 ONNX: Using CUDA + DirectML (NVIDIA GPU)${NC}"
            ;;
        vulkan|hipblas)
            if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "win32" ]]; then
                ORT_FEATURES="--features ort-directml"
                echo -e "${GREEN}🔧 ONNX: Using DirectML (Windows GPU)${NC}"
            else
                ORT_FEATURES="--features ort-openvino"
                echo -e "${GREEN}🔧 ONNX: Using OpenVINO (Intel GPU + NPU)${NC}"
            fi
            ;;
        openblas)
            ORT_FEATURES="--features ort-cpu"
            echo -e "${GREEN}🔧 ONNX: Using OpenBLAS CPU linear algebra (not a GPU)${NC}"
            ;;
        none)
            ORT_FEATURES="--features ort-cpu"
            echo -e "${GREEN}🔧 ONNX: CPU-only (TAURI_GPU_FEATURE=none)${NC}"
            ;;
        *)
            echo -e "${RED}❌ Unknown GPU feature: '$TAURI_GPU_FEATURE'${NC}"
            print_valid_gpu_features
            exit 1
            ;;
    esac
else
    ORT_FEATURES="--features ort-cpu"
    echo -e "${GREEN}🔧 ONNX: No GPU detected, using CPU execution provider${NC}"
fi

# Build llama-helper
echo ""
echo -e "${BLUE}🦙 Building llama-helper sidecar (release)...${NC}"

HELPER_DIR="$WORKSPACE_ROOT/llama-helper"
if [ ! -d "$HELPER_DIR" ]; then
    echo -e "${RED}❌ Could not find llama-helper directory${NC}"
    exit 1
fi

# Determine llama-helper features
# Note: llama-cpp-2 does NOT support coreml, only metal/cuda/vulkan
# So for macOS Apple Silicon (which returns 'coreml' for Whisper), use 'metal' for llama-helper
HELPER_FEATURES=""
if [ -n "$TAURI_GPU_FEATURE" ]; then
    LLAMA_FEATURE="$TAURI_GPU_FEATURE"
    if [ "$LLAMA_FEATURE" = "coreml" ]; then
        LLAMA_FEATURE="metal"
        echo -e "${YELLOW}   Note: llama-cpp-2 doesn't support CoreML, using Metal instead${NC}"
    fi
    HELPER_FEATURES="--features $LLAMA_FEATURE"
fi

echo -e "   Building in $HELPER_DIR with features: ${HELPER_FEATURES:-none}"
# Keep $HELPER_FEATURES unquoted: it deliberately word-splits into
# ("--features", "<value>"). Quoting would pass one argv entry and break cargo.
# llama.cpp's bundled cpp-httplib does not compile against OpenSSL 4 (const X509_NAME); the helper never uses HTTPS, so build it without OpenSSL.
if ! (cd "$HELPER_DIR" && CMAKE_DISABLE_FIND_PACKAGE_OpenSSL=TRUE cargo build --release $HELPER_FEATURES); then
    echo -e "${RED}❌ Failed to build llama-helper${NC}"
    exit 1
fi

echo -e "${GREEN}✅ llama-helper built successfully${NC}"

# Detect target triple
echo ""
echo -e "${BLUE}🎯 Detecting target triple...${NC}"
TARGET_TRIPLE=$(rustc -vV | grep "host:" | awk '{print $2}')
echo -e "   Target: $TARGET_TRIPLE"

# Copy binary
BINARIES_DIR="$FRONTEND_DIR/src-tauri/binaries"
mkdir -p "$BINARIES_DIR"

# Clean old binaries
find "$BINARIES_DIR" -name "llama-helper*" -delete

BASE_BINARY="llama-helper"
SIDECAR_BINARY="llama-helper-$TARGET_TRIPLE"

if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "win32" ]]; then
    BASE_BINARY="llama-helper.exe"
    SIDECAR_BINARY="llama-helper-$TARGET_TRIPLE.exe"
fi

# The binary is in the workspace target directory, one level above frontend.
# WORKSPACE_ROOT is absolute, so neither the caller's cwd nor an earlier `cd`
# can change what this path means.
SRC_PATH="$WORKSPACE_ROOT/target/release/$BASE_BINARY"
DEST_PATH="$BINARIES_DIR/$SIDECAR_BINARY"

if [ -f "$SRC_PATH" ]; then
    cp "$SRC_PATH" "$DEST_PATH"
    echo -e "${GREEN}✅ Copied binary to $DEST_PATH${NC}"
else
    echo -e "${RED}❌ Binary not found at $SRC_PATH${NC}"
    # List contents of target/release to help debugging
    echo -e "${YELLOW}Contents of target/release:${NC}"
    ls -la "$WORKSPACE_ROOT/target/release/" || true
    exit 1
fi

# Build using npm scripts
echo -e "${BLUE}Building complete Tauri application...${NC}"
echo ""

# NO_STRIP true due to issues with bundling appImage

# Append ONNX Runtime features to TAURI_GPU_FEATURE for tauri build
# This combines whisper features (e.g., "coreml") with ort features (e.g., "ort-coreml")
# into a single comma-separated string passed to `--features` by tauri-auto.js
if [ -n "$ORT_FEATURES" ]; then
    ORT_FLAG="$(parse_ort_features "$ORT_FEATURES")"
    if [ -n "$ORT_FLAG" ]; then
        if [ -n "$TAURI_GPU_FEATURE" ]; then
            export TAURI_GPU_FEATURE="${TAURI_GPU_FEATURE},${ORT_FLAG}"
        else
            export TAURI_GPU_FEATURE="${ORT_FLAG}"
        fi
    fi
fi

NO_STRIP=true $PKG_MGR run tauri:build

if [ $? -eq 0 ]; then
  echo ""
  echo -e "${GREEN}✅ Build completed successfully!${NC}"
  echo ""
  echo -e "${GREEN}🎉 Complete Tauri application built with GPU acceleration!${NC}"
else
  echo ""
  echo -e "${RED}❌ Build failed${NC}"
  exit 1
fi

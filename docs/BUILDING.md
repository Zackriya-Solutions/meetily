# Building From Source

## Prerequisites

Verified from the current toolchain and manifests:

- Node.js
- `pnpm`
- Rust toolchain
- Platform-native build tools required by Tauri
- `cmake` is required to build [`llama-helper/`](../llama-helper/)

### Windows Note

`cargo check -p meetily` currently reaches `whisper-rs-sys`, which uses bindgen. On this machine the build required `libclang` to be available via `PATH` or `LIBCLANG_PATH`.

## Desktop App

The application code lives in [`desktop/`](../desktop/).

### Install Dependencies

```bash
cd desktop
pnpm install
```

From the repository root, build the local summary sidecar before running Tauri builds:

```bash
cargo build -p llama-helper
```

### Development

```bash
pnpm run dev
pnpm run tauri:dev
```

### Explicit Transcription/GPU Variants

```bash
pnpm run tauri:dev:cpu
pnpm run tauri:dev:cuda
pnpm run tauri:dev:vulkan
pnpm run tauri:dev:metal
pnpm run tauri:dev:coreml
pnpm run tauri:dev:openblas
pnpm run tauri:dev:hipblas
```

### Production Build

```bash
pnpm run tauri:build
```

## Helper Scripts Present In The Repo

The repository also includes wrapper scripts such as:

- [`desktop/clean_run.sh`](../desktop/clean_run.sh)
- [`desktop/clean_run_windows.bat`](../desktop/clean_run_windows.bat)
- [`desktop/build-gpu.sh`](../desktop/build-gpu.sh)
- [`desktop/dev-gpu.sh`](../desktop/dev-gpu.sh)

These scripts exist in the tree, but the `pnpm` commands above are the clearest direct entry points.

## Workspace-Level Rust Checks

From the repository root:

```bash
cargo metadata --no-deps --format-version 1
cargo check -p meetily
```

`cargo check -p meetily` also expects a built `llama-helper` binary to be available so the Tauri bundle step can copy it into [`desktop/src-tauri/binaries/`](../desktop/src-tauri/binaries/).

## What This Repo Does Not Require

The current product path does not require starting a separate FastAPI backend.

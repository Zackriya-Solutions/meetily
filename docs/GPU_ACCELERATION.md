# GPU Acceleration

Meetily's desktop app exposes build-time acceleration options through Cargo features and helper scripts.

## Auto-Detection Script

[`desktop/scripts/auto-detect-gpu.js`](../desktop/scripts/auto-detect-gpu.js) currently selects features with this logic:

- macOS Apple Silicon: `coreml`
- macOS Intel: `metal`
- Windows or Linux with CUDA available: `cuda`
- Linux with ROCm available: `hipblas`
- Windows or Linux with Vulkan SDK and BLAS include paths available: `vulkan`
- BLAS include paths only: `openblas`
- otherwise: CPU-only

[`desktop/scripts/tauri-auto.js`](../desktop/scripts/tauri-auto.js) uses that result to run `tauri dev` or `tauri build` with the corresponding feature flag.

## Cargo Features

The workspace currently defines these optional transcription acceleration features in [`desktop/src-tauri/Cargo.toml`](../desktop/src-tauri/Cargo.toml):

- `cuda`
- `vulkan`
- `metal`
- `coreml`
- `openblas`
- `openmp`
- `hipblas`

## Platform Notes

- macOS dependencies enable Metal and CoreML support in the manifest.
- Windows dependencies currently default to `whisper-rs` raw API without a GPU feature in the target-specific dependency block.
- Linux dependencies default to `whisper-rs` raw API without a GPU feature in the target-specific dependency block.

## Manual Examples

From [`desktop/`](../desktop/):

```bash
pnpm run tauri:dev:cuda
pnpm run tauri:dev:vulkan
pnpm run tauri:dev:metal
pnpm run tauri:dev:cpu
```

## Important Limitation

Availability of a feature flag in the manifest does not guarantee that the host machine has the native SDKs required to compile or run that acceleration path.

# GPU Acceleration Guide

Meetily supports GPU acceleration for transcription, which can significantly improve performance. This guide provides detailed information on how to set up and configure GPU acceleration for your system.

## Supported Backends

Meetily uses the `whisper-rs` library, which supports several GPU acceleration backends:

*   **CUDA:** For NVIDIA GPUs.
*   **Metal:** For Apple Silicon and modern Intel-based Macs.
*   **Core ML:** An additional acceleration layer for Apple Silicon.
*   **Vulkan:** A cross-platform solution for modern AMD and Intel GPUs.
*   **OpenBLAS:** A CPU-based optimization that can provide a significant speed-up over standard CPU processing.

## Automatic GPU Detection

The build scripts (`dev-gpu.sh`, `build-gpu.sh`) are designed to automatically detect your GPU and enable the appropriate feature flag during the build process. The detection is handled by the `scripts/auto-detect-gpu.js` script.

Here's the detection priority:

1.  **CUDA (NVIDIA)**
2.  **Metal (Apple)**
3.  **Vulkan (AMD/Intel)**
4.  **OpenBLAS (CPU)**

If no GPU is detected, the application will fall back to CPU-only processing.

## Manual Configuration

You can enable a specific backend by building with the matching Tauri feature:

```powershell
pnpm run tauri:build:cuda
pnpm run tauri:build:vulkan
```

On Windows CUDA builds, Meetily will try to detect the GPU compute capability automatically via `nvidia-smi` and pass it to CMake. If you need to override that manually, set `WHISPER_CUDA_ARCHITECTURES` before building:

```powershell
$env:WHISPER_CUDA_ARCHITECTURES = "89"
pnpm run tauri:build:cuda
```

## Platform-Specific Instructions

### Linux

For detailed instructions on setting up GPU acceleration on Linux, please refer to the [Linux build instructions](BUILDING.md#--building-on-linux).

### macOS

On macOS, Metal GPU acceleration is enabled by default. No additional configuration is required.

### Windows

To enable GPU acceleration on Windows, you will need to install the appropriate toolkit for your GPU (e.g., the CUDA Toolkit for NVIDIA GPUs) and then build the application with the corresponding feature flag enabled.

# NOTICE

Meet4Specs is distributed under the MIT License (see [LICENSE.md](LICENSE.md)).
This file documents upstream and third-party attribution.

## Upstream project

Meet4Specs is a fork of and a derivative work from
[Zackriya-Solutions/meetily](https://github.com/Zackriya-Solutions/meetily),
licensed under the MIT License, Copyright (c) 2024 Zackriya Solutions.

Vendored or derived upstream code lives under:

- `backend/`
- `backend/whisper-custom/`
- `backend/whisper.cpp` (git submodule, see `.gitmodules`)

## Third-party components fetched at build or run time

These components are not vendored in this repository. They are downloaded
during build or at runtime, and remain under their own licenses.

| Component | Upstream | License |
| --- | --- | --- |
| whisper.cpp | https://github.com/Zackriya-Solutions/whisper.cpp (fork of https://github.com/ggerganov/whisper.cpp), used through the `whisper-rs` crate | MIT |
| FFmpeg prebuilt binaries | https://github.com/Zackriya-Solutions/ffmpeg-binaries (downloaded by `frontend/src-tauri/build/ffmpeg.rs`) | FFmpeg LGPL-2.1-or-later / GPL-2.0-or-later — see note below |
| ONNX Runtime (via the `ort` crate) | https://github.com/pykeio/ort — https://github.com/microsoft/onnxruntime | MIT |
| llama.cpp (via the `llama-cpp-2` crate) | https://github.com/utilityai/llama-cpp-rs — https://github.com/ggml-org/llama.cpp | MIT |
| Whisper GGML models | https://huggingface.co/ggerganov/whisper.cpp | Model weights under their upstream terms |
| Parakeet ONNX models | https://huggingface.co/istupakov/parakeet-tdt-0.6b-v2-onnx | Model weights under their upstream terms |
| Node.js (installed at runtime for the OpenSpec CLI) | https://nodejs.org | MIT (with third-party components under their own licenses) |

### FFmpeg licensing note

FFmpeg is licensed under the LGPL-2.1-or-later, and builds that enable
GPL-licensed components are licensed under the GPL-2.0-or-later. The
FFmpeg LGPL/GPL licensing terms apply to the distributed FFmpeg build,
not to the Meet4Specs source code, which remains MIT.

## Code signing

Upstream third-party binaries bundled inside Meet4Specs installers are not
signed with this project's code signing certificate.

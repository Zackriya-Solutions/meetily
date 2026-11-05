# Orukeet local transcription

Select **Orukeet** in the Parakeet model picker and download its 672 MB INT8 bundle. It uses the existing local transcription engine and supports the same 25 European languages as Parakeet v3. Lightning/v3 remains the recommended default.

Weights: CC BY-SA 4.0. Downloads include weight terms and attribution, plus the separate converter and preprocessor licenses. The [pinned bundle](https://huggingface.co/oruk/orukeet/tree/1751fce6ecde442f14543cf1804800c49b3e415c/onnx/combined-v0.1.0-int8) documents Derek Zeng's decoder composition and Ivan Stupakov's preprocessor. No Python environment, hosted API or account is needed.

The model config is checked before native loading; older Parakeet directories without a config remain supported. Download progress, resume, cancellation, cached reload and deletion use the existing model manager.

## Validation

Meetily's Rust inference core on an AMD EPYC 9B45 CPU measured a **195 ms warm median / 324 ms p95** for Orukeet versus **262 / 425 ms** for the shipped Parakeet v3 files. Sequential throughput was **47.30 versus 36.62 audio seconds per processing second**. The four baseline files were byte-identical to Meetily's CDN files.

Thirty fixed FLEURS validation clips (first five each in English, German, Spanish, French, Russian and Ukrainian), 629 normalized reference words, two timed passes per model in baseline/candidate/candidate/baseline order. WER was 9.70% versus 10.02%; German and Russian regressed. This small sample does not establish general accuracy superiority. Timing includes preprocessing and decoding, excluding model load, WAV reads, VAD, GUI and capture. It is not a concurrent server capacity measurement.

[Raw results, per-language scores, hashes and method](https://huggingface.co/oruk/orukeet/blob/1751fce6ecde442f14543cf1804800c49b3e415c/onnx/combined-v0.1.0-int8/README.md).

Validation covered the actual Rust model manager downloading the pinned bundle, transcription, unload and cached reload; its download/cancellation/error tests; config rejection; and the Next.js production build. Native recording permissions and signed desktop packages were not exercised.

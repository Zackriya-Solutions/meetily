<div align="center">
    <img src="docs/Meetily-6.png" style="border-radius: 10px;" alt="Meetily" />
    <h1>Meetily Speaker ID</h1>
    <p><b>A local, privacy-first AI meeting assistant that knows who said what.</b></p>
</div>

This is a fork of [Meetily](https://github.com/Zackriya-Solutions/meetily) that adds **speaker identification** to the open-source desktop app. Meetings are still captured, transcribed and summarised on your own machine. Every transcript line is now also labelled with its speaker.

## Speaker identification

Transcripts are split into speaker turns, labelled **Me**, **Speaker 1**, **Speaker 2** and so on. You can give each speaker a real name.

- **Live recordings.** When both microphone and system audio are captured, speech that comes mainly from your microphone is labelled **Me**. Other voices are grouped by voiceprint, so remote participants get their own labels, and so do people in the same room as you.
- **Import and re-transcribe.** Imported audio and re-transcribed meetings are split wherever the speaker changes before transcription. Long stretches of back-and-forth conversation are attributed line by line.
- **Rename and merge.** On a meeting's page, click a speaker chip or use the **Speakers** bar to rename a speaker for that meeting. If one person was split into two speakers, merge them.
- **Copy and AI summaries.** Copied transcripts and the text sent for AI summaries are prefixed with speaker names, so summaries can say who decided what and who owns each action item.
- **Fully local.** Voiceprints come from a small on-device model (3D-Speaker CAM++, Chinese + English, 28 MB). The model is downloaded once from Hugging Face and checked against a pinned SHA-256 hash, and all speaker processing runs on your computer.

### Turning it on

1. Open **Settings → Transcription** and find **Speaker Identification**.
2. Leave **Identify speakers** switched on, then click **Download** next to the voice model.
3. Record, import, or re-transcribe (**Enhance**) a meeting.

Without the voice model, recordings that capture system audio are still split into **Me** and **Others**. Microphone-only recordings, imports and re-transcriptions need the model.

### Known limitations

- The voice-matching thresholds were tuned on synthetic speech. Real meetings may occasionally split one person in two (fix it with **merge**) or, more rarely, combine two similar voices.
- When two people talk at the same time, that speech gets a single label.
- Imports and re-transcriptions work from the mixed recording, so they label speakers by voice only and never use **Me**.
- Speaker names reset when a meeting is re-transcribed, because the speaker numbering is recalculated.
- Live speaker labelling has not yet been validated with real devices across platforms.

## Other features

These features come from Meetily itself:

- Real-time local transcription with **Whisper** or **Parakeet**.
- AI summaries with **Ollama** (local), Claude, Groq, OpenRouter, OpenAI, or any OpenAI-compatible endpoint.
- Microphone and system-audio capture with mixing.
- Import existing audio files, and re-transcribe meetings with a different model or language.
- GPU acceleration: Metal/CoreML on macOS, Vulkan on Windows, configurable source builds on Linux.

## Building from source

This fork does not publish installers; build it yourself. You need Rust, Node.js and pnpm 9.15.9.

1. Build and stage the `llama-helper` sidecar as described in [first-run sidecar setup](docs/BUILDING.md#first-run-sidecar-setup).
2. From `frontend/`, install dependencies and run the app:

   ```bash
   pnpm install --frozen-lockfile
   ```

   ```bash
   pnpm run tauri:dev
   ```

3. For a production build, run `pnpm run tauri:build`.

See [BUILDING.md](docs/BUILDING.md), [Building on Linux](docs/building_in_linux.md) and [GPU acceleration](docs/GPU_ACCELERATION.md) for platform prerequisites. If `llama-helper` fails to compile on macOS with an `X509_NAME` error, Homebrew's OpenSSL 4 is being picked up. Build against OpenSSL 3 instead:

```bash
OPENSSL_ROOT_DIR=/opt/homebrew/opt/openssl@3 cargo build -p llama-helper
```

> **Sharing data with an existing install:** this build uses the same app ID and database as Meetily. On first launch it adds a `meeting_speakers` table to your meetings database, and after that an older Meetily build will refuse to open the database. Back up the `meeting_minutes.sqlite` file in the app data folder first if you plan to switch back.

## How it works

- **Live:** the audio pipeline records microphone and system-audio energy for each speech segment and tags the segment with its dominant source. The audio itself is still mixed as before. Each segment's voiceprint is matched against the speakers heard so far, with matching kept separate for microphone and system audio.
- **Batch:** short overlapping windows (1.5 s every 0.5 s) are voiceprinted and grouped across the whole recording. Segments are cut where the speaker changes, and each piece is transcribed separately.
- **Storage:** `transcripts.speaker` stores a stable speaker ID (`me`, `others` or `speaker_N`). Custom names live in a separate `meeting_speakers` table, so renaming never rewrites transcripts.

The code lives in [`frontend/src-tauri/src/speaker/`](frontend/src-tauri/src/speaker/). See [architecture](docs/architecture.md#speaker-identification) for details.

## License and credits

MIT, as in the original project: see [LICENSE.md](LICENSE.md). This fork is based on Meetily by Zackriya Solutions.

- Speaker embeddings: the [3D-Speaker](https://github.com/modelscope/3D-Speaker) CAM++ model, in the ONNX export published by [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx).
- Speech recognition: [Whisper.cpp](https://github.com/ggerganov/whisper.cpp), and NVIDIA's **Parakeet** in the [ONNX conversion by istupakov](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx).
- Upstream Meetily also credits [Screenpipe](https://github.com/mediar-ai/screenpipe) and [transcribe-rs](https://crates.io/crates/transcribe-rs).

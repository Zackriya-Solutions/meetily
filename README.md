<div align="center">
  <img src="frontend/src-tauri/icons/128x128@2x.png" width="96" alt="" />
  <h1>Open Meeting Notes</h1>
  <p>Records your calls, transcribes them on your own machine with each speaker labelled, keeps the recording for playback, and writes the summary with a local model or the provider you choose.</p>
  <p>
    <a href="https://github.com/MashdorDev/open-meeting-notes/releases/latest">Download for Windows and macOS</a>
  </p>
</div>

## What it does

- **Transcribes locally** with Whisper or Parakeet. Audio never leaves the machine unless you pick a cloud summary provider.
- **Labels who spoke**: your microphone is "Me", the other side of the call is split into Speaker 1, Speaker 2 and so on, and names found in the conversation are suggested for them.
- **Keeps the recording**: every meeting can be played back from the app, or opened in its folder.
- **Summarizes** with a built-in local model, Ollama, or an API such as Claude, Groq, OpenRouter or any OpenAI-compatible endpoint.
- **Removes speaker echo** from the microphone track, so the other side is not transcribed twice.
- **Updates itself** from this repository's releases, signed with this project's own key.

## Install

Download the latest installer from [Releases](https://github.com/MashdorDev/open-meeting-notes/releases/latest):

- **Windows**: `Open Meeting Notes_<version>_x64-setup.exe`
- **macOS (Apple Silicon)**: `Open Meeting Notes_<version>_aarch64.dmg`

The installers are not code-signed yet. On Windows, SmartScreen asks you to confirm ("More info", then "Run anyway"). On macOS, right-click the app and choose Open the first time.

## Build from source

See [docs/BUILDING.md](docs/BUILDING.md) and [docs/GPU_ACCELERATION.md](docs/GPU_ACCELERATION.md). In short:

```bash
cd frontend
pnpm install
pnpm run tauri:dev
```

## Contributing

Bug reports and ideas go in [GitHub issues](https://github.com/MashdorDev/open-meeting-notes/issues). Code contributions: [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT. See [LICENSE.md](LICENSE.md).

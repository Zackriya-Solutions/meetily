# Apple Silicon transcription

Build the optional Parakeet MLX helper on an Apple Silicon Mac. Use a separate Python 3.12 environment and run these commands from the repository root.

```sh
uv venv --python 3.12 /path/to/mlx-build-env
uv pip install --python /path/to/mlx-build-env/bin/python \
  mlx==0.32.4 parakeet-mlx==0.7.0 librosa==1.0.1 numpy==2.5.5 pyinstaller==6.20.0
scripts/parakeet-mlx/build.sh /path/to/mlx-build-env/bin/python
cd frontend
pnpm tauri build --target aarch64-apple-darwin --config src-tauri/tauri.parakeet-mlx.conf.json
```

The **Build and Test - macOS** workflow includes the helper by default. It can also build without it.

For a signed build, set `APPLE_SIGNING_IDENTITY` when freezing the helper. The helper ships with its own runtime. Download and select **Lightning (MLX)** in **Settings → Transcription**.

With **Lightning (MLX)** selected, Meetily uses the same local MLX model for live captions and the final transcript. Live captions show a draft during speech and settle at a pause or after a bounded window. After recording stops, Meetily processes saved audio in 24-second regions with three seconds of context on both sides. If the final pass fails, Meetily keeps the settled live transcript.

The helper uses the [Parakeet MLX implementation](https://github.com/senstella/parakeet-mlx) and the [Parakeet v3 MLX model](https://huggingface.co/mlx-community/parakeet-tdt-0.6b-v3). FFmpeg decodes audio in Meetily. The helper reads only local model files and receives audio through its input pipe.

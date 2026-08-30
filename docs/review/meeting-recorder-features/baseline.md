# Baseline: the state of `main` before this branch

Measured on an untouched checkout, Ubuntu 24.04 x86_64, Rust 1.94.1, Node
22.22.2, pnpm 10.33.0. Recorded because everything downstream is judged
against it, and two of these were surprises.

## 1. The repository is not rustfmt-clean

`cargo fmt --check` reports **1062 diffs** on untouched code.

Consequence: a tree-wide `cargo fmt --check` gate would fail every PR
regardless of content, and reformatting the tree to satisfy one would bury the
real change under thousands of unrelated lines. `.github/workflows/pr-checks.yml`
therefore formats **only the `.rs` files a PR touches**, diffed against the base
SHA. This is achievable and actionable, and it stops the tree drifting further.

## 2. `cargo check` fails from a clean clone

```
error: failed to run custom build command for `meetily v0.4.0`
  resource path `binaries/llama-helper-x86_64-unknown-linux-gnu` doesn't exist
```

`tauri.conf.json` declares `bundle.externalBin: ["binaries/llama-helper",
"binaries/ffmpeg"]`, and `tauri_build::build()` hard-fails when either binary is
missing for the host triple. So **the crate does not compile — not even
`cargo check` — until both sidecars exist.**

- `ffmpeg` is handled: `build.rs` → `build/ffmpeg.rs` downloads and verifies it
  on first build (79 MB, works).
- `llama-helper` is **not**. Only the release workflows build it
  (`.github/workflows/build-{devtest,macos,windows}.yml` each run
  `cargo build --release -p llama-helper` then copy to
  `frontend/src-tauri/binaries/llama-helper-<triple>[.exe]`). No
  developer-facing script does — `frontend/clean_run.sh` goes straight from
  `pnpm run build` to `pnpm run tauri dev`.

So a contributor following the documented local flow on a fresh Linux clone
hits a build failure with no obvious cause. This is a pre-existing repo gap,
not something this branch introduced.

**Handled here:** `scripts/review-env-setup.sh` builds and installs the sidecar
(debug — nothing in the check suite executes it, it only has to exist), and
`pr-checks.yml` does the same before its Rust steps.

## 3. Clippy baseline

Not yet measured — blocked behind (2). `pr-checks.yml` currently runs
`cargo clippy --all-targets -- -D warnings`. **If the untouched tree has
pre-existing clippy warnings, that gate is not yet honest** and must be scoped
the same way the format check was. To be resolved before the PR is opened.

## What this environment can and cannot verify

| | |
|---|---|
| No audio devices (`/dev/snd` absent, no PulseAudio) | **The recording path cannot be exercised at all.** Every Feature 1 / Feature 3 manual case needs real hardware. |
| No Obsidian | `obsidian://` launch untestable. Vault *writes* and containment **are** testable against a fabricated `.obsidian` directory, so the destructive paths still get real coverage. |
| Linux x86_64 only | macOS (ScreenCaptureKit, Metal, `~/Library/CloudStorage`) and Windows (WASAPI, reserved filenames, `cmd /C start` quoting) are unverifiable here. |
| `claude` CLI present at `/opt/node22/bin/claude` | Probe, argv construction and envelope parsing are testable. Authenticated generation depends on this session's credentials and should not be taken to represent a user's machine. |

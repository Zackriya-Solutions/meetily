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

## 3. `pnpm install --frozen-lockfile` failed (FIXED)

`package.json` declared five `pnpm.overrides` (the `prosemirror-*` pins) that
`pnpm-lock.yaml` had no `overrides:` block for — the lockfile predated them.

Regenerated with `pnpm install --no-frozen-lockfile`. The result is as clean as
this could be: **50 diff lines, and not one `version:` line changed.** The
overrides were already in force during resolution; only the config block and the
five specifier strings were missing. No dependency drift, no behavioural risk.
`--frozen-lockfile` now passes.

## 4. `pnpm run lint` has never worked

`next lint` under Next 14 does not understand the flat-config
`eslint.config.mjs` in this repo (that is Next 15 style). Finding no
`.eslintrc*`, it falls through to an **interactive** "How would you like to
configure ESLint?" prompt, which fails in CI. And `eslint`,
`eslint-config-next` and `@eslint/eslintrc` are not dependencies at all, so the
config could not load even if it were found.

Not gated. Fixing it properly (install ESLint and migrate the config, or drop
the file) is a real change unrelated to this branch.

## 5. `tsc --noEmit` has one pre-existing error

`tests/lib/blocknote-markdown.test.ts` imports `bun:test`; no bun types are
installed. Not in the Next build graph, so the app is unaffected.

Not gated. **`pnpm run build` is the real type gate and it PASSES** — `tsconfig`
sets `strict: true` and `next.config.js` does not set
`typescript.ignoreBuildErrors`.

## 6. Clippy: 168 warnings and 2 deny-by-default ERRORS

`cargo clippy --all-targets` **fails outright** on untouched code, not merely
under `-D warnings`:

```
error: type `AudioCaptureBackend` implements inherent method `to_string(&self)`
       which shadows the implementation of `Display`
  --> src/audio/capture/backend_config.rs:55

error: this comparison involving the minimum or maximum element for this type
       contains a case that is always true or always false
  --> src/audio/system_audio_commands.rs:123
      assert!(device_list.len() >= 0);
```

Both are real bugs — the second is a test assertion that cannot fail — and both
are two-line fixes. They are unrelated to this branch, so clippy **reports and
does not gate**. Making it a real gate is a good follow-up: fix those two, then
decide whether the 168 warnings get fixed or allow-listed. The most common are
`&PathBuf` instead of `&Path` (10), module-inception (10), and redundant
closures (9).

## What this environment can and cannot verify

| | |
|---|---|
| No audio devices (`/dev/snd` absent, no PulseAudio) | **The recording path cannot be exercised at all.** Every Feature 1 / Feature 3 manual case needs real hardware. |
| No Obsidian | `obsidian://` launch untestable. Vault *writes* and containment **are** testable against a fabricated `.obsidian` directory, so the destructive paths still get real coverage. |
| Linux x86_64 only | macOS (ScreenCaptureKit, Metal, `~/Library/CloudStorage`) and Windows (WASAPI, reserved filenames, `cmd /C start` quoting) are unverifiable here. |
| `claude` CLI present at `/opt/node22/bin/claude` | Probe, argv construction and envelope parsing are testable. Authenticated generation depends on this session's credentials and should not be taken to represent a user's machine. |

## 7. `cargo test`: 185 pass, 2 FAIL

```
audio::device_detection::tests::test_calculate_buffer_timeout_bluetooth
  assert_eq! on Durations derived from float maths: 159.999996ms != 160ms.
  A real test bug — fails in any environment.

audio::incremental_saver::tests::test_checkpoint_creation
  "FFmpeg not found." Needs the ffmpeg sidecar resolvable at RUN time;
  find_ffmpeg_path looks beside the executable, not in binaries/.
```

Not gated. Skipping named tests to force green is quarantining, which this
branch does not do. Fixing both is a well-scoped follow-up that would turn this
into a real gate: the first needs a tolerance instead of `assert_eq!`, the
second needs test-time ffmpeg resolution.

## What actually gates, and why

Six of the checks a normal Rust/Next repo would gate on are red on untouched
code. Rather than ship gates that cannot pass, or fix six unrelated things from
an infrastructure PR, `pr-checks.yml` gates only what is honest and reports the
rest with its measured baseline in a comment, so the numbers cannot drift
quietly.

| Check | Baseline | Gated? |
|---|---|---|
| `pnpm install --frozen-lockfile` | fixed in this PR | **gate** |
| `pnpm run build` | PASS | **gate** |
| `cargo check --all-targets` | PASS (once the sidecar exists) | **gate** |
| `rustfmt` on **added** `.rs` files | PASS | **gate** |
| `rustfmt` on modified files | 1062 diffs tree-wide | advisory |
| `cargo clippy` | 168 warnings + 2 deny-by-default errors | advisory |
| `cargo test` | 185 pass / 2 fail | advisory |
| `pnpm run lint` | never worked (no eslint installed) | removed |
| `tsc --noEmit` | 1 pre-existing error | removed (`build` covers the app) |

### Follow-up punch list

Each is small, none belongs in this PR, and together they would let four of the
advisory rows become gates:

1. Fix `test_calculate_buffer_timeout_bluetooth` (use a tolerance).
2. Make `test_checkpoint_creation` resolve ffmpeg at test time, or gate it on
   ffmpeg being present.
3. Fix the two clippy errors (`inherent_to_string_shadow_display`,
   `absurd_extreme_comparisons`).
4. Decide on the 168 clippy warnings: fix or allow-list.
5. Install ESLint and migrate `eslint.config.mjs`, or delete it and the `lint`
   script.
6. Add bun types or exclude `tests/` from `tsc`.
7. `cargo fmt` the tree once, as its own commit, so formatting can be gated.

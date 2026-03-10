# Linux System Audio: Our Branch vs PR #309

Comparing `enhance/linux-system-audio` (this branch) against PR #309
(`thalesac:fix/linux-pulseaudio-system-audio`).

## TL;DR

| | PR #309 | This branch |
|---|---|---|
| Files changed | 82 (7,452 deletions) | 5 (focused) |
| Mergeable upstream | No — bundles unrelated removals | Yes |
| Friendly device names | Yes (complex mapping) | No (raw PulseAudio names) |
| PULSE_SOURCE trick | Yes | Yes |
| `alsa_input.*` as system audio | Yes | No |
| Partial name matching | Yes | No |
| `default_system_audio_device` location | `speakers.rs` (standalone) | `linux.rs` (colocated) |

---

## 1. Scope

**PR #309** is a large refactoring PR. In addition to the Linux audio fix it
removes: audio import, retranscription, Anthropic/Groq/OpenAI direct
integrations, beta features, and several UI components. The audio fix is
buried inside ~7,400 lines of deletions, making it impossible to review or
merge in isolation.

**This branch** touches exactly 5 files, all within the audio device layer.
No features are removed.

---

## 2. `linux.rs` — Device enumeration

**PR #309** reads `/proc/asound/cards` to build a friendly-name map, then
calls `pactl list sources short` twice (once for monitors, once for inputs).
Each device is renamed before being added to the list:
- `alsa_output.pci-*` → `"Built-in Audio (System Audio)"`
- `bluez_sink.*` → `"Bluetooth (System Audio)"`
- etc.

`default_system_audio_device()` is not in this file — it lives in `speakers.rs`.

**This branch** skips the friendly-name layer entirely. Raw PulseAudio names
(e.g. `alsa_output.pci-0000_00_1f.3...HiFi__hw_sofhdadsp__sink.monitor`) are
used as-is. This avoids wrong mappings when the heuristics don't match.
`default_system_audio_device()` lives here alongside the other pactl helpers,
keeping all Linux-specific logic in one place.

---

## 3. `configuration.rs` — Device resolution

Both use the same core trick: set `PULSE_SOURCE=<monitor_name>` and return the
`"pulse"` ALSA device so CPAL captures from the correct monitor.

**PR #309** also:
- Treats `alsa_input.*` device names as PulseAudio sources (in addition to
  `alsa_output.*` and `*.monitor`)
- Falls through to an **exact match** pass across all input devices
- Falls through to a **partial match** pass (substring matching in both
  directions) when exact match fails
- Logs the full list of available devices when nothing is found

**This branch** only treats `*.monitor` and `alsa_output.*` as PulseAudio
sources (the two patterns that actually indicate monitor sources). After the
PULSE_SOURCE path it falls back to a single exact-match pass — no fuzzy
matching. Partial matching risks returning the wrong device silently.

---

## 4. `speakers.rs` — `default_system_audio_device`

**PR #309** implements the full pactl query inline inside `speakers.rs`,
duplicating the parsing logic that already exists in `linux.rs`.

**This branch** adds a thin wrapper in `speakers.rs` that delegates to
`linux.rs::default_system_audio_device()`, avoiding duplication.

---

## 5. `recording_manager.rs` — Linux recording path

Both split the `#[cfg(not(target_os = "macos"))]` block into separate Linux
and Windows arms and wire up `default_system_audio_device()` for the Linux
path. The logic is equivalent. PR #309 has slightly more verbose warning
messages when the monitor is not found.

---

## 6. `devices/mod.rs`

**PR #309** does not re-export `default_system_audio_device` — it is imported
directly inside `recording_manager.rs`.

**This branch** exports it from `mod.rs` so any other module can use it
without reaching into `speakers` directly.

---

## 7. Known limitation in both implementations

Neither branch changes `capture/system.rs`, which still returns
`bail!("System audio capture not yet implemented for this platform")` for
non-macOS. This is intentional: the Linux path routes system audio through the
CPAL input stream (via the `"pulse"` device) rather than going through the
`SystemAudioCapture` struct, so `system.rs` is never called for Linux.

---

## 8. Prerequisite on Ubuntu

The `"pulse"` ALSA device must exist. It is provided by:

```
sudo apt install pulseaudio-alsa        # PulseAudio systems
# or
sudo apt install pipewire-alsa          # PipeWire systems (Ubuntu 22.10+)
```

Both are installed by default on standard Ubuntu desktop installs.

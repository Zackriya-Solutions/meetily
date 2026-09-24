## Description

This PR introduces end-to-end speaker diarization support using NVIDIA Parakeet + Nemotron-3 (Sortformer architecture), an interactive post-meeting speaker renaming modal with live audio snippet playback, and critical stability fixes for Windows builds and UI initialization.

### Reference Links:
- **Nemotron-3 Diarization Model**: [nvidia/Nemotron-3-Diarization on Hugging Face](https://huggingface.co/nvidia/Nemotron-3-Diarization)
- **Official Announcement & Technical Blog**: [Know Who Spoke When: Build Real-Time, Multi-Speaker AI with NVIDIA Nemotron 3 Diarization](https://huggingface.co/blog/nvidia-nemotron-3-diarization)

---

### Beta Status & Reviewer Note:
> [!NOTE]
> **Beta Feature Notice**: This implementation is in early stages and should be considered **Beta**.
> If preferred during review, the new **Diarization settings** panel can be easily relocated under the **Beta** settings tab instead of general settings. Feedback on UI placement and default thresholds is very welcome!

---

### Key Changes:

#### 1. NVIDIA Parakeet + Nemotron-3 Diarization Engine
- **Turn Splitting & Segmentation**: Added sentence-level speaker boundary detection for conversations where participants speak sequentially or interrupt each other within the same STT recognition chunk. Large transcript blocks are automatically segmented into distinct chronological turns instead of being lumped into compound labels.
- **Acoustic Bleed Filtering**: Dual-track recordings now filter low-volume microphone bleed of remote participant voices, preventing false `"You + Speaker 1"` compound attributions on guest turns.
- **Word-Level Alignment**: Implemented timestamp alignment between Parakeet CTC token timings and Nemotron-3 speaker activity segments.
- **Sliding FIFO Buffer & Speaker Cache (`spkcache`)**: Implemented sliding-window audio chunk buffering with long-term speaker embedding memory to maintain consistent speaker identities across conversational pauses.
- **Native Tauri Commands**: Added backend commands for audio feature extraction, speaker diarization inference, and per-speaker WAV snippet extraction.

#### 2. Qwen3-ASR Engine Integration (0.6B & 1.7B)
- **Live & Post-Call Transcription**: Added Alibaba's Qwen3-ASR models as selectable transcription engines in settings, post-call processing, and retranscription dialogs.
- **Verified ONNX Model Management**: Automated downloads with SHA256 integrity verification, pause/resume download state, and model deletion support directly in the settings UI.

#### 3. Speaker Renaming Modal with Live Audio Preview
- **Interactive Modal (`SpeakerRenameModal.tsx`)**: Easily accessed via the "Rename Speakers" button in the meeting details view.
- **Live Audio Playback**: Users can listen to a short audio snippet for any detected speaker directly inside the modal to accurately verify identity before renaming.
- **Global Transcript Updating**: Renaming updates all corresponding turns across the entire meeting transcript, updating both the SQLite database and client-side virtualized transcript view instantly.

#### 4. SQLite Database Resilience & Self-Healing Migrations
- Added self-healing schema migration logic in `database/manager.rs` to handle legacy schemas, missing columns, or orphaned transcript entries gracefully without app crashes.
- Added database methods for cascading speaker rename updates across meetings and transcripts.

#### 5. Windows Build & UI Startup Reliability Fixes
- **WebView2 Race Condition Fix**: Created `check-or-start-dev.js` and `"dev:ready"` pre-warming script to ensure Next.js has completed compiling the root route before Tauri attaches the WebView2 window.
- **Process Cleanup**: Updated `dev-gpu.bat` and `build-gpu.bat` to terminate orphaned `msedgewebview2.exe` background processes that previously locked the `EBWebView` cache directory.
- **Window Activation & Focus**: Added explicit window focus flags in `tauri.conf.json`, startup focus triggers in `lib.rs`, and a client-side pointer-events recovery watchdog in `app/layout.tsx`.

---

## Related Issue
Addresses speaker diarization integration, speaker identification workflows, and Windows WebView2 UI responsiveness.

---

## Type of Change
- [x] Bug fix (non-breaking change which fixes an issue)
- [x] New feature (non-breaking change which adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to not work as expected)
- [x] Performance improvement
- [x] Code refactoring
- [ ] Documentation update

---

## Testing
- [x] Manual testing performed on Windows 11 with NVIDIA CUDA acceleration.
- [x] Verified full production release build compilation (`build-gpu.bat` / Next.js export / Tauri NSIS bundle).
- [x] Verified Parakeet + Nemotron-3 word-level alignment and speaker assignment.
- [x] Verified speaker renaming modal with live WAV playback and transcript persistence.
- [x] Verified dev server pre-warming and eliminated blank/frozen UI state on launch.
- [x] Verified SQLite database migration and self-healing with existing user databases.

---

## Checklist
- [x] Code follows project style guidelines.
- [x] Self-reviewed the code changes.
- [x] Added comments for complex diarization math, FIFO buffer management, and window lifecycle logic.
- [x] Verified local TypeScript compilation (`next build` static export succeeded with 0 errors).
- [x] Verified Rust compilation (`cargo check --features cuda` and release build succeeded).
- [x] No merge conflicts with base branch.

---

## Additional Notes
- To build the release version on Windows with CUDA acceleration:
  ```cmd
  cd frontend
  build-gpu.bat
  ```
- Binary outputs:
  - Portable EXE: `target\release\meetily.exe`
  - Windows Installer: `target\release\bundle\nsis\meetily_*_x64-setup.exe`

---

## AI Disclaimer
> [!NOTE]
> **AI Disclaimer**: This feature and pull request were developed with AI assistance, but have been thoroughly and rigorously tested end-to-end by myself on Windows with an active NVIDIA GPU and CUDA environment.

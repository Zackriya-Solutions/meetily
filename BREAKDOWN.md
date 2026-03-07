# Clearminutes Repository Breakdown

## 1. High-Level Overview

**Clearminutes** is a privacy-first AI-powered meeting assistant that captures, transcribes, and summarizes meetings entirely on your local machine. It combines desktop UI, real-time audio processing, and GPU-accelerated transcription via Whisper.cpp.

### Problem Solved
- **Privacy**: No cloud processing—all audio stays on your device
- **Speed**: GPU acceleration (CUDA, Metal, CoreML, Vulkan) for real-time transcription
- **Accessibility**: Local LLM summarization via llama.cpp
- **Ease**: Desktop app (Windows/macOS/Linux) with minimal setup

### Main Components
1. **Tauri Desktop App** (frontend/) — React/TypeScript UI for recording, playback, transcription display
2. **Python FastAPI Backend** (backend/) — Whisper.cpp wrapper, database, meeting storage, summarization
3. **Rust Sidecar** (llama-helper/) — GPU-accelerated LLM inference via llama.cpp
4. **Whisper Engine** (backend/whisper-custom/) — Optimized Whisper.cpp build with GPU support

---

## 2. Repository Structure

```
clearminutes/
├── frontend/                    # Tauri + React/TypeScript desktop app
│   ├── src/                     # React components
│   │   ├── App.tsx              # Main app shell
│   │   ├── pages/               # Route pages (home, settings, etc.)
│   │   └── components/          # UI components
│   ├── src-tauri/               # Tauri backend (Rust)
│   │   ├── src/
│   │   │   ├── main.rs          # Tauri entry point + command handlers
│   │   │   ├── lib.rs           # Tauri module exports
│   │   │   ├── audio.rs         # Audio capture logic
│   │   │   ├── api.rs           # HTTP client to Python backend
│   │   │   └── state.rs         # App state management
│   │   ├── binaries/            # Sidecar binaries (llama-helper)
│   │   ├── tauri.conf.json      # Tauri configuration
│   │   └── Cargo.toml           # Rust dependencies
│   ├── package.json             # npm/pnpm dependencies + scripts
│   ├── build-gpu.sh             # GPU-accelerated build script
│   ├── next.config.js           # Next.js config (if SSR used)
│   └── tsconfig.json            # TypeScript config
│
├── backend/                     # Python FastAPI server + Whisper
│   ├── app/
│   │   ├── main.py              # FastAPI app entry point
│   │   ├── db.py                # Database (SQLite) setup
│   │   ├── transcript_processor.py  # Whisper integration
│   │   └── schema_validator.py  # Request/response validation
│   ├── whisper-custom/          # Modified Whisper.cpp repo
│   │   ├── models/              # GGML model files
│   │   ├── CMakeLists.txt       # C++ build config
│   │   └── src/                 # Whisper.cpp source
│   ├── requirements.txt         # Python dependencies
│   ├── docker-compose.yml       # Compose file for dev/prod
│   ├── Dockerfile.*             # CPU, GPU, macOS variants
│   ├── setup-db.sh              # Database initialization
│   └── README.md                # Backend docs
│
├── llama-helper/                # Rust GPU-accelerated LLM sidecar
│   ├── src/
│   │   ├── main.rs              # Entry point (standalone binary)
│   │   ├── lib.rs               # Module exports
│   │   └── gpu.rs               # GPU abstraction layer
│   ├── Cargo.toml               # Features: cuda, metal, vulkan
│   └── README.md                # Build instructions
│
├── docs/                        # Architecture, building, GPU setup
│   ├── architecture.md          # High-level system design
│   ├── BUILDING.md              # Build instructions
│   ├── GPU_ACCELERATION.md      # GPU setup per OS
│   └── building_in_linux.md     # Linux-specific build
│
├── scripts/                     # Utility scripts
│   ├── auto-detect-gpu.js       # GPU feature detection
│   ├── generate-update-manifest-github.js
│   └── inject_transcript.py     # Transcript injection utility
│
├── Cargo.toml                   # Workspace root (frontend + llama-helper)
├── Cargo.lock                   # Locked dependency versions
├── README.md                    # Project overview
├── CONTRIBUTING.md              # Contribution guidelines
├── PRIVACY_POLICY.md            # Privacy assurance
└── CLAUDE.md                    # Development notes

```

### Key Files by Responsibility

| File | Purpose |
|------|---------|
| `frontend/src-tauri/src/main.rs` | Tauri command handlers, lifecycle, audio capture |
| `frontend/src/App.tsx` | Main React component, routing |
| `backend/app/main.py` | FastAPI endpoints, Whisper integration |
| `backend/transcript_processor.py` | Whisper.cpp wrapper, transcription logic |
| `llama-helper/src/main.rs` | LLM inference sidecar entry point |
| `frontend/build-gpu.sh` | GPU detection + conditional Rust/JS compilation |
| `Cargo.toml` | Workspace definition |
| `frontend/package.json` | React/TypeScript dependencies, npm scripts |
| `backend/requirements.txt` | Python FastAPI, Whisper bindings, SQLite |

---

## 3. How the Application Runs

### Startup Flow

#### Local Development
```bash
# Terminal 1: Start Python backend
cd backend
python -m venv venv
source venv/bin/activate
pip install -r requirements.txt
python app/main.py  # Starts on http://localhost:8000

# Terminal 2: Start Tauri dev mode
cd frontend
pnpm install
pnpm tauri dev    # Auto-detects GPU, builds llama-helper, launches app
```

#### Production Build
```bash
# Build GPU-accelerated desktop app
cd frontend
./build-gpu.sh    # Orchestrates entire build pipeline
```

### Build Pipeline (build-gpu.sh)

```
build-gpu.sh
  ├─ Detect OS (macOS, Linux)
  ├─ Detect GPU feature (CUDA, Metal, CoreML, Vulkan)
  ├─ Build llama-helper (Rust sidecar)
  │   └─ Compile with GPU feature flags
  ├─ Copy binary to src-tauri/binaries/
  ├─ Run npm scripts
  │   ├─ pnpm install (if needed)
  │   └─ pnpm tauri:build
  │       ├─ Build React app (next export)
  │       ├─ Build Rust Tauri backend
  │       ├─ Bundle into .dmg (macOS), .AppImage (Linux), .msi (Windows)
  │       └─ Sign bundle (if keychain available)
  └─ Output: frontend/src-tauri/target/release/bundle/
```

### Runtime Entry Points

**Desktop App (Tauri)**
- `frontend/src-tauri/src/main.rs` — Tauri setup, window creation, command registration
- Window spawns React app from `frontend/src/` (compiled to `dist/`)

**Python Backend**
- `backend/app/main.py` — FastAPI app starts, initializes DB, loads Whisper model
- Listens on `localhost:8000` (dev) or port specified by environment

**Sidecar (llama-helper)**
- Spawned by Tauri when needed (lazy-loaded for summarization)
- Binary path: `src-tauri/binaries/llama-helper-{target_triple}`

### Communication Flow

```
┌─────────────────────────────────────────────────────────────┐
│            Tauri Desktop App (Rust + React)                 │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  React UI                                            │   │
│  │  (Meeting list, recorder, transcription display)    │   │
│  └────────────┬─────────────────────────────────────────┘   │
│               │ TypeScript invoke()                         │
│  ┌────────────▼─────────────────────────────────────────┐   │
│  │  Tauri Commands (Rust)                               │   │
│  │  - record_audio()                                     │   │
│  │  - stop_recording()                                   │   │
│  │  - summarize_meeting()                                │   │
│  └────────────┬──────────────────────────────────────────┘   │
│               │ HTTP POST/GET                               │
└───────────────┼───────────────────────────────────────────────┘
                │
┌───────────────▼───────────────────────────────────────────────┐
│        Python FastAPI Backend (localhost:8000)                │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │  Flask/FastAPI Routes                                  │  │
│  │  POST /transcribe - Call Whisper.cpp                    │  │
│  │  GET  /meetings - List recorded meetings               │  │
│  │  POST /summarize - Send to llama-helper                │  │
│  └─────────────────────────────────────────────────────────┘  │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │  Whisper Processor                                      │  │
│  │  (Wraps whisper.cpp C++ binary)                         │  │
│  └─────────────────────────────────────────────────────────┘  │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │  SQLite Database                                        │  │
│  │  (meetings, transcripts, summaries)                     │  │
│  └─────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
                │
┌───────────────▼───────────────────────────────────────────────┐
│  Sidecar: llama-helper (Rust binary, GPU-accelerated)         │
│  - Runs only when summarization requested                     │
│  - Uses llama.cpp with CUDA/Metal/etc.                        │
│  - Communicates via stdio or HTTP (depending on impl)         │
└─────────────────────────────────────────────────────────────────┘
```

---

## 4. Architecture and Layers

### Layer 1: UI Layer (React/TypeScript)
**Location**: `frontend/src/`

**Responsibilities**:
- Display meeting list, recording controls, transcription
- Audio waveform visualization
- Settings (device selection, GPU settings, Whisper model)
- Real-time transcription updates via HTTP polling or WebSocket

**Key Files**:
- `App.tsx` — Router, main layout
- `pages/Recorder.tsx` — Recording UI
- `pages/Transcript.tsx` — Display transcription + summary
- `components/AudioVisualizer.tsx` — Waveform rendering

### Layer 2: Desktop Runtime (Tauri/Rust)
**Location**: `frontend/src-tauri/src/`

**Responsibilities**:
- Window lifecycle, system menu, tray icon
- Audio capture (OS-specific APIs: CoreAudio on macOS, ALSA/PulseAudio on Linux, WASAPI on Windows)
- Command invocation bridge to Python backend
- File I/O (saving recordings, logs)
- Sidecar process management (llama-helper)

**Key Files**:
- `main.rs` — Window setup, command handlers
- `audio.rs` — Audio capture via cpal
- `api.rs` — HTTP client to Python backend
- `state.rs` — Tauri State<T> for app-wide config

**Key Crates**:
- `tauri` — Desktop framework
- `tokio` — Async runtime
- `cpal` — Cross-platform audio
- `reqwest` — HTTP client

### Layer 3: Backend API (Python/FastAPI)
**Location**: `backend/app/`

**Responsibilities**:
- RESTful API for transcription, meeting storage, summarization
- Whisper.cpp integration (via ctypes or subprocess)
- SQLite database management
- Meeting metadata, transcript persistence

**Key Files**:
- `main.py` — FastAPI app, routes
- `transcript_processor.py` — Whisper wrapper
- `db.py` — SQLite schema, queries
- `schema_validator.py` — Pydantic models

**Key Dependencies**:
- `fastapi` — Web framework
- `sqlite3` — Database
- `numpy` — Audio processing
- `ctypes` / `subprocess` — Whisper.cpp binding

### Layer 4: Transcription Engine (Whisper.cpp)
**Location**: `backend/whisper-custom/`

**Responsibilities**:
- Speech-to-text via OpenAI Whisper model (GGML quantized format)
- GPU acceleration: CUDA (NVIDIA), Metal (Apple), Vulkan (cross-platform)
- Runs as separate process, called from Python

**Key Build Options**:
- `WHISPER_CUDA=1` — NVIDIA GPU
- `WHISPER_METAL=1` — Apple GPU
- `WHISPER_VULKAN=1` — Cross-platform GPU

### Layer 5: LLM Inference (llama-helper/Rust)
**Location**: `llama-helper/src/`

**Responsibilities**:
- GPU-accelerated LLM inference for meeting summarization
- Wraps `llama.cpp` C++ library
- Spawned as sidecar when summarization requested
- Returns summary text to backend

**Key Crates**:
- `llama-cpp-2` — Rust binding to llama.cpp
- `tokio` — Async execution
- `serde_json` — Response serialization

**Supported GPU Features** (conditional compilation):
```toml
[features]
cuda    = ["llama-cpp-2/cuda"]
metal   = ["llama-cpp-2/metal"]
vulkan  = ["llama-cpp-2/vulkan"]
```

### Layer Dependencies

```
User ─► React UI (TS)
          │
          └─► Tauri Commands (Rust)
                │
                ├─► Audio Capture (cpal)
                ├─► File I/O
                └─► HTTP Client (reqwest)
                      │
                      └─► FastAPI Backend (Python)
                            │
                            ├─► Whisper.cpp (C++)
                            │     └─► GPU (CUDA/Metal)
                            │
                            ├─► SQLite (local DB)
                            │
                            └─► llama-helper (Rust sidecar)
                                  └─► llama.cpp (C++)
                                        └─► GPU (CUDA/Metal/Vulkan)
```

---

## 5. Dependency Flow

### External Dependencies (Critical)

**Frontend**:
- `react` — UI framework
- `next.js` — Build tooling (optional SSR)
- `tailwindcss` — Styling
- `typescript` — Type safety
- `@tauri-apps/api` — Tauri bridge

**Tauri (Rust)**:
- `tauri` — Desktop framework
- `tokio` — Async I/O
- `cpal` — Audio capture
- `reqwest` — HTTP client
- `serde` — Serialization

**Backend (Python)**:
- `fastapi` — Web framework
- `pydantic` — Data validation
- `numpy` — Audio processing
- `sqlite3` — Database (built-in)

**Whisper.cpp**:
- Submodule: `backend/whisper-custom/`
- Custom build with GPU support compiled in

**llama-helper**:
- `llama-cpp-2` — Rust binding to llama.cpp
- Feature-gated GPU backends: `cuda`, `metal`, `vulkan`

### Internal Module Dependencies

```
frontend/src/App.tsx
  └─► frontend/src/pages/*
        └─► frontend/src/components/*
              └─► Tauri API (@tauri-apps/api)

frontend/src-tauri/src/main.rs
  ├─► audio.rs (capture)
  ├─► api.rs (http to backend)
  └─► state.rs (config)

backend/app/main.py
  ├─► transcript_processor.py
  ├─► db.py
  └─► schema_validator.py

llama-helper/src/main.rs
  ├─► lib.rs (exports)
  └─► gpu.rs (feature selection)
```

---

## 6. Key Workflows

### Workflow 1: Record a Meeting
```
1. User clicks "Start Recording" in UI
2. React component calls Tauri command:
   invoke('start_recording', { device: 'Microphone' })
3. Tauri handler in main.rs:
   - Initializes audio stream via cpal
   - Buffers PCM data to in-memory Vec<u8>
   - Starts timer/UI update loop
4. Audio keeps streaming until user clicks "Stop"
5. Audio buffer saved to disk:
   backend/meetings/{timestamp}.wav
6. Tauri notifies React: recording stopped
```

### Workflow 2: Transcribe Recording
```
1. User views recording in UI, clicks "Transcribe"
2. React calls Tauri command:
   invoke('transcribe_meeting', { file: 'recording.wav' })
3. Tauri sends HTTP POST to backend:
   POST /transcribe { audio_file: path, model: 'base.en' }
4. Python backend (transcript_processor.py):
   - Loads Whisper.cpp binary
   - Passes .wav to Whisper (GPU-accelerated)
   - Receives JSON: { text: "...", segments: [...] }
5. Backend saves to SQLite: meetings table
6. Response sent back to React
7. UI displays transcription with timestamps
```

### Workflow 3: Summarize Meeting
```
1. User clicks "Summarize" button on transcript view
2. React calls Tauri command:
   invoke('summarize_meeting', { meeting_id: '123' })
3. Tauri sends HTTP POST to backend:
   POST /summarize { meeting_id: '123' }
4. Python backend:
   - Retrieves transcript from SQLite
   - Spawns llama-helper sidecar process
   - Sends transcript to llama-helper stdin
5. llama-helper (Rust):
   - Loads llama.cpp with GPU support
   - Runs inference on transcript
   - Streams summary back
6. Backend stores summary in SQLite
7. React displays summary in UI
```

### Workflow 4: Export Meeting
```
1. User right-clicks meeting, selects "Export"
2. React calls Tauri command:
   invoke('export_meeting', { id: '123', format: 'pdf' })
3. Tauri:
   - Retrieves data from backend
   - Generates PDF (or Markdown/TXT)
   - Opens file dialog for save location
   - Returns file path
4. File saved to user's Downloads or custom location
```

---

## 7. Configuration and Environment

### Environment Variables

**Backend (.env or env file)**:
```bash
WHISPER_MODEL=base.en              # Whisper model size
WHISPER_DEVICE=cuda                # CPU or CUDA
SQLITE_DB=./meetings.db            # Database path
API_PORT=8000                       # FastAPI port
API_HOST=127.0.0.1                 # Localhost only
LOG_LEVEL=INFO                      # Logging verbosity
```

**Frontend (Tauri)**:
```bash
TAURI_GPU_FEATURE=cuda              # Auto-detected, can override
TAURI_ENV=development|production    # Build target
NODE_ENV=development                # React env
```

### Configuration Files

**Tauri App Config**
- `frontend/src-tauri/tauri.conf.json`
  - Window dimensions, app name, version
  - Security settings (allowlist for IPC commands)
  - Build targets

**Database Schema**
- `backend/app/db.py`
  - `meetings` — filename, date, duration, device
  - `transcripts` — meeting_id, text, segments (JSON)
  - `summaries` — meeting_id, summary_text, generated_at

**GPU Feature Detection**
- `frontend/scripts/auto-detect-gpu.js`
  - Detects OS + GPU capability
  - Returns feature string: "cuda", "metal", "vulkan", or "none"
  - Used by `build-gpu.sh` to set compilation flags

### Secrets Handling

- **No sensitive data stored** — Architecture is entirely local
- **Whisper models**: Downloaded on first use, cached locally
- **LLM models**: User downloads manually or via app UI
- **API keys**: Not used (everything local)

### Environment Differences

| Aspect | Dev | Production |
|--------|-----|------------|
| Backend URL | localhost:8000 | Embedded (IPC) |
| Whisper Device | Auto-detected | Auto-detected |
| Logging | DEBUG | INFO |
| Hot Reload | Enabled (pnpm dev) | Disabled |
| Bundling | Uncompressed | Signed, compressed |

---

## 8. Deployment Model

### Local Desktop Deployment

**Supported Platforms**:
- **macOS** (x86_64, ARM64 Apple Silicon)
- **Linux** (x86_64, x86)
- **Windows** (x86_64)

**Distribution**:
- `.dmg` (macOS) — Drag-and-drop installer
- `.AppImage` (Linux) — Single executable, no install needed
- `.msi` (Windows) — Standard Windows installer
- `.exe` (Windows) — Portable executable

**Build Artifacts**
- `frontend/src-tauri/target/release/bundle/`
  - Each platform has a subfolder with final artifacts

### Backend Deployment (Optional)

**Docker** (for server deployment, if Clearminutes runs as service):
- `backend/Dockerfile.app` — FastAPI + Whisper
- `backend/Dockerfile.server-cpu` — CPU-only backend
- `backend/Dockerfile.server-gpu` — NVIDIA GPU backend
- `backend/Dockerfile.server-macos` — Apple Silicon backend

**docker-compose.yml**:
```yaml
services:
  backend:
    build:
      context: .
      dockerfile: Dockerfile.app
    ports:
      - "8000:8000"
    volumes:
      - ./meetings:/app/meetings
    environment:
      - WHISPER_MODEL=base.en
      - WHISPER_DEVICE=cuda
```

### GPU Acceleration Deployment

**Conditional Compilation (build-gpu.sh)**:
- Detects GPU at build time
- Bakes GPU support into binaries
- No runtime GPU detection or fallback

**Build Variants**:
- CUDA build: Requires NVIDIA GPU at runtime
- Metal build: Requires Apple GPU (M1/M2+)
- Vulkan build: Cross-platform but slower
- CPU-only: No GPU required, slowest

---

## 9. Things That Are Easy to Miss

### 1. **GPU Feature Detection is Build-Time, Not Runtime**
- `build-gpu.sh` detects GPU once and compiles for that GPU
- You cannot run a CUDA build on Metal GPU (they're different binaries)
- Users must build on the machine they'll run on, OR build generic CPU version

### 2. **Whisper.cpp is a Submodule**
- `backend/whisper-custom/` is a modified Whisper.cpp repo
- Custom build flags for GPU are baked in
- Rebuilding backend requires re-compiling C++ code (slow)

### 3. **llama-helper is a Sidecar, Not Embedded**
- Lives in `src-tauri/binaries/llama-helper-{target}`
- Spawned on-demand only when summarization runs
- Can add startup latency if LLM model not cached

### 4. **No Hot Reload for Whisper Models**
- Whisper model loaded once at backend startup
- Changing model size requires restart
- Model file is large (100MB-1.5GB) — watch disk space

### 5. **Audio Capture is Cross-Platform via cpal**
- Handling device enumeration, sample rates, channel counts
- Each OS has different quirks (CoreAudio vs ALSA vs WASAPI)
- Test on all three platforms before release

### 6. **Database is SQLite, Single-File**
- No schema migrations framework
- Manual `ALTER TABLE` if schema changes
- Concurrent writes can cause lock contention (design around it)

### 7. **Tauri IPC is JSON-Only**
- Commands must serialize to/from JSON
- Large audio files should be streamed, not passed as JSON
- Use file I/O instead for audio buffers

### 8. **Python Backend Doesn't Scale**
- Single FastAPI process, single thread for Whisper
- Two concurrent transcription requests will queue
- Consider adding job queue (Celery) if scaling needed

### 9. **GPU Memory Usage is Not Managed**
- Whisper.cpp may load entire model into VRAM
- Multiple GPU tasks might OOM
- Document memory requirements clearly

### 10. **No Telemetry, But Log Files Might Leak Info**
- Logs written to disk during development
- Ensure logs don't contain PII or audio paths
- Clean up logs before shipping

---

## 10. Visual Summaries

### System Architecture Diagram

```
┌───────────────────────────────────────────────────────────────────┐
│                         User's Machine                            │
│                                                                   │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │              Clearminutes Desktop App (Tauri + React)            │ │
│  │  ┌───────────────────────────────────────────────────────┐  │ │
│  │  │  React UI                                             │  │ │
│  │  │  - Recording controls  - Transcript display           │  │ │
│  │  │  - Meeting list        - Summary view                 │  │ │
│  │  │  - Settings            - Waveform visualizer          │  │ │
│  │  └────────────────┬────────────────────────────────────┘  │ │
│  │                   │ Tauri invoke()                        │ │
│  │  ┌────────────────▼────────────────────────────────────┐  │ │
│  │  │  Tauri Runtime (Rust)                               │  │ │
│  │  │  - Window management   - Audio capture (cpal)        │  │ │
│  │  │  - File I/O            - HTTP client (reqwest)        │  │ │
│  │  │  - Sidecar management  - Command routing              │  │ │
│  │  └────────────────┬────────────────────────────────────┘  │ │
│  └─────────────────┼──────────────────────────────────────────┘ │
│                    │                                              │
│                    │ HTTP                                         │
│                    │ localhost:8000                              │
│                    ▼                                              │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │        Python FastAPI Backend Process                      │ │
│  │  ┌───────────────────────────────────────────────────────┐ │ │
│  │  │  Routes                                               │ │ │
│  │  │  - POST /transcribe      - GET /meetings              │ │ │
│  │  │  - POST /summarize       - POST /export               │ │ │
│  │  └────────────┬──────────────────────────────────────────┘ │ │
│  │               │                                             │ │
│  │  ┌────────────▼──────────────────────────────────────────┐ │ │
│  │  │  Whisper.cpp Integration                              │ │ │
│  │  │  (C++ binary + GPU acceleration)                       │ │ │
│  │  │  ▶ Detects: CUDA / Metal / Vulkan / CPU               │ │ │
│  │  └─────────────────────────────────────────────────────┘ │ │
│  │               │                                             │ │
│  │  ┌────────────▼──────────────────────────────────────────┐ │ │
│  │  │  SQLite Database                                       │ │ │
│  │  │  - meetings table                                      │ │ │
│  │  │  - transcripts table                                   │ │ │
│  │  │  - summaries table                                     │ │ │
│  │  └─────────────────────────────────────────────────────┘ │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                                                                   │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │  llama-helper Sidecar (Rust, GPU-accelerated)              │ │
│  │  - Spawned on-demand for summarization                     │ │
│  │  - Uses llama.cpp + GPU (CUDA/Metal/Vulkan)                │ │
│  │  - Returns summary text via IPC                            │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                                                                   │
└───────────────────────────────────────────────────────────────────┘
```

### Audio Pipeline (Capture → Transcription → Storage)

```
Audio Input Stream (OS Audio Device)
         │
         ▼
    [cpal] Audio Capture (Tauri)
    - Detects available audio devices
    - Configures sample rate, channels
    - Buffers PCM data in real-time
         │
         ▼
    Buffer → Temp File
    ~/.config/clearminutes/temp_recording.wav
         │
         ▼
    User Clicks "Stop Recording"
         │
         ▼
    [HTTP] Send to Backend
    POST /transcribe { file: "temp_recording.wav" }
         │
         ▼
    [Python] transcript_processor.py
    - Loads audio file (librosa / soundfile)
    - Passes to Whisper.cpp binary
         │
         ▼
    [Whisper.cpp] Speech → Text
    GPU Accelerated (CUDA/Metal/Vulkan)
         │
         ▼
    JSON Response
    { 
      "text": "Meeting transcript here...",
      "segments": [
        { "start": 0.0, "end": 5.5, "text": "Hello" },
        { "start": 5.5, "end": 10.0, "text": "World" }
      ]
    }
         │
         ▼
    [SQLite] Store in DB
    meetings table:   { id, filename, date, duration, ... }
    transcripts table: { id, meeting_id, text, segments_json, ... }
         │
         ▼
    [HTTP Response] Return to Tauri
         │
         ▼
    [React] Display in UI
    Show waveform + transcript with timestamps
```

### Build Pipeline (GPU Detection)

```
./build-gpu.sh (Entry Point)
    │
    ├─► Detect OS: macOS / Linux / Windows
    │
    ├─► Detect GPU:
    │   ├─ macOS:  Check for Metal / CoreML capability
    │   ├─ Linux:  Check for NVIDIA CUDA / AMD Vulkan
    │   └─ Windows: Check for NVIDIA CUDA / AMD Vulkan
    │
    ├─► Run auto-detect-gpu.js
    │   └─ Output: "cuda" | "metal" | "vulkan" | "none"
    │
    ├─► Build llama-helper (Rust)
    │   ├─ cargo build --release --features $FEATURE
    │   └─ Output: target/release/llama-helper
    │
    ├─► Copy binary
    │   └─ cp target/release/llama-helper → src-tauri/binaries/llama-helper-{target}
    │
    ├─► Build Tauri App
    │   ├─ pnpm install
    │   └─ pnpm tauri:build
    │       ├─ Build React (npm run build)
    │       ├─ Build Rust (cargo build --release)
    │       ├─ Bundle platform-specific (dmg / AppImage / msi)
    │       └─ Sign if keychain available
    │
    └─► Output: src-tauri/target/release/bundle/
        ├─ macos/*.dmg
        ├─ linux/*.AppImage
        └─ msi/*.msi
```

### Dependency Tree (Simplified)

```
Clearminutes App (Tauri)
├─ React UI
├─ Tauri Runtime
│  ├─ tokio (async)
│  ├─ cpal (audio capture)
│  ├─ reqwest (HTTP)
│  └─ serde (JSON)
│
├─ Python Backend
│  ├─ FastAPI
│  ├─ sqlite3
│  ├─ numpy
│  └─ Whisper.cpp (C++ subprocess)
│      ├─ CUDA (if available)
│      ├─ Metal (if available)
│      └─ Vulkan (if available)
│
└─ llama-helper Sidecar (Rust)
   └─ llama-cpp-2 binding
      └─ llama.cpp (C++ library)
         ├─ CUDA (if available)
         ├─ Metal (if available)
         └─ Vulkan (if available)
```

---

## 11. How to Navigate This Codebase

### 15-Minute Quick Start

**Goal**: Understand how recording → transcription → display works.

1. **Read** `frontend/src-tauri/src/main.rs` (2 min)
   - Find `#[tauri::command]` handlers
   - Identify `record_audio` and `stop_recording` functions
   - Note how they call Python backend via HTTP

2. **Read** `backend/app/main.py` (2 min)
   - Find FastAPI route `@app.post("/transcribe")`
   - See `transcript_processor.py` import
   - Understand request → Whisper → response flow

3. **Read** `backend/app/transcript_processor.py` (2 min)
   - Understand how Whisper.cpp binary is invoked
   - See JSON parsing of Whisper output
   - Understand segment storage

4. **Read** `frontend/src/App.tsx` or `pages/Recorder.tsx` (2 min)
   - Find `invoke('record_audio', {...})`
   - See UI state management (recording in progress)
   - Find transcription display logic

5. **Skim** `frontend/build-gpu.sh` (2 min)
   - Understand GPU detection flow
   - See how llama-helper is built + copied
   - Recognize build orchestration

6. **Skim** `frontend/src-tauri/tauri.conf.json` (1 min)
   - See window config, security allowlist
   - Find sidecar definition for llama-helper

7. **Skim** `PRIVACY_POLICY.md` (2 min)
   - Confirm data stays local
   - Understand no telemetry
   - Know where files are stored

**Time Remaining**: Explore specific areas as needed.

---

### Deep Dive Paths

#### Path A: Audio Capture Specialist
Want to add multi-device support or improve audio quality?

1. `frontend/src-tauri/src/audio.rs` — How cpal is initialized
2. `frontend/src-tauri/src/main.rs` — Command handlers for device selection
3. Cpal documentation: https://docs.rs/cpal/
4. Test on Windows, macOS, Linux for device enumeration quirks
5. Update Tauri commands to pass device ID

#### Path B: Transcription Pipeline Engineer
Want to add multi-language support or model switching?

1. `backend/app/transcript_processor.py` — Whisper wrapper
2. `backend/app/main.py` — `/transcribe` route
3. `backend/requirements.txt` — Check Whisper dependencies
4. `docs/building_in_linux.md` — Whisper.cpp build specifics
5. Modify `transcript_processor.py` to accept model/language parameters
6. Add database column for language, model metadata

#### Path C: GPU Optimization Specialist
Want to reduce latency or add more GPU backends?

1. `frontend/build-gpu.sh` — Build orchestration
2. `frontend/scripts/auto-detect-gpu.js` — GPU detection logic
3. `llama-helper/Cargo.toml` — Feature definitions
4. `backend/Dockerfile.server-gpu` — Docker GPU build
5. Profile transcription + summarization with flame graph
6. Consider ONNX runtime, TensorRT, or quantization

#### Path D: UI/UX Developer
Want to improve visual feedback or add features?

1. `frontend/src/App.tsx` — Main app structure
2. `frontend/src/pages/Recorder.tsx` — Recording UI
3. `frontend/src/components/` — Reusable components
4. `frontend/tailwind.config.js` — Styling setup
5. Tauri API docs: https://tauri.app/docs/
6. Add visualizer, real-time transcription streaming, etc.

#### Path E: Deployment & Packaging
Want to automate builds, add CI/CD, or support new platforms?

1. `frontend/src-tauri/tauri.conf.json` — App configuration
2. `frontend/build-gpu.sh` — Build script
3. `backend/docker-compose.yml` — Backend deployment
4. GitHub Actions workflows (if present in `.github/workflows/`)
5. Signing/notarization (macOS), Authenticode (Windows)
6. Release artifacts publishing

#### Path F: Database & Storage
Want to add search, export, or analytics?

1. `backend/app/db.py` — Schema definition
2. `backend/app/main.py` — Routes that query DB
3. `scripts/inject_transcript.py` — Data injection utility
4. Plan schema migrations
5. Add indexes for search performance
6. Implement full-text search on transcripts

---

### File Reference by Task

| Task | Start Here |
|------|-----------|
| Record audio | `frontend/src-tauri/src/audio.rs` |
| Transcribe meeting | `backend/app/transcript_processor.py` |
| Summarize transcript | `llama-helper/src/main.rs` |
| Display transcript | `frontend/src/pages/Transcript.tsx` |
| Change Whisper model | `backend/app/main.py` + `transcript_processor.py` |
| Add GPU backend | `frontend/build-gpu.sh` + `llama-helper/Cargo.toml` |
| Deploy to production | `frontend/build-gpu.sh` + `backend/docker-compose.yml` |
| Add new route | `backend/app/main.py` |
| Add new React page | `frontend/src/pages/` + `frontend/src/App.tsx` |
| Debug Tauri commands | `frontend/src-tauri/src/main.rs` |
| Modify database schema | `backend/app/db.py` |
| Export meetings | `frontend/src-tauri/src/main.rs` + `backend/app/main.py` |

---

## 12. Common Development Tasks

### Setup Local Development

```bash
# Clone repo
git clone <clearminutes-repo>
cd clearminutes

# Install frontend deps
cd frontend
pnpm install
cd ..

# Setup Python backend
cd backend
python3 -m venv venv
source venv/bin/activate
pip install -r requirements.txt
cd ..

# Download Whisper model (first time)
cd backend
python3 -c "import whisper; whisper.load_model('base.en')"
cd ..

# Terminal 1: Start backend
cd backend
source venv/bin/activate
python app/main.py

# Terminal 2: Start Tauri dev
cd frontend
pnpm tauri dev
```

### Build Production Desktop App

```bash
cd frontend
./build-gpu.sh
# Output: src-tauri/target/release/bundle/
```

### Run Tests

```bash
# Frontend
cd frontend
pnpm test

# Backend
cd backend
pytest app/
```

### Debug GPU Detection

```bash
cd frontend
node scripts/auto-detect-gpu.js
# Output: cuda | metal | vulkan | none
```

### Profile Transcription Performance

```bash
cd backend
# Manually invoke Whisper on audio file
./whisper-custom/build/bin/whisper audio.wav \
  --model models/ggml-base.en.bin \
  --device cuda
```

---

## 13. Troubleshooting

| Issue | Solution |
|-------|----------|
| Build fails: "llama-helper not found" | Ensure llama-helper/Cargo.toml exists, run `cargo build --release` from llama-helper/ |
| Whisper model not loading | Download manually: `python3 -c "import whisper; whisper.load_model('base.en')"` |
| GPU not detected | Run `node frontend/scripts/auto-detect-gpu.js` to debug |
| Python backend won't start | Check `backend/requirements.txt` installed, port 8000 not in use |
| Tauri window won't open | Check `frontend/src-tauri/tauri.conf.json` paths are correct |
| Audio capture fails | Verify OS permissions (macOS: Settings → Privacy & Security → Microphone) |
| Transcription very slow | Check `WHISPER_DEVICE=cuda` env var set, Whisper model size reasonable |
| Summarization OOMs | Reduce LLM model size or increase system RAM |

---

## 14. Key Takeaways

1. **Privacy First**: All processing happens locally; no cloud calls.
2. **GPU-Accelerated**: Conditional compilation at build time for CUDA, Metal, Vulkan.
3. **Three-Tier Architecture**: React UI → Tauri desktop → Python backend → Whisper/LLM.
4. **Modular**: Whisper (transcription) and llama-helper (summarization) are separate processes.
5. **Cross-Platform**: Supports macOS, Linux, Windows with platform-specific build variants.
6. **SQLite for Storage**: Simple, single-file database; no external DB needed.
7. **Sidecar Pattern**: llama-helper spawned on-demand, keeps memory footprint down.
8. **Build Complexity**: GPU support requires careful feature flagging; test all platforms.

---

**Last Updated**: 2026-03-01  
**For Developers**: Start with the 15-minute quick start, then pick a deep-dive path based on your focus area.


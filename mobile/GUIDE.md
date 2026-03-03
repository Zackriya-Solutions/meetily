# IQ:capture Mobile — Build, Test & Run Guide

## Prerequisites

### System Requirements

| Tool | Version | Check |
|------|---------|-------|
| **Node.js** | 18+ | `node --version` |
| **pnpm** | 8+ | `pnpm --version` |
| **Python** | 3.10+ | `python3 --version` |
| **Xcode** | 15+ (macOS, for iOS) | `xcodebuild -version` |
| **Android Studio** | Hedgehog+ (for Android) | Open Android Studio > About |
| **CocoaPods** | 1.14+ (iOS) | `pod --version` |
| **Java JDK** | 17 (Android) | `java --version` |

### macOS-specific (iOS development)

```bash
# Install Xcode command line tools
xcode-select --install

# Install CocoaPods
sudo gem install cocoapods
# or
brew install cocoapods
```

### Android-specific

1. Install Android Studio from https://developer.android.com/studio
2. In Android Studio > SDK Manager, install:
   - Android SDK Platform 34 (Android 14)
   - Android SDK Build-Tools 34
   - Android SDK Command-line Tools
3. Set environment variables (add to `~/.zshrc` or `~/.bash_profile`):

```bash
export ANDROID_HOME=$HOME/Library/Android/sdk   # macOS
export PATH=$PATH:$ANDROID_HOME/emulator
export PATH=$PATH:$ANDROID_HOME/platform-tools
```

---

## Environment Setup

### 1. Backend Environment Variables

Create or update `/backend/.env`:

```env
# Required for cloud mode
DEPLOYMENT_MODE=cloud
MONGODB_URI=mongodb+srv://<user>:<password>@<cluster>.mongodb.net/
MONGODB_DATABASE=iqcapture

# LLM providers (for summarization)
ANTHROPIC_API_KEY=sk-ant-...
GROQ_API_KEY=gsk_...
OPENAI_API_KEY=sk-...

# Transcription provider
DEEPGRAM_API_KEY=...

# JWT secret (generate a random string)
JWT_SECRET=your-secret-key-here
```

### 2. Mobile Environment Variables

Create `/mobile/.env.local`:

```env
# Point to your backend API
NEXT_PUBLIC_CLOUD_API_URL=http://localhost:5167
```

For device testing (iOS/Android device on same network):

```env
# Use your machine's local IP instead of localhost
NEXT_PUBLIC_CLOUD_API_URL=http://192.168.1.XXX:5167
```

---

## Quick Start

### Step 1: Start the Backend

```bash
cd backend

# Create virtual environment (first time only)
python3 -m venv venv
source venv/bin/activate     # macOS/Linux
# venv\Scripts\activate      # Windows

# Install dependencies
pip install -r requirements.txt

# Start in cloud mode
DEPLOYMENT_MODE=cloud uvicorn app.main:app --host 0.0.0.0 --port 5167 --reload
```

Verify: open http://localhost:5167/docs — you should see the Swagger UI with cloud endpoints (`/api/meetings`, `/api/transcription`, `/api/summarize`).

### Step 2: Install Mobile Dependencies

```bash
cd mobile
pnpm install
```

### Step 3: Build the Web App

```bash
pnpm build
```

This runs `next build` which produces a static export in the `out/` directory. Capacitor serves this directory inside the native shell.

### Step 4: Add Native Platforms (first time only)

```bash
# Add iOS platform
npx cap add ios

# Add Android platform
npx cap add android
```

This creates the `ios/` and `android/` directories with native project files.

### Step 5: Sync Web Assets to Native Projects

```bash
npx cap sync
```

Run this after every `pnpm build` to copy the `out/` directory into the native projects and sync Capacitor plugin configurations.

---

## Running on iOS

### Simulator

```bash
# Build + sync + open Xcode
pnpm build && npx cap sync && npx cap open ios
```

In Xcode:
1. Select a simulator (e.g. iPhone 15 Pro)
2. Press `Cmd+R` to build and run

Or run directly from command line:

```bash
npx cap run ios --target "iPhone 15 Pro"
```

To list available simulators:

```bash
xcrun simctl list devices available
```

### Physical Device

1. Open the project in Xcode: `npx cap open ios`
2. In Xcode, select your team under Signing & Capabilities
3. Connect your iPhone via USB
4. Select your device as the build target
5. Press `Cmd+R`

> Note: You need an Apple Developer account (free or paid) for device testing.

---

## Running on Android

### Emulator

```bash
# Build + sync + open Android Studio
pnpm build && npx cap sync && npx cap open android
```

In Android Studio:
1. Create an AVD (Android Virtual Device) via Device Manager if you don't have one
2. Select the emulator
3. Press the Run button (green triangle)

Or run from command line:

```bash
npx cap run android
```

To list available emulators:

```bash
emulator -list-avds
```

### Physical Device

1. Enable Developer Options on your Android device (Settings > About > tap Build Number 7 times)
2. Enable USB Debugging in Developer Options
3. Connect via USB
4. Run:

```bash
npx cap run android
```

---

## Development Workflow

### Live Reload (Browser)

For rapid iteration on UI and business logic, run the Next.js dev server:

```bash
cd mobile
pnpm dev
```

Open http://localhost:3119 in Chrome. Use Chrome DevTools mobile emulation (`Cmd+Shift+M`) to simulate phone screen sizes.

> Note: Capacitor native plugins (SQLite, Secure Storage, Network) won't work in the browser. The app includes fallbacks (in-memory database, localStorage) for browser development.

### Live Reload (Simulator/Device)

For testing with native plugins, enable the dev server URL in `capacitor.config.ts`:

```typescript
const config: CapacitorConfig = {
  // ... existing config
  server: {
    url: 'http://localhost:3119',  // Uncomment this line
    cleartext: true,                // Uncomment this line
  },
}
```

Then:

```bash
# Terminal 1: Start dev server
pnpm dev

# Terminal 2: Sync config and run on device/simulator
npx cap sync
npx cap run ios   # or npx cap run android
```

The app in the simulator will load from your dev server with hot reload.

> **Important**: Comment out the `server.url` before building for production or TestFlight.

### Typical Edit-Test Cycle

```bash
# 1. Make code changes in mobile/src/

# 2. For browser testing:
pnpm dev   # already running, changes hot-reload

# 3. For native testing:
pnpm build && npx cap sync
npx cap run ios    # or open Xcode and press Cmd+R
```

---

## Testing Checklist

### Auth Flow

- [ ] Register a new account at the login screen
- [ ] Log out and log back in
- [ ] Close and reopen the app — session should persist
- [ ] Verify the Settings screen shows your email and account info

### Offline-First Behaviour

- [ ] **Airplane mode recording**: Enable airplane mode, record a meeting, verify the meeting appears in the list with status "pending_upload"
- [ ] **Offline viewing**: Load meetings while online, then go offline — meetings list and details should still be accessible
- [ ] **Sync on reconnect**: Restore connectivity, wait ~60s (or pull-to-refresh), verify pending items sync
- [ ] **Network banner**: The yellow "You're offline" banner should appear/disappear with connectivity changes

### Recording

- [ ] Tap Record tab, start recording, verify timer counts up
- [ ] Stop recording — meeting should be created in the list
- [ ] On first recording, the app will request microphone permissions (iOS/Android)

### Transcription (requires backend + Deepgram API key)

- [ ] Record and stop a meeting while online
- [ ] Verify status progresses: pending_upload -> uploading -> transcribing -> completed
- [ ] Open the meeting detail — transcript should appear under the Transcript tab

### Summarization (requires backend + LLM API key)

- [ ] Open a completed meeting with a transcript
- [ ] Switch to the Summary tab
- [ ] Tap "Generate Summary"
- [ ] Verify the summary appears after processing

### Sync

- [ ] Create a meeting on mobile, verify it appears in cloud (check MongoDB or backend logs)
- [ ] Pull-to-refresh on the meetings list to trigger manual sync
- [ ] Check the Settings screen for "Last synced" timestamp and pending item count

---

## Project Structure

```
mobile/
├── capacitor.config.ts      # Capacitor native config
├── package.json             # Dependencies and scripts
├── next.config.js           # Static export + webpack config
├── tsconfig.json            # TypeScript config
├── tailwind.config.ts       # Tailwind CSS config
├── postcss.config.js        # PostCSS config
├── .env.local               # Environment variables (create this)
├── out/                     # Built static files (generated by pnpm build)
├── ios/                     # Xcode project (generated by cap add ios)
├── android/                 # Android Studio project (generated by cap add android)
└── src/
    ├── app/                 # Next.js pages
    │   ├── layout.tsx       # Root layout (providers, tab bar)
    │   ├── page.tsx         # Home — meetings list
    │   ├── globals.css      # Tailwind + mobile overrides
    │   ├── record/          # Recording screen
    │   ├── meeting/[id]/    # Meeting detail
    │   ├── settings/        # Settings screen
    │   └── auth/            # Login + register
    ├── components/          # React components
    │   ├── TabBar.tsx       # Bottom navigation
    │   ├── MeetingsList.tsx # Meetings list with pull-to-refresh
    │   ├── MeetingCard.tsx  # Meeting list item
    │   ├── MeetingDetail.tsx # Meeting detail with tabs
    │   ├── TranscriptView.tsx
    │   ├── SummaryView.tsx
    │   ├── RecordingScreen.tsx
    │   ├── NetworkBanner.tsx
    │   ├── SettingsScreen.tsx
    │   └── AuthPrompt.tsx
    ├── contexts/            # React context providers
    │   ├── AuthContext.tsx   # Auth state + token management
    │   ├── SyncContext.tsx   # Online/offline + sync state
    │   └── RecordingContext.tsx
    ├── hooks/               # Custom hooks
    │   ├── useTranscription.ts  # Polls transcription status
    │   └── useSummarization.ts  # Polls summary status
    ├── services/            # Data + API layer
    │   ├── authService.ts   # Auth API calls + token storage
    │   ├── database.ts      # Local SQLite (offline-first)
    │   ├── syncService.ts   # Background sync engine
    │   ├── meetingRepository.ts  # Meeting CRUD (local-first)
    │   ├── transcriptionService.ts
    │   ├── summarizationService.ts
    │   ├── usageService.ts  # Usage event tracking
    │   └── deviceService.ts # Device ID generation
    └── types/
        └── index.ts         # Shared TypeScript types
```

---

## Backend Cloud Endpoints (for reference)

The backend must run with `DEPLOYMENT_MODE=cloud` to expose these:

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/auth/register` | Register new user |
| POST | `/api/auth/login` | Login, returns JWT tokens |
| POST | `/api/auth/refresh` | Refresh access token |
| POST | `/api/meetings` | Create meeting |
| GET | `/api/meetings` | List meetings (paginated) |
| GET | `/api/meetings/{id}` | Get meeting |
| PUT | `/api/meetings/{id}` | Update meeting |
| DELETE | `/api/meetings/{id}` | Soft-delete meeting |
| POST | `/api/meetings/sync` | Push/pull sync |
| POST | `/api/transcription/upload` | Upload audio for transcription |
| GET | `/api/transcription/{id}/status` | Poll transcription status |
| GET | `/api/transcription/quota` | Check quota |
| POST | `/api/summarize` | Start summarization |
| GET | `/api/summarize/{id}/status` | Poll summary status |
| POST | `/api/usage/events` | Submit usage events |

---

## Troubleshooting

### "Module not found" errors during build

```bash
cd mobile
rm -rf node_modules .next out
pnpm install
pnpm build
```

### Capacitor plugin not working in browser

Expected. Native plugins (SQLite, Secure Storage, Network) only work on iOS/Android. The app falls back to in-memory storage and localStorage in the browser.

### iOS build fails with signing error

Open `mobile/ios/App/App.xcworkspace` in Xcode, go to the App target > Signing & Capabilities, and select your development team.

### Android build fails with SDK error

Ensure `ANDROID_HOME` is set and you have SDK Platform 34 installed. Run:

```bash
$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager "platforms;android-34" "build-tools;34.0.0"
```

### Backend returns 401 Unauthorized

Check that you've registered an account and are passing the JWT token. The mobile auth context handles this automatically — verify you're logged in.

### Transcription fails

Check that:
1. `DEEPGRAM_API_KEY` is set in backend `.env`
2. The audio file is a supported format (m4a, wav, mp3, webm, mp4, ogg, flac)
3. The file is under 100MB

### Meetings don't sync

1. Check the backend is running in cloud mode (`DEPLOYMENT_MODE=cloud`)
2. Check MongoDB is accessible (verify `MONGODB_URI` in `.env`)
3. Pull-to-refresh on the meetings list to force a sync
4. Check the browser/device console for `[SyncService]` log messages

### Hot reload not working on simulator

Make sure `server.url` is uncommented in `capacitor.config.ts` and points to your dev server. Then re-run `npx cap sync`.

---

## Building for Release

### iOS (TestFlight)

1. In Xcode, select "Any iOS Device" as the build target
2. Product > Archive
3. In the Organizer, click "Distribute App"
4. Choose "App Store Connect" > Upload
5. In App Store Connect, submit to TestFlight for testing

### Android (Play Store / APK)

In Android Studio:
1. Build > Generate Signed Bundle / APK
2. Choose "Android App Bundle" for Play Store or "APK" for direct install
3. Create or select a keystore
4. Build the release variant

Or from command line:

```bash
cd mobile/android
./gradlew assembleRelease    # APK
./gradlew bundleRelease      # AAB for Play Store
```

The output will be in `mobile/android/app/build/outputs/`.

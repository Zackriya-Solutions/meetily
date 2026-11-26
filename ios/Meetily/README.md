# Meetily iOS

This is the first native iOS surface for Meetily. It is intentionally separated from the existing Tauri desktop client while the mobile architecture is established.

## Current slice

- SwiftUI meeting list
- Pull-to-refresh and meeting detail view
- HTTP client for the existing backend endpoints:
  - `GET /get-meetings`
  - `GET /get-meeting/{meeting_id}`
- In-app backend URL configuration for simulator and physical-device testing

## Open in Xcode

Open `Meetily.xcodeproj` in Xcode 16 or later, choose an iOS 17+ simulator, and run the shared `Meetily` scheme. `Package.swift` is also included for source-level validation and lightweight tooling.

Start the existing backend from `backend/` before opening the app. The default URL is `http://127.0.0.1:8000`. On a physical iPhone, use the computer's LAN IP address and make sure the backend is reachable from the device.

## Validation on this checkout

The repository checkout has Swift and the command-line tools, but not the full Xcode installation. The Swift package can still be type-checked with `swift build` on macOS; simulator and signing validation requires Xcode.

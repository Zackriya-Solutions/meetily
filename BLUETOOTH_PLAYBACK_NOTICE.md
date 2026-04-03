# Bluetooth Playback Warning

The desktop app includes playback-device detection and can warn when Bluetooth audio output is active during recording review.

## What The Warning Means

- The warning is about playback review.
- It does not mean the recording file is definitely corrupt.
- It is a prompt to compare playback on built-in speakers or wired headphones before assuming the capture path failed.

## What The Code Actually Does

- Detects the active output device.
- Flags Bluetooth playback devices.
- Shows a warning banner in the UI during playback review.

## Practical Guidance

If a recording sounds wrong during playback:

1. Try built-in speakers.
2. Try wired headphones.
3. Compare results before treating the recording as bad input data.

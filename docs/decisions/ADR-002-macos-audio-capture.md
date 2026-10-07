# ADR-002 — macOS audio capture: ScreenCaptureKit, one stream, two outputs

Status: Proposed — confirm with the Milestone 2 spike · 2026-10-05

## Context
Aura must capture the seller (microphone) and the meeting (system audio) as separate, time-aligned streams, without virtual audio drivers.

## Decision
One `SCStream` with `capturesAudio` and `captureMicrophone`, consuming `.audio` and `.microphone` outputs separately. Minimum macOS 15.0. Aura's own windows and audio are excluded from the stream.

## Alternatives
- **Core Audio process taps** (macOS 14.2+) for system audio plus `AVAudioEngine` for the microphone — avoids screen-capture semantics, but two APIs and two clocks.
- **Virtual audio device** (BlackHole-style) — requires installing a driver and changes the user's audio routing. Rejected.
- **Meeting-app bots or SDKs** — per-vendor, visible to participants, no coverage of in-person or phone calls. Rejected for the MVP.

## Consequences
- Requires the Screen & System Audio Recording permission, the heaviest prompt macOS has. If pilot users balk, revisit process taps.
- Echo without headphones is unsolved by this choice; the spike must decide between voice-processing input and correlation-based suppression.
- Stable code signing is needed from Milestone 2 so permission grants survive rebuilds.

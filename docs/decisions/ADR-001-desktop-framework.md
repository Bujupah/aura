# ADR-001 — Desktop framework: Tauri 2 with native panels

Status: Accepted · 2026-10-05

## Context
Aura needs a small always-on-top overlay, deep macOS integration (ScreenCaptureKit, AppKit panels, Keychain), a systems-language core for audio and state, and fast UI iteration. macOS first; Windows later.

## Decision
Tauri 2. React + TypeScript for presentation only; Rust for the core; Swift where an Apple framework requires it. Aura's windows are converted to non-activating `NSPanel`s with `tauri-nspanel`.

## Alternatives
- **Native SwiftUI/AppKit** — best platform fit, but no path to Windows and slower UI iteration.
- **Electron** — mature, but a Node main process is the wrong home for realtime audio and secrets, and the footprint is large for an overlay.
- **Plain Tauri windows** — cannot stay non-activating or reliably sit above full-screen apps.

## Consequences
- Transparent windows need `macOSPrivateApi` (private WebKit API): no Mac App Store, and a risk on OS updates. Acceptable for internal distribution.
- `tauri-nspanel` is a git dependency. Pin to a reviewed revision before distribution.
- All AppKit calls must happen on the main thread; the shell funnels them through one `reconcile` function.

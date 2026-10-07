# ADR-007 — Screen-share privacy: layered mitigation, no guarantee

Status: Accepted · 2026-10-05

## Context
Sellers share their screens. Aura's overlay shows private guidance. Apple documents `NSWindow.SharingType.none` as a legacy constant that macOS no longer uses and advises against relying on it to hide content; ScreenCaptureKit-based capture on macOS 15+ is reported to include such windows.

## Decision
Treat invisibility as impossible to guarantee and say so. Provide layers: a best-effort content-protection flag; exclusion from Aura's own capture; Presentation Safe Mode; a global emergency hide that the state machine enforces; guidance to share a window rather than a display; and an abstraction for future per-app share detection.

## Alternatives
- **Claim the overlay is invisible to screen sharing** — false on current macOS.
- **Private APIs or window-server tricks** — fragile, likely to break, and the kind of unreliable hack the brief forbids calling a guarantee.
- **Second-device companion (phone or tablet)** — the only true guarantee; out of MVP scope but worth revisiting.

## Consequences
- Product copy and onboarding must teach window sharing and the hide shortcut.
- Emergency hide must never be defeated by a feature: while hidden, only restore is honoured (tested in `aura-core`).

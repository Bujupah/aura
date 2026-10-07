# ADR-003 — Realtime transport: GPT-Live over WebSocket, one session per audio source

Status: Accepted for development · 2026-10-05 (supersedes the transcription-session draft of the same day)

## Context
Transcription latency is the product. Aura captures PCM natively in a Rust/Swift process, needs each speaker's stream kept separate, and must keep provider keys away from the webview. The project owner chose GPT-Live as the realtime model.

## Decision
The Rust core (`crates/aura-live`) opens one GPT-Live WebSocket session per audio source — microphone and system audio — with client delegation and `store: false`. Audio goes directly from the device to the provider as continuous PCM16 at 24 kHz. Output audio is discarded in code.

## Alternatives
- **Realtime transcription sessions** — purpose-built for passive transcription and likely cheaper for the meeting stream; no delegation signal and no conversational lane. Kept as a drop-in for the meeting stream if cost requires.
- **WebRTC** — the documented route for clients holding short-lived credentials, but built around browser media tracks; Aura's audio is already PCM in a native process.
- **Proxy all audio through the gateway** — full control and audit, one extra hop on the hot path. Kept as a configuration option.
- **Responses delegation** — less to build, but the provider's managed loop would run backend work outside Aura's authorization and verification.

## Consequences
- Verified end to end against the live API (see ai.md §3.1): accurate technical transcription at roughly 0.4 s lag.
- GPT-Live cannot be prompted into silence; it will speak and delegate on the meeting stream. Aura ignores the speech and treats delegations as signals.
- Two always-on sessions are billed per second each. Meeting length × 2 is the cost model to validate.
- Development uses a long-lived token from the environment. Whether a short-lived credential can open a Live WebSocket natively is unconfirmed; production may need the gateway proxy or WebRTC.
- Customer audio goes from the device straight to the AI provider. This needs sign-off on data-processing terms.
- The API marks no turn boundaries; Aura segments turns itself.

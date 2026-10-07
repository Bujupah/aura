# ADR-008 — Data retention: five classes, policy-driven, no raw audio at rest

Status: Accepted · 2026-10-05

## Context
Meeting content is sensitive and regulated differently across regions and customers. Retention must be an enterprise decision, not a hardcoded behaviour.

## Decision
Five data classes with independent retention: transient audio, transcript, structured meeting state, generated artifacts, audit log. Raw audio lives only in short memory buffers unless a recording policy explicitly enables persistence. Local data is encrypted in SQLite with a Keychain-held key. The seller can choose "Do Not Save Session" and "Delete Meeting". Organization policy can only tighten these defaults from the client's point of view.

## Alternatives
- **Keep audio for re-transcription and debugging** — useful, but the largest privacy liability in the product.
- **Cloud-first storage** — easier sync and analytics; worse default privacy posture. Left as a policy option.

## Consequences
- Bugs in transcription cannot be reproduced from audio; synthetic fixtures and event replay carry that load.
- The audit log must be designed to be useful without containing content.

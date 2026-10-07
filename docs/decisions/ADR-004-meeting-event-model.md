# ADR-004 — Meeting event model: append-only typed log, state as a fold

Status: Accepted · 2026-10-05

## Context
UI, storage, evaluation and demos all need the same account of what happened in a meeting, and development cannot depend on live meetings.

## Decision
Every meaningful occurrence is a typed event in an append-only, per-meeting log with a gap-free sequence number and a monotonic capture timestamp. Meeting state is derived by folding the log. The UI subscribes to state and events, never to model responses.

## Alternatives
- **Mutable state written directly by AI responses** — simpler at first, impossible to replay or audit.
- **Full event-sourcing framework** — more machinery than a single-user desktop app needs.

## Consequences
- `aura replay <meeting-id>` is cheap: feed recorded transcript events through the same code.
- Model calls are non-deterministic, so replay has two modes: recorded responses (UI tests, demos) and live re-run (evaluation).
- Event schemas are a compatibility surface and need versioning from the first release that persists them.
- The shell already follows the pattern in miniature (command → pure transition → reconcile → event).

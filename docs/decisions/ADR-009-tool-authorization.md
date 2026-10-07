# ADR-009 — Tool authorization: enforced by the orchestrator, never by the model

Status: Accepted · 2026-10-05

## Context
Aura will eventually act — CRM updates, emails, meetings. Models read untrusted text (customer speech, documents) and can be steered by it.

## Decision
Every tool declares its schema, required permissions, risk class (`READ`, `PREPARE`, `WRITE`, `EXTERNAL`), approval requirement, allowed data classifications and audit behaviour. Models emit requests; the orchestrator validates, authorizes, obtains approval where required, executes and audits. `READ` and `PREPARE` may run automatically; `WRITE` requires confirmation; `EXTERNAL` always requires explicit confirmation of the exact content. The MVP ships no `WRITE` or `EXTERNAL` tools.

## Alternatives
- **Let the model decide when to ask for approval** — a prompt injection away from acting unprompted.
- **Blanket per-session approval** — convenient, and removes the control exactly when it matters.

## Consequences
- More friction for write actions, deliberately.
- Third-party tool servers are proxied and allowlisted; their output is untrusted data.

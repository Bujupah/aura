# ADR-005 — AI model responsibilities: fast lane and deep lane

Status: Accepted · 2026-10-05

## Context
Useful help must arrive within a couple of seconds, but good technical reasoning takes longer and costs more.

## Decision
Two lanes with separate budgets. A low-latency lane, built on GPT-Live with client delegation, transcribes both streams, signals when something needs an answer, and converses with the seller; lightweight classification and extraction run on its finalized turns. A reasoning lane runs only on triggers and works from compact state, not the transcript. Both write to the meeting state; one prioritizer decides what is shown. Models sit behind interfaces chosen by configuration.

## Alternatives
- **One large prompt with everything** — explicitly ruled out by the brief; slow, expensive, hard to evaluate.
- **Reasoning model on every turn** — latency and cost scale with talk time.

## Consequences
- Two lanes can disagree; the reasoning lane reconciles state, and its output supersedes fast-lane inferences but never observations.
- Each lane degrades independently.
- Trigger policy becomes a tunable that needs its own evaluation. GPT-Live's delegation events are one input to it, not the whole policy.
- With client delegation Aura must keep the conversation context itself; the delegation event carries no task text.

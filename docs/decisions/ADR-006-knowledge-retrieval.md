# ADR-006 — Knowledge retrieval: curated product graph plus hybrid document search

Status: Proposed · 2026-10-05

## Context
Sellers need answers grounded in approved BMC material, with sources, and the portfolio changes faster than the app ships.

## Decision
Two sources used together: a small reviewed product graph in YAML (products, capabilities, integrations, relationships, competitors, terminology), and a document index queried by metadata filter → keyword + vector search → rerank. Results are evidence objects with provenance. Verification state is computed from evidence by code.

## Alternatives
- **Vector RAG only** — misses exact capability and version matches; relationships are implicit.
- **Graph database** — premature at this size; revisit when YAML stops being reviewable.
- **Model knowledge alone** — unacceptable for compatibility, licensing, roadmap, security or pricing.

## Consequences
- Someone must own the approved corpus and its refresh. This is an organizational dependency, not a technical one.
- Audience filtering happens server-side before ranking.
- The YAML graph is hand-maintained; keep it small and test it.

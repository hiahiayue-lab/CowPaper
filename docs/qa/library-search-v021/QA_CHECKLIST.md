# CowPaper v0.2.1 Library Search — QA Preparation

Status: **QA PREPARATION**. Baseline is `origin/main` / `49ea75fd3002a080ebb3c0e67acf23d11655a7b5`. This document deliberately does not claim final PASS: the current baseline has no Library Search implementation, and final QA is blocked until backend, UI, and design handoffs exist.

## Preparation completed

- Audited `origin/spike/library-search`; it contains research notes only.
- Audited the current Rust/SQLite test structure and existing Library tests.
- Added the synthetic A–L fixture contract in `fixtures.json`; it covers the requested English/Chinese titles, Note ESG, Chinese/English abstracts, author/journal/DOI, AI+ESG collections, two tags, parent/child, an override-only title match, multi-Collection dedup, zero-count tag behavior, and recommendation isolation.
- Kept all additions under `docs/qa/library-search-v021/`; no product code, schema, migrations, UI behavior, release metadata, or v0.2.0/v0.1.4 files were changed.

## Fixture coverage map

| Fixture/scenario | Coverage |
|---|---|
| A | English AI title; author, journal, DOI, governance content; AI collection; two tags |
| B | Chinese title/abstract; `人工智能` and Chinese normalization symmetry |
| C | Note containing ESG; ESG collection |
| D | Chinese abstract containing ESG/治理; content scope |
| E | English governance abstract; membership in AI and ESG; two tags; OR/dedup |
| F | Author/journal/DOI metadata matches |
| G + H | Parent/child identity and visible-result dedup |
| I | Effective title override match only; canonical paper isolation |
| J | Multi-Collection membership and `papers.id` dedup |
| K | Zero-count tag visible/dimmed/drag-target behavior |
| L | Recommendation snapshot isolation |
| scenarios | Collection OR, Tag AND, combined scope, paper.id dedup, parent/child dedup, effective-value semantics |

## Backend handoff audit checklist

Run against the handed-off backend branch and its final `main` merge, using the fixture contract. Record command/test names and observed result for every row.

| Area | Required assertion | Evidence to capture |
|---|---|---|
| v19 migration | Fresh DB and v18→v19 upgrade create the exact search tables/indexes; rerun is idempotent and legacy rows survive | migration test output; `sqlite_master` snapshot; row counts |
| FTS5 | Bundled SQLite reports FTS5; insert/update/delete and rebuild work on macOS; Windows bundled SQLite path has the same behavior | `pragma compile_options`; CRUD + rebuild results on both platforms |
| Unicode | `unicode61` tokenization is stable; Chinese application bigram normalization is symmetric for stored/query forms (`人工智能` ↔ `人工 智能` as specified by implementation) | normalization truth table; explain/query result IDs |
| Query scopes | Quick, Metadata, Content use the documented effective values and do not leak fields across scopes | A–I per-scope matrix |
| Collection OR | AI OR ESG returns the union once, including multi-member E | ordered unique IDs; no duplicate rows |
| Tag AND | Core AND Review returns only A/E; zero-count tag remains visible as a facet | IDs and facet counts |
| Combined filters | Collection + Tags + Search composes with AND between dimensions and search | scenario expected IDs |
| Dedup | Joins and multiple collections/tags collapse by `papers.id`; parent+child resolves to one canonical visible paper | duplicate-row and G/H assertions |
| Sync/rebuild | Index sync is incremental, rebuild is complete, stale/deleted rows disappear, and failed sync does not leave partial index state | before/after counts; rollback test |
| Transactions | Search index writes participate in the same transaction as Library changes; injected failure leaves DB and index consistent | failure injection + consistency query |
| Performance | Cold/warm search latency and rebuild time are recorded on a representative fixture; no N+1 query explosion | timings, query plan, threshold agreed at handoff |
| Recommendation isolation | Search/filter/index/rebuild never changes recommendation rows, scores, order, or cycle state | before/after serialized recommendation snapshot |

## UI handoff checklist

- One Search Box in the Library workspace; no competing web-like search surfaces.
- Suggestions are deterministic, scoped, dismissible, and keyboard navigable.
- Keyboard: focus, typing, Escape, arrow navigation, Enter, tab order.
- IME: composition start/update/end does not issue premature queries; Chinese input is searchable after composition end.
- Empty query restores normal scope; zero-result state is explicit and non-destructive.
- Quick / Metadata / Content scope controls expose effective-value behavior clearly.
- Collection OR, Tag AND, and combined filters preserve selected state while typing.
- Zero-count tags remain visible, visibly dimmed, and remain valid drag targets.
- Result identity is stable under multiple collections/tags and parent/child records.
- Effective override-only title I is discoverable through effective search while canonical data and recommendation fields remain unchanged.

## Design regression checklist

Review screenshots at the supported widths and a narrow window:

- Sidebar hierarchy, indentation, counts, and selected state remain legible.
- Toolbar/Search Box aligns with the existing Library hierarchy and does not steal excessive vertical space.
- Table columns, row density, separators, and selection remain quiet and scannable.
- Inspector hierarchy remains clear; search/filter state does not flatten or reorder metadata unexpectedly.
- No web-like cards, gradient/glow treatment, oversized empty states, or unnecessary animation.
- Focus/hover/drag states are restrained and retain accessible contrast.
- Chinese text does not cause clipping, unexpected wrapping, or layout drift.

## Narrow audit status

**Not started / waiting for backend handoff.** The narrow audit scope is intentionally limited to: v19 migration, FTS index, transaction safety, Chinese normalization, effective values, Collection OR, Tag AND, `papers.id` dedup, parent/child handling, and recommendation isolation. Do not broaden this audit into unrelated metadata, annotations, release, or v0.2.0/v0.1.4 work.

## Ready for final QA

**Not ready for final QA.** Preparation materials are ready. Final QA can start only after all three handoffs are present (backend, UI, design), followed by a fresh fetch of final `main` and an independent rerun of the narrow audit plus the UI/design checklist. No merge, tag, release, cleanup, or product-code mutation was performed by this QA preparation.

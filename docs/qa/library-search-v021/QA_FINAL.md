# CowPaper v0.2.1 Library Search — Independent QA

Status: **AUTOMATED QA PASS; READY FOR MANUAL RUNTIME QA**

Candidate under test: `main` at `9c13b3c` after the v0.2.1 version metadata commit. The frozen v0.2.0 tag remains at `49ea75fd3002a080ebb3c0e67acf23d11655a7b5`.

## Automated evidence

| Check | Result |
|---|---|
| `cargo test --locked` on the rebased QA worktree | PASS — 214 passed, 0 failed, 5 ignored |
| `test_library_search_v19_fts_filters_effective_values_and_sync` | PASS |
| `rc5_collection_scope_facets_global_tags_and_and_filter` | PASS — parent/child scope and zero-count tag visibility |
| v13/v14 legacy migration preservation tests | PASS |
| v19 migration idempotence | PASS |
| bundled SQLite FTS5 capability | PASS |
| Vanilla TypeScript search state tests | PASS — `librarySearch tests passed` |
| `git diff --check` | PASS |

## Contract coverage

The A–L fixture contract in `fixtures.json` covers English and Chinese title/abstract search, note and metadata search, effective overrides, Collection OR, Library Tag AND, parent/child scope, canonical `papers.id` de-duplication, zero-count Tag visibility/drag target behavior, and recommendation isolation. The Rust search test verifies the core rows and sync/rebuild behavior without changing canonical Paper or recommendation data.

## Final QA boundary

No final manual-runtime PASS is claimed here. The remaining checks require launching the exact v0.2.1 candidate against the preserved user database: visual density/alignment, one Search Box interaction, IME, Paper Table/Inspector, PDF attachment, Collection/Tag drag behavior, Discovery/recommendation regression, updater no-downgrade behavior, and DB 18→19 preservation.

This QA branch contains preparation/evidence documents only. It does not modify product code, release metadata, the v0.2.0 tag/release, or v0.1.4.

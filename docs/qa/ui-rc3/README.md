# CowPaper 0.2.0 RC3 UI evidence

Base: `da02a26a500ec151367655df168930b9e158ae7e`; branch: `feat/library-ui-rc3`.

## Scope and limitations

Actual frontend rendering in an isolated headless Chrome profile, with 27 synthetic library records and an explicitly isolated in-memory command fixture. The harness bundles the worktree's actual `main.ts`, replaces only Tauri invoke/event adapters and startup, and serves the worktree HTML/CSS. It does not use the installed application, user database, PDFs, settings, network AI, or production command implementation. No fixture or substitute backend is shipped in `app/`.

Reference inspected: the user-supplied `codex-clipboard-fca21aaf-8c5d-4f9b-bcac-a0e11367acd9.png`. Manual comparison confirms the continuous panels, narrow sidebar, compact rows, gray selection, thin separators and Inspector typography are materially closer; this is not a claim of pixel identity. Main task provisionally accepted the 1512 screenshot.

## Build checks

- `npx --no-install tsc --noEmit`: exit 0.
- `npm run build`: exit 0 (Vite production build).
- `git diff --check`: exit 0.

## Measured layout

All screenshots are actual browser captures at CSS pixel dimensions stated in their filenames, device scale factor 1.

| Window | Sidebar | Inspector | List client/scroll width | Five columns visible |
| --- | --- | --- | --- | --- |
| 1440 × 982 | 168 | 300 | 971 / 971 | yes |
| 1512 × 982 | 168 | 300 | 1043 / 1043 | yes |
| 1536 × 982 | 168 | 300 | 1067 / 1067 | yes |

At 1000 pixels Inspector auto-collapses; 740 additionally hides Note; 600 additionally hides Authors. Title/Journal/Year remain visible, with no document or list horizontal overflow. At narrow widths selecting a paper reopens Inspector as a closable overlay. `layout.json` contains exact column measurements.

## Frontend event verification

`interactions.json` records 21 successful fixture assertions. These cover missing/canonical Chinese title controls, missing abstract isolation from legacy canonical text/AI summary, create child Collection with parent, add/remove membership, reject parent deletion with children, create/delete Library Tag and Collection through Inspector, retained Paper identity, drag hover and ADD command parameters, column resizing/reset, and narrow Inspector open/close.

The title translation test verifies `translate_library_title {paperId, model}` and its error UI; the fixture deliberately rejects AI execution, so this is not successful translation evidence. `command-calls.json` records frontend command requests against the fixture, not Rust/SQLite execution.

## Integration contract / pending runtime verification

- `add_paper_to_collection {paperId, collectionId}` and `add_paper_library_tag {paperId, tagId}` are the Metadata agent's additive backend commands.
- `translate_library_title {paperId, model}` writes only the Library Chinese title override; real AI success and persistence remain pending Metadata integration and main-task runtime QA.
- v17 effective Journal/Publisher/PublicationDate/Volume/Issue/Pages are displayed; `pages` stays a string including e-locators. New replacement overrides are preserved by inline edits.
- Abstract reads only `effectiveAbstract` / `effectiveChineseAbstract`; null never falls back to canonical legacy text or AI summary. Real legacy_unverified filtering and provenance behavior remain pending backend runtime QA.
- Parent-child delete enforcement in Rust, real Collection/Tag persistence and recommendation isolation remain pending backend integration/runtime QA.
- No Search, Annotation, recommendation/scoring schema, storage backend, Release, tag or main-branch mutation is part of this change.

Temporary reproducibility harness: `/tmp/cowpaper-rc3-qa/{prepare.cjs,fixture.js,interactions.mjs,check.mjs}`. It is intentionally outside production sources.

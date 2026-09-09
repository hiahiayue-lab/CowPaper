# CowPaper v0.2.1 RC2 Library Search QA

Status: **QA preparation**  
Candidate baseline: `8eacbffc5bb12b83fc54a2ccc8b2da3602c6ba97`  
Fixture contract: [`fixtures-rc2.json`](./fixtures-rc2.json), extending [`fixtures.json`](./fixtures.json)

This checklist covers the ten RC2 acceptance paths requested for Library Search. It is a QA artifact only: it does not change database schema, implement annotations, seed production data, create a release, or create a tag.

## Reproducible setup

1. Start from the candidate SHA above and verify the worktree has no unrelated changes.
2. Use an isolated in-memory/test database or a disposable copy of a user database. Never import A–L into the user's live database.
3. Load the synthetic records A–L and the collection/tag relationships in `fixtures.json`. Preserve canonical IDs, with G as the parent identity for H and J belonging to both AI and ESG.
4. Run the automated checks below. For UI-only rows, launch the exact candidate build and record the candidate SHA, fixture database identifier, viewport, query, selected tokens, result IDs, and a screenshot.

## Automated checks

```text
cd app
npm run test:search
npm run test:search:rc2
cd ..
cargo test --locked rc2_library_search_runtime_mixed_language_and_incremental_index
```

The Rust command runs from `app/src-tauri` when invoked directly. The full regression command is:

```text
cd app/src-tauri
cargo test --locked
```

Expected RC2 evidence:

- `librarySearch RC2 tests passed (TEST 1-10)`.
- `rc2_library_search_runtime_mixed_language_and_incremental_index` passes.
- Existing Library Search regression `test_library_search_v19_fts_filters_effective_values_and_sync` passes.
- `git diff --check` is clean.

### Preparation-run evidence (2026-09-09)

- PASS — `npm run test:search:rc2` (TEST 1–10) and the prior `npm run test:search`.
- PASS — `cargo test --locked library_search`: 2 passed, 1 ignored benchmark.
- PASS — `cargo test --locked rc2_library_search_runtime_mixed_language_and_incremental_index`.
- PASS — RC2 fixture JSON parse and `git diff --check`.
- NOT RUN — `npm run build` could not start in this clean worktree because the local Node dependencies are absent (`tsc: command not found`).
- ENVIRONMENT BLOCKER — full `cargo test --locked` reached 220 tests but failed in the existing DeepSeek HTTP simulation tests (`title_translation_*` and `test_title_only_*`); all Library Search tests, including the new RC2 runtime test, passed. Re-run those network/simulation tests separately on the main QA environment before release sign-off.

## TEST 1–10

| ID | Reproduction | Pass criteria | Evidence / automation |
|---|---|---|---|
| TEST 1 | Focus the single Library Search box; select Collection `AI`; continue typing `治理`; select Library Tag `Core`. | Collection and Tag remain locked scope tokens, `治理` remains editable text, and no token selection clears the other state. | Pure state test in `librarySearch.rc2.test.ts`; capture one UI screenshot showing both tokens plus text. |
| TEST 2 | Run `AI OR ESG`; then `Core AND Review`; then `AI + Core + Review + governance`. | Results are respectively A–L, A/E, and A/E. OR applies within Collections; AND applies within Tags; dimensions compose with AND. | Pure state test plus backend `test_library_search_v19_fts_filters_effective_values_and_sync`. |
| TEST 3 | Select both `AI` and `ESG`, search `Multi Collection Identity`. | J appears once, with one canonical `paper.id`; no duplicate row or duplicate result count. | Pure state test; backend regression covers multi-Collection OR de-dup. |
| TEST 4 | Open parent Collection scope containing G/H and search `Parent Record`. | G/H resolve to one visible canonical result G; H is not rendered as a second paper. | Pure state test for descendant expansion and canonical-id de-dup; verify one UI row. |
| TEST 5 | Select Collection `AI` in the Library sidebar, open Search, type `governance`. | Sidebar scope is automatically represented as a locked Collection token with descendants enabled; typing does not replace it. | Pure suggestion/state contract; manual UI verification required because sidebar state lives in `main.ts`. |
| TEST 6 | Inspect the token row and suggestion list after selecting Library scopes. | Only Library Collection/Library Tag tokens are shown; no web/provider/external duplicate chips; one chip per logical Collection/Tag ID. Canonical Paper identity remains `papers.id`. | Pure suggestion uniqueness test; manual DOM/UI inspection required. |
| TEST 7 | In a scoped facet view, find `Zero Count` from fixture K. | Tag remains visible, count is 0, appearance is dimmed, it is clickable/selectable, and it remains a valid drag target. | Pure suggestion test; backend facet regression covers zero-count visibility; manual drag verification required. |
| TEST 8 | Add a fixture Paper to Library; search immediately. Change its effective title/note; rename its Library Tag; remove it from Library. | New values are searchable without manual rebuild, old values disappear, removed Paper disappears from all search results, and canonical Paper/recommendation fields remain intact. | Rust `rc2_library_search_runtime_mixed_language_and_incremental_index`; existing v19 sync test. |
| TEST 9 | In the actual candidate runtime, run `人工智能`, `AI 治理`, `ESG 平台`, and `人工智能 AI 治理 ESG 平台`. | Each query returns the intended mixed-language fixture; Han normalization is symmetric and terms are ANDed. | Rust runtime test; repeat once in the UI and capture committed query/result IDs. |
| TEST 10 | Start Chinese IME composition; press Enter, Escape, and arrow keys; commit; then execute. | No premature query, close, or suggestion navigation during composition. After compositionend, committed text is searchable and Enter executes once. | Pure state test; manual candidate-runtime IME verification required. |

## Manual runtime protocol

For each UI-only assertion, use the same isolated fixture database and the same candidate build. Reset Search between tests with the clear control, then record:

```text
TEST ID · candidate SHA · viewport/OS · sidebar scope · visible tokens · raw text · mode · result paper IDs · pass/fail · screenshot path
```

For TEST 8, take a before/after snapshot of the canonical Paper row and any recommendation row before mutating Library metadata. A passing search update must not be used as evidence that canonical metadata or recommendation state changed.

For TEST 10, use a real Chinese IME rather than pasted text. Confirm that the input event stream during composition does not execute a backend query; only the committed value after `compositionend` may update the result.

## Known boundary / Codex-main现场验证

The following cannot be proven by the framework-free tests and must be checked in the exact candidate runtime:

- visual token locking and continued typing in one Search Box;
- sidebar auto-token insertion;
- absence of external/duplicate chips in the rendered UI;
- zero-count Tag drag target and pointer feedback;
- mixed-language query behavior through the real Tauri command and UI hydration;
- real IME composition event ordering and single execution after commit.

### Baseline observation for Codex-main

Read-only inspection of this baseline finds the scope state in `librarySearch.ts`, but the Library toolbar render path currently exposes a mode selector, one text input, and a clear button; it does not visibly render Collection/Tag token chips. Also, empty-query suggestion refresh returns no suggestions. Therefore TEST 1, TEST 5, and TEST 6 are explicit handoff checks and should remain **not passed** unless the candidate runtime supplies the token UI and its interaction path.

Do not mark RC2 final PASS until these rows have runtime evidence. This preparation does not claim a release decision.

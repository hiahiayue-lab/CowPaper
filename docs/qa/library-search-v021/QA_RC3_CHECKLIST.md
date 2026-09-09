# CowPaper v0.2.1 RC3 — Independent QA Preparation

Status: **QA PREPARATION / READY FOR INDEPENDENT QA**
Preparation baseline: 5b8355d70a72a49974586e940403176cf794bf18
Search matrix: SEARCH_INTERACTION_MATRIX_RC3.md
Visual matrix: FULL_APP_VISUAL_CONSISTENCY_MATRIX_RC3.md
Fixtures: fixtures-rc3.json

This Phase 1 package prepares an independent tester for the next CowPaper
v0.2.1 RC3 candidate. It does not claim that the baseline passes the candidate
gates, and it does not make product, database, release, or user-data changes.

## Preparation contents

- RC3 synthetic A–M fixture contract with Collection/Tag/free-text scenarios, CJK and mixed-language queries, field-clause tokens, deduplication, matched_fields/snippets, real IME expectations, visual viewports, reduced-motion policy, and the future Annotation boundary.
- Search interaction matrix S01–S23 covering Search Action removal, outside-click close, Escape/reopen, tokens/results preservation, Collection/Tag/free-text composition, field clauses, response metadata, dedup, mixed-language terms, Paper identity, empty results, real IME, workspace boundaries, keyboard focus, and motion.
- Full-app visual consistency matrix V01–V30 covering Discovery, Library, Settings, toolbar, sidebar, table, Inspector, Search/status/column/relation popovers, buttons, tags/chips, empty states, responsive layouts, overflow, typography, density, contrast, default motion, and reduced motion.
- A framework-free semantic/state test and a fixture-contract validator. They are intentionally independent of the Tauri runtime; candidate-only DOM, backend-response, and visual assertions remain explicit manual/runtime gates.

## Strict boundary

The following are explicitly out of scope for this preparation:

- No migration, schema change, DB v19 work, FTS rebuild, or production seed mutation.
- No import, overwrite, cleanup, or mutation of a user's database or files.
- No release, tag, version metadata, updater publication, or v0.2.0/v0.1.4 change.
- No large-scale main.ts, styles.css, or index.html rewrite. This commit only adds QA artifacts, tests, and the RC3 test command.
- No Annotation implementation. RC3 must not expose an empty 标注 tab, annotation extraction, annotation persistence, annotation schema, or annotation migration. A paper-linked 标注 tab after populated 元数据 remains a v0.3.0 future direction only.

## Baseline audit notes

These are handoff facts from the preparation baseline, not release findings for
a future candidate:

| Candidate gate | Baseline observation | Treatment |
|---|---|---|
| Search Action removal | buildLibrarySearchSuggestions() still creates a searchAction suggestion and the renderer has an 操作 group. | Keep S02/V08 red until the candidate removes it and proves one execution on Enter. |
| Outside-click close | Search event delegation handles focus, composition, pointer selection, and clicks on suggestions/tokens, but has no explicit outside-click close path. | Keep S03 as a candidate DOM gate. |
| Field-clause tokens | Current state only models Collection/Library Tag scope and free text. | Keep S13 as a contract/manual gate; do not implement it in Phase 1. |
| matched_fields/snippets | Current backend/UI adapter returns canonical Paper IDs and hydrated rows, without these response fields. | Keep S15/V30 as a response-handoff gate; do not invent a backend change here. |
| Real IME | Pure reducer protection exists; no real candidate runtime recording is present in this package. | Require real Chinese IME evidence in S19. |
| Reduced motion | Search CSS is static by contract, but full-app reduced-motion verification still requires candidate runtime/computed-style evidence. | Run V27–V28 on the exact candidate. |

## Reproducible setup

1. Verify the candidate checkout and record its exact SHA. Do not assume that main or a tag points to the candidate.
2. Use an in-memory or disposable fixture database. Load fixtures-rc3.json only into that isolated database and preserve canonical IDs 101–112; G/H intentionally share Paper ID 107.
3. Record OS, app/runtime version, browser/Tauri mode, viewport, device-pixel ratio, and fixture database identifier before the first test.
4. Reset the Search Box between independent rows with the explicit clear affordance. For preservation rows, capture the before state before closing or switching surfaces.
5. Run the automated preparation gates below. Then replay the manual/runtime rows in both matrices against the same candidate build.

## Automated preparation gates

Run from app/:

~~~text
npm run test:search
npm run test:search:rc2
npm run test:search:rc3
npm run test:search:rc3:fixtures
~~~

Expected output:

- librarySearch tests passed
- librarySearch RC2 tests passed (TEST 1-10)
- librarySearch RC3 tests passed (semantic/state preparation gates)
- RC3 fixture contract passed (interaction, result, visual, IME, and Annotation boundaries)

Run the static/app checks after dependencies are installed in the candidate
environment:

~~~text
npx --no-install tsc --noEmit
npm run build
~~~

From the repository root, also run:

~~~text
git diff --check
~~~

These commands validate the preparation package and existing app build only.
They do not authorize a schema migration, database rebuild, release, tag, or
user-data mutation. Existing backend/library-search tests may be run separately
by the integration owner if the candidate handoff requires them, but they are
not a Phase 1 migration task.

## Manual/runtime execution order

1. **Search ownership and close/reopen:** S01–S05, then V05–V08. Establish one Search surface, Search Action removal, outside-click behavior, Escape semantics, and preservation before testing result details.
2. **Scope and text composition:** S06–S12, S16–S18. Exercise Collection/Tag tokens, free text, OR/AND, descendants, zero-count Tags, mixed-language terms, Paper identity, dedup, and empty recovery.
3. **Candidate result contract:** S13–S15 and S17. Capture serialized queries and raw result JSON before evaluating the rendered row/Inspector.
4. **IME and keyboard:** S19 and S21. Use a real Chinese IME; pasted text is not acceptable evidence.
5. **Full-app regression:** S20, S22–S23 and V01–V30 across Discovery, Library, and Settings at all six fixture viewports.
6. **Annotation guard:** V16 and the future contract in fixtures-rc3.json. Confirm absence rather than adding a placeholder.

## Evidence requirements

For every S row, record:

~~~text
ID · candidate SHA · OS/runtime · viewport · starting view/scope · raw query · visible tokens · active suggestion · ordered result paperIds · selected Paper/Inspector · backend invocation count · pass/fail · screenshot/recording path · notes
~~~

For every V row, record:

~~~text
ID · candidate SHA · viewport/DPR · workspace/view · state/query/tokens · anchor measurements · overflow result · screenshot path · pass/fail · notes
~~~

At minimum, the independent evidence bundle must include:

- Search screenshots showing no Search Action row, outside-click close, reopen, Collection + Tag + free text, zero-count Tag, empty results, and the selected Paper/Inspector.
- Raw response JSON for AI 治理, ESG 平台, and 人工智能 AI 治理 ESG 平台, including matched_fields and snippets when the candidate claims to support them.
- A real IME screen recording showing composition-time Enter/Escape/arrow keys do not execute, close, or navigate prematurely.
- Discovery, Library, and Settings screenshots at 1440×982 and 600×982, plus responsive captures at 1512×982, 1536×982, 1000×982, and 740×982 as needed by V01–V30.
- Normal-motion and prefers-reduced-motion: reduce recordings for Search open/close, filtering, result replacement, and Inspector updates.
- A before/after recommendation snapshot for Paper 111 when running Library Search; search must not change recommendation run, rank, score, or cycle.

## Independent QA exit criteria

Independent QA may begin when the exact candidate SHA, isolated fixture
database ID, and runtime/viewport details are supplied. Preparation is complete
when the automated gates run, every manual row has an evidence record, and all
candidate-only failures are either fixed and rerun or explicitly accepted by
the release owner with a linked finding.

**READY FOR INDEPENDENT QA** means this handoff is actionable. It is not a
release PASS, migration approval, tag approval, or final sign-off.

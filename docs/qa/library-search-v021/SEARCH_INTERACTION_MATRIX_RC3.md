# CowPaper v0.2.1 RC3 — Search Interaction Matrix

Status: **QA preparation / candidate gates not executed**
Baseline: `5b8355d70a72a49974586e940403176cf794bf18`
Fixtures: [`fixtures-rc3.json`](./fixtures-rc3.json)
Pure state coverage: [`librarySearch.rc3.test.ts`](../../../app/src/librarySearch.rc3.test.ts)

This matrix is the independent QA contract for the Library Search surface. It
is intentionally written before the next candidate implementation is handed
over. A row marked **candidate gate** must be replayed against the exact
candidate build; it is not a PASS for the baseline. The baseline audit found
that `searchAction` is still emitted, there is no explicit outside-click close
handler, and no `matched_fields`/`snippets` result payload is defined. Those
observations are recorded as handoff risks rather than silently accepted.

## Test boundary

- Search is Library-local. It must not search, filter, or mutate Discovery or Settings.
- The current product projection is effective English title, Chinese title, authors, year, source/journal, Library Tags, note, English abstract, and Chinese abstract.
- DOI, URL, publisher, volume, issue, and pages are display metadata and remain excluded from unified full-text matching unless a later product decision changes the contract.
- Collection scope is OR, includes descendants, and may be represented by removable Collection tokens. Multiple Library Tags are AND-ed. Free text composes with both dimensions using AND.
- Search results remain canonical `paperId` rows. Selecting a Paper suggestion locates the existing row and does not replace the query text with a duplicate title chip.
- Search open/close, filtering, keyboard navigation, and Inspector updates have no movement animation. Reduced motion removes movement from any incidental feedback while retaining useful color/opacity state.

## Evidence protocol

Run each manual row in an isolated synthetic fixture database. Do not import
the fixture into a live user database. Capture one record per attempt:

```text
ID · candidate SHA · OS/runtime · viewport · starting view/scope · raw query · visible tokens · active suggestion · result paperIds · selected paper/Inspector · invocation count · pass/fail · screenshot or recording path · notes
```

For rows involving execution or result preservation, capture the backend
request count and the ordered canonical `paperId` list. For UI-only rows,
capture a DOM snapshot or screenshot in addition to the visible result.

## Interaction matrix

| ID | Path | Reproduction | Pass criteria | Evidence | Type / prep status |
|---|---|---|---|---|---|
| S01 | Sole Search surface | Enter Library at 1440px and inspect the chrome. Focus the Search Box with an empty value. | Exactly one persistent Search Box exists in the Library toolbar; no sidebar/table/Inspector duplicate; empty focus does not dump every Paper/Collection/Tag. | DOM count for `.library-search-toolbar`, screenshot of toolbar, suggestion count. | Manual candidate gate |
| S02 | Search Action removal | Type `governance` and inspect every suggestion group; press Enter with the query active. | No `searchAction`, Quick/Metadata/Content mode row, or action chip is rendered. Enter executes the current query once, without changing scope or adding a token. | DOM suggestion IDs/groups, one `search_library` call, result IDs `[101,105,112]`. | Candidate gate; baseline emits `action:search` |
| S03 | Outside-click close | With `governance`, Collection `AI`, Tag `Core`, and results visible, click a neutral area outside the Search Box and popover. | Popover closes. Query text, both tokens, applied result IDs `[101,105,112]`, selected Paper, and Inspector content are unchanged; no duplicate execution is triggered. | Before/after screenshot, state/result snapshot, request count. | Candidate gate; baseline handler absent |
| S04 | Escape close without data loss | Open suggestions for `治理`, press Escape once. | Suggestions close without clearing free text, tokens, applied result IDs, selected row, or Inspector. The explicit clear control remains the only clear operation. | Keyboard recording and before/after state snapshot. | Candidate gate |
| S05 | Reopen | Close the popover using outside click or Escape, then focus the same Search Box again. Repeat with an empty query. | The previous query/tokens/results/selection are restored on reopen; an empty query remains quiet and does not list all suggestions. | Two screenshots plus query/token/result snapshots. | Candidate gate |
| S06 | Collection token + free text | Select Collection `AI`; type `治理`; execute. | Collection token remains locked, `治理` stays editable, descendant Collection `AI / Governance` is included, and the result set is `[101,102,105,112]` before any additional Tag constraint. | Visible token row, query payload, result IDs. | Pure state + manual candidate gate |
| S07 | Tag token + free text | Starting from Collection `AI` + `治理`, select Library Tag `Core`; type `平台`; execute. | Collection and Tag tokens coexist; temporary suggestion text is consumed at selection, subsequent `平台` remains free text, and dimensions compose with AND. | Screenshot with both tokens/free text, payload, result IDs `[112]`. | Pure state + manual candidate gate |
| S08 | Token removal | With Collection `AI`, Tag `Core`, and free text `治理`, remove one token at a time. | Removing Collection preserves Tag and free text; removing Tag preserves Collection and free text. Results refresh to the remaining scope and never reset the other token. | Before/after token screenshots, request payloads, result IDs. | Candidate gate |
| S09 | Collection OR and descendants | Select `AI` and `ESG` Collection tokens, then search `Multi Collection Identity`. | Collection scope is the union, `AI` includes child `AI / Governance`, and Paper `109` appears once despite membership in both Collections. | Tokens, scope payload, visible row count, ordered IDs. | Pure state + manual candidate gate |
| S10 | Library Tag AND | Select `Core` and `Review` Tag tokens with no Collection. | Only Paper IDs `[101,105,112]` remain. A zero-count Tag stays visible/dimmed/clickable/draggable when encountered. | Tag tokens, result IDs, zero-count Tag DOM attributes and drag recording. | Pure state + manual candidate gate |
| S11 | Free-text field projection | Run `人工智能`, `AI 治理`, and `ESG 平台` in All Library. | Results are respectively `[101,102,112]`, `[101,105,112]`, and `[103,104,105,112]`; terms are AND-ed case-insensitively for Latin text and symmetrically for the supported CJK forms. | Query/result table, backend request payloads, screenshots. | Pure state + candidate runtime gate |
| S12 | Mixed-language AND | Run the exact query `人工智能 AI 治理 ESG 平台`. | Only the mixed-language anchor Paper `112` remains. No whitespace normalization or CJK tokenization step drops a term. | Committed query screenshot, result IDs, runtime search trace. | Pure state + real-runtime candidate gate |
| S13 | Field-clause tokens | Exercise `title:人工智能`, `author:Fiona`, `source:Information Systems Research`, `note:ESG`, and `content:治理`. Then exercise `title:AI content:治理`. | Each clause becomes a removable field token with the expected field/value label; clause values are scoped to their field, and multiple clauses compose with AND. Collection/Tag clauses are scope tokens, not duplicate free-text chips. | Token DOM/ARIA snapshot, serialized query, expected IDs from F-01–F-08. | Contract/manual candidate gate; not implemented on baseline |
| S14 | Display-only fields | Search for fixture DOI, URL, publisher, volume, issue, and pages both as free text and field clauses. | These fields do not match under the v0.2.1 unified-search contract and never appear in `matched_fields`. | Query/result IDs and response payload. | Contract/backend candidate gate |
| S15 | `matched_fields` and snippets | Query `AI 治理` and `ESG 平台`; inspect one result with hits in multiple fields. | Every canonical result reports a unique `matched_fields` list using searchable field IDs only. `snippets` has exactly the same keys, contains bounded UI-safe text, and does not expose display-only fields or a duplicate hit for a joined row. | Raw result JSON, rendered result/Inspector screenshot, field-key comparison. | Contract/manual candidate gate; baseline payload absent |
| S16 | Paper suggestion identity | Type a Paper title that appears in the fixture suggestion list and choose it. | The existing canonical row is selected and scrolled into view; query text is not replaced by a second Paper/title token; Inspector follows the same row. | Suggestion ID, selected `paperId`, query before/after, screenshot. | Candidate gate |
| S17 | Join de-duplication | Search `Multi Collection Identity` with `AI` + `ESG`; search `Parent Record` with the parent Collection. | Paper `109` is one row despite duplicate Collection joins. Parent/child records G/H resolve to one canonical row `107`; no duplicate result count or duplicated snippet group appears. | Raw rows, canonical IDs, visible row count, `matched_fields` count. | Pure state + candidate runtime gate |
| S18 | Empty-result recovery | Search a syntactically valid value with no match, then use the clear/recovery affordance. | Empty state explains the result and offers one clear recovery action. Clearing removes free text/results but does not leave stale tokens; the popover closes on Escape/outside click. | Empty-state screenshot, before/after query/token/result snapshot. | Candidate gate |
| S19 | Real IME composition | Use a real Chinese IME to compose `人工智能` in the Search Box. While composing, press Enter, Escape, ArrowDown, and ArrowUp; commit the composition and execute. | No premature execution, close, navigation, or suggestion movement occurs during composition. After `compositionend`, committed text searches and Enter executes once. Pasted text is not accepted as IME evidence. | Screen recording, event/request trace, final query/result IDs `[101,102,112]`. | Pure state + real-runtime candidate gate |
| S20 | Discovery/Library/Settings boundary | Start with a committed Library query, switch to Discovery, visit Settings, then return to Library. | Search is hidden and inert outside Library; Discovery results/recommendations and Settings controls are unchanged. Leaving a Library popover closes it; returning to Library preserves the committed Library query/tokens/results unless the user explicitly cleared them. | Three workspace screenshots, state/result snapshot before/after, no Discovery request caused by Library text. | Full-app candidate gate |
| S21 | Focus and keyboard ownership | Use Tab/Shift-Tab to enter Search, ArrowDown/Up through suggestions, Enter on a suggestion/query, and Escape. | Focus ring remains visible; arrow navigation is deterministic; Enter selects an active suggestion or executes the current query exactly once; Escape only closes the open popover. | Accessibility tree, keyboard recording, request count. | Candidate gate |
| S22 | Result/Inspector preservation | Select Paper `101`, run a query that keeps it, close/reopen Search, then run a query that excludes it. | Search updates the existing table and Inspector path; selection remains stable while the Paper is present and is cleared/reassigned only according to normal table behavior when absent. Search never mutates canonical metadata or recommendation state. | Before/after Inspector screenshot, canonical/recommendation snapshot, result IDs. | Candidate runtime gate |
| S23 | Motion and reduced motion | Repeat S02–S05 with normal motion and with OS `prefers-reduced-motion: reduce`. | Search open/close, result replacement, focus movement, and filtering are instant in both modes. No `transition: all`, layout-property animation, or rapid keyframe restart is observed. Reduced mode removes incidental movement while preserving useful color/opacity feedback. | Screen recording at normal/reduced settings, computed-style snapshot. | Full-app candidate gate |

## Result contract reference

The exact examples live in `fixtures-rc3.json` under
`resultContractExamples`. The candidate response shape is expected to retain
the existing canonical identity fields and add, where supported by the
candidate handoff:

```json
{
  "paperId": 101,
  "rank": 1,
  "relevance": 1.0,
  "matched_fields": ["title", "chinese_abstract"],
  "snippets": {
    "title": "AI Governance in Organizations",
    "chinese_abstract": "人工智能治理机制与组织责任。"
  }
}
```

`matched_fields` is a QA assertion about searchable effective fields, not an
instruction to expose DOI or other display-only metadata. Snippets must be
safe to render as text; any visual match highlighting belongs to the candidate
rendering layer and must not change the stored fixture text.

## Annotation boundary

Annotation is a future contract only. RC3 QA must confirm that the current
Inspector exposes the populated `元数据` surface without an empty `标注` tab,
annotation extraction, annotation persistence, or annotation migration. A
future v0.3.0 direction may add a paper-linked `标注` tab after `元数据` with
stable tab order, but Phase 1 does not implement or placeholder that surface.

## Exit condition

This matrix is **READY FOR INDEPENDENT QA** when an exact candidate SHA,
isolated fixture database identifier, runtime/OS, and viewport are supplied.
That phrase means the preparation package is ready for an independent tester;
it is not a release sign-off or an assertion that the baseline passes.

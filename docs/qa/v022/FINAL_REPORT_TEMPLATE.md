# CowPaper v0.2.2 独立 QA Final Report

状态：`DRAFT / PASS / PASS WITH ACCEPTED FINDINGS / FAIL / BLOCKED`

## Candidate identity

| 字段 | 值 |
|---|---|
| Baseline | `33652b059fc20427da105976afd8ddabab423403` |
| Candidate SHA | `<exact 40-char SHA>` |
| Candidate branch/ref | `<ref>` |
| macOS | `<version + architecture>` |
| App bundle | `<absolute path>` |
| App bundle SHA-256 | `<hash>` |
| Frontend/runtime | `<Node/Tauri/WebView/runtime details>` |
| Fixture DB | `<disposable DB path/id; never user DB>` |
| CI run | `<URL>` |
| CI checkout SHA | `<must equal Candidate SHA>` |
| Tester/date | `<name/date/timezone>` |

## Scope and boundary declaration

- [ ] 仅验收 v0.2.2 candidate。
- [ ] 未修改 v0.2.1、v0.2.0、v0.1.4。
- [ ] 未执行/提交 migration。
- [ ] 未创建 tag/Release。
- [ ] 未向生产或用户数据库写入 fixture。
- [ ] 未修改用户数据；validation 前后 backup/hash 与 row invariants 已记录。

## Summary

| Area | PASS | FAIL | BLOCKED | N/A | Evidence bundle |
|---|---:|---:|---:|---:|---|
| Search | 0 | 0 | 0 | 0 | `<path>` |
| Library hierarchy/membership | 0 | 0 | 0 | 0 | `<path>` |
| PDF/reader | 0 | 0 | 0 | 0 | `<path>` |
| Visual | 0 | 0 | 0 | 0 | `<path>` |
| Motion/reduced-motion | 0 | 0 | 0 | 0 | `<path>` |
| Final gates | 0 | 0 | 0 | 0 | `<path>` |

结论：`<one-paragraph decision>`

## Automated/local validation

| Check | Command/result | SHA verified | Evidence |
|---|---|---|---|
| Frontend typecheck | `<PASS/FAIL + output>` | `<yes/no>` | `<path>` |
| Frontend build | `<PASS/FAIL + output>` | `<yes/no>` | `<path>` |
| Search pure tests | `<PASS/FAIL + output>` | `<yes/no>` | `<path>` |
| Fixture contract | `<PASS/FAIL + output>` | `<yes/no>` | `<path>` |
| Rust check | `<PASS/FAIL + output>` | `<yes/no>` | `<path>` |
| Rust tests | `<PASS/FAIL + count>` | `<yes/no>` | `<path>` |
| Diff/forbidden-path guard | `<PASS/FAIL>` | `<yes/no>` | `<path>` |

## CI same-SHA verification

```text
CI run URL: <url>
Workflow/job: <name>
Reported checkout SHA: <sha>
Candidate SHA: <sha>
Artifact names/checksums: <details>
Result: <PASS/FAIL/BLOCKED>
```

Required CI evidence: frontend build/typecheck, Rust check/tests, target artifact build, and artifact metadata showing the exact candidate SHA.

## Exact macOS candidate verification

| Check | Result | Evidence |
|---|---|---|
| `.app` launches | `<PASS/FAIL>` | `<path>` |
| architecture/signature | `<PASS/FAIL>` | `<path>` |
| restart persistence | `<PASS/FAIL>` | `<path>` |
| System Default reader | `<PASS/FAIL>` | `<path>` |
| macOS `.app` picker | `<PASS/FAIL>` | `<path>` |
| missing reader fallback | `<PASS/FAIL>` | `<path>` |
| parent/child + Inspector/context open | `<PASS/FAIL>` | `<path>` |
| visual viewports | `<PASS/FAIL>` | `<path>` |
| normal motion | `<PASS/FAIL>` | `<path>` |
| reduced motion | `<PASS/FAIL>` | `<path>` |

## Matrix results

Use one row per item from [`QA_MATRIX.md`](./QA_MATRIX.md). Keep the raw evidence outside this template and link it.

| ID | Result | Candidate SHA | Evidence path | Finding/notes |
|---|---|---|---|---|
| S01–S16 | `<PASS/FAIL/BLOCKED>` | `<sha>` | `<path>` | `<notes>` |
| L01–L13 | `<PASS/FAIL/BLOCKED>` | `<sha>` | `<path>` | `<notes>` |
| P01–P12 | `<PASS/FAIL/BLOCKED>` | `<sha>` | `<path>` | `<notes>` |
| V01–V14 | `<PASS/FAIL/BLOCKED>` | `<sha>` | `<path>` | `<notes>` |
| M01–M07 | `<PASS/FAIL/BLOCKED>` | `<sha>` | `<path>` | `<notes>` |
| G01–G07 | `<PASS/FAIL/BLOCKED>` | `<sha>` | `<path>` | `<notes>` |

## Search evidence summary

Record the exact query payload, ordered canonical Paper IDs, request count, selected Paper, and Inspector state for each runtime run.

| Scenario | Query/tokens | Expected IDs | Observed IDs | matched_fields/snippets | Result |
|---|---|---|---|---|---|
| Collection + Tag + governance | `AI` + `ESG` + `governance` | `<fixture contract>` | `<ids>` | `<JSON path>` | `<result>` |
| Field-specific Chinese title | `title/chineseTitle: AI` | `<fixture contract>` | `<ids>` | `<JSON path>` | `<result>` |
| Multi-field Paper | `<query>` | `<one canonical row>` | `<ids>` | `<fields/snippets>` | `<result>` |
| Mixed language | `人工智能 AI 治理 ESG 平台` | `[112]` | `<ids>` | `<JSON path>` | `<result>` |
| Outside click/reopen | `<query/tokens>` | `<unchanged>` | `<state diff>` | `<n/a>` | `<result>` |
| Real IME | `人工智能`, `AI 治理`, `ESG 平台` | `<contract>` | `<ids>` | `<event trace>` | `<result>` |

## DB19 integrity and user-data checks

These are read-only checks against the disposable candidate DB or a verified backup copy.

```text
PRAGMA user_version: <expected 19 / observed>
FTS5 compile option: <observed>
library_search_documents: <present/count>
library_search_fts: <present/count>
search document ↔ canonical Library rows: <consistent/inconsistent>
duplicate canonical paper IDs in result projection: <0>
orphan library/PDF/history/recommendation rows: <0 or details>
settings/Collection/Tag/order invariants: <pass/details>
before/after user-data manifest: <identical/approved differences>
```

No schema repair, migration rerun, index rebuild, production DB write, or user-data cleanup is allowed as part of reporting a failure.

## Findings and disposition

| ID | Severity | Matrix row | Repro | Impact | Owner | Disposition | Retest evidence |
|---|---|---|---|---|---|---|---|
| `<F-01>` | `<P0/P1/P2>` | `<S/L/P/V/M/G>` | `<steps>` | `<impact>` | `<owner>` | `<open/fixed/accepted>` | `<path>` |

## Final sign-off

- [ ] Every candidate-only failure is fixed and rerun on the exact final candidate SHA, or explicitly accepted by the release owner.
- [ ] Search result identity, `matched_fields`, snippets, IME, outside-click preservation, and workspace boundaries are evidenced by real runtime.
- [ ] Collection/Tag reorder, intentional nest/promote, zero-count Tag, Inspector/context navigation, and Library removal preservation are evidenced.
- [ ] System Default, macOS `.app` picker, restart persistence, parent/child attachment, and missing-reader fallback are evidenced on macOS candidate.
- [ ] Visual and reduced-motion evidence covers Discovery, Library, Settings and all required high-frequency paths.
- [ ] DB19 integrity and user-data preservation checks pass.
- [ ] No tag/Release/migration/version-backport action was performed.

Release recommendation: `<ACCEPT / ACCEPT WITH EXPLICIT FINDINGS / DO NOT ACCEPT>`

QA owner: `<name>`
Release owner: `<name>`
Date: `<date>`

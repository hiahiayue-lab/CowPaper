# CowPaper v0.2.2 Search UX handoff

Status: **READY FOR UI INTEGRATION**

Scope: narrow Search UX state/API contract and the smallest runtime fixes for
the integration thread. Baseline is `33652b059fc20427da105976afd8ddabab423403`.

## State contract

`LibrarySearchQuery` has exactly four dimensions:

```ts
{
  collectionIds: number[];     // OR, including descendants
  libraryTagIds: number[];     // AND
  fieldClauses: FieldClause[]; // AND; each clause is scoped to one field
  freeTextQuery: string;       // AND across searchable fields
}
```

Collection, Library Tag, and Field Filter tokens preserve the query text that
already exists and consume only the transient suggestion text. Paper
suggestions select the existing canonical `papers.id` row and never create a
title token or replace free text.

`OUTSIDE_CLICK` and the first `ESCAPE` close only the dropdown. They preserve
tokens, free text, applied result IDs, selected paper, and Inspector state.
Re-focusing the Search Box may reopen suggestions without reconstructing or
clearing the query.

Search input `Enter` is **NONE**: it does not execute, auto-select a
Collection/Tag/Field, or clear input. Search is live on input and after
`compositionend`; Chinese IME composition owns Enter, Escape, and arrow keys
until composition ends. Only an explicit pointer/mouse click on a suggestion
commits it. Arrow keys may move the visual active index but cannot commit it.

## API/result contract

The searchable projection is exactly: English Title, Chinese Title, Authors,
Year, Journal/source, Library Tags, Note, English Abstract, and Chinese
Abstract. DOI, URL, Publisher, Volume, Issue, and Pages remain excluded.

Results retain canonical `paperId` identity and are deduplicated by
`papers.id`. Each hit retains `matched_fields` using only searchable field IDs
and bounded text snippets. These are display metadata only and must not mutate
canonical Paper or recommendation state.

## Design contract

The dropdown has only these groups, in this order: `文集` (Collections), `标签`
(Library Tags), `论文` (Papers), and `字段筛选` (Field Filters). There is no
Search Actions group or action item. Empty query remains quiet. Outside click
closes immediately while preserving state. Field tokens use the compact form
`[中文标题: AI ×]`; match fields and snippets render in their own bounded lane
below the paper title so suggestions, tokens, and evidence do not overlap.

No Rust, DB, migration, recommendation, FTS5, or canonical schema changes are
part of this handoff. DB remains v19 and **MIGRATION NONE**.

## Test handoff

Run from `app/`:

```text
npm run test:search
npm run test:search:rc2
npm run test:search:rc3
npm run test:search:v022
```

The v0.2.2 contract test covers: Search Action absence, four suggestion kinds,
outside-click preservation, free-text continuation, field token formatting,
`matched_fields`, snippets, canonical dedup, collection OR, tag AND, field
clause AND, Enter NONE, and IME protection. The integration owner should add
the manual UI gates for pointer click, real Chinese IME, dropdown reopening,
and the four-group visual layout.

## Integration checklist

- SEARCH ACTION PRESENT: **NO**
- OUTSIDE CLICK: **CLOSE + PRESERVE**
- FREE TEXT: **LIVE / AND**
- FIELD SEARCH: **SCOPED CLAUSES / AND**
- MATCHED_FIELDS: **PRESENT**
- SNIPPET: **BOUNDED**
- DEDUP: **canonical `papers.id`**
- TOKEN CONTINUATION: **PRESERVED**
- ENTER NONE: **YES**
- IME: **composition-owned**
- MIGRATION NONE: **YES; DB v19**
- READY FOR INTEGRATION: **YES**

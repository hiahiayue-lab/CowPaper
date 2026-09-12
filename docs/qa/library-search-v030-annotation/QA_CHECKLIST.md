# v0.3.0 RC — Library Search and PDF Annotation QA Checklist

This checklist covers the Library Search annotation handoff and the adjacent
regression boundaries. It is intentionally independent of the PDF annotation
extractor: search must remain safe when no annotation table, attachment, or
extractable text is available.

## Automated evidence

- [ ] `npm run test:search`
- [ ] `npm run test:search:rc2`
- [ ] `npm run test:search:rc3`
- [ ] `npm run test:search:v022`
- [ ] `npx --no-install tsc --noEmit`
- [ ] `cargo test --manifest-path app/src-tauri/Cargo.toml`
- [ ] `git diff --check`

## Search and UI regressions

- [ ] Library has exactly one toolbar Search field; Search Enter remains a
      no-op, and title Enter remains a no-op.
- [ ] `⌘F`, IME composition, Escape/outside-click behavior, Collection OR,
      Library Tag AND, field clauses, and empty-result recovery remain intact.
- [ ] Annotation hits show the understandable `Annotation` field label and a
      short escaped snippet; quoted text, comment, and translation are all
      eligible search evidence.
- [ ] Multiple annotations and multiple Collection joins still produce one
      row for one canonical `papers.id`, with no duplicate count or snippet
      group.
- [ ] Refresh/rebuild removes stale annotation text and makes changed text
      searchable without changing title, abstract, tags, collections, or
      recommendation fields.

## Library-only and Inspector boundaries

- [ ] An annotation-bearing external PDF is searchable only after it is a
      Library attachment/annotation source; it does not become Discovery input
      or recommendation/AI analysis input.
- [ ] Library has no Discovery AI action. Selecting a search result opens the
      existing canonical Library row and Inspector; no search-specific empty
      annotation tab is introduced by this integration.
- [ ] Inspector still displays the existing metadata, abstract, PDF, and
      citation groups without row reflow or accidental annotation duplication.
- [ ] Manual title translation uses only the current UI title/draft; searching
      or selecting an annotation never changes the translation source.

## Import, refresh, relink, and malformed PDFs

- [ ] Importing a PDF is read-only with respect to the source file: no
      flattening, rewrite, deletion, or canonical Paper deletion occurs.
- [ ] Exact DOI/relink refresh preserves `paper_id`, attachment identity,
      Library membership, annotations, and user metadata; changed content is
      marked/reviewed by the annotation owner rather than silently moved.
- [ ] Missing attachment/relink-required state retains existing data and does
      not create duplicate annotations or search rows.
- [ ] Malformed, encrypted, scanned, unsupported, or flattened PDFs do not
      produce guessed quoted text; comments/raw diagnostics remain outside the
      searchable quote when extraction is not reliable.
- [ ] `/Contents` is treated as comment/text, never as Highlight quoted text;
      failed quote extraction is not indexed as a quote.

## Data safety and release hygiene

- [ ] No canonical `papers` row, PDF file, recommendation score/history, or
      frozen v0.2.2 tag/release is modified, deleted, moved, retagged, or
      released.
- [ ] No `latest.json` or release metadata is updated.
- [ ] The existing v19 search schema remains backward-compatible when
      `paper_annotations` is absent; the annotation migration/owner must call
      the shared search refresh/rebuild path after annotation writes.
- [ ] Search result metadata is display-only: it does not mutate canonical
      Paper identity or introduce a second paper entity.

## Integration notes

The search side expects the annotation owner’s forward schema to expose
`paper_annotations.paper_id` and any available text columns among
`quoted_text`, `comment`, and `translation`. The search projection joins all
available values in stable annotation-row order, indexes the aggregate in the
existing v19 `annotation_text` column, and hydrates it as `LibraryPaper`’s
optional `annotationText`. The search change adds no migration and is a no-op
until that table exists.

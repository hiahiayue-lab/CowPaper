# PDF annotation storage handoff

Schema v20 adds `paper_annotations` as the production storage boundary for
read-only embedded-PDF imports. The layer is deliberately independent of
Discovery, recommendation scoring, Research Tags, and the existing Library
paper projection.

## Rust API

- `db::upsert_paper_annotation(conn, paper_id, attachment_id, input)` inserts
  or updates one record and returns `models::PaperAnnotation`.
- `db::refresh_paper_annotations(conn, paper_id, attachment_id, inputs)`
  upserts one extraction batch atomically and returns that attachment’s full
  annotation list.
- `db::list_paper_annotations(conn, paper_id, attachment_id)` lists by
  canonical Paper, optionally scoped to one attachment.
- `db::delete_paper_annotation(conn, paper_id, annotation_id)` is an explicit
  Paper-scoped delete. It does not delete a Paper, attachment, or PDF.

`PaperAnnotationInput.quadpoints` and `.rect` are source geometry inputs.
Keep the exact source geometry and PDF relationship data in
`raw_metadata_json`; the typed geometry is used only for identity matching.
Supported kinds are `highlight`, `underline`, `strikeout`, `text`, and
`freetext`. Supported extraction states are `extracted`, `no_text`, `scanned`,
`encrypted`, `malformed`, `unsupported`, and `missing_attachment`.

## Identity and refresh rules

1. Validate that `attachment_id` belongs to `paper_id`; the migration also
   installs a composite foreign key for the same invariant.
2. A unique `/NM` uses `v1:nm:{attachment_id}:{page_index}:{NM}`. `/NM` is
   treated as page-scoped and is never used when duplicate source IDs are
   present in a batch or already ambiguous in storage.
3. The fallback uses a versioned SHA-256 over attachment, page, kind,
   half-point-quantized geometry, NFKC/whitespace/case-folded quote, and
   comment. Raw source values remain unchanged for display.
4. A unique geometry match may absorb a comment edit. Ambiguous geometry is
   never merged automatically.
5. Refresh is additive and idempotent. Rows absent from a later extraction are
   retained so a flattened, encrypted, malformed, or temporarily missing PDF
   cannot silently erase imported history. Deletion is explicit.

`quoted_text` is the verified page-text excerpt; `/Contents` belongs in
`comment` and must never be used as a highlight quote. Failed extraction may
retain metadata and comment with a null `quoted_text`. The existing v19 FTS
projection is unchanged; a later search agent may index verified quote,
comment, and translation fields without copying full paper content.

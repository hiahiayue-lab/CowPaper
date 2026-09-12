# Library Inspector annotations — v0.3.0 RC candidate

Status: **implemented as a read-only Inspector view**

## Contract

The Inspector invokes `list_library_annotations` with the canonical `paperId`.
The backend resolves the paper's existing `paper_attachments` rows and reads
embedded PDF `/Annots` metadata from those files. The response is the existing
attachment-scoped shape:

```ts
type LibraryAnnotation = {
  id: string;
  paperId: number;
  attachmentId: number;
  attachmentName: string;
  kind: "highlight" | "underline" | "strikeout" | "text" | "freetext";
  color: string | null;
  pageIndex: number; // zero-based PDF page index
  excerpt: string | null;
  note: string | null; // PDF /Contents; never treated as excerpt
  extractionStatus: string;
};
```

`/NM` is used as a page-scoped external identity with the attachment and page
included. When `/NM` is absent, the backend returns a versioned fingerprint
based on attachment, page, type, color, note, and rectangle metadata. This is
for stable display identity only; no annotation row is persisted.

## UI behavior

- The `标注` group is a compact sibling of the existing PDF and citation groups.
- Cards show type, color, human-facing page number, optional excerpt, optional
  note, and an explicit unavailable-excerpt status when geometry is not present.
- `刷新` re-reads the current attachment files. Loading, empty, missing-PDF,
  and read-error states remain inside the Inspector.
- Relinking or detaching an attachment invalidates its paper's annotation view.
- The view is paper-scoped and never changes canonical metadata, Library-only
  metadata, attachment ownership, Discovery membership, or Library Search.

## Deliberate boundary

This candidate uses the existing bounded `lopdf` runtime for read-only PDF
annotation metadata. It does not guess quoted text from `/Contents`: that field
is a comment for markup annotations. `excerpt` is therefore null until a
geometry-capable extractor can prove page text against `/QuadPoints`. No
schema version, migration, annotation persistence, FTS projection, OCR, PDF
write-back, or new UI framework is introduced.


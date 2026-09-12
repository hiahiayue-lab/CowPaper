# Library Inspector annotations — v0.3.0 RC candidate

Status: **implemented as an Inspector view backed by persisted v20 records**

## Contract

The Inspector invokes `list_paper_annotations` with the canonical `paperId`.
The backend reads persisted, attachment-scoped records from `paper_annotations`.
The explicit `refresh_pdf_annotations` command re-reads an attachment without
writing to its source PDF and updates existing rows idempotently. The response
is the attachment-scoped shape:

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
included. When `/NM` is absent, the backend uses a versioned fingerprint based
on attachment, page, type, geometry, and normalized text. Ambiguous duplicate
identities remain distinct rather than being guessed into one row.

## UI behavior

- The `标注` group is a compact sibling of the existing PDF and citation groups.
- Cards show type, color, human-facing page number, optional excerpt, optional
  note, and an explicit unavailable-excerpt status when geometry is not present.
- `刷新` re-reads the current attachment files. Loading, empty, missing-PDF,
  and read-error states remain inside the Inspector.
- Relinking or detaching an attachment invalidates its paper's annotation view;
  relinking triggers a bounded refresh for the new file.
- The view is paper-scoped and never changes canonical metadata, Library-only
  metadata, attachment ownership, or Discovery membership. Annotation text is
  projected into the existing Library Search index under the `Annotation`
  field and remains one result per canonical paper.

## Deliberate boundary

This candidate uses the existing bounded `lopdf` runtime for PDF annotation
metadata. It does not guess quoted text from `/Contents`: that field is a
comment for markup annotations. `excerpt` is therefore null until a
geometry-capable extractor can prove page text against `/QuadPoints`. v20 is an
additive migration only; it preserves the canonical Paper, attachment, source
PDF, Library Search projection, and user metadata. No OCR, PDF write-back, or
new UI framework is introduced.

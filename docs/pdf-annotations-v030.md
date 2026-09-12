# PDF annotation extraction contract

This is the v0.3.0 RC implementation contract for embedded annotations in a
Library PDF. It is deliberately read-only: CowPaper never writes, flattens,
deletes, OCRs, or sends the source PDF or annotation text to a network
provider.

## Source and supported types

The source of truth is the PDF page /Annots array. The extractor supports
/Highlight, /Underline, /StrikeOut, /Text, and /FreeText. Links, stamps, ink,
widgets, and unknown subtypes are ignored and counted as unsupported. A popup
is relationship metadata, not a second annotation row.

Text markup uses /QuadPoints when present. Page and quote coordinates are
interpreted in PDF user space. quoted_text is populated only when the
geometry can be matched to a bounded, decodable page text layer. /Contents is
always stored as comment; it is never used as a quote fallback. Notes and free
text therefore have a comment but no fabricated quote.

## Identity and refresh

paper_annotations is keyed by paper_id plus the owning paper_attachments.id.
A page-scoped /NM is preferred for identity and is combined with page index and
subtype. When /NM is missing or duplicated, a versioned fingerprint of
subtype, page, geometry, quote, and comment is used. The file path is never an
identity key.

Attach, relink, managed-copy/move, and the explicit
refresh_pdf_annotations(attachmentId) command run the same scan. Repeated
scans upsert the same rows. If a new valid PDF no longer contains an earlier
row, that row is retained with extractionStatus: stale; it is not silently
deleted. A missing or malformed PDF updates the attachment scan status and
retains existing rows.

## Failure states

Attachment scan state is one of never_scanned, completed, missing_attachment,
or malformed. Annotation rows use extracted, no_text, scanned, malformed, or
stale. The raw_metadata_json field retains rectangle, quadpoint, color, popup,
reply, subtype, and source object diagnostics for future Inspector/navigation
work.

The v20 migration is additive. It preserves all existing data, and deleting a
CowPaper attachment only removes the relation and its annotation rows; it
never deletes the user PDF.

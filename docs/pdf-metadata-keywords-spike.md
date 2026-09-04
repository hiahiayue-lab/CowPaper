# PDF Metadata Enrichment + Bibliographic Keywords Spike

## Scope and guardrails

- Worktree: `/Users/hiahiayue/Developer/CowPaper-metadata`
- Branch: `spike/pdf-metadata-keywords`
- Base: current local `origin/main` (`fd8a119` at spike start)
- Mode: Research / Spike Mode
- Scope: PDF import metadata enrichment and Bibliographic Keywords only
- Formal migration created: **NO**
- User DB touched: **NO**
- Recommendation impact: **NONE**
- Formal merge: **NO**

## Baseline audit

The current code already has one canonical `papers` entity and a separate
`library_items` membership relation. `LibraryPaper` wraps the same `Paper`
row; it does not create a second paper record.

The current external-PDF path is intentionally local-only:

1. `parse_external_pdf_metadata()` scans raw PDF bytes for lightweight Info/XMP
   strings, then falls back to filename stem and PDF `CreationDate`/`ModDate`.
2. It does not extract page text, so it cannot reliably discover a DOI, title,
   byline, or publication year from page one.
3. `Crossref::work_by_doi()` and `OpenAlex::work_by_doi()` currently parse
   bibliographic fields and abstracts, but not keyword fields.
4. Provider `raw_json` is already retained in `source_records`, which can be
   reused as provenance for a future keyword table.
5. There is no Bibliographic Keywords model, table, DTO, or UI. Existing
   recommendation tags (`tags`) and user Library Tags (`library_tags`) are
   separate concepts and must remain separate.

Important current weakness: PDF `CreationDate`/`ModDate` is a file/document
date, not a publication date. It must not be promoted to canonical `Paper.year`
when enrichment is implemented. A filename DOI can remain a low-confidence
hint, but must not outrank embedded metadata or page-one text and must not be
treated as validated metadata without an exact provider/registry check.

## CURRENT KEYWORD SUPPORT: NO

There is no independent Bibliographic Keywords model. The existing tag models
are not a substitute:

- **Bibliographic Keywords**: source-provided descriptive terms for the paper.
- **Library Tags**: user-maintained organization labels.
- **Research Tags**: CowPaper recommendation interests and AI scoring inputs.

They must have separate storage, DTO fields, UI labels, and behavior.

## Source findings

| Source | What it can provide | Recommended interpretation |
|---|---|---|
| PDF Info dictionary | `/Title`, `/Author`, `/Subject`, `/Keywords`, dates | Document evidence. Attribution is unknown; do not call it author keywords by default. |
| PDF XMP | `dc:title`, `dc:creator`, `dc:subject`, `pdf:Keywords`, dates, plus vendor namespaces | Better structured document evidence. Preserve exact namespace/property as provenance; attribution still needs an explicit label. |
| PDF page one text | DOI, visible title, byline, publication/citation year, sometimes a visible `Keywords` block | Strong local evidence, but still a candidate until exact DOI/provider validation. No OCR in this spike. |
| Crossref exact DOI | DOI, title, authors, dates, abstract, and often `subject` | `subject` is a Crossref/SciVal subject category, not an author keyword. Store as `kind=subject`. Crossref does not provide a dependable deposited author-keyword field in the standard Works model. |
| OpenAlex exact DOI | title, authors, date, `keywords`, `topics`, legacy `concepts`, MeSH where applicable | `keywords` are derived from assigned topics and title/abstract embedding similarity; `concepts` are machine-classified legacy labels and are deprecated/frozen. Store as algorithmic subject/concept evidence, never as author keywords. |
| Publisher landing page/JATS | Explicit JATS `<kwd-group>`, `citation_keywords`/structured `keywords`, `dc.subject`, publisher-specific fields | Useful for original bibliographic keywords only when the page identity is verified by the exact DOI. Use explicit group attribution (`author`, `author-created`, `publisher`, taxonomy) when available. |

### Provider conclusions

- Crossref is a reliable exact-DOI bibliographic source, but its `subject`
  array must be surfaced as **Subject**, not **Author Keywords**.
- OpenAlex `concepts` must not be mapped to author keywords. The same rule
  applies to current OpenAlex work-level `keywords`: they are OpenAlex-derived
  aboutness labels, not evidence that an author supplied those terms.
- Publisher/JATS metadata is the best external source for author keywords when
  it explicitly labels them. An unlabeled HTML/XMP `keywords` value should be
  kept as publisher/document evidence, not upgraded to `author_keyword`.
- PDF Info/XMP keyword strings are valuable for display and candidate search,
  but their provenance is the file's metadata stream; a user may have edited
  them after publication.

## OPENALEX CONCEPTS SHOULD BE AUTHOR KEYWORDS: NO

This is a hard rule. OpenAlex concepts are machine-classified legacy subject
labels; current OpenAlex keywords are also derived from topics and similarity
to work text. If retained, they must be visibly labeled as OpenAlex-derived
concepts/keywords and must never be shown as author-provided bibliographic
keywords.

## PDF DOI EXTRACTION STRATEGY

Recommended order within the local PDF:

1. Read the PDF Info dictionary and document-level XMP stream through a real
   PDF parser. Check explicit DOI-like properties and preserve their exact
   location (`Info/DOI`, `prism:doi`, `bibo:doi`, etc.).
2. Extract only page one text with a decompression/output limit. Prefer DOI
   labels and resolver forms (`doi:`, `https://doi.org/`, `doi.org/`) before a
   bare DOI-looking token.
3. Normalize the candidate using one shared DOI normalizer: remove the label
   or resolver prefix, trim surrounding whitespace and citation punctuation,
   lowercase only for comparison/storage identity, and retain the original
   spelling separately.
4. Validate the normalized DOI by exact lookup. Existing `papers.normalized_doi`
   is an exact identity lookup; a new DOI should be enriched from Crossref,
   then OpenAlex, then another already-approved provider. A failed provider
   lookup is a validation/provenance state, not permission for title fuzzy
   matching.
5. Treat a filename DOI only as a low-confidence hint or candidate; it cannot
   override conflicting embedded/page evidence.

The DOI syntax is an opaque prefix/suffix identifier. The suffix may contain
punctuation, so a parser must not use a simplistic `10.<digits>/<word>` regex
that truncates valid suffixes. A practical detector may use a conservative
DOI-looking regex, then normalize, strip citation delimiters, and exact-verify.

## PDF TITLE/AUTHOR/YEAR STRATEGY

Use these values for enrichment and candidate search, not silent identity
merging:

- **Title**: exact DOI provider metadata first; otherwise explicit XMP/Info
  title; otherwise the prominent title block on page one. Filename stem is
  display fallback only.
- **Authors**: exact DOI provider metadata first; otherwise explicit XMP/Info
  author evidence; otherwise the page-one byline between title and affiliation/
  abstract markers. Strip numeric/symbol affiliation markers and preserve the
  extracted raw line for review. `/Author` is not automatically a scholarly
  byline.
- **Year**: exact provider publication/issued date first; otherwise page-one
  `published`, copyright, volume/issue citation, or explicitly labeled date.
  Never use PDF file creation/modification dates as publication year.
- **Scanned/image-only PDF**: no text candidate should be fabricated. Return
  metadata-only evidence and require manual input/provider resolution; OCR is
  outside this spike and, if later added, remains non-authoritative until
  verified.

The first-page parser should retain field-level provenance and confidence. A
single page can contain multiple years or author-like names; take the value
only when its local label/layout is strong enough, otherwise leave it unset.

## METADATA ENRICHMENT ORDER

```text
PDF embedded Info/XMP
        ↓
PDF page-one DOI detection
        ↓
exact normalized DOI identity
        ↓
Crossref exact DOI
        ↓
OpenAlex exact DOI
        ↓
other already-approved provider / verified publisher metadata
```

For a PDF with no DOI, `title + authors + year` may drive Crossref/OpenAlex
candidate search only. It is not an identity key and must never trigger an
automatic merge. Exact DOI enrichment may fill missing canonical fields and
append source provenance; it must not overwrite Library-only overrides.

## MANUAL CONFIRMATION RULE

- Existing exact DOI: attach the PDF to that canonical Paper and enrich/fill
  canonical metadata according to source precedence.
- New exact DOI with a validated provider record: create one canonical Paper
  and attach the PDF; no duplicate Paper is created for the provider response.
- No DOI: show provider/title-author-year candidates. Any association with an
  existing Paper requires explicit user confirmation.
- No DOI and no candidate: a separate external canonical Paper may be created,
  but it is not a merge and its local title/author/year remain provisional.
- Any conflicting DOI/provider/title evidence is a review state. Never use
  title fuzzy auto-merge, and never silently attach a PDF to the first fuzzy
  result.

## RECOMMENDED KEYWORD MODEL

Use an append-oriented relation attached to the canonical `papers.id`:

```sql
paper_keywords (
    id                 INTEGER PRIMARY KEY,
    paper_id           INTEGER NOT NULL REFERENCES papers(id) ON DELETE CASCADE,
    keyword            TEXT NOT NULL,
    normalized_keyword TEXT NOT NULL,
    kind               TEXT NOT NULL,
    source             TEXT NOT NULL,
    source_locator     TEXT,
    source_record_id   INTEGER REFERENCES source_records(id),
    confidence         TEXT NOT NULL,
    language           TEXT,
    position           INTEGER,
    retrieved_at       TEXT NOT NULL,
    created_at         TEXT NOT NULL
)
```

Recommended `kind` values:

- `author_keyword`: only explicit author-provided/author-created attribution.
- `publisher_keyword`: publisher-provided article keyword with no stronger
  author attribution.
- `subject`: Crossref `subject`, MeSH, or another explicitly named subject
  vocabulary.
- `concept`: legacy OpenAlex concepts or other provider classification labels.
- `document_keyword`: unattributed PDF Info/XMP keyword values.
- `algorithmic_keyword`: current OpenAlex topic-derived work keywords, kept
  separate from bibliographic author/publisher keywords.

`source_locator` is required in practice even if nullable in the first schema,
for example `Info/Keywords`, `xmp:dc:subject`,
`crossref:message.subject[0]`, `openalex:keywords[0]`, or
`publisher-jats:article-meta/kwd-group[@kwd-group-type='author']`.
Do not collapse equal text across sources: separate provenance rows are useful
when providers disagree. A uniqueness rule should deduplicate repeated values
from the same paper/source/kind/locator while preserving cross-source rows.

### AI boundary

Do not write AI-generated terms into `paper_keywords`. If future suggestions
are allowed, use a separate `paper_keyword_suggestions` model/table and a
literal UI label **AI Suggested Keywords**. They must not be included in the
Bibliographic Keywords list or treated as source metadata.

## KEYWORD SOURCES:

1. Verified publisher/JATS keywords with explicit attribution.
2. Verified publisher landing-page structured metadata, preserving the exact
   field and URL.
3. PDF/XMP values, classified as `document_keyword` unless attribution is
   explicit.
4. Crossref `subject`, classified as `subject`.
5. OpenAlex `keywords`/`topics`/`concepts`, classified as
   `algorithmic_keyword`/`concept`, never `author_keyword`.

Every row should retain source, locator, retrieval time, and (when applicable)
the existing `source_records.id`/raw provider response. Confidence is an
evidence level, not a claim that provider output is a calibrated probability.

## DISCOVERY → LIBRARY

The shared canonical identity is the design advantage:

```text
Discovery view ─┐
                ├── papers.id ── paper_keywords
Library view  ──┘
```

Library should display canonical metadata and canonical keywords through the
same `paper_id`; no metadata or keyword copy belongs in a Library-only table.
When keywords are later fetched for a canonical Paper, Discovery and Library
will see the same rows automatically. Library Tags remain user-owned labels and
Research Tags remain recommendation inputs.

## Rust stack recommendation

Use `lopdf` plus a real XML parser such as `quick-xml` for the first
implementation:

- `lopdf` exposes PDF Info metadata including title, author, subject, keywords,
  creation/modification dates, page count, and encryption state.
- Its bounded `extract_text_with_limit()`/page APIs fit untrusted local files
  better than an unbounded whole-document extraction.
- It is pure Rust and avoids shipping a separate `pdftotext` binary, which is a
  better fit for the existing Tauri packaging matrix.
- `pdf-extract` is a reasonable convenience wrapper for text-by-page extraction,
  but it does not solve attribution or identity validation.
- `pdfium-render` can provide higher-fidelity rendering/text layout, but adds a
  native PDFium runtime and packaging burden. Reserve it for a later
  coordinate-aware parser or OCR-assisted workflow.

The implementation should stop on malformed/encrypted/unextractable files with
an explicit evidence state; it should not fabricate title, authors, year, or
keywords. Add fixture tests for Info-only, XMP-only, DOI-on-page-one,
multi-column text, malformed PDF, encrypted PDF, and scanned PDF before any
formal migration is considered.

## Final report

| Required item | Result |
|---|---|
| CURRENT KEYWORD SUPPORT | **NO** |
| RECOMMENDED KEYWORD MODEL | Canonical `paper_keywords` relation with field-level provenance; separate AI suggestions |
| KEYWORD SOURCES | Verified publisher/JATS or landing page; PDF/XMP document evidence; Crossref subjects; OpenAlex-derived concepts/keywords |
| OPENALEX CONCEPTS SHOULD BE AUTHOR KEYWORDS | **NO** |
| PDF DOI EXTRACTION STRATEGY | Info/XMP → bounded page-one text → normalize → exact verify |
| PDF TITLE/AUTHOR/YEAR STRATEGY | Provider exact DOI metadata first; local values are evidence/candidates; file dates are not publication year |
| METADATA ENRICHMENT ORDER | PDF Info/XMP → PDF text DOI → exact DOI → Crossref → OpenAlex → other approved provider |
| MANUAL CONFIRMATION RULE | Required for every no-DOI association with an existing Paper; no title fuzzy auto-merge |
| PROPOSED SCHEMA | `paper_keywords` on canonical `papers.id`; provenance retained per source/value |
| FORMAL MIGRATION CREATED | **NO** |
| USER DB TOUCHED | **NO** |
| RECOMMENDATION IMPACT | **NONE** |
| READY FOR IMPLEMENTATION | **YES** |

## References

- [Crossref REST API](https://www.crossref.org/documentation/retrieve-metadata/rest-api/)
- [Crossref metadata sources and member deposits](https://www.crossref.org/documentation/retrieve-metadata/)
- [Crossref work JSON format (`subject`)](https://github.com/CrossRef/rest-api-doc/blob/master/api_format.md)
- [Crossref discussion of `subject` provenance](https://community.crossref.org/t/retrieve-subjects-and-subject-from-journals-and-works/2403)
- [OpenAlex Works attributes](https://help.openalex.org/data/works/attributes/)
- [OpenAlex Keywords: topic-derived and scored](https://help.openalex.org/data/keywords/)
- [OpenAlex Concepts: deprecated, machine-classified legacy labels](https://help.openalex.org/data/concepts/)
- [NISO JATS `kwd-group` and author-provided attribution](https://jats.nlm.nih.gov/publishing/tag-library/1.4/chapter/tag-keywords.html)
- [Adobe XMP namespaces](https://developer.adobe.com/xmp/docs/xmp-namespaces/)
- [Adobe PDF XMP namespace (`pdf:Keywords`)](https://developer.adobe.com/xmp/docs/xmp-namespaces/pdf/)
- [`lopdf::PdfMetadata`](https://docs.rs/lopdf/latest/lopdf/struct.PdfMetadata.html)
- [`lopdf::Document` bounded page/text extraction](https://docs.rs/lopdf/latest/lopdf/struct.Document.html)
- [`pdf-extract` page-by-page extraction](https://docs.rs/pdf-extract/latest/pdf_extract/fn.extract_text_by_pages.html)
- [`pdfium-render` capability and native-runtime tradeoff](https://docs.rs/pdfium-render/latest/pdfium_render/index.html)

# Missing-Abstract Coverage Audit

> Snapshot: 2026-09-01 · Scope: economics/business journal records with a missing abstract

This document preserves the durable findings from the read-only missing-abstract audit. It is an evidence snapshot and design handoff; it does not change the frozen product behavior or authorize a new migration.

## Executive findings

- The audit covered 330 records out of 993 papers. All were fresh 2026 records (July–September) and were marked missing at first ingest; 319 came from Crossref and 11 from OpenAlex.
- Missing abstracts were highly concentrated: Nature contributed 162 (49.1%), and the top five journals contributed 76.4%. This is a source-coverage problem concentrated in a small number of publishers, not a uniform database failure.
- Crossref re-check recovered 0/330 abstracts. OpenAlex re-check found 24/330 (7.3%), so a delayed re-check has modest but real value.
- Publication type matters: 182 records (55.2%) were research-like (`research_article` + `review`), while 148 (44.8%) were `not_expected` content such as news, letters, front matter, corrections, book reviews, editorials, and commentary.

## Classification evidence

The observed classification was:

| content kind | count | share |
|---|---:|---:|
| research_article | 173 | 52.4% |
| news | 97 | 29.4% |
| letter | 14 | 4.2% |
| front_matter | 13 | 3.9% |
| review | 9 | 2.7% |
| correction | 8 | 2.4% |
| book_review | 8 | 2.4% |
| editorial | 6 | 1.8% |
| commentary | 2 | 0.6% |

Crossref's broad `journal-article` type was not useful for this distinction: 328/330 records had that value, including non-research material. Explicit provider types and conservative title heuristics are safer than treating broad container types as research evidence. Eighteen low-evidence rows defaulted to research-like classification, so the true research share may be slightly lower.

## Source experiments

### Semantic Scholar

- DOI-exact matching was available for 259/330 records (78.5%); 16 abstracts were recovered (4.8%).
- DOI-exact publication types were valuable for classification: News 95, LettersAndComments 15, Review 14, and Editorial 3.
- Value assessment: low for fresh-record abstract recovery, high for identity and publication-type classification.

### OpenAIRE and working-paper versions

- Among 296 unresolved queries, 63 DOI matches were found (21.3%), but 0 abstracts were recovered; the raw XML did not contain an abstract element.
- OpenAIRE exposed 31 version DOI records. It is better treated as a version-discovery index than as an abstract source.
- Two verified examples linked journal records to SSRN versions: JAE `10.1016/j.jacceco.2026.101864` ↔ SSRN `10.2139/ssrn.4968511`, and JFE `10.1016/j.jfineco.2026.104309` ↔ SSRN `10.2139/ssrn.4616881`.
- Working-paper adoption should be restricted to `EXACT`/`HIGH` identity confidence, with the version DOI and provenance stored. The journal version remains preferred when source quality is otherwise equal.

### Publisher public metadata

Twenty-eight pages were sampled by DOI and publisher prefix:

| host | result | structured abstract |
|---|---:|---:|
| nature.com | 6/6 successful | 6/6 |
| link.springer.com | 3/3 successful | 3/3 |
| cambridge.org | 1/1 metadata stub | JS-rendered; parser needs an API or renderer |
| Elsevier landing pages | 4/4 shells | 0/4 |
| UChicago, Science, AoM, APA, Sage, Wiley, INFORMS, AEA, MISQ | 14/14 blocked | 0 |

All successful pages exposed deterministic metadata (`dc.description` or `citation_abstract`); no AI parsing was needed in the sample. DOI identity was verified on every successful page. The audit therefore supports extending the allowlist and preserving strict DOI identity checks, but not bypassing bot protection or paywalls.

### RePEc

RePEc was low value for this particular set: 66/330 records were from hard-economics journals where working-paper matching was more direct through SSRN/Crossref-linked versions. RePEc may become medium value only if the journal mix shifts toward NBER-style working papers.

## Durable recommendations

Recommended source order:

```text
Crossref → OpenAlex → scheduled OpenAlex re-check (≥7 days)
  → Semantic Scholar DOI-exact identity/type pass
  → verified publisher public metadata
  → OpenAIRE version discovery + Crossref working-paper matching
  → unresolved queue
```

- Use deterministic metadata parsing first. An LLM may assist with parsing or classification of complex public metadata, but must never generate, summarize, or polish an abstract and attribute it to a source.
- Adopt an abstract only when the source identity is verified. Preserve source, URL, retrieval time, original text, confidence, and nullable version DOI.
- Continue recovery for research articles and reviews; mark non-research kinds `not_expected` and exclude them from recovery. Keep title-only translation behavior harmless and separately gated.
- Prioritize publisher fallback for Nature, Springer, and Cambridge coverage; Cambridge requires dedicated handling for JS-rendered metadata.
- Treat Semantic Scholar primarily as classification/identity evidence and OpenAIRE primarily as version discovery. Keep both rate-limited and recovery-scoped.

## Current implementation boundary

The current mainline already absorbs the core semantic safeguards: `content_kind`, `abstract_status`, strict publisher DOI identity validation, provenance fields, and public metadata support for Nature and Springer. The following remain future work and are intentionally not part of this audit-preservation commit: Semantic Scholar integration, OpenAIRE working-paper adoption, Cambridge handling, scheduled re-check orchestration, and a recovery funnel report card.

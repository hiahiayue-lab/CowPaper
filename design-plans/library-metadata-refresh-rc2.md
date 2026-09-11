# Library Inspector metadata refresh — RC2 handoff

Status: **READY FOR IMPLEMENTATION / implemented in this branch**  
Baseline: `321b393533688507823ef86194c04591944f8c86` (`origin/main`)  
Product: `0.2.2` · Database: v19 · Migration: **NONE**

## Scope

Polish the Library Inspector's Citation group and add one lightweight `刷新元数据` entry point. Keep `library_item_metadata` as the Library-only edit layer and keep the existing effective projection. The visible UI does not expose internal override terminology.

## Contract

- Refresh is available only when the canonical Paper has a normalized DOI.
- The command fetches the DOI exactly from Crossref and OpenAlex, in that fixed provider order; provider responses must carry the same normalized DOI.
- Canonical bibliographic fields may be refreshed through the existing provider/abstract quality pipeline. A provider response is never merged by fuzzy title, author, or year similarity.
- `library_item_metadata` is never written by refresh. Manual title, citation fields, abstract, translated fields, DOI/URL, and note therefore continue to win in the effective Library projection.
- Collections, Library Tags, recommendation fields, AI state, and attachments are outside the refresh contract and remain untouched. No LLM, schema change, migration, release, or tag work is in scope.
- A successful no-op is still reported as a completed refresh; provider failures are surfaced without changing local data.

## Audited paths

- UI: `app/src/main.ts` Inspector render and delegated Library actions.
- Commands: existing `get/set/update_library_item_metadata` and `clear_library_item_overrides`; the new command is `refresh_library_item_metadata`.
- Providers: `api::crossref::Crossref::work_by_doi` and `api::openalex::OpenAlex::work_by_doi`.
- Persistence: `db::library_paper` effective projection, `db::merge_abstract`, source-record/provenance path, and v19 schema.
- Tests: existing effective-override/canonical-isolation tests plus exact-DOI refresh and mismatch-protection coverage.

## Acceptance evidence

- Typecheck/build passes.
- Rust tests pass, including canonical isolation, manual-field preservation, provider refresh, and DOI mismatch rejection.
- Static UI inspection contains no obsolete internal English label.

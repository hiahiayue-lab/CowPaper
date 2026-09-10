/**
 * Library Search v0.2.1 RC3 framework-free state and adapter contract.
 *
 * This module deliberately knows nothing about Tauri or the DOM. The UI can
 * replace the adapter with a native backend implementation without changing
 * keyboard, scope, field-clause, suggestion, or de-duplication semantics.
 */

export type LibrarySearchPhase = "closed" | "open" | "composing";

/** The only fields that may participate in a Library full-text query. */
export const LIBRARY_SEARCH_FIELDS = [
  "title",
  "chineseTitle",
  "authors",
  "year",
  "source",
  "libraryTags",
  "note",
  "abstract",
  "chineseAbstract",
] as const;

export type LibrarySearchField = typeof LIBRARY_SEARCH_FIELDS[number];

export interface LibrarySearchFieldDefinition {
  field: LibrarySearchField;
  label: string;
}

export const LIBRARY_SEARCH_FIELD_DEFINITIONS: readonly LibrarySearchFieldDefinition[] = [
  { field: "title", label: "English title" },
  { field: "chineseTitle", label: "Chinese title" },
  { field: "authors", label: "Authors" },
  { field: "year", label: "Year" },
  { field: "source", label: "Journal / source" },
  { field: "libraryTags", label: "Library Tags" },
  { field: "note", label: "Note" },
  { field: "abstract", label: "English abstract" },
  { field: "chineseAbstract", label: "Chinese abstract" },
];

/** Display-only metadata. These names are intentionally not searchable. */
export const LIBRARY_SEARCH_EXCLUDED_FIELDS = [
  "publisher",
  "doi",
  "url",
  "volume",
  "issue",
  "pages",
] as const;

export type LibrarySearchExcludedField = typeof LIBRARY_SEARCH_EXCLUDED_FIELDS[number];

export type LibrarySearchSuggestionKind = "collection" | "libraryTag" | "paper" | "field";

export interface SearchCollection {
  id: number;
  parentId: number | null;
  name: string;
}

export interface SearchLibraryTag {
  id: number;
  name: string;
  color?: string | null;
}

export interface SearchPaper {
  id: number;
  title?: string | null;
  chineseTitle?: string | null;
  authors?: string | string[] | null;
  source?: string | null;
  year?: number | string | null;
  publisher?: string | null;
  doi?: string | null;
  url?: string | null;
  volume?: string | null;
  issue?: string | null;
  pages?: string | null;
  note?: string | null;
  abstract?: string | null;
  chineseAbstract?: string | null;
  collectionIds?: number[];
  tagIds?: number[];
  tags?: string[];
}

export interface LibrarySearchFieldClause {
  /** One of the nine values in LIBRARY_SEARCH_FIELDS. */
  field: LibrarySearchField;
  /** Terms in one clause are AND-ed within this field. */
  query: string;
}

/** Input shape also accepts legacy queryText and wire-friendly aliases. */
export interface LibrarySearchFieldClauseInput {
  field?: string | null;
  query?: string | number | null;
  value?: string | number | null;
  text?: string | number | null;
}

export interface LibrarySearchScope {
  /** Multiple collections are OR-ed, including their descendants. */
  collectionIds: number[];
  /** Multiple Library Tags are AND-ed. */
  libraryTagIds: number[];
}

export interface LibrarySearchQuery extends LibrarySearchScope {
  /** Optional field-scoped clauses; clauses are AND-ed with freeTextQuery. */
  fieldClauses: LibrarySearchFieldClause[];
  /** Terms are AND-ed across the nine allowed fields. */
  freeTextQuery: string;
}

export interface LibrarySearchQueryInput {
  fieldClauses?: readonly LibrarySearchFieldClauseInput[] | null;
  freeTextQuery?: string | number | null;
  /** Accepted only as a compatibility input; never emitted by normalize. */
  queryText?: string | number | null;
  collectionIds?: readonly number[] | null;
  libraryTagIds?: readonly number[] | null;
}

export interface LibrarySearchRequest extends LibrarySearchQuery {
  view?: "all" | "recent" | "unfiled";
}

export interface LibrarySearchSnippet {
  field: LibrarySearchField;
  /** A compact, display-safe excerpt. It is not an HTML fragment. */
  text: string;
}

export interface LibrarySearchHit {
  /** Canonical papers.id; never a row/collection membership identity. */
  paperId: number;
  rank?: number;
  relevance?: number;
  /** Wire/API spelling is deliberate and remains stable for the UI handoff. */
  matched_fields: LibrarySearchField[];
  snippets: LibrarySearchSnippet[];
}

export interface LibrarySearchResult {
  papers: SearchPaper[];
  /** Canonical paper IDs, unique and in hit order. */
  paperIds: number[];
  /** Optional while the v19 command is being upgraded; local adapters fill it. */
  hits?: LibrarySearchHit[];
}

export interface SearchSuggestionScope extends Partial<LibrarySearchScope> {
  fieldClauses?: LibrarySearchFieldClause[];
}

export interface LibrarySearchSuggestion {
  id: string;
  kind: LibrarySearchSuggestionKind;
  label: string;
  detail?: string;
  scope?: SearchSuggestionScope;
  /** Set on a field suggestion; scope.fieldClauses carries a committed value. */
  field?: LibrarySearchField;
  count?: number;
  paperId?: number;
  /** Zero-count tags stay visible, dimmed, clickable, and draggable. */
  dimmed?: boolean;
  draggable?: boolean;
}

export interface LibrarySearchIndex {
  collections: SearchCollection[];
  tags: SearchLibraryTag[];
  papers: SearchPaper[];
  tagCounts?: Map<number, number> | Record<number, number>;
}

export interface LibrarySearchApi {
  search(request: LibrarySearchRequest): Promise<LibrarySearchResult>;
  getSuggestions(request: LibrarySearchRequest): Promise<LibrarySearchSuggestion[]>;
}

export interface LibrarySearchSidebarScope {
  /** When present, replace the Collection dimension with this sidebar choice. */
  collectionId?: number | null;
  /** When present, replace the Tag dimension with this sidebar selection. */
  libraryTagIds?: readonly number[];
}

export interface LibrarySearchFieldToken {
  id: string;
  kind: "field";
  field: LibrarySearchField;
  query: string;
  label: string;
}

export function emptyLibrarySearchScope(): LibrarySearchScope {
  return { collectionIds: [], libraryTagIds: [] };
}

export function emptyLibrarySearchQuery(): LibrarySearchQuery {
  return { fieldClauses: [], freeTextQuery: "", ...emptyLibrarySearchScope() };
}

function uniqueIds(ids: readonly number[]): number[] {
  return [...new Set(ids.filter((id) => Number.isInteger(id) && id > 0))];
}

function valueText(value: string | number | null | undefined): string {
  return value == null ? "" : String(value);
}

function authorText(authors: SearchPaper["authors"]): string {
  return Array.isArray(authors) ? authors.join(" ") : valueText(authors);
}

const FIELD_ALIASES: Record<string, LibrarySearchField> = {
  title: "title",
  englishTitle: "title",
  英文标题: "title",
  chineseTitle: "chineseTitle",
  chinese_title: "chineseTitle",
  中文标题: "chineseTitle",
  authors: "authors",
  author: "authors",
  作者: "authors",
  year: "year",
  年份: "year",
  source: "source",
  journal: "source",
  期刊: "source",
  libraryTags: "libraryTags",
  library_tags: "libraryTags",
  tags: "libraryTags",
  标签: "libraryTags",
  note: "note",
  notes: "note",
  备注: "note",
  abstract: "abstract",
  英文摘要: "abstract",
  chineseAbstract: "chineseAbstract",
  chinese_abstract: "chineseAbstract",
  中文摘要: "chineseAbstract",
};

function normalizeField(value: unknown): LibrarySearchField | null {
  return typeof value === "string" ? FIELD_ALIASES[value] || null : null;
}

function normalizeFieldClauses(clauses: readonly LibrarySearchFieldClauseInput[] | null | undefined): LibrarySearchFieldClause[] {
  if (!clauses) return [];
  const seen = new Set<string>();
  const normalized: LibrarySearchFieldClause[] = [];
  for (const clause of clauses) {
    const field = normalizeField(clause.field);
    const rawQuery = clause.query ?? clause.value ?? clause.text;
    const query = valueText(rawQuery).trim();
    if (!field || !query) continue;
    const key = `${field}\u0000${query}`;
    if (seen.has(key)) continue;
    seen.add(key);
    normalized.push({ field, query });
  }
  return normalized;
}

/** Normalize IDs, whitespace, aliases, and invalid field clauses at the API boundary. */
export function normalizeLibrarySearchQuery(query: LibrarySearchQueryInput = {}): LibrarySearchQuery {
  const rawFreeText = query.freeTextQuery ?? query.queryText;
  return {
    fieldClauses: normalizeFieldClauses(query.fieldClauses),
    freeTextQuery: valueText(rawFreeText).trim(),
    collectionIds: uniqueIds(query.collectionIds || []),
    libraryTagIds: uniqueIds(query.libraryTagIds || []),
  };
}

export interface ParsedLibrarySearchInput {
  queryText: string;
  fieldClauses: LibrarySearchFieldClause[];
}

/**
 * Support the compact `field:value` keyboard form as a convenience. The
 * primary UI path is an explicit field suggestion, which is less ambiguous
 * for Chinese and IME input. Unknown prefixes remain ordinary free text.
 */
export function parseLibrarySearchInput(input: string): ParsedLibrarySearchInput {
  const raw = input.trim();
  const match = raw.match(/^([^:\s]+)\s*:\s*(?:"([^"]+)"|'([^']+)'|(.+))$/u);
  if (!match) return { queryText: raw, fieldClauses: [] };
  const field = normalizeField(match[1]);
  const query = (match[2] ?? match[3] ?? match[4] ?? "").trim();
  if (!field || !query) return { queryText: raw, fieldClauses: [] };
  return { queryText: "", fieldClauses: [{ field, query }] };
}

export function hasLibrarySearchInput(query: LibrarySearchQueryInput): boolean {
  const normalized = normalizeLibrarySearchQuery(query);
  return Boolean(normalized.freeTextQuery || normalized.fieldClauses.length || normalized.collectionIds.length || normalized.libraryTagIds.length);
}

/** Expand selected parent collections without duplicating papers or looping on malformed trees. */
export function expandCollectionIds(collections: readonly SearchCollection[], selectedIds: readonly number[]): number[] {
  const children = new Map<number, number[]>();
  for (const collection of collections) {
    if (collection.parentId == null) continue;
    const values = children.get(collection.parentId) || [];
    values.push(collection.id);
    children.set(collection.parentId, values);
  }
  const expanded: number[] = [];
  const visited = new Set<number>();
  const visit = (id: number) => {
    if (visited.has(id)) return;
    visited.add(id);
    expanded.push(id);
    for (const child of children.get(id) || []) visit(child);
  };
  for (const id of uniqueIds(selectedIds)) visit(id);
  return expanded;
}

/** Project a Paper into the nine searchable fields. Excluded metadata is absent by construction. */
export function librarySearchFieldValues(paper: SearchPaper): Record<LibrarySearchField, string> {
  return {
    title: valueText(paper.title),
    chineseTitle: valueText(paper.chineseTitle),
    authors: authorText(paper.authors),
    year: valueText(paper.year),
    source: valueText(paper.source),
    libraryTags: (paper.tags || []).join(" "),
    note: valueText(paper.note),
    abstract: valueText(paper.abstract),
    chineseAbstract: valueText(paper.chineseAbstract),
  };
}

function normalizedText(value: string): string {
  return value.normalize("NFKC").toLocaleLowerCase();
}

function searchTerms(value: string): string[] {
  return normalizedText(value).split(/\s+/u).map((term) => term.trim()).filter(Boolean);
}

function fieldContainsTerms(value: string, terms: readonly string[]): boolean {
  const haystack = normalizedText(value);
  return terms.every((term) => haystack.includes(term));
}

function hasAllFieldClauses(values: Record<LibrarySearchField, string>, clauses: readonly LibrarySearchFieldClause[]): boolean {
  return clauses.every((clause) => fieldContainsTerms(values[clause.field], searchTerms(clause.query)));
}

/** Match free text across all searchable fields plus field clauses in their named field. */
export function matchesLibrarySearchQuery(paper: SearchPaper, query: LibrarySearchQueryInput): boolean {
  const normalized = normalizeLibrarySearchQuery(query);
  const values = librarySearchFieldValues(paper);
  const freeTerms = searchTerms(normalized.freeTextQuery);
  const allFields = Object.values(values).join(" ");
  return fieldContainsTerms(allFields, freeTerms) && hasAllFieldClauses(values, normalized.fieldClauses);
}

/** Compatibility helper for callers that only need the all-field text dimension. */
export function matchesLibrarySearchText(paper: SearchPaper, queryText: string): boolean {
  return matchesLibrarySearchQuery(paper, { freeTextQuery: queryText });
}

function hasAllTags(paper: SearchPaper, tagIds: readonly number[]): boolean {
  if (!tagIds.length) return true;
  const paperTags = new Set(paper.tagIds || []);
  return tagIds.every((tagId) => paperTags.has(tagId));
}

function hasAnyCollection(paper: SearchPaper, collectionIds: readonly number[]): boolean {
  if (!collectionIds.length) return true;
  const paperCollections = new Set(paper.collectionIds || []);
  return collectionIds.some((collectionId) => paperCollections.has(collectionId));
}

/** Keep the first row for each canonical papers.id, preserving stable backend order. */
export function dedupeLibrarySearchPapers(papers: readonly SearchPaper[]): SearchPaper[] {
  const byId = new Map<number, SearchPaper>();
  for (const paper of papers) {
    if (!Number.isInteger(paper.id) || paper.id <= 0 || byId.has(paper.id)) continue;
    byId.set(paper.id, paper);
  }
  return [...byId.values()];
}

/** Client fallback used by the pre-backend adapter and by deterministic tests. */
export function filterLibrarySearchPapers(papers: readonly SearchPaper[], query: LibrarySearchQueryInput): SearchPaper[] {
  const normalized = normalizeLibrarySearchQuery(query);
  return dedupeLibrarySearchPapers(papers.filter((paper) =>
    hasAnyCollection(paper, normalized.collectionIds)
    && hasAllTags(paper, normalized.libraryTagIds)
    && matchesLibrarySearchQuery(paper, normalized),
  ));
}

function tagCount(index: LibrarySearchIndex, id: number): number {
  if (index.tagCounts instanceof Map) return index.tagCounts.get(id) || 0;
  return index.tagCounts?.[id] || 0;
}

function paperSuggestionLabel(paper: SearchPaper): string {
  return valueText(paper.title || paper.chineseTitle) || `Paper #${paper.id}`;
}

const FIELD_INTENT_LABELS: Record<LibrarySearchField, string> = {
  title: "英文标题中搜索",
  chineseTitle: "中文标题中搜索",
  authors: "作者中搜索",
  year: "年份中搜索",
  source: "期刊中搜索",
  libraryTags: "标签中搜索",
  note: "备注中搜索",
  abstract: "英文摘要中搜索",
  chineseAbstract: "中文摘要中搜索",
};

/** Build one flat dropdown list; there is deliberately no Search Action item. */
export function buildLibrarySearchSuggestions(index: LibrarySearchIndex, query: LibrarySearchQueryInput): LibrarySearchSuggestion[] {
  const normalized = normalizeLibrarySearchQuery(query);
  const terms = searchTerms(normalized.freeTextQuery);
  const includes = (label: string) => {
    const normalizedLabel = normalizedText(label);
    return terms.every((term) => normalizedLabel.includes(term));
  };
  const suggestions: LibrarySearchSuggestion[] = [];
  const collections = new Map<number, SearchCollection>();
  for (const collection of index.collections) collections.set(collection.id, collection);
  for (const collection of collections.values()) {
    if (!includes(collection.name)) continue;
    suggestions.push({ id: `collection:${collection.id}`, kind: "collection", label: collection.name, detail: "文集 · OR（含子文集）", scope: { collectionIds: [collection.id] }, draggable: true });
  }
  const tags = new Map<number, SearchLibraryTag>();
  for (const tag of index.tags) tags.set(tag.id, tag);
  for (const tag of tags.values()) {
    if (!includes(tag.name)) continue;
    const count = tagCount(index, tag.id);
    suggestions.push({ id: `libraryTag:${tag.id}`, kind: "libraryTag", label: tag.name, detail: `Library Tag · ${count}`, count, dimmed: count === 0, draggable: true, scope: { libraryTagIds: [tag.id] } });
  }
  if (normalized.freeTextQuery) {
    for (const field of LIBRARY_SEARCH_FIELD_DEFINITIONS) {
      suggestions.push({
        id: `field:${field.field}:${encodeURIComponent(normalized.freeTextQuery)}`,
        kind: "field",
        field: field.field,
        label: `${FIELD_INTENT_LABELS[field.field]} “${normalized.freeTextQuery}”`,
        detail: field.label,
        scope: { fieldClauses: [{ field: field.field, query: normalized.freeTextQuery }] },
      });
    }
  }
  for (const paper of dedupeLibrarySearchPapers(index.papers)) {
    const label = paperSuggestionLabel(paper);
    if (!includes(label)) continue;
    suggestions.push({ id: `paper:${paper.id}`, kind: "paper", label, detail: paper.source || "论文", paperId: paper.id });
  }
  return suggestions;
}

/** Optional field picker for a UI that wants explicit field tokens. */
export function buildLibrarySearchFieldSuggestions(text = ""): LibrarySearchSuggestion[] {
  const needle = normalizedText(text.trim());
  return LIBRARY_SEARCH_FIELD_DEFINITIONS
    .filter(({ field, label }) => !needle || normalizedText(field).includes(needle) || normalizedText(label).includes(needle))
    .map(({ field, label }) => ({ id: `field:${field}`, kind: "field" as const, field, label, detail: "字段 token" }));
}

export function applyLibrarySearchFieldClause(query: LibrarySearchQueryInput, clause: LibrarySearchFieldClauseInput): LibrarySearchQuery {
  const normalized = normalizeLibrarySearchQuery(query);
  return normalizeLibrarySearchQuery({ ...normalized, fieldClauses: [...normalized.fieldClauses, clause] });
}

export function removeLibrarySearchFieldClause(query: LibrarySearchQueryInput, index: number): LibrarySearchQuery {
  const normalized = normalizeLibrarySearchQuery(query);
  if (!Number.isInteger(index) || index < 0 || index >= normalized.fieldClauses.length) return normalized;
  return normalizeLibrarySearchQuery({ ...normalized, fieldClauses: normalized.fieldClauses.filter((_, clauseIndex) => clauseIndex !== index) });
}

export function buildLibrarySearchFieldTokens(query: LibrarySearchQueryInput): LibrarySearchFieldToken[] {
  const normalized = normalizeLibrarySearchQuery(query);
  return normalized.fieldClauses.map((clause) => ({
    id: `field:${clause.field}:${encodeURIComponent(clause.query)}`,
    kind: "field" as const,
    field: clause.field,
    query: clause.query,
    label: `${LIBRARY_SEARCH_FIELD_DEFINITIONS.find((definition) => definition.field === clause.field)?.label || clause.field}: ${clause.query}`,
  }));
}

/** Sidebar scope is an auto-token operation; it must not erase free text or field tokens. */
export function applyLibrarySearchSidebarScope(query: LibrarySearchQueryInput, scope: LibrarySearchSidebarScope): LibrarySearchQuery {
  const normalized = normalizeLibrarySearchQuery(query);
  return normalizeLibrarySearchQuery({
    ...normalized,
    collectionIds: "collectionId" in scope ? (scope.collectionId == null ? [] : [scope.collectionId]) : normalized.collectionIds,
    libraryTagIds: "libraryTagIds" in scope ? [...(scope.libraryTagIds || [])] : normalized.libraryTagIds,
  });
}

export const applyLibrarySidebarScope = applyLibrarySearchSidebarScope;

function fieldTermsForQuery(field: LibrarySearchField, query: LibrarySearchQuery): string[] {
  return [
    ...searchTerms(query.freeTextQuery),
    ...query.fieldClauses.filter((clause) => clause.field === field).flatMap((clause) => searchTerms(clause.query)),
  ];
}

function matchingFields(paper: SearchPaper, query: LibrarySearchQuery): LibrarySearchField[] {
  const values = librarySearchFieldValues(paper);
  const freeTerms = searchTerms(query.freeTextQuery);
  return LIBRARY_SEARCH_FIELDS.filter((field) => {
    const value = values[field];
    const matchesFreeText = freeTerms.some((term) => normalizedText(value).includes(term));
    const matchesFieldClause = query.fieldClauses.some((clause) => clause.field === field && fieldContainsTerms(value, searchTerms(clause.query)));
    return matchesFreeText || matchesFieldClause;
  });
}

const MAX_SEARCH_SNIPPET_LENGTH = 120;
const MAX_SEARCH_SNIPPETS = 3;

function shortSearchSnippet(value: string, terms: readonly string[]): string {
  const clean = value.trim();
  if (!clean) return "";
  const lowered = normalizedText(clean);
  const index = terms.map((term) => lowered.indexOf(term)).filter((position) => position >= 0).sort((a, b) => a - b)[0] ?? 0;
  const start = Math.max(0, index - 36);
  const hasPrefix = start > 0;
  const initialEnd = Math.min(clean.length, start + MAX_SEARCH_SNIPPET_LENGTH - (hasPrefix ? 1 : 0));
  const hasSuffix = initialEnd < clean.length;
  const end = Math.min(clean.length, start + MAX_SEARCH_SNIPPET_LENGTH - (hasPrefix ? 1 : 0) - (hasSuffix ? 1 : 0));
  return `${hasPrefix ? "…" : ""}${clean.slice(start, end)}${hasSuffix ? "…" : ""}`;
}

/** Build UI metadata without changing canonical Paper data. */
export function matchLibrarySearchPaper(paper: SearchPaper, query: LibrarySearchQueryInput, rank?: number, relevance?: number): LibrarySearchHit {
  const normalized = normalizeLibrarySearchQuery(query);
  const fields = matchingFields(paper, normalized);
  const values = librarySearchFieldValues(paper);
  const snippets = fields
    .map((field) => ({ field, text: shortSearchSnippet(values[field], fieldTermsForQuery(field, normalized)) }))
    .filter((snippet) => snippet.text)
    .slice(0, MAX_SEARCH_SNIPPETS);
  return { paperId: paper.id, ...(rank == null ? {} : { rank }), ...(relevance == null ? {} : { relevance }), matched_fields: fields, snippets };
}

export const buildLibrarySearchHit = matchLibrarySearchPaper;

export interface LibrarySearchAdapterDependencies {
  listPapers: (request: { view: "all" | "recent" | "unfiled"; collectionId: number | null; tagIds: number[] }) => Promise<SearchPaper[]>;
  listCollections: () => Promise<SearchCollection[]>;
  listTags: () => Promise<SearchLibraryTag[]>;
  getTagCounts?: (collectionId: number | null) => Promise<Map<number, number> | Record<number, number>>;
}

/** Adapter over the existing Library commands. The UI can swap in a native search API. */
export function createLibrarySearchAdapter(deps: LibrarySearchAdapterDependencies): LibrarySearchApi {
  const api: LibrarySearchApi = {
    async search(request) {
      const query = normalizeLibrarySearchQuery(request);
      const view = request.view || "all";
      const collections = query.collectionIds.length ? await deps.listCollections() : [];
      const collectionIds = query.collectionIds.length ? expandCollectionIds(collections, query.collectionIds) : [null];
      const batches = await Promise.all(collectionIds.map((collectionId) => deps.listPapers({ view, collectionId, tagIds: [...query.libraryTagIds] })));
      const papers = filterLibrarySearchPapers(batches.flat(), { ...query, collectionIds: collectionIds.filter((id): id is number => id != null) });
      const hits = papers.map((paper) => matchLibrarySearchPaper(paper, query));
      return { papers, paperIds: hits.map((hit) => hit.paperId), hits };
    },
    async getSuggestions(request) {
      const query = normalizeLibrarySearchQuery(request);
      const [collections, tags, result] = await Promise.all([
        deps.listCollections(),
        deps.listTags(),
        api.search({ ...query, view: request.view || "all" }),
      ]);
      const counts = deps.getTagCounts
        ? await deps.getTagCounts(query.collectionIds.length === 1 ? query.collectionIds[0] : null)
        : undefined;
      return buildLibrarySearchSuggestions({ collections, tags, papers: result.papers, tagCounts: counts }, query);
    },
  };
  return api;
}

export interface LibrarySearchState {
  phase: LibrarySearchPhase;
  query: LibrarySearchQuery;
  suggestions: LibrarySearchSuggestion[];
  activeSuggestionIndex: number;
  isComposing: boolean;
  requestVersion: number;
}

export type LibrarySearchAction =
  | { type: "FOCUS" }
  | { type: "INPUT"; text: string }
  | { type: "START_COMPOSITION" }
  | { type: "END_COMPOSITION"; text?: string }
  | { type: "SUGGESTIONS"; suggestions: LibrarySearchSuggestion[] }
  | { type: "MOVE_ACTIVE"; delta: 1 | -1 }
  | { type: "SELECT_ACTIVE" }
  | { type: "SELECT_SUGGESTION"; suggestion: LibrarySearchSuggestion }
  | { type: "ADD_FIELD_CLAUSE"; clause: LibrarySearchFieldClauseInput }
  | { type: "REMOVE_FIELD_CLAUSE"; index: number }
  | { type: "SET_SIDEBAR_SCOPE"; scope: LibrarySearchSidebarScope }
  | { type: "DISMISS_SUGGESTIONS" }
  | { type: "OUTSIDE_CLICK" }
  | { type: "ESCAPE" }
  | { type: "CLEAR" }
  | { type: "EXECUTE" };

export function createLibrarySearchState(query: LibrarySearchQueryInput = {}): LibrarySearchState {
  return { phase: "closed", query: normalizeLibrarySearchQuery(query), suggestions: [], activeSuggestionIndex: -1, isComposing: false, requestVersion: 0 };
}

function dismissSuggestions(state: LibrarySearchState): LibrarySearchState {
  return { ...state, phase: "closed", suggestions: [], activeSuggestionIndex: -1 };
}

function searchStateHasQuery(state: LibrarySearchState): boolean {
  return Boolean(state.query.freeTextQuery || state.query.fieldClauses.length || state.query.collectionIds.length || state.query.libraryTagIds.length);
}

export function reduceLibrarySearchState(state: LibrarySearchState, action: LibrarySearchAction): LibrarySearchState {
  switch (action.type) {
    case "FOCUS":
      return { ...state, phase: state.isComposing ? "composing" : "open" };
    case "INPUT":
      return {
        ...state,
        phase: state.isComposing ? "composing" : "open",
        query: normalizeLibrarySearchQuery({ ...state.query, freeTextQuery: action.text }),
        suggestions: [],
        activeSuggestionIndex: -1,
        requestVersion: state.requestVersion + 1,
      };
    case "START_COMPOSITION":
      return { ...state, phase: "composing", isComposing: true };
    case "END_COMPOSITION":
      return {
        ...state,
        phase: "open",
        isComposing: false,
        query: action.text == null ? state.query : normalizeLibrarySearchQuery({ ...state.query, freeTextQuery: action.text }),
        suggestions: [],
        activeSuggestionIndex: -1,
        requestVersion: action.text == null ? state.requestVersion : state.requestVersion + 1,
      };
    case "SUGGESTIONS":
      // Never preselect the first suggestion. A plain Enter after typing is a
      // text-search action; a Collection/Tag is added only after an explicit
      // pointer click or keyboard navigation to a suggestion.
      return { ...state, suggestions: action.suggestions, activeSuggestionIndex: -1 };
    case "MOVE_ACTIVE": {
      const count = state.suggestions.length;
      if (!count || state.phase === "closed") return state;
      const next = state.activeSuggestionIndex < 0
        ? (action.delta > 0 ? 0 : count - 1)
        : (state.activeSuggestionIndex + action.delta + count) % count;
      return { ...state, activeSuggestionIndex: next };
    }
    case "SELECT_ACTIVE": {
      const suggestion = state.suggestions[state.activeSuggestionIndex];
      return suggestion ? reduceLibrarySearchState(state, { type: "SELECT_SUGGESTION", suggestion }) : state;
    }
    case "SELECT_SUGGESTION":
      return {
        ...state,
        phase: "open",
        query: applyLibrarySearchSuggestion(state.query, action.suggestion),
        suggestions: [],
        activeSuggestionIndex: -1,
        requestVersion: state.requestVersion + 1,
      };
    case "ADD_FIELD_CLAUSE":
      return {
        ...state,
        phase: "open",
        query: applyLibrarySearchFieldClause(state.query, action.clause),
        suggestions: [],
        activeSuggestionIndex: -1,
        requestVersion: state.requestVersion + 1,
      };
    case "REMOVE_FIELD_CLAUSE":
      return {
        ...state,
        phase: "open",
        query: removeLibrarySearchFieldClause(state.query, action.index),
        suggestions: [],
        activeSuggestionIndex: -1,
        requestVersion: state.requestVersion + 1,
      };
    case "SET_SIDEBAR_SCOPE":
      return {
        ...state,
        query: applyLibrarySearchSidebarScope(state.query, action.scope),
        suggestions: [],
        activeSuggestionIndex: -1,
        requestVersion: state.requestVersion + 1,
      };
    case "DISMISS_SUGGESTIONS":
    case "OUTSIDE_CLICK":
      return dismissSuggestions(state);
    case "ESCAPE":
      if (state.isComposing) return state;
      // Escape #1 only dismisses the popover. The query, field tokens, and
      // scope tokens remain exactly as entered.
      if (state.phase !== "closed") return dismissSuggestions(state);
      // Escape #2 clears the complete search expression after the popover is
      // already closed. An empty closed search is a no-op.
      if (!searchStateHasQuery(state)) return { ...state, suggestions: [], activeSuggestionIndex: -1 };
      return { ...state, phase: "closed", query: emptyLibrarySearchQuery(), suggestions: [], activeSuggestionIndex: -1, requestVersion: state.requestVersion + 1 };
    case "CLEAR":
      return { ...state, phase: "open", query: emptyLibrarySearchQuery(), suggestions: [], activeSuggestionIndex: -1, requestVersion: state.requestVersion + 1 };
    case "EXECUTE":
      return { ...state, phase: "closed", suggestions: [], activeSuggestionIndex: -1, requestVersion: state.requestVersion + 1 };
  }
}

export function applyLibrarySearchSuggestion(query: LibrarySearchQueryInput, suggestion: LibrarySearchSuggestion): LibrarySearchQuery {
  const normalized = normalizeLibrarySearchQuery(query);
  const consumesInput = suggestion.kind === "collection" || suggestion.kind === "libraryTag" || suggestion.kind === "field";
  const scope = suggestion.scope;
  return normalizeLibrarySearchQuery({
    ...normalized,
    collectionIds: suggestion.kind === "collection" && scope?.collectionIds
      ? [...normalized.collectionIds, ...scope.collectionIds]
      : normalized.collectionIds,
    libraryTagIds: suggestion.kind === "libraryTag" && scope?.libraryTagIds
      ? [...normalized.libraryTagIds, ...scope.libraryTagIds]
      : normalized.libraryTagIds,
    fieldClauses: suggestion.kind === "field" && scope?.fieldClauses
      ? [...normalized.fieldClauses, ...scope.fieldClauses]
      : normalized.fieldClauses,
    // The typed value that produced a Collection, Tag, or field suggestion is
    // committed into the token. Clear only that transient input so the user
    // can immediately continue composing the next token or free-text term.
    freeTextQuery: consumesInput ? "" : normalized.freeTextQuery,
  });
}

export interface LibrarySearchKeyboardInput {
  key: string;
  isComposing?: boolean;
}

/** Keyboard skeleton: IME composition always wins over Enter/Escape/navigation. */
export function reduceLibrarySearchKeyboard(state: LibrarySearchState, input: LibrarySearchKeyboardInput): LibrarySearchState {
  if (state.isComposing || input.isComposing || input.key === "Process" || input.key === "Unidentified") return state;
  if (input.key === "ArrowDown") return reduceLibrarySearchState(state, { type: "MOVE_ACTIVE", delta: 1 });
  if (input.key === "ArrowUp") return reduceLibrarySearchState(state, { type: "MOVE_ACTIVE", delta: -1 });
  // Enter has no Library Search behavior. In particular, it must not execute
  // text or turn a highlighted suggestion into a field/scope token.
  if (input.key === "Enter") return state;
  if (input.key === "Escape") return reduceLibrarySearchState(state, { type: "ESCAPE" });
  return state;
}

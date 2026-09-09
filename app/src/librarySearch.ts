/**
 * Library Search v0.2.1's framework-free state and adapter layer.
 *
 * The module deliberately knows nothing about Tauri or the DOM. It owns the
 * search contract (scope, field clauses, keyboard state, matching metadata)
 * so the Library UI can stay a thin renderer over canonical paper rows.
 */

export type LibrarySearchPhase = "closed" | "open" | "composing";
export type LibrarySearchSuggestionKind = "collection" | "libraryTag" | "paper";

/** The only fields exposed to full-text search. Keep metadata-only fields out. */
export const LIBRARY_SEARCH_FIELDS = [
  { key: "title", label: "英文标题", aliases: ["title", "英文标题"] },
  { key: "chineseTitle", label: "中文标题", aliases: ["chinese_title", "chinese-title", "中文标题"] },
  { key: "authors", label: "作者", aliases: ["author", "authors", "作者"] },
  { key: "year", label: "年份", aliases: ["year", "年份"] },
  { key: "journal", label: "期刊", aliases: ["journal", "source", "期刊"] },
  { key: "libraryTags", label: "Library Tag", aliases: ["tag", "tags", "library_tag", "library-tags", "标签"] },
  { key: "note", label: "备注", aliases: ["note", "备注"] },
  { key: "abstract", label: "英文摘要", aliases: ["abstract", "英文摘要"] },
  { key: "chineseAbstract", label: "中文摘要", aliases: ["chinese_abstract", "chinese-abstract", "中文摘要"] },
] as const;

export type LibrarySearchFieldKey = typeof LIBRARY_SEARCH_FIELDS[number]["key"];

/** Deliberately not included in LIBRARY_SEARCH_FIELDS. */
export const LIBRARY_SEARCH_EXCLUDED_FIELDS = ["doi", "url", "publisher", "volume", "issue", "pages"] as const;

export interface LibrarySearchFieldClause {
  field: LibrarySearchFieldKey;
  text: string;
}

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

export interface LibrarySearchScope {
  /** Multiple collections are OR-ed. */
  collectionIds: number[];
  /** Multiple Library Tags are AND-ed. */
  libraryTagIds: number[];
}

export interface LibrarySearchQuery extends LibrarySearchScope {
  /** Free text terms are AND-ed across the nine allowed search fields. */
  queryText: string;
  /** Explicit field clauses are AND-ed with free text and each other. */
  fieldClauses?: LibrarySearchFieldClause[];
}

export interface LibrarySearchRequest extends LibrarySearchQuery {
  view?: "all" | "recent" | "unfiled";
}

export interface LibrarySearchMatch {
  /** Stable field keys, suitable for compact UI labels and analytics. */
  matchedFields: LibrarySearchFieldKey[];
  /** Short, untrusted display excerpts keyed by the matched field. */
  snippets: Partial<Record<LibrarySearchFieldKey, string>>;
}

export interface LibrarySearchResult {
  papers: SearchPaper[];
  /** Canonical paper IDs make the de-duplication contract explicit. */
  paperIds: number[];
  /** Frontend fallback/adapter can carry deterministic match evidence. */
  matches?: Record<number, LibrarySearchMatch>;
}

export interface SearchSuggestionScope extends Partial<LibrarySearchScope> {
  queryText?: string;
}

export interface LibrarySearchSuggestion {
  id: string;
  kind: LibrarySearchSuggestionKind;
  label: string;
  detail?: string;
  scope?: SearchSuggestionScope;
  count?: number;
  paperId?: number;
  /** Zero-count tags stay visible, dimmed, clickable, and draggable. */
  dimmed?: boolean;
  draggable?: boolean;
  matchedFields?: LibrarySearchFieldKey[];
  snippet?: string;
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

export function emptyLibrarySearchScope(): LibrarySearchScope {
  return { collectionIds: [], libraryTagIds: [] };
}

export function emptyLibrarySearchQuery(): LibrarySearchQuery {
  return { ...emptyLibrarySearchScope(), queryText: "", fieldClauses: [] };
}

function uniqueIds(ids: readonly number[]): number[] {
  return [...new Set(ids.filter((id) => Number.isInteger(id) && id > 0))];
}

function fieldDefinition(field: string): typeof LIBRARY_SEARCH_FIELDS[number] | undefined {
  const needle = field.trim().toLocaleLowerCase();
  return LIBRARY_SEARCH_FIELDS.find((definition) => definition.aliases.some((alias) => alias.toLocaleLowerCase() === needle));
}

export function normalizeLibrarySearchFieldClauses(clauses: readonly Partial<LibrarySearchFieldClause>[] = []): LibrarySearchFieldClause[] {
  const seen = new Set<string>();
  const normalized: LibrarySearchFieldClause[] = [];
  for (const clause of clauses) {
    const field = fieldDefinition(String(clause.field || ""))?.key;
    const text = String(clause.text ?? "").trim();
    if (!field || !text) continue;
    const key = `${field}\u0000${text.toLocaleLowerCase()}`;
    if (seen.has(key)) continue;
    seen.add(key);
    normalized.push({ field, text });
  }
  return normalized;
}

export function normalizeLibrarySearchQuery(query: Partial<LibrarySearchQuery> = {}): LibrarySearchQuery {
  return {
    queryText: query.queryText?.trim() || "",
    collectionIds: uniqueIds(query.collectionIds || []),
    libraryTagIds: uniqueIds(query.libraryTagIds || []),
    fieldClauses: normalizeLibrarySearchFieldClauses(query.fieldClauses || []),
  };
}

/** True when a query has any text, explicit field token, or Library scope. */
export function hasLibrarySearchInput(query: Partial<LibrarySearchQuery>): boolean {
  const normalized = normalizeLibrarySearchQuery(query);
  return Boolean(normalized.queryText || normalized.fieldClauses?.length || normalized.collectionIds.length || normalized.libraryTagIds.length);
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

function valueText(value: string | number | null | undefined): string {
  return value == null ? "" : String(value);
}

function authorText(authors: SearchPaper["authors"]): string {
  return Array.isArray(authors) ? authors.join(" ") : valueText(authors);
}

export function getLibrarySearchFieldValues(paper: SearchPaper): Record<LibrarySearchFieldKey, string> {
  return {
    title: valueText(paper.title),
    chineseTitle: valueText(paper.chineseTitle),
    authors: authorText(paper.authors),
    year: valueText(paper.year),
    journal: valueText(paper.source),
    libraryTags: (paper.tags || []).join(" "),
    note: valueText(paper.note),
    abstract: valueText(paper.abstract),
    chineseAbstract: valueText(paper.chineseAbstract),
  };
}

function allSearchFields(paper: SearchPaper): Array<[LibrarySearchFieldKey, string]> {
  const values = getLibrarySearchFieldValues(paper);
  return LIBRARY_SEARCH_FIELDS.map(({ key }) => [key, values[key]]);
}

function searchTerms(value: string): string[] {
  return value.toLocaleLowerCase().split(/\s+/u).map((term) => term.trim()).filter(Boolean);
}

function containsTerm(value: string, term: string): boolean {
  return value.toLocaleLowerCase().includes(term.toLocaleLowerCase());
}

/** Make a bounded excerpt around the first matching term without HTML markup. */
export function buildLibrarySearchSnippet(value: string, terms: readonly string[], maxLength = 96): string {
  const clean = value.trim().replace(/\s+/gu, " ");
  if (!clean) return "";
  const lower = clean.toLocaleLowerCase();
  const match = terms.map((term) => lower.indexOf(term.toLocaleLowerCase())).filter((index) => index >= 0).sort((a, b) => a - b)[0] ?? 0;
  if (clean.length <= maxLength) return clean;
  const half = Math.max(18, Math.floor((maxLength - 2) / 2));
  let start = Math.max(0, match - half);
  let end = Math.min(clean.length, start + maxLength - 2);
  if (end - start < maxLength - 2) start = Math.max(0, end - (maxLength - 2));
  const prefix = start > 0 ? "…" : "";
  const suffix = end < clean.length ? "…" : "";
  return `${prefix}${clean.slice(start, end).trim()}${suffix}`;
}

/** Return matched field keys and bounded snippets for a paper/query pair. */
export function matchLibrarySearchPaper(paper: SearchPaper, query: Pick<LibrarySearchQuery, "queryText" | "fieldClauses">): LibrarySearchMatch | null {
  const normalized = normalizeLibrarySearchQuery(query);
  const fields = allSearchFields(paper);
  const values = Object.fromEntries(fields) as Record<LibrarySearchFieldKey, string>;
  const matched = new Set<LibrarySearchFieldKey>();
  const termsByField = new Map<LibrarySearchFieldKey, string[]>();

  const freeTerms = searchTerms(normalized.queryText);
  for (const term of freeTerms) {
    const hits = fields.filter(([, value]) => containsTerm(value, term)).map(([field]) => field);
    if (!hits.length) return null;
    for (const field of hits) {
      matched.add(field);
      termsByField.set(field, [...(termsByField.get(field) || []), term]);
    }
  }

  for (const clause of normalized.fieldClauses || []) {
    const terms = searchTerms(clause.text);
    const value = values[clause.field];
    if (!terms.length || !terms.every((term) => containsTerm(value, term))) return null;
    matched.add(clause.field);
    termsByField.set(clause.field, [...(termsByField.get(clause.field) || []), ...terms]);
  }

  if (!freeTerms.length && !(normalized.fieldClauses || []).length) return { matchedFields: [], snippets: {} };
  const matchedFields = LIBRARY_SEARCH_FIELDS.map(({ key }) => key).filter((field) => matched.has(field));
  const snippets: Partial<Record<LibrarySearchFieldKey, string>> = {};
  for (const field of matchedFields) {
    const snippet = buildLibrarySearchSnippet(values[field], termsByField.get(field) || [], 96);
    if (snippet) snippets[field] = snippet;
  }
  return { matchedFields, snippets };
}

export function matchesLibrarySearchText(paper: SearchPaper, queryText: string): boolean {
  return matchLibrarySearchPaper(paper, { queryText, fieldClauses: [] }) !== null;
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

/** Client fallback used by the pre-backend adapter and deterministic tests. */
export function filterLibrarySearchPapers(papers: readonly SearchPaper[], query: LibrarySearchQuery): SearchPaper[] {
  const normalized = normalizeLibrarySearchQuery(query);
  const byId = new Map<number, SearchPaper>();
  for (const paper of papers) {
    if (byId.has(paper.id)) continue;
    if (!hasAnyCollection(paper, normalized.collectionIds)) continue;
    if (!hasAllTags(paper, normalized.libraryTagIds)) continue;
    if (!matchLibrarySearchPaper(paper, normalized)) continue;
    byId.set(paper.id, paper);
  }
  return [...byId.values()];
}

/** Parse `field:value` / `field:"quoted value"` clauses from the search input. */
export function parseLibrarySearchInput(input: string): { queryText: string; fieldClauses: LibrarySearchFieldClause[] } {
  const clauses: LibrarySearchFieldClause[] = [];
  const ranges: Array<[number, number]> = [];
  const pattern = /(^|\s)([A-Za-z_][\w-]*|[\u3400-\u9fff\uF900-\uFAFF]+)\s*:\s*(?:"([^"]+)"|'([^']+)'|([^\s]+))/gu;
  for (const match of input.matchAll(pattern)) {
    const definition = fieldDefinition(match[2]);
    if (!definition) continue;
    const text = (match[3] ?? match[4] ?? match[5] ?? "").trim();
    if (!text) continue;
    clauses.push({ field: definition.key, text });
    const start = match.index ?? 0;
    ranges.push([start, start + match[0].length]);
  }
  if (!ranges.length) return { queryText: input.trim(), fieldClauses: [] };
  let freeText = input;
  for (const [start, end] of [...ranges].sort((a, b) => b[0] - a[0])) freeText = `${freeText.slice(0, start)} ${freeText.slice(end)}`;
  return { queryText: freeText.replace(/\s+/gu, " ").trim(), fieldClauses: normalizeLibrarySearchFieldClauses(clauses) };
}

/** Build lightweight Collection/Tag/Paper suggestions. There is no Search Action. */
export function buildLibrarySearchSuggestions(index: LibrarySearchIndex, query: LibrarySearchQuery): LibrarySearchSuggestion[] {
  const normalized = normalizeLibrarySearchQuery(query);
  const needle = normalized.queryText.trim().toLocaleLowerCase();
  const includes = (label: string) => !needle || label.toLocaleLowerCase().includes(needle);
  const suggestions: LibrarySearchSuggestion[] = [];
  for (const collection of index.collections) {
    if (!includes(collection.name)) continue;
    suggestions.push({ id: `collection:${collection.id}`, kind: "collection", label: collection.name, detail: "文集 · OR（含子文集）", scope: { collectionIds: [collection.id] }, draggable: true });
  }
  for (const tag of index.tags) {
    if (!includes(tag.name)) continue;
    const count = tagCount(index, tag.id);
    suggestions.push({ id: `libraryTag:${tag.id}`, kind: "libraryTag", label: tag.name, detail: `Library Tag · ${count}`, count, dimmed: count === 0, draggable: true, scope: { libraryTagIds: [tag.id] } });
  }
  for (const paper of index.papers) {
    const label = valueText(paper.title || paper.chineseTitle) || `Paper #${paper.id}`;
    if (!includes(label)) continue;
    const match = matchLibrarySearchPaper(paper, normalized);
    suggestions.push({
      id: `paper:${paper.id}`,
      kind: "paper",
      label,
      detail: match?.matchedFields.length ? match.matchedFields.map((field) => LIBRARY_SEARCH_FIELDS.find((item) => item.key === field)?.label || field).join(" · ") : (paper.source || "论文"),
      paperId: paper.id,
      matchedFields: match?.matchedFields,
      snippet: match ? Object.values(match.snippets)[0] : undefined,
    });
  }
  return suggestions;
}

export function applyLibrarySearchSuggestion(query: LibrarySearchQuery, suggestion: LibrarySearchSuggestion): LibrarySearchQuery {
  const next = normalizeLibrarySearchQuery(query);
  if (suggestion.kind === "collection" && suggestion.scope?.collectionIds) {
    next.collectionIds = uniqueIds([...next.collectionIds, ...suggestion.scope.collectionIds]);
    next.queryText = "";
  }
  if (suggestion.kind === "libraryTag" && suggestion.scope?.libraryTagIds) {
    next.libraryTagIds = uniqueIds([...next.libraryTagIds, ...suggestion.scope.libraryTagIds]);
    next.queryText = "";
  }
  return next;
}

export interface LibrarySearchAdapterDependencies {
  listPapers: (request: { view: "all" | "recent" | "unfiled"; collectionId: number | null; tagIds: number[] }) => Promise<SearchPaper[]>;
  listCollections: () => Promise<SearchCollection[]>;
  listTags: () => Promise<SearchLibraryTag[]>;
  getTagCounts?: (collectionId: number | null) => Promise<Map<number, number> | Record<number, number>>;
}

/** Adapter over the existing Library commands. */
export function createLibrarySearchAdapter(deps: LibrarySearchAdapterDependencies): LibrarySearchApi {
  return {
    async search(request) {
      const query = normalizeLibrarySearchQuery(request);
      const view = request.view || "all";
      const collections = query.collectionIds.length ? await deps.listCollections() : [];
      const collectionIds = query.collectionIds.length ? expandCollectionIds(collections, query.collectionIds) : [null];
      const batches = await Promise.all(collectionIds.map((collectionId) => deps.listPapers({ view, collectionId, tagIds: query.libraryTagIds })));
      const papers = filterLibrarySearchPapers(batches.flat(), { ...query, collectionIds: [] });
      const matches: Record<number, LibrarySearchMatch> = {};
      for (const paper of papers) {
        const match = matchLibrarySearchPaper(paper, query);
        if (match) matches[paper.id] = match;
      }
      return { papers, paperIds: papers.map((paper) => paper.id), matches };
    },
    async getSuggestions(request) {
      const query = normalizeLibrarySearchQuery(request);
      const [collections, tags, result] = await Promise.all([
        deps.listCollections(),
        deps.listTags(),
        this.search({ ...query, view: request.view || "all" }),
      ]);
      const counts = deps.getTagCounts ? await deps.getTagCounts(query.collectionIds[0] || null) : undefined;
      return buildLibrarySearchSuggestions({ collections, tags, papers: result.papers, tagCounts: counts }, query);
    },
  };
}

export interface LibrarySearchState {
  phase: LibrarySearchPhase;
  query: LibrarySearchQuery;
  suggestions: LibrarySearchSuggestion[];
  activeSuggestionIndex: number;
  isComposing: boolean;
  requestVersion: number;
  /** Last raw input value; field clauses are committed on Enter. */
  rawInput: string;
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
  | { type: "ESCAPE" }
  | { type: "CLEAR" }
  | { type: "EXECUTE" };

export function createLibrarySearchState(query: Partial<LibrarySearchQuery> = {}): LibrarySearchState {
  const normalized = normalizeLibrarySearchQuery(query);
  return { phase: "closed", query: normalized, suggestions: [], activeSuggestionIndex: -1, isComposing: false, requestVersion: 0, rawInput: normalized.queryText };
}

function hasTextOrFieldClause(query: LibrarySearchQuery): boolean {
  return Boolean(query.queryText || query.fieldClauses?.length);
}

export function reduceLibrarySearchState(state: LibrarySearchState, action: LibrarySearchAction): LibrarySearchState {
  switch (action.type) {
    case "FOCUS": return { ...state, phase: state.isComposing ? "composing" : "open" };
    case "INPUT": return { ...state, phase: state.isComposing ? "composing" : "open", query: normalizeLibrarySearchQuery({ ...state.query, queryText: action.text }), rawInput: action.text, activeSuggestionIndex: -1, requestVersion: state.requestVersion + 1 };
    case "START_COMPOSITION": return { ...state, phase: "composing", isComposing: true };
    case "END_COMPOSITION": {
      const text = action.text == null ? state.query.queryText : action.text;
      return { ...state, phase: "open", isComposing: false, query: normalizeLibrarySearchQuery({ ...state.query, queryText: text }), rawInput: text, activeSuggestionIndex: -1, requestVersion: state.requestVersion + 1 };
    }
    case "SUGGESTIONS": return { ...state, suggestions: action.suggestions, activeSuggestionIndex: action.suggestions.length ? 0 : -1 };
    case "MOVE_ACTIVE": {
      const count = state.suggestions.length;
      if (!count) return state;
      const next = state.activeSuggestionIndex < 0 ? (action.delta > 0 ? 0 : count - 1) : (state.activeSuggestionIndex + action.delta + count) % count;
      return { ...state, activeSuggestionIndex: next };
    }
    case "SELECT_ACTIVE": {
      const suggestion = state.suggestions[state.activeSuggestionIndex];
      return suggestion ? reduceLibrarySearchState(state, { type: "SELECT_SUGGESTION", suggestion }) : { ...state, phase: "open", requestVersion: state.requestVersion + 1 };
    }
    case "SELECT_SUGGESTION": return { ...state, phase: "open", query: applyLibrarySearchSuggestion(state.query, action.suggestion), rawInput: "", activeSuggestionIndex: -1, requestVersion: state.requestVersion + 1 };
    // Escape is deliberately two-stage: close the suggestion surface first;
    // the next Escape clears the committed query/scope input.
    case "ESCAPE": return state.phase !== "closed"
      ? { ...state, phase: "closed", activeSuggestionIndex: -1 }
      : hasTextOrFieldClause(state.query)
        ? { ...state, query: emptyLibrarySearchQuery(), rawInput: "", activeSuggestionIndex: -1, requestVersion: state.requestVersion + 1 }
        : state;
    case "CLEAR": return { ...state, phase: "closed", query: emptyLibrarySearchQuery(), rawInput: "", activeSuggestionIndex: -1, requestVersion: state.requestVersion + 1 };
    case "EXECUTE": return { ...state, phase: "closed", activeSuggestionIndex: -1, requestVersion: state.requestVersion + 1 };
  }
}

export interface LibrarySearchKeyboardInput {
  key: string;
  isComposing?: boolean;
}

/** IME composition always wins over Enter/Escape/navigation. */
export function reduceLibrarySearchKeyboard(state: LibrarySearchState, input: LibrarySearchKeyboardInput): LibrarySearchState {
  if (state.isComposing || input.isComposing || input.key === "Process" || input.key === "Unidentified") return state;
  if (input.key === "ArrowDown") return reduceLibrarySearchState(state, { type: "MOVE_ACTIVE", delta: 1 });
  if (input.key === "ArrowUp") return reduceLibrarySearchState(state, { type: "MOVE_ACTIVE", delta: -1 });
  if (input.key === "Enter") return reduceLibrarySearchState(state, { type: state.activeSuggestionIndex >= 0 ? "SELECT_ACTIVE" : "EXECUTE" });
  if (input.key === "Escape") return reduceLibrarySearchState(state, { type: "ESCAPE" });
  return state;
}

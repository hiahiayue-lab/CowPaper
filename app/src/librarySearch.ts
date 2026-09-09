/**
 * Library Search v0.2.1's framework-free state and adapter layer.
 *
 * This module deliberately knows nothing about Tauri or the DOM.  The UI can
 * replace the adapter with a native backend implementation without changing
 * keyboard, scope, suggestion, or de-duplication semantics.
 */

export type LibrarySearchPhase = "closed" | "open" | "composing";
export type LibrarySearchSuggestionKind = "collection" | "libraryTag" | "paper" | "searchAction";

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
  /** Search terms are AND-ed across the complete Library search projection. */
  queryText: string;
}

export interface LibrarySearchRequest extends LibrarySearchQuery {
  view?: "all" | "recent" | "unfiled";
}

export interface LibrarySearchResult {
  papers: SearchPaper[];
  /** Canonical paper IDs make the de-duplication contract explicit. */
  paperIds: number[];
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
  return { ...emptyLibrarySearchScope(), queryText: "" };
}

function uniqueIds(ids: readonly number[]): number[] {
  return [...new Set(ids.filter((id) => Number.isInteger(id) && id > 0))];
}

export function normalizeLibrarySearchQuery(query: Partial<LibrarySearchQuery> = {}): LibrarySearchQuery {
  return {
    queryText: query.queryText?.trim() || "",
    collectionIds: uniqueIds(query.collectionIds || []),
    libraryTagIds: uniqueIds(query.libraryTagIds || []),
  };
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

function searchFields(paper: SearchPaper): string[] {
  // Keep this list identical to the product contract. Publisher, DOI, URL,
  // volume, issue, and pages remain display metadata but are not searchable.
  return [
    paper.title,
    paper.chineseTitle,
    authorText(paper.authors),
    paper.year,
    paper.source,
    ...(paper.tags || []),
    paper.note,
    paper.abstract,
    paper.chineseAbstract,
  ].map(valueText);
}

export function matchesLibrarySearchText(paper: SearchPaper, queryText: string): boolean {
  const terms = queryText.toLocaleLowerCase().split(/\s+/u).map((term) => term.trim()).filter(Boolean);
  if (!terms.length) return true;
  const haystack = searchFields(paper).join(" ").toLocaleLowerCase();
  return terms.every((term) => haystack.includes(term));
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

/** Client fallback used by the pre-backend adapter and by deterministic tests. */
export function filterLibrarySearchPapers(papers: readonly SearchPaper[], query: LibrarySearchQuery): SearchPaper[] {
  const normalized = normalizeLibrarySearchQuery(query);
  const byId = new Map<number, SearchPaper>();
  for (const paper of papers) {
    if (!hasAnyCollection(paper, normalized.collectionIds)) continue;
    if (!hasAllTags(paper, normalized.libraryTagIds)) continue;
    if (!matchesLibrarySearchText(paper, normalized.queryText)) continue;
    byId.set(paper.id, paper);
  }
  return [...byId.values()];
}

function tagCount(index: LibrarySearchIndex, id: number): number {
  if (index.tagCounts instanceof Map) return index.tagCounts.get(id) || 0;
  return index.tagCounts?.[id] || 0;
}

/** Build one flat dropdown list; ordering is stable and category-specific. */
export function buildLibrarySearchSuggestions(index: LibrarySearchIndex, query: LibrarySearchQuery): LibrarySearchSuggestion[] {
  const needle = query.queryText.trim().toLocaleLowerCase();
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
    suggestions.push({ id: `paper:${paper.id}`, kind: "paper", label, detail: paper.source || "论文", paperId: paper.id });
  }
  if (needle) {
    suggestions.push({ id: "action:search", kind: "searchAction", label: `在当前范围搜索“${query.queryText.trim()}”`, detail: "Library Search" });
  }
  return suggestions;
}

export function applyLibrarySearchSuggestion(query: LibrarySearchQuery, suggestion: LibrarySearchSuggestion): LibrarySearchQuery {
  const next = normalizeLibrarySearchQuery(query);
  if (suggestion.kind === "collection" && suggestion.scope?.collectionIds) {
    next.collectionIds = uniqueIds([...next.collectionIds, ...suggestion.scope.collectionIds]);
  }
  if (suggestion.kind === "libraryTag" && suggestion.scope?.libraryTagIds) next.libraryTagIds = uniqueIds([...next.libraryTagIds, ...suggestion.scope.libraryTagIds]);
  return next;
}

export interface LibrarySearchAdapterDependencies {
  listPapers: (request: { view: "all" | "recent" | "unfiled"; collectionId: number | null; tagIds: number[] }) => Promise<SearchPaper[]>;
  listCollections: () => Promise<SearchCollection[]>;
  listTags: () => Promise<SearchLibraryTag[]>;
  getTagCounts?: (collectionId: number | null) => Promise<Map<number, number> | Record<number, number>>;
}

/** Adapter over the existing Library commands. The UI can swap in the native search API. */
export function createLibrarySearchAdapter(deps: LibrarySearchAdapterDependencies): LibrarySearchApi {
  return {
    async search(request) {
      const query = normalizeLibrarySearchQuery(request);
      const view = request.view || "all";
      const collections = query.collectionIds.length ? await deps.listCollections() : [];
      const collectionIds = query.collectionIds.length ? expandCollectionIds(collections, query.collectionIds) : [null];
      const batches = await Promise.all(collectionIds.map((collectionId) => deps.listPapers({ view, collectionId, tagIds: query.libraryTagIds })));
      const papers = filterLibrarySearchPapers(batches.flat(), { ...query, collectionIds: [] });
      const deduped = new Map<number, SearchPaper>();
      for (const paper of papers) deduped.set(paper.id, paper);
      return { papers: [...deduped.values()], paperIds: [...deduped.keys()] };
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
  return { phase: "closed", query: normalizeLibrarySearchQuery(query), suggestions: [], activeSuggestionIndex: -1, isComposing: false, requestVersion: 0 };
}

export function reduceLibrarySearchState(state: LibrarySearchState, action: LibrarySearchAction): LibrarySearchState {
  switch (action.type) {
    case "FOCUS": return { ...state, phase: state.isComposing ? "composing" : "open" };
    case "INPUT": return { ...state, phase: state.isComposing ? "composing" : "open", query: normalizeLibrarySearchQuery({ ...state.query, queryText: action.text }), activeSuggestionIndex: -1, requestVersion: state.requestVersion + 1 };
    case "START_COMPOSITION": return { ...state, phase: "composing", isComposing: true };
    case "END_COMPOSITION": return { ...state, phase: "open", isComposing: false, query: action.text == null ? state.query : normalizeLibrarySearchQuery({ ...state.query, queryText: action.text }), activeSuggestionIndex: -1, requestVersion: state.requestVersion + 1 };
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
    case "SELECT_SUGGESTION": return { ...state, phase: "open", query: applyLibrarySearchSuggestion(state.query, action.suggestion), activeSuggestionIndex: -1, requestVersion: state.requestVersion + 1 };
    case "ESCAPE": return { ...state, phase: "closed", activeSuggestionIndex: -1 };
    case "CLEAR": return { ...state, phase: "open", query: emptyLibrarySearchQuery(), activeSuggestionIndex: -1, requestVersion: state.requestVersion + 1 };
    case "EXECUTE": return { ...state, phase: "closed", activeSuggestionIndex: -1, requestVersion: state.requestVersion + 1 };
  }
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
  if (input.key === "Enter") return reduceLibrarySearchState(state, { type: state.activeSuggestionIndex >= 0 ? "SELECT_ACTIVE" : "EXECUTE" });
  if (input.key === "Escape") return reduceLibrarySearchState(state, { type: "ESCAPE" });
  return state;
}

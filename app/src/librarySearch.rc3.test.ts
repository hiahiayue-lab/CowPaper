import {
  LIBRARY_SEARCH_EXCLUDED_FIELDS,
  LIBRARY_SEARCH_FIELDS,
  applyLibrarySearchSuggestion,
  applyLibrarySearchFieldClause,
  applyLibrarySearchSidebarScope,
  buildLibrarySearchFieldTokens,
  buildLibrarySearchSuggestions,
  createLibrarySearchState,
  dedupeLibrarySearchPapers,
  emptyLibrarySearchQuery,
  filterLibrarySearchPapers,
  matchLibrarySearchPaper,
  matchesLibrarySearchQuery,
  reduceLibrarySearchKeyboard,
  reduceLibrarySearchState,
  type LibrarySearchSuggestion,
  type SearchPaper,
} from "./librarySearch.ts";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function equal<T>(actual: T, expected: T, message: string): void {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${message}: ${JSON.stringify(actual)} !== ${JSON.stringify(expected)}`);
  }
}

const paper: SearchPaper = {
  id: 101,
  title: "AI Governance",
  chineseTitle: "人工智能治理",
  authors: ["Ada Lovelace", "Grace Hopper"],
  source: "Journal X",
  year: 2024,
  publisher: "Display Publisher Only",
  doi: "10.5555/rc3-excluded",
  url: "https://example.invalid/rc3-excluded",
  volume: "99",
  issue: "7",
  pages: "1-9",
  note: "keep this Library note",
  abstract: "Platforms shape governance in practice.",
  chineseAbstract: "平台治理摘要",
  collectionIds: [10, 11],
  tagIds: [1, 2],
  tags: ["Core", "Review"],
  annotationText: "Reviewer highlighted the governance mechanism.",
};
const secondPaper: SearchPaper = {
  id: 102,
  title: "Methods for Networks",
  source: "Journal Y",
  year: 2023,
  abstract: "Econometric methods for network data.",
  collectionIds: [20],
  tagIds: [1],
  tags: ["Core"],
};
const duplicatePaper = { ...paper, title: "Duplicate row must not escape papers.id", annotationText: "A second annotation on the same paper." };

// API field contract: the nine original fields plus the v0.3 annotation field,
// with display metadata
// kept outside the projection.
equal(LIBRARY_SEARCH_FIELDS.length, 10, "v0.3 has ten searchable fields");
assert(LIBRARY_SEARCH_FIELDS.includes("libraryTags"), "Library Tags are searchable");
assert(LIBRARY_SEARCH_EXCLUDED_FIELDS.every((field) => !LIBRARY_SEARCH_FIELDS.includes(field as never)), "excluded fields are not allowed fields");
for (const field of LIBRARY_SEARCH_EXCLUDED_FIELDS) {
  assert(!matchesLibrarySearchQuery(paper, { freeTextQuery: paper[field as keyof SearchPaper] as string }), `${field} is metadata-only`);
}

// Field clauses are narrow and AND-ed with free text across the complete
// allowed projection.
const fieldQuery = applyLibrarySearchFieldClause({ freeTextQuery: "平台", collectionIds: [], libraryTagIds: [] }, { field: "title", query: "AI" });
assert(matchesLibrarySearchQuery(paper, fieldQuery), "field clause composes with free text");
assert(!matchesLibrarySearchQuery(paper, { ...fieldQuery, fieldClauses: [{ field: "authors", query: "AI" }] }), "field clause does not leak across fields");
const fieldTokens = buildLibrarySearchFieldTokens(fieldQuery);
equal(fieldTokens.map((token) => token.field), ["title"], "field clauses render as field tokens");

// Collection OR + Tag AND + text/field dimensions compose with AND, and every
// result is unique by canonical papers.id.
const resultPapers = filterLibrarySearchPapers([paper, duplicatePaper, secondPaper], {
  ...emptyLibrarySearchQuery(),
  freeTextQuery: "governance",
  collectionIds: [10, 20],
  libraryTagIds: [1, 2],
});
equal(resultPapers.map(({ id }) => id), [101], "scope dimensions and papers.id de-dup");
equal(dedupeLibrarySearchPapers([paper, duplicatePaper, secondPaper]).map(({ id }) => id), [101, 102], "canonical row de-dup");

const hit = matchLibrarySearchPaper(paper, { ...emptyLibrarySearchQuery(), freeTextQuery: "治理" });
assert(hit.paperId === 101, "hit keeps canonical paper id");
assert(hit.matched_fields.includes("chineseTitle") && hit.matched_fields.includes("chineseAbstract"), "matched_fields reports actual fields");
assert(hit.matched_fields.every((field) => LIBRARY_SEARCH_FIELDS.includes(field)), "matched_fields never reports excluded metadata");
assert(hit.snippets.length <= 3 && hit.snippets.every((snippet) => snippet.text.length <= 120), "snippets are short and bounded");
const annotationHit = matchLibrarySearchPaper(paper, { ...emptyLibrarySearchQuery(), freeTextQuery: "highlighted" });
assert(annotationHit.matched_fields.includes("annotation"), "annotation hits expose the Annotation field");
assert(annotationHit.snippets.some((snippet) => snippet.field === "annotation" && snippet.text.includes("highlighted")), "annotation hits expose a short snippet");
const annotationRows = filterLibrarySearchPapers([paper, duplicatePaper], { ...emptyLibrarySearchQuery(), freeTextQuery: "annotation" });
assert(annotationRows.length === 1 && annotationRows[0].id === paper.id, "multiple annotation-bearing rows collapse to one canonical paper row");

// The dropdown contains only Library-local suggestions; selecting scope does
// not destroy an already-entered free-text expression.
const collectionSuggestion = buildLibrarySearchSuggestions({
  collections: [{ id: 10, parentId: null, name: "AI" }],
  tags: [{ id: 1, name: "Core" }],
  papers: [paper, duplicatePaper],
  tagCounts: new Map([[1, 1]]),
}, { ...emptyLibrarySearchQuery(), freeTextQuery: "AI" }).find((item) => item.id === "collection:10")!;
const chineseTitleSuggestion = buildLibrarySearchSuggestions({
  collections: [],
  tags: [],
  papers: [paper],
}, { ...emptyLibrarySearchQuery(), freeTextQuery: "AI" }).find((item) => item.kind === "field" && item.field === "chineseTitle")!;
assert(chineseTitleSuggestion.label.includes("中文标题中搜索"), "field intent names the searched field");
const fieldTokenQuery = applyLibrarySearchSuggestion({ ...emptyLibrarySearchQuery(), freeTextQuery: "AI" }, chineseTitleSuggestion);
equal(fieldTokenQuery.fieldClauses, [{ field: "chineseTitle", query: "AI" }], "field suggestion locks a clause");
equal(fieldTokenQuery.freeTextQuery, "", "field suggestion consumes transient input");
const tagSuggestion: LibrarySearchSuggestion = { id: "libraryTag:1", kind: "libraryTag", label: "Core", scope: { libraryTagIds: [1] } };
let state = createLibrarySearchState();
state = reduceLibrarySearchState(state, { type: "SELECT_SUGGESTION", suggestion: collectionSuggestion });
state = reduceLibrarySearchState(state, { type: "INPUT", text: "治理" });
state = reduceLibrarySearchState(state, { type: "SELECT_SUGGESTION", suggestion: tagSuggestion });
equal(state.query.collectionIds, [10], "Collection token remains locked");
equal(state.query.libraryTagIds, [1], "Tag token remains locked");
equal(state.query.freeTextQuery, "", "token selection consumes transient input");
state = reduceLibrarySearchState(state, { type: "INPUT", text: "治理" });
equal(state.query.freeTextQuery, "治理", "Collection/Tag/text remain composable");
assert(!buildLibrarySearchSuggestions({ collections: [], tags: [], papers: [] }, { ...emptyLibrarySearchQuery(), freeTextQuery: "network" }).some((item) => (item.kind as string) === "searchAction"), "Search Action is removed");

const sidebarQuery = applyLibrarySearchSidebarScope({
  ...state.query,
  fieldClauses: [{ field: "title", query: "AI" }],
}, { collectionId: 20, libraryTagIds: [2] });
equal(sidebarQuery, { fieldClauses: [{ field: "title", query: "AI" }], freeTextQuery: "治理", collectionIds: [20], libraryTagIds: [2] }, "Sidebar auto-token preserves text and field tokens");

// Outside click and Escape have deliberately different ownership. Outside
// click/Escape #1 only close suggestions; Escape #2 clears the expression.
let focused = reduceLibrarySearchState(state, { type: "FOCUS" });
focused = reduceLibrarySearchState(focused, { type: "SUGGESTIONS", suggestions: [collectionSuggestion] });
const plainEnter = reduceLibrarySearchKeyboard(focused, { key: "Enter" });
assert(plainEnter === focused, "plain Enter is a no-op");
equal(plainEnter.query.collectionIds, state.query.collectionIds, "plain Enter does not auto-select the first suggestion");
assert(plainEnter.query.freeTextQuery === "治理", "plain Enter preserves the typed query");
const navigatedEnter = reduceLibrarySearchKeyboard(
  reduceLibrarySearchState(focused, { type: "MOVE_ACTIVE", delta: 1 }),
  { key: "Enter" },
);
assert(navigatedEnter.query.freeTextQuery === "治理", "Enter remains inert after suggestion navigation");
equal(navigatedEnter.query.collectionIds, state.query.collectionIds, "Enter does not convert a highlighted suggestion into a token");
const queryBeforeDismiss = focused.query;
const versionBeforeDismiss = focused.requestVersion;
const outside = reduceLibrarySearchState(focused, { type: "OUTSIDE_CLICK" });
equal(outside.query, queryBeforeDismiss, "outside click preserves query and tokens");
assert(outside.phase === "closed" && outside.requestVersion === versionBeforeDismiss, "outside click only closes suggestion");
const firstEscape = reduceLibrarySearchKeyboard(focused, { key: "Escape" });
equal(firstEscape.query, queryBeforeDismiss, "Escape #1 preserves query and tokens");
const secondEscape = reduceLibrarySearchKeyboard(firstEscape, { key: "Escape" });
equal(secondEscape.query, emptyLibrarySearchQuery(), "Escape #2 clears complete search expression");

// Chinese runtime strings and IME composition must not execute, close, or
// navigate until compositionend.
assert(matchesLibrarySearchQuery(paper, { freeTextQuery: "人工智能 AI 治理" }), "mixed-language runtime search remains ANDed");
let composing = reduceLibrarySearchState(focused, { type: "START_COMPOSITION" });
for (const key of ["Enter", "Escape", "ArrowDown"]) {
  assert(reduceLibrarySearchKeyboard(composing, { key }).phase === "composing", `IME protects ${key}`);
}
composing = reduceLibrarySearchState(composing, { type: "END_COMPOSITION", text: "人工智能" });
assert(!composing.isComposing && composing.query.freeTextQuery === "人工智能", "compositionend commits Chinese text");

console.log("librarySearch RC3 contract tests passed");

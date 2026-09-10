import {
  applyLibrarySearchSuggestion,
  buildLibrarySearchFieldTokens,
  buildLibrarySearchSuggestions,
  createLibrarySearchState,
  emptyLibrarySearchQuery,
  expandCollectionIds,
  filterLibrarySearchPapers,
  matchesLibrarySearchQuery,
  reduceLibrarySearchKeyboard,
  reduceLibrarySearchState,
  type SearchPaper,
} from "./librarySearch.ts";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function equal<T>(actual: T, expected: T, message: string): void {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error(`${message}: ${JSON.stringify(actual)} !== ${JSON.stringify(expected)}`);
}

const papers: SearchPaper[] = [
  { id: 1, title: "AI and strategy", source: "Journal A", abstract: "content alpha", collectionIds: [10, 11], tagIds: [1, 2] },
  { id: 2, title: "Strategy routines", source: "Journal B", abstract: "content beta", collectionIds: [12], tagIds: [1] },
  // Same canonical paper returned by two collection requests must remain once.
  { id: 1, title: "AI and strategy", source: "Journal A", collectionIds: [10, 11], tagIds: [1, 2] },
];

equal(expandCollectionIds([
  { id: 10, parentId: null, name: "Root" },
  { id: 11, parentId: 10, name: "Child" },
  { id: 12, parentId: 11, name: "Grandchild" },
], [10]), [10, 11, 12], "parent descendants");

equal(filterLibrarySearchPapers(papers, {
  freeTextQuery: "AI strategy", fieldClauses: [], collectionIds: [10, 12], libraryTagIds: [1, 2],
}).map((paper) => paper.id), [1], "OR collections + AND tags + text");

equal(filterLibrarySearchPapers(papers, {
  freeTextQuery: "content", fieldClauses: [], collectionIds: [], libraryTagIds: [],
}).map((paper) => paper.id), [1, 2], "all-field search");

const suggestionQuery = { freeTextQuery: "", fieldClauses: [], collectionIds: [], libraryTagIds: [] };
const suggestions = buildLibrarySearchSuggestions({
  collections: [{ id: 10, parentId: null, name: "Root" }],
  tags: [{ id: 1, name: "Active" }, { id: 99, name: "Unused" }],
  papers,
  tagCounts: new Map([[1, 2], [99, 0]]),
}, suggestionQuery);
const unused = suggestions.find((suggestion) => suggestion.id === "libraryTag:99");
assert(unused?.dimmed === true, "zero-count tags are dimmed");
assert(unused?.draggable === true, "zero-count tags remain draggable");
equal(applyLibrarySearchSuggestion(suggestionQuery, unused!), { fieldClauses: [], freeTextQuery: "", collectionIds: [], libraryTagIds: [99] }, "tag suggestion updates scope");

const typedSuggestions = buildLibrarySearchSuggestions({ collections: [], tags: [], papers: [] }, { ...suggestionQuery, freeTextQuery: "network" });
assert(typedSuggestions.some((suggestion) => suggestion.kind === "field" && suggestion.field === "title"), "field intents are available for typed text");
assert(!typedSuggestions.some((suggestion) => (suggestion.kind as string) === "searchAction"), "Search Action is removed");
assert(suggestions.some((suggestion) => suggestion.id.startsWith("paper:")), "paper suggestions remain available for direct selection");

const fieldQuery = {
  ...suggestionQuery,
  freeTextQuery: "content",
  fieldClauses: [{ field: "title" as const, query: "AI" }],
};
assert(matchesLibrarySearchQuery(papers[0], fieldQuery), "field clause composes with free text");
assert(!matchesLibrarySearchQuery(papers[1], fieldQuery), "field clause stays in its named field");
equal(buildLibrarySearchFieldTokens(fieldQuery).map((token) => token.field), ["title"], "field token projection");
equal(emptyLibrarySearchQuery(), { fieldClauses: [], freeTextQuery: "", collectionIds: [], libraryTagIds: [] }, "empty RC3 query");

let state = createLibrarySearchState();
state = reduceLibrarySearchState(state, { type: "FOCUS" });
state = reduceLibrarySearchState(state, { type: "SUGGESTIONS", suggestions: suggestions.slice(0, 2) });
assert(state.activeSuggestionIndex === -1, "typed suggestions are not preselected");
state = reduceLibrarySearchKeyboard(state, { key: "ArrowDown" });
assert(state.activeSuggestionIndex === 0, "ArrowDown explicitly selects the first suggestion");
state = reduceLibrarySearchKeyboard(state, { key: "ArrowDown" });
assert(state.activeSuggestionIndex === 1, "a second ArrowDown selects the next suggestion");
state = reduceLibrarySearchKeyboard(state, { key: "Enter" });
assert(state.query.libraryTagIds.length === 0, "Enter never applies a suggestion");

const composing = reduceLibrarySearchState(state, { type: "START_COMPOSITION" });
assert(reduceLibrarySearchKeyboard(composing, { key: "Enter" }).phase === "composing", "IME protects Enter");
assert(reduceLibrarySearchKeyboard(composing, { key: "Escape" }).phase === "composing", "IME protects Escape");

console.log("librarySearch tests passed");

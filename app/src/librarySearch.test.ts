import {
  applyLibrarySearchSuggestion,
  buildLibrarySearchSuggestions,
  createLibrarySearchState,
  expandCollectionIds,
  filterLibrarySearchPapers,
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
  mode: "quick", text: "AI strategy", collectionIds: [10, 12], includeDescendants: false, libraryTagIds: [1, 2],
}).map((paper) => paper.id), [1], "OR collections + AND tags + text");

equal(filterLibrarySearchPapers(papers, {
  mode: "content", text: "content", collectionIds: [], includeDescendants: false, libraryTagIds: [],
}).map((paper) => paper.id), [1, 2], "content search");

const suggestionQuery = { mode: "quick" as const, text: "", collectionIds: [], includeDescendants: false, libraryTagIds: [] };
const suggestions = buildLibrarySearchSuggestions({
  collections: [{ id: 10, parentId: null, name: "Root" }],
  tags: [{ id: 1, name: "Active" }, { id: 99, name: "Unused" }],
  papers,
  tagCounts: new Map([[1, 2], [99, 0]]),
}, suggestionQuery);
const unused = suggestions.find((suggestion) => suggestion.id === "libraryTag:99");
assert(unused?.dimmed === true, "zero-count tags are dimmed");
assert(unused?.draggable === true, "zero-count tags remain draggable");
equal(applyLibrarySearchSuggestion(suggestionQuery, unused!), { ...suggestionQuery, libraryTagIds: [99] }, "tag suggestion updates scope");

let state = createLibrarySearchState();
state = reduceLibrarySearchState(state, { type: "FOCUS" });
state = reduceLibrarySearchState(state, { type: "SUGGESTIONS", suggestions: suggestions.slice(0, 2) });
state = reduceLibrarySearchKeyboard(state, { key: "ArrowDown" });
assert(state.activeSuggestionIndex === 1, "ArrowDown moves active suggestion");
state = reduceLibrarySearchKeyboard(state, { key: "Enter" });
assert(state.query.libraryTagIds.length === 1, "Enter applies suggestion");

const composing = reduceLibrarySearchState(state, { type: "START_COMPOSITION" });
assert(reduceLibrarySearchKeyboard(composing, { key: "Enter" }).phase === "composing", "IME protects Enter");
assert(reduceLibrarySearchKeyboard(composing, { key: "Escape" }).phase === "composing", "IME protects Escape");

console.log("librarySearch tests passed");

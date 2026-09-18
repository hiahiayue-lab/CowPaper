import {
  applyLibrarySearchSuggestion,
  buildLibrarySearchSuggestions,
  createLibrarySearchState,
  emptyLibrarySearchQuery,
  libraryBrowseScopeForQuery,
  libraryDataViewForQuery,
  reduceLibrarySearchKeyboard,
  reduceLibrarySearchState,
} from "./librarySearch.ts";

function equal(actual: unknown, expected: unknown, message: string): void {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${message}: ${JSON.stringify(actual)} !== ${JSON.stringify(expected)}`);
  }
}

const browse = { collectionId: 10, tagIds: [3] };
const empty = emptyLibrarySearchQuery();
equal(libraryBrowseScopeForQuery(empty, browse), browse, "empty Search restores Collection and Tag browse filters");
equal(libraryBrowseScopeForQuery({ ...empty, freeTextQuery: "governance" }, browse), { collectionId: null, tagIds: [] }, "typing searches the whole Library");
equal(libraryBrowseScopeForQuery({ ...empty, collectionIds: [20] }, browse), { collectionId: null, tagIds: [] }, "explicit Collection token replaces browse filter");
equal(libraryDataViewForQuery(empty, "recent"), "recent", "empty Search restores browse view");
equal(libraryDataViewForQuery({ ...empty, freeTextQuery: "governance" }, "recent"), "all", "typing also searches beyond the Recent view");
equal(libraryDataViewForQuery({ ...empty, collectionIds: [20] }, "unfiled"), "all", "explicit token searches beyond the Unfiled view");

const suggestions = buildLibrarySearchSuggestions({
  collections: [{ id: 20, parentId: null, name: "Governance Collection" }],
  tags: [],
  papers: [],
}, { ...empty, freeTextQuery: "governance" });
const collection = suggestions.find((item) => item.kind === "collection");
if (!collection) throw new Error("Collection name should appear in suggestions");
equal(collection.label, "Governance Collection", "Collection name suggestion uses its name");
equal(applyLibrarySearchSuggestion({ ...empty, freeTextQuery: "governance" }, collection), {
  ...empty, collectionIds: [20], freeTextQuery: "",
}, "pointer selection inserts Collection token and consumes the typed name");

let state = createLibrarySearchState({ freeTextQuery: "governance" });
state = reduceLibrarySearchState(state, { type: "FOCUS" });
state = reduceLibrarySearchState(state, { type: "SUGGESTIONS", suggestions });
state = reduceLibrarySearchKeyboard(state, { key: "ArrowDown" });
equal(reduceLibrarySearchKeyboard(state, { key: "Enter" }), state, "Enter does not insert a highlighted Collection token");
state = reduceLibrarySearchState(state, { type: "SELECT_SUGGESTION", suggestion: collection });
equal(state.query.collectionIds, [20], "explicit suggestion selection inserts Collection token");
state = reduceLibrarySearchState(state, { type: "CLEAR" });
equal(libraryBrowseScopeForQuery(state.query, browse), browse, "clearing Search returns to prior browse state");

console.log("librarySearch v0.4.1 tests passed");

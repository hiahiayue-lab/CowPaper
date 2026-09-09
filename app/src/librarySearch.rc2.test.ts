import {
  applyLibrarySearchSuggestion,
  buildLibrarySearchSuggestions,
  createLibrarySearchAdapter,
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
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${message}: ${JSON.stringify(actual)} !== ${JSON.stringify(expected)}`);
  }
}

const collections = [
  { id: 10, parentId: null, name: "AI" },
  { id: 11, parentId: 10, name: "AI / Governance" },
  { id: 20, parentId: null, name: "ESG" },
];
const tags = [{ id: 1, name: "Core" }, { id: 2, name: "Review" }, { id: 99, name: "Zero Count" }];
const papers: SearchPaper[] = [
  { id: 1, title: "AI Governance in Organizations", source: "Journal A", abstract: "治理 platform", collectionIds: [10], tagIds: [1, 2] },
  { id: 2, title: "Institutions and Disclosure", source: "Journal B", abstract: "AI 治理 ESG 平台", collectionIds: [10, 20], tagIds: [1, 2] },
  { id: 2, title: "Institutions and Disclosure", source: "Journal B", abstract: "AI 治理 ESG 平台", collectionIds: [10, 20], tagIds: [1, 2] },
  { id: 3, title: "Parent Record", source: "Journal C", collectionIds: [10], tagIds: [1] },
  { id: 3, title: "Parent Record — accepted manuscript", source: "Journal C", collectionIds: [10], tagIds: [1] },
];

const emptyQuery = { queryText: "", collectionIds: [], libraryTagIds: [] };
const indexedPapers = papers.filter((paper, index, all) => all.findIndex((candidate) => candidate.id === paper.id) === index);
const index = { collections, tags, papers: indexedPapers, tagCounts: new Map([[1, 2], [2, 2], [99, 0]]) };
const metadataOnlyPaper: SearchPaper = { ...papers[0], doi: "10.5555/metadata-only", publisher: "Publisher Only", volume: "99", issue: "7", pages: "1-9", url: "https://metadata-only.invalid" };
assert(filterLibrarySearchPapers([metadataOnlyPaper], { ...emptyQuery, queryText: "10.5555/metadata-only" }).length === 0, "excluded metadata is not full-text searchable");

// TEST 1: selecting a scope token locks the scope while free text remains editable.
let state = createLibrarySearchState();
state = reduceLibrarySearchState(state, { type: "FOCUS" });
const ai = buildLibrarySearchSuggestions(index, emptyQuery).find((item) => item.id === "collection:10")!;
const core = buildLibrarySearchSuggestions(index, emptyQuery).find((item) => item.id === "libraryTag:1")!;
state = reduceLibrarySearchState(state, { type: "SELECT_SUGGESTION", suggestion: ai });
state = reduceLibrarySearchState(state, { type: "INPUT", text: "治理" });
state = reduceLibrarySearchState(state, { type: "SELECT_SUGGESTION", suggestion: core });
equal(state.query.collectionIds, [10], "TEST 1 collection token remains locked");
equal(state.query.libraryTagIds, [1], "TEST 1 tag token remains locked");
equal(state.query.queryText, "", "TEST 1 token selection clears temporary input");
state = reduceLibrarySearchState(state, { type: "INPUT", text: "治理" });
equal(state.query.queryText, "治理", "TEST 1 continued input remains editable");

// TEST 2/3/4: OR collections, AND tags, cross-dimension AND, and canonical-id de-dup.
equal(filterLibrarySearchPapers(papers, { ...emptyQuery, collectionIds: [10, 20] }).map((p) => p.id), [1, 2, 3], "TEST 2 collection OR");
equal(filterLibrarySearchPapers(papers, { ...emptyQuery, libraryTagIds: [1, 2] }).map((p) => p.id), [1, 2], "TEST 2 tag AND");
equal(filterLibrarySearchPapers(papers, { ...state.query }).map((p) => p.id), [1, 2], "TEST 2 dimensions compose with AND");
equal(filterLibrarySearchPapers(papers, { ...emptyQuery, collectionIds: [10, 20], queryText: "Institutions" }).map((p) => p.id), [2], "TEST 3 multi-Collection de-dup");
equal(filterLibrarySearchPapers(papers, { ...emptyQuery, queryText: "Parent Record" }).map((p) => p.id), [3], "TEST 4 parent/child de-dup by paper id");
equal(expandCollectionIds(collections, [10]), [10, 11], "TEST 4 parent includes child scope");

// TEST 5: the same scope-token operation is the pure contract used by sidebar auto-tokening.
const sidebarQuery = applyLibrarySearchSuggestion(emptyQuery, ai);
equal(sidebarQuery, { ...emptyQuery, collectionIds: [10] }, "TEST 5 sidebar auto token scope");

// TEST 6: suggestion categories stay Library-local, with one logical item per id.
const suggestionIds = buildLibrarySearchSuggestions(index, emptyQuery).map((item) => item.id);
assert(suggestionIds.every((id) => id.startsWith("collection:") || id.startsWith("libraryTag:") || id.startsWith("paper:") || id.startsWith("action:")), "TEST 6 has an external suggestion kind");
equal(new Set(suggestionIds).size, suggestionIds.length, "TEST 6 duplicate suggestion chips");

// TEST 7: zero-count tags are still visible, dimmed, and draggable/clickable.
const zero = buildLibrarySearchSuggestions(index, emptyQuery).find((item) => item.id === "libraryTag:99")!;
assert(zero.count === 0 && zero.dimmed === true && zero.draggable === true, "TEST 7 zero-count tag affordances");

// TEST 8: adapter reads current runtime data on every search; no stale local snapshot.
let runtimePapers = papers.slice(0, 2);
const adapter = createLibrarySearchAdapter({
  listPapers: async () => runtimePapers,
  listCollections: async () => collections,
  listTags: async () => tags,
});
const runtimeQuery = { ...emptyQuery, queryText: "AI Governance" };
equal((await adapter.search(runtimeQuery)).paperIds, [1], "TEST 8 initial runtime index");
runtimePapers = [{ ...runtimePapers[0], title: "Renamed Runtime Paper" }, runtimePapers[1]];
equal((await adapter.search({ ...emptyQuery, queryText: "Renamed Runtime" })).paperIds, [1], "TEST 8 runtime update");
equal((await adapter.search(runtimeQuery)).paperIds, [], "TEST 8 stale old term removed");

// TEST 9: actual CJK/mixed token contract is exercised by the Rust runtime test;
// this client assertion ensures mixed terms remain ANDed in the fallback.
const mixed = { ...emptyQuery, queryText: "AI 治理 ESG 平台" };
equal(filterLibrarySearchPapers(papers, mixed).map((p) => p.id), [2], "TEST 9 mixed terms are ANDed");

// TEST 10: IME composition owns keyboard handling until compositionend.
let composing = reduceLibrarySearchState(state, { type: "START_COMPOSITION" });
for (const key of ["Enter", "Escape", "ArrowDown"]) {
  assert(reduceLibrarySearchKeyboard(composing, { key }).phase === "composing", `TEST 10 IME protects ${key}`);
}
composing = reduceLibrarySearchState(composing, { type: "END_COMPOSITION", text: "人工智能" });
assert(composing.isComposing === false && composing.query.queryText === "人工智能", "TEST 10 compositionend commits text");

console.log("librarySearch RC2 tests passed (TEST 1-10)");

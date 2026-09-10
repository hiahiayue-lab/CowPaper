import {
  LIBRARY_SEARCH_EXCLUDED_FIELDS,
  LIBRARY_SEARCH_FIELDS,
  buildLibrarySearchFieldTokens,
  buildLibrarySearchSuggestions,
  createLibrarySearchState,
  dedupeLibrarySearchPapers,
  emptyLibrarySearchQuery,
  filterLibrarySearchPapers,
  librarySearchFieldLabel,
  matchLibrarySearchPaper,
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

const paper: SearchPaper = {
  id: 101,
  title: "AI Governance",
  chineseTitle: "人工智能治理",
  authors: "Fiona Chen",
  source: "Journal X",
  year: 2024,
  note: "platform note",
  abstract: "Platforms shape governance.",
  chineseAbstract: "平台治理摘要",
  tags: ["Core"],
  tagIds: [1],
  collectionIds: [10],
};
const duplicate = { ...paper, title: "duplicate joined row" };

// v0.2.2 state/API handoff: the wire query stays four-dimensional and the
// searchable projection remains exactly the nine allowed fields.
equal(Object.keys(emptyLibrarySearchQuery()).sort(), ["collectionIds", "fieldClauses", "freeTextQuery", "libraryTagIds"], "query shape is stable");
equal(LIBRARY_SEARCH_FIELDS.length, 9, "searchable field count");
assert(LIBRARY_SEARCH_EXCLUDED_FIELDS.every((field) => !LIBRARY_SEARCH_FIELDS.includes(field as never)), "display metadata stays excluded");

// Field filters are removable continuation tokens, with the canonical visual
// form used by the UI (for example: [中文标题: AI ×]).
equal(librarySearchFieldLabel("chineseTitle"), "中文标题", "Chinese field label");
equal(buildLibrarySearchFieldTokens({ fieldClauses: [{ field: "chineseTitle", query: "AI" }] })[0].label, "中文标题: AI", "field token label");

// The dropdown has four logical groups at most, no Search Action, and one
// logical item per canonical id.
const suggestions = buildLibrarySearchSuggestions({
  collections: [{ id: 10, parentId: null, name: "AI" }],
  tags: [{ id: 1, name: "Core" }, { id: 99, name: "Unused" }],
  papers: [paper, duplicate],
  tagCounts: new Map([[1, 1], [99, 0]]),
}, { ...emptyLibrarySearchQuery(), freeTextQuery: "AI" });
const kinds = new Set(suggestions.map((suggestion) => suggestion.kind));
assert([...kinds].every((kind) => ["collection", "libraryTag", "paper", "field"].includes(kind)), "suggestion groups stay bounded");
assert(!suggestions.some((suggestion) => (suggestion.kind as string) === "searchAction"), "Search Action is absent");
equal(suggestions.filter((suggestion) => suggestion.kind === "paper").map((suggestion) => suggestion.id), ["paper:101"], "paper suggestions are canonical and deduped");
const zero = buildLibrarySearchSuggestions({ collections: [], tags: [{ id: 99, name: "Unused" }], papers: [], tagCounts: new Map([[99, 0]]) }, { ...emptyLibrarySearchQuery(), freeTextQuery: "Unused" })[0];
assert(zero?.dimmed && zero.draggable, "zero-count tag remains dimmed and selectable");

// Collection OR, Tag AND, free text AND, field clauses AND, and canonical
// papers.id de-duplication remain adapter-independent.
equal(filterLibrarySearchPapers([paper, duplicate], { ...emptyLibrarySearchQuery(), freeTextQuery: "人工智能 平台", collectionIds: [10], libraryTagIds: [1] }).map(({ id }) => id), [101], "dimensions compose with AND");
equal(filterLibrarySearchPapers([paper, duplicate], { ...emptyLibrarySearchQuery(), fieldClauses: [{ field: "chineseTitle", query: "人工智能" }, { field: "note", query: "platform" }] }).map(({ id }) => id), [101], "field clauses compose with AND");
equal(dedupeLibrarySearchPapers([paper, duplicate]).map(({ id }) => id), [101], "canonical papers.id dedup");
const hit = matchLibrarySearchPaper(paper, { ...emptyLibrarySearchQuery(), freeTextQuery: "AI 治理" });
assert(hit.paperId === 101 && hit.matched_fields.includes("title") && hit.matched_fields.includes("chineseTitle"), "matched_fields survives");
assert(hit.snippets.length > 0 && hit.snippets.every((snippet) => snippet.text.length <= 120), "short snippets survive");

// Outside click preserves query/results-owned state and only dismisses the
// dropdown. Enter is a complete no-op, including after ArrowDown.
let state = createLibrarySearchState({ freeTextQuery: "治理", collectionIds: [10] });
state = reduceLibrarySearchState(state, { type: "FOCUS" });
state = reduceLibrarySearchState(state, { type: "SUGGESTIONS", suggestions });
const beforeEnter = state;
assert(reduceLibrarySearchKeyboard(state, { key: "Enter" }) === beforeEnter, "ENTER NONE");
const afterOutside = reduceLibrarySearchState(state, { type: "OUTSIDE_CLICK" });
equal(afterOutside.query, state.query, "outside click preserves query/tokens");
assert(afterOutside.phase === "closed" && afterOutside.suggestions.length === 0, "outside click closes only dropdown");

// Explicit pointer selection remains the only path that commits a scope or
// field token; keyboard navigation only changes the visual active index.
const collection = suggestions.find((suggestion) => suggestion.kind === "collection")!;
const selected = reduceLibrarySearchState(state, { type: "SELECT_SUGGESTION", suggestion: collection });
assert(selected.query.collectionIds.includes(10), "explicit suggestion selection commits scope");
const navigated = reduceLibrarySearchKeyboard(state, { key: "ArrowDown" });
assert(reduceLibrarySearchKeyboard(navigated, { key: "Enter" }) === navigated, "keyboard cannot commit suggestion");

// IME owns Enter/Escape/navigation until compositionend and then leaves the
// committed CJK text available for live search.
let composing = reduceLibrarySearchState(state, { type: "START_COMPOSITION" });
for (const key of ["Enter", "Escape", "ArrowDown", "ArrowUp"]) {
  assert(reduceLibrarySearchKeyboard(composing, { key }).phase === "composing", `IME protects ${key}`);
}
composing = reduceLibrarySearchState(composing, { type: "END_COMPOSITION", text: "人工智能" });
assert(!composing.isComposing && composing.query.freeTextQuery === "人工智能", "IME compositionend commits text");

console.log("librarySearch v0.2.2 UX contract tests passed");

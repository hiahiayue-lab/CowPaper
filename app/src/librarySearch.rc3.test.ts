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
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${message}: ${JSON.stringify(actual)} !== ${JSON.stringify(expected)}`);
  }
}

const collections = [
  { id: 10, parentId: null, name: "AI" },
  { id: 11, parentId: 10, name: "AI / Governance" },
  { id: 20, parentId: null, name: "ESG" },
];

const tags = [
  { id: 1, name: "Core" },
  { id: 2, name: "Review" },
  { id: 99, name: "Zero Count" },
];

const papers: SearchPaper[] = [
  {
    id: 101,
    title: "AI Governance in Organizations",
    authors: ["Alice Smith"],
    source: "Journal of AI Studies",
    year: 2026,
    abstract: "Governance mechanisms for responsible AI adoption.",
    chineseAbstract: "人工智能治理机制与组织责任。",
    collectionIds: [10],
    tagIds: [1, 2],
    tags: ["Core", "Review"],
  },
  {
    id: 102,
    title: "人工智能治理研究",
    authors: ["李明"],
    source: "管理科学学报",
    year: 2025,
    abstract: "研究人工智能治理与组织责任。",
    chineseAbstract: "人工智能治理的中文摘要。",
    collectionIds: [10, 11],
    tagIds: [1],
    tags: ["Core"],
  },
  {
    id: 103,
    title: "ESG Platform Evidence",
    chineseTitle: "ESG 平台证据",
    authors: ["Carlos Green"],
    source: "ESG Review",
    year: 2024,
    note: "ESG reading list",
    abstract: "Evidence from an ESG platform and reporting outcomes.",
    chineseAbstract: "ESG 平台与披露结果。",
    collectionIds: [20],
    tagIds: [2],
    tags: ["Review"],
  },
  {
    id: 104,
    title: "环境、社会与治理披露",
    chineseTitle: "环境、社会与治理披露",
    authors: ["王芳"],
    source: "会计研究",
    year: 2023,
    abstract: "环境、社会与治理披露的实证研究。",
    chineseAbstract: "ESG 平台披露与资本市场反应。",
    collectionIds: [20],
    tagIds: [1],
    tags: ["Core"],
  },
  {
    id: 105,
    title: "Institutions and Disclosure",
    authors: ["Evan Jones"],
    source: "Governance Quarterly",
    year: 2022,
    abstract: "Evidence on governance and disclosure quality.",
    chineseAbstract: "AI 治理与 ESG 平台。",
    collectionIds: [10, 20],
    tagIds: [1, 2],
    tags: ["Core", "Review"],
  },
  {
    id: 106,
    title: "Searchable Metadata Methods",
    authors: ["Fiona Chen"],
    source: "Information Systems Research",
    year: 2021,
    note: "Journal/source clause anchor",
    abstract: "Metadata quality and reproducible search.",
    collectionIds: [10],
    tagIds: [2],
    tags: ["Review"],
  },
  {
    id: 107,
    title: "Parent Record",
    source: "Methods Journal",
    abstract: "Parent record used for canonical de-duplication.",
    collectionIds: [10],
    tagIds: [1],
  },
  // The attachment/version shares the canonical paper id and must not create
  // a second visible result.
  {
    id: 107,
    title: "Parent Record — accepted manuscript",
    source: "Methods Journal",
    abstract: "Child attachment/version of the parent record.",
    collectionIds: [10],
    tagIds: [1],
  },
  {
    id: 112,
    title: "Responsible AI and ESG Platform",
    chineseTitle: "人工智能治理与 ESG 平台",
    authors: ["Mei Lin"],
    source: "Cross-Domain Governance",
    year: 2026,
    note: "Mixed-language search anchor",
    abstract: "AI governance and ESG platform evidence.",
    chineseAbstract: "人工智能治理与 ESG 平台研究。",
    collectionIds: [10, 20],
    tagIds: [1, 2],
    tags: ["Core", "Review"],
  },
];

const emptyQuery = { queryText: "", collectionIds: [], libraryTagIds: [] };

// Q-01–Q-04: real mixed-language terms remain ANDed and search the effective
// title/abstract projection used by the existing client fallback.
equal(filterLibrarySearchPapers(papers, { ...emptyQuery, queryText: "人工智能" }).map((paper) => paper.id), [101, 102, 112], "Q-01 Chinese term");
equal(filterLibrarySearchPapers(papers, { ...emptyQuery, queryText: "AI 治理" }).map((paper) => paper.id), [101, 105, 112], "Q-02 AI governance");
equal(filterLibrarySearchPapers(papers, { ...emptyQuery, queryText: "ESG 平台" }).map((paper) => paper.id), [103, 104, 105, 112], "Q-03 ESG platform");
equal(filterLibrarySearchPapers(papers, { ...emptyQuery, queryText: "人工智能 AI 治理 ESG 平台" }).map((paper) => paper.id), [112], "Q-04 mixed-language AND");

// Q-05–Q-10: Collection is OR (including descendants), Tags are AND, the
// dimensions compose with AND, and canonical paper ids de-duplicate rows.
equal(expandCollectionIds(collections, [10]), [10, 11], "parent Collection includes descendants");
equal(filterLibrarySearchPapers(papers, { ...emptyQuery, collectionIds: [10, 20] }).map((paper) => paper.id), [101, 102, 103, 104, 105, 106, 107, 112], "Q-05 Collection OR and dedup");
equal(filterLibrarySearchPapers(papers, { ...emptyQuery, libraryTagIds: [1, 2] }).map((paper) => paper.id), [101, 105, 112], "Q-06 Tag AND");
equal(filterLibrarySearchPapers(papers, { ...emptyQuery, collectionIds: [10], libraryTagIds: [1, 2], queryText: "governance" }).map((paper) => paper.id), [101, 105, 112], "Q-07 scope plus free text");
equal(filterLibrarySearchPapers(papers, { ...emptyQuery, queryText: "Parent Record" }).map((paper) => paper.id), [107], "Q-09 parent-child dedup");

// Scope token selection consumes only the temporary suggestion text. Existing
// tokens remain, and the user can continue with free text in the same box.
const index = { collections, tags, papers, tagCounts: new Map([[1, 3], [2, 3], [99, 0]]) };
const collectionSuggestion = buildLibrarySearchSuggestions(index, { ...emptyQuery, queryText: "AI" }).find((item) => item.id === "collection:10");
const tagSuggestion = buildLibrarySearchSuggestions(index, emptyQuery).find((item) => item.id === "libraryTag:1");
assert(collectionSuggestion && tagSuggestion, "scope suggestions exist for token contract");
let state = createLibrarySearchState({ queryText: "AI", libraryTagIds: [1] });
state = reduceLibrarySearchState(state, { type: "FOCUS" });
state = reduceLibrarySearchState(state, { type: "SELECT_SUGGESTION", suggestion: collectionSuggestion });
equal(state.query, { queryText: "", collectionIds: [10], libraryTagIds: [1] }, "token selection preserves prior token and clears temporary text");
state = reduceLibrarySearchState(state, { type: "INPUT", text: "治理" });
equal(state.query, { queryText: "治理", collectionIds: [10], libraryTagIds: [1] }, "free text remains editable after token selection");
state = reduceLibrarySearchState(state, { type: "SELECT_SUGGESTION", suggestion: tagSuggestion });
equal(state.query.libraryTagIds, [1], "duplicate logical Tag token is not added");

// Closing and reopening the same Search Box does not clear query or scope.
const preservedQuery = { queryText: "governance", collectionIds: [10], libraryTagIds: [1] };
state = createLibrarySearchState(preservedQuery);
state = reduceLibrarySearchState(state, { type: "FOCUS" });
state = reduceLibrarySearchState(state, { type: "SUGGESTIONS", suggestions: [tagSuggestion] });
state = reduceLibrarySearchState(state, { type: "ESCAPE" });
assert(state.phase === "closed", "Escape closes Search suggestions");
state = reduceLibrarySearchState(state, { type: "FOCUS" });
assert(state.phase === "open", "focus reopens Search suggestions");
equal(state.query, preservedQuery, "reopen preserves query and scope tokens");

// Keyboard navigation remains deterministic, while a real IME owns every key
// event until compositionend. Runtime outside-click and result preservation
// are intentionally covered by the RC3 manual matrix because the reducer has
// no DOM event surface.
state = reduceLibrarySearchState(state, { type: "SUGGESTIONS", suggestions: [tagSuggestion, collectionSuggestion] });
state = reduceLibrarySearchKeyboard(state, { key: "ArrowDown" });
assert(state.activeSuggestionIndex === 1, "ArrowDown moves active suggestion");
const composing = reduceLibrarySearchState(state, { type: "START_COMPOSITION" });
for (const key of ["Enter", "Escape", "ArrowDown", "ArrowUp"]) {
  const duringComposition = reduceLibrarySearchKeyboard(composing, { key });
  assert(duringComposition === composing, `real IME protects ${key}`);
}
const committed = reduceLibrarySearchState(composing, { type: "END_COMPOSITION", text: "人工智能" });
assert(!committed.isComposing && committed.query.queryText === "人工智能", "compositionend commits CJK text");

// Zero-count Tags remain in the suggestion model and retain their drag target.
const zeroCount = buildLibrarySearchSuggestions(index, emptyQuery).find((item) => item.id === "libraryTag:99");
assert(zeroCount?.count === 0 && zeroCount.dimmed === true && zeroCount.draggable === true, "zero-count Tag affordances");

// The fixture/matrix test owns the candidate-only gates that are not in this
// framework-free reducer yet: Search Action removal, outside-click DOM close,
// field-clause tokens, and matched_fields/snippets rendering.
console.log("librarySearch RC3 tests passed (semantic/state preparation gates)");

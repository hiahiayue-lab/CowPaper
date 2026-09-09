import { readFileSync } from "node:fs";

const fixture = JSON.parse(readFileSync(new URL("./fixtures-rc3.json", import.meta.url), "utf8"));

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

function equal(actual, expected, message) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${message}: ${JSON.stringify(actual)} !== ${JSON.stringify(expected)}`);
  }
}

assert(fixture.baseline === "5b8355d70a72a49974586e940403176cf794bf18", "fixture baseline drifted");
assert(fixture.phase === "phase-1-preparation", "fixture is not marked as Phase 1 preparation");
assert(fixture.isolation.syntheticOnly === true, "fixture must remain synthetic-only");
assert(fixture.isolation.allowProductionSeedMutation === false, "fixture must not seed production data");
assert(fixture.isolation.allowUserDataMutation === false, "fixture must not mutate user data");
assert(fixture.isolation.migrationOrSchemaWork === false, "fixture must not authorize migrations/schema work");
assert(fixture.isolation.releaseOrTagWork === false, "fixture must not authorize release/tag work");

const recordsById = new Map(fixture.records.map((record) => [record.recordId, record]));
assert(recordsById.size === fixture.records.length, "fixture record ids must be unique");
assert(fixture.records.length === 13, "RC3 fixture must contain A-M");
const knownPaperIds = new Set(fixture.records.map((record) => record.paperId));
const duplicatePaperIds = [...new Set(fixture.records.map((record) => record.paperId).filter((id, index, ids) => ids.indexOf(id) !== index))];
equal(duplicatePaperIds, [107], "only the intentional parent/child id is duplicated");
assert(recordsById.get("H")?.parentRecordId === "G", "parent/child relationship is explicit");

for (const scenario of fixture.queryScenarios) {
  assert(Array.isArray(scenario.expectedPaperIds), `${scenario.id} is missing expected paper ids`);
  assert(new Set(scenario.expectedPaperIds).size === scenario.expectedPaperIds.length, `${scenario.id} contains duplicate expected ids`);
  for (const paperId of scenario.expectedPaperIds) assert(knownPaperIds.has(paperId), `${scenario.id} references unknown paper ${paperId}`);
}
equal(
  fixture.queryScenarios.find((scenario) => scenario.id === "Q-04")?.expectedPaperIds,
  [112],
  "mixed-language query is a single canonical result",
);
equal(
  fixture.queryScenarios.find((scenario) => scenario.id === "Q-10")?.visibleRows,
  1,
  "multi-Collection result is one visible row",
);

const displayOnly = new Set(fixture.displayOnlyFields);
const searchableFields = new Set(["title", "chinese_title", "authors", "year", "source", "tags", "note", "abstract", "chinese_abstract"]);
for (const scenario of fixture.fieldClauseScenarios) {
  assert(typeof scenario.input === "string" && scenario.input.includes(":"), `${scenario.id} is not a field-clause case`);
  if (scenario.matched_fields) {
    assert(new Set(scenario.matched_fields).size === scenario.matched_fields.length, `${scenario.id} has duplicate matched fields`);
    for (const field of scenario.matched_fields) {
      assert(searchableFields.has(field), `${scenario.id} uses an unknown matched field ${field}`);
      assert(!displayOnly.has(field), `${scenario.id} exposes a display-only field as searchable`);
    }
  }
  if (scenario.expectedPaperIds) {
    for (const paperId of scenario.expectedPaperIds) assert(knownPaperIds.has(paperId), `${scenario.id} references unknown paper ${paperId}`);
  }
}
assert(fixture.fieldClauseScenarios.some((scenario) => scenario.tokenCount === 2 && scenario.clausesComposeWith === "AND"), "field clauses need an explicit AND case");
assert(fixture.fieldClauseScenarios.some((scenario) => scenario.token?.kind === "collection"), "Collection clause token contract is missing");
assert(fixture.fieldClauseScenarios.some((scenario) => scenario.token?.kind === "libraryTag"), "Tag clause token contract is missing");

for (const result of fixture.resultContractExamples) {
  assert(knownPaperIds.has(result.paperId), `${result.id} references unknown paper`);
  assert(Array.isArray(result.matched_fields) && result.matched_fields.length > 0, `${result.id} needs matched_fields`);
  assert(result.snippets && typeof result.snippets === "object", `${result.id} needs snippets`);
  equal(Object.keys(result.snippets).sort(), [...result.matched_fields].sort(), `${result.id} snippets must cover matched_fields exactly`);
  assert(new Set(result.matched_fields).size === result.matched_fields.length, `${result.id} has duplicate matched_fields`);
  for (const field of result.matched_fields) {
    assert(searchableFields.has(field), `${result.id} uses an unknown matched field`);
    assert(!displayOnly.has(field), `${result.id} uses a display-only matched field`);
    assert(typeof result.snippets[field] === "string" && result.snippets[field].length > 0, `${result.id} has an empty snippet`);
  }
}

const actionRemoval = fixture.interactionFixtures.searchActionRemoval;
assert(actionRemoval.forbiddenSuggestionKinds.includes("searchAction"), "Search Action removal gate is missing");
assert(actionRemoval.allowedSuggestionKinds.includes("collection"), "Collection suggestion must remain allowed");
assert(actionRemoval.allowedSuggestionKinds.includes("libraryTag"), "Tag suggestion must remain allowed");
assert(actionRemoval.allowedSuggestionKinds.includes("paper"), "Paper suggestion must remain allowed");
assert(actionRemoval.allowedSuggestionKinds.every((kind) => !actionRemoval.forbiddenSuggestionKinds.includes(kind)), "suggestion allow/deny lists overlap");

const outsideClick = fixture.interactionFixtures.outsideClickPreserves;
assert(outsideClick.after.suggestionsVisible === false, "outside click must close the suggestion surface");
assert(outsideClick.after.queryAndTokensUnchanged === true, "outside click must preserve query and tokens");
assert(outsideClick.after.resultPaperIdsUnchanged === true, "outside click must preserve applied results");
assert(outsideClick.after.selectedPaperIdUnchanged === true, "outside click must preserve selected paper/Inspector");

const reopen = fixture.interactionFixtures.reopenPreserves;
assert(reopen.after.queryAndTokensRestored === true, "reopen must restore query and tokens");
assert(reopen.after.appliedResultsRestored === true, "reopen must restore applied results");
assert(reopen.after.selectedPaperAndInspectorRestored === true, "reopen must restore selection/Inspector");
assert(reopen.after.emptyQueryDoesNotDumpSuggestions === true, "empty-query reopen must stay quiet");

const ime = fixture.interactionFixtures.realIme;
assert(ime.compositionKeys.includes("Enter") && ime.compositionKeys.includes("Escape"), "IME fixture must cover commit and close keys");
assert(ime.duringComposition.execute === false && ime.duringComposition.close === false, "IME must block premature execution/close");
assert(ime.duringComposition.moveActiveSuggestion === false, "IME must block premature suggestion navigation");
assert(ime.afterCompositionEnd.committedTextSearches === true && ime.afterCompositionEnd.executeOnce === true, "IME commit contract is incomplete");

const viewportWidths = fixture.visualFixtures.viewports.map((viewport) => viewport.width);
equal(viewportWidths, [1440, 1512, 1536, 1000, 740, 600], "visual viewport matrix drifted");
for (const viewport of fixture.visualFixtures.viewports) {
  assert(viewport.horizontalOverflow === false, `${viewport.name} permits horizontal overflow`);
}
for (const surface of ["Discovery", "Library", "Settings", "toolbar", "sidebar", "table", "Inspector", "search-popover", "status-popover", "column-menu", "relation-popover", "buttons", "tags", "empty-states"]) {
  assert(fixture.visualFixtures.surfaces.includes(surface), `visual surface missing: ${surface}`);
}
assert(fixture.visualFixtures.motion.default.includes("no Search"), "default Search motion policy missing");
assert(fixture.visualFixtures.motion.reduced.includes("remove movement"), "reduced-motion policy missing");

assert(fixture.futureAnnotationContract.implementedInRc3 === false, "Annotation must remain unimplemented in RC3");
assert(fixture.futureAnnotationContract.forbiddenInPhase1.includes("annotation migration"), "Annotation migration boundary missing");
assert(fixture.futureAnnotationContract.forbiddenInPhase1.includes("placeholder annotation UI"), "Annotation placeholder boundary missing");

console.log("RC3 fixture contract passed (interaction, result, visual, IME, and Annotation boundaries)");

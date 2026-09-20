import {
  emptyLibrarySidebarBrowseSelection,
  isLibrarySidebarCollectionActive,
  isLibrarySidebarTagActive,
  selectLibrarySidebarCollection,
  selectLibrarySidebarTag,
} from "./librarySidebar.ts";

function equal(actual: unknown, expected: unknown, message: string): void {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${message}: ${JSON.stringify(actual)} !== ${JSON.stringify(expected)}`);
  }
}

const none = emptyLibrarySidebarBrowseSelection();
const collectionA = selectLibrarySidebarCollection(10);
const collectionB = selectLibrarySidebarCollection(11);
const tagX = selectLibrarySidebarTag(collectionA, 20);
const tagY = selectLibrarySidebarTag(tagX, 21);

equal(collectionA, { kind: "collection", id: 10 }, "Collection selection is canonical");
equal(collectionB, { kind: "collection", id: 11 }, "Collection selection replaces prior Collection");
equal(tagX, { kind: "tag", id: 20 }, "Tag selection clears Collection selection");
equal(tagY, { kind: "tag", id: 21 }, "Tag selection replaces prior Tag");
equal(selectLibrarySidebarTag(tagX, 20), none, "Repeated Tag selection preserves existing toggle semantics");

if (!isLibrarySidebarCollectionActive(collectionA, 10) || isLibrarySidebarCollectionActive(collectionA, 11)) {
  throw new Error("Only the selected Collection may be active");
}
if (!isLibrarySidebarTagActive(tagX, 20) || isLibrarySidebarTagActive(tagX, 21)) {
  throw new Error("Only the selected Tag may be active");
}
if (isLibrarySidebarCollectionActive(tagX, 10) || isLibrarySidebarTagActive(collectionA, 20)) {
  throw new Error("Collection and Tag active visuals must be mutually exclusive");
}

console.log("librarySidebar tests passed");


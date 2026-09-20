import { reorderLibraryColumnOrder, type LibraryColumn } from "./libraryColumns.ts";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function equal(actual: readonly LibraryColumn[], expected: readonly LibraryColumn[], message: string): void {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${message}: ${JSON.stringify(actual)} !== ${JSON.stringify(expected)}`);
  }
}

const initial: LibraryColumn[] = ["title", "note", "source", "year", "authors"];
equal(reorderLibraryColumnOrder(initial, "source", "title"), ["source", "title", "note", "year", "authors"], "dragging a column before the first column");
equal(reorderLibraryColumnOrder(initial, "note", "authors"), ["title", "source", "year", "authors", "note"], "dragging a column to the end");
equal(reorderLibraryColumnOrder(initial, "source", "source"), initial, "dropping on the same column is a no-op");
assert(JSON.stringify(initial) === JSON.stringify(["title", "note", "source", "year", "authors"]), "reordering does not mutate the stored order");

console.log("libraryColumns tests passed");

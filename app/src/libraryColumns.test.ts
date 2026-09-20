import { canStartLibraryColumnReorder, LIBRARY_COLUMNS, reorderLibraryColumnOrder, targetLibraryColumnIndex, type LibraryColumn } from "./libraryColumns.ts";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function equal(actual: readonly LibraryColumn[], expected: readonly LibraryColumn[], message: string): void {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${message}: ${JSON.stringify(actual)} !== ${JSON.stringify(expected)}`);
  }
}

const initial: LibraryColumn[] = ["title", "note", "source", "year", "authors"];
equal([...LIBRARY_COLUMNS], initial, "restore default keeps the default order");
assert(!canStartLibraryColumnReorder(false, "note"), "checkbox click does not start a drag");
assert(canStartLibraryColumnReorder(true, "note"), "handle drag starts reorder");
assert(!canStartLibraryColumnReorder(true, "unknown"), "unknown column cannot start reorder");
const rowMidpoints = [10, 40, 70, 100, 130];
const upwardTarget = targetLibraryColumnIndex(rowMidpoints, 2, 39);
assert(upwardTarget === 1, "upward drag crosses the adjacent row midpoint once");
equal(reorderLibraryColumnOrder(initial, initial[2], initial[upwardTarget]), ["title", "source", "note", "year", "authors"], "dragging a column upward");

const downwardTarget = targetLibraryColumnIndex(rowMidpoints, 1, 71);
assert(downwardTarget === 2, "downward drag crosses the adjacent row midpoint once");
equal(reorderLibraryColumnOrder(initial, initial[1], initial[downwardTarget]), ["title", "source", "note", "year", "authors"], "dragging a column downward");

assert(targetLibraryColumnIndex(rowMidpoints, 2, 69) === 2, "not crossing a midpoint does not reorder");
equal(reorderLibraryColumnOrder(initial, "source", "source"), initial, "dropping on the same column is a no-op");
assert(JSON.stringify(initial) === JSON.stringify(["title", "note", "source", "year", "authors"]), "reordering does not mutate the stored order");

console.log("libraryColumns tests passed");

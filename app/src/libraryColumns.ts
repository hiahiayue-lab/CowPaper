export const LIBRARY_COLUMNS = ["title", "note", "source", "year", "authors"] as const;
export type LibraryColumn = typeof LIBRARY_COLUMNS[number];

export function canStartLibraryColumnReorder(hasHandle: boolean, column: string | undefined): boolean {
  return hasHandle && column != null && LIBRARY_COLUMNS.includes(column as LibraryColumn);
}

export function reorderLibraryColumnOrder(
  order: readonly LibraryColumn[],
  source: LibraryColumn,
  destination: LibraryColumn,
): LibraryColumn[] {
  const next = [...order];
  const from = next.indexOf(source);
  const to = next.indexOf(destination);
  if (from < 0 || to < 0 || from === to) return next;
  next.splice(from, 1);
  next.splice(to, 0, source);
  return next;
}

export function targetLibraryColumnIndex(
  rowMidpoints: readonly number[],
  sourceIndex: number,
  pointerY: number,
): number {
  let targetIndex = 0;
  for (let index = 0; index < rowMidpoints.length; index += 1) {
    if (index !== sourceIndex && pointerY >= rowMidpoints[index]) targetIndex += 1;
  }
  return Math.max(0, Math.min(rowMidpoints.length - 1, targetIndex));
}

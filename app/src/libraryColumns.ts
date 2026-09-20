export const LIBRARY_COLUMNS = ["title", "note", "source", "year", "authors"] as const;
export type LibraryColumn = typeof LIBRARY_COLUMNS[number];

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

export type LibrarySidebarBrowseSelection =
  | { kind: "collection"; id: number }
  | { kind: "tag"; id: number }
  | { kind: "none" };

export const emptyLibrarySidebarBrowseSelection = (): LibrarySidebarBrowseSelection => ({ kind: "none" });

export function selectLibrarySidebarCollection(id: number): LibrarySidebarBrowseSelection {
  return { kind: "collection", id };
}

export function selectLibrarySidebarTag(
  current: LibrarySidebarBrowseSelection,
  id: number,
): LibrarySidebarBrowseSelection {
  return current.kind === "tag" && current.id === id ? emptyLibrarySidebarBrowseSelection() : { kind: "tag", id };
}

export function isLibrarySidebarCollectionActive(
  selection: LibrarySidebarBrowseSelection,
  id: number,
): boolean {
  return selection.kind === "collection" && selection.id === id;
}

export function isLibrarySidebarTagActive(
  selection: LibrarySidebarBrowseSelection,
  id: number,
): boolean {
  return selection.kind === "tag" && selection.id === id;
}


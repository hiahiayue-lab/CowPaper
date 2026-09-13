export interface LibrarySelectionState {
  selectedIds: number[];
  anchorId: number | null;
}

export interface LibrarySelectionModifiers {
  metaKey?: boolean;
  ctrlKey?: boolean;
  shiftKey?: boolean;
}

function orderedUniqueVisible(ids: readonly number[], visibleIds: readonly number[]): number[] {
  const visible = new Set(visibleIds);
  const selected = new Set<number>();
  for (const id of ids) {
    if (visible.has(id)) selected.add(id);
  }
  return visibleIds.filter((id) => selected.has(id));
}

/**
 * Pure desktop-style selection reducer. The caller supplies the current
 * visible order, so range selection never reaches across a hidden search or
 * Collection scope.
 */
export function reduceLibrarySelection(
  state: LibrarySelectionState,
  paperId: number,
  visibleIds: readonly number[],
  modifiers: LibrarySelectionModifiers = {},
): LibrarySelectionState {
  if (!visibleIds.includes(paperId)) return { selectedIds: [], anchorId: null };
  const current = orderedUniqueVisible(state.selectedIds, visibleIds);
  const additive = Boolean(modifiers.metaKey || modifiers.ctrlKey);
  const anchorId = state.anchorId != null && visibleIds.includes(state.anchorId) ? state.anchorId : paperId;

  if (modifiers.shiftKey) {
    const anchorIndex = visibleIds.indexOf(anchorId);
    const targetIndex = visibleIds.indexOf(paperId);
    const start = Math.min(anchorIndex, targetIndex);
    const end = Math.max(anchorIndex, targetIndex);
    const range = visibleIds.slice(start, end + 1);
    return {
      selectedIds: orderedUniqueVisible(additive ? [...current, ...range] : range, visibleIds),
      anchorId,
    };
  }

  if (additive) {
    const next = current.includes(paperId)
      ? current.filter((id) => id !== paperId)
      : [...current, paperId];
    return { selectedIds: orderedUniqueVisible(next, visibleIds), anchorId: paperId };
  }

  return { selectedIds: [paperId], anchorId: paperId };
}

export function clearLibrarySelection(): LibrarySelectionState {
  return { selectedIds: [], anchorId: null };
}

/** Resolve the payload for a Paper-originated drag. Selection membership is
 * deliberately separate from the Collection/Tag navigation drag payload. */
export function resolveLibraryPaperDragIds(selectedIds: readonly number[], sourceId: number): number[] {
  const selected = [...new Set(selectedIds.filter((id) => id > 0))];
  return selected.includes(sourceId) && selected.length ? selected : [sourceId];
}

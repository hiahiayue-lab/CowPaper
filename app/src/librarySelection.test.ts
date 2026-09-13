import { clearLibrarySelection, reduceLibrarySelection, resolveLibraryPaperDragIds } from "./librarySelection.ts";

const visible = [1, 2, 3, 4, 5];
let state = clearLibrarySelection();

state = reduceLibrarySelection(state, 2, visible);
if (state.selectedIds.join(",") !== "2" || state.anchorId !== 2) throw new Error("plain selection failed");

state = reduceLibrarySelection(state, 4, visible, { metaKey: true });
if (state.selectedIds.join(",") !== "2,4") throw new Error("Command additive selection failed");

state = reduceLibrarySelection(state, 2, visible, { ctrlKey: true });
if (state.selectedIds.join(",") !== "4") throw new Error("Control toggle selection failed");

state = reduceLibrarySelection({ selectedIds: [2], anchorId: 2 }, 5, visible, { shiftKey: true });
if (state.selectedIds.join(",") !== "2,3,4,5") throw new Error("Shift range selection failed");

state = reduceLibrarySelection({ selectedIds: [1, 5], anchorId: 1 }, 3, visible, { metaKey: true, shiftKey: true });
if (state.selectedIds.join(",") !== "1,2,3,5") throw new Error("additive shift selection failed");

state = reduceLibrarySelection({ selectedIds: [1, 99], anchorId: 99 }, 3, [1, 2, 3], { shiftKey: true });
if (state.selectedIds.join(",") !== "3" || state.anchorId !== 3) throw new Error("hidden scope selection failed");

if (resolveLibraryPaperDragIds([1, 2, 3], 2).join(",") !== "1,2,3") throw new Error("selected Paper drag payload failed");
if (resolveLibraryPaperDragIds([1, 2], 3).join(",") !== "3") throw new Error("unselected Paper drag payload failed");
if (resolveLibraryPaperDragIds([2, 2, 3], 2).join(",") !== "2,3") throw new Error("Paper drag payload normalization failed");

console.log("library selection tests passed");

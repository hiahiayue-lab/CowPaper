import {
  actionControlState,
  resolveManualTitleTranslationSource,
  resolveTitleEditorKeyDecision,
  shouldAutoDismissSuccess,
  SUCCESS_FEEDBACK_MS,
  titleTranslationAriaLabel,
  titleTranslationControlState,
  type TitleEditorKeyEvent,
  type InspectorActionFeedback,
} from "./titleTranslation.ts";

function assert(condition: unknown, message = "assertion failed"): asserts condition {
  if (!condition) throw new Error(message);
}

/** Plain helper so a literal `===` comparison cannot narrow the checked value. */
function assertNot(actual: string, unexpected: string, message: string): void {
  if (actual === unexpected) throw new Error(message);
}

function key(overrides: Partial<TitleEditorKeyEvent>): TitleEditorKeyEvent {
  return { key: "Enter", shiftKey: false, isComposing: false, keyCode: 13, ...overrides };
}

// ---------- TEST D: the visible effective title wins over the canonical title.
// canonical = "Old Canonical Title", UI shows "The Knowledge-Engineering Paradox".
{
  const resolved = resolveManualTitleTranslationSource({
    draft: null,
    effectiveTitle: "The Knowledge-Engineering Paradox",
    canonicalTitle: "Old Canonical Title",
  });
  const source: string = resolved.source;
  assert(source === "The Knowledge-Engineering Paradox", "TEST D: effective title must be the translation source");
  assertNot(source, "Old Canonical Title", "TEST D: the old canonical title must never be used");
  assert(resolved.hasDraft === false, "TEST D: a persisted effective title is not a draft");
  assert(resolved.error === null);
}

// ---------- TEST E: an unsaved draft is the source, and is persisted first.
// DB has "Saved Title", the editor shows "A Completely New English Title".
{
  const resolved = resolveManualTitleTranslationSource({
    draft: "A Completely New English Title",
    effectiveTitle: "Saved Title",
    canonicalTitle: "Old Canonical Title",
  });
  assert(resolved.source === "A Completely New English Title", "TEST E: the unsaved draft must be the source");
  assert(resolved.hasDraft === true, "TEST E: the draft must be flagged so it is saved with the same string");
}

// ---------- Priority chain: draft -> effective -> canonical.
{
  const canonicalOnly = resolveManualTitleTranslationSource({ draft: null, effectiveTitle: null, canonicalTitle: "Canonical Only" });
  assert(canonicalOnly.source === "Canonical Only", "canonical title is the last resort");
  assert(canonicalOnly.hasDraft === false);
  const blankEffective = resolveManualTitleTranslationSource({ draft: null, effectiveTitle: "   ", canonicalTitle: "Canonical Only" });
  assert(blankEffective.source === "Canonical Only", "a blank effective title falls through to canonical");
  const noTitle = resolveManualTitleTranslationSource({ draft: null, effectiveTitle: null, canonicalTitle: null });
  assert(noTitle.error !== null && noTitle.source === "", "no title must fail loudly, never silently");
  const clearedDraft = resolveManualTitleTranslationSource({ draft: "  ", effectiveTitle: "Saved Title", canonicalTitle: "Canonical" });
  assert(clearedDraft.error !== null, "an explicitly cleared editor must not reuse another title");
  assert(clearedDraft.source === "");
}

// ---------- TEST F: Enter is inert in the English Title editor.
{
  const decision = resolveTitleEditorKeyDecision(key({}), "title");
  assert(decision.action === "none", "TEST F: Enter must not commit a title edit");
  assert(decision.preventDefault === true, "TEST F: Enter must not submit/blur a title editor");
  assert(decision.stopPropagation === true, "TEST F: Enter must not reach global shortcuts");
}

// ---------- Chinese Title editor behaves identically.
{
  const decision = resolveTitleEditorKeyDecision(key({}), "chineseTitle");
  assert(decision.action === "none", "Chinese Title Enter action must be none");
  assert(decision.preventDefault === true, "Chinese Title Enter must not submit or blur");
}

// ---------- IME: composition Enter belongs to the input method.
{
  for (const field of ["title", "chineseTitle", "authors", "note", "abstract"]) {
    const composing = resolveTitleEditorKeyDecision(key({ isComposing: true }), field);
    assert(composing.action === "none", `IME Enter must be inert for ${field}`);
    assert(composing.preventDefault === false, `IME Enter must not be cancelled for ${field}`);
    assert(composing.stopPropagation === false, `IME Enter must not be swallowed for ${field}`);
    const legacyComposing = resolveTitleEditorKeyDecision(key({ keyCode: 229 }), field);
    assert(legacyComposing.action === "none" && legacyComposing.preventDefault === false, `keyCode 229 must be treated as IME for ${field}`);
  }
}

// ---------- Non-title fields keep the existing Enter-to-save behaviour.
{
  assert(resolveTitleEditorKeyDecision(key({}), "authors").action === "commit", "other single-line fields still commit on Enter");
  assert(resolveTitleEditorKeyDecision(key({}), "note").action === "commit", "notes still commit on Enter");
  assert(resolveTitleEditorKeyDecision(key({ shiftKey: true }), "note").action === "none", "Shift+Enter stays a newline");
  assert(resolveTitleEditorKeyDecision(key({ key: "Escape" }), "title").action === "cancel", "Escape still cancels the editor");
  assert(resolveTitleEditorKeyDecision(key({ key: "a" }), "title").action === "none", "typing is untouched");
}

// ---------- Translate icon: idle / translating / success / error states.
{
  const feedback = (phase: InspectorActionFeedback["phase"], paperId = 7, requestId = 1, message = ""): InspectorActionFeedback =>
    ({ paperId, requestId, phase, message });

  const idle = titleTranslationControlState(null, 7);
  assert(idle.statusText === "" && idle.disabled === false && idle.busy === false, "idle state renders no standing text");

  const running = titleTranslationControlState(feedback("running"), 7);
  assert(running.busy === true && running.disabled === true && running.statusText !== "", "translating state must be visible and reentrant-safe");

  const done = titleTranslationControlState(feedback("done", 7, 1, "中文标题已更新"), 7);
  assert(done.busy === false && done.statusText === "中文标题已更新" && done.tone === "done", "success state");

  const failed = titleTranslationControlState(feedback("error", 7, 1, "中文标题翻译失败：network"), 7);
  assert(failed.busy === false && failed.tone === "error" && failed.statusText.includes("失败"), "errors must never be silent");

  const otherPaper = titleTranslationControlState(feedback("running", 9), 7);
  assert(otherPaper.busy === false && otherPaper.disabled === false && otherPaper.statusText === "", "state is scoped to one paper");
}

// ---------- The icon carries no standing text label; meaning lives in tooltip/aria.
{
  assert(titleTranslationAriaLabel(true) === "重新翻译中文标题", "an existing Chinese title offers a retranslate label");
  assert(titleTranslationAriaLabel(false) === "翻译中文标题", "a missing Chinese title offers a translate label");
}

// ---------- Success feedback is transient; errors are not.
{
  assert(SUCCESS_FEEDBACK_MS >= 1500 && SUCCESS_FEEDBACK_MS <= 2000, "success display duration must be ~1.5-2s");

  const success: InspectorActionFeedback = { paperId: 7, requestId: 4, phase: "done", message: "中文标题已更新" };
  assert(shouldAutoDismissSuccess(success, 7, 4), "the request that scheduled the timer may clear its own success state");
  // A stale timer from an older request of the same paper must not clear it.
  assert(!shouldAutoDismissSuccess(success, 7, 3), "an older request must not clear newer success state");
  // Switching papers must not let the old paper's timer wipe the new state.
  const nextPaper: InspectorActionFeedback = { paperId: 9, requestId: 5, phase: "done", message: "中文标题已更新" };
  assert(!shouldAutoDismissSuccess(nextPaper, 7, 4), "another paper's timer must not clear this paper's state");
  assert(shouldAutoDismissSuccess(nextPaper, 9, 5), "the new paper's own timer still works");
  // Errors never auto-dismiss.
  const failure: InspectorActionFeedback = { paperId: 7, requestId: 4, phase: "error", message: "中文标题翻译失败" };
  assert(!shouldAutoDismissSuccess(failure, 7, 4), "an error must never auto-dismiss, even for its own request");
  assert(!shouldAutoDismissSuccess(null, 7, 4), "nothing to clear");
}

// ---------- Metadata refresh reuses the same transient contract, not the LLM.
{
  const view = (phase: InspectorActionFeedback["phase"], message = "") =>
    actionControlState({ paperId: 3, requestId: 2, phase, message }, 3, {
      busy: "正在更新引用元数据…",
      done: "引用元数据已更新",
      error: "引用元数据更新失败",
    });
  assert(view("running").busy === true && view("running").statusText === "正在更新引用元数据…", "metadata refresh busy state");
  assert(view("done").tone === "done" && view("done").statusText === "引用元数据已更新", "metadata refresh success state");
  assert(view("error").tone === "error" && !shouldAutoDismissSuccess({ paperId: 3, requestId: 2, phase: "error", message: "" }, 3, 2), "metadata refresh errors persist");
}

console.log("title translation tests passed");

import {
  resolveManualTitleTranslationSource,
  resolveTitleEditorKeyDecision,
  titleTranslationButtonState,
  type TitleEditorKeyEvent,
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

// ---------- Translate button state machine: idle / translating / success / error.
{
  const idle = titleTranslationButtonState(null, 7, false);
  assert(idle.label === "翻译中文标题" && idle.disabled === false && idle.statusText === "", "idle state");
  const idleWithChinese = titleTranslationButtonState(null, 7, true);
  assert(idleWithChinese.label === "重新翻译中文标题", "an existing Chinese title offers a retranslate label");

  const running = titleTranslationButtonState({ paperId: 7, phase: "running", message: "" }, 7, false);
  assert(running.disabled === true && running.label === "翻译中…" && running.statusText !== "", "translating state must be visible and reentrant-safe");

  const done = titleTranslationButtonState({ paperId: 7, phase: "done", message: "中文标题已更新" }, 7, false);
  assert(done.disabled === false && done.statusText === "中文标题已更新" && done.tone === "done", "success state");

  const failed = titleTranslationButtonState({ paperId: 7, phase: "error", message: "中文标题翻译失败：network" }, 7, false);
  assert(failed.disabled === false && failed.tone === "error" && failed.statusText.includes("失败"), "errors must never be silent");

  const otherPaper = titleTranslationButtonState({ paperId: 9, phase: "running", message: "" }, 7, false);
  assert(otherPaper.disabled === false && otherPaper.label === "翻译中文标题", "state is scoped to one paper");
}

console.log("title translation tests passed");

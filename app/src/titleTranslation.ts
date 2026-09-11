/**
 * Title-translation policy (v0.2.2 RC4 invariant, permanent).
 *
 * 1. AUTO TITLE TRANSLATION = NO. A Chinese title is only ever produced by an
 *    explicit user click on “翻译中文标题”. PDF import, Library import,
 *    metadata enrichment, metadata refresh, English-title save, app startup,
 *    background workers, recommendation analysis and Discovery analysis never
 *    translate.
 * 2. MANUAL ONLY, and the source is always the English title the user is
 *    currently looking at:
 *      a. the open editor draft,
 *      b. otherwise the effective persisted English title,
 *      c. and the canonical English title only when (a) and (b) are absent.
 *    The canonical title must never be substituted for a visible title.
 *
 * Everything here is pure so the policy is regression-tested without a DOM;
 * `main.ts` supplies the draft/effective/canonical strings it rendered.
 */

export interface ManualTitleTranslationSource {
  /** Exact string to send to the provider. */
  source: string;
  /** True when the string came from a live editor draft (persist it first). */
  hasDraft: boolean;
  /** Non-null when no title could be resolved; nothing may translate silently. */
  error: string | null;
}

/**
 * `draft` is `null` when no title editor is open. An open but empty editor is
 * an explicit empty title rather than a missing one, so it fails loudly instead
 * of falling back to a title the user just cleared.
 */
export function resolveManualTitleTranslationSource(input: {
  draft: string | null;
  effectiveTitle: string | null;
  canonicalTitle: string | null;
}): ManualTitleTranslationSource {
  const missing = (error: string): ManualTitleTranslationSource => ({ source: "", hasDraft: false, error });
  if (input.draft !== null) {
    const draft = input.draft.trim();
    return draft
      ? { source: draft, hasDraft: true, error: null }
      : missing("请先填写英文标题");
  }
  const effective = (input.effectiveTitle ?? "").trim();
  if (effective) return { source: effective, hasDraft: false, error: null };
  const canonical = (input.canonicalTitle ?? "").trim();
  if (canonical) return { source: canonical, hasDraft: false, error: null };
  return missing("这篇文献还没有英文标题");
}

export type TitleEditorKeyAction = "none" | "cancel" | "commit";

export interface TitleEditorKeyEvent {
  key: string;
  shiftKey: boolean;
  isComposing: boolean;
  /** Some IMEs deliver the commit Enter as keyCode 229 without isComposing. */
  keyCode?: number;
}

export interface TitleEditorKeyDecision {
  action: TitleEditorKeyAction;
  preventDefault: boolean;
  stopPropagation: boolean;
}

/**
 * Enter never means “edit finished” for a title editor.
 *
 * The English Title and Chinese Title editors are intentionally not
 * commit-on-Enter controls: Enter must not save, submit, finish the edit, blur,
 * lose the draft or start a translation, and the browser must not treat it as a
 * form submit. `blur`, an outside click, or Escape remain the ways out.
 * Composition events are handed entirely to the IME, so a commit Enter can
 * never be mistaken for a save.
 */
export function resolveTitleEditorKeyDecision(
  event: TitleEditorKeyEvent,
  field: string,
): TitleEditorKeyDecision {
  const inert: TitleEditorKeyDecision = { action: "none", preventDefault: false, stopPropagation: false };
  if (event.isComposing || event.keyCode === 229) return inert;
  if (event.key === "Escape") return { action: "cancel", preventDefault: true, stopPropagation: false };
  if (event.key !== "Enter") return inert;
  if (field === "title" || field === "chineseTitle") {
    return { action: "none", preventDefault: true, stopPropagation: true };
  }
  if (event.shiftKey) return inert;
  return { action: "commit", preventDefault: true, stopPropagation: false };
}

export type InspectorActionPhase = "idle" | "running" | "done" | "error";

/**
 * Feedback for one explicit Inspector action (manual title translation,
 * deterministic metadata refresh).
 *
 * `requestId` is what makes transient success safe across the Inspector's
 * `innerHTML` rebuilds: a timer may only clear the feedback it scheduled
 * itself, so a stale timer can never wipe a newer request's state — and never
 * a state belonging to a different paper.
 */
export interface InspectorActionFeedback {
  paperId: number;
  requestId: number;
  phase: InspectorActionPhase;
  message: string;
}

/** Success is transient feedback; errors are not. */
export const SUCCESS_FEEDBACK_MS = 1800;

/**
 * True only for the exact request that scheduled the dismiss timer. A stale
 * timer (older request of the same paper, or any request of another paper)
 * must leave the current feedback untouched.
 */
export function shouldAutoDismissSuccess(
  feedback: InspectorActionFeedback | null,
  paperId: number,
  requestId: number,
): boolean {
  return feedback !== null
    && feedback.phase === "done"
    && feedback.paperId === paperId
    && feedback.requestId === requestId;
}

export interface InspectorActionControlState {
  /** The control is showing work in flight (disabled + `aria-busy`). */
  busy: boolean;
  disabled: boolean;
  /**
   * Short inline label for the row's *fixed-width* feedback slot. It is short
   * on purpose: the slot width is reserved up front, so the label can never
   * resize the row's trailing column, wrap the value, or move an icon.
   */
  statusText: string;
  /** Full sentence for `title` and the global status region. */
  detail: string;
  tone: "idle" | "running" | "done" | "error";
}

/** Resolve the visible feedback for the given action and paper. */
export function actionControlState(
  feedback: InspectorActionFeedback | null,
  paperId: number,
  messages: { busy: string; done: string; error: string },
): InspectorActionControlState {
  if (!feedback || feedback.paperId !== paperId) {
    return { busy: false, disabled: false, statusText: "", detail: "", tone: "idle" };
  }
  const label = (fallback: string): { statusText: string; detail: string } => ({
    statusText: fallback,
    detail: feedback.message || fallback,
  });
  switch (feedback.phase) {
    case "running":
      return { busy: true, disabled: true, ...label(messages.busy), tone: "running" };
    case "done":
      return { busy: false, disabled: false, ...label(messages.done), tone: "done" };
    case "error":
      return { busy: false, disabled: false, ...label(messages.error), tone: "error" };
    default:
      return { busy: false, disabled: false, statusText: "", detail: "", tone: "idle" };
  }
}

/**
 * The icon affordance for a manual title translation. The visible string is
 * only ever a tooltip / accessible name — the standing text label is gone, so
 * when a Chinese title already exists the action reads as a *re*translation.
 */
export function titleTranslationControlState(
  feedback: InspectorActionFeedback | null,
  paperId: number,
): InspectorActionControlState {
  return actionControlState(feedback, paperId, {
    busy: "翻译中",
    done: "已更新",
    error: "翻译失败",
  });
}

export function titleTranslationAriaLabel(hasChineseTitle: boolean): string {
  return hasChineseTitle ? "重新翻译中文标题" : "翻译中文标题";
}

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

export type TitleTranslationPhase = "idle" | "running" | "done" | "error";

export interface TitleTranslationState {
  paperId: number;
  phase: TitleTranslationPhase;
  message: string;
}

export interface TitleTranslationButtonState {
  /** Never a silent no-op: every phase renders something the user can see. */
  label: string;
  disabled: boolean;
  statusText: string;
  tone: "idle" | "running" | "done" | "error";
}

/**
 * Derive the Translate button and its inline status from module state rather
 * than from the DOM node that was clicked. The Library Inspector is rebuilt
 * with `innerHTML` whenever Library data reloads (including the reload caused
 * by the title editor's own blur-save), which detaches the clicked button — so
 * anything written directly onto that node is discarded. Rendering from state
 * keeps `translating` / `done` / `error` visible across those rerenders.
 */
export function titleTranslationButtonState(
  state: TitleTranslationState | null,
  paperId: number,
  hasChineseTitle: boolean,
): TitleTranslationButtonState {
  const idleLabel = hasChineseTitle ? "重新翻译中文标题" : "翻译中文标题";
  if (!state || state.paperId !== paperId) {
    return { label: idleLabel, disabled: false, statusText: "", tone: "idle" };
  }
  switch (state.phase) {
    case "running":
      return { label: "翻译中…", disabled: true, statusText: "正在翻译中文标题…", tone: "running" };
    case "done":
      return { label: idleLabel, disabled: false, statusText: state.message || "中文标题已更新", tone: "done" };
    case "error":
      return { label: idleLabel, disabled: false, statusText: state.message || "中文标题翻译失败", tone: "error" };
    default:
      return { label: idleLabel, disabled: false, statusText: "", tone: "idle" };
  }
}

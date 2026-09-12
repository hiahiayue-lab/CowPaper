/**
 * The Library annotation display contract.
 *
 * The backend owns PDF parsing and attachment identity. The Inspector only
 * renders the normalized record; it never treats a comment as quoted text and
 * never creates or persists an annotation row in the UI.
 */

export type LibraryAnnotation = {
  id: string;
  paperId: number;
  attachmentId: number;
  attachmentName: string;
  kind: string;
  color: string | null;
  pageIndex: number;
  excerpt: string | null;
  note: string | null;
  extractionStatus: string;
};

function text(value: unknown): string | null {
  if (typeof value === "number" && Number.isFinite(value)) return String(value);
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed || null;
}

function integer(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isInteger(value) ? value : fallback;
}

/** Drop malformed records at the UI boundary without hiding a valid sibling. */
export function normalizeLibraryAnnotations(value: unknown): LibraryAnnotation[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((raw): LibraryAnnotation[] => {
    if (!raw || typeof raw !== "object") return [];
    const record = raw as Record<string, unknown>;
    const id = text(record.id);
    const paperId = integer(record.paperId, 0);
    const attachmentId = integer(record.attachmentId, 0);
    const pageIndex = integer(record.pageIndex, -1);
    if (!id || paperId <= 0 || attachmentId <= 0 || pageIndex < 0) return [];
    return [{
      id,
      paperId,
      attachmentId,
      attachmentName: text(record.attachmentName) || "PDF 附件",
      kind: text(record.kind) || "unknown",
      color: text(record.color),
      pageIndex,
      excerpt: text(record.excerpt) || text(record.quotedText),
      note: text(record.note) || text(record.comment),
      extractionStatus: text(record.extractionStatus) || "metadata_only",
    }];
  });
}

export function annotationKindLabel(kind: string): string {
  switch (kind) {
    case "highlight": return "高亮";
    case "underline": return "下划线";
    case "strikeout": return "删除线";
    case "text": return "便签";
    case "freetext": return "文字框";
    default: return "标注";
  }
}

export function annotationStatusLabel(status: string): string {
  switch (status) {
    case "extracted":
    case "completed": return "";
    case "no_text": return "暂无可验证的页面摘录";
    case "scanned": return "扫描 PDF · 没有文字层";
    case "malformed": return "标注数据不完整";
    case "missing_attachment": return "PDF 附件不可用";
    case "encrypted": return "PDF 已加密，无法读取标注";
    case "unsupported": return "标注类型暂不支持";
    case "stale": return "该标注已不在当前 PDF 中";
    default: return "页面摘录暂不可用";
  }
}

function escapeHtml(value: string): string {
  const entities: Record<string, string> = {
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    "'": "&#39;",
    '"': "&quot;",
  };
  return value.replace(/[&<>'"]/g, (character) => entities[character] || character);
}

function safeColor(color: string | null): string {
  return color && /^#[0-9a-f]{6}$/i.test(color) ? color : "#d7dce2";
}

/** Render one compact card using only the existing Inspector surface language. */
export function renderLibraryAnnotationCard(annotation: LibraryAnnotation): string {
  const status = annotationStatusLabel(annotation.extractionStatus);
  const excerpt = annotation.excerpt
    ? `<blockquote class="library-annotation-excerpt">${escapeHtml(annotation.excerpt)}</blockquote>`
    : `<span class="library-annotation-empty">${escapeHtml(status || "暂无页面摘录")}</span>`;
  const note = annotation.note
    ? `<div class="library-annotation-note"><span class="library-annotation-note-label">批注</span><p>${escapeHtml(annotation.note)}</p></div>`
    : "";
  return `<article class="library-annotation-card" data-annotation-id="${escapeHtml(annotation.id)}"><div class="library-annotation-head"><span class="library-annotation-kind"><span class="library-annotation-color" style="background:${safeColor(annotation.color)}" aria-label="标注颜色" title="标注颜色"></span><strong>${escapeHtml(annotationKindLabel(annotation.kind))}</strong><span>第 ${annotation.pageIndex + 1} 页</span></span><span class="library-annotation-attachment" title="${escapeHtml(annotation.attachmentName)}">${escapeHtml(annotation.attachmentName)}</span></div>${excerpt}${status && annotation.excerpt ? `<span class="library-annotation-status">${escapeHtml(status)}</span>` : ""}${note}</article>`;
}

/* ================= Inspector tab contract (v0.3.0) ================= */

/**
 * The Inspector is a two-view surface. `元数据` owns canonical/Library
 * metadata; `标注` owns the attachment-scoped annotation projection. The
 * annotation view is never rendered inside the metadata view.
 */
export type InspectorTab = "metadata" | "annotations";

export const INSPECTOR_TABS: ReadonlyArray<{ id: InspectorTab; label: string }> = [
  { id: "metadata", label: "元数据" },
  { id: "annotations", label: "标注" },
];

/**
 * Selecting a paper defaults to `元数据`. Switching papers keeps the tab the
 * user is already on instead of forcing them back. With no selected paper the
 * Inspector keeps its existing empty behavior and no tab is rendered.
 *
 * Tab state is front-end session state only: no DB field, no migration.
 */
export function resolveInspectorTab(current: InspectorTab | null, hasSelectedPaper: boolean): InspectorTab | null {
  if (!hasSelectedPaper) return null;
  return current === "annotations" ? "annotations" : "metadata";
}

export type AnnotationPanelKind =
  | "no-pdf"
  | "pdf-missing"
  | "error"
  | "reading"
  | "unread"
  | "empty"
  | "list";

export interface AnnotationPanelState {
  kind: AnnotationPanelKind;
  /** The read/refresh icon action is offered and enabled. */
  canRead: boolean;
  /** Annotation rows for the selected paper (0 unless `kind === "list"`). */
  count: number;
  /** Inline message for empty/error states; empty for `list`. */
  message: string;
  tone: "muted" | "error";
}

/**
 * Resolve the annotation tab body from the paper's attachment shape and the
 * in-memory read state. Every state is explicit: a paper without a PDF, a PDF
 * that cannot be read, a read in flight, a read that has not been requested
 * yet, an empty result, an error, and a populated list.
 */
export function resolveAnnotationPanelState(input: {
  attachmentCount: number;
  usableAttachmentCount: number;
  readState: "unread" | "loading" | "loaded" | "error";
  error: string | null;
  count: number;
}): AnnotationPanelState {
  const { attachmentCount, usableAttachmentCount, readState, error, count } = input;
  if (attachmentCount === 0) {
    return { kind: "no-pdf", canRead: false, count: 0, message: "此文献尚未关联 PDF", tone: "muted" };
  }
  if (usableAttachmentCount === 0) {
    return { kind: "pdf-missing", canRead: false, count: 0, message: "PDF 文件不可用，请先重新链接", tone: "muted" };
  }
  if (readState === "error") {
    return { kind: "error", canRead: true, count: 0, message: `无法读取 PDF 标注：${error || "未知错误"}`, tone: "error" };
  }
  if (readState === "loading") {
    return { kind: "reading", canRead: false, count: 0, message: "正在读取 PDF 标注…", tone: "muted" };
  }
  if (readState === "unread") {
    return { kind: "unread", canRead: true, count: 0, message: "尚未读取 PDF 标注", tone: "muted" };
  }
  if (count > 0) {
    return { kind: "list", canRead: true, count, message: "", tone: "muted" };
  }
  return { kind: "empty", canRead: true, count: 0, message: "此 PDF 暂无标注", tone: "muted" };
}

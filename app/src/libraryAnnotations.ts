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
  /** First marked region, in PDF coordinates, for stable page ordering. */
  position: { top: number; left: number } | null;
};

export type AnnotationFilter = "all" | "highlight" | "underline" | "note";

export type LibraryAnnotationNormalizeOptions = {
  attachmentNames?: ReadonlyMap<number, string>;
  attachmentOrder?: ReadonlyMap<number, number>;
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

function numberArray(value: unknown): number[] | null {
  if (!Array.isArray(value)) return null;
  const values = value.filter((item): item is number => typeof item === "number" && Number.isFinite(item));
  return values.length ? values : null;
}

function annotationPosition(value: unknown): { top: number; left: number } | null {
  if (typeof value !== "string") return null;
  let raw: unknown;
  try {
    raw = JSON.parse(value);
  } catch {
    return null;
  }
  if (!raw || typeof raw !== "object") return null;
  const record = raw as Record<string, unknown>;
  const points = numberArray(record.quadPoints) || numberArray(record.quadpoints);
  const geometry = points && points.length >= 8
    ? points.slice(0, 8)
    : numberArray(record.rect) || numberArray(record.Rect);
  if (!geometry || geometry.length < 4) return null;
  const xs = geometry.filter((_, index) => index % 2 === 0);
  const ys = geometry.filter((_, index) => index % 2 === 1);
  if (!xs.length || !ys.length) return null;
  // PDF coordinates grow upward. Sorting by negative max-Y gives the visual
  // top-to-bottom order without requiring page dimensions.
  return { top: -Math.max(...ys), left: Math.min(...xs) };
}

function compareNumber(left: number, right: number): number {
  return left === right ? 0 : left < right ? -1 : 1;
}

/** Stable attachment/page/geometry order; the annotation id is the final tie-breaker. */
export function sortLibraryAnnotations(
  annotations: LibraryAnnotation[],
  attachmentOrder?: ReadonlyMap<number, number>,
): LibraryAnnotation[] {
  return [...annotations].sort((left, right) => {
    const leftAttachmentOrder = attachmentOrder?.get(left.attachmentId) ?? left.attachmentId;
    const rightAttachmentOrder = attachmentOrder?.get(right.attachmentId) ?? right.attachmentId;
    return compareNumber(leftAttachmentOrder, rightAttachmentOrder)
      || compareNumber(left.attachmentId, right.attachmentId)
      || compareNumber(left.pageIndex, right.pageIndex)
      || (left.position && right.position
        ? compareNumber(left.position.top, right.position.top) || compareNumber(left.position.left, right.position.left)
        : left.position ? -1 : right.position ? 1 : 0)
      || (left.id === right.id ? 0 : left.id < right.id ? -1 : 1);
  });
}

/** Drop malformed records at the UI boundary without hiding a valid sibling. */
export function normalizeLibraryAnnotations(value: unknown, options: LibraryAnnotationNormalizeOptions = {}): LibraryAnnotation[] {
  if (!Array.isArray(value)) return [];
  const annotations = value.flatMap((raw): LibraryAnnotation[] => {
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
      attachmentName: text(record.attachmentName) || options.attachmentNames?.get(attachmentId) || "PDF 附件",
      kind: text(record.kind) || "unknown",
      color: text(record.color),
      pageIndex,
      excerpt: text(record.excerpt) || text(record.quotedText),
      note: text(record.note) || text(record.comment),
      extractionStatus: text(record.extractionStatus) || "metadata_only",
      position: annotationPosition(record.rawMetadataJson || record.raw_metadata_json),
    }];
  });
  return sortLibraryAnnotations(annotations, options.attachmentOrder);
}

function annotationFilterBucket(kind: string): Exclude<AnnotationFilter, "all"> {
  return kind === "highlight" ? "highlight" : kind === "underline" ? "underline" : "note";
}

export function annotationFilterMatches(annotation: LibraryAnnotation, filter: AnnotationFilter): boolean {
  return filter === "all" || annotationFilterBucket(annotation.kind) === filter;
}

export function filterLibraryAnnotations(annotations: LibraryAnnotation[], filter: AnnotationFilter): LibraryAnnotation[] {
  return annotations.filter((annotation) => annotationFilterMatches(annotation, filter));
}

export function annotationFilterOptions(annotations: LibraryAnnotation[], activeFilter?: AnnotationFilter): ReadonlyArray<{ id: AnnotationFilter; label: string }> {
  const options: Array<{ id: AnnotationFilter; label: string }> = [{ id: "all", label: "全部" }];
  const counts = new Map<Exclude<AnnotationFilter, "all">, number>();
  for (const annotation of annotations) {
    const bucket = annotationFilterBucket(annotation.kind);
    counts.set(bucket, (counts.get(bucket) || 0) + 1);
  }
  for (const id of ["highlight", "underline", "note"] as const) {
    if ((counts.get(id) || 0) > 0 || activeFilter === id) options.push({ id, label: id === "highlight" ? "高亮" : id === "underline" ? "下划线" : "批注" });
  }
  return options;
}

export function annotationKindLabel(kind: string): string {
  switch (kind) {
    case "highlight": return "高亮";
    case "underline": return "下划线";
    case "strikeout": return "删除线";
    case "text":
    case "freetext":
    case "note":
    case "sticky_note": return "批注";
    default: return "标注";
  }
}

/** True for annotations that mark up existing page text (so a PDF quote is expected). */
export function annotationKindHasPageText(kind: string): boolean {
  return kind === "highlight" || kind === "underline" || kind === "strikeout";
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

function expandableText(value: string): boolean {
  return value.length > 240 || value.split(/\r?\n/).length > 6;
}

function annotationDomId(annotation: LibraryAnnotation, field: "quote" | "note"): string {
  const safeId = annotation.id.replace(/[^a-zA-Z0-9_-]/g, "-");
  return `library-annotation-${field}-${safeId}`;
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

/**
 * Render one compact card using only the existing Inspector surface language.
 *
 * The PDF quote and the annotation comment are separate sections and are never
 * mixed: the quote is page text recovered from the annotation geometry, the
 * comment is the annotation's own /Contents. A markup annotation without a
 * recoverable quote states that plainly (the specific reason stays in `title`)
 * instead of substituting the comment or nearby text.
 */
export function renderLibraryAnnotationCard(
  annotation: LibraryAnnotation,
  options: { showAttachmentName?: boolean; quoteExpanded?: boolean; noteExpanded?: boolean } = {},
): string {
  const status = annotationStatusLabel(annotation.extractionStatus);
  const showAttachmentName = options.showAttachmentName !== false;
  const quoteExpanded = Boolean(options.quoteExpanded);
  const noteExpanded = Boolean(options.noteExpanded);
  const expandable = (value: string, field: "quote" | "note", expanded: boolean, element: string, className: string): string => {
    const canExpand = expandableText(value);
    const domId = annotationDomId(annotation, field);
    const collapsed = canExpand && !expanded ? " is-collapsed" : "";
    const toggle = canExpand
      ? `<button type="button" class="library-annotation-expand ghost small" data-action="library-toggle-annotation-text" data-annotation-id="${escapeHtml(annotation.id)}" data-annotation-field="${field}" aria-expanded="${expanded}" aria-controls="${domId}">${expanded ? "收起" : "展开"}</button>`
      : "";
    return `<div class="library-annotation-text-block"><${element} id="${domId}" class="${className}${collapsed}">${escapeHtml(value)}</${element}>${toggle}</div>`;
  };
  const quote = annotationKindHasPageText(annotation.kind)
    ? `<div class="library-annotation-quote"><span class="library-annotation-section-label">PDF 原文</span>${
        annotation.excerpt
          ? expandable(annotation.excerpt, "quote", quoteExpanded, "blockquote", "library-annotation-excerpt")
          : `<span class="library-annotation-empty" title="${escapeHtml(status)}">无法提取此标注的页面原文</span>`
      }</div>`
    : "";
  // An empty comment section is never rendered.
  const note = annotation.note
    ? `<div class="library-annotation-note"><span class="library-annotation-section-label">批注</span>${expandable(annotation.note, "note", noteExpanded, "p", "library-annotation-note-text")}</div>`
    : "";
  const attachment = showAttachmentName
    ? `<span class="library-annotation-attachment" title="${escapeHtml(annotation.attachmentName)}">${escapeHtml(annotation.attachmentName)}</span>`
    : "";
  return `<article class="library-annotation-card" data-annotation-id="${escapeHtml(annotation.id)}"><div class="library-annotation-head"><span class="library-annotation-kind"><span class="library-annotation-color" style="background:${safeColor(annotation.color)}" aria-label="标注颜色" title="标注颜色"></span><strong>${escapeHtml(annotationKindLabel(annotation.kind))}</strong><span>第 ${annotation.pageIndex + 1} 页</span></span>${attachment}</div>${quote}${note}</article>`;
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

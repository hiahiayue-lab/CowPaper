/**
 * The read-only Library annotation contract.
 *
 * The backend owns PDF parsing and attachment identity. The Inspector only
 * renders the normalized record; it never treats a comment as quoted text and
 * never creates or persists an annotation row.
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
      excerpt: text(record.excerpt),
      note: text(record.note),
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
    case "extracted": return "";
    case "no_text": return "暂无可验证的页面摘录";
    case "scanned": return "扫描 PDF · 没有文字层";
    case "malformed": return "标注数据不完整";
    case "missing_attachment": return "PDF 附件不可用";
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

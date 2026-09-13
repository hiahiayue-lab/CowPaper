import {
  annotationFilterOptions,
  annotationKindLabel,
  annotationStatusLabel,
  filterLibraryAnnotations,
  INSPECTOR_TABS,
  normalizeLibraryAnnotations,
  renderLibraryAnnotationCard,
  resolveAnnotationPanelState,
  resolveInspectorTab,
} from "./libraryAnnotations.ts";

const annotations = normalizeLibraryAnnotations([
  {
    id: "v1:nm:7:2:note-1",
    paperId: 11,
    attachmentId: 7,
    attachmentName: "paper.pdf",
    kind: "highlight",
    color: "#ffd84d",
    pageIndex: 2,
    excerpt: "Reliable quoted text.",
    note: "Review this result",
    extractionStatus: "extracted",
  },
  { id: "bad", paperId: 0, attachmentId: 7, pageIndex: 1 },
]);

if (annotations.length !== 1) throw new Error("normalizer keeps only valid attachment records");
if (annotations[0].pageIndex !== 2) throw new Error("page index is preserved as zero-based backend data");
if (annotationKindLabel("strikeout") !== "删除线") throw new Error("annotation kind label mismatch");
if (annotationStatusLabel("no_text") !== "暂无可验证的页面摘录") throw new Error("annotation status label mismatch");

const html = renderLibraryAnnotationCard(annotations[0]);
if (!html.includes("高亮") || !html.includes("第 3 页") || !html.includes("paper.pdf")) throw new Error("annotation card metadata missing");
if (!html.includes("Reliable quoted text.") || !html.includes("Review this result")) throw new Error("annotation card content missing");
if (!html.includes("PDF 原文") || !html.includes("批注")) throw new Error("quote and comment need separate section labels");
const quoteIndex = html.indexOf("PDF 原文");
const noteIndex = html.indexOf("批注");
if (quoteIndex < 0 || noteIndex < 0 || quoteIndex > noteIndex) throw new Error("the PDF quote section must come first");
if (html.includes("<script>")) throw new Error("annotation text must be escaped");

const metadataOnly = normalizeLibraryAnnotations([{
  id: "v1:fingerprint:2",
  paperId: 11,
  attachmentId: 7,
  attachmentName: "paper.pdf",
  kind: "text",
  color: null,
  pageIndex: 0,
  excerpt: null,
  note: "A note",
  extractionStatus: "metadata_only",
}])[0];
// A sticky note has no marked-up page text, so it must not claim a PDF quote.
const stickyNote = renderLibraryAnnotationCard(metadataOnly);
if (stickyNote.includes("PDF 原文")) throw new Error("a text note must not invent a PDF quote section");
if (!stickyNote.includes("批注") || !stickyNote.includes("A note")) throw new Error("a text note keeps its comment");

// A markup annotation whose quote could not be recovered states that plainly and
// keeps the specific extraction reason in the tooltip.
const unrecovered = renderLibraryAnnotationCard({ ...annotations[0], excerpt: null, extractionStatus: "no_text", note: "Review this result" });
if (!unrecovered.includes("PDF 原文")) throw new Error("a markup annotation keeps its PDF quote section");
if (!unrecovered.includes("无法提取此标注的页面原文")) throw new Error("missing quote must state so plainly");
if (!unrecovered.includes(`title="${annotationStatusLabel("no_text")}"`)) throw new Error("the specific extraction reason belongs in the title");
if (!unrecovered.includes("Review this result")) throw new Error("the comment is still shown next to an unrecovered quote");
if (unrecovered.includes(`>${"暂无可验证的页面摘录"}<`)) throw new Error("the status wording must stay in the tooltip, not the body");

// No comment, no comment section.
if (renderLibraryAnnotationCard({ ...annotations[0], note: null }).includes("批注")) throw new Error("an empty comment section must not be rendered");

// Page order is derived from the first QuadPoints region (PDF Y is inverted
// for visual ordering), then falls back to the stable annotation id.
const ordered = normalizeLibraryAnnotations([
  { ...annotations[0], id: "right", pageIndex: 0, rawMetadataJson: JSON.stringify({ quadPoints: [100, 100, 120, 100, 100, 90, 120, 90] }) },
  { ...annotations[0], id: "next-page", pageIndex: 1, rawMetadataJson: JSON.stringify({ quadPoints: [1, 100, 20, 100, 1, 90, 20, 90] }) },
  { ...annotations[0], id: "left", pageIndex: 0, rawMetadataJson: JSON.stringify({ quadPoints: [10, 100, 30, 100, 10, 90, 30, 90] }) },
  { ...annotations[0], id: "second-attachment", attachmentId: 8, pageIndex: 0, rawMetadataJson: JSON.stringify({ quadPoints: [1, 100, 20, 100, 1, 90, 20, 90] }) },
], { attachmentOrder: new Map([[7, 0], [8, 1]]) });
if (ordered.map((annotation) => annotation.id).join("|") !== "left|right|next-page|second-attachment") throw new Error("annotation attachment/page/geometry ordering must be deterministic");

const filterFixtures = normalizeLibraryAnnotations([
  { ...annotations[0], id: "filter-highlight", kind: "highlight" },
  { ...annotations[0], id: "filter-underline", kind: "underline" },
  { ...annotations[0], id: "filter-note", kind: "text" },
  { ...annotations[0], id: "filter-strikeout", kind: "strikeout" },
]);
if (annotationFilterOptions(filterFixtures).map((option) => option.id).join("|") !== "all|highlight|underline|note") throw new Error("annotation filters must map supported kinds to compact user categories");
if (filterLibraryAnnotations(filterFixtures, "highlight").length !== 1) throw new Error("highlight filter mismatch");
if (filterLibraryAnnotations(filterFixtures, "underline").length !== 1) throw new Error("underline filter mismatch");
if (filterLibraryAnnotations(filterFixtures, "note").length !== 2) throw new Error("note filter must include text and strikeout annotations");

const longText = Array.from({ length: 7 }, (_, index) => `Long annotation line ${index + 1}`).join("\n");
const collapsed = renderLibraryAnnotationCard({ ...annotations[0], excerpt: longText, note: longText }, { showAttachmentName: false });
if (!collapsed.includes("library-annotation-excerpt is-collapsed") || !collapsed.includes("library-annotation-note-text is-collapsed")) throw new Error("long quote and comment must collapse by default");
if (!collapsed.includes('data-annotation-field="quote"') || !collapsed.includes('data-annotation-field="note"')) throw new Error("long text needs explicit expand controls");
if (collapsed.includes("paper.pdf")) throw new Error("single-attachment annotation cards should hide repeated attachment names");
const expanded = renderLibraryAnnotationCard({ ...annotations[0], excerpt: longText, note: longText }, { quoteExpanded: true, noteExpanded: true });
if (expanded.includes("library-annotation-excerpt is-collapsed") || expanded.includes("library-annotation-note-text is-collapsed")) throw new Error("expanded annotation text must show the full block");
if (!expanded.includes(">收起</button>")) throw new Error("expanded annotation text needs a collapse control");

console.log("Library annotation rendering contract passed");

// ---------- v0.3.0: Inspector is a real two-tab surface ----------
{
  if (INSPECTOR_TABS.map((tab) => tab.label).join("|") !== "元数据|标注") throw new Error("Inspector must expose exactly 元数据 + 标注");
  if (resolveInspectorTab(null, true) !== "metadata") throw new Error("a newly selected paper opens 元数据");
  if (resolveInspectorTab("annotations", true) !== "annotations") throw new Error("switching papers keeps the 标注 tab");
  if (resolveInspectorTab("metadata", true) !== "metadata") throw new Error("switching papers keeps the 元数据 tab");
  if (resolveInspectorTab("annotations", false) !== null) throw new Error("no selected paper keeps the existing empty Inspector");

  const panel = (o: Partial<Parameters<typeof resolveAnnotationPanelState>[0]>) =>
    resolveAnnotationPanelState({ attachmentCount: 1, usableAttachmentCount: 1, readState: "loaded", error: null, count: 0, ...o });

  const noPdf = panel({ attachmentCount: 0, usableAttachmentCount: 0, readState: "unread" });
  if (noPdf.kind !== "no-pdf" || noPdf.message !== "此文献尚未关联 PDF" || noPdf.canRead) throw new Error("no-PDF empty state");

  const missing = panel({ usableAttachmentCount: 0 });
  if (missing.kind !== "pdf-missing" || missing.canRead) throw new Error("unavailable PDF state");

  const unread = panel({ readState: "unread" });
  if (unread.kind !== "unread" || unread.message !== "尚未读取 PDF 标注" || !unread.canRead) throw new Error("unread state must offer a read action");

  const reading = panel({ readState: "loading" });
  if (reading.kind !== "reading" || reading.canRead || reading.message !== "正在读取 PDF 标注…") throw new Error("reading state");

  const failed = panel({ readState: "error", error: "malformed" });
  if (failed.kind !== "error" || failed.tone !== "error" || !failed.message.includes("malformed") || !failed.canRead) throw new Error("extraction error state must stay visible and recoverable");

  const emptyPdf = panel({});
  if (emptyPdf.kind !== "empty" || emptyPdf.message !== "此 PDF 暂无标注" || !emptyPdf.canRead) throw new Error("no-annotation empty state");

  const listed = panel({ count: 3 });
  if (listed.kind !== "list" || listed.count !== 3) throw new Error("annotation list state");
}



console.log("Library annotation tab + layout contract passed");

import {
  annotationKindLabel,
  annotationStatusLabel,
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
if (!renderLibraryAnnotationCard(metadataOnly).includes("页面摘录暂不可用")) throw new Error("missing excerpt state is visible");

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

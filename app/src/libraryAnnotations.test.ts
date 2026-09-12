import {
  annotationKindLabel,
  annotationStatusLabel,
  normalizeLibraryAnnotations,
  renderLibraryAnnotationCard,
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

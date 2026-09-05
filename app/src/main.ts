import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

interface Journal {
  id: number;
  name: string;
  printIssn: string | null;
  onlineIssn: string | null;
  issnL: string | null;
  publisher: string | null;
  enabled: boolean;
  coverageStatus: string | null;
  abstractCoverageRate: number | null;
  lastSuccessfulSyncAt: string | null;
  lastPaperDate: string | null;
  paperCount: number;
  identifiers: JournalIdentifier[];
  collections: string[];
  possibleDuplicate: boolean;
  metadataNeedsReview: boolean;
}

interface JournalIdentifier {
  id: number;
  journalId: number;
  identifierType: string;
  value: string;
  source: string | null;
  createdAt: string;
  updatedAt: string;
}

interface Author {
  given: string | null;
  family: string | null;
  name: string | null;
}

interface TagMatch {
  tag: string;
  score: number;
  tagId?: number | null;
  semanticHash?: string | null;
}

interface PaperKeyword {
  id: number;
  paperId: number;
  keyword: string;
  normalizedKeyword: string;
  kind: "author_keyword" | "publisher_keyword" | "subject" | "concept" | string;
  source: string;
  confidence: string;
  sourceLocator: string | null;
  sourceRecordId: number | null;
  language: string | null;
  position: number | null;
  retrievedAt: string;
  createdAt: string;
}

interface PaperKeywordInput {
  keyword: string;
  kind: string;
  source: string;
  confidence: string;
  sourceLocator?: string | null;
  language?: string | null;
  position?: number | null;
}

interface Paper {
  id: number;
  journalId: number;
  journalName: string | null;
  publisher?: string | null;
  volume?: string | null;
  issue?: string | null;
  pages?: string | null;
  abstractProvenance?: "provider" | "pdf_structured" | "missing" | "legacy_unverified";
  normalizedDoi: string | null;
  title: string | null;
  authors: Author[];
  publishedDate: string | null;
  abstractText: string | null;
  abstractSource: string | null;
  abstractQuality: string;
  abstractRetrievedAt: string | null;
  /** Round 7 Phase 1：摘要来源落地页 URL（provenance） */
  abstractSourceUrl: string | null;
  /** Round 7 Phase 1：内容类型（技术字段，UI 不直接展示） */
  contentKind: string;
  /** Round 7 Phase 1：内容类型判定置信度（技术字段） */
  contentKindConfidence: string;
  /** Round 7 Phase 1：摘要语义状态 available | missing_recoverable | not_expected | unknown */
  abstractStatus: string;
  url: string | null;
  discoverySource: string | null;
  analysisStatus: string;
  chineseTitle: string | null;
  chineseAbstract: string | null;
  oneSentenceSummary: string | null;
  tagMatches: TagMatch[];
  totalScore: number | null;
  isFavorite: boolean;
  isRead: boolean;
  isIgnored: boolean;
  /** 卡片内摘要语言覆盖（仅 UI 状态，不持久化） */
  _lang?: "zh" | "en";
  /** 所属期刊的 collection code（paper → journal → collections 派生） */
  collections: string[];
  /** canonical bibliographic keywords; separate from Library Tags */
  keywords: PaperKeyword[];
}

interface LibraryCollection {
  id: number;
  parentId: number | null;
  name: string;
  sortOrder: number;
  createdAt: string;
  updatedAt: string;
}

interface LibraryTag {
  id: number;
  name: string;
  color: string | null;
  createdAt: string;
  updatedAt: string;
}

interface LibraryMembership {
  paperId: number;
  addedAt: string;
  addedSource: string;
  collectionIds: number[];
  tagIds: number[];
}

interface PaperAttachment {
  id: number;
  paperId: number;
  kind: string;
  storageMode: "linked" | "managed" | string;
  absolutePath: string;
  relativePath: string | null;
  url: string | null;
  filename: string;
  mimeType: string;
  sha256: string | null;
  createdAt: string;
  updatedAt: string;
  missing: boolean;
}

interface LibraryItemMetadata {
  journalOverride?: string | null;
  publisherOverride?: string | null;
  publicationDateOverride?: string | null;
  volumeOverride?: string | null;
  issueOverride?: string | null;
  pagesOverride?: string | null;
  paperId: number;
  titleOverride: string | null;
  chineseTitleOverride: string | null;
  sourceOverride: string | null;
  yearOverride: number | null;
  authorsOverride: Author[] | null;
  abstractOverride: string | null;
  chineseAbstractOverride: string | null;
  note: string | null;
  updatedAt: string;
}

interface LibraryItemMetadataInput {
  journalOverride?: string | null;
  publisherOverride?: string | null;
  publicationDateOverride?: string | null;
  volumeOverride?: string | null;
  issueOverride?: string | null;
  pagesOverride?: string | null;
  titleOverride: string | null;
  chineseTitleOverride: string | null;
  sourceOverride: string | null;
  yearOverride: number | null;
  authorsOverride: Author[] | null;
  abstractOverride: string | null;
  chineseAbstractOverride: string | null;
  note: string | null;
}

interface LibraryPaper {
  paper: Paper;
  addedAt: string;
  addedSource: string;
  collections: LibraryCollection[];
  tags: LibraryTag[];
  metadata: LibraryItemMetadata | null;
  effectiveJournal?: string | null;
  effectivePublisher?: string | null;
  effectivePublicationDate?: string | null;
  effectiveVolume?: string | null;
  effectiveIssue?: string | null;
  effectivePages?: string | null;
  effectiveTitle: string | null;
  effectiveChineseTitle: string | null;
  effectiveSource: string | null;
  effectiveYear: number | null;
  effectiveAuthors: Author[];
  effectiveAbstract: string | null;
  effectiveChineseAbstract: string | null;
  note: string | null;
  attachments: PaperAttachment[];
}

type LibraryInlineField = "title" | "chineseTitle" | "source" | "publisher" | "publicationDate" | "volume" | "issue" | "pages" | "year" | "authors" | "abstract" | "chineseAbstract" | "note";

interface LibraryDropItem {
  name: string;
  path: string | null;
  state: "queued" | "processing" | "done" | "error";
  message?: string;
}

interface ExternalPdfCandidate {
  paperId: number;
  title: string | null;
  authors: Author[];
  year: number | null;
}

interface ExternalPdfImportResult {
  outcome: "existingDoi" | "existingScholarlyId" | "needsManualConfirmation" | "manualConfirmation" | "createdExternalPaper" | string;
  paperId: number | null;
  attachment: PaperAttachment | null;
  metadata: {
    filename: string;
    title: string | null;
    authors: Author[];
    year: number | null;
    doi: string | null;
    scholarlyId: string | null;
    abstractText: string | null;
    keywords: PaperKeywordInput[];
  };
  candidate: ExternalPdfCandidate | null;
  candidates: ExternalPdfCandidate[];
  requiresConfirmation: boolean;
}

interface AiStatus {
  state: string;
  batchSize: number;
  completed: number;
  success: number;
  failed: number;
  skipped: number;
  remaining: number;
  currentPaperId: number | null;
  currentPaperTitle: string | null;
  batchStartedAt: string | null;
  lastProgressAt: string | null;
  currentPaperStartedAt: string | null;
  retryWaiting: boolean;
  retryUntil: string | null;
  lastError: string | null;
  elapsedSeconds: number;
  etaSeconds: number | null;
  lastRun: LastAiRun | null;
}

interface LastAiRun {
  total: number;
  success: number;
  failed: number;
  skipped: number;
  remaining: number;
  finalStatus: string;
  startedAt: string | null;
  finishedAt: string | null;
  errorSummary: string | null;
}

interface Settings {
  startupAutoSync: boolean;
  dailyAutoSync: boolean;
  dailySyncTime: string;
  autoAnalyzeNew: boolean;
  defaultAbstractLang: string;
  pdfFileHandlingMode: "none" | "copy" | "move";
  pdfLibraryRoot: string;
  pdfNamingTemplate: string;
  pdfSubfolderRule: "none" | "year" | "journal/source";
}

interface SyncProgress {
  batchId: number;
  trigger: string;
  journalTotal: number;
  journalCompleted: number;
  journalFailed: number;
  currentJournal: string | null;
  recordsFound: number;
  papersInserted: number;
  papersExisting: number;
  abstractsAdded: number;
  startedAt: string;
}

interface SyncBatch {
  id: number;
  trigger: string;
  status: string;
  createdAt: string;
  startedAt: string | null;
  finishedAt: string | null;
  journalTotal: number;
  journalCompleted: number;
  journalFailed: number;
  recordsFound: number;
  papersInserted: number;
  papersExisting: number;
  abstractsAdded: number;
  waitingAbstract: number;
  errorSummary: string | null;
}

interface SyncBatchPaper {
  syncBatchId: number;
  paperId: number;
  result: string;
  title: string | null;
}

interface AnalysisBatch {
  id: number;
  sourceSyncBatchId: number | null;
  parentBatchId: number | null;
  trigger: string;
  status: string;
  modelName: string | null;
  promptVersion: string | null;
  createdAt: string;
  startedAt: string | null;
  finishedAt: string | null;
  total: number;
  completed: number;
  succeeded: number;
  failed: number;
  skipped: number;
  remaining: number;
  errorSummary: string | null;
}

interface AnalysisBatchItem {
  id: number;
  analysisBatchId: number;
  paperId: number;
  status: string;
  attemptCount: number;
  startedAt: string | null;
  finishedAt: string | null;
  errorType: string | null;
  errorSummary: string | null;
  title: string | null;
}

interface AbstractRecoveryBatch {
  id: number; status: string; total: number; completed: number; recovered: number; notFound: number; failed: number; remaining: number;
  createdAt: string; startedAt: string | null; finishedAt: string | null; errorSummary: string | null;
}
interface AbstractRecoveryItem { paperId: number; outcome: string | null; currentSource: string | null; title: string | null; nextRetryAt: string | null; errorSummary: string | null; }
interface AbstractRecoveryProgress { batchId: number; completed: number; total: number; currentPaperTitle: string | null; currentSource: string | null; phase: string; recovered: number; notFound: number; failed: number; remaining: number; }

interface ActivityState {
  syncBatch: SyncBatch | null;
  analysisBatch: AnalysisBatch | null;
  lastSync: SyncBatch | null;
  lastAnalysis: AnalysisBatch | null;
  retryWaiting: boolean;
  /** 当前仍待分析数量（实时 DB 计数；与 lastAnalysis.total 严格区分） */
  pendingAnalysis: number;
  /** 分析失败数量 */
  analysisFailed: number;
  /** 等待摘要数量（不计入 pendingAnalysis） */
  waitingForAbstract: number;
}

interface JournalCollection {
  id: number;
  code: string;
  name: string;
  version: string | null;
  effectiveFrom: string | null;
  sourceName: string | null;
  sourceUrl: string | null;
  lastVerifiedAt: string | null;
  createdAt: string;
  updatedAt: string;
  memberCount: number;
}

function isBuiltinCollection(code: string): boolean {
  return code === "UTD24" || code === "FT50";
}

interface BulkSubscribeResult {
  subscribed: number;
  already: number;
  failed: number;
}

interface RecommendationRun {
  id: number;
  cycleKey: string;
  cycleStart: string;
  cycleEnd: string | null;
  status: string;
  createdAt: string;
  finalizedAt: string | null;
  itemCount: number;
  maxScore: number | null;
  journalCount: number;
}

interface RecommendationItemView {
  runId: number;
  paperId: number;
  rank: number;
  scoreSnapshot: number;
  paper: Paper;
}

interface RecommendationRunView {
  run: RecommendationRun;
  items: RecommendationItemView[];
}
interface DailyPaperSummary { cycleKey: string; paperCount: number; missingCount: number; recommendationRunId: number | null; recommendationCount: number; }

let todayView: "recommend" | "missing" = "recommend";
let historyCycleKey: string | null = null;
let historyTab: "recommend" | "missing" = "recommend";
/// 推荐区渲染的 Paper 副本（run 命令返回；供卡片交互查用）
let recPapers: Paper[] = [];

const MODEL_NAME = "cowpaper_model";
const DEFAULT_MODEL = "deepseek-v4-flash"; // 已验证可用的模型

let journals: Journal[] = [];
let papers: Paper[] = [];
let libraryPapers: LibraryPaper[] = [];
let libraryCollections: LibraryCollection[] = [];
let libraryTags: LibraryTag[] = [];
let libraryView: "all" | "recent" | "unfiled" = "all";
let selectedLibraryPaperId: number | null = null;
let libraryCapturePaperId: number | null = null;
let libraryScope: { kind: "collection" | "tag"; id: number } | null = null;
const libraryPaperIds = new Set<number>();
let activeWorkspace: "discovery" | "library" = "discovery";
let aiStatus: AiStatus = emptyAiStatus();
let activity: ActivityState = emptyActivity();
let settings: Settings | null = null;
let abstractLang: "zh" | "en" = "zh";
let libraryInspectorAbstractLang: "zh" | "en" = "zh";
let libraryInspectorCollapsed = false;
let libraryPdfBusyPaperId: number | null = null;
let libraryPdfImportBusy = false;
let libraryDropTargetPaperId: number | null = null;
let libraryDropActive = false;
let libraryDropQueue: LibraryDropItem[] = [];
let libraryInlineCreate: { kind: "collection" | "tag"; parentId: number | null } | null = null;
let libraryColumnWidths: Record<LibraryColumn, number>;
let libraryInspectorWidth: number;
let libraryColumnResize: { column: LibraryColumn; next: LibraryColumn; startX: number; startWidth: number; nextWidth: number } | null = null;
let libraryPointerDrag: { paperId: number; startX: number; startY: number; active: boolean } | null = null;
let librarySuppressNextClick = false;
const expandedLibraryAttachmentPaperIds = new Set<number>();
let libraryToastTimer = 0;
let libraryInspectorResize: { startX: number; startWidth: number } | null = null;
let currentAppVersion = "0.1.4";
let pendingUpdate: Update | null = null;
let updateBusy = false;
/// 纯卡片 UI 状态必须按实例隔离；favorite/ignore 等持久业务状态仍按 paper id。
const expandedCardInstanceIds = new Set<string>();
const cardLanguageState = new Map<string, "zh" | "en">();
const cardPaperState = new Map<string, Paper>();

async function finishRecoveryBatch(batchId: number) {
  const [batch, items] = await invoke<[AbstractRecoveryBatch, AbstractRecoveryItem[]]>("get_abstract_recovery_batch", { id: batchId });
  await loadPapers(); await refreshWorkState();
  const summary = `补回 ${batch.recovered} 篇 · 未找到 ${batch.notFound} 篇 · 来源失败 ${batch.failed} 篇`;
  const recoveredIds = items.filter((i) => i.outcome === "recovered").map((i) => i.paperId);
  const analyze = recoveredIds.length > 0 && await showConfirmModal({ title: "摘要补全完成", message: summary, confirmText: `分析本次补回的 ${recoveredIds.length} 篇`, cancelText: "完成" });
  if (analyze) await startAnalyze(recoveredIds, "manual");
  setStatus(summary, batch.failed ? "error" : "done");
}

function emptyActivity(): ActivityState {
  return {
    syncBatch: null, analysisBatch: null, lastSync: null, lastAnalysis: null, retryWaiting: false,
    pendingAnalysis: 0, analysisFailed: 0, waitingForAbstract: 0,
  };
}

function emptyAiStatus(): AiStatus {
  return {
    state: "idle", batchSize: 0, completed: 0, success: 0, failed: 0, skipped: 0, remaining: 0,
    currentPaperId: null, currentPaperTitle: null, batchStartedAt: null, lastProgressAt: null,
    currentPaperStartedAt: null, retryWaiting: false, retryUntil: null, lastError: null,
    elapsedSeconds: 0, etaSeconds: null, lastRun: null,
  };
}

function $(id: string): HTMLElement {
  return document.getElementById(id)!;
}

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

function fmtDate(s: string | null): string {
  if (!s) return "—";
  return s.slice(0, 10);
}

function fmtDur(sec: number): string {
  if (sec < 60) return `${sec} 秒`;
  if (sec < 3600) return `${Math.round(sec / 60)} 分钟`;
  return `${(sec / 3600).toFixed(1)} 小时`;
}

function authorText(authors: Author[]): string {
  const parts = authors
    .map((a) => a.name || [a.given, a.family].filter(Boolean).join(" "))
    .filter(Boolean);
  if (parts.length > 3) return parts.slice(0, 3).join(", ") + " et al.";
  return parts.length ? parts.join(", ") : "—";
}

function statusLabel(s: string): string {
  switch (s) {
    case "waitingForAbstract": return "等待摘要";
    case "pendingAnalysis": return "待分析";
    case "queued": return "排队中";
    case "analyzing": return "正在分析";
    case "analysisSucceeded": return "已分析";
    case "analysisFailed": return "AI 分析失败";
    default: return s;
  }
}

function setStatus(text: string, cls: "idle" | "running" | "error" | "done") {
  const el = $("status");
  el.textContent = text;
  el.className = `status ${cls}`;
  window.clearTimeout(libraryToastTimer);
  if (cls !== "idle" && cls !== "running") libraryToastTimer = window.setTimeout(() => el.classList.add("dismissed"), 5000);
}

const LIBRARY_COLUMNS = ["title", "note", "source", "year", "authors"] as const;
type LibraryColumn = typeof LIBRARY_COLUMNS[number];
const LIBRARY_COLUMN_MIN: Record<LibraryColumn, number> = { title: 100, note: 50, source: 80, year: 48, authors: 70 };
const LIBRARY_COLUMN_DEFAULT: Record<LibraryColumn, number> = { title: 220, note: 120, source: 150, year: 68, authors: 140 };
const LIBRARY_COLUMN_STORAGE_KEY = "cowpaper.library.columns.v2";
const LIBRARY_INSPECTOR_STORAGE_KEY = "cowpaper.library.inspector-width.v1";
const LIBRARY_COLUMN_ORDER_DEFAULT: LibraryColumn[] = [...LIBRARY_COLUMNS];
let libraryColumnOrder: LibraryColumn[] = [...LIBRARY_COLUMN_ORDER_DEFAULT];
let libraryHiddenColumns = new Set<LibraryColumn>();

function loadLibraryColumnWidths(): Record<LibraryColumn, number> {
  try {
    const raw = JSON.parse(localStorage.getItem(LIBRARY_COLUMN_STORAGE_KEY) || "null") as (Partial<Record<LibraryColumn, number>> & { widths?: Partial<Record<LibraryColumn, number>>; order?: string[]; hidden?: string[] }) | null;
    const widths = raw?.widths || raw;
    const storedOrder = Array.isArray(raw?.order) ? raw.order : [];
    libraryColumnOrder = [...storedOrder.filter((column): column is LibraryColumn => LIBRARY_COLUMNS.includes(column as LibraryColumn)), ...LIBRARY_COLUMNS.filter((column) => !storedOrder.includes(column))];
    libraryHiddenColumns = new Set((raw?.hidden || []).filter((column): column is LibraryColumn => LIBRARY_COLUMNS.includes(column as LibraryColumn)));
    return Object.fromEntries(LIBRARY_COLUMNS.map((column) => {
      const value = widths?.[column];
      return [column, typeof value === "number" && Number.isFinite(value) ? Math.max(LIBRARY_COLUMN_MIN[column], Math.round(value)) : LIBRARY_COLUMN_DEFAULT[column]];
    })) as Record<LibraryColumn, number>;
  } catch {
    return { ...LIBRARY_COLUMN_DEFAULT };
  }
}

function loadLibraryInspectorWidth(): number {
  const value = Number(localStorage.getItem(LIBRARY_INSPECTOR_STORAGE_KEY));
  return Number.isFinite(value) && value >= 300 && value <= 560 ? Math.round(value) : 300;
}

libraryColumnWidths = loadLibraryColumnWidths();
libraryInspectorWidth = loadLibraryInspectorWidth();

function persistLibraryLayout(): void {
  localStorage.setItem(LIBRARY_COLUMN_STORAGE_KEY, JSON.stringify({ widths: libraryColumnWidths, order: libraryColumnOrder, hidden: [...libraryHiddenColumns] }));
  localStorage.setItem(LIBRARY_INSPECTOR_STORAGE_KEY, String(libraryInspectorWidth));
}

function libraryVisibleColumns(): LibraryColumn[] {
  return libraryColumnOrder.filter((column) => !libraryHiddenColumns.has(column));
}

function libraryColumnTemplate(): string {
  return libraryVisibleColumns().map((column) => column === "year" ? `${Math.min(100, Math.max(48, libraryColumnWidths.year))}px` : `minmax(0, ${libraryColumnWidths[column]}fr)`).join(" ") || "minmax(0, 1fr)";
}

function applyLibraryLayoutMetrics(): void {
  const pane = $("library-list-pane");
  const layout = $("library-layout");
  if (pane) pane.style.setProperty("--library-columns", libraryColumnTemplate());
  if (layout) layout.style.setProperty("--inspector-width", `${libraryInspectorWidth}px`);
  if (pane) for (const column of LIBRARY_COLUMNS) pane.style.setProperty(`--column-${column}`, `${libraryColumnWidths[column]}fr`);
}

const LIBRARY_COLUMN_LABELS: Record<LibraryColumn, string> = { title: "标题", note: "备注", source: "期刊", year: "年份", authors: "作者" };

function renderLibraryTableHeader(): void {
  const head = $("library-table-head");
  if (!head) return;
  head.innerHTML = libraryVisibleColumns().map((column) => `<span class="library-column-header" draggable="true" data-column="${column}" data-column-drag="${column}" title="拖动以重排列"><span>${LIBRARY_COLUMN_LABELS[column]}</span><span class="column-resizer" data-column-resize="${column}" aria-hidden="true"></span></span>`).join("");
}

function renderLibraryColumnMenu(): void {
  const menu = $("library-column-menu");
  if (!menu) return;
  const visibleCount = libraryVisibleColumns().length;
  menu.innerHTML = `<div class="library-column-menu-title">显示列 · 拖动表头可重排</div>${libraryColumnOrder.map((column) => `<label class="library-column-option"><input type="checkbox" data-action="library-toggle-column" data-column="${column}" ${libraryHiddenColumns.has(column) ? "" : "checked"} ${column === "title" || (visibleCount === 1 && !libraryHiddenColumns.has(column)) ? "disabled" : ""}/> <span>${LIBRARY_COLUMN_LABELS[column]}</span></label>`).join("")}<button type="button" class="library-column-reset" data-action="library-reset-columns">恢复默认列</button>`;
}

function toggleLibraryColumnMenu(): void {
  const menu = $("library-column-menu");
  menu.classList.toggle("hidden");
  if (!menu.classList.contains("hidden")) renderLibraryColumnMenu();
}

function resetLibraryColumns(): void {
  libraryColumnWidths = { ...LIBRARY_COLUMN_DEFAULT };
  libraryColumnOrder = [...LIBRARY_COLUMN_ORDER_DEFAULT];
  libraryHiddenColumns.clear();
  persistLibraryLayout();
  renderLibrary();
}

function renderLibraryDropState(): void {
  const pane = $("library-list-pane");
  const overlay = $("library-drop-overlay");
  const label = $("library-drop-label");
  if (!pane || !overlay || !label) return;
  pane.classList.toggle("drop-active", libraryDropActive);
  pane.classList.toggle("drop-existing", libraryDropActive && libraryDropTargetPaperId != null);
  document.querySelectorAll(".library-paper-row").forEach((row) => {
    row.classList.toggle("drop-target", libraryDropActive && Number((row as HTMLElement).dataset.paperId) === libraryDropTargetPaperId);
  });
  if (libraryDropActive) {
    label.textContent = libraryDropTargetPaperId == null ? "导入为新文献" : "添加 PDF 到此文献";
    overlay.classList.remove("hidden");
  } else {
    overlay.classList.add("hidden");
  }
  renderLibraryDropQueue();
}

function renderLibraryDropQueue(): void {
  const queue = $("library-drop-queue");
  if (!queue) return;
  if (!libraryDropQueue.length) {
    queue.classList.add("hidden");
    queue.innerHTML = "";
    return;
  }
  const completed = libraryDropQueue.filter((item) => item.state === "done").length;
  const active = libraryDropQueue.find((item) => item.state === "processing");
  queue.classList.remove("hidden");
  queue.innerHTML = `<div class="drop-queue-head"><strong>PDF 队列</strong><span>${completed}/${libraryDropQueue.length}</span></div><div class="drop-queue-list">${libraryDropQueue.map((item) => `<div class="drop-queue-item ${item.state}"><span class="drop-queue-icon" aria-hidden="true">${item.state === "done" ? "✓" : item.state === "error" ? "!" : item.state === "processing" ? "…" : "•"}</span><span title="${escapeHtml(item.name)}">${escapeHtml(item.name)}</span><span class="muted small">${escapeHtml(item.message || (item.state === "processing" ? "处理中" : item.state === "done" ? "完成" : item.state === "error" ? "失败" : "排队中"))}</span></div>`).join("")}</div>${active ? `<div class="drop-queue-progress" role="progressbar" aria-label="PDF 处理进度" aria-valuenow="${completed}" aria-valuemin="0" aria-valuemax="${libraryDropQueue.length}"><span style="width:${Math.round((completed / libraryDropQueue.length) * 100)}%"></span></div>` : ""}`;
}

function getModel(): string {
  return localStorage.getItem(MODEL_NAME) || DEFAULT_MODEL;
}

// ---------- API Key（本地 secret 文件，前端不长期保存真实 Key） ----------

async function hasKey(): Promise<boolean> {
  try {
    return await invoke<boolean>("has_api_key");
  } catch {
    return false;
  }
}

/// Key 状态（本地 secret 文件，经 Rust 命令；前端无法读取完整 Key）。
async function refreshKeyStatus() {
  const el = $("key-status");
  if (!el) return;
  const has = await hasKey();
  el.textContent = has ? "✓ 已保存在本机" : "尚未配置 API Key";
  el.className = has ? "ok small" : "muted small";
}

// ---------- 加载 ----------

async function loadJournals() {
  journals = await invoke<Journal[]>("list_journals");
  renderJournals();
  renderFilter();
}

async function loadPapers() {
  papers = await invoke<Paper[]>("list_papers", { journalId: null });
  renderPapers();
  renderRecommend();
  renderFavorites();
}

async function loadLibraryData(view: "all" | "recent" | "unfiled" = libraryView) {
  libraryView = view;
  try {
    [libraryPapers, libraryCollections, libraryTags] = await Promise.all([
      invoke<LibraryPaper[]>("list_library_papers", { view }),
      invoke<LibraryCollection[]>("list_library_collections"),
      invoke<LibraryTag[]>("list_library_tags"),
    ]);
    libraryPaperIds.clear();
    // The all view is also the cheap membership index used by Discovery cards.
    if (view !== "all") {
      const all = await invoke<LibraryPaper[]>("list_library_papers", { view: "all" });
      all.forEach((item) => libraryPaperIds.add(item.paper.id));
    } else {
      libraryPapers.forEach((item) => libraryPaperIds.add(item.paper.id));
    }
    renderLibraryNavigation();
    renderLibrary();
    renderRecommend();
    renderFavorites();
  } catch (err) {
    console.error("loadLibraryData 失败:", err);
    libraryPapers = [];
    renderLibrary();
  }
}

async function loadAiStatus() {
  try {
    aiStatus = await invoke<AiStatus>("get_ai_status");
  } catch {
    aiStatus = emptyAiStatus();
  }
}

const DEFAULT_PDF_NAMING_TEMPLATE = "{title} - {journal} - {first_author} - {year}.pdf";
const PDF_TEMPLATE_TOKENS = ["title", "journal", "source", "first_author", "authors", "year", "doi"] as const;
const PDF_TEMPLATE_EXAMPLE: Record<typeof PDF_TEMPLATE_TOKENS[number], string> = {
  title: "Minds and machines",
  journal: "Research Policy",
  source: "Research Policy",
  first_author: "Mattia Pedota",
  authors: "Mattia Pedota - John Smith",
  year: "2026",
  doi: "10.1016/j.respol.2026.105600",
};

function renderPdfTemplateExample(): string {
  const templateInput = $("set-pdf-naming-template") as HTMLInputElement | null;
  const modeInput = $("set-pdf-file-handling-mode") as HTMLSelectElement | null;
  const rootInput = $("set-pdf-library-root") as HTMLInputElement | null;
  const folderInput = $("set-pdf-subfolder-rule") as HTMLSelectElement | null;
  if (!templateInput || !modeInput || !rootInput || !folderInput) return "";
  const template = templateInput.value.trim() || DEFAULT_PDF_NAMING_TEMPLATE;
  const unknownTokens = [...template.matchAll(/\{([^{}]+)\}/g)].map((match) => match[1]).filter((token) => !(PDF_TEMPLATE_TOKENS as readonly string[]).includes(token));
  const filename = template.replace(/\{([^{}]+)\}/g, (_match, token: string) => (PDF_TEMPLATE_EXAMPLE as Record<string, string>)[token] ?? "");
  const folder = folderInput.value === "year" ? PDF_TEMPLATE_EXAMPLE.year : folderInput.value === "journal/source" ? `${PDF_TEMPLATE_EXAMPLE.journal}/${PDF_TEMPLATE_EXAMPLE.source}` : "";
  const root = rootInput.value.trim() || "Library root";
  const previewPath = [root, folder, filename || "document.pdf"].filter(Boolean).join("/");
  const preview = $("pdf-template-preview");
  if (preview) preview.textContent = modeInput.value === "none" ? `链接模式 · ${filename || "document.pdf"}` : previewPath;
  const warning = $("pdf-template-warning");
  if (warning) {
    warning.textContent = unknownTokens.length ? `未知 token 将留空：${unknownTokens.map((token) => `{${token}}`).join(", ")}` : "";
    warning.classList.toggle("hidden", unknownTokens.length === 0);
  }
  const moveWarning = $("pdf-move-warning");
  if (moveWarning) moveWarning.classList.toggle("hidden", modeInput.value !== "move");
  return previewPath;
}

async function selectPdfLibraryRoot(): Promise<void> {
  try {
    const selected = await openFileDialog({ directory: true, multiple: false });
    const path = Array.isArray(selected) ? selected[0] : selected;
    if (path) {
      ($("set-pdf-library-root") as HTMLInputElement).value = path;
      renderPdfTemplateExample();
    }
  } catch (error) {
    setStatus(`选择 PDF 文件库目录失败：${String(error)}`, "error");
  }
}

function resetPdfLibraryRoot(): void {
  ($("set-pdf-library-root") as HTMLInputElement).value = "";
  renderPdfTemplateExample();
}

function insertPdfTemplateToken(token: string): void {
  const input = $("set-pdf-naming-template") as HTMLInputElement;
  const insertion = `{${token}}`;
  const start = input.selectionStart ?? input.value.length;
  const end = input.selectionEnd ?? start;
  input.value = `${input.value.slice(0, start)}${insertion}${input.value.slice(end)}`;
  input.focus();
  input.setSelectionRange(start + insertion.length, start + insertion.length);
  renderPdfTemplateExample();
}

async function loadSettings() {
  try {
    settings = await invoke<Settings>("get_settings");
  } catch {
    settings = null;
  }
  if (settings) {
    abstractLang = settings.defaultAbstractLang === "en" ? "en" : "zh";
    ($("set-auto-analyze") as HTMLInputElement).checked = settings.autoAnalyzeNew;
    ($("set-startup-sync") as HTMLInputElement).checked = settings.startupAutoSync;
    ($("set-daily-sync") as HTMLInputElement).checked = settings.dailyAutoSync;
    ($("set-daily-time") as HTMLInputElement).value = settings.dailySyncTime;
    ($("set-abstract-lang") as HTMLSelectElement).value = abstractLang;
    ($("set-pdf-file-handling-mode") as HTMLSelectElement).value = settings.pdfFileHandlingMode;
    ($("set-pdf-library-root") as HTMLInputElement).value = settings.pdfLibraryRoot;
    ($("set-pdf-naming-template") as HTMLInputElement).value = settings.pdfNamingTemplate;
    ($("set-pdf-subfolder-rule") as HTMLSelectElement).value = settings.pdfSubfolderRule;
  }
  const templateInput = $("set-pdf-naming-template") as HTMLInputElement;
  if (!templateInput.value.trim()) templateInput.value = DEFAULT_PDF_NAMING_TEMPLATE;
  renderPdfTemplateExample();
}

function setUpdateStatus(text: string, cls: "muted small" | "ok small" | "error") {
  const el = $("update-status");
  el.textContent = text;
  el.className = cls;
}

function setUpdateButtonState() {
  const button = $("btn-check-update") as HTMLButtonElement;
  const install = $("btn-install-update");
  button.disabled = updateBusy;
  if (pendingUpdate && !updateBusy) install.classList.remove("hidden");
  else install.classList.add("hidden");
}

async function loadCurrentVersion() {
  try {
    currentAppVersion = await getVersion();
  } catch {
    // Vite/browser preview has no Tauri runtime; keep the manifest fallback.
  }
  $("current-version").textContent = currentAppVersion;
}

async function checkForUpdates() {
  if (updateBusy) return;
  updateBusy = true;
  await pendingUpdate?.close().catch(() => {});
  pendingUpdate = null;
  setUpdateButtonState();
  $("latest-version").textContent = "检查中…";
  $("update-notes").classList.add("hidden");
  setUpdateStatus("正在检查更新…", "muted small");
  try {
    const update = await check({ timeout: 15_000 });
    pendingUpdate = update;
    if (!update) {
      $("latest-version").textContent = currentAppVersion;
      setUpdateStatus("已是最新版本。", "ok small");
      return;
    }
    $("latest-version").textContent = update.version;
    const notes = [update.body, update.date ? `发布日期：${update.date.slice(0, 10)}` : ""]
      .filter(Boolean).join("\n");
    const notesEl = $("update-notes");
    notesEl.textContent = notes;
    notesEl.classList.toggle("hidden", !notes);
    setUpdateStatus("发现新版本，请确认后下载并安装。", "ok small");
  } catch (error) {
    $("latest-version").textContent = "检查失败";
    setUpdateStatus(`更新检查失败，CowPaper 仍可正常使用：${String(error)}`, "error");
  } finally {
    updateBusy = false;
    setUpdateButtonState();
  }
}

async function installPendingUpdate() {
  if (!pendingUpdate || updateBusy) return;
  const update = pendingUpdate;
  const confirmed = await showConfirmModal({
    title: `安装 CowPaper ${update.version}？`,
    message: "更新只替换应用 bundle/installer，不删除本机数据库、设置、API Key 或 Library 数据。签名验证失败时安装会被拒绝。",
    confirmText: "下载并安装",
    cancelText: "稍后",
  });
  if (!confirmed) return;
  updateBusy = true;
  setUpdateButtonState();
  setUpdateStatus("正在下载并验证签名…", "muted small");
  try {
    let downloaded = 0;
    await update.downloadAndInstall((event) => {
      if (event.event === "Started") {
        downloaded = 0;
        setUpdateStatus("开始下载更新…", "muted small");
      } else if (event.event === "Progress") {
        downloaded += event.data.chunkLength;
        setUpdateStatus(`正在下载更新… ${Math.round(downloaded / 1024 / 1024)} MB`, "muted small");
      } else {
        setUpdateStatus("下载完成，正在重启…", "ok small");
      }
    }, { restartAfterInstall: false });
    await relaunch();
  } catch (error) {
    // The plugin rejects invalid signatures and failed downloads. Keep the app
    // usable and allow a later manual retry.
    setUpdateStatus(`更新未安装，CowPaper 仍可正常使用：${String(error)}`, "error");
    updateBusy = false;
    setUpdateButtonState();
  }
}

// ---------- 渲染 ----------

let catalogCollections: JournalCollection[] = [];
let catalogDetail: Journal[] = [];
let selectedCatalogCode: string | null = null;
const catalogChecked = new Set<number>(); // 选中的 journal_id

let journalTab: "catalog" | "manual" = "catalog";
let addMemberState: { collectionId: number; code: string; query: string; checked: Set<number>; candidates: Journal[] } | null = null;

/// 打开"添加期刊到集合"内嵌面板（搜索所有已知 journals，checkbox 多选）。
async function openAddMemberPanel(collectionId: number, code: string) {
  const [all, members] = await Promise.all([
    invoke<Journal[]>("list_journals"),
    invoke<Journal[]>("get_collection_journals", { code }),
  ]);
  const memberIds = new Set(members.map((m) => m.id));
  addMemberState = {
    collectionId,
    code,
    query: "",
    checked: new Set(),
    candidates: all.filter((j) => !memberIds.has(j.id)),
  };
  renderAddMemberPanel();
}

function renderAddMemberPanel() {
  const panel = $("add-member-panel");
  if (!addMemberState) {
    panel.classList.add("hidden");
    panel.innerHTML = "";
    return;
  }
  panel.classList.remove("hidden");
  const q = addMemberState.query.toLowerCase();
  const shown = addMemberState.candidates.filter((j) => {
    if (!q) return true;
    const print = j.identifiers.find((i) => i.identifierType === "print")?.value ?? j.printIssn ?? "";
    const online = j.identifiers.find((i) => i.identifierType === "online")?.value ?? j.onlineIssn ?? "";
    return (j.name + " " + print + " " + online).toLowerCase().includes(q);
  });
  const rows = shown
    .map(
      (j) => `
        <li class="card catalog-journal">
          <label class="check grow">
            <input type="checkbox" data-journal-id="${j.id}" ${addMemberState!.checked.has(j.id) ? "checked" : ""} />
            <span><span class="title">${escapeHtml(j.name)}</span></span>
          </label>
        </li>`,
    )
    .join("");
  panel.innerHTML = `
    <div class="catalog-detail-head"><span class="title">添加期刊到集合</span></div>
    <input id="add-member-search" class="modal-input" type="text" placeholder="搜索期刊名 / ISSN" value="${escapeHtml(addMemberState.query)}" />
    <ul class="list">${rows || '<li class="empty">没有可添加的期刊</li>'}</ul>
    <div class="catalog-actions">
      <span id="add-member-count" class="muted small">已选择 ${addMemberState.checked.size} 本</span>
      <button class="ghost small" data-action="add-member-close">取消</button>
      <button class="primary" data-action="add-member-submit" ${addMemberState.checked.size === 0 ? "disabled" : ""}>添加 ${addMemberState.checked.size} 本</button>
    </div>`;
  const search = $("add-member-search") as HTMLInputElement;
  search.addEventListener("input", () => {
    if (addMemberState) addMemberState.query = search.value;
    renderAddMemberPanel();
  });
}

function closeAddMemberPanel() {
  addMemberState = null;
  renderAddMemberPanel();
}

/// 期刊订阅页两 tab 严格互斥（catalog ↔ manual），统一入口，不直接操纵 display。
function setJournalTab(tab: "catalog" | "manual") {
  journalTab = tab;
  const catalogActive = tab === "catalog";
  $("tab-common").classList.toggle("active", catalogActive);
  $("tab-manual").classList.toggle("active", !catalogActive);
  $("catalog-view").classList.toggle("hidden", !catalogActive);
  $("manual-view").classList.toggle("hidden", catalogActive);
  if (catalogActive) renderCatalogCollections();
}

/// 常用期刊页：集合列表（DB 视角 = built-in + 用户集合）。
async function renderCatalogCollections() {
  const box = $("catalog-collections");
  try {
    catalogCollections = await invoke<JournalCollection[]>("list_collections");
  } catch {
    box.innerHTML = '<div class="empty">期刊集合加载失败</div>';
    return;
  }
  const seg = catalogCollections
    .map(
      (c) => `
        <button class="catalog-seg ${c.code === selectedCatalogCode ? "selected" : ""}" data-catalog-code="${escapeHtml(c.code)}">
          ${escapeHtml(c.name)} · ${c.memberCount}
        </button>`,
    )
    .join("");
  const sel = catalogCollections.find((c) => c.code === selectedCatalogCode);
  box.innerHTML = `
    <div class="catalog-seg-row">
      ${seg}
      <button class="ghost small" data-action="create-collection">+ 新建集合</button>
    </div>
    <div class="catalog-seg-meta muted small">${
      sel
        ? `${escapeHtml(sel.name)}${sel.version && sel.version !== "current" ? " · " + escapeHtml(sel.version) : ""}${sel.effectiveFrom ? " · 更新 " + escapeHtml(sel.effectiveFrom) : ""} · ${escapeHtml(sel.sourceName || "")}`
        : "选择一个期刊集合查看期刊"
    }</div>
    ${
      sel && !isBuiltinCollection(sel.code)
        ? `<div class="catalog-manage">
            <button class="ghost small" data-action="rename-collection" data-collection-id="${sel.id}">重命名</button>
            <button class="ghost small danger" data-action="delete-collection" data-collection-id="${sel.id}">删除集合</button>
            <button class="ghost small" data-action="add-member-open" data-collection-id="${sel.id}" data-collection-code="${escapeHtml(sel.code)}">添加期刊到集合</button>
          </div>`
        : ""
    }`;
}

/// 渲染选中集合的期刊列表（受搜索框 / 仅显示未订阅过滤；不重新 invoke）。
function renderCatalogRows() {
  const box = $("catalog-detail");
  if (!selectedCatalogCode) return;
  const q = ($("catalog-search") as HTMLInputElement).value.trim().toLowerCase();
  const unsubOnly = ($("catalog-unsub-only") as HTMLInputElement).checked;
  const isUser = selectedCatalogCode != null && !isBuiltinCollection(selectedCatalogCode);
  const coll = catalogCollections.find((c) => c.code === selectedCatalogCode);
  const filtered = catalogDetail.filter((j) => {
    if (unsubOnly && j.enabled) return false;
    if (q) {
      const print = j.identifiers.find((i) => i.identifierType === "print")?.value ?? j.printIssn ?? "";
      const online = j.identifiers.find((i) => i.identifierType === "online")?.value ?? j.onlineIssn ?? "";
      const hay = (j.name + " " + print + " " + online).toLowerCase();
      if (!hay.includes(q)) return false;
    }
    return true;
  });
  const rows = filtered
    .map((j) => {
      const subscribed = j.enabled;
      const disabled = subscribed;
      const checked = subscribed ? "checked disabled" : "";
      const badge = j.collections.map((c) => `<span class="coll-badge">${escapeHtml(c)}</span>`).join("");
      const review = j.metadataNeedsReview ? '<span class="muted small">需复核</span>' : "";
      const print = j.identifiers.find((i) => i.identifierType === "print")?.value ?? j.printIssn ?? "";
      const online = j.identifiers.find((i) => i.identifierType === "online")?.value ?? j.onlineIssn ?? "";
      const removeBtn = isUser
        ? `<button class="ghost small" data-action="remove-collection-member" data-collection-id="${coll?.id ?? ""}" data-journal-id="${j.id}">移出集合</button>`
        : "";
      return `
        <li class="card catalog-journal">
          <label class="check grow">
            <input type="checkbox" data-journal-id="${j.id}" ${checked} ${disabled} />
            <span>
              <span class="title">${escapeHtml(j.name)} ${badge} ${review}</span>
              <span class="muted small">${print ? "Print " + escapeHtml(print) : ""}${online ? " · Online " + escapeHtml(online) : ""}</span>
              ${subscribed ? '<span class="chip ok-chip">已订阅</span>' : ""}
            </span>
          </label>
          ${removeBtn}
        </li>`;
    })
    .join("");
  box.innerHTML = `
    <div class="catalog-tools">
      <button class="ghost small" data-action="catalog-select-unsub">全选未订阅</button>
    </div>
    <ul class="list">${rows || (filtered.length === 0 && (q || unsubOnly) ? '<li class="empty">没有符合条件的期刊</li>' : '<li class="empty">无期刊</li>')}</ul>
    <div class="catalog-actions"><span id="catalog-selected" class="muted small">已选择 0 本</span>
      <button class="ghost small" data-action="catalog-clear">清除</button>
      <button id="catalog-subscribe" class="primary" data-action="catalog-subscribe">订阅 0 本</button>
    </div>`;
  updateCatalogSelected();
}

/// 集合详情：期刊 checkbox 列表 + 批量操作。
async function renderCatalogDetail(code: string) {
  selectedCatalogCode = code;
  catalogChecked.clear();
  $("catalog-detail").classList.remove("hidden");
  try {
    // DB 视角（built-in 与用户集合统一）：包含 catalog 导入期刊与手动添加期刊
    catalogDetail = await invoke<Journal[]>("get_collection_journals", { code });
  } catch {
    $("catalog-detail").innerHTML = '<div class="empty">期刊列表加载失败</div>';
    return;
  }
  renderCatalogCollections(); // 刷新集合选中态与管理按钮
  renderCatalogRows();
}

function updateCatalogSelected() {
  const n = catalogChecked.size;
  $("catalog-selected").textContent = `已选择 ${n} 本`;
  ($("catalog-subscribe") as HTMLButtonElement).textContent = `订阅 ${n} 本`;
  ($("catalog-subscribe") as HTMLButtonElement).disabled = n === 0;
}

/// 批量添加（只做订阅记录，不同步）；结果摘要 + 询问是否同步。
async function doCatalogSubscribe() {
  const ids = [...catalogChecked];
  if (ids.length === 0) return;
  let res: BulkSubscribeResult;
  try {
    res = await invoke<BulkSubscribeResult>("subscribe_journals", { ids });
  } catch (err) {
    setStatus("批量添加失败", "error");
    console.error(err);
    return;
  }
  setStatus(`已添加 ${res.subscribed} 本期刊（已订阅 ${res.already} · 失败 ${res.failed}）`, "done");
  await loadJournals();
  catalogChecked.clear();
  await renderCatalogDetail(selectedCatalogCode!);
  if (res.subscribed > 0) {
    const goSync = await showConfirmModal({
      title: "批量添加完成",
      message: `已添加 ${res.subscribed} 本期刊。\n是否现在检查新论文？`,
      confirmText: "开始同步",
      cancelText: "稍后",
    });
    if (goSync) {
      const idsToSync = catalogDetail.map((j) => j.id);
      await startSync(idsToSync.length ? idsToSync : null);
    }
  }
}

function renderJournals() {
  const ul = $("journal-list");
  ul.innerHTML = "";
  if (journals.length === 0) {
    ul.innerHTML = '<li class="empty">暂无订阅，请在上方添加期刊</li>';
    return;
  }
  const q = ($("journal-search") as HTMLInputElement).value.trim().toLowerCase();
  const shown = q
    ? journals.filter(
        (j) =>
          j.name.toLowerCase().includes(q) ||
          (j.printIssn || "").toLowerCase().includes(q) ||
          (j.onlineIssn || "").toLowerCase().includes(q),
      )
    : journals;
  if (shown.length === 0) {
    ul.innerHTML = '<li class="empty">没有符合条件的期刊</li>';
    return;
  }
  for (const j of shown) {
    const rate = j.abstractCoverageRate != null ? Math.round(j.abstractCoverageRate * 100) + "%" : "—";
    // 多 ISSN / ISSN-L 显示：优先 identifiers（canonical），未知则不显示假值
    const print = j.identifiers.find((i) => i.identifierType === "print")?.value ?? j.printIssn ?? "";
    const online = j.identifiers.find((i) => i.identifierType === "online")?.value ?? j.onlineIssn ?? "";
    const other = j.identifiers.filter((i) => i.identifierType === "other").map((i) => i.value);
    const issnLine = [
      print ? `Print: ${print}` : "",
      online ? `Online: ${online}` : "",
      j.issnL ? `ISSN-L: ${j.issnL}` : "",
    ]
      .filter(Boolean)
      .concat(other)
      .join(" · ");
    const badge = j.possibleDuplicate ? '<span class="chip warn-chip">疑似重复</span>' : "";
    const colls = j.collections.length
      ? `<div class="coll-badges">${j.collections.map((c) => `<span class="coll-badge">${escapeHtml(c)}</span>`).join("")}</div>`
      : "";
    const li = document.createElement("li");
    li.className = "card journal";
    li.innerHTML = `
      <div class="row">
        <div class="grow">
          <div class="title">${escapeHtml(j.name)} ${badge}</div>
          <div class="muted small">${escapeHtml(issnLine || "ISSN: 未知")}${j.publisher ? " · " + escapeHtml(j.publisher) : ""}</div>
        </div>
        <button class="ghost small" data-action="sync-one" data-id="${j.id}">同步</button>
        <button class="ghost small" data-action="toggle" data-id="${j.id}">${j.enabled ? "停用" : "启用"}</button>
        <button class="ghost small danger" data-action="delete" data-id="${j.id}">删除</button>
      </div>
      <div class="muted small">${escapeHtml(j.coverageStatus || "未同步")} · 摘要覆盖 ${rate} · 论文 ${j.paperCount} · 最近 ${fmtDate(j.lastPaperDate)}</div>
      ${colls}
    `;
    ul.appendChild(li);
  }
}

function renderFilter() {
  const sel = $("journal-filter") as HTMLSelectElement;
  const cur = sel.value;
  sel.innerHTML =
    '<option value="">全部期刊</option>' +
    journals.map((j) => `<option value="${j.id}" ${String(j.id) === cur ? "selected" : ""}>${escapeHtml(j.name)}</option>`).join("");
}

interface TagDraftItem {
  id: number;
  name: string;
  description: string | null;
  enabled: boolean;
  deleted: boolean;
}
interface TagBaseline {
  items: TagDraftItem[];
  source: string;
  scheduledEffectiveCycleKey: string | null;
}
interface TagConfigDiff {
  added: string[];
  removed: string[];
  disabled: string[];
  enabled: string[];
  semanticChanged: string[];
  unchanged: string[];
}
interface SaveTagConfigResult {
  mode: string;
  effectiveCycleKey: string | null;
  diff: TagConfigDiff;
  aiNeededPapers: number;
}

let tagBaseline: TagDraftItem[] = [];
let tagDraft: TagDraftItem[] = [];
let tagConfigDirty = false;
let tagBaselineSource = "active";
let tagScheduledCycleKey: string | null = null;

function tagDraftEqual(a: TagDraftItem[], b: TagDraftItem[]): boolean {
  if (a.length !== b.length) return false;
  return a.every((x, i) => {
    const y = b[i];
    return x.id === y.id && x.name === y.name && (x.description ?? "") === (y.description ?? "") && x.enabled === y.enabled && x.deleted === y.deleted;
  });
}

function setTagDirty(v: boolean) {
  tagConfigDirty = v;
  $("tag-draft-dirty").classList.toggle("hidden", !v);
  updateTagActionState();
  const n = draftChanges();
  const sum = $("tag-action-summary");
  if (sum) sum.textContent = v ? `${n} 项未保存修改` : "";
}

/// 统一按钮 enable 状态（Round 6.5.2）：
/// saveScheduled = dirty；immediate = dirty || (存在 scheduled 且 scheduled != active)。
function updateTagActionState() {
  const schedBtn = document.querySelector("[data-action='save-tag-config-scheduled']") as HTMLButtonElement | null;
  const immBtn = document.querySelector("[data-action='activate-tag-config-now']") as HTMLButtonElement | null;
  const hasScheduled = tagBaselineSource === "scheduled";
  if (schedBtn) schedBtn.disabled = !tagConfigDirty;
  if (immBtn) immBtn.disabled = !(tagConfigDirty || hasScheduled);
  const status = $("tag-config-status");
  if (status && hasScheduled && !tagConfigDirty) {
    status.textContent = `✓ 已保存 · 等待下个周期生效（将于 ${fmtCycle(tagScheduledCycleKey || "")}）`;
  } else if (status && hasScheduled && tagConfigDirty) {
    status.textContent = `有一组标签设置将在 ${fmtCycle(tagScheduledCycleKey || "")} 生效（当前编辑为新修改）`;
  } else if (status) {
    status.textContent = "";
  }
}

function draftChanges(): number {
  return tagDraft.filter((d, i) => {
    const b = tagBaseline[i];
    return !b || d.name !== b.name || (d.description ?? "") !== (b.description ?? "") || d.enabled !== b.enabled || d.deleted !== b.deleted;
  }).length;
}

async function loadTagEditor() {
  try {
    const base = await invoke<TagBaseline>("get_tag_config_baseline");
    tagBaseline = base.items;
    tagDraft = base.items.map((x) => ({ ...x }));
    tagConfigDirty = false;
    tagBaselineSource = base.source;
    tagScheduledCycleKey = base.scheduledEffectiveCycleKey;
    updateTagActionState();
  } catch (err) {
    $("tag-config-status").textContent = "标签配置加载失败";
    console.error(err);
    tagDraft = [];
  }
  renderTagEditor();
}

/// Tags Draft Editor：只渲染 draft（修改不写 DB）。
function renderTagEditor() {
  const ul = $("tag-list");
  ul.innerHTML = tagDraft
    .map((d, i) => `
      <li class="card tag tag-draft-row">
        <label class="check"><input type="checkbox" data-action="tag-draft-toggle" data-idx="${i}" ${d.enabled ? "checked" : ""} /></label>
        <input class="tag-name-input" type="text" data-action="tag-draft-name" data-idx="${i}" value="${escapeHtml(d.name)}" placeholder="标签名" />
        <input class="tag-desc-input" type="text" data-action="tag-draft-desc" data-idx="${i}" value="${escapeHtml(d.description || "")}" placeholder="说明（作为 AI 评分标准）" />
        <button class="ghost small danger" data-action="tag-draft-delete" data-idx="${i}">删除</button>
      </li>`)
    .join("") || '<li class="empty">暂无标签。点击「+ 新增标签」创建。</li>';
  setTagDirty(!tagDraftEqual(tagDraft, tagBaseline));
}

/// 保存 draft。
/// 纯逻辑（可测试）：计算保存预览（immediate 确认 Modal 用）。
interface TagSavePreview {
  added: number;
  semanticChanged: number;
  removed: number;
  disabled: number;
  needsAi: boolean;
}
function computeTagSavePreview(draft: TagDraftItem[], baseline: TagDraftItem[]): TagSavePreview {
  const added = draft.filter((d) => d.id === 0 && !d.deleted).length;
  const removed = baseline.filter((b) => b.id > 0 && !draft.some((d) => d.id === b.id)).length;
  const disabled = draft.filter((d) => {
    const b = baseline.find((x) => x.id === d.id);
    return b ? b.enabled && !d.enabled : false;
  }).length;
  const norm = (x: string) => x.replace(/\s/g, "").toLowerCase();
  const semanticChanged = draft.filter((d) => {
    if (d.id === 0 || d.deleted) return false;
    const b = baseline.find((x) => x.id === d.id);
    if (!b) return false;
    return norm(b.name) !== norm(d.name) || norm(b.description || "") !== norm(d.description || "");
  }).length;
  return { added, semanticChanged, removed, disabled, needsAi: added + semanticChanged > 0 };
}

function setTagSaveStatus(text: string, cls: "idle" | "running" | "error" | "done") {
  const el = $("tag-action-summary");
  if (!el) return;
  el.textContent = text;
  el.className = `muted small ${cls === "error" ? "error" : cls === "running" ? "running" : ""}`;
}

function safeError(err: unknown): string {
  const m = err instanceof Error ? err.message : String(err);
  return m.length > 160 ? m.slice(0, 160) + "…" : m;
}

/// 保存，下个推荐周期生效：仅持久化 scheduled（需 dirty；不调 AI、不改 active、不重排）。
async function saveTagConfigScheduled() {
  if (!tagConfigDirty) return;
  const items = tagDraft.filter((d) => !(d.deleted && d.id === 0));
  setTagSaveStatus("正在保存…", "running");
  try {
    const res = await invoke<SaveTagConfigResult>("save_tag_config", { items, mode: "scheduled" });
    setTagSaveStatus(`已保存 · 将于 ${fmtCycle(res.effectiveCycleKey || "")} 生效`, "done");
    await loadTagEditor();
  } catch (err) {
    setTagSaveStatus(`保存失败：${safeError(err)}`, "error");
  }
}

/// 纯逻辑（可测试）：immediate candidate 选择。
/// dirty → draft（最新意图）；否则 scheduled（若存在且与 active 不同由调用方判断）；
/// 否则空（无候选）。
function getImmediateCandidateConfig(
  dirty: boolean,
  source: string,
  draft: TagDraftItem[],
  baseline: TagDraftItem[],
): TagDraftItem[] {
  if (dirty) return draft.filter((d) => !(d.deleted && d.id === 0));
  if (source === "scheduled") return baseline.filter((d) => !d.deleted);
  return [];
}

/// 立即更新排序：candidate = 用户最新意图（dirty → draft；否则 scheduled）。
/// 不依赖 dirty；无候选时给出可见提示，绝不 silent return。
async function activateTagConfigNow() {
  setTagSaveStatus("正在准备更新排序…", "running");
  const candidate = getImmediateCandidateConfig(tagConfigDirty, tagBaselineSource, tagDraft, tagBaseline);
  if (candidate.length === 0) {
    setTagSaveStatus("当前没有需要立即生效的标签修改", "idle");
    return;
  }
  let activeItems: TagDraftItem[];
  try {
    activeItems = await invoke<TagDraftItem[]>("get_active_tag_config");
  } catch (err) {
    setTagSaveStatus(`立即更新失败：${safeError(err)}`, "error");
    return;
  }
  const preview = computeTagSavePreview(candidate, activeItems);
  if (preview.needsAi) {
    const ok = await showConfirmModal({
      title: "立即更新标签评分？",
      message: `本次配置需要为已有论文补充标签评分。\n新增标签 ${preview.added} · 说明/名称修改 ${preview.semanticChanged}\n\n只更新相关标签，不会重新生成标题、摘要或总结。`,
      confirmText: "立即更新",
      cancelText: "取消",
    });
    if (!ok) {
      setTagSaveStatus("已取消（修改仍保留）", "idle");
      return; // 不保存、不启动 AI；dirty/scheduled 均保持
    }
  } else {
    setTagSaveStatus("正在重新计算排序…", "running");
  }
  try {
    const res = await invoke<SaveTagConfigResult>("save_tag_config", { items: candidate, mode: "immediate" });
    setTagSaveStatus(
      res.aiNeededPapers > 0 ? "正在更新标签评分…" : "标签设置已生效 · 当前推荐已更新",
      res.aiNeededPapers > 0 ? "running" : "done",
    );
    await loadTagEditor();
    await loadPapers();
    await refreshRecommendations();
  } catch (err) {
    setTagSaveStatus(`立即更新失败：${safeError(err)}`, "error");
  }
}

function tagChips(matches: TagMatch[]): string {
  // 防御性去重：同一逻辑 Tag（按 tag_id，fallback name）最多一个 chip（root cause 在 merge 层已修）
  const seen = new Map<string, TagMatch>();
  for (const m of matches) {
    if (m.score <= 0) continue;
    const key = m.tagId != null ? `id:${m.tagId}` : `name:${m.tag}`;
    const prev = seen.get(key);
    if (!prev || m.score > prev.score) seen.set(key, m);
  }
  const shown = [...seen.values()];
  return shown.map((m) => `<span class="tag-chip">${escapeHtml(m.tag)} ${m.score.toFixed(1)}</span>`).join("");
}

interface RenderPaperOptions {
  withAbstract: boolean;
  /** Stable identity for this rendered card, separate from the paper's business identity. */
  context: string;
  rank?: number;
  scoreSnapshot?: number;
  /// 历史总分覆盖（用 score_snapshot，避免显示当前分造成混淆）
  scoreOverride?: number;
}

/// 统一 Paper Card（今日推荐 / 所有论文 / 收藏 / 历史共用；不维护各自残缺版本）。
function renderPaperCard(p: Paper, opts: RenderPaperOptions): string {
  const cardInstanceId = `${opts.context}:paper:${p.id}`;
  cardPaperState.set(cardInstanceId, p);
  const cls = p.isIgnored ? "card paper ignored" : "card paper";
  const status = p.analysisStatus === "analysisSucceeded" ? "" : `<span class="chip muted-chip">${statusLabel(p.analysisStatus)}</span>`;
  const titleZh = p.chineseTitle ? `<div class="paper-title">${escapeHtml(p.chineseTitle)}</div>` : "";
  const titleEn = p.chineseTitle
    ? `<div class="paper-title-en">${escapeHtml(p.title || "")}</div>`
    : `<div class="paper-title">${escapeHtml(p.title || "（无标题）")}</div>`;
  // 摘要质量提示（Round 5B）：partial → 低强调橙（非 error red）；基于 partial 的 AI 结果低调注明
  const partialNote = p.abstractQuality === "partial" ? `<span class="abs-quality partial">摘要可能不完整${p.abstractSource ? " · " + escapeHtml(p.abstractSource) : ""}</span>` : "";
  const partialAiNote =
    p.abstractQuality === "partial" && p.analysisStatus === "analysisSucceeded"
      ? `<div class="muted small abs-partial-ai">基于不完整摘要分析</div>`
      : "";
  const summary = p.oneSentenceSummary
    ? `<div class="paper-summary">${escapeHtml(p.oneSentenceSummary)}${partialAiNote}</div>`
    : partialAiNote;
  const displayScore = opts.scoreOverride ?? p.totalScore;
  const scoreBadge = displayScore != null ? `<span class="score-badge">总分 ${displayScore.toFixed(1)}</span>` : "";
  // 研究标签评分 + 总分同一 score row（标签在前、总分最后）
  const scoreRow = `<div class="score-row">${tagChips(p.tagMatches)}${scoreBadge}</div>`;
  // 历史快照行：固定用当日 rank / 当日 score_snapshot（当前分不显示，避免混乱）
  const rankLine =
    opts.rank != null && opts.scoreSnapshot != null
      ? `<div class="rec-rank muted small">当日排名 #${opts.rank} · 当日总分 ${opts.scoreSnapshot.toFixed(1)}</div>`
      : "";
  // Collection badge：小、低强调、无 score（与 AI tag 评分视觉分层）
  const collBadges = p.collections.length
    ? `<div class="coll-badges">${p.collections.map((c) => `<span class="coll-badge">${escapeHtml(c)}</span>`).join("")}</div>`
    : "";

  let abstractHtml = "";
  if (opts.withAbstract) {
    const zhAbs = p.chineseAbstract;
    const enAbs = p.abstractText;
    let lang = cardLanguageState.get(cardInstanceId) ?? abstractLang;
    if (lang === "zh" && !zhAbs) lang = "en";
    const text = lang === "zh" ? zhAbs : enAbs;
    const isExpanded = expandedCardInstanceIds.has(cardInstanceId);
    const hasZh = !!zhAbs;
    if (text) {
      const trunc = isExpanded ? text : text.slice(0, 400) + (text.length > 400 ? "…" : "");
      abstractHtml = `
        <div class="abstract-wrap">
          <div class="abstract-langs">
            <button class="abs-lang ${lang === "zh" ? "on" : ""}" data-action="toggle-paper-lang" data-paper-id="${p.id}" data-lang="zh" ${hasZh ? "" : "disabled"}>中文</button>
            <button class="abs-lang ${lang === "en" ? "on" : ""}" data-action="toggle-paper-lang" data-paper-id="${p.id}" data-lang="en">English</button>
          </div>
          ${partialNote}
          <div class="abstract">${escapeHtml(trunc)}</div>
          ${text.length > 400 ? `<button class="ghost small abs-expand" data-action="toggle-paper-abstract" data-paper-id="${p.id}">${isExpanded ? "收起摘要" : "展开摘要"}</button>` : ""}
        </div>`;
    } else if (lang === "zh" && !zhAbs) {
      abstractHtml = `<div class="abstract muted">中文摘要待生成</div>`;
    } else {
      // missing：按摘要语义状态区分产品语义（Round 7 Phase 1）。
      // not_expected（news/editorial/correction/front_matter/...）不提供 recovery 按钮。
      const absStatus = p.abstractStatus || "unknown";
      if (absStatus === "not_expected") {
        abstractHtml = `<div class="abstract muted">该内容类型通常不提供研究摘要</div>`;
      } else {
        // missing_recoverable / unknown：保持原「未找到公开摘要」+ 可重试语义。
        abstractHtml = `<div class="abstract muted">未找到公开摘要</div>${p.analysisStatus === "waitingForAbstract" ? `<div class="muted small">可检查 Crossref · OpenAlex · Publisher</div><button class="ghost small" data-action="recover-paper-abstract" data-paper-id="${p.id}">重新获取摘要</button>` : ""}`;
      }
    }
  }

  return `
    <li class="${cls} paper-card" data-card-instance-id="${escapeHtml(cardInstanceId)}" data-card-context="${escapeHtml(opts.context)}" data-paper-id="${p.id}"${opts.rank != null ? ` data-card-rank="${opts.rank}"` : ""}${opts.scoreSnapshot != null ? ` data-card-score-snapshot="${opts.scoreSnapshot}"` : ""}${opts.scoreOverride != null ? ` data-card-score-override="${opts.scoreOverride}"` : ""}>
      ${status}
      ${titleZh}
      ${titleEn}
      ${rankLine}
      ${summary}
      <div class="paper-meta">${escapeHtml(authorText(p.authors))} · ${escapeHtml(p.journalName || "")} · ${fmtDate(p.publishedDate)}</div>
      ${collBadges}
      ${scoreRow}
      ${abstractHtml}
      <div class="paper-actions">
        ${libraryPaperIds.has(p.id) ? '<button class="ghost small" data-action="open-library">✓ 已收录</button>' : '<button class="ghost small" data-action="add-library" data-paper-id="' + p.id + '">收录</button>'}
        <button class="ghost small" data-action="attach-pdf" data-paper-id="${p.id}"${libraryPdfBusyPaperId === p.id ? " disabled" : ""}>${libraryPdfBusyPaperId === p.id ? "添加中…" : "＋ PDF"}</button>
        ${libraryPaperIds.has(p.id) ? '' : `<button class="ghost small" data-action="toggle-favorite" data-paper-id="${p.id}">${p.isFavorite ? "★ 稍后看" : "☆ 稍后看"}</button>`}
        <button class="ghost small" data-action="ignore" data-id="${p.id}">${p.isIgnored ? "取消忽略" : "忽略"}</button>
        ${p.url ? `<a href="#" class="ghost small" data-action="open" data-url="${escapeHtml(p.url)}">原文 ↗</a>` : ""}
        <span class="muted small detail">${escapeHtml(p.normalizedDoi || "")} · 来源 ${escapeHtml(p.discoverySource || "—")} · ${p.abstractSource ? "摘要 " + escapeHtml(p.abstractSource) : ""}</span>
      </div>
    </li>
  `;
}

/** Rebuild exactly the card that received a local UI interaction. */
function renderCardInstance(card: HTMLElement): string {
  const id = Number(card.dataset.paperId);
  const context = card.dataset.cardContext;
  const cardInstanceId = card.dataset.cardInstanceId;
  const p = (cardInstanceId ? cardPaperState.get(cardInstanceId) : undefined)
    ?? papers.find((x) => x.id === id)
    ?? recPapers.find((x) => x.id === id);
  if (!p || !context) return card.outerHTML;
  const num = (value: string | undefined) => value == null ? undefined : Number(value);
  return renderPaperCard(p, {
    withAbstract: true,
    context,
    rank: num(card.dataset.cardRank),
    scoreSnapshot: num(card.dataset.cardScoreSnapshot),
    scoreOverride: num(card.dataset.cardScoreOverride),
  });
}

function renderPapers() {
  const jsel = $("journal-filter") as HTMLSelectElement;
  const fsel = $("flag-filter") as HTMLSelectElement;
  const asel = $("ai-filter") as HTMLSelectElement;
  const absel = $("abs-filter") as HTMLSelectElement;
  const csel = $("coll-filter") as HTMLSelectElement;
  const jid = jsel.value ? parseInt(jsel.value, 10) : null;
  const flag = fsel.value;
  const aist = asel.value;
  const abst = absel.value;
  const collt = csel.value;

  // Keep recovery reachable from All Papers rather than a hidden filter or
  // Activity-only panel.
  const missingAbstracts = papers.filter((p) => p.abstractQuality !== "complete").length;
  $("abstract-recovery-banner").innerHTML = missingAbstracts > 0
    ? `<div class="pending-row abstract-recovery-banner"><span>缺失摘要 <strong>${missingAbstracts} 篇</strong></span><button class="ghost small" data-action="translate-missing-titles">翻译缺摘要标题</button></div>`
    : "";

  let list = papers;
  if (jid != null) list = list.filter((p) => p.journalId === jid);
  if (flag === "unread") list = list.filter((p) => !p.isRead);
  else if (flag === "favorite") list = list.filter((p) => p.isFavorite);
  else if (flag === "ignored") list = list.filter((p) => p.isIgnored);
  if (aist) list = list.filter((p) => p.analysisStatus === aist);
  if (abst) list = list.filter((p) => p.abstractQuality === abst);
  if (collt) list = list.filter((p) => p.collections.includes(collt));

  $("paper-list").innerHTML = list.length
    ? list.map((p) => renderPaperCard(p, { withAbstract: true, context: "all-papers" })).join("")
    : '<li class="empty">暂无符合条件的论文</li>';
}

/// 推荐数据源（Round 6）：当前推荐 = open run；历史 = finalized run。
/// 前端不得再从所有 papers 自行推导历史推荐。
async function renderRecommend() {
  const list = $("recommend-list");
  const status = $("rec-status");
  try {
    const [view, missing] = await Promise.all([invoke<RecommendationRunView>("get_current_recommendation_run"), invoke<Paper[]>("list_today_missing_papers")]);
    recPapers = view.items.map((i) => i.paper);
    const [, m, d] = view.run.cycleKey.split("-").map(Number);
    const dtime = getDailyCheckTime();
    const now = new Date();
    const nowHm = now.getHours().toString().padStart(2, "0") + ":" + now.getMinutes().toString().padStart(2, "0");
    const nextLabel = nowHm < dtime ? ` · 下一批 ${dtime} 自动更新` : "";
    status.textContent = `今日推荐 · ${m}月${d}日${nextLabel}`;
    $("today-segments").innerHTML = `<button class="seg ${todayView === "recommend" ? "on" : ""}" data-action="today-tab" data-tab="recommend">推荐 ${view.items.length}</button><button class="seg ${todayView === "missing" ? "on" : ""}" data-action="today-tab" data-tab="missing">缺摘要 ${missing.length}</button>`;
    const missingIds = missing.map((p) => p.id).join(",");
    $("today-missing-actions").innerHTML = todayView === "missing" ? `<div class="rec-head"><span class="muted small">今日缺失摘要 ${missing.length} 篇</span>${missing.length ? `<button class="ghost small" data-action="recover-scoped-abstracts" data-paper-ids="${missingIds}" data-recovery-label="今日">重新获取今日摘要</button>` : ""}</div>` : "";
    list.innerHTML = todayView === "recommend"
      ? (view.items.length ? view.items.map((v) => renderPaperCard(v.paper, { withAbstract: true, context: `today:recommend:${view.run.id}` })).join("") : '<li class="empty">今天暂无新的推荐论文。</li>')
      : (missing.length ? missing.map((p) => renderPaperCard(p, { withAbstract: true, context: `today:missing:${view.run.id}` })).join("") : '<li class="empty">今天没有缺失摘要的新增论文。</li>');
  } catch (err) {
    console.error("renderRecommend 失败:", err);
    list.innerHTML = '<li class="empty">暂无推荐。保存 API Key 后点「AI 分析」，或同步新论文后自动分析。</li>';
  }
}

function getDailyCheckTime(): string {
  const el = $("set-daily-time") as HTMLInputElement | null;
  return el?.value || "09:00";
}

function fmtCycle(key: string): string {
  const [y, m, d] = key.split("-").map(Number);
  return `${y}年${m}月${d}日`;
}

/// 统一刷新当前推荐（open run 幂等重算；finalized 冻结）。
async function refreshRecommendations() {
  try {
    await invoke<number>("refresh_current_recommendations");
  } catch (err) {
    console.error("refresh_current_recommendations 失败:", err);
  }
  await renderRecommend();
}

/// Daily Papers and the recommendation snapshot are separate dimensions.
async function renderRecommendHistory() {
  const picker = $("rec-history-picker");
  const list = $("recommend-history-list");
  try {
    if (!historyCycleKey) {
      const days = await invoke<DailyPaperSummary[]>("list_daily_paper_summaries");
      picker.innerHTML = '<div class="rec-head"><span class="title">历史</span></div>';
      list.innerHTML = days.length ? `<div class="history-grid">${days.map((d) => `<button class="history-card" data-action="open-history-day" data-cycle-key="${d.cycleKey}"><div class="title">${fmtCycle(d.cycleKey)}</div><div class="muted small">推荐 ${d.recommendationCount} 篇 · 缺摘要 ${d.missingCount} 篇</div><div class="muted small">›</div></button>`).join("")}</div>` : '<li class="empty">暂无历史收录。</li>';
    } else {
      picker.innerHTML = `
        <div class="rec-head">
          <button class="ghost small" data-action="history-back">‹ 历史</button><span class="title">${fmtCycle(historyCycleKey)}</span>
          <span class="segmented"><button class="seg ${historyTab === "recommend" ? "on" : ""}" data-action="history-tab" data-tab="recommend">推荐</button><button class="seg ${historyTab === "missing" ? "on" : ""}" data-action="history-tab" data-tab="missing">缺摘要</button></span></div>`;
      if (historyTab === "recommend") {
        const view = await invoke<RecommendationRunView | null>("get_daily_recommendation_run", { cycleKey: historyCycleKey });
        list.innerHTML = view?.items.length ? view.items.map((v) => renderPaperCard(v.paper, { withAbstract: true, context: `history:${historyCycleKey}:recommend:${view.run.id}`, rank: v.rank, scoreSnapshot: v.scoreSnapshot, scoreOverride: v.scoreSnapshot })).join("") : '<li class="empty">该日暂无推荐</li>';
      } else {
        const ps = await invoke<Paper[]>("list_daily_papers", { cycleKey: historyCycleKey, missingOnly: historyTab === "missing" });
        const missingIds = ps.map((p) => p.id).join(",");
        list.innerHTML = `${ps.length ? `<div class="rec-head"><span class="muted small">当日缺失摘要 ${ps.length} 篇</span><button class="ghost small" data-action="recover-scoped-abstracts" data-paper-ids="${missingIds}" data-recovery-label="当日">重新获取当日摘要</button></div>${ps.map((p) => renderPaperCard(p, { withAbstract: true, context: `history:${historyCycleKey}:missing` })).join("")}` : '<li class="empty">该日暂无论文</li>'}`;
      }
    }
  } catch (err) {
    console.error("renderRecommendHistory 失败:", err);
    list.innerHTML = '<li class="empty">暂无历史推荐</li>';
  }
}

function showHistoryOverview() {
  historyCycleKey = null;
  renderRecommendHistory();
}

function renderFavorites() {
  const list = papers.filter((p) => p.isFavorite);
  $("favorites-list").innerHTML = list.length
    ? list.map((p) => renderPaperCard(p, { withAbstract: true, context: "favorites" })).join("")
    : '<li class="empty">暂无稍后看的论文。在论文卡片上点「稍后看」。</li>';
}

function libraryEnglishTitle(item: LibraryPaper): string {
  return item.effectiveTitle?.trim() || "（无标题）";
}

function libraryChineseTitle(item: LibraryPaper): string {
  return item.effectiveChineseTitle?.trim() || "";
}

function librarySource(item: LibraryPaper): string {
  return item.effectiveJournal?.trim() || item.effectiveSource?.trim() || "—";
}

function libraryYear(item: LibraryPaper): string {
  return item.effectiveYear == null ? "—" : String(item.effectiveYear);
}

function libraryAuthors(item: LibraryPaper): Author[] {
  return item.effectiveAuthors || [];
}

function libraryAbstract(item: LibraryPaper, language: "zh" | "en"): string {
  const value = language === "zh" ? item.effectiveChineseAbstract : item.effectiveAbstract;
  return value?.trim() || "";
}

function libraryNote(item: LibraryPaper): string {
  return item.note?.trim() || "";
}

function libraryAuthorsFromInput(value: string): Author[] | null {
  const names = value.split(/[,;，；\n]/).map((name) => name.trim()).filter(Boolean);
  return names.length ? names.map((name) => ({ given: null, family: null, name })) : null;
}

function libraryInlineEditButton(paperId: number, field: LibraryInlineField, label: string): string {
  return `<button type="button" class="inline-edit-button" title="编辑 ${escapeHtml(label)}" aria-label="编辑 ${escapeHtml(label)}" data-action="library-inline-edit" data-paper-id="${paperId}" data-field="${field}">✎</button>`;
}

function libraryInspectorRow(label: string, value: string, editButton = "", className = ""): string {
  return `<div class="inspector-form-row ${className}"><span class="field-label">${escapeHtml(label)}</span><span class="field-value" title="${escapeHtml(value.replace(/<[^>]+>/g, ""))}">${value}</span>${editButton}</div>`;
}

function libraryInlineFieldLabel(field: LibraryInlineField): string {
  return ({
    title: "English Title", chineseTitle: "中文标题", source: "期刊", publisher: "出版社",
    publicationDate: "出版日期", volume: "卷", issue: "期", pages: "页码", year: "年份",
    authors: "作者", abstract: "摘要", chineseAbstract: "中文摘要", note: "备注",
  } as Record<LibraryInlineField, string>)[field];
}

function libraryInlineCurrentValue(item: LibraryPaper, field: LibraryInlineField): string {
  switch (field) {
    case "title": return libraryEnglishTitle(item) === "（无标题）" ? "" : libraryEnglishTitle(item);
    case "chineseTitle": return libraryChineseTitle(item);
    case "source": return librarySource(item) === "—" ? "" : librarySource(item);
    case "publisher": return item.effectivePublisher || "";
    case "publicationDate": return item.effectivePublicationDate || "";
    case "volume": return item.effectiveVolume || "";
    case "issue": return item.effectiveIssue || "";
    case "pages": return item.effectivePages || "";
    case "year": return libraryYear(item) === "—" ? "" : libraryYear(item);
    case "authors": return authorText(libraryAuthors(item)) === "—" ? "" : authorText(libraryAuthors(item));
    case "abstract": return item.effectiveAbstract || "";
    case "chineseAbstract": return item.effectiveChineseAbstract || "";
    case "note": return libraryNote(item);
  }
}

function libraryMetadataInput(item: LibraryPaper): LibraryItemMetadataInput {
  const existing = item.metadata;
  return {
    journalOverride: existing?.journalOverride ?? null,
    publisherOverride: existing?.publisherOverride ?? null,
    publicationDateOverride: existing?.publicationDateOverride ?? null,
    volumeOverride: existing?.volumeOverride ?? null,
    issueOverride: existing?.issueOverride ?? null,
    pagesOverride: existing?.pagesOverride ?? null,
    titleOverride: existing?.titleOverride ?? null,
    chineseTitleOverride: existing?.chineseTitleOverride ?? null,
    sourceOverride: existing?.sourceOverride ?? null,
    yearOverride: existing?.yearOverride ?? null,
    authorsOverride: existing?.authorsOverride ?? null,
    abstractOverride: existing?.abstractOverride ?? null,
    chineseAbstractOverride: existing?.chineseAbstractOverride ?? null,
    note: existing?.note ?? null,
  };
}

function beginLibraryInlineEdit(paperId: number, field: LibraryInlineField, button: HTMLElement): void {
  const item = libraryPapers.find((candidate) => candidate.paper.id === paperId);
  if (!item) return;
  const row = button.closest<HTMLElement>(".inspector-form-row") || button.closest<HTMLElement>(".inspector-title-line") || button.closest<HTMLElement>(".inspector-section-head");
  if (!row || row.querySelector("[data-library-inline-input]")) return;
  const valueElement = row.querySelector<HTMLElement>(".field-value, h2, .inspector-abstract-text") || button.closest<HTMLElement>(".inspector-group")?.querySelector<HTMLElement>(".inspector-abstract-text");
  if (!valueElement) return;
  const multiline = field === "abstract" || field === "chineseAbstract" || field === "note";
  const input = document.createElement(multiline ? "textarea" : "input") as HTMLInputElement & HTMLTextAreaElement;
  input.className = "library-inline-input";
  input.dataset.libraryInlineInput = field;
  input.value = libraryInlineCurrentValue(item, field);
  input.placeholder = "清空后恢复原始值";
  if (!multiline) input.type = field === "year" ? "number" : "text";
  if (multiline) input.rows = field === "note" ? 2 : 5;
  valueElement.replaceWith(input);
  button.hidden = true;
  let finished = false;
  const cancel = () => {
    if (finished) return;
    finished = true;
    renderLibrary();
  };
  const save = async () => {
    if (finished) return;
    finished = true;
    const trimmed = input.value.trim();
    const metadata = libraryMetadataInput(item);
    if (field === "year") {
      if (!trimmed) metadata.yearOverride = null;
      else {
        const year = Number(trimmed);
        if (!Number.isInteger(year) || year < 0 || year > 9999) {
          finished = false;
          setStatus("年份必须是有效的整数", "error");
          input.focus();
          return;
        }
        metadata.yearOverride = year;
      }
    }
    if (field === "title") metadata.titleOverride = trimmed || null;
    if (field === "chineseTitle") metadata.chineseTitleOverride = trimmed || null;
    if (field === "source") { metadata.journalOverride = trimmed || null; metadata.sourceOverride = trimmed || null; }
    if (field === "publisher") metadata.publisherOverride = trimmed || null;
    if (field === "publicationDate") metadata.publicationDateOverride = trimmed || null;
    if (field === "volume") metadata.volumeOverride = trimmed || null;
    if (field === "issue") metadata.issueOverride = trimmed || null;
    if (field === "pages") metadata.pagesOverride = trimmed || null;
    if (field === "authors") metadata.authorsOverride = libraryAuthorsFromInput(trimmed);
    if (field === "abstract") metadata.abstractOverride = trimmed || null;
    if (field === "chineseAbstract") metadata.chineseAbstractOverride = trimmed || null;
    if (field === "note") metadata.note = trimmed || null;
    try {
      await invoke("set_library_item_metadata", { paperId, metadata });
      await loadLibraryData(libraryView);
      setStatus(`${libraryInlineFieldLabel(field)} 已更新`, "done");
    } catch (error) {
      finished = false;
      setStatus(`${libraryInlineFieldLabel(field)} 更新失败：${String(error)}`, "error");
      input.focus();
    }
  };
  input.addEventListener("keydown", (event) => {
    if (event.key === "Escape") { event.preventDefault(); cancel(); }
    else if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); void save(); }
  });
  input.addEventListener("blur", () => { void save(); }, { once: true });
  input.focus();
  input.select();
}

function libraryInlineCreateRow(kind: "collection" | "tag", parentId: number | null): string {
  if (!libraryInlineCreate || libraryInlineCreate.kind !== kind || libraryInlineCreate.parentId !== parentId) return "";
  const label = kind === "collection" ? "新建文集" : "新建标签";
  return `<div class="library-inline-create-row" data-inline-create-kind="${kind}"><span class="${kind === "collection" ? "folder-symbol" : "tag-dot"}" aria-hidden="true"></span><input id="library-inline-create-input" type="text" maxlength="120" placeholder="${label}" aria-label="${label}" autofocus /><button type="button" data-action="library-inline-create-submit" title="创建" aria-label="创建">✓</button><button type="button" data-action="library-inline-create-cancel" title="取消" aria-label="取消">×</button></div>`;
}

function beginLibraryInlineCreate(kind: "collection" | "tag", parentId: number | null): void {
  libraryInlineCreate = { kind, parentId };
  renderLibraryNavigation();
  window.requestAnimationFrame(() => {
    const input = $("library-inline-create-input") as HTMLInputElement | null;
    if (!input) return;
    input.focus();
    input.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        libraryInlineCreate = null;
        renderLibraryNavigation();
      } else if (event.key === "Enter") {
        event.preventDefault();
        void submitLibraryInlineCreate();
      }
    });
  });
}

async function submitLibraryInlineCreate(): Promise<void> {
  const state = libraryInlineCreate;
  const input = $("library-inline-create-input") as HTMLInputElement | null;
  if (!state || !input) return;
  const name = input.value.trim();
  if (!name) { input.focus(); return; }
  input.disabled = true;
  try {
    if (state.kind === "collection") await invoke("create_library_collection", { name, parentId: state.parentId });
    else await invoke("create_library_tag", { name, color: null });
    libraryInlineCreate = null;
    await loadLibraryData(libraryView);
    setStatus(`${state.kind === "collection" ? "文集" : "Library Tag"}已创建`, "done");
  } catch (error) {
    input.disabled = false;
    setStatus(`创建失败：${String(error)}`, "error");
    input.focus();
  }
}

function renderLibraryNavigation() {
  document.querySelectorAll(".library-nav-item-view").forEach((item) => {
    const view = (item as HTMLElement).dataset.view;
    const active = !libraryScope && ((libraryView === "all" && view === "library-all") || (libraryView === "recent" && view === "library-recent") || (libraryView === "unfiled" && view === "library-unfiled"));
    item.classList.toggle("active", active);
  });
  const collections = $("library-collection-nav");
  const children = (parentId: number | null, depth = 0): string => libraryInlineCreateRow("collection", parentId) + libraryCollections
    .filter((c) => c.parentId === parentId)
    .map((c) => `<div class="library-nav-item"><button class="library-nav-row${libraryScope?.kind === "collection" && libraryScope.id === c.id ? " active" : ""}" style="padding-left:${12 + depth * 14}px" data-drop-kind="collection" data-action="library-filter-collection" data-collection-id="${c.id}"><span class="nav-symbol folder-symbol" aria-hidden="true"></span><span class="nav-label">${escapeHtml(c.name)}</span><span class="nav-drop-plus" aria-hidden="true">＋</span></button><button class="nav-child" title="在此文集下新建" aria-label="在此文集下新建" data-action="library-create-child" data-parent-id="${c.id}">＋</button><button class="nav-manage" title="重命名文集" aria-label="重命名文集" data-action="library-rename-collection" data-collection-id="${c.id}">✎</button><button class="nav-manage danger" title="删除文集" aria-label="删除文集" data-action="library-delete-collection" data-collection-id="${c.id}">×</button></div>${children(c.id, depth + 1)}`)
    .join("");
  collections.innerHTML = children(null) || '<span class="muted small nav-empty">暂无文献夹</span>';
  const tagRows = libraryTags.map((t) => `<div class="library-nav-item"><button class="library-nav-row${libraryScope?.kind === "tag" && libraryScope.id === t.id ? " active" : ""}" data-drop-kind="tag" data-action="library-filter-tag" data-tag-id="${t.id}"><span class="tag-dot" style="background:${escapeHtml(t.color || "#9ca3af")}"></span><span class="nav-label">${escapeHtml(t.name)}</span><span class="nav-drop-plus" aria-hidden="true">＋</span></button><button class="nav-manage" title="重命名 Library Tag" aria-label="重命名 Library Tag" data-action="library-rename-tag" data-tag-id="${t.id}">✎</button><button class="nav-manage danger" title="删除 Library Tag" aria-label="删除 Library Tag" data-action="library-delete-tag" data-tag-id="${t.id}">×</button></div>`).join("");
  $("library-tag-nav").innerHTML = libraryInlineCreateRow("tag", null) + (tagRows || '<span class="muted small nav-empty">暂无文献标签</span>');
}

function renderLibrary() {
  const title = libraryView === "recent" ? "最近收录" : libraryView === "unfiled" ? "未分类" : "全部文献";
  const titleEl = activeWorkspace === "library" ? $("view-title") : null;
  const layout = $("library-layout");
  layout.classList.toggle("inspector-collapsed", libraryInspectorCollapsed);
  renderLibraryTableHeader();
  renderLibraryColumnMenu();
  applyLibraryLayoutMetrics();
  if (titleEl) titleEl.textContent = libraryScope?.kind === "collection" ? libraryCollections.find(c => c.id === libraryScope?.id)?.name || title : libraryScope?.kind === "tag" ? libraryTags.find(t => t.id === libraryScope?.id)?.name || title : title;
  const count = $("library-count");
  const visiblePapers = libraryScope?.kind === "collection"
    ? libraryPapers.filter((item) => item.collections.some((c) => c.id === libraryScope!.id))
    : libraryScope?.kind === "tag"
      ? libraryPapers.filter((item) => item.tags.some((t) => t.id === libraryScope!.id))
      : libraryPapers;
  if (count) count.textContent = `${visiblePapers.length} 篇`;
  const list = $("library-list");
  if (!list) return;
  if (!visiblePapers.some(item => item.paper.id === selectedLibraryPaperId)) {
    selectedLibraryPaperId = visiblePapers[0]?.paper.id ?? null;
    libraryInspectorAbstractLang = visiblePapers[0]?.effectiveChineseAbstract?.trim() ? "zh" : "en";
  }
  list.innerHTML = visiblePapers.length ? visiblePapers.map((item) => {
    const selected = item.paper.id === selectedLibraryPaperId ? " selected" : "";
    const chineseTitle = libraryChineseTitle(item);
    const note = libraryNote(item);
    const source = librarySource(item);
    const authors = authorText(libraryAuthors(item));
    const cells: Record<LibraryColumn, string> = {
      title: `<span class="library-cell library-row-title" data-column="title" title="${escapeHtml(libraryEnglishTitle(item) + (chineseTitle ? ` · ${chineseTitle}` : ""))}"><span class="library-title-en"><span class="paper-symbol" aria-hidden="true"></span>${escapeHtml(libraryEnglishTitle(item))}</span>${chineseTitle ? `<span class="library-title-zh">${escapeHtml(chineseTitle)}</span>` : ""}</span>`,
      note: `<span class="library-cell library-row-note" data-column="note" title="${escapeHtml(note || "暂无备注")}">${escapeHtml(note || "—")}</span>`,
      source: `<span class="library-cell library-row-source" data-column="source" title="${escapeHtml(source)}">${escapeHtml(source)}</span>`,
      year: `<span class="library-cell library-row-year" data-column="year" title="${escapeHtml(libraryYear(item))}">${escapeHtml(libraryYear(item))}</span>`,
      authors: `<span class="library-cell library-row-authors" data-column="authors" title="${escapeHtml(authors)}">${escapeHtml(authors)}</span>`,
    };
    const attachment = item.attachments[0];
    const child = expandedLibraryAttachmentPaperIds.has(item.paper.id) && attachment
      ? `<div class="library-attachment-child" data-paper-id="${item.paper.id}"><span class="attachment-child-icon">PDF</span><span class="attachment-child-name" title="${escapeHtml(attachment.absolutePath)}">${escapeHtml(attachment.filename)}</span><span class="muted small">${attachment.missing ? "文件缺失" : attachment.storageMode === "managed" ? "已管理" : "已链接"}</span><span class="attachment-child-actions">${attachment.missing ? "" : `<button class="ghost small" data-action="library-open-pdf" data-attachment-id="${attachment.id}">打开</button><button class="ghost small" data-action="library-reveal-pdf" data-attachment-id="${attachment.id}">显示位置</button>`}<button class="ghost small" data-action="library-relink-pdf" data-attachment-id="${attachment.id}">重新链接</button><button class="ghost small danger" data-action="library-detach-pdf" data-attachment-id="${attachment.id}">解除关联</button></span></div>`
      : "";
    const titleCell = attachment
      ? cells.title.replace("<span class=\"paper-symbol\"", `<span class="attachment-disclosure" data-action="library-toggle-attachments" data-paper-id="${item.paper.id}" role="button" tabindex="0" aria-label="展开 PDF 附件" title="展开 PDF 附件">${expandedLibraryAttachmentPaperIds.has(item.paper.id) ? "⌄" : "›"}</span><span class="paper-symbol"`)
      : cells.title;
    return `<button type="button" class="library-paper-row${selected}" aria-pressed="${Boolean(selected)}" data-action="library-select-paper" data-paper-id="${item.paper.id}">${libraryVisibleColumns().map((column) => column === "title" ? titleCell : cells[column]).join("")}</button>${child}`;
  }).join("") : '<div class="empty">文献库还是空的。可以从发现页收录论文。</div>';
  if (libraryInspectorCollapsed) {
    $("library-inspector").innerHTML = '<div class="empty">Inspector 已收起。选择一篇文献查看详情</div>';
    renderLibraryDropState();
    return;
  }
  const selected = visiblePapers.find((item) => item.paper.id === selectedLibraryPaperId) || visiblePapers[0];
  if (selected) {
    selectedLibraryPaperId = selected.paper.id;
    renderLibraryInspector(selected);
  } else {
    selectedLibraryPaperId = null;
    $("library-inspector").innerHTML = '<div class="empty">选择一篇文献查看详情</div>';
  }
  renderLibraryDropState();
}

function renderLibraryRelations(item: LibraryPaper, kind: "collection" | "tag"): string {
  const selected = kind === "collection" ? item.collections : item.tags;
  const all = kind === "collection" ? libraryCollections : libraryTags;
  const label = kind === "collection" ? "文集" : "Library Tags";
  const chips = selected.map(value => `<span class="relation-chip">${kind === "collection" ? '<span class="folder-symbol" aria-hidden="true"></span>' : '<span class="tag-dot" aria-hidden="true"></span>'}<span>${escapeHtml(value.name)}</span><button title="移除 ${escapeHtml(value.name)} 的论文关系" aria-label="移除 ${escapeHtml(value.name)} 的论文关系" data-action="library-relation-remove" data-kind="${kind}" data-id="${value.id}" data-paper-id="${item.paper.id}">×</button></span>`).join("");
  const options = all.map(value => `<div class="relation-option"><button data-action="library-relation-add" data-kind="${kind}" data-id="${value.id}" data-paper-id="${item.paper.id}" ${selected.some(x => x.id === value.id) ? "disabled" : ""}>${escapeHtml(value.name)} ${selected.some(x => x.id === value.id) ? "✓" : "+"}</button><button class="danger" title="删除${label} ${escapeHtml(value.name)}" aria-label="删除${label} ${escapeHtml(value.name)}" data-action="library-delete-${kind === "collection" ? "collection" : "tag"}" data-${kind === "collection" ? "collection" : "tag"}-id="${value.id}">×</button></div>`).join("");
  return `<div class="inspector-form-row inspector-form-row-stack"><span class="field-label">${label}</span><div class="relation-controls">${chips}<details class="relation-picker"><summary title="添加或管理${label}" aria-label="添加或管理${label}">＋</summary><div class="relation-menu"><div class="relation-options">${options || '<span class="muted small">暂无可用项目</span>'}</div><label>新建${label}<input id="library-new-${kind}-name" placeholder="名称" maxlength="120" /></label>${kind === "collection" ? `<label>上级文集<select id="library-new-collection-parent"><option value="">无（顶级文集）</option>${libraryCollections.map(c => `<option value="${c.id}">${escapeHtml(c.name)}</option>`).join("")}</select></label>` : ""}<button class="ghost small" data-action="library-relation-create" data-kind="${kind}" data-paper-id="${item.paper.id}">新建并添加</button></div></details></div></div>`;
}

async function addLibraryRelation(paperId: number, kind: "collection" | "tag", id: number): Promise<void> {
  await invoke(kind === "collection" ? "add_paper_to_collection" : "add_paper_library_tag", kind === "collection" ? { paperId, collectionId: id } : { paperId, tagId: id });
  await loadLibraryData(libraryView);
  setStatus(kind === "collection" ? "已添加到文集，原文集归类已保留" : "已添加 Library Tag", "done");
}

function installLibraryMembershipDrag(): void {
  // Internal Paper → Collection/Library Tag drag is pointer-managed below.
  // Do not install HTML5 drag listeners here: WebKit/Tauri can route those
  // events through the native file-drop path and silently lose the payload.
}

function renderLibraryInspector(item: LibraryPaper) {
  const p = item.paper;
  const hasChineseAbstract = Boolean(item.effectiveChineseAbstract?.trim());
  const abstractLanguage = libraryInspectorAbstractLang;
  const abstractText = libraryAbstract(item, abstractLanguage);
  const englishAbstract = libraryAbstract(item, "en");
  const note = libraryNote(item);
  const attachmentRows = item.attachments.length
    ? item.attachments.map((attachment) => `<div class="attachment-row${attachment.missing ? " missing" : ""}"><div class="attachment-main"><span class="attachment-icon" aria-hidden="true">PDF</span><div class="attachment-copy"><strong title="${escapeHtml(attachment.absolutePath)}">${escapeHtml(attachment.filename)}</strong><span class="muted small">${attachment.missing ? "PDF 文件已移动 / 找不到文件" : attachment.storageMode === "managed" ? "已纳入 CowPaper 文件库 · managed" : "已链接 · 原文件保留"}</span></div></div><div class="attachment-actions">${attachment.missing ? "" : `<button class="ghost small" data-action="library-open-pdf" data-attachment-id="${attachment.id}">打开</button><button class="ghost small" data-action="library-reveal-pdf" data-attachment-id="${attachment.id}">显示位置</button>`}<button class="ghost small" data-action="library-relink-pdf" data-attachment-id="${attachment.id}">重新链接</button><button class="ghost small danger" data-action="library-detach-pdf" data-attachment-id="${attachment.id}">解除关联</button></div></div>`).join("")
    : '<div class="inspector-placeholder"><span class="placeholder-icon" aria-hidden="true">⌑</span><span>尚未添加 PDF 附件。</span></div>';
  const attachmentBusy = libraryPdfBusyPaperId === p.id;
  const attachmentAdd = `<button class="ghost small" data-action="library-attach-pdf" data-paper-id="${p.id}"${attachmentBusy ? " disabled" : ""}>${attachmentBusy ? "添加中…" : "＋ 添加 PDF"}</button>`;
  const abstractTranslate = englishAbstract && !hasChineseAbstract ? `<button class="ghost small" data-action="library-translate-abstract" data-paper-id="${p.id}">翻译为中文</button>` : "";
  const englishTitle = libraryEnglishTitle(item);
  const chineseTitle = libraryChineseTitle(item);
  const authors = authorText(libraryAuthors(item));
  const citation = `${authors}${libraryYear(item) !== "—" ? ` (${libraryYear(item)})` : ""}. ${englishTitle}. ${librarySource(item)}.`;
  const chineseTitleValue = chineseTitle ? escapeHtml(chineseTitle) : `<button class="inspector-link" data-action="library-translate-title" data-paper-id="${p.id}">翻译中文标题</button>`;
  $("library-inspector").innerHTML = `<div class="inspector-tab">元数据<button class="inspector-mobile-close icon-button" aria-label="收起 Inspector" data-action="library-close-inspector">×</button></div><div class="inspector-head"><span class="muted small">期刊论文</span><button type="button" class="ghost small danger" data-action="library-remove" data-paper-id="${p.id}">移出文献库</button></div>
    <header class="inspector-title-block"><div class="inspector-title-line"><h2 title="${escapeHtml(englishTitle)}">${escapeHtml(englishTitle)}</h2>${libraryInlineEditButton(p.id, "title", "Title")}</div>${libraryInspectorRow("中文标题", chineseTitleValue, libraryInlineEditButton(p.id, "chineseTitle", "中文标题"), "inspector-hero-row")}${libraryInspectorRow("作者", escapeHtml(authors), libraryInlineEditButton(p.id, "authors", "作者"), "inspector-hero-row")}</header>
    <section class="inspector-group inspector-metadata"><div class="inspector-section-head"><h3>引用</h3><span class="muted small">Library personal override</span></div><div class="inspector-rows">${libraryInspectorRow("期刊", escapeHtml(librarySource(item)), libraryInlineEditButton(p.id, "source", "期刊"))}${libraryInspectorRow("出版社", escapeHtml(item.effectivePublisher || "—"), libraryInlineEditButton(p.id, "publisher", "出版社"))}${libraryInspectorRow("年份", escapeHtml(libraryYear(item)), libraryInlineEditButton(p.id, "year", "年份"))}${libraryInspectorRow("月份日期", escapeHtml(item.effectivePublicationDate || p.publishedDate || "—"), libraryInlineEditButton(p.id, "publicationDate", "出版日期"))}${libraryInspectorRow("卷", escapeHtml(item.effectiveVolume || "—"), libraryInlineEditButton(p.id, "volume", "卷"))}${libraryInspectorRow("期", escapeHtml(item.effectiveIssue || "—"), libraryInlineEditButton(p.id, "issue", "期"))}${libraryInspectorRow("页码", escapeHtml(item.effectivePages || "—"), libraryInlineEditButton(p.id, "pages", "页码"))}${libraryInspectorRow("DOI", escapeHtml(p.normalizedDoi || "—"))}${libraryInspectorRow("URL", p.url ? `<button class="inspector-link" data-action="open" data-url="${escapeHtml(p.url)}">${escapeHtml(p.url)}</button>` : "—")}</div></section>
    <section class="inspector-group inspector-library"><div class="inspector-section-head"><h3>文库</h3></div><div class="inspector-rows">${libraryInspectorRow("备注", `<span class="${note ? "" : "empty-value"}">${escapeHtml(note || "未添加备注")}</span>`, libraryInlineEditButton(p.id, "note", "备注"))}${renderLibraryRelations(item, "collection")}${renderLibraryRelations(item, "tag")}</div></section>
    <section class="inspector-group inspector-abstract"><div class="inspector-section-head"><h3>摘要</h3><div class="inspector-section-actions"><div class="inspector-language-toggle" role="group" aria-label="摘要语言"><button class="seg ${abstractLanguage === "zh" ? "on" : ""}" data-action="library-abstract-lang" data-lang="zh">中文</button><button class="seg ${abstractLanguage === "en" ? "on" : ""}" data-action="library-abstract-lang" data-lang="en">English</button></div>${libraryInlineEditButton(p.id, abstractLanguage === "zh" ? "chineseAbstract" : "abstract", abstractLanguage === "zh" ? "中文摘要" : "摘要")}</div></div><p class="inspector-abstract-text${abstractText ? "" : " empty-value"}">${escapeHtml(abstractText || "暂无摘要")}</p>${abstractTranslate}</section>
    <section class="inspector-group inspector-attachments"><div class="inspector-section-head"><h3>PDF</h3>${attachmentAdd}</div><div class="attachment-list">${attachmentRows}</div></section>
    <section class="inspector-group inspector-citation"><div class="inspector-section-head"><h3>引用格式</h3></div><p>${escapeHtml(citation)}</p></section>`;

}

async function openLibraryCapture(paperId: number) {
  libraryCapturePaperId = paperId;
  const membership = await invoke<LibraryMembership | null>("get_library_membership", { paperId });
  const selectedCollections = new Set(membership?.collectionIds || []);
  const selectedTags = new Set(membership?.tagIds || []);
  $("library-capture-body").innerHTML = `<p class="muted small">可不选文献夹或标签，稍后也可以在 Inspector 中调整。</p>
    <h4>文献夹 <button class="ghost small" data-action="library-capture-create-collection">＋ 新建</button></h4><div class="capture-options">${libraryCollections.map((c) => `<label class="check compact"><input type="checkbox" data-capture-collection-id="${c.id}" ${selectedCollections.has(c.id) ? "checked" : ""}/> ${escapeHtml(c.name)}</label>`).join("") || '<span class="muted small">暂无文献夹</span>'}</div>
    <h4>文献标签 <button class="ghost small" data-action="library-capture-create-tag">＋ 新建</button></h4><div class="capture-options">${libraryTags.map((t) => `<label class="check compact"><input type="checkbox" data-capture-tag-id="${t.id}" ${selectedTags.has(t.id) ? "checked" : ""}/> ${escapeHtml(t.name)}</label>`).join("") || '<span class="muted small">暂无文献标签</span>'}</div>`;
  $("library-capture-modal").classList.remove("hidden");
}

function closeLibraryCapture() {
  libraryCapturePaperId = null;
  $("library-capture-modal").classList.add("hidden");
}

async function pickPdfPath(): Promise<string | null> {
  const selected = await openFileDialog({
    multiple: false,
    directory: false,
    filters: [{ name: "PDF", extensions: ["pdf"] }],
  });
  if (Array.isArray(selected)) return selected[0] || null;
  return selected || null;
}

function updatePdfActionUi() {
  const importButton = document.querySelector<HTMLButtonElement>("[data-action='library-import-pdf']");
  if (importButton) {
    importButton.disabled = libraryPdfImportBusy;
    importButton.textContent = libraryPdfImportBusy ? "导入中…" : "＋ 导入 PDF";
  }
  document.querySelectorAll<HTMLButtonElement>("[data-action='attach-pdf'], [data-action='library-attach-pdf']").forEach((button) => {
    const busy = libraryPdfBusyPaperId != null && Number(button.dataset.paperId) === libraryPdfBusyPaperId;
    button.disabled = busy;
    button.textContent = busy ? "添加中…" : (button.dataset.action === "library-attach-pdf" ? "＋ 添加 PDF" : "＋ PDF");
  });
}

function formatPdfCandidate(candidate: ExternalPdfCandidate): string {
  const title = candidate.title?.trim() || "（无标题）";
  const authors = authorText(candidate.authors);
  const year = candidate.year == null ? "年份未知" : String(candidate.year);
  return `标题：${title}\n作者：${authors}\n年份：${year}`;
}

function externalPdfOutcomeLabel(outcome: string): string {
  switch (outcome) {
    case "existingAttachmentRefreshed": return "已重新识别现有 PDF 元数据";
    case "existingDoi": return "已根据 exact DOI 关联到现有论文";
    case "existingScholarlyId": return "已根据 exact scholarly identity 关联到现有论文";
    case "manualConfirmation": return "已按确认结果关联到现有论文";
    case "createdExternalPaper": return "已创建 canonical Paper 并加入文献库";
    default: return "PDF 已加入文献库";
  }
}

async function refreshLibrarySelection(paperId: number) {
  selectedLibraryPaperId = paperId;
  libraryInspectorCollapsed = false;
  libraryScope = null;
  libraryView = "all";
  await Promise.all([loadPapers(), loadLibraryData("all")]);
}

async function importExternalPdf() {
  if (libraryPdfImportBusy) return;
  let path: string | null = null;
  try {
    path = await pickPdfPath();
  } catch (error) {
    setStatus(`PDF 文件选择器打开失败：${String(error)}`, "error");
    return;
  }
  if (!path) return;

  libraryPdfImportBusy = true;
  updatePdfActionUi();
  setStatus("正在读取并导入 PDF…", "running");
  try {
    let result = await invoke<ExternalPdfImportResult>("import_pdf", { path, confirmedPaperId: null });
    if (result.requiresConfirmation) {
      const candidate = result.candidate || result.candidates[0];
      if (!candidate) throw new Error("PDF 返回了待确认状态，但没有可确认的论文候选");
      const confirmed = await showConfirmModal({
        title: "确认关联现有论文",
        message: `PDF 元数据与一篇现有论文可能匹配：\n\n${formatPdfCandidate(candidate)}\n\nCowPaper 不会根据模糊标题自动合并。确认后只会新增 PDF 关联，不会复制 Paper。`,
        confirmText: "关联并导入",
        cancelText: "取消",
      });
      if (!confirmed) {
        setStatus("已取消 PDF 导入", "idle");
        return;
      }
      result = await invoke<ExternalPdfImportResult>("import_pdf", { path, confirmedPaperId: candidate.paperId });
    }
    if (result.paperId == null || result.attachment == null) {
      throw new Error("PDF 导入未返回 Paper 或 attachment identity");
    }
    await refreshLibrarySelection(result.paperId);
    setStatus(`${externalPdfOutcomeLabel(result.outcome)}：${result.attachment.filename}`, "done");
  } catch (error) {
    setStatus(`PDF 导入失败：${String(error)}`, "error");
  } finally {
    libraryPdfImportBusy = false;
    updatePdfActionUi();
  }
}

async function attachPdfToPaper(paperId: number) {
  if (libraryPdfBusyPaperId != null) return;
  let path: string | null = null;
  try {
    path = await pickPdfPath();
  } catch (error) {
    setStatus(`PDF 文件选择器打开失败：${String(error)}`, "error");
    return;
  }
  if (!path) return;

    libraryPdfBusyPaperId = paperId;
    updatePdfActionUi();
  if (activeWorkspace === "library") renderLibrary();
    setStatus("正在添加 PDF…", "running");
  try {
    const attachment = await attachPdfPathToPaper(paperId, path, libraryPaperIds.has(paperId));
    const refreshView = activeWorkspace === "library" ? libraryView : "all";
    await Promise.all([loadPapers(), loadLibraryData(refreshView)]);
    if (activeWorkspace === "library") {
      selectedLibraryPaperId = paperId;
      libraryInspectorCollapsed = false;
      renderLibrary();
    }
    setStatus(`PDF 已添加：${attachment.filename}`, "done");
  } catch (error) {
    setStatus(`添加 PDF 失败：${String(error)}`, "error");
  } finally {
    libraryPdfBusyPaperId = null;
    updatePdfActionUi();
    if (activeWorkspace === "library") renderLibrary();
    renderPapers();
  }
}

let libraryInlineActionResolver: ((confirmed: boolean) => void) | null = null;

function requestLibraryInlineAction(message: string, confirmText: string, cancelText: string): Promise<boolean> {
  const box = $("library-inline-action");
  if (libraryInlineActionResolver) libraryInlineActionResolver(false);
  box.innerHTML = `<span>${escapeHtml(message)}</span><button type="button" class="primary small" data-action="library-inline-action-confirm">${escapeHtml(confirmText)}</button><button type="button" class="ghost small" data-action="library-inline-action-cancel">${escapeHtml(cancelText)}</button>`;
  box.classList.remove("hidden");
  return new Promise((resolve) => {
    libraryInlineActionResolver = (confirmed) => {
      libraryInlineActionResolver = null;
      box.classList.add("hidden");
      box.innerHTML = "";
      resolve(confirmed);
    };
    (box.querySelector("[data-action='library-inline-action-confirm']") as HTMLButtonElement | null)?.focus();
  });
}

async function attachPdfPathToPaper(paperId: number, path: string, isLibraryPaper: boolean): Promise<PaperAttachment> {
  const existing = isLibraryPaper
    ? (libraryPapers.find((item) => item.paper.id === paperId)?.attachments || await invoke<PaperAttachment[]>("list_paper_attachments", { paperId }))
    : [];
  if (existing.length) {
    const confirmed = await requestLibraryInlineAction("已有 PDF，替换关联？", "替换", "取消");
    if (!confirmed) throw new Error("已取消替换 PDF");
    // A linked attachment can be safely relinked in place, so there is no
    // second relation even transiently. Managed attachments use the existing
    // safe attach/manage path, then old relations are detached without ever
    // deleting their source files.
    if (existing[0].storageMode === "linked") {
      const replacement = await invoke<PaperAttachment>("relink_pdf", { attachmentId: existing[0].id, path });
      await Promise.all(existing.slice(1).map((old) => invoke("detach_pdf", { attachmentId: old.id })));
      expandedLibraryAttachmentPaperIds.delete(paperId);
      return replacement;
    }
    const replacement = await invoke<PaperAttachment>("attach_pdf", { paperId, path });
    await Promise.all(existing.map((old) => invoke("detach_pdf", { attachmentId: old.id })));
    expandedLibraryAttachmentPaperIds.delete(paperId);
    return replacement;
  }
  const command = isLibraryPaper ? "attach_pdf" : "attach_discovery_pdf";
  const attachment = await invoke<PaperAttachment>(command, { paperId, path });
  expandedLibraryAttachmentPaperIds.delete(paperId);
  return attachment;
}

interface LibraryDroppedFile {
  name: string;
  path: string | null;
}

async function importDroppedPdf(path: string): Promise<ExternalPdfImportResult> {
  let result = await invoke<ExternalPdfImportResult>("import_pdf", { path, confirmedPaperId: null });
  if (result.requiresConfirmation) {
    const candidate = result.candidate || result.candidates[0];
    if (!candidate) throw new Error("PDF 返回待确认状态，但没有可确认的论文候选");
    const confirmed = await showConfirmModal({
      title: "确认关联现有论文",
      message: `PDF 元数据与一篇现有论文可能匹配：\n\n${formatPdfCandidate(candidate)}\n\n只新增 PDF 关联，不复制 Paper。`,
      confirmText: "关联并导入",
      cancelText: "跳过此文件",
    });
    if (!confirmed) throw new Error("已取消关联");
    result = await invoke<ExternalPdfImportResult>("import_pdf", { path, confirmedPaperId: candidate.paperId });
  }
  if (result.paperId == null || result.attachment == null) throw new Error("PDF 导入未返回 Paper 或 attachment identity");
  return result;
}

async function processLibraryDrop(files: LibraryDroppedFile[], targetPaperId: number | null): Promise<void> {
  if (libraryDropQueue.length) return;
  libraryDropQueue = files.map((file) => ({ name: file.name, path: file.path, state: "queued" }));
  libraryDropTargetPaperId = targetPaperId;
  libraryDropActive = false;
  renderLibraryDropState();
  let succeeded = 0;
  for (const item of libraryDropQueue) {
    item.state = "processing";
    item.message = targetPaperId == null ? "导入中" : "添加中";
    renderLibraryDropQueue();
    try {
      if (!item.path) throw new Error("当前 WebView 未提供本地文件路径");
      if (targetPaperId != null) {
        const attachment = await attachPdfPathToPaper(targetPaperId, item.path, true);
        item.message = attachment.filename;
      } else {
        const result = await importDroppedPdf(item.path);
        item.message = result.attachment?.filename || "已导入";
      }
      item.state = "done";
      succeeded += 1;
    } catch (error) {
      item.state = "error";
      item.message = String(error).replace(/^Error:\s*/, "");
    }
    renderLibraryDropQueue();
  }
  if (succeeded > 0) {
    await Promise.all([loadPapers(), loadLibraryData(libraryView)]);
    if (targetPaperId != null) {
      selectedLibraryPaperId = targetPaperId;
      libraryInspectorCollapsed = false;
      renderLibrary();
    }
  }
  const failed = files.length - succeeded;
  setStatus(`PDF 队列完成：成功 ${succeeded}${failed ? ` · 失败 ${failed}` : ""}`, failed ? "error" : "done");
  window.setTimeout(() => {
    if (!libraryDropQueue.some((item) => item.state === "processing")) {
      libraryDropQueue = [];
      libraryDropTargetPaperId = null;
      renderLibraryDropQueue();
    }
  }, 3600);
}

function droppedFilesFromDataTransfer(dataTransfer: DataTransfer): LibraryDroppedFile[] {
  return [...dataTransfer.files]
    .filter((file) => file.name.toLowerCase().endsWith(".pdf"))
    .map((file) => ({ name: file.name, path: (file as File & { path?: string }).path || null }));
}

function libraryDropTargetFromElement(element: Element | null): number | null {
  const row = element?.closest(".library-paper-row") as HTMLElement | null;
  return row ? Number(row.dataset.paperId) : null;
}

function setLibraryDropTarget(target: EventTarget | null): void {
  const element = target instanceof Element ? target : null;
  libraryDropTargetPaperId = libraryDropTargetFromElement(element);
  libraryDropActive = true;
  renderLibraryDropState();
}

function installLibraryInteractions(): void {
  installLibraryMembershipDrag();
  const clearMembershipDropHover = () => document.querySelectorAll(".membership-drop-hover").forEach((el) => el.classList.remove("membership-drop-hover"));
  document.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    const target = event.target as HTMLElement;
    const row = target.closest<HTMLElement>(".library-paper-row");
    const control = target.closest("[data-action]");
    if (!row || (control && control !== row)) return;
    libraryPointerDrag = { paperId: Number(row.dataset.paperId), startX: event.clientX, startY: event.clientY, active: false };
  });
  window.addEventListener("pointermove", (event) => {
    const drag = libraryPointerDrag;
    if (!drag) return;
    if (!drag.active && Math.hypot(event.clientX - drag.startX, event.clientY - drag.startY) < 6) return;
    drag.active = true;
    document.body.classList.add("is-dragging-library-paper");
    clearMembershipDropHover();
    const target = document.elementFromPoint(event.clientX, event.clientY)?.closest<HTMLElement>("[data-drop-kind]");
    if (target) target.classList.add("membership-drop-hover");
    event.preventDefault();
  });
  window.addEventListener("pointerup", async (event) => {
    const drag = libraryPointerDrag;
    libraryPointerDrag = null;
    document.body.classList.remove("is-dragging-library-paper");
    clearMembershipDropHover();
    if (!drag?.active) return;
    librarySuppressNextClick = true;
    window.setTimeout(() => { librarySuppressNextClick = false; }, 0);
    const target = document.elementFromPoint(event.clientX, event.clientY)?.closest<HTMLElement>("[data-drop-kind]");
    if (!target) return;
    const kind = target.dataset.dropKind === "collection" ? "collection" : "tag";
    try {
      await addLibraryRelation(drag.paperId, kind, Number(kind === "collection" ? target.dataset.collectionId : target.dataset.tagId));
      setStatus(kind === "collection" ? "已添加到文集，原文集归类已保留" : "已添加 Library Tag", "done");
    } catch (error) {
      setStatus(`添加失败：${String(error)}`, "error");
    }
  });
  window.addEventListener("pointercancel", () => {
    libraryPointerDrag = null;
    document.body.classList.remove("is-dragging-library-paper");
    clearMembershipDropHover();
  });
  document.addEventListener("dragstart", (event) => {
    const header = (event.target as HTMLElement).closest<HTMLElement>("[data-column-drag]");
    if (!header || !event.dataTransfer) return;
    event.dataTransfer.setData("application/x-cowpaper-column", header.dataset.columnDrag || "");
    event.dataTransfer.effectAllowed = "move";
    header.classList.add("column-dragging");
  });
  document.addEventListener("dragover", (event) => {
    if (!event.dataTransfer?.types.includes("application/x-cowpaper-column")) return;
    const header = (event.target as HTMLElement).closest<HTMLElement>("[data-column-drag]");
    document.querySelectorAll(".column-drag-over").forEach((el) => el.classList.remove("column-drag-over"));
    if (!header) return;
    event.preventDefault();
    header.classList.add("column-drag-over");
  });
  document.addEventListener("dragend", (event) => {
    (event.target as HTMLElement).closest<HTMLElement>("[data-column-drag]")?.classList.remove("column-dragging");
    document.querySelectorAll(".column-drag-over").forEach((el) => el.classList.remove("column-drag-over"));
  });
  document.addEventListener("drop", (event) => {
    if (!event.dataTransfer?.types.includes("application/x-cowpaper-column")) return;
    const header = (event.target as HTMLElement).closest<HTMLElement>("[data-column-drag]");
    const dragged = event.dataTransfer.getData("application/x-cowpaper-column") as LibraryColumn;
    if (!header || !LIBRARY_COLUMNS.includes(dragged)) return;
    const destination = header.dataset.columnDrag as LibraryColumn;
    const from = libraryColumnOrder.indexOf(dragged);
    const to = libraryColumnOrder.indexOf(destination);
    event.preventDefault();
    document.querySelectorAll(".column-drag-over").forEach((el) => el.classList.remove("column-drag-over"));
    if (from < 0 || to < 0 || from === to) return;
    libraryColumnOrder.splice(from, 1);
    libraryColumnOrder.splice(to, 0, dragged);
    persistLibraryLayout();
    renderLibrary();
    setStatus("列顺序已保存", "done");
  });
  const pane = $("library-list-pane");
  pane.addEventListener("dragenter", (event) => {
    if (!event.dataTransfer?.types.includes("Files")) return;
    event.preventDefault();
    setLibraryDropTarget(event.target);
  });
  pane.addEventListener("dragover", (event) => {
    if (!event.dataTransfer?.types.includes("Files")) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "copy";
    setLibraryDropTarget(event.target);
  });
  pane.addEventListener("dragleave", (event) => {
    const related = event.relatedTarget as Node | null;
    if (related && pane.contains(related)) return;
    libraryDropActive = false;
    libraryDropTargetPaperId = null;
    renderLibraryDropState();
  });
  pane.addEventListener("drop", async (event) => {
    if (!event.dataTransfer?.types.includes("Files")) return;
    event.preventDefault();
    const targetPaperId = libraryDropTargetFromElement(event.target instanceof Element ? event.target : null);
    const files = event.dataTransfer ? droppedFilesFromDataTransfer(event.dataTransfer) : [];
    libraryDropActive = false;
    libraryDropTargetPaperId = targetPaperId;
    renderLibraryDropState();
    if (!files.length) {
      setStatus("只支持 PDF 文件", "error");
      return;
    }
    await processLibraryDrop(files, targetPaperId);
  });

  // Tauri desktop drops expose absolute paths through webview drag events;
  // keep the DOM handlers above for browser/dev-server fallback.
  void listen("tauri://drag-over", (event) => {
    const payload = event.payload as { position?: { x: number; y: number } };
    const point = payload.position ? document.elementFromPoint(payload.position.x, payload.position.y) : null;
    if (point && pane.contains(point)) setLibraryDropTarget(point);
  });
  void listen("tauri://drag-leave", () => {
    libraryDropActive = false;
    libraryDropTargetPaperId = null;
    renderLibraryDropState();
  });
  void listen("tauri://drag-drop", (event) => {
    const payload = event.payload as { paths?: string[]; position?: { x: number; y: number } };
    const point = payload.position ? document.elementFromPoint(payload.position.x, payload.position.y) : null;
    const targetPaperId = libraryDropTargetFromElement(point);
    const files = (payload.paths || [])
      .filter((path) => path.toLowerCase().endsWith(".pdf"))
      .map((path) => ({ name: path.split(/[\\/]/).pop() || path, path }));
    libraryDropActive = false;
    libraryDropTargetPaperId = targetPaperId;
    renderLibraryDropState();
    if (files.length) void processLibraryDrop(files, targetPaperId);
    else setStatus("只支持 PDF 文件", "error");
  });

  document.addEventListener("pointerdown", (event) => {
    const target = (event.target as HTMLElement).closest("[data-column-resize]") as HTMLElement | null;
    if (target) {
      const column = target.dataset.columnResize as LibraryColumn;
      if (!LIBRARY_COLUMNS.includes(column)) return;
      const visible = LIBRARY_COLUMNS.filter(c => (document.querySelector(`[data-column="${c}"]`) as HTMLElement)?.offsetWidth > 0);
      const next = visible[visible.indexOf(column) + 1];
      if (!next) return;
      visible.forEach(c => { libraryColumnWidths[c] = (document.querySelector(`[data-column="${c}"]`) as HTMLElement).getBoundingClientRect().width; });
      libraryColumnResize = { column, next, startX: event.clientX, startWidth: libraryColumnWidths[column], nextWidth: libraryColumnWidths[next] };
      document.body.classList.add("is-resizing-library");
      event.preventDefault();
      return;
    }
    if ((event.target as HTMLElement).closest("[data-inspector-resize]")) {
      libraryInspectorResize = { startX: event.clientX, startWidth: libraryInspectorWidth };
      document.body.classList.add("is-resizing-library");
      event.preventDefault();
    }
  });
  window.addEventListener("pointermove", (event) => {
    if (libraryColumnResize) {
      const { column, next, startX, startWidth, nextWidth } = libraryColumnResize;
      const minDelta = Math.max(LIBRARY_COLUMN_MIN[column] - startWidth, next === "year" ? nextWidth - 100 : -Infinity);
      const maxDelta = Math.min(nextWidth - LIBRARY_COLUMN_MIN[next], column === "year" ? 100 - startWidth : Infinity);
      const delta = Math.max(minDelta, Math.min(maxDelta, event.clientX - startX));
      libraryColumnWidths[column] = startWidth + delta;
      libraryColumnWidths[next] = nextWidth - delta;
      applyLibraryLayoutMetrics();
      return;
    }
    if (libraryInspectorResize) {
      const { startX, startWidth } = libraryInspectorResize;
      const maxWidth = Math.min(560, Math.max(300, Math.round(window.innerWidth * 0.52)));
      libraryInspectorWidth = Math.min(maxWidth, Math.max(300, Math.round(startWidth - (event.clientX - startX))));
      applyLibraryLayoutMetrics();
    }
  });
  window.addEventListener("pointerup", () => {
    if (libraryColumnResize || libraryInspectorResize) persistLibraryLayout();
    libraryColumnResize = null;
    libraryInspectorResize = null;
    document.body.classList.remove("is-resizing-library");
  });
  window.addEventListener("resize", () => {
    if (window.innerWidth < 1100 && !libraryInspectorCollapsed && activeWorkspace === "library") {
      libraryInspectorCollapsed = true;
      renderLibrary();
    }
    applyLibraryLayoutMetrics();
  });
  applyLibraryLayoutMetrics();
}

async function submitLibraryCapture() {
  if (libraryCapturePaperId == null) return;
  const collectionIds = [...document.querySelectorAll<HTMLInputElement>("[data-capture-collection-id]:checked")].map((e) => Number(e.dataset.captureCollectionId));
  const tagIds = [...document.querySelectorAll<HTMLInputElement>("[data-capture-tag-id]:checked")].map((e) => Number(e.dataset.captureTagId));
  const paper = papers.find((p) => p.id === libraryCapturePaperId) || recPapers.find((p) => p.id === libraryCapturePaperId);
  const context = (document.querySelector(`[data-paper-id="${libraryCapturePaperId}"]`) as HTMLElement | null)?.dataset.cardContext || "manual";
  const addedSource = context.startsWith("today") ? "recommendation" : context.startsWith("history") ? "history" : context === "favorites" ? "read_later" : "manual";
  await invoke("add_paper_to_library", { paperId: libraryCapturePaperId, collectionIds, tagIds, addedSource });
  libraryPaperIds.add(libraryCapturePaperId);
  if (paper) { paper.isFavorite = false; }
  closeLibraryCapture();
  setStatus("已收录到文献库", "done");
  await Promise.all([loadPapers(), loadLibraryData(libraryView)]);
}

function renderBacklog() {
  const banner = $("backlog-banner");
  const s = aiStatus;
  if (s.state === "paused" && s.remaining > 0) {
    banner.classList.remove("hidden");
    banner.innerHTML = `上次分析未完成，剩余 <strong>${s.remaining}</strong> 篇 · <button class="ghost small" data-action="ai-resume">继续</button>`;
    return;
  }
  const pending = activity.pendingAnalysis;
  if (s.state === "idle" && s.remaining === 0 && pending > 0) {
    banner.classList.remove("hidden");
    banner.innerHTML = `待分析论文 <strong>${pending}</strong> 篇 · <button class="ghost small" data-action="ai-backlog">开始分析</button>`;
    return;
  }
  banner.classList.add("hidden");
  banner.innerHTML = "";
}


// ================= Round 4B：Activity Bar / Center =================

const TRIGGER_ZH: Record<string, string> = {
  manual: "手动检查", startup: "启动检查", daily: "每日检查", tray: "托盘检查",
  journalTest: "期刊测试", autoAfterSync: "同步后自动分析", syncAutoAnalysis: "同步后自动分析",
  retryFailed: "重试失败",
  resumeRecovered: "恢复继续",
  abstractUpgraded: "摘要补全后重新分析",
};
const STATUS_ZH: Record<string, string> = {
  running: "运行中", paused: "已暂停", completed: "完成", completedWithErrors: "完成（有错误）",
  stopped: "已停止", failed: "失败", queued: "排队中", succeeded: "成功",
  cancelled: "已取消", skipped: "跳过",
};

/// 统一工作状态刷新入口：所有界面（Work Center / 积压横幅 /
/// Activity 待处理区 / 设置页计数）消费同一份 (aiStatus, activity) 全局状态。
/// 调用时机：启动 / 同步开始·进度·完成 / 手动 AI 接受 / AI 进度·完成 /
/// pause·resume·stop / retry 完成。任何事件都不允许绕过本函数单独刷新部分 UI。
async function refreshWorkState() {
  await Promise.all([loadAiStatus(), loadActivity()]);
  renderWorkCenter();
  renderBacklog();
  renderPendingCount();
}

async function loadActivity() {
  try {
    activity = await invoke<ActivityState>("get_activity_state");
  } catch {
    activity = emptyActivity();
  }
}

/// 设置页"当前待分析"计数（唯一来源 activity.pendingAnalysis）。
function renderPendingCount() {
  $("pending-count").textContent = `当前待分析：${activity.pendingAnalysis} 篇`;
}

function fmtTimeNow(): string {
  return new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/// 工作中心：状态行（可点击进入活动页）+ 检查新论文 / AI 分析主操作。
/// 状态语义严格区分：running（蓝）→ 进行中；paused/pending（橙）→ 待处理；
/// error（红）→ 真失败；ok（绿少量）→ 无待处理。
function renderWorkCenter() {
  const statusEl = $("work-status");
  const syncBtn = $("btn-sync-main") as HTMLButtonElement;
  const aiBtn = $("btn-ai-main") as HTMLButtonElement;
  const a = activity;
  const s = aiStatus;
  const syncRunning = !!(a.syncBatch && a.syncBatch.status === "running");
  const detailEl = $("work-status-detail");
  let compact = "已更新";
  let detail = "已更新 · AI 待分析 0";
  let cls: string;

  if (syncRunning) {
    cls = "running";
    compact = "同步中";
    detail = `正在检查新论文 · ${a.syncBatch!.journalCompleted}/${a.syncBatch!.journalTotal} 本期刊`;
    syncBtn.disabled = true;
    syncBtn.textContent = "同步中…";
    aiBtn.disabled = false;
    aiBtn.textContent = s.state === "paused" ? "继续" : "AI 分析";
  } else {
    syncBtn.disabled = false;
    syncBtn.textContent = "检查新论文";
    if (s.state === "running" || s.state === "pausing") {
      cls = "running";
      const cur = s.currentPaperTitle
        ? ` · 当前：${s.currentPaperTitle.length > 42 ? s.currentPaperTitle.slice(0, 42) + "…" : s.currentPaperTitle}`
        : "";
      compact = "AI 分析中";
      detail = `AI 分析中 · ${s.completed}/${s.batchSize}${cur}`;
      aiBtn.textContent = "暂停";
    } else if (s.state === "paused") {
      cls = "paused";
      compact = "AI 已暂停";
      detail = `AI 已暂停 · ${s.completed}/${s.batchSize}（剩余 ${s.remaining} 篇）`;
      aiBtn.textContent = "继续";
    } else if (s.remaining > 0) {
      cls = "paused";
      compact = `待分析 ${s.remaining}`;
      detail = `AI 任务未完成 · 剩余 ${s.remaining} 篇`;
      aiBtn.textContent = "继续";
    } else if (a.analysisFailed > 0) {
      cls = "error";
      compact = "分析失败";
      detail = `AI 分析失败 ${a.analysisFailed} 篇 · 可在活动中重试`;
      aiBtn.textContent = "重试失败";
    } else if (a.pendingAnalysis > 0) {
      cls = "pending";
      compact = `待分析 ${a.pendingAnalysis}`;
      detail = `AI 待分析 ${a.pendingAnalysis} 篇`;
      aiBtn.textContent = "AI 分析";
    } else {
      cls = "ok";
      const last = a.lastAnalysis;
      const time = fmtTimeNow();
      const lastText =
        last && last.succeeded > 0
          ? ` · 上次成功分析 ${last.succeeded} 篇${last.finishedAt ? " · " + new Date(last.finishedAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : ""}`
          : "";
      compact = "已更新";
      detail = `已更新 · ${time} · AI 待分析 0${lastText}`;
      aiBtn.textContent = "AI 分析";
    }
  }

  statusEl.className = `work-status ${cls}`;
  statusEl.innerHTML = `<span class="status-dot" aria-hidden="true"></span><span class="toolbar-status-label">${escapeHtml(compact)}</span>`;
  statusEl.setAttribute("aria-label", detail.replace(/<[^>]+>/g, ""));
  statusEl.setAttribute("title", `${detail.replace(/<[^>]+>/g, "")} · 点击查看详情`);
  statusEl.setAttribute("data-status-detail", detail.replace(/<[^>]+>/g, ""));
  if (detailEl) detailEl.textContent = detail.replace(/<[^>]+>/g, "");
}

/// Work Center 的 AI 按钮：按当前上下文分发（暂停 / 继续 / 重试失败 / 开始分析）。
/// 各 action 内部已做统一状态刷新。
async function workAiAction() {
  const s = aiStatus;
  if (s.state === "running" || s.state === "pausing") {
    await pauseAi();
  } else if (s.state === "paused" || s.remaining > 0) {
    await resumeAi();
  } else if (activity.analysisFailed > 0) {
    await retryFailed(null);
  } else {
    await manualAnalyze();
  }
}

function activityItemTitle(p: { title: string | null; paperId: number }): string {
  return p.title || `论文 #${p.paperId}`;
}

let selectedActivity: { type: string; id: number } | null = null;

/// 待处理区（待分析 / 失败重试 / 等待摘要）：计数来自 activity（统一状态），不再单独查询。
function renderActivityPending() {
  const box = $("activity-pending");
  const { pendingAnalysis: pending, analysisFailed: failed, waitingForAbstract: waiting } = activity;
  box.innerHTML = `
    <div class="title">待处理</div>
    <div class="pending-rows">
      <div class="pending-row"><span>待 AI 分析</span><strong>${pending} 篇</strong>
        ${pending > 0 ? `<button class="ghost small" data-action="manual-analyze">开始分析</button>` : ""}</div>
      <div class="pending-row"><span>AI 分析失败</span><strong>${failed} 篇</strong>
        ${failed > 0 ? `<button class="ghost small" data-action="retry-failed">重新分析</button>` : ""}</div>
      <div class="pending-row"><span>缺失摘要</span><strong>${waiting} 篇</strong></div>
    </div>`;
}

/// master-detail：左侧最近活动列表 + 右侧选中批次详情。默认选中最近一条。
async function renderActivityCenter() {
  renderActivityPending();
  await renderActivityHistory();
  if (!selectedActivity) {
    // 默认选中最近一条 activity
    const items = recentActivityItems;
    if (items.length > 0) {
      selectedActivity = { type: items[0].type, id: items[0].id };
    }
  }
  await renderActivityDetail();
}

let recentActivityItems: Array<{ time: string; kind: string; line: string; status: string; id: number; type: string }> = [];

async function renderActivityHistory() {
  const ul = $("activity-history");
  const [sbs, abs, recovers] = await Promise.all([
    invoke<SyncBatch[]>("list_sync_batches", { limit: 25 }).catch(() => []),
    invoke<AnalysisBatch[]>("list_analysis_batches", { limit: 25 }).catch(() => []),
    invoke<AbstractRecoveryBatch[]>("list_abstract_recovery_batches", { limit: 25 }).catch(() => []),
  ]);
  const items: typeof recentActivityItems = [];
  for (const b of sbs) {
    const t = b.finishedAt || b.startedAt || b.createdAt;
    const extra = b.status === "completed" ? `新增 ${b.papersInserted} · 补摘要 ${b.abstractsAdded}` : b.errorSummary || STATUS_ZH[b.status] || b.status;
    items.push({ time: t, kind: "sync", type: "sync", id: b.id, status: b.status, line: `${TRIGGER_ZH[b.trigger] || b.trigger} · ${b.journalTotal} 本期刊 · ${extra}` });
  }
  for (const b of abs) {
    const t = b.finishedAt || b.startedAt || b.createdAt;
    const line = `${TRIGGER_ZH[b.trigger] || b.trigger} · ${b.total} 篇 · 成功 ${b.succeeded}${b.failed ? " · 失败 " + b.failed : ""}`;
    items.push({ time: t, kind: "ai", type: "analysis", id: b.id, status: b.status, line });
  }
  for (const b of recovers) {
    const t = b.finishedAt || b.startedAt || b.createdAt;
    items.push({ time: t, kind: "recovery", type: "recovery", id: b.id, status: b.status, line: `摘要补全 · 已处理 ${b.completed}/${b.total} · 补回 ${b.recovered} · 未找到 ${b.notFound} · 失败 ${b.failed}` });
  }
  items.sort((x, y) => (y.time || "").localeCompare(x.time || ""));
  recentActivityItems = items.slice(0, 50);
  ul.innerHTML = recentActivityItems
    .map(
      (i) => `
      <li class="card activity-item ${selectedActivity && selectedActivity.type === i.type && selectedActivity.id === i.id ? "selected" : ""}" data-activity-type="${i.type}" data-activity-id="${i.id}">
        <div class="row">
          <div class="grow">
            <div class="title">${i.kind === "sync" ? "检查新论文" : i.kind === "recovery" ? "摘要补全" : "AI 分析"} #${i.id} <span class="chip muted-chip">${STATUS_ZH[i.status] || i.status}</span></div>
            <div class="muted small">${i.line}</div>
          </div>
          <span class="muted small">${i.time ? new Date(i.time).toLocaleTimeString() : "—"}</span>
        </div>
      </li>`,
    )
    .join("") || '<li class="empty">暂无活动记录</li>';
}

async function renderActivityDetail() {
  const box = $("activity-detail");
  if (!selectedActivity) {
    box.innerHTML = '<div class="empty">请选择一项活动查看详情</div>';
    return;
  }
  const { type, id } = selectedActivity;
  if (type === "sync") {
    const [b, papers] = await invoke<[SyncBatch, SyncBatchPaper[]]>("get_sync_batch", { id });
    const groups: Record<string, number> = {};
    for (const p of papers) groups[p.result] = (groups[p.result] || 0) + 1;
    const dur = b.startedAt && b.finishedAt ? fmtDur(Math.max(0, Math.round((new Date(b.finishedAt).getTime() - new Date(b.startedAt).getTime()) / 1000))) : "—";
    box.innerHTML = `
      <div class="card">
        <div class="title">同步 #${b.id} <span class="chip muted-chip">${STATUS_ZH[b.status] || b.status}</span></div>
        <div class="muted small">${TRIGGER_ZH[b.trigger] || b.trigger} · ${b.startedAt ? new Date(b.startedAt).toLocaleTimeString() : "—"} → ${b.finishedAt ? new Date(b.finishedAt).toLocaleTimeString() : "—"} · 耗时 ${dur}</div>
        <div class="paper-meta">期刊：${b.journalCompleted} / ${b.journalTotal}（失败 ${b.journalFailed}）</div>
        <div class="paper-meta">记录：发现 ${b.recordsFound} · 新增 ${b.papersInserted} · 已有 ${b.papersExisting} · 补摘要 ${b.abstractsAdded}</div>
        <div class="paper-meta muted small">本次涉及论文 ${papers.length} 篇（${Object.entries(groups).map(([k, v]) => `${k} ${v}`).join(" · ")}）</div>
      </div>`;
  } else if (type === "recovery") {
    const [b, items] = await invoke<[AbstractRecoveryBatch, AbstractRecoveryItem[]]>("get_abstract_recovery_batch", { id });
    box.innerHTML = `<div class="card"><div class="title">摘要补全 #${b.id} <span class="chip muted-chip">${STATUS_ZH[b.status] || b.status}</span></div><div class="paper-meta">已处理 ${b.completed}/${b.total} · 补回 ${b.recovered} · 未找到 ${b.notFound} · 来源失败 ${b.failed}</div><div class="abstract muted small">${items.filter((i) => i.outcome && i.outcome !== "recovered").slice(0, 8).map((i) => `${escapeHtml(i.title || "未命名")}：${i.outcome}${i.nextRetryAt ? `（下次 ${fmtDate(i.nextRetryAt)}）` : ""}`).join("；") || "所有论文已补回摘要"}</div></div>`;
  } else {
    const [b, items] = await invoke<[AnalysisBatch, AnalysisBatchItem[]]>("get_analysis_batch", { id });
    const failed = items.filter((i) => i.status === "failed");
    const dur = b.startedAt && b.finishedAt ? fmtDur(Math.max(0, Math.round((new Date(b.finishedAt).getTime() - new Date(b.startedAt).getTime()) / 1000))) : "—";
    const controls =
      b.status === "running"
        ? `<button class="ghost small" data-action="ai-pause">暂停</button><button class="ghost small" data-action="ai-stop">停止本次任务</button>`
        : b.status === "paused"
          ? `<button class="primary small" data-action="ai-resume">继续分析</button><button class="ghost small" data-action="ai-stop">停止本次任务</button>`
          : b.failed > 0
            ? `<button class="ghost small" data-action="ai-retry" data-batch="${b.id}">重试失败论文</button>`
            : "";
    box.innerHTML = `
      <div class="card">
        <div class="row">
          <div class="grow">
            <div class="title">AI 分析 #${b.id} <span class="chip muted-chip">${STATUS_ZH[b.status] || b.status}</span></div>
            <div class="muted small">${TRIGGER_ZH[b.trigger] || b.trigger}${b.sourceSyncBatchId ? ` · 来自同步 #${b.sourceSyncBatchId}` : ""}${b.parentBatchId ? ` · 重试自 #${b.parentBatchId}` : ""} · model ${b.modelName || "—"}</div>
          </div>
        </div>
        <div class="paper-meta">开始 ${b.startedAt ? new Date(b.startedAt).toLocaleTimeString() : "—"} · 结束 ${b.finishedAt ? new Date(b.finishedAt).toLocaleTimeString() : "—"} · 耗时 ${dur}</div>
        <div class="paper-meta">总数 ${b.total} · 成功 ${b.succeeded} · 失败 ${b.failed} · 跳过 ${b.skipped} · 剩余 ${b.remaining}</div>
        ${failed.length ? `<div class="abstract muted small">失败论文：${failed.slice(0, 5).map((f) => activityItemTitle(f)).join("；")}${failed.length > 5 ? "…" : ""}</div>` : ""}
        <div class="activity-actions">${controls}</div>
      </div>`;
  }
  // 同步左侧选中态
  document.querySelectorAll(".activity-item").forEach((el) => {
    const e = el as HTMLElement;
    e.classList.toggle("selected", e.dataset.activityType === type && parseInt(e.dataset.activityId!, 10) === id);
  });
}

function renderNextCheck() {
  const el = $("next-check");
  if (!settings) return;
  if (!settings.dailyAutoSync) {
    el.textContent = "每日自动检查已关闭";
    return;
  }
  const now = new Date();
  const [h, m] = settings.dailySyncTime.split(":").map(Number);
  const next = new Date(now);
  next.setHours(h || 9, m || 0, 0, 0);
  if (next <= now) next.setDate(next.getDate() + 1);
  const prefix = next.toDateString() === now.toDateString() ? "今天" : "明天";
  el.textContent = `下一次计划检查：${prefix} ${settings.dailySyncTime}（CowPaper 运行时按计划检查；完全退出后会在下次启动时补检查）`;
}

// ---------- 动作 ----------

function switchView(name: string) {
  // Unsaved Guard（Round 6.5）：标签有未保存修改时拦截导航
  if (tagConfigDirty && name !== "tags") {
    showConfirmModal({
      title: "标签设置尚未保存",
      message: "放弃后将恢复到上次保存的标签设置。",
      confirmText: "放弃修改",
      cancelText: "继续编辑",
    }).then((ok) => {
      if (ok) {
        tagDraft = tagBaseline.map((x) => ({ ...x }));
        setTagDirty(false);
        doSwitch(name);
      }
    });
    return;
  }
  doSwitch(name);
}

function doSwitch(name: string) {
  const isLibrary = name.startsWith("library-");
  if (isLibrary && ["library-all", "library-recent", "library-unfiled"].includes(name)) {
    // Standard Library views own the scope; collection/tag filters are a
    // separate sidebar mode and should not leak into Recent or Unfiled.
    libraryScope = null;
    libraryInspectorCollapsed = window.innerWidth < 1100;
  }
  activeWorkspace = isLibrary ? "library" : "discovery";
  document.body.classList.toggle("library-workspace", isLibrary);
  document.querySelectorAll(".workspace-nav").forEach((nav) => nav.classList.toggle("hidden", (nav as HTMLElement).dataset.workspaceNav !== activeWorkspace));
  document.querySelectorAll(".workspace-tab").forEach((tab) => tab.classList.toggle("active", (tab as HTMLElement).dataset.workspace === activeWorkspace));
  document.querySelectorAll(".nav-item").forEach((t) => t.classList.toggle("active", (t as HTMLElement).dataset.view === name));
  // Recent/unfiled are views of the same three-column shell; only the data set changes.
  const visualView = isLibrary ? "library-all" : name;
  document.querySelectorAll(".view").forEach((v) => v.classList.toggle("active", v.id === `view-${visualView}`));
  const titles: Record<string, string> = {
    recommend: "今日", "recommend-history": "历史", papers: "所有论文", favorites: "稍后看", journals: "期刊", tags: "研究兴趣", settings: "设置", activity: "活动",
    "library-all": "全部文献", "library-recent": "最近收录", "library-unfiled": "未分类",
  };
  $("view-title").textContent = titles[name] || name;
  // 进入活动页时渲染 master-detail（数据来自统一 activity + 批次查询）
  if (name === "activity") renderActivityCenter().catch(() => {});
  // 进入期刊订阅页：保持当前 tab（互斥渲染）
  if (name === "journals") setJournalTab(journalTab);
  // 进入历史推荐页时渲染快照
  if (name === "recommend-history") {
    historyCycleKey = null;
    renderRecommendHistory();
  }
  // 进入标签页时加载 Draft Editor
  if (name === "tags") loadTagEditor();
  if (isLibrary) {
    const view = name === "library-recent" ? "recent" : name === "library-unfiled" ? "unfiled" : "all";
    loadLibraryData(view);
  }
}

interface SyncStartResult {
  started: boolean;
  reason: string;
  trigger: string | null;
  startedAt: string | null;
}

async function startSync(ids: number[] | null, trigger: string = "manual") {
  const res = await invoke<SyncStartResult>("sync_journals", { trigger, ids });
  if (!res.started) {
    // 已有全局同步在执行：不得再启动第二份
    setStatus("正在检查新论文…", "running");
    return;
  }
  setStatus("同步中…", "running");
}

async function requireKey(): Promise<boolean> {
  if (await hasKey()) return true;
  setStatus("请先在「设置」保存 DeepSeek API Key", "error");
  switchView("settings");
  return false;
}

/**
 * Run one bounded title-only backlog batch.  The backend selects both newly
 * discovered and historical missing-abstract papers, so callers must not
 * restrict this to the current sync result.
 */
let missingTitleBacklogInFlight = false;
let missingTitleBacklogDraining = false;
let missingTitleLastProgressAt = 0;
let missingTitleLivenessTimer: number | null = null;
// Rust bounds one title request at 45 seconds and retries it at most once.
// Give event delivery/rendering headroom, but never leave the frontend gate
// permanently occupied if the worker dies before it can emit a terminal event.
const TITLE_TRANSLATION_LIVENESS_WINDOW_MS = 120_000;

function clearMissingTitleLivenessWatch(): void {
  if (missingTitleLivenessTimer !== null) {
    window.clearInterval(missingTitleLivenessTimer);
    missingTitleLivenessTimer = null;
  }
}

function releaseStaleMissingTitleState(): void {
  if (!missingTitleBacklogInFlight) return;
  if (Date.now() - missingTitleLastProgressAt <= TITLE_TRANSLATION_LIVENESS_WINDOW_MS) return;
  // This only releases stale frontend state. The Rust process-wide permit
  // remains authoritative, so a later invoke cannot create a second worker.
  missingTitleBacklogInFlight = false;
  missingTitleBacklogDraining = false;
  clearMissingTitleLivenessWatch();
  setStatus("标题翻译任务长时间无进度；前端状态已释放，后端仍会防止重复任务", "error");
  console.error("title-only translation liveness timeout");
}

function startMissingTitleLivenessWatch(): void {
  clearMissingTitleLivenessWatch();
  missingTitleLivenessTimer = window.setInterval(releaseStaleMissingTitleState, 5_000);
}

function releaseMissingTitleState(): void {
  missingTitleBacklogInFlight = false;
  missingTitleBacklogDraining = false;
  clearMissingTitleLivenessWatch();
}

async function startMissingTitleTranslation(drainBacklog: boolean): Promise<number> {
  if (missingTitleBacklogInFlight) {
    setStatus("标题翻译正在进行中…", "running");
    return 0;
  }
  if (!(await hasKey())) return 0;

  missingTitleBacklogInFlight = true;
  missingTitleLastProgressAt = Date.now();
  startMissingTitleLivenessWatch();
  if (drainBacklog) missingTitleBacklogDraining = true;
  try {
    const scheduled = await invoke<number>("translate_missing_titles", { paperIds: null, model: getModel() });
    if (scheduled) {
      setStatus(`正在翻译 ${scheduled} 篇缺摘要论文标题…`, "running");
    } else {
      // The command does not emit a completion event when there is no work.
      // Always release the UI gate on this path so a future sync/launch can retry.
      releaseMissingTitleState();
    }
    return scheduled;
  } catch (err) {
    releaseMissingTitleState();
    setStatus(`标题翻译启动失败：${safeError(err)}`, "error");
    console.error("translate_missing_titles invoke failed", err);
    return 0;
  }
}

async function scheduleMissingTitleBacklog(): Promise<number> {
  if (!settings?.autoAnalyzeNew) return 0;
  return startMissingTitleTranslation(true);
}

/// 统一的 start_ai 调用（所有入口必须走这里）：带 trigger + 错误捕获 + 即时反馈。
async function startAnalyze(paperIds: number[] | null, trigger: string, sourceSyncBatchId: number | null = null): Promise<boolean> {
  if (!(await requireKey())) return false;
  try {
    await invoke("start_ai", { paperIds, model: getModel(), trigger, sourceSyncBatchId });
    await refreshWorkState();
    return true;
  } catch (err) {
    setStatus("无法开始 AI 分析", "error");
    console.error("start_ai 调用失败:", err); // 二级技术原因
    return false;
  }
}

async function pauseAi() {
  try {
    await invoke("pause_ai");
  } catch (err) {
    setStatus("无法暂停分析", "error");
    console.error("pause_ai 调用失败:", err);
  }
  await refreshWorkState();
}
async function resumeAi() {
  if (!(await requireKey())) return;
  try {
    await invoke("resume_ai", { model: getModel() });
  } catch (err) {
    setStatus("无法继续分析", "error");
    console.error("resume_ai 调用失败:", err);
  }
  await refreshWorkState();
}
async function stopAi() {
  const ok = await showConfirmModal({
    title: "停止 AI 分析",
    message: "停止本次分析？\n已完成结果会保留，未完成论文回到待分析。",
    confirmText: "停止",
    cancelText: "取消",
  });
  if (!ok) return; // 正常取消
  try {
    await invoke("stop_ai");
  } catch (err) {
    setStatus("无法停止分析", "error");
    console.error("stop_ai 调用失败:", err);
  }
  await refreshWorkState();
}

// ---------- 应用内确认 Modal（替代 WebView 原生 confirm/alert/prompt） ----------

interface ConfirmModalOptions {
  title: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
}

/// 轻量应用内确认 Modal，返回 Promise<boolean>。
/// 确认 → true；取消（取消按钮 / Escape / 点击遮罩）→ false。
/// 不依赖 window.confirm/alert/prompt：macOS WKWebView 下原生 dialog 不可靠。
function showConfirmModal(opts: ConfirmModalOptions): Promise<boolean> {
  const overlay = $("confirm-modal");
  const titleEl = $("confirm-modal-title");
  const msgEl = $("confirm-modal-message");
  const okBtn = $("confirm-modal-ok") as HTMLButtonElement;
  const cancelBtn = $("confirm-modal-cancel") as HTMLButtonElement;
  titleEl.textContent = opts.title;
  msgEl.textContent = opts.message;
  okBtn.textContent = opts.confirmText ?? "确认";
  cancelBtn.textContent = opts.cancelText ?? "取消";
  return new Promise<boolean>((resolve) => {
    let done = false;
    const finish = (result: boolean) => {
      if (done) return;
      done = true;
      overlay.classList.add("hidden");
      $("confirm-modal-input").classList.add("hidden");
      okBtn.removeEventListener("click", onOk);
      cancelBtn.removeEventListener("click", onCancel);
      overlay.removeEventListener("click", onOverlay);
      window.removeEventListener("keydown", onKey);
      resolve(result);
    };
    function onOk() {
      finish(true);
    }
    function onCancel() {
      finish(false);
    }
    function onOverlay(e: MouseEvent) {
      if (e.target === overlay) finish(false);
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") finish(false);
    }
    okBtn.addEventListener("click", onOk);
    cancelBtn.addEventListener("click", onCancel);
    overlay.addEventListener("click", onOverlay);
    window.addEventListener("keydown", onKey);
    overlay.classList.remove("hidden");
    cancelBtn.focus();
  });
}

/// 输入型 App Modal（新建/重命名集合等）：返回输入值；取消/Escape/遮罩 → null。
function showPromptModal(title: string, placeholder: string, defaultValue = ""): Promise<string | null> {
  const input = $("confirm-modal-input") as HTMLInputElement;
  input.placeholder = placeholder;
  input.value = defaultValue;
  return new Promise((resolve) => {
    // 复用 showConfirmModal 的 Promise 流程，输入值在确认时读取
    showConfirmModal({
      title,
      message: "",
      confirmText: "确定",
      cancelText: "取消",
    }).then((ok) => {
      if (!ok) {
        resolve(null);
        return;
      }
      resolve(input.value.trim() || null);
    });
    input.classList.remove("hidden");
    input.focus();
  });
}

/// AI 分析确认：数量 + 费用提示，所有手动 AI 入口统一经它确认。
function confirmAiAnalysis(count: number): Promise<boolean> {
  return showConfirmModal({
    title: "开始 AI 分析",
    message: `当前有 ${count} 篇论文待分析。\n分析将调用 DeepSeek API，可能产生 API 使用费用。`,
    confirmText: "开始分析",
    cancelText: "取消",
  });
}

/// 统一的重试失败入口（可指定来源批次）。
async function retryFailed(parentBatchId: number | null = null): Promise<boolean> {
  if (!(await requireKey())) return false;
  try {
    await invoke("retry_failed_ai", { model: getModel(), parentBatchId });
    setStatus("已加入失败论文重试队列", "running");
    await refreshWorkState();
    return true;
  } catch (err) {
    setStatus("无法开始重试", "error");
    console.error("retry_failed_ai 调用失败:", err);
    return false;
  }
}

/// 唯一的手动 AI 入口（顶部按钮 / Activity 待处理 / 所有论文轻入口都调它）。
/// 确认环节使用应用内 Modal（WebView 原生 confirm 不可靠），整个调用链有顶层 try/catch，
/// 任何环节失败都产生明确反馈，不允许 silent failure。
async function manualAnalyze(): Promise<boolean> {
  try {
    if (!(await hasKey())) {
      setStatus("请先在设置中配置 DeepSeek API Key", "error");
      switchView("settings");
      return false;
    }
    if (aiStatus.state !== "idle" || aiStatus.remaining > 0) {
      setStatus("已有 AI 分析任务正在运行", "error");
      await refreshWorkState();
      switchView("activity");
      return false;
    }
    let pending: number;
    try {
      pending = await invoke<number>("get_pending_ai_count");
    } catch (err) {
      setStatus("无法获取待分析论文数，请稍后重试", "error");
      console.error("get_pending_ai_count 调用失败:", err);
      return false;
    }
    if (pending <= 0) {
      setStatus("当前没有待分析论文", "done");
      return false;
    }
    // 应用内确认 Modal；用户取消 = 正常取消：不建 batch、不调用 DeepSeek、不显示错误
    const confirmed = await confirmAiAnalysis(pending);
    if (!confirmed) return false;
    // 立即反馈：不等首篇 DeepSeek 返回、不等 ai://progress
    setStatus(`正在准备 AI 分析 · ${pending} 篇`, "running");
    return await startAnalyze(null, "manual");
  } catch (err) {
    setStatus("无法开始 AI 分析", "error");
    console.error("manualAnalyze 失败:", err);
    return false;
  }
}

async function addJournalHandler(e: Event) {
  e.preventDefault();
  const name = ($("add-name") as HTMLInputElement).value.trim();
  const printIssn = ($("add-print-issn") as HTMLInputElement).value.trim();
  const onlineIssn = ($("add-online-issn") as HTMLInputElement).value.trim();
  if (!printIssn && !onlineIssn) {
    $("add-error").textContent = "至少填写一个 ISSN";
    return;
  }
  $("add-error").textContent = "";
  try {
    await invoke("add_journal", { name: name || null, printIssn: printIssn || null, onlineIssn: onlineIssn || null });
    ($("add-name") as HTMLInputElement).value = "";
    ($("add-print-issn") as HTMLInputElement).value = "";
    ($("add-online-issn") as HTMLInputElement).value = "";
    await loadJournals();
  } catch (err) {
    if (String(err).includes("ISSN_PAIR_UNKNOWN_CONFIRMATION")) {
      const confirmed = await showConfirmModal({
        title: "无法确认 ISSN 关联",
        message: "暂未能从公开元数据确认两个 ISSN 的关联。\n如果你确认它们属于同一期刊，可以继续添加。",
        confirmText: "仍然添加",
        cancelText: "取消",
      });
      if (!confirmed) return;
      try {
        await invoke("add_journal", { name: name || null, printIssn: printIssn || null, onlineIssn: onlineIssn || null, confirmUnknown: true });
        ($("add-name") as HTMLInputElement).value = "";
        ($("add-print-issn") as HTMLInputElement).value = "";
        ($("add-online-issn") as HTMLInputElement).value = "";
        await loadJournals();
      } catch (confirmationErr) {
        $("add-error").textContent = String(confirmationErr);
      }
      return;
    }
    $("add-error").textContent = String(err);
  }
}

async function setFlag(id: number, flag: string, value: boolean) {
  await invoke("set_paper_flag", { id, flag, value });
  const p = papers.find((x) => x.id === id);
  if (p) {
    if (flag === "favorite") p.isFavorite = value;
    else if (flag === "read") p.isRead = value;
    else if (flag === "ignored") p.isIgnored = value;
  }
  renderPapers();
  renderRecommend();
  renderFavorites();
}

/// DeepSeek 模块内专用状态区（测试连接 / 保存 / 删除结果只显示在这里）。
function setDeepSeekStatus(text: string, cls: string) {
  const el = $("deepseek-status");
  el.textContent = text;
  el.className = `deepseek-status ${cls}`;
}

async function saveKey() {
  const key = ($("api-key") as HTMLInputElement).value.trim();
  const model = ($("model") as HTMLInputElement).value.trim() || DEFAULT_MODEL;
  localStorage.setItem(MODEL_NAME, model);
  if (!key) {
    setDeepSeekStatus("请输入 API Key", "error");
    return;
  }
  try {
    await invoke("save_api_key", { key });
    ($("api-key") as HTMLInputElement).value = ""; // 不回显真实 Key
    setDeepSeekStatus("✓ API Key 已保存在本机", "ok small");
    await refreshKeyStatus();
  } catch (err) {
    setDeepSeekStatus(String(err), "error");
  }
}

async function testConnection() {
  const model = ($("model") as HTMLInputElement).value.trim() || DEFAULT_MODEL;
  setDeepSeekStatus("测试中…", "muted small");
  try {
    const r = await invoke<{ ok: boolean; message: string }>("test_api_connection", { model });
    setDeepSeekStatus(r.message, r.ok ? "ok small" : "error");
  } catch (err) {
    setDeepSeekStatus(String(err), "error");
  }
}

async function saveSettings() {
  const s = {
    startupAutoSync: ($("set-startup-sync") as HTMLInputElement).checked,
    dailyAutoSync: ($("set-daily-sync") as HTMLInputElement).checked,
    dailySyncTime: ($("set-daily-time") as HTMLInputElement).value || "09:00",
    autoAnalyzeNew: ($("set-auto-analyze") as HTMLInputElement).checked,
    defaultAbstractLang: ($("set-abstract-lang") as HTMLSelectElement).value,
    pdfFileHandlingMode: ($("set-pdf-file-handling-mode") as HTMLSelectElement).value as "none" | "copy" | "move",
    pdfLibraryRoot: ($("set-pdf-library-root") as HTMLInputElement).value.trim(),
    pdfNamingTemplate: ($("set-pdf-naming-template") as HTMLInputElement).value,
    pdfSubfolderRule: ($("set-pdf-subfolder-rule") as HTMLSelectElement).value as "none" | "year" | "journal/source",
  };
  if (s.pdfFileHandlingMode !== "none" && !s.pdfLibraryRoot) {
    $("settings-msg").textContent = "copy / move 模式需要先选择 Library root directory";
    $("settings-msg").className = "error small";
    return;
  }
  if (!s.pdfNamingTemplate.trim()) {
    $("settings-msg").textContent = "PDF 命名模板不能为空";
    $("settings-msg").className = "error small";
    return;
  }
  try {
    await invoke("set_settings", { s });
    settings = s;
    abstractLang = s.defaultAbstractLang === "en" ? "en" : "zh";
    renderPapers();
    renderNextCheck();
    renderPdfTemplateExample();
    $("settings-msg").textContent = "设置已保存（每日时间修改后，运行中的调度器将在 30s 内采用）";
    $("settings-msg").className = "ok small";
  } catch (err) {
    $("settings-msg").textContent = `设置未保存：${String(err)}`;
    $("settings-msg").className = "error small";
  }
}

// ---------- 事件监听 ----------

async function setupListeners() {
  await listen("abstract://progress", (e) => {
    const p = e.payload as AbstractRecoveryProgress;
    const source = p.currentSource ? ` · ${p.currentSource}` : "";
    setStatus(`正在补全摘要 · ${p.completed}/${p.total}${source}`, "running");
  });
  await listen("abstract://done", async (e) => { await finishRecoveryBatch(e.payload as number); });
  await listen("abstract://error", (e) => { setStatus(`摘要补全失败：${String(e.payload)}`, "error"); });
  await listen("sync://start", async () => {
    setStatus("同步中…", "running");
    await refreshWorkState();
  });
  await listen("sync://journal-start", (e) => setStatus(`正在同步 ${e.payload}`, "running"));
  await listen("sync://journal-error", (e) => setStatus(`同步错误：${e.payload}`, "error"));
  // 同步进度为高频事件：只轻量更新 Work Center 状态行，不触发全量刷新
  await listen("sync://progress", (e) => {
    const p = e.payload as SyncProgress;
    const el = $("work-status");
    el.className = "work-status running";
    const checked = p.journalCompleted + p.journalFailed;
    const detail = `正在检查新论文 · ${checked}/${p.journalTotal} 本期刊${p.journalFailed ? ` · 失败 ${p.journalFailed}` : ""}`;
    el.innerHTML = `<span class="status-dot" aria-hidden="true"></span><span class="toolbar-status-label">同步中</span>`;
    el.title = `${detail} · 点击查看详情`;
    el.setAttribute("aria-label", detail);
    const detailEl = $("work-status-detail");
    if (detailEl) detailEl.textContent = detail;
    if (p.currentJournal) {
      setStatus(`正在同步 ${p.currentJournal} · ${checked}/${p.journalTotal}`, "running");
    } else {
      setStatus(`已检查 ${checked}/${p.journalTotal} 本期刊${p.journalFailed ? ` · 失败 ${p.journalFailed}` : ""}`, p.journalFailed ? "error" : "running");
    }
  });
  await listen("sync://done", async (e) => {
    const r = e.payload as any;
    setStatus(`同步完成：新增 ${r.newPapers} · 已有 ${r.existingPapers} · 补摘要 ${r.abstractsAdded || 0}${r.abstractsUpgraded ? " · 摘要升级 " + r.abstractsUpgraded : ""}`, "done");
    // 统一刷新：papers + 工作状态（Work Center / 徽标 / 面板 / 待处理区 / 计数）
    await loadJournals();
    await loadPapers();
    await refreshWorkState();
    // 今日推荐随同步结果更新（Round 6：open run 自动重算）
    await refreshRecommendations();
    // Post-Sync 自动分析（Round 5B.1）：一次 sync 最多启动一个 AnalysisBatch。
    // 新论文受「同步后自动分析新论文」checkbox 控制；摘要升级论文默认自动重新分析。
    // 两类目标合并为单一 batch（trigger=syncAutoAnalysis），按 paper id 去重，只调用一次 start_ai。
    const newEligible =
      settings?.autoAnalyzeNew && Array.isArray(r.newPaperIds) ? (r.newPaperIds as number[]) : [];
    const upgradedEligible =
      Array.isArray(r.abstractUpgradedIds) ? (r.abstractUpgradedIds as number[]) : [];
    const postSyncIds = [...new Set([...newEligible, ...upgradedEligible])];
    if (postSyncIds.length > 0 && (await hasKey())) {
      await invoke("start_ai", {
        paperIds: postSyncIds,
        model: getModel(),
        trigger: "syncAutoAnalysis",
        sourceSyncBatchId: r.batchId || null,
      });
      await refreshWorkState();
    }
    // Missing abstracts never enter full analysis, but title translation is
    // safe and useful without an abstract. It remains a separate, title-only
    // operation and therefore cannot affect recommendation eligibility.
    await scheduleMissingTitleBacklog();
  });

  await listen("title-translation://done", async (e) => {
    const r = e.payload as { translated: number; failed: number; translatedIds?: number[]; errors?: string[] };
    const continueDraining = missingTitleBacklogDraining && r.translated > 0 && r.failed === 0;
    // Release before any rendering work: a listener/rendering failure must
    // never leave the automatic backlog permanently suppressed.
    releaseMissingTitleState();
    try {
      await loadPapers();
      // The title-only worker may finish while Today or a historical missing
      // list is visible. Rebuild that visible source immediately so users do
      // not have to switch tabs, sync again, or restart to see chineseTitle.
      await refreshRecommendations();
      if (historyCycleKey && historyTab === "missing") await renderRecommendHistory();
      if (r.translated || r.failed) {
        const firstError = r.errors?.[0];
        setStatus(`标题翻译完成：${r.translated}${r.failed ? ` · 失败 ${r.failed}${firstError ? `：${firstError}` : ""}` : ""}`, r.failed ? "error" : "done");
      }
    } finally {
      // Only a fully successful batch drains further. A failure stops this
      // run and leaves failed papers eligible for a later launch/sync/manual retry.
      if (continueDraining) window.setTimeout(() => { void scheduleMissingTitleBacklog(); }, 0);
    }
  });
  await listen("title-translation://started", (e) => {
    const r = e.payload as { scheduled: number; paperIds: number[] };
    missingTitleLastProgressAt = Date.now();
    console.info("title-only translation started", r);
    setStatus(`正在翻译 ${r.scheduled} 篇缺摘要论文标题…`, "running");
  });
  await listen("title-translation://progress", (e) => {
    const progress = e.payload as { paperId?: number; attempt?: number; stage: string; elapsedMs: number; error?: string };
    missingTitleLastProgressAt = Date.now();
    console.info("title-only translation progress", progress);
  });
  await listen("title-translation://fatal", (e) => {
    const r = e.payload as { error?: string };
    releaseMissingTitleState();
    setStatus(`标题翻译任务异常终止：${r.error || "未知错误"}`, "error");
  });

  await listen("ai://progress", (e) => {
    aiStatus = e.payload as AiStatus;
    renderWorkCenter();
    renderBacklog();
  });
  await listen("ai://retry", async () => {
    await refreshWorkState();
  });
  await listen("ai://error", (e) => setStatus(`AI：${e.payload}`, "error"));
  await listen("ai://finished", async () => {
    setStatus("AI 分析批次结束", "done");
    // 必须统一刷新：papers（论文列表/推荐）+ 工作状态（pending 计数立即归零，
    // 杜绝"AI 待处理 7"与"无待处理"并存）+ 今日推荐（Round 6）
    await loadPapers();
    await refreshWorkState();
    await refreshRecommendations();
  });

  installLibraryInteractions();

  // Catalog 详情 checkbox 选择（change 冒泡 → 委托处理）
  document.addEventListener("input", (ev) => {
    const el = ev.target as HTMLInputElement;
    const action = el.dataset.action;
    if (action === "tag-draft-name" || action === "tag-draft-desc") {
      const i = parseInt(el.dataset.idx!, 10);
      if (isNaN(i) || !tagDraft[i]) return;
      if (action === "tag-draft-name") tagDraft[i].name = el.value;
      else tagDraft[i].description = el.value;
      setTagDirty(true);
    }
    if (el.matches("#set-pdf-library-root, #set-pdf-naming-template")) renderPdfTemplateExample();
  });
  document.addEventListener("change", (ev) => {
    const el = ev.target as HTMLInputElement;
    // 中文 IME 兜底：input 事件在 composition 期间可能延迟/丢失，
    // change 在失焦/回车时可靠触发，确保 draft 始终同步（否则 dirty=false → 按钮 disabled → 点击无反应）
    const action = el.dataset.action;
    if (action === "library-toggle-column") {
      const column = el.dataset.column as LibraryColumn;
      if (!LIBRARY_COLUMNS.includes(column)) return;
      if (column === "title") return;
      if (el.checked) libraryHiddenColumns.delete(column);
      else if (libraryVisibleColumns().length > 1) libraryHiddenColumns.add(column);
      persistLibraryLayout();
      renderLibrary();
      return;
    }
    if (action === "tag-draft-name" || action === "tag-draft-desc") {
      const i = parseInt(el.dataset.idx!, 10);
      if (isNaN(i) || !tagDraft[i]) return;
      if (action === "tag-draft-name") tagDraft[i].name = el.value;
      else tagDraft[i].description = el.value;
      setTagDirty(true);
      return;
    }
    if (el.matches("#set-pdf-file-handling-mode, #set-pdf-library-root, #set-pdf-naming-template, #set-pdf-subfolder-rule")) {
      renderPdfTemplateExample();
      return;
    }
    if (el.matches("[data-action='tag-draft-toggle']")) {
      const i = parseInt(el.dataset.idx!, 10);
      if (!isNaN(i) && tagDraft[i]) {
        tagDraft[i].enabled = el.checked;
        setTagDirty(true);
      }
      return;
    }
    if (el.matches("#add-member-panel input[type=checkbox]")) {
      if (!addMemberState) return;
      const id = parseInt(el.dataset.journalId!, 10);
      if (isNaN(id)) return;
      if (el.checked) addMemberState.checked.add(id);
      else addMemberState.checked.delete(id);
      renderAddMemberPanel();
      return;
    }
    if (!el.matches("#catalog-detail input[type=checkbox]:not(:disabled)")) return;
    const id = parseInt(el.dataset.journalId!, 10);
    if (isNaN(id)) return;
    if (el.checked) catalogChecked.add(id);
    else catalogChecked.delete(id);
    updateCatalogSelected();
  });

  document.addEventListener("click", async (ev) => {
    const t = ev.target as HTMLElement;
    const workspaceTab = t.closest("[data-workspace]") as HTMLElement | null;
    if (workspaceTab) {
      const workspace = workspaceTab.dataset.workspace as "discovery" | "library";
      if (workspace === "library") {
        libraryScope = null;
        libraryInspectorCollapsed = false;
        doSwitch("library-all");
        await loadLibraryData("all");
      } else {
        doSwitch("recommend");
      }
      return;
    }
    const nav = t.closest(".nav-item") as HTMLElement | null;
    if (nav) {
      switchView(nav.dataset.view!);
      return;
    }
    if (t.closest("[data-action='select-pdf-library-root']")) {
      await selectPdfLibraryRoot();
      return;
    }
    if (t.closest("[data-action='reset-pdf-library-root']")) {
      resetPdfLibraryRoot();
      return;
    }
    const pdfToken = t.closest("[data-pdf-token]") as HTMLElement | null;
    if (pdfToken) {
      const token = pdfToken.dataset.pdfToken;
      if (token) insertPdfTemplateToken(token);
      return;
    }
    if (t.closest("[data-action='library-import-pdf']")) {
      await importExternalPdf();
      return;
    }
    if (t.closest("[data-action='library-inline-action-confirm']")) {
      libraryInlineActionResolver?.(true);
      return;
    }
    if (t.closest("[data-action='library-inline-action-cancel']")) {
      libraryInlineActionResolver?.(false);
      return;
    }
    if (t.closest("[data-action='library-columns']")) {
      toggleLibraryColumnMenu();
      return;
    }
    if (t.closest("[data-action='library-reset-columns']")) {
      resetLibraryColumns();
      setStatus("已恢复默认列设置", "done");
      return;
    }
    if (t.closest("[data-action='library-toggle-attachments']")) {
      const disclosure = t.closest<HTMLElement>("[data-action='library-toggle-attachments']")!;
      const paperId = Number(disclosure.dataset.paperId);
      if (expandedLibraryAttachmentPaperIds.has(paperId)) expandedLibraryAttachmentPaperIds.delete(paperId);
      else expandedLibraryAttachmentPaperIds.add(paperId);
      renderLibrary();
      return;
    }
    if (t.closest("[data-action='library-inline-create-submit']")) {
      await submitLibraryInlineCreate();
      return;
    }
    if (t.closest("[data-action='library-inline-create-cancel']")) {
      libraryInlineCreate = null;
      renderLibraryNavigation();
      return;
    }
    const addLibrary = t.closest("[data-action='add-library']") as HTMLElement | null;
    if (addLibrary) {
      try { await openLibraryCapture(parseInt(addLibrary.dataset.paperId!, 10)); }
      catch (err) { setStatus(`打开收录面板失败：${String(err)}`, "error"); }
      return;
    }
    if (t.closest("[data-action='open-library']")) {
      const card = t.closest("[data-paper-id]") as HTMLElement | null;
      libraryScope = null;
      libraryInspectorCollapsed = false;
      doSwitch("library-all");
      await loadLibraryData("all");
      if (card) {
        selectedLibraryPaperId = parseInt(card.dataset.paperId!, 10);
        renderLibrary();
      }
      return;
    }
    if (t.closest("[data-action='library-capture-create-collection']")) {
      const name = await showPromptModal("新建文献夹", "名称");
      if (!name || libraryCapturePaperId == null) return;
      await invoke("create_library_collection", { name, parentId: null });
      await loadLibraryData(libraryView);
      await openLibraryCapture(libraryCapturePaperId);
      return;
    }
    if (t.closest("[data-action='library-capture-create-tag']")) {
      const name = await showPromptModal("新建文献标签", "名称");
      if (!name || libraryCapturePaperId == null) return;
      await invoke("create_library_tag", { name, color: null });
      await loadLibraryData(libraryView);
      await openLibraryCapture(libraryCapturePaperId);
      return;
    }
    if (t.closest("[data-action='library-capture-cancel']")) { closeLibraryCapture(); return; }
    if (t.closest("[data-action='library-capture-submit']")) {
      try { await submitLibraryCapture(); } catch (err) { setStatus(`收录失败：${String(err)}`, "error"); }
      return;
    }
    const attachPdf = t.closest("[data-action='attach-pdf']") as HTMLElement | null;
    if (attachPdf) {
      await attachPdfToPaper(Number(attachPdf.dataset.paperId));
      return;
    }
    const inlineEdit = t.closest("[data-action='library-inline-edit']") as HTMLElement | null;
    if (inlineEdit) {
      beginLibraryInlineEdit(Number(inlineEdit.dataset.paperId), inlineEdit.dataset.field as LibraryInlineField, inlineEdit);
      return;
    }
    if (t.closest("[data-action='library-close-inspector']")) {
      libraryInspectorCollapsed = true;
      renderLibrary();
      return;
    }
    if (t.closest("[data-action='library-open-inspector']")) {
      libraryInspectorCollapsed = false;
      renderLibrary();
      return;
    }
    if (t.closest("[data-action='library-toggle-inspector']")) {
      libraryInspectorCollapsed = !libraryInspectorCollapsed;
      renderLibrary();
      return;
    }
    const abstractLanguage = t.closest("[data-action='library-abstract-lang']") as HTMLElement | null;
    if (abstractLanguage) {
      libraryInspectorAbstractLang = abstractLanguage.dataset.lang === "en" ? "en" : "zh";
      const item = libraryPapers.find((candidate) => candidate.paper.id === selectedLibraryPaperId);
      if (item) renderLibraryInspector(item);
      return;
    }
    const librarySelect = t.closest("[data-action='library-select-paper']") as HTMLElement | null;
    if (librarySelect) {
      if (librarySuppressNextClick) {
        librarySuppressNextClick = false;
        return;
      }
      selectedLibraryPaperId = parseInt(librarySelect.dataset.paperId!, 10);
      libraryInspectorCollapsed = false;
      libraryInspectorAbstractLang = libraryPapers.find(item => item.paper.id === selectedLibraryPaperId)?.effectiveChineseAbstract?.trim() ? "zh" : "en";
      renderLibrary();
      return;
    }
    const collectionFilter = t.closest("[data-action='library-filter-collection']") as HTMLElement | null;
    if (collectionFilter) {
      libraryScope = { kind: "collection", id: parseInt(collectionFilter.dataset.collectionId!, 10) };
      renderLibraryNavigation();
      if (libraryView !== "all") await loadLibraryData("all"); else renderLibrary();
      return;
    }
    const renameCollection = t.closest("[data-action='library-rename-collection']") as HTMLElement | null;
    if (renameCollection) {
      const id = Number(renameCollection.dataset.collectionId);
      const collection = libraryCollections.find((item) => item.id === id);
      const name = await showPromptModal("重命名文献夹", "名称", collection?.name || "");
      if (!name) return;
      try {
        await invoke("rename_library_collection", { id, name });
        await loadLibraryData(libraryView);
        setStatus("文献夹已重命名", "done");
      } catch (err) {
        setStatus(`重命名文献夹失败：${String(err)}`, "error");
      }
      return;
    }
    const deleteCollection = t.closest("[data-action='library-delete-collection']") as HTMLElement | null;
    if (deleteCollection) {
      const id = Number(deleteCollection.dataset.collectionId);
      const collection = libraryCollections.find((item) => item.id === id);
      if (libraryCollections.some(item => item.parentId === id)) {
        setStatus("请先移动或删除子文集，再删除此文集", "error");
        return;
      }
      const ok = await requestLibraryInlineAction(`删除「${collection?.name || "此文集"}」？文献仍保留。`, "删除文集", "取消");
      if (!ok) return;
      try {
        await invoke("delete_library_collection", { id });
        if (libraryScope?.kind === "collection" && libraryScope.id === id) libraryScope = null;
        await loadLibraryData(libraryView);
        setStatus("文献夹已删除，文献仍保留", "done");
      } catch (err) {
        setStatus(`删除文献夹失败：${String(err)}`, "error");
      }
      return;
    }
    const tagFilter = t.closest("[data-action='library-filter-tag']") as HTMLElement | null;
    if (tagFilter) {
      libraryScope = { kind: "tag", id: parseInt(tagFilter.dataset.tagId!, 10) };
      renderLibraryNavigation();
      if (libraryView !== "all") await loadLibraryData("all"); else renderLibrary();
      return;
    }
    const renameLibraryTag = t.closest("[data-action='library-rename-tag']") as HTMLElement | null;
    if (renameLibraryTag) {
      const id = Number(renameLibraryTag.dataset.tagId);
      const tag = libraryTags.find((item) => item.id === id);
      const name = await showPromptModal("重命名文献标签", "名称", tag?.name || "");
      if (!name) return;
      try {
        await invoke("rename_library_tag", { id, name });
        await loadLibraryData(libraryView);
        setStatus("文献标签已重命名", "done");
      } catch (err) {
        setStatus(`重命名文献标签失败：${String(err)}`, "error");
      }
      return;
    }
    const deleteLibraryTag = t.closest("[data-action='library-delete-tag']") as HTMLElement | null;
    if (deleteLibraryTag) {
      const id = Number(deleteLibraryTag.dataset.tagId);
      const tag = libraryTags.find((item) => item.id === id);
      const ok = await requestLibraryInlineAction(`删除「${tag?.name || "此 Library Tag"}」？论文仍保留。`, "删除标签", "取消");
      if (!ok) return;
      try {
        await invoke("delete_library_tag", { id });
        if (libraryScope?.kind === "tag" && libraryScope.id === id) libraryScope = null;
        await loadLibraryData(libraryView);
        setStatus("文献标签已删除，论文仍保留", "done");
      } catch (err) {
        setStatus(`删除文献标签失败：${String(err)}`, "error");
      }
      return;
    }
    if (t.closest("[data-action='library-reset-columns']")) {
      libraryColumnWidths = { ...LIBRARY_COLUMN_DEFAULT };
      persistLibraryLayout(); applyLibraryLayoutMetrics();
      setStatus("已恢复默认列宽", "done"); return;
    }
    const relationAction = t.closest<HTMLElement>("[data-action^='library-relation-']");
    if (relationAction) {
      const paperId = Number(relationAction.dataset.paperId);
      const kind = relationAction.dataset.kind === "collection" ? "collection" : "tag";
      relationAction.setAttribute("disabled", "");
      try {
        if (relationAction.dataset.action === "library-relation-create") {
          const input = $(`library-new-${kind}-name`) as HTMLInputElement;
          const name = input.value.trim();
          if (!name) { input.focus(); return; }
          const parent = kind === "collection" ? ($("library-new-collection-parent") as HTMLSelectElement).value : "";
          const created = await invoke<{ id: number }>(kind === "collection" ? "create_library_collection" : "create_library_tag", kind === "collection" ? { name, parentId: parent ? Number(parent) : null } : { name, color: null });
          await addLibraryRelation(paperId, kind, created.id);
        } else if (relationAction.dataset.action === "library-relation-add") {
          await addLibraryRelation(paperId, kind, Number(relationAction.dataset.id));
        } else {
          const membership = await invoke<LibraryMembership | null>("get_library_membership", { paperId });
          if (!membership) throw new Error("文献已移出文献库");
          const ids = (kind === "collection" ? membership.collectionIds : membership.tagIds).filter(id => id !== Number(relationAction.dataset.id));
          await invoke(kind === "collection" ? "set_paper_collections" : "set_paper_library_tags", kind === "collection" ? { paperId, collectionIds: ids } : { paperId, tagIds: ids });
          await loadLibraryData(libraryView);
          setStatus("已移除论文关系", "done");
        }
      } catch (error) { setStatus(`更新失败：${String(error)}`, "error"); }
      finally { relationAction.removeAttribute("disabled"); }
      return;
    }
    const translateTitle = t.closest<HTMLButtonElement>("[data-action='library-translate-title']");
    if (translateTitle) {
      if (!(await hasKey())) { setStatus("请先在设置中保存 DeepSeek API Key", "error"); return; }
      translateTitle.disabled = true; translateTitle.textContent = "翻译中…";
      try {
        await invoke("translate_library_title", { paperId: Number(translateTitle.dataset.paperId), model: getModel() });
        await loadLibraryData(libraryView); setStatus("中文标题已保存", "done");
      } catch (error) { setStatus(`中文标题翻译失败：${String(error)}`, "error"); translateTitle.disabled = false; translateTitle.textContent = "翻译中文标题"; }
      return;
    }
    if (t.closest("[data-action='library-refresh']")) { await loadLibraryData(libraryView); return; }
    const libraryRemove = t.closest("[data-action='library-remove']") as HTMLElement | null;
    if (libraryRemove) {
      const ok = await requestLibraryInlineAction("移出文献库？论文与原始 PDF 均保留。", "移出", "取消");
      if (!ok) return;
      await invoke("remove_paper_from_library", { paperId: parseInt(libraryRemove.dataset.paperId!, 10) });
      selectedLibraryPaperId = null;
      await Promise.all([loadPapers(), loadLibraryData(libraryView)]);
      setStatus("已移出文献库", "done");
      return;
    }
    const translateAbstract = t.closest("[data-action='library-translate-abstract']") as HTMLElement | null;
    if (translateAbstract) {
      const paperId = Number(translateAbstract.dataset.paperId);
      if (!(await hasKey())) {
        setStatus("请先在设置中保存 DeepSeek API Key，再翻译中文摘要", "error");
        return;
      }
      const translateButton = translateAbstract as HTMLButtonElement;
      translateButton.disabled = true;
      translateButton.textContent = "翻译中…";
      try {
        setStatus("正在翻译中文摘要…", "running");
        await invoke("translate_library_abstract", { paperId, model: getModel() });
        await loadLibraryData(libraryView);
        setStatus("中文摘要已保存为 Library personal translation", "done");
      } catch (error) {
        setStatus(`中文摘要翻译失败：${String(error)}`, "error");
        translateButton.disabled = false;
        translateButton.textContent = "翻译为中文";
      }
      return;
    }
    const attachLibraryPdf = t.closest("[data-action='library-attach-pdf']") as HTMLElement | null;
    if (attachLibraryPdf) {
      await attachPdfToPaper(Number(attachLibraryPdf.dataset.paperId));
      return;
    }
    const openPdf = t.closest("[data-action='library-open-pdf']") as HTMLElement | null;
    if (openPdf) {
      const openButton = openPdf as HTMLButtonElement;
      openButton.disabled = true;
      openButton.textContent = "打开中…";
      try {
        setStatus("正在打开 PDF…", "running");
        await invoke("open_pdf", { attachmentId: Number(openPdf.dataset.attachmentId) });
        setStatus("PDF 已打开", "done");
      } catch (error) {
        setStatus(`打开 PDF 失败：${String(error)}`, "error");
        openButton.disabled = false;
        openButton.textContent = "打开";
      }
      return;
    }
    const revealPdf = t.closest("[data-action='library-reveal-pdf']") as HTMLElement | null;
    if (revealPdf) {
      const revealButton = revealPdf as HTMLButtonElement;
      revealButton.disabled = true;
      revealButton.textContent = "显示中…";
      try {
        setStatus("正在显示 PDF 文件位置…", "running");
        await invoke("reveal_pdf", { attachmentId: Number(revealPdf.dataset.attachmentId) });
        setStatus("已显示 PDF 文件位置", "done");
      } catch (error) {
        setStatus(`显示 PDF 文件位置失败：${String(error)}`, "error");
        revealButton.disabled = false;
        revealButton.textContent = "显示位置";
      }
      return;
    }
    const relinkPdf = t.closest("[data-action='library-relink-pdf']") as HTMLElement | null;
    if (relinkPdf) {
      let path: string | null = null;
      try {
        path = await pickPdfPath();
      } catch (error) {
        setStatus(`PDF 文件选择器打开失败：${String(error)}`, "error");
        return;
      }
      if (!path) return;
      const relinkButton = relinkPdf as HTMLButtonElement;
      relinkButton.disabled = true;
      relinkButton.textContent = "链接中…";
      try {
        setStatus("正在重新链接 PDF…", "running");
        await invoke("relink_pdf", { attachmentId: Number(relinkPdf.dataset.attachmentId), path });
        await loadLibraryData(libraryView);
        setStatus("PDF 已重新链接", "done");
      } catch (error) {
        setStatus(`重新链接 PDF 失败：${String(error)}`, "error");
        relinkButton.disabled = false;
        relinkButton.textContent = "重新链接";
      }
      return;
    }
    const detachPdf = t.closest("[data-action='library-detach-pdf']") as HTMLElement | null;
    if (detachPdf) {
      const confirmed = await requestLibraryInlineAction("解除 PDF 关联？原始文件不会被删除。", "解除关联", "取消");
      if (!confirmed) return;
      const detachButton = detachPdf as HTMLButtonElement;
      detachButton.disabled = true;
      detachButton.textContent = "解除中…";
      try {
        setStatus("正在解除 PDF 关联…", "running");
        await invoke("detach_pdf", { attachmentId: Number(detachPdf.dataset.attachmentId) });
        await loadLibraryData(libraryView);
        setStatus("PDF 关联已解除，原始文件保留", "done");
      } catch (error) {
        setStatus(`解除 PDF 关联失败：${String(error)}`, "error");
        detachButton.disabled = false;
        detachButton.textContent = "解除关联";
      }
      return;
    }
    if (t.closest("[data-action='library-create-collection']")) {
      beginLibraryInlineCreate("collection", null);
      return;
    }
    const childCollection = t.closest("[data-action='library-create-child']") as HTMLElement | null;
    if (childCollection) {
      beginLibraryInlineCreate("collection", Number(childCollection.dataset.parentId));
      return;
    }
    if (t.closest("[data-action='library-create-tag']")) {
      beginLibraryInlineCreate("tag", null);
      return;
    }
    const open = t.closest("[data-action='open']") as HTMLElement | null;
    if (open) {
      ev.preventDefault();
      await openUrl(open.dataset.url!);
      return;
    }
    const syncOne = t.closest("[data-action='sync-one']") as HTMLElement | null;
    if (syncOne) {
      await startSync([parseInt(syncOne.dataset.id!, 10)], "journalTest");
      return;
    }
    const toggle = t.closest("[data-action='toggle']") as HTMLElement | null;
    if (toggle) {
      const id = parseInt(toggle.dataset.id!, 10);
      const j = journals.find((x) => x.id === id);
      if (j) {
        await invoke("set_journal_enabled", { id, enabled: !j.enabled });
        await loadJournals();
      }
      return;
    }
    const del = t.closest("[data-action='delete']") as HTMLElement | null;
    if (del) {
      const ok = await showConfirmModal({
        title: "删除期刊",
        message: "删除该期刊及其所有论文？此操作不可撤销。",
        confirmText: "删除",
        cancelText: "取消",
      });
      if (!ok) return;
      await invoke("delete_journal", { id: parseInt(del.dataset.id!, 10) });
      await loadJournals();
      await loadPapers();
      await refreshWorkState();
      return;
    }
    const fav = t.closest("[data-action='fav'], [data-action='toggle-favorite']") as HTMLElement | null;
    if (fav) {
      const id = parseInt(fav.dataset.paperId || fav.dataset.id!, 10);
      const p = papers.find((x) => x.id === id) ?? recPapers.find((x) => x.id === id);
      if (p) {
        await setFlag(id, "favorite", !p.isFavorite);
        if (recPapers.some((x) => x.id === id)) await renderRecommend();
      }
      return;
    }
    const read = t.closest("[data-action='read']") as HTMLElement | null;
    if (read) {
      const id = parseInt(read.dataset.id!, 10);
      const p = papers.find((x) => x.id === id) ?? recPapers.find((x) => x.id === id);
      if (p) {
        await setFlag(id, "read", !p.isRead);
        if (recPapers.some((x) => x.id === id)) await renderRecommend();
      }
      return;
    }
    const ignore = t.closest("[data-action='ignore']") as HTMLElement | null;
    if (ignore) {
      const id = parseInt(ignore.dataset.id!, 10);
      const p = papers.find((x) => x.id === id) ?? recPapers.find((x) => x.id === id);
      if (p) {
        await setFlag(id, "ignored", !p.isIgnored);
        if (recPapers.some((x) => x.id === id)) await renderRecommend();
      }
      return;
    }
    const absLang = t.closest("[data-action='toggle-paper-lang']") as HTMLElement | null;
    if (absLang) {
      const lang = absLang.dataset.lang as "zh" | "en";
      const card = absLang.closest(".paper-card") as HTMLElement | null;
      const cardInstanceId = card?.dataset.cardInstanceId;
      if (!cardInstanceId) return;
      cardLanguageState.set(cardInstanceId, lang);
      card.outerHTML = renderCardInstance(card);
      return;
    }
    const absExpand = t.closest("[data-action='toggle-paper-abstract']") as HTMLElement | null;
    if (absExpand) {
      const card = absExpand.closest(".paper-card") as HTMLElement | null;
      const cardInstanceId = card?.dataset.cardInstanceId;
      if (!cardInstanceId) return;
      if (expandedCardInstanceIds.has(cardInstanceId)) expandedCardInstanceIds.delete(cardInstanceId);
      else expandedCardInstanceIds.add(cardInstanceId);
      card.outerHTML = renderCardInstance(card);
      return;
    }
    const recoverAbstract = t.closest("[data-action='recover-paper-abstract']") as HTMLElement | null;
    if (recoverAbstract) {
      const id = parseInt(recoverAbstract.dataset.paperId!, 10);
      try {
        setStatus("正在重新获取摘要…", "running");
        const b = await invoke<AbstractRecoveryBatch>("recover_paper_abstract", { paperId: id });
        setStatus(`正在补全摘要 · 0/${b.total}`, "running");
      } catch (err) { setStatus(`摘要恢复失败：${String(err)}`, "error"); }
      return;
    }
    if (t.closest("[data-action='translate-missing-titles']")) {
      const scheduled = await startMissingTitleTranslation(false);
      if (!scheduled && !missingTitleBacklogInFlight) setStatus("没有需要翻译的缺摘要论文标题", "done");
      return;
    }
    const scopedRecovery = t.closest("[data-action='recover-scoped-abstracts']") as HTMLElement | null;
    if (scopedRecovery) {
      const ids = (scopedRecovery.dataset.paperIds || "").split(",").map(Number).filter(Number.isInteger);
      const label = scopedRecovery.dataset.recoveryLabel || "当前列表";
      const ok = await showConfirmModal({ title: `重新获取${label}摘要`, message: "仅检查当前页面显示的缺摘要论文；将从 Crossref、OpenAlex 和官方 publisher 页面重新尝试获取公开摘要，不会调用 AI。", confirmText: "开始获取", cancelText: "取消" });
      if (!ok) return;
      try {
        setStatus(`正在获取摘要 · 0/${ids.length}`, "running");
        const b = await invoke<AbstractRecoveryBatch>("recover_scoped_abstracts", { paperIds: ids });
        setStatus(`正在补全摘要 · 0/${b.total}`, "running");
      } catch (err) { setStatus(`摘要补全失败：${String(err)}`, "error"); }
      return;
    }
    // Tag Draft Editor 操作（Round 6.5：只改 draft，不写 DB）
    if (t.closest("[data-action='tag-draft-add']")) {
      tagDraft.push({ id: 0, name: "", description: "", enabled: true, deleted: false });
      renderTagEditor();
      return;
    }
    const tagDel = t.closest("[data-action='tag-draft-delete']") as HTMLElement | null;
    if (tagDel) {
      const i = parseInt(tagDel.dataset.idx!, 10);
      if (!isNaN(i) && tagDraft[i]) {
        tagDraft.splice(i, 1);
        renderTagEditor();
      }
      return;
    }
    if (t.closest("[data-action='save-tag-config-scheduled']")) {
      await saveTagConfigScheduled();
      return;
    }
    if (t.closest("[data-action='activate-tag-config-now']")) {
      await activateTagConfigNow();
      return;
    }
    // AI 控制
    if (t.closest("[data-action='ai-pause']")) { await pauseAi(); return; }
    if (t.closest("[data-action='ai-resume']")) { await resumeAi(); return; }
    if (t.closest("[data-action='ai-stop']")) { await stopAi(); return; }
    if (t.closest("[data-action='ai-retry']")) {
      const el = t.closest("[data-action='ai-retry']") as HTMLElement;
      const parent = el.dataset.batch ? parseInt(el.dataset.batch, 10) : null;
      await retryFailed(parent);
      return;
    }
    if (t.closest("[data-action='ai-backlog']")) {
      await manualAnalyze();
      return;
    }
    // Catalog 详情操作（统一事件委托；返回按钮不再依赖 render 后绑定）
    if (t.closest("[data-action='catalog-select-unsub']")) {
      // 全选未订阅：勾选所有未订阅项（已订阅 disabled 不可选）
      catalogChecked.clear();
      document.querySelectorAll("#catalog-detail input[type=checkbox]:not(:disabled)").forEach((el) => {
        const id = parseInt((el as HTMLInputElement).dataset.journalId!, 10);
        if (!isNaN(id)) catalogChecked.add(id);
        (el as HTMLInputElement).checked = true;
      });
      updateCatalogSelected();
      return;
    }
    if (t.closest("[data-action='catalog-clear']")) {
      catalogChecked.clear();
      document.querySelectorAll("#catalog-detail input[type=checkbox]:not(:disabled)").forEach((el) => {
        (el as HTMLInputElement).checked = false;
      });
      updateCatalogSelected();
      return;
    }
    if (t.closest("[data-action='catalog-subscribe']")) {
      await doCatalogSubscribe();
      return;
    }
    if (t.closest("[data-action='create-collection']")) {
      const name = await showPromptModal("新建期刊集合", "集合名称，如 数字平台");
      if (!name) return;
      try {
        await invoke("create_user_collection", { name });
      } catch (err) {
        setStatus(String(err), "error");
        return;
      }
      setStatus(`已创建集合「${name}」`, "done");
      await renderCatalogCollections();
      return;
    }
    const renameBtn = t.closest("[data-action='rename-collection']") as HTMLElement | null;
    if (renameBtn) {
      const id = parseInt(renameBtn.dataset.collectionId!, 10);
      const coll = catalogCollections.find((c) => c.id === id);
      const name = await showPromptModal("重命名集合", "新名称", coll?.name || "");
      if (!name) return;
      try {
        await invoke("rename_collection", { id, name });
      } catch (err) {
        setStatus(String(err), "error");
        return;
      }
      await renderCatalogCollections();
      if (selectedCatalogCode) await renderCatalogDetail(selectedCatalogCode);
      return;
    }
    const delColl = t.closest("[data-action='delete-collection']") as HTMLElement | null;
    if (delColl) {
      const id = parseInt(delColl.dataset.collectionId!, 10);
      const ok = await showConfirmModal({
        title: "删除集合",
        message: "删除该集合？\n只删除集合与成员关系，不删除期刊、不取消订阅、不删除论文。",
        confirmText: "删除",
        cancelText: "取消",
      });
      if (!ok) return;
      try {
        await invoke("delete_collection", { id });
      } catch (err) {
        setStatus(String(err), "error");
        return;
      }
      selectedCatalogCode = null;
      catalogChecked.clear();
      $("catalog-detail").classList.add("hidden");
      closeAddMemberPanel();
      await renderCatalogCollections();
      return;
    }
    const rmMember = t.closest("[data-action='remove-collection-member']") as HTMLElement | null;
    if (rmMember) {
      const collectionId = parseInt(rmMember.dataset.collectionId!, 10);
      const journalId = parseInt(rmMember.dataset.journalId!, 10);
      try {
        await invoke("remove_collection_member", { collectionId, journalId });
      } catch (err) {
        setStatus(String(err), "error");
        return;
      }
      await renderCatalogDetail(selectedCatalogCode!);
      return;
    }
    if (t.closest("[data-action='add-member-open']")) {
      const btn = t.closest("[data-action='add-member-open']") as HTMLElement;
      await openAddMemberPanel(parseInt(btn.dataset.collectionId!, 10), btn.dataset.collectionCode!);
      return;
    }
    if (t.closest("[data-action='add-member-close']")) {
      closeAddMemberPanel();
      return;
    }
    if (t.closest("[data-action='add-member-submit']")) {
      if (!addMemberState) return;
      const ids = [...addMemberState.checked];
      for (const jid of ids) {
        await invoke("add_collection_member", { collectionId: addMemberState.collectionId, journalId: jid });
      }
      setStatus(`已添加 ${ids.length} 本期刊到集合`, "done");
      closeAddMemberPanel();
      await renderCatalogDetail(selectedCatalogCode!);
      return;
    }
    const todayTab = t.closest("[data-action='today-tab']") as HTMLElement | null;
    if (todayTab) {
      todayView = todayTab.dataset.tab as "recommend" | "missing";
      await renderRecommend();
      return;
    }
    const historyDay = t.closest("[data-action='open-history-day']") as HTMLElement | null;
    if (historyDay) {
      historyCycleKey = historyDay.dataset.cycleKey!;
      historyTab = "recommend";
      await renderRecommendHistory();
      return;
    }
    const historyTabEl = t.closest("[data-action='history-tab']") as HTMLElement | null;
    if (historyTabEl) {
      historyTab = historyTabEl.dataset.tab as "recommend" | "missing";
      await renderRecommendHistory();
      return;
    }
    if (t.closest("[data-action='history-back']")) {
      showHistoryOverview();
      return;
    }
    const catalogCol = t.closest("[data-catalog-code]") as HTMLElement | null;
    if (catalogCol) {
      closeAddMemberPanel();
      await renderCatalogDetail(catalogCol.dataset.catalogCode!);
      return;
    }
    if (t.closest("[data-action='open-activity']")) {
      $("work-status-popover").classList.add("hidden");
      $("work-status").setAttribute("aria-expanded", "false");
      switchView("activity");
      return;
    }
    if (t.closest("#work-status")) {
      const popover = $("work-status-popover");
      const open = popover.classList.toggle("hidden");
      $("work-status").setAttribute("aria-expanded", String(!open));
      return;
    }
    if (!t.closest(".toolbar-status-wrap")) {
      $("work-status-popover").classList.add("hidden");
      $("work-status").setAttribute("aria-expanded", "false");
    }
    if (t.closest("#btn-sync-main")) {
      await startSync(null);
      return;
    }
    if (t.closest("#btn-ai-main")) {
      await workAiAction();
      return;
    }
    const actItem = t.closest("[data-activity-type]") as HTMLElement | null;
    if (actItem) {
      selectedActivity = { type: actItem.dataset.activityType!, id: parseInt(actItem.dataset.activityId!, 10) };
      renderActivityHistory();
      await renderActivityDetail();
      return;
    }
    if (t.closest("[data-action='manual-analyze']")) {
      await manualAnalyze();
      await refreshWorkState();
      return;
    }
    if (t.closest("[data-action='retry-failed']")) {
      await retryFailed(null);
      await refreshWorkState();
      return;
    }
  });

  document.addEventListener("dblclick", (ev) => {
    const target = ev.target as HTMLElement;
    if (target.closest(".attachment-child-actions")) return;
    const child = target.closest<HTMLElement>(".library-attachment-child");
    const openButton = child?.querySelector<HTMLButtonElement>("[data-action='library-open-pdf']");
    if (!openButton || openButton.disabled) return;
    ev.preventDefault();
    openButton.click();
  });
}

window.addEventListener("DOMContentLoaded", () => {
  $("btn-settings-global").addEventListener("click", () => switchView("settings"));
  $("add-form").addEventListener("submit", addJournalHandler);
  $("btn-refresh").addEventListener("click", loadPapers);
  $("journal-filter").addEventListener("change", renderPapers);
  $("ai-filter").addEventListener("change", renderPapers);
  $("abs-filter").addEventListener("change", renderPapers);
  $("coll-filter").addEventListener("change", renderPapers);
  $("flag-filter").addEventListener("change", renderPapers);
  $("btn-save-key").addEventListener("click", saveKey);
  $("btn-test").addEventListener("click", testConnection);
  $("btn-clear-key").addEventListener("click", async () => {
    try {
      await invoke("delete_api_key");
      ($("api-key") as HTMLInputElement).value = "";
      setDeepSeekStatus("已删除本机保存的 Key", "muted small");
      await refreshKeyStatus();
    } catch (err) {
      setDeepSeekStatus(String(err), "error");
    }
  });
  $("btn-save-settings").addEventListener("click", saveSettings);
  $("btn-check-update").addEventListener("click", checkForUpdates);
  $("btn-install-update").addEventListener("click", installPendingUpdate);
  $("journal-search").addEventListener("input", renderJournals);
  $("catalog-search").addEventListener("input", () => {
    if (selectedCatalogCode) renderCatalogRows();
  });
  $("catalog-unsub-only").addEventListener("change", () => {
    if (selectedCatalogCode) {
      catalogChecked.clear();
      renderCatalogRows();
    }
  });
  $("tab-common").addEventListener("click", () => setJournalTab("catalog"));
  $("tab-manual").addEventListener("click", () => setJournalTab("manual"));

  // Key 保存在本地 secret 文件，不回填到输入框（输入框仅用于「替换 Key」时输入）
  ($("api-key") as HTMLInputElement).value = "";
  ($("model") as HTMLInputElement).value = getModel();

  (async () => {
    await setupListeners();
    await Promise.all([loadJournals(), loadSettings(), loadCurrentVersion()]);
    await loadPapers();
    await loadLibraryData("all");
    // 统一工作状态刷新（Work Center / 积压 / 待处理区 / 计数）
    await refreshWorkState();
    // 每日推荐：启动时前滚周期并填充当前推荐（Rust 侧已 ensure；此处同步 UI）
    await refreshRecommendations();
    renderNextCheck();
    await refreshKeyStatus();
    // Title-only translations use the same automatic-AI preference as
    // post-sync analysis. This starts one rate-limited historical backlog
    // batch even when no new papers are discovered this session.
    await scheduleMissingTitleBacklog();
    // 启动自动同步（阈值判断在 Rust 端）
    await invoke("maybe_auto_sync").catch(() => {});
  })();
});

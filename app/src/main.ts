import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
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

interface Paper {
  id: number;
  journalId: number;
  journalName: string | null;
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
let aiStatus: AiStatus = emptyAiStatus();
let activity: ActivityState = emptyActivity();
let settings: Settings | null = null;
let abstractLang: "zh" | "en" = "zh";
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

async function loadAiStatus() {
  try {
    aiStatus = await invoke<AiStatus>("get_ai_status");
  } catch {
    aiStatus = emptyAiStatus();
  }
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
  }
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
        <button class="ghost small" data-action="toggle-favorite" data-paper-id="${p.id}">${p.isFavorite ? "★ 收藏" : "☆ 收藏"}</button>
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
    : '<li class="empty">暂无收藏。在论文卡片上点 ☆ 收藏。</li>';
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
  let html: string;
  let cls: string;

  if (syncRunning) {
    // 同步优先：进行中显示进度，禁止同时高强调"待分析 N"
    cls = "running";
    html = `正在检查新论文 · ${a.syncBatch!.journalCompleted}/${a.syncBatch!.journalTotal} 本期刊`;
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
        ? ` · 当前：${escapeHtml(s.currentPaperTitle.length > 42 ? s.currentPaperTitle.slice(0, 42) + "…" : s.currentPaperTitle)}`
        : "";
      html = `AI 分析中 · ${s.completed}/${s.batchSize}${cur}`;
      aiBtn.textContent = "暂停";
    } else if (s.state === "paused") {
      cls = "paused";
      html = `AI 已暂停 · ${s.completed}/${s.batchSize}（剩余 ${s.remaining} 篇）`;
      aiBtn.textContent = "继续";
    } else if (s.remaining > 0) {
      cls = "paused";
      html = `AI 任务未完成 · 剩余 ${s.remaining} 篇`;
      aiBtn.textContent = "继续";
    } else if (a.analysisFailed > 0) {
      // 真失败：红色，需要用户处理
      cls = "error";
      html = `AI 分析失败 ${a.analysisFailed} 篇 · <button class="link-btn" data-action="ai-retry">重试</button>`;
      aiBtn.textContent = "重试失败";
    } else if (a.pendingAnalysis > 0) {
      // 待处理：中性橙，低强调
      cls = "pending";
      html = `AI：待分析 ${a.pendingAnalysis} 篇`;
      aiBtn.textContent = "AI 分析";
    } else {
      // 已完成 / healthy：绿色仅少量使用
      cls = "ok";
      const last = a.lastAnalysis;
      const time = fmtTimeNow();
      const lastText =
        last && last.succeeded > 0
          ? ` · 上次成功分析 ${last.succeeded} 篇${last.finishedAt ? " · " + new Date(last.finishedAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : ""}`
          : "";
      html = `✓ 已更新 · ${time}　AI：待分析 0${lastText}`;
      aiBtn.textContent = "AI 分析";
    }
  }

  statusEl.className = `work-status ${cls}`;
  statusEl.innerHTML = html;
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
  document.querySelectorAll(".nav-item").forEach((t) => t.classList.toggle("active", (t as HTMLElement).dataset.view === name));
  document.querySelectorAll(".view").forEach((v) => v.classList.toggle("active", v.id === `view-${name}`));
  const titles: Record<string, string> = {
    recommend: "今日推荐", "recommend-history": "历史", papers: "所有论文", favorites: "收藏", journals: "期刊订阅", tags: "研究标签", settings: "设置", activity: "活动",
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
  };
  try {
    await invoke("set_settings", { s });
    settings = s;
    abstractLang = s.defaultAbstractLang === "en" ? "en" : "zh";
    renderPapers();
    renderNextCheck();
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
    el.innerHTML = `正在检查新论文 · ${checked}/${p.journalTotal} 本期刊${p.journalFailed ? ` · 失败 ${p.journalFailed}` : ""}`;
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
  });
  document.addEventListener("change", (ev) => {
    const el = ev.target as HTMLInputElement;
    // 中文 IME 兜底：input 事件在 composition 期间可能延迟/丢失，
    // change 在失焦/回车时可靠触发，确保 draft 始终同步（否则 dirty=false → 按钮 disabled → 点击无反应）
    const action = el.dataset.action;
    if (action === "tag-draft-name" || action === "tag-draft-desc") {
      const i = parseInt(el.dataset.idx!, 10);
      if (isNaN(i) || !tagDraft[i]) return;
      if (action === "tag-draft-name") tagDraft[i].name = el.value;
      else tagDraft[i].description = el.value;
      setTagDirty(true);
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
    const nav = t.closest(".nav-item") as HTMLElement | null;
    if (nav) {
      switchView(nav.dataset.view!);
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
    if (t.closest("#work-status")) {
      switchView("activity");
      return;
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
}

window.addEventListener("DOMContentLoaded", () => {
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

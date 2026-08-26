import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";

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

interface Tag {
  id: number;
  name: string;
  description: string | null;
  enabled: boolean;
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

interface CatalogCollectionView {
  code: string;
  name: string;
  version: string;
  effectiveFrom: string | null;
  sourceName: string;
  sourceUrl: string;
  count: number;
}

interface CatalogJournalView {
  catalogId: string;
  canonicalTitle: string;
  publisher: string | null;
  printIssn: string | null;
  onlineIssn: string | null;
  issnL: string | null;
  collections: string[];
  metadataNeedsReview: boolean;
  journalId: number | null;
  subscribed: boolean;
}

interface BulkSubscribeResult {
  subscribed: number;
  already: number;
  failed: number;
}

const MODEL_NAME = "cowpaper_model";
const DEFAULT_MODEL = "deepseek-v4-flash"; // 已验证可用的模型

let journals: Journal[] = [];
let tags: Tag[] = [];
let papers: Paper[] = [];
let aiStatus: AiStatus = emptyAiStatus();
let activity: ActivityState = emptyActivity();
let settings: Settings | null = null;
let abstractLang: "zh" | "en" = "zh";
const expandedAbstracts = new Set<number>();

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

// ---------- API Key（存 macOS Keychain，前端不长期保存真实 Key） ----------

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

async function loadTags() {
  tags = await invoke<Tag[]>("list_tags");
  renderTags();
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

// ---------- 渲染 ----------

let catalogCollections: CatalogCollectionView[] = [];
let catalogDetail: CatalogJournalView[] = [];
let selectedCatalogCode: string | null = null;
const catalogChecked = new Set<number>(); // 选中的 journal_id

/// 常用期刊页：集合列表。
async function renderCatalogCollections() {
  const box = $("catalog-collections");
  if (catalogCollections.length === 0) {
    try {
      catalogCollections = await invoke<CatalogCollectionView[]>("list_catalog_collections");
    } catch {
      box.innerHTML = '<div class="empty">常用期刊目录加载失败</div>';
      return;
    }
  }
  box.innerHTML = `
    <div class="catalog-grid">
      ${catalogCollections
        .map(
          (c) => `
        <button class="card catalog-col" data-catalog-code="${escapeHtml(c.code)}">
          <div class="title">${escapeHtml(c.name)}<span class="muted small"> · ${c.count} 本期刊</span></div>
          <div class="muted small">${escapeHtml(c.version === "current" ? "当前版" : "版本 " + c.version)}${c.effectiveFrom ? " · 更新 " + escapeHtml(c.effectiveFrom) : ""}</div>
          <div class="muted small">${escapeHtml(c.sourceName)}</div>
        </button>`,
        )
        .join("")}
    </div>`;
}

/// 集合详情：期刊 checkbox 列表 + 批量操作。
async function renderCatalogDetail(code: string) {
  selectedCatalogCode = code;
  catalogChecked.clear();
  const box = $("catalog-detail");
  box.classList.remove("hidden");
  try {
    catalogDetail = await invoke<CatalogJournalView[]>("list_catalog_journals", { code });
  } catch {
    box.innerHTML = '<div class="empty">期刊列表加载失败</div>';
    return;
  }
  const coll = catalogCollections.find((c) => c.code === code);
  const head = coll
    ? `<div class="title">${escapeHtml(coll.name)}<span class="muted small"> · ${catalogDetail.length} 本期刊</span></div>
       <div class="muted small">${escapeHtml(coll.sourceName)}${coll.effectiveFrom ? " · 更新日期 " + escapeHtml(coll.effectiveFrom) : ""}</div>`
    : "";
  const rows = catalogDetail
    .map((j) => {
      const subscribed = j.subscribed;
      const disabled = subscribed;
      const checked = subscribed ? "checked disabled" : "";
      const badge = j.collections.map((c) => `<span class="coll-badge">${escapeHtml(c)}</span>`).join("");
      const review = j.metadataNeedsReview ? '<span class="muted small">需复核</span>' : "";
      return `
        <li class="card catalog-journal">
          <label class="check grow">
            <input type="checkbox" data-journal-id="${j.journalId ?? ""}" ${checked} ${disabled} />
            <span>
              <span class="title">${escapeHtml(j.canonicalTitle)} ${badge} ${review}</span>
              <span class="muted small">${j.printIssn ? "Print " + escapeHtml(j.printIssn) : ""}${j.onlineIssn ? " · Online " + escapeHtml(j.onlineIssn) : ""}${j.issnL ? " · ISSN-L " + escapeHtml(j.issnL) : ""}</span>
              ${subscribed ? '<span class="chip ok-chip">已订阅</span>' : ""}
            </span>
          </label>
        </li>`;
    })
    .join("");
  box.innerHTML = `
    <div class="catalog-detail-head">
      <button class="ghost small" id="catalog-back">← 返回</button>
      ${head}
    </div>
    <div class="catalog-tools">
      <button class="ghost small" id="catalog-select-all">全选</button>
      <button class="ghost small" id="catalog-select-unsub">仅选择未订阅</button>
      <button class="ghost small" id="catalog-clear">取消全选</button>
    </div>
    <ul class="list">${rows || '<li class="empty">无期刊</li>'}</ul>
    <div class="catalog-actions"><span id="catalog-selected" class="muted small">已选择 0 本</span>
      <button class="primary" id="catalog-subscribe">添加 N 本</button>
    </div>`;
  $("catalog-back").addEventListener("click", () => {
    box.classList.add("hidden");
    renderCatalogCollections();
  });
  $("catalog-select-all").addEventListener("click", () => {
    catalogChecked.clear();
    document.querySelectorAll("#catalog-detail input[type=checkbox]:not(:disabled)").forEach((el) => {
      const id = parseInt((el as HTMLInputElement).dataset.journalId!, 10);
      if (!isNaN(id)) catalogChecked.add(id);
      (el as HTMLInputElement).checked = true;
    });
    updateCatalogSelected();
  });
  $("catalog-select-unsub").addEventListener("click", () => {
    catalogChecked.clear();
    document.querySelectorAll("#catalog-detail input[type=checkbox]:not(:disabled)").forEach((el) => {
      (el as HTMLInputElement).checked = false;
    });
    updateCatalogSelected();
  });
  $("catalog-clear").addEventListener("click", () => {
    catalogChecked.clear();
    document.querySelectorAll("#catalog-detail input[type=checkbox]:not(:disabled)").forEach((el) => {
      (el as HTMLInputElement).checked = false;
    });
    updateCatalogSelected();
  });
  document.querySelectorAll("#catalog-detail input[type=checkbox]:not(:disabled)").forEach((el) => {
    el.addEventListener("change", () => {
      const id = parseInt((el as HTMLInputElement).dataset.journalId!, 10);
      if (isNaN(id)) return;
      if ((el as HTMLInputElement).checked) catalogChecked.add(id);
      else catalogChecked.delete(id);
      updateCatalogSelected();
    });
  });
  const subBtn = $("catalog-subscribe") as HTMLButtonElement;
  subBtn.addEventListener("click", () => doCatalogSubscribe());
  updateCatalogSelected();
}

function updateCatalogSelected() {
  const n = catalogChecked.size;
  $("catalog-selected").textContent = `已选择 ${n} 本`;
  ($("catalog-subscribe") as HTMLButtonElement).textContent = `添加 ${n} 本`;
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
      const idsToSync = catalogDetail.filter((j) => j.journalId != null && res.subscribed > 0).map((j) => j.journalId!) as number[];
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
  for (const j of journals) {
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

function renderTags() {
  const ul = $("tag-list");
  ul.innerHTML = "";
  if (tags.length === 0) {
    ul.innerHTML = '<li class="empty">暂无标签</li>';
    return;
  }
  for (const t of tags) {
    const li = document.createElement("li");
    li.className = "card tag";
    li.innerHTML = `
      <div class="row">
        <div class="grow">
          <div class="title">${escapeHtml(t.name)}</div>
          <div class="muted small">${escapeHtml(t.description || "（无说明）")}</div>
        </div>
        <button class="ghost small" data-action="tag-toggle" data-id="${t.id}">${t.enabled ? "停用" : "启用"}</button>
        <button class="ghost small danger" data-action="tag-delete" data-id="${t.id}">删除</button>
      </div>
    `;
    ul.appendChild(li);
  }
}

function tagChips(matches: TagMatch[]): string {
  const shown = matches.filter((m) => m.score > 0);
  if (!shown.length) return "";
  return (
    `<div class="tags-line">` +
    shown.map((m) => `<span class="tag-chip">${escapeHtml(m.tag)} ${m.score.toFixed(1)}</span>`).join("") +
    `</div>`
  );
}

function paperCard(p: Paper, withAbstract: boolean): string {
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
  const score = p.totalScore != null ? `<span class="score-badge">总分 ${p.totalScore.toFixed(1)}</span>` : "";
  // Collection badge：小、低强调、无 score（与 AI tag 评分视觉分层）
  const collBadges = p.collections.length
    ? `<div class="coll-badges">${p.collections.map((c) => `<span class="coll-badge">${escapeHtml(c)}</span>`).join("")}</div>`
    : "";

  let abstractHtml = "";
  if (withAbstract) {
    const zhAbs = p.chineseAbstract;
    const enAbs = p.abstractText;
    let lang = p._lang || abstractLang;
    if (lang === "zh" && !zhAbs) lang = "en";
    const text = lang === "zh" ? zhAbs : enAbs;
    const isExpanded = expandedAbstracts.has(p.id);
    if (text) {
      const trunc = isExpanded ? text : text.slice(0, 400) + (text.length > 400 ? "…" : "");
      abstractHtml = `
        <div class="abstract-wrap">
          <div class="abstract-langs">
            <button class="abs-lang ${lang === "zh" ? "on" : ""}" data-action="abs-lang" data-id="${p.id}" data-lang="zh">中文</button>
            <button class="abs-lang ${lang === "en" ? "on" : ""}" data-action="abs-lang" data-id="${p.id}" data-lang="en">English</button>
          </div>
          ${partialNote}
          <div class="abstract">${escapeHtml(trunc)}</div>
          ${text.length > 400 ? `<button class="ghost small abs-expand" data-action="abs-expand" data-id="${p.id}">${isExpanded ? "收起" : "展开完整摘要"}</button>` : ""}
        </div>`;
    } else if (lang === "zh" && !zhAbs) {
      abstractHtml = `<div class="abstract muted">中文摘要待生成</div>`;
    } else {
      // missing：明确显示"暂无摘要"，不空白
      abstractHtml = `<div class="abstract muted">暂无摘要</div>`;
    }
  }

  return `
    <li class="${cls}">
      ${status}
      ${titleZh}
      ${titleEn}
      ${summary}
      <div class="paper-meta">${escapeHtml(authorText(p.authors))} · ${escapeHtml(p.journalName || "")} · ${fmtDate(p.publishedDate)}</div>
      ${collBadges}
      ${tagChips(p.tagMatches)} ${score}
      ${abstractHtml}
      <div class="paper-actions">
        <button class="ghost small" data-action="fav" data-id="${p.id}">${p.isFavorite ? "★" : "☆"}</button>
        <button class="ghost small" data-action="read" data-id="${p.id}">${p.isRead ? "已读" : "未读"}</button>
        <button class="ghost small" data-action="ignore" data-id="${p.id}">${p.isIgnored ? "取消忽略" : "忽略"}</button>
        ${p.url ? `<a href="#" class="ghost small" data-action="open" data-url="${escapeHtml(p.url)}">原文 ↗</a>` : ""}
        <span class="muted small detail">${escapeHtml(p.normalizedDoi || "")} · 来源 ${escapeHtml(p.discoverySource || "—")} · ${p.abstractSource ? "摘要 " + escapeHtml(p.abstractSource) : ""}</span>
      </div>
    </li>
  `;
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

  let list = papers;
  if (jid != null) list = list.filter((p) => p.journalId === jid);
  if (flag === "unread") list = list.filter((p) => !p.isRead);
  else if (flag === "favorite") list = list.filter((p) => p.isFavorite);
  else if (flag === "ignored") list = list.filter((p) => p.isIgnored);
  if (aist) list = list.filter((p) => p.analysisStatus === aist);
  if (abst) list = list.filter((p) => p.abstractQuality === abst);
  if (collt) list = list.filter((p) => p.collections.includes(collt));

  $("paper-list").innerHTML = list.length
    ? list.map((p) => paperCard(p, true)).join("")
    : '<li class="empty">暂无符合条件的论文</li>';
}

function renderRecommend() {
  const analyzed = papers
    .filter((p) => p.totalScore != null && !p.isIgnored)
    .sort((a, b) => {
      const d = (b.totalScore ?? 0) - (a.totalScore ?? 0);
      if (d !== 0) return d;
      return (b.publishedDate || "").localeCompare(a.publishedDate || "");
    });
  $("recommend-list").innerHTML = analyzed.length
    ? analyzed.map((p) => paperCard(p, true)).join("")
    : '<li class="empty">暂无推荐。保存 API Key 后点「AI 分析」，或同步新论文后自动分析。</li>';
}

function renderFavorites() {
  const list = papers.filter((p) => p.isFavorite);
  $("favorites-list").innerHTML = list.length
    ? list.map((p) => paperCard(p, true)).join("")
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
      <div class="pending-row"><span>等待摘要</span><strong>${waiting} 篇</strong></div>
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
  const [sbs, abs] = await Promise.all([
    invoke<SyncBatch[]>("list_sync_batches", { limit: 25 }).catch(() => []),
    invoke<AnalysisBatch[]>("list_analysis_batches", { limit: 25 }).catch(() => []),
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
  items.sort((x, y) => (y.time || "").localeCompare(x.time || ""));
  recentActivityItems = items.slice(0, 50);
  ul.innerHTML = recentActivityItems
    .map(
      (i) => `
      <li class="card activity-item ${selectedActivity && selectedActivity.type === i.type && selectedActivity.id === i.id ? "selected" : ""}" data-activity-type="${i.type}" data-activity-id="${i.id}">
        <div class="row">
          <div class="grow">
            <div class="title">${i.kind === "sync" ? "检查新论文" : "AI 分析"} #${i.id} <span class="chip muted-chip">${STATUS_ZH[i.status] || i.status}</span></div>
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
  document.querySelectorAll(".nav-item").forEach((t) => t.classList.toggle("active", (t as HTMLElement).dataset.view === name));
  document.querySelectorAll(".view").forEach((v) => v.classList.toggle("active", v.id === `view-${name}`));
  const titles: Record<string, string> = {
    recommend: "今日推荐", papers: "所有论文", favorites: "收藏", journals: "期刊订阅", tags: "标签", settings: "设置", activity: "活动",
  };
  $("view-title").textContent = titles[name] || name;
  // 进入活动页时渲染 master-detail（数据来自统一 activity + 批次查询）
  if (name === "activity") renderActivityCenter().catch(() => {});
  // 进入期刊订阅页时加载常用期刊目录
  if (name === "journals") renderCatalogCollections();
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
  const issn = ($("add-issn") as HTMLInputElement).value.trim();
  if (!name && !issn) {
    $("add-error").textContent = "请输入期刊名称或 ISSN";
    return;
  }
  $("add-error").textContent = "";
  try {
    await invoke("add_journal", { name: name || null, issn: issn || null });
    ($("add-name") as HTMLInputElement).value = "";
    ($("add-issn") as HTMLInputElement).value = "";
    await loadJournals();
  } catch (err) {
    $("add-error").textContent = String(err);
  }
}

async function addTagHandler(e: Event) {
  e.preventDefault();
  const name = ($("tag-name") as HTMLInputElement).value.trim();
  if (!name) {
    $("tag-error").textContent = "请输入标签名";
    return;
  }
  $("tag-error").textContent = "";
  const desc = ($("tag-desc") as HTMLInputElement).value.trim();
  try {
    await invoke("add_tag", { name, description: desc || null });
    ($("tag-name") as HTMLInputElement).value = "";
    ($("tag-desc") as HTMLInputElement).value = "";
    await loadTags();
  } catch (err) {
    $("tag-error").textContent = String(err);
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
  await invoke("set_settings", { s });
  settings = s;
  abstractLang = s.defaultAbstractLang === "en" ? "en" : "zh";
  renderPapers();
  renderNextCheck();
  $("settings-msg").textContent = "设置已保存（每日时间修改后，运行中的调度器将在 30s 内采用）";
  $("settings-msg").className = "ok small";
}

// ---------- 事件监听 ----------

async function setupListeners() {
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
    el.innerHTML = `正在检查新论文 · ${p.journalCompleted}/${p.journalTotal} 本期刊`;
  });
  await listen("sync://done", async (e) => {
    const r = e.payload as any;
    setStatus(`同步完成：新增 ${r.newPapers} · 已有 ${r.existingPapers} · 补摘要 ${r.abstractsAdded || 0}${r.abstractsUpgraded ? " · 摘要升级 " + r.abstractsUpgraded : ""}`, "done");
    // 统一刷新：papers + 工作状态（Work Center / 徽标 / 面板 / 待处理区 / 计数）
    await loadJournals();
    await loadPapers();
    await refreshWorkState();
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
    // 杜绝"AI 待处理 7"与"无待处理"并存）
    await loadPapers();
    await refreshWorkState();
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
      if (confirm("删除该期刊及其所有论文？")) {
        await invoke("delete_journal", { id: parseInt(del.dataset.id!, 10) });
        await loadJournals();
        await loadPapers();
        await refreshWorkState();
      }
      return;
    }
    const fav = t.closest("[data-action='fav']") as HTMLElement | null;
    if (fav) {
      const id = parseInt(fav.dataset.id!, 10);
      const p = papers.find((x) => x.id === id);
      if (p) await setFlag(id, "favorite", !p.isFavorite);
      return;
    }
    const read = t.closest("[data-action='read']") as HTMLElement | null;
    if (read) {
      const id = parseInt(read.dataset.id!, 10);
      const p = papers.find((x) => x.id === id);
      if (p) await setFlag(id, "read", !p.isRead);
      return;
    }
    const ignore = t.closest("[data-action='ignore']") as HTMLElement | null;
    if (ignore) {
      const id = parseInt(ignore.dataset.id!, 10);
      const p = papers.find((x) => x.id === id);
      if (p) await setFlag(id, "ignored", !p.isIgnored);
      return;
    }
    const absLang = t.closest("[data-action='abs-lang']") as HTMLElement | null;
    if (absLang) {
      const id = parseInt(absLang.dataset.id!, 10);
      const p = papers.find((x) => x.id === id);
      if (p) {
        p._lang = absLang.dataset.lang as "zh" | "en";
        renderPapers();
        renderRecommend();
      }
      return;
    }
    const absExpand = t.closest("[data-action='abs-expand']") as HTMLElement | null;
    if (absExpand) {
      const id = parseInt(absExpand.dataset.id!, 10);
      if (expandedAbstracts.has(id)) expandedAbstracts.delete(id);
      else expandedAbstracts.add(id);
      renderPapers();
      renderRecommend();
      return;
    }
    const tagToggle = t.closest("[data-action='tag-toggle']") as HTMLElement | null;
    if (tagToggle) {
      const id = parseInt(tagToggle.dataset.id!, 10);
      const tg = tags.find((x) => x.id === id);
      if (tg) {
        await invoke("update_tag", { id, name: tg.name, description: tg.description, enabled: !tg.enabled });
        await loadTags();
      }
      return;
    }
    const tagDelete = t.closest("[data-action='tag-delete']") as HTMLElement | null;
    if (tagDelete) {
      if (confirm("删除该标签？")) {
        await invoke("delete_tag", { id: parseInt(tagDelete.dataset.id!, 10) });
        await loadTags();
      }
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
    const catalogCol = t.closest("[data-catalog-code]") as HTMLElement | null;
    if (catalogCol) {
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
  $("tag-form").addEventListener("submit", addTagHandler);
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
  $("tab-common").addEventListener("click", () => {
    $("tab-common").classList.add("active");
    $("tab-manual").classList.remove("active");
    $("catalog-view").classList.remove("hidden");
    $("manual-view").classList.add("hidden");
    renderCatalogCollections();
  });
  $("tab-manual").addEventListener("click", () => {
    $("tab-manual").classList.add("active");
    $("tab-common").classList.remove("active");
    $("catalog-view").classList.add("hidden");
    $("manual-view").classList.remove("hidden");
  });

  // Key 保存在本地 secret 文件，不回填到输入框（输入框仅用于「替换 Key」时输入）
  ($("api-key") as HTMLInputElement).value = "";
  ($("model") as HTMLInputElement).value = getModel();

  (async () => {
    await setupListeners();
    await Promise.all([loadJournals(), loadTags(), loadSettings()]);
    await loadPapers();
    // 统一工作状态刷新（Work Center / 积压 / 待处理区 / 计数）
    await refreshWorkState();
    renderNextCheck();
    await refreshKeyStatus();
    // 启动自动同步（阈值判断在 Rust 端）
    await invoke("maybe_auto_sync").catch(() => {});
  })();
});

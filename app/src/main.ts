import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";

interface Journal {
  id: number;
  name: string;
  printIssn: string | null;
  onlineIssn: string | null;
  publisher: string | null;
  enabled: boolean;
  coverageStatus: string | null;
  abstractCoverageRate: number | null;
  lastSuccessfulSyncAt: string | null;
  lastPaperDate: string | null;
  paperCount: number;
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

const KEY_NAME = "cowpaper_api_key"; // 旧版 localStorage Key（仅用于一次性迁移，不再写入）
const MODEL_NAME = "cowpaper_model";
const DEFAULT_MODEL = "deepseek-v4-flash"; // 已验证可用的模型

let journals: Journal[] = [];
let tags: Tag[] = [];
let papers: Paper[] = [];
let aiStatus: AiStatus = emptyAiStatus();
let settings: Settings | null = null;
let abstractLang: "zh" | "en" = "zh";
const expandedAbstracts = new Set<number>();

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

/// 一次性迁移：把旧 localStorage Key 写入 Keychain，写入成功后才删除 localStorage。
async function migrateLegacyKey() {
  const legacy = localStorage.getItem(KEY_NAME);
  if (!legacy) return;
  try {
    if (!(await hasKey())) {
      await invoke("save_api_key", { key: legacy });
    }
    localStorage.removeItem(KEY_NAME); // 只有 Keychain 写入成功后才删旧 Key
  } catch {
    // 写入失败：保留 localStorage，下次启动再试；不在这里输出 Key
  }
}

async function refreshKeyStatus() {
  const el = $("key-status");
  if (!el) return;
  const has = await hasKey();
  el.textContent = has ? "✓ 已保存到 macOS 钥匙串" : "未保存 Key";
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
  renderAiBadge();
  renderAiPanel();
  renderBacklog();
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
  const pending = await invoke<number>("get_pending_ai_count").catch(() => 0);
  $("pending-count").textContent = `当前待分析：${pending} 篇`;
}

// ---------- 渲染 ----------

function renderJournals() {
  const ul = $("journal-list");
  ul.innerHTML = "";
  if (journals.length === 0) {
    ul.innerHTML = '<li class="empty">暂无订阅，请在上方添加期刊</li>';
    return;
  }
  for (const j of journals) {
    const rate = j.abstractCoverageRate != null ? Math.round(j.abstractCoverageRate * 100) + "%" : "—";
    const li = document.createElement("li");
    li.className = "card journal";
    li.innerHTML = `
      <div class="row">
        <div class="grow">
          <div class="title">${escapeHtml(j.name)}</div>
          <div class="muted small">${escapeHtml(j.printIssn || "")} ${escapeHtml(j.onlineIssn || "")} · ${escapeHtml(j.publisher || "")}</div>
        </div>
        <button class="ghost small" data-action="sync-one" data-id="${j.id}">同步</button>
        <button class="ghost small" data-action="toggle" data-id="${j.id}">${j.enabled ? "停用" : "启用"}</button>
        <button class="ghost small danger" data-action="delete" data-id="${j.id}">删除</button>
      </div>
      <div class="muted small">${escapeHtml(j.coverageStatus || "未同步")} · 摘要覆盖 ${rate} · 论文 ${j.paperCount} · 最近 ${fmtDate(j.lastPaperDate)}</div>
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
  const summary = p.oneSentenceSummary ? `<div class="paper-summary">${escapeHtml(p.oneSentenceSummary)}</div>` : "";
  const score = p.totalScore != null ? `<span class="score-badge">总分 ${p.totalScore.toFixed(1)}</span>` : "";

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
          <div class="abstract">${escapeHtml(trunc)}</div>
          ${text.length > 400 ? `<button class="ghost small abs-expand" data-action="abs-expand" data-id="${p.id}">${isExpanded ? "收起" : "展开完整摘要"}</button>` : ""}
        </div>`;
    } else if (lang === "zh" && !zhAbs) {
      abstractHtml = `<div class="abstract muted">中文摘要待生成</div>`;
    } else {
      abstractHtml = `<div class="abstract muted">暂未取得摘要</div>`;
    }
  }

  return `
    <li class="${cls}">
      ${status}
      ${titleZh}
      ${titleEn}
      ${summary}
      <div class="paper-meta">${escapeHtml(authorText(p.authors))} · ${escapeHtml(p.journalName || "")} · ${fmtDate(p.publishedDate)}</div>
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
  const jid = jsel.value ? parseInt(jsel.value, 10) : null;
  const flag = fsel.value;
  const aist = asel.value;

  let list = papers;
  if (jid != null) list = list.filter((p) => p.journalId === jid);
  if (flag === "unread") list = list.filter((p) => !p.isRead);
  else if (flag === "favorite") list = list.filter((p) => p.isFavorite);
  else if (flag === "ignored") list = list.filter((p) => p.isIgnored);
  if (aist) list = list.filter((p) => p.analysisStatus === aist);

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

// ---------- AI 状态徽标 / 面板 / 积压 ----------

function aiBadgeText(): string {
  const s = aiStatus;
  if (s.state === "running" || s.state === "pausing") return `AI ${s.completed}/${s.batchSize}`;
  if (s.state === "paused") return `AI 已暂停 · ${s.completed}/${s.batchSize}`;
  if (s.state === "stopping") return "AI 停止中";
  if (s.remaining > 0) return `AI 未完成 ${s.remaining}`;
  if (s.failed > 0) return "AI 需要处理";
  return "✓ 已更新";
}

function renderAiBadge() {
  const badge = $("ai-badge");
  badge.textContent = aiBadgeText();
  badge.className = `ai-badge ${aiStatus.state}`;
  const pending = papers.filter((p) => p.analysisStatus === "pendingAnalysis" && p.abstractText).length;
  if (aiStatus.state === "idle" && aiStatus.remaining === 0 && aiStatus.failed === 0 && pending > 0) {
    badge.textContent = `AI 待处理 ${pending}`;
    badge.className = "ai-badge idle-has";
  }
}

function renderAiPanel() {
  const panel = $("ai-panel");
  const s = aiStatus;
  const pending = papers.filter((p) => p.analysisStatus === "pendingAnalysis" && p.abstractText).length;

  // 上一次运行摘要（§七：批次结束后保留，直到下一次运行完成覆盖）
  const lastRun = s.lastRun
    ? `<div class="ai-last-run muted small">上次分析：${s.lastRun.total} 篇 · 成功 ${s.lastRun.success} · 失败 ${s.lastRun.failed}${s.lastRun.finishedAt ? " · 完成于 " + new Date(s.lastRun.finishedAt).toLocaleTimeString() : ""}</div>`
    : "";

  // 任何状态都渲染有意义内容，绝不允许空白横条
  if (s.state === "idle" && s.remaining === 0 && s.failed === 0 && pending === 0) {
    panel.innerHTML = `
      <div class="ai-panel-head"><strong>AI 分析</strong><span class="muted small">当前无待处理任务</span></div>
      ${lastRun}`;
    return;
  }
  if (s.state === "idle" && s.remaining === 0 && s.failed === 0) {
    panel.innerHTML = `
      <div class="ai-panel-head"><strong>AI 分析</strong><span class="muted small">待分析 ${pending} 篇</span></div>
      ${lastRun}
      <div class="ai-panel-actions"><button class="primary small" data-action="ai-backlog">开始分析</button></div>`;
    return;
  }
  if (s.state === "idle" && s.remaining === 0 && s.failed > 0) {
    const reason = s.lastError
      ? `<div class="ai-error">最近失败原因：${escapeHtml(s.lastError)}</div>`
      : "";
    panel.innerHTML = `
      <div class="ai-panel-head"><strong>AI 分析</strong><span class="muted small">${s.failed} 篇分析失败</span></div>
      ${reason}${lastRun}
      <div class="ai-panel-actions"><button class="ghost small" data-action="ai-retry">重试失败论文</button></div>`;
    return;
  }

  // running / pausing / paused / stopping：全量状态
  const eta = s.etaSeconds != null ? `预计剩余约 ${fmtDur(s.etaSeconds)}（估算）` : "样本不足，暂无 ETA";
  const current = s.currentPaperTitle
    ? `<div class="ai-current"><span class="muted small">当前：</span>${escapeHtml(s.currentPaperTitle)}</div>`
    : "";
  const retry = s.retryWaiting ? `<div class="ai-retry">因网络 / API 限流等待重试</div>` : "";
  const err = s.lastError ? `<div class="ai-error">${escapeHtml(s.lastError)}</div>` : "";
  const slow =
    s.state === "running" && s.currentPaperStartedAt
      ? isStale(s.currentPaperStartedAt, 60000)
        ? `<div class="ai-warn">当前请求耗时较长（超过 1 分钟），请检查网络或 API 状态。</div>`
        : ""
      : "";

  panel.innerHTML = `
    <div class="ai-panel-head">
      <strong>AI 分析</strong>
      <span class="muted small">总计 ${s.batchSize} · 已完成 ${s.completed} · 成功 ${s.success} · 失败 ${s.failed} · 跳过 ${s.skipped} · 剩余 ${s.remaining} · 并发 2</span>
    </div>
    <div class="ai-panel-meta muted small">
      已运行 ${fmtDur(s.elapsedSeconds)} · 最近完成 ${s.lastProgressAt ? new Date(s.lastProgressAt).toLocaleTimeString() : "—"} · ${eta}
    </div>
    ${current}${retry}${slow}${err}
    <div class="ai-panel-actions">
      ${s.state === "running" || s.state === "pausing" ? `<button class="ghost small" data-action="ai-pause">暂停</button>` : ""}
      ${s.state === "paused" ? `<button class="primary small" data-action="ai-resume">继续分析</button>` : ""}
      ${s.state !== "idle" || s.remaining > 0 ? `<button class="ghost small" data-action="ai-stop">停止本次任务</button>` : ""}
      ${s.failed > 0 && s.state === "idle" ? `<button class="ghost small" data-action="ai-retry">重试失败论文</button>` : ""}
    </div>
  `;
}

function isStale(iso: string, ms: number): boolean {
  const t = new Date(iso).getTime();
  return !isNaN(t) && Date.now() - t > ms;
}

function renderBacklog() {
  const banner = $("backlog-banner");
  const s = aiStatus;
  if (s.state === "paused" && s.remaining > 0) {
    banner.classList.remove("hidden");
    banner.innerHTML = `上次分析未完成，剩余 <strong>${s.remaining}</strong> 篇 · <button class="ghost small" data-action="ai-resume">继续</button>`;
    return;
  }
  const pending = papers.filter((p) => p.analysisStatus === "pendingAnalysis" && p.abstractText).length;
  if (s.state === "idle" && s.remaining === 0 && pending > 0) {
    banner.classList.remove("hidden");
    banner.innerHTML = `待分析论文 <strong>${pending}</strong> 篇 · <button class="ghost small" data-action="ai-backlog">开始分析</button>`;
    return;
  }
  banner.classList.add("hidden");
  banner.innerHTML = "";
}

// ---------- 动作 ----------

function switchView(name: string) {
  document.querySelectorAll(".nav-item").forEach((t) => t.classList.toggle("active", (t as HTMLElement).dataset.view === name));
  document.querySelectorAll(".view").forEach((v) => v.classList.toggle("active", v.id === `view-${name}`));
  const titles: Record<string, string> = {
    recommend: "今日推荐", papers: "所有论文", favorites: "收藏", journals: "期刊订阅", tags: "标签", settings: "设置",
  };
  $("view-title").textContent = titles[name] || name;
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

async function startAnalyze(paperIds: number[] | null) {
  if (!(await requireKey())) return;
  await invoke("start_ai", { paperIds, model: getModel() });
  setStatus("AI 分析已开始", "running");
}

async function pauseAi() {
  await invoke("pause_ai");
}
async function resumeAi() {
  if (!(await requireKey())) return;
  await invoke("resume_ai", { model: getModel() });
}
async function stopAi() {
  if (confirm("停止本次分析？已完成结果会保留，未完成论文回到待分析。")) {
    await invoke("stop_ai");
  }
}
async function retryFailedAi() {
  if (!(await requireKey())) return;
  await invoke("retry_failed_ai", { model: getModel() });
}

/// 顶部「AI 分析」手动入口：只处理有摘要、尚未成功、且当前无其他 batch 的论文。
async function manualAnalyze() {
  if (!(await requireKey())) return;
  if (aiStatus.state !== "idle" || aiStatus.remaining > 0) {
    setStatus("已有 AI 任务在运行，可在 AI 面板暂停或停止", "error");
    $("ai-panel").classList.remove("hidden");
    $("ai-badge").classList.add("open");
    return;
  }
  const pending = await invoke<number>("get_pending_ai_count").catch(() => 0);
  if (pending <= 0) {
    setStatus("当前没有待分析的论文", "done");
    return;
  }
  if (!confirm(`待分析 ${pending} 篇，将调用 DeepSeek 产生费用。开始分析？`)) return;
  await startAnalyze(null);
  await loadAiStatus();
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

async function saveKey() {
  const key = ($("api-key") as HTMLInputElement).value.trim();
  const model = ($("model") as HTMLInputElement).value.trim() || DEFAULT_MODEL;
  localStorage.setItem(MODEL_NAME, model);
  if (!key) {
    $("settings-msg").textContent = "请输入 API Key";
    $("settings-msg").className = "error";
    return;
  }
  try {
    await invoke("save_api_key", { key });
    ($("api-key") as HTMLInputElement).value = ""; // 不回显真实 Key
    localStorage.removeItem(KEY_NAME); // 迁移完成，不再保留 localStorage
    $("settings-msg").textContent = "已保存到 macOS 钥匙串";
    $("settings-msg").className = "ok small";
    await refreshKeyStatus();
  } catch (err) {
    $("settings-msg").textContent = String(err);
    $("settings-msg").className = "error";
  }
}

async function testConnection() {
  const model = ($("model") as HTMLInputElement).value.trim() || DEFAULT_MODEL;
  $("settings-msg").textContent = "测试中…";
  $("settings-msg").className = "muted small";
  try {
    const r = await invoke<{ ok: boolean; message: string }>("test_api_connection", { model });
    $("settings-msg").textContent = r.message;
    $("settings-msg").className = r.ok ? "ok small" : "error";
  } catch (err) {
    $("settings-msg").textContent = String(err);
    $("settings-msg").className = "error";
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
  $("settings-msg").textContent = "设置已保存";
  $("settings-msg").className = "ok small";
}

// ---------- 事件监听 ----------

async function setupListeners() {
  await listen("sync://start", () => setStatus("同步中…", "running"));
  await listen("sync://journal-start", (e) => setStatus(`正在同步 ${e.payload}`, "running"));
  await listen("sync://journal-error", (e) => setStatus(`同步错误：${e.payload}`, "error"));
  await listen("sync://done", async (e) => {
    const r = e.payload as any;
    setStatus(`同步完成：新增 ${r.newPapers} · 已有 ${r.existingPapers} · 补摘要 ${r.abstractsFilled}`, "done");
    ($("btn-sync") as HTMLButtonElement).disabled = false;
    await loadJournals();
    await loadPapers();
    // 同步后自动分析新论文（§十一）
    if (settings?.autoAnalyzeNew && Array.isArray(r.newPaperIds) && r.newPaperIds.length > 0 && (await hasKey())) {
      await invoke("start_ai", { paperIds: r.newPaperIds, model: getModel() });
    }
  });

  await listen("ai://progress", async (e) => {
    aiStatus = e.payload as AiStatus;
    renderAiBadge();
    renderAiPanel();
    renderBacklog();
  });
  await listen("ai://retry", async () => {
    await loadAiStatus();
  });
  await listen("ai://error", (e) => setStatus(`AI：${e.payload}`, "error"));
  await listen("ai://finished", async () => {
    setStatus("AI 分析批次结束", "done");
    await loadAiStatus();
    await loadPapers();
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
    if (t.closest("[data-action='ai-retry']")) { await retryFailedAi(); return; }
    if (t.closest("[data-action='ai-backlog']")) {
      const pending = papers.filter((p) => p.analysisStatus === "pendingAnalysis" && p.abstractText).length;
      if (confirm(`本次最多有 ${pending} 篇论文待分析，将调用 DeepSeek 产生费用。开始分析？`)) {
        await startAnalyze(null);
      }
      return;
    }
    if (t.closest("#ai-badge")) {
      const panel = $("ai-panel");
      const nowOpen = panel.classList.toggle("hidden") === false;
      $("ai-badge").classList.toggle("open", nowOpen);
      return;
    }
  });
}

window.addEventListener("DOMContentLoaded", () => {
  $("add-form").addEventListener("submit", addJournalHandler);
  $("tag-form").addEventListener("submit", addTagHandler);
  $("btn-sync").addEventListener("click", () => startSync(null));
  $("btn-refresh").addEventListener("click", loadPapers);
  $("journal-filter").addEventListener("change", renderPapers);
  $("ai-filter").addEventListener("change", renderPapers);
  $("flag-filter").addEventListener("change", renderPapers);
  $("btn-save-key").addEventListener("click", saveKey);
  $("btn-test").addEventListener("click", testConnection);
  $("btn-clear-key").addEventListener("click", async () => {
    try {
      await invoke("delete_api_key");
      localStorage.removeItem(KEY_NAME);
      ($("api-key") as HTMLInputElement).value = "";
      $("settings-msg").textContent = "已删除钥匙串中的 Key";
      $("settings-msg").className = "muted small";
      await refreshKeyStatus();
    } catch (err) {
      $("settings-msg").textContent = String(err);
      $("settings-msg").className = "error";
    }
  });
  $("btn-save-settings").addEventListener("click", saveSettings);
  $("btn-analyze-backlog").addEventListener("click", async () => {
    const pending = await invoke<number>("get_pending_ai_count").catch(() => 0);
    if (confirm(`本次最多有 ${pending} 篇论文待分析，将调用 DeepSeek 产生费用。开始分析？`)) {
      await startAnalyze(null);
    }
  });
  $("btn-ai-manual").addEventListener("click", manualAnalyze);
  $("btn-retry-failed").addEventListener("click", async () => {
    if (aiStatus.state !== "idle" || aiStatus.remaining > 0) {
      setStatus("请先等待当前任务结束或停止后再重试失败论文", "error");
      return;
    }
    await retryFailedAi();
    setStatus("已加入失败论文重试队列", "running");
  });

  // Key 存 Keychain，不再回填到输入框（输入框仅用于「替换 Key」时输入）
  ($("api-key") as HTMLInputElement).value = "";
  ($("model") as HTMLInputElement).value = getModel();

  (async () => {
    await setupListeners();
    await Promise.all([loadJournals(), loadTags(), loadSettings()]);
    await loadAiStatus();
    await loadPapers();
    // 旧 localStorage Key 一次性迁移到 Keychain（写入成功后才删除旧 Key）
    await migrateLegacyKey();
    await refreshKeyStatus();
    // 启动自动同步（阈值判断在 Rust 端）
    await invoke("maybe_auto_sync").catch(() => {});
  })();
});

use serde::{Deserialize, Serialize};

// 论文 AI 分析状态（内部英文值，UI 负责中文化）
pub const ST_WAITING_ABSTRACT: &str = "waitingForAbstract";
pub const ST_PENDING: &str = "pendingAnalysis";
#[allow(dead_code)]
pub const ST_QUEUED: &str = "queued";
pub const ST_ANALYZING: &str = "analyzing";
pub const ST_SUCCEEDED: &str = "analysisSucceeded";
#[allow(dead_code)]
pub const ST_FAILED: &str = "analysisFailed";

// 摘要质量（Round 5B）：由本地确定性 heuristic 判定并持久化，与 analysis_status 独立维度。
pub const ABQ_MISSING: &str = "missing";
pub const ABQ_PARTIAL: &str = "partial";
pub const ABQ_COMPLETE: &str = "complete";

// AI 队列自身状态
pub const QS_IDLE: &str = "idle";
pub const QS_RUNNING: &str = "running";
pub const QS_PAUSING: &str = "pausing";
pub const QS_PAUSED: &str = "paused";
pub const QS_STOPPING: &str = "stopping";

// 同步触发来源（区分 manual/startup/daily/tray/journalTest）
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncTrigger {
    Manual,
    Startup,
    Daily,
    Tray,
    JournalTest,
}

impl SyncTrigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            SyncTrigger::Manual => "manual",
            SyncTrigger::Startup => "startup",
            SyncTrigger::Daily => "daily",
            SyncTrigger::Tray => "tray",
            SyncTrigger::JournalTest => "journalTest",
        }
    }
}

/// 同步启动结果：started=false 且 reason="syncAlreadyRunning" 表示已有全局同步在执行。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStartResult {
    pub started: bool,
    pub reason: String,
    pub trigger: Option<String>,
    pub started_at: Option<String>,
}

/// 上一次 AI 运行摘要（供 UI 在队列空闲时展示，直到下一次运行覆盖）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastAiRun {
    pub total: i64,
    pub success: i64,
    pub failed: i64,
    pub skipped: i64,
    /// 停止时未执行的论文数；正常完成为 0。
    pub remaining: i64,
    /// 终态：completed | stopped（未来可扩展 failed/cancelled）。
    pub final_status: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub error_summary: Option<String>,
}

/// 期刊标识符类型（Round 5A canonical identity）。
pub const IDT_PRINT: &str = "print";
pub const IDT_ONLINE: &str = "online";
pub const IDT_OTHER: &str = "other";

/// 单个 ISSN 标识符（规范化后），属于某个 canonical Journal。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalIdentifier {
    pub id: i64,
    pub journal_id: i64,
    pub identifier_type: String,
    /// canonical 形式 NNNN-NNNX
    pub value: String,
    pub source: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Tag 配置版本（Round 6.5）：active / scheduled / retired。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagConfigVersion {
    pub id: i64,
    pub status: String,
    pub effective_cycle_key: Option<String>,
    pub created_at: String,
    pub activated_at: Option<String>,
}

/// Tag 配置版本项（name/description 快照；tag_id 是稳定 identity）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagConfigItem {
    pub version_id: i64,
    pub tag_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub deleted: bool,
}

/// 前端 Draft 提交项（id=0 表示新增）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagDraftItem {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub deleted: bool,
}

/// Tag 配置 diff 分类（一次保存统一处理）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagConfigDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub disabled: Vec<String>,
    pub enabled: Vec<String>,
    pub semantic_changed: Vec<String>,
    pub unchanged: Vec<String>,
}

/// Tags 页 baseline（scheduled 优先，否则 active）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagBaseline {
    pub items: Vec<TagDraftItem>,
    /// baseline 来源：active | scheduled
    pub source: String,
    pub scheduled_effective_cycle_key: Option<String>,
}

/// 保存结果（前端确认/提示用）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveTagConfigResult {
    pub mode: String,
    pub effective_cycle_key: Option<String>,
    pub diff: TagConfigDiff,
    pub ai_needed_papers: i64,
}

/// 推荐周期（每日 snapshot）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendationRun {
    pub id: i64,
    pub cycle_key: String,
    pub cycle_start: String,
    pub cycle_end: Option<String>,
    pub status: String,
    pub created_at: String,
    pub finalized_at: Option<String>,
    pub item_count: i64,
    /// 该 run 最高 score_snapshot（历史概览卡片统计）
    pub max_score: Option<f64>,
    /// 涉及期刊数（按 paper.journal_id 去重；历史概览卡片统计）
    pub journal_count: i64,
}

/// 推荐项（只存 paper_id + rank + score_snapshot，不复制 Paper 内容）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendationItem {
    pub id: i64,
    pub run_id: i64,
    pub paper_id: i64,
    pub rank: i64,
    pub score_snapshot: f64,
    pub added_at: String,
}

/// 推荐项 + Paper 内容（历史/当前页展示用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendationItemView {
    pub run_id: i64,
    pub paper_id: i64,
    pub rank: i64,
    pub score_snapshot: f64,
    pub paper: Paper,
}

/// 推荐周期视图（run + items）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendationRunView {
    pub run: RecommendationRun,
    pub items: Vec<RecommendationItemView>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyPaperSummary {
    pub cycle_key: String,
    pub paper_count: i64,
    pub missing_count: i64,
    pub recommendation_run_id: Option<i64>,
    pub recommendation_count: i64,
}

/// 集合视图（前端常用期刊页）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogCollectionView {
    pub code: String,
    pub name: String,
    pub version: String,
    pub effective_from: Option<String>,
    pub source_name: String,
    pub source_url: String,
    /// 当前 DB 中该集合的 membership 数（导入后）
    pub count: i64,
}

/// Catalog 期刊视图（常用期刊页列表项）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogJournalView {
    pub catalog_id: String,
    pub canonical_title: String,
    pub publisher: Option<String>,
    pub print_issn: Option<String>,
    pub online_issn: Option<String>,
    pub issn_l: Option<String>,
    pub collections: Vec<String>,
    pub metadata_needs_review: bool,
    /// DB 中对应的 canonical Journal（resolve 后）
    pub journal_id: Option<i64>,
    /// 是否已订阅（enabled）
    pub subscribed: bool,
}

/// 批量订阅结果摘要。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkSubscribeResult {
    pub subscribed: i64,
    pub already: i64,
    pub failed: i64,
}

/// 期刊集合（UTD24 / FT50 等 Journal metadata，不参与 AI 评分）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalCollection {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub version: Option<String>,
    pub effective_from: Option<String>,
    pub source_name: Option<String>,
    pub source_url: Option<String>,
    pub last_verified_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// 成员数（list_collections 聚合）
    pub member_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Journal {
    pub id: i64,
    pub name: String,
    pub print_issn: Option<String>,
    pub online_issn: Option<String>,
    /// ISSN-L（linking ISSN），与媒介版本无关的 canonical 关联
    pub issn_l: Option<String>,
    pub publisher: Option<String>,
    pub enabled: bool,
    pub priority: i64,
    pub rss_url: Option<String>,
    pub openalex_source_id: Option<String>,
    pub publisher_adapter: Option<String>,
    pub last_successful_sync_at: Option<String>,
    pub last_paper_date: Option<String>,
    pub coverage_status: Option<String>,
    pub abstract_coverage_rate: Option<f64>,
    pub paper_count: i64,
    pub created_at: String,
    pub updated_at: String,
    /// 该 journal 的全部标识符（print/online/other，canonical 形式）
    pub identifiers: Vec<JournalIdentifier>,
    /// 所属集合 code 列表（如 ["TEST-UTD","TEST-FT"]）
    pub collections: Vec<String>,
    /// 疑似重复（与另一 journal 共享 ISSN-L 或相同标题规范化），仅供人工处理
    pub possible_duplicate: bool,
    /// catalog identifier 未可靠解决（需人工复核）
    pub metadata_needs_review: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Author {
    pub given: Option<String>,
    pub family: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TagMatch {
    pub tag: String,
    pub score: f64,
    /// 稳定 tag identity（Round 6.5；旧数据可能缺失 → 按 name 匹配）
    #[serde(default)]
    pub tag_id: Option<i64>,
    /// tag 语义 hash（tag_id + name + description + score prompt version）
    #[serde(default)]
    pub semantic_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Paper {
    pub id: i64,
    pub journal_id: i64,
    pub journal_name: Option<String>,
    pub normalized_doi: Option<String>,
    pub original_doi: Option<String>,
    pub title: Option<String>,
    pub authors: Vec<Author>,
    pub published_date: Option<String>,
    pub year: Option<i32>,
    pub abstract_text: Option<String>,
    pub abstract_source: Option<String>,
    pub abstract_retrieved_at: Option<String>,
    /// 摘要质量（complete / partial / missing），由 Rust 本地判定持久化
    pub abstract_quality: String,
    /// 上次摘要检查时间（recovery 节流依据）
    pub abstract_last_checked_at: Option<String>,
    /// 摘要补抓失败计数
    pub abstract_retry_count: i64,
    /// Round 7 Phase 1：摘要来源落地页 URL（provenance，如 publisher:nature 页面地址）
    #[serde(default)]
    pub abstract_source_url: Option<String>,
    /// Round 7 Phase 1：内容类型（research_article / review / ... / unknown）。
    /// UI 不直接展示技术字段，仅用于 recovery / recommendation 语义门控与产品文案。
    #[serde(default)]
    pub content_kind: String,
    /// 内容类型判定证据来源（如 crossref:type / openalex:type / title-heuristic）
    #[serde(default)]
    pub content_kind_source: Option<String>,
    /// 内容类型判定置信度（EXACT / HIGH / LOW / UNKNOWN）
    #[serde(default)]
    pub content_kind_confidence: String,
    /// 摘要语义状态（available / missing_recoverable / not_expected / unknown）
    #[serde(default)]
    pub abstract_status: String,
    pub url: Option<String>,
    pub publisher_article_id: Option<String>,
    pub openalex_work_id: Option<String>,
    pub discovery_source: Option<String>,
    pub is_favorite: bool,
    pub is_read: bool,
    pub is_ignored: bool,
    pub analysis_status: String,
    pub chinese_title: Option<String>,
    pub chinese_abstract: Option<String>,
    pub one_sentence_summary: Option<String>,
    pub tag_matches: Vec<TagMatch>,
    pub total_score: Option<f64>,
    pub model_name: Option<String>,
    pub prompt_version: Option<String>,
    pub evidence_hash: Option<String>,
    pub analyzed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// 所属期刊的 collection code 列表（paper → journal → collections 派生，不冗余）
    pub collections: Vec<String>,
}

/// Literature workspace collection. This is intentionally separate from
/// journal_collections, which groups subscribed journals.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryCollection {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryTag {
    pub id: i64,
    pub name: String,
    pub color: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryMembership {
    pub paper_id: i64,
    pub added_at: String,
    pub added_source: String,
    pub collection_ids: Vec<i64>,
    pub tag_ids: Vec<i64>,
}

/// Library list row: the canonical Paper plus library-only membership data.
/// No second paper entity is introduced.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPaper {
    pub paper: Paper,
    pub added_at: String,
    pub added_source: String,
    pub collections: Vec<LibraryCollection>,
    pub tags: Vec<LibraryTag>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddJournalResult {
    pub journal: Journal,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub batch_id: i64,
    pub trigger: String,
    pub checked_journals: i64,
    pub found_records: i64,
    pub new_papers: i64,
    pub existing_papers: i64,
    pub abstracts_added: i64,
    /// 摘要升级（质量提高 / 更完整版本被采纳）数量
    pub abstracts_upgraded: i64,
    pub waiting_for_abstract: i64,
    pub ai_success: i64,
    pub ai_failed: i64,
    pub failed_journals: i64,
    pub duration_ms: i64,
    /// 本次同步新增的论文 id（供前端自动入队 AI 分析）。
    pub new_paper_ids: Vec<i64>,
    /// 本次同步摘要升级的论文 id（供前端自动触发 abstractUpgraded 重新分析）。
    pub abstract_upgraded_ids: Vec<i64>,
}

/// 内部使用的候选论文（来自某个数据源），由同步引擎合并入库。
#[derive(Debug, Clone)]
pub struct PaperCandidate {
    pub normalized_doi: Option<String>,
    pub original_doi: Option<String>,
    pub title: Option<String>,
    pub authors: Vec<Author>,
    pub published_date: Option<String>,
    pub year: Option<i32>,
    pub abstract_text: Option<String>,
    pub abstract_source: Option<String>,
    /// Round 7 Phase 1：摘要来源落地页 URL（provenance；publisher fallback 必填，
    /// API 来源保持 None，来源身份由 abstract_source 标识）。
    pub abstract_source_url: Option<String>,
    pub url: Option<String>,
    pub publisher_article_id: Option<String>,
    pub openalex_work_id: Option<String>,
    pub discovery_source: String,
    pub source_id: Option<String>,
    pub raw_json: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    pub state: String,
    pub analysis_batch_id: Option<i64>,
    pub batch_size: i64,
    pub completed: i64,
    pub success: i64,
    pub failed: i64,
    pub skipped: i64,
    pub remaining: i64,
    pub current_paper_id: Option<i64>,
    pub current_paper_title: Option<String>,
    pub batch_started_at: Option<String>,
    pub last_progress_at: Option<String>,
    pub current_paper_started_at: Option<String>,
    pub retry_waiting: bool,
    pub retry_until: Option<String>,
    pub last_error: Option<String>,
    pub elapsed_seconds: i64,
    pub eta_seconds: Option<i64>,
    /// 上一次完成的 AI 运行摘要（队列空闲时仍可展示）。
    pub last_run: Option<LastAiRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub startup_auto_sync: bool,
    pub daily_auto_sync: bool,
    pub daily_sync_time: String,
    pub auto_analyze_new: bool,
    pub default_abstract_lang: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResult {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum UpsertOutcome {
    New(i64),
    Existing {
        id: i64,
        abstract_filled: bool,
        /// canonical 摘要升级（质量提高或更完整版本被采纳）
        abstract_upgraded: bool,
    },
}

// ================= Round 4：Batch & Activity =================

// SyncBatch 状态
#[allow(dead_code)]
pub const SBC_RUNNING: &str = "running";
pub const SBC_COMPLETED: &str = "completed";
pub const SBC_COMPLETED_WITH_ERRORS: &str = "completedWithErrors";
#[allow(dead_code)]
pub const SBC_FAILED: &str = "failed";

// AnalysisBatch 状态
pub const ABC_RUNNING: &str = "running";
pub const ABC_PAUSED: &str = "paused";
pub const ABC_COMPLETED: &str = "completed";
pub const ABC_COMPLETED_WITH_ERRORS: &str = "completedWithErrors";
pub const ABC_STOPPED: &str = "stopped";
#[allow(dead_code)]
pub const ABC_FAILED: &str = "failed";

// AnalysisBatch item 状态
#[allow(dead_code)]
pub const ABI_QUEUED: &str = "queued";
pub const ABI_RUNNING: &str = "running";
pub const ABI_SUCCEEDED: &str = "succeeded";
pub const ABI_FAILED: &str = "failed";
pub const ABI_SKIPPED: &str = "skipped";
#[allow(dead_code)]
pub const ABI_CANCELLED: &str = "cancelled";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncBatch {
    pub id: i64,
    pub trigger: String,
    pub status: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub journal_total: i64,
    pub journal_completed: i64,
    pub journal_failed: i64,
    pub records_found: i64,
    pub papers_inserted: i64,
    pub papers_existing: i64,
    pub abstracts_added: i64,
    pub waiting_abstract: i64,
    pub error_summary: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncBatchPaper {
    pub sync_batch_id: i64,
    pub paper_id: i64,
    pub result: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisBatch {
    pub id: i64,
    pub source_sync_batch_id: Option<i64>,
    pub parent_batch_id: Option<i64>,
    pub trigger: String,
    pub status: String,
    pub model_name: Option<String>,
    pub prompt_version: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub total: i64,
    pub completed: i64,
    pub succeeded: i64,
    pub failed: i64,
    pub skipped: i64,
    pub remaining: i64,
    pub error_summary: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisBatchItem {
    pub id: i64,
    pub analysis_batch_id: i64,
    pub paper_id: i64,
    pub status: String,
    pub attempt_count: i64,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub error_type: Option<String>,
    pub error_summary: Option<String>,
    pub title: Option<String>,
}

/// Public, persisted abstract recovery work. Unlike a sync batch, this runs
/// one paper/source at a time and can safely survive an app restart.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbstractRecoveryBatch {
    pub id: i64, pub status: String, pub created_at: String, pub started_at: Option<String>, pub finished_at: Option<String>,
    pub total: i64, pub completed: i64, pub recovered: i64, pub not_found: i64, pub failed: i64, pub remaining: i64, pub error_summary: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AbstractRecoveryItem {
    pub id: i64, pub batch_id: i64, pub paper_id: i64, pub title: Option<String>, pub status: String,
    pub current_source: Option<String>, pub outcome: Option<String>, pub started_at: Option<String>, pub completed_at: Option<String>,
    pub next_retry_at: Option<String>, pub error_summary: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AbstractRecoveryProgress {
    pub batch_id: i64, pub completed: i64, pub total: i64, pub current_paper_title: Option<String>, pub current_source: Option<String>,
    pub phase: String, pub recovered: i64, pub not_found: i64, pub failed: i64, pub remaining: i64,
}

/// 同步进度事件负载（Activity Bar 渲染来源之一）。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub batch_id: i64,
    pub trigger: String,
    pub journal_total: i64,
    pub journal_completed: i64,
    pub journal_failed: i64,
    pub current_journal: Option<String>,
    pub records_found: i64,
    pub papers_inserted: i64,
    pub papers_existing: i64,
    pub abstracts_added: i64,
    pub started_at: String,
}

/// 全局 Activity 状态聚合（get_activity_state 返回）。
/// 所有界面必须消费同一份该状态：顶部 badge / AI 面板 / 积压横幅 / 待处理区 / 设置页计数
/// 均从这里读取，不得各自从 papers 数组重新计算。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityState {
    pub sync_batch: Option<SyncBatch>,
    pub analysis_batch: Option<AnalysisBatch>,
    pub last_sync: Option<SyncBatch>,
    pub last_analysis: Option<AnalysisBatch>,
    pub retry_waiting: bool,
    /// 当前仍待分析数量（实时 DB 计数，与 last_analysis.total 严格区分）
    pub pending_analysis: i64,
    /// 分析失败数量（analysisFailed）
    pub analysis_failed: i64,
    /// 等待摘要数量（waitingForAbstract，不计入 pending_analysis）
    pub waiting_for_abstract: i64,
}

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
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub error_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Journal {
    pub id: i64,
    pub name: String,
    pub print_issn: Option<String>,
    pub online_issn: Option<String>,
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
    pub checked_journals: i64,
    pub found_records: i64,
    pub new_papers: i64,
    pub existing_papers: i64,
    pub abstracts_filled: i64,
    pub waiting_for_abstract: i64,
    pub ai_success: i64,
    pub ai_failed: i64,
    pub failed_journals: i64,
    pub duration_ms: i64,
    /// 本次同步新增的论文 id（供前端自动入队 AI 分析）。
    pub new_paper_ids: Vec<i64>,
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
    Existing { id: i64, abstract_filled: bool },
}

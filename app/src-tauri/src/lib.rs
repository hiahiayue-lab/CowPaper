mod abstract_quality;
mod abstract_recovery;
mod catalog;
mod content_kind;
mod recommendation;
mod tag_config;
mod ai_queue;
mod analyze;
mod api;
mod db;
mod models;
mod secure_store;
mod sync;
mod sync_coordinator;
mod util;

#[cfg(test)]
mod tests;

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tauri_plugin_notification::NotificationExt;

use crate::ai_queue::{AiQueue, QueueCommand};
use crate::models::{SyncStartResult, SyncTrigger};
use crate::secure_store::{LocalFileSecretStore, SecureStore};
use crate::sync_coordinator::{SyncCoordinator, SyncGuard};

const MAILTO: &str = "dev@cowpaper.local";
/// 启动自动同步的最小间隔（避免频繁重启触发大量请求）。
const AUTO_SYNC_MIN_INTERVAL: chrono::Duration = chrono::Duration::minutes(30);

type Db = Arc<Mutex<Connection>>;
type Secure = Arc<dyn SecureStore>;

/// One process-wide permit for title-only translation. Both automatic backlog
/// draining and the manual UI command use this guard, so the same paper can
/// never be sent to DeepSeek by two workers at once.
#[derive(Clone, Default)]
struct TitleTranslationGate(Arc<Mutex<bool>>);

struct TitleTranslationPermit(TitleTranslationGate);

impl TitleTranslationGate {
    fn acquire(&self) -> Result<TitleTranslationPermit, String> {
        let mut running = self.0.lock().map_err(|_| "标题翻译状态锁定".to_string())?;
        if *running {
            return Err("标题翻译正在进行中".to_string());
        }
        *running = true;
        Ok(TitleTranslationPermit(self.clone()))
    }
}

impl Drop for TitleTranslationPermit {
    fn drop(&mut self) {
        if let Ok(mut running) = self.0.0.lock() {
            *running = false;
        }
    }
}

/// Emit title-only lifecycle telemetry without ever exposing credentials or a
/// response body.  An emit failure must be visible in the runtime log rather
/// than silently leaving the frontend waiting for an event it will never get.
fn emit_title_event(app: &AppHandle, event: &str, payload: serde_json::Value) -> bool {
    if let Err(error) = app.emit(event, payload) {
        eprintln!("title translation emit failed: event={event}; error={error}");
        false
    } else {
        true
    }
}

fn emit_title_progress(
    app: &AppHandle,
    stage: &str,
    paper_id: Option<i64>,
    attempt: Option<usize>,
    elapsed_ms: u128,
    error: Option<&str>,
) {
    let mut payload = serde_json::json!({
        "stage": stage,
        "elapsedMs": elapsed_ms,
    });
    if let Some(id) = paper_id { payload["paperId"] = serde_json::json!(id); }
    if let Some(number) = attempt { payload["attempt"] = serde_json::json!(number); }
    if let Some(message) = error { payload["error"] = serde_json::json!(message); }
    let _ = emit_title_event(app, "title-translation://progress", payload);
}

/// Ensures a persisted SyncBatch cannot remain `running` when its worker
/// unwinds. Process-exit recovery below covers the case where Drop cannot run.
struct SyncBatchFinalizer {
    db: Db,
    batch_id: i64,
    finalized: bool,
}

impl SyncBatchFinalizer {
    fn new(db: Db, batch_id: i64) -> Self {
        Self { db, batch_id, finalized: false }
    }

    fn mark_finalized(&mut self) {
        self.finalized = true;
    }
}

impl Drop for SyncBatchFinalizer {
    fn drop(&mut self) {
        if !self.finalized && self.batch_id != 0 {
            if let Ok(conn) = self.db.lock() {
                let _ = db::finalize_sync_batch(
                    &conn,
                    self.batch_id,
                    crate::models::SBC_FAILED,
                    Some("同步任务异常终止"),
                );
            }
        }
    }
}

// ---------- 期刊 ----------

/// Post-Sync 自动分析目标合并（Round 5B.1）：一次 sync 最多一个自动 AnalysisBatch。
/// 新论文受 auto_new（「同步后自动分析新论文」checkbox）控制；摘要升级论文默认自动。
/// 结果按 paper id 去重且只出现一次（前端另有 Set dedup 作为防御）。
#[allow(dead_code)] // 语义由测试锁定；前端以等价 JS 逻辑消费
pub(crate) fn post_sync_analysis_ids(new_ids: &[i64], upgraded_ids: &[i64], auto_new: bool) -> Vec<i64> {
    let mut out: Vec<i64> = Vec::with_capacity(new_ids.len() + upgraded_ids.len());
    if auto_new {
        out.extend_from_slice(new_ids);
    }
    out.extend_from_slice(upgraded_ids);
    out.sort_unstable();
    out.dedup();
    out
}

#[tauri::command]
fn list_journals(state: State<Db>) -> Result<Vec<models::Journal>, String> {
    let conn = state.inner().lock().unwrap();
    db::list_journals(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn add_journal(
    name: Option<String>,
    print_issn: Option<String>,
    online_issn: Option<String>,
    confirm_unknown: Option<bool>,
    state: State<Db>,
) -> Result<models::AddJournalResult, String> {
    let crossref = api::crossref::Crossref::new(MAILTO);
    let openalex = api::openalex::OpenAlex::new(MAILTO);
    let print = normalize_manual_issn(print_issn.as_deref(), "Print ISSN")?;
    let online = normalize_manual_issn(online_issn.as_deref(), "Online ISSN")?;
    if print.is_none() && online.is_none() {
        return Err("至少填写一个 ISSN".to_string());
    }

    // Network calls deliberately happen before taking the SQLite mutex.
    let mut supplied = Vec::new();
    for issn in [&print, &online].into_iter().flatten() {
        if !supplied.contains(issn) { supplied.push(issn.clone()); }
    }
    let mut evidence = IssnIdentityEvidence::default();
    for issn in &supplied {
        let crossref_meta = crossref.journal_meta(issn);
        let openalex_identity = openalex.source_identity_by_issn(issn).ok().flatten();
        if print.as_deref() == Some(issn) {
            evidence.print_crossref = crossref_meta;
            evidence.print_openalex = openalex_identity;
        } else {
            evidence.online_crossref = crossref_meta;
            evidence.online_openalex = openalex_identity;
        }
    }
    let metas = evidence.crossref_metas();
    let oa_id = evidence.openalex_id();

    let conn = state.inner().lock().unwrap();
    let direct_ids: Vec<i64> = [&print, &online]
        .into_iter()
        .flatten()
        .map(|issn| db::resolve_journal_by_identifier(&conn, issn))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
        .into_iter()
        .flatten()
        .collect();
    let mut targets = direct_ids.clone();
    for meta in &metas {
        for value in [meta.print_issn.as_deref(), meta.online_issn.as_deref()]
            .into_iter().flatten().filter_map(crate::util::normalize_issn) {
            if let Some(id) = db::resolve_journal_by_identifier(&conn, &value).map_err(|e| e.to_string())? {
                targets.push(id);
            }
        }
        if let Some(issn_l) = meta.issn_l.as_deref().and_then(crate::util::normalize_issn) {
            if let Some(id) = db::find_journal_by_issn_l(&conn, &issn_l)
                .map_err(|e| e.to_string())?
                .or(db::resolve_journal_by_identifier(&conn, &issn_l).map_err(|e| e.to_string())?) {
                targets.push(id);
            }
        }
    }
    targets.sort_unstable();
    targets.dedup();
    if targets.len() > 1 {
        return Err("Print ISSN 与 Online ISSN 对应的期刊不一致，请检查后重试。".to_string());
    }
    let direct_same_journal = direct_ids.len() == 2 && direct_ids[0] == direct_ids[1];
    let remote_relation = match (print.as_deref(), online.as_deref()) {
        (Some(print), Some(online)) => resolve_paired_issn_identity(print, online, &evidence),
        _ => IssnIdentityRelation::Unknown,
    };
    let relation = if targets.len() > 1 {
        IssnIdentityRelation::Conflict
    } else if direct_same_journal {
        IssnIdentityRelation::Same
    } else {
        remote_relation
    };
    if relation == IssnIdentityRelation::Conflict {
        return Err("Print ISSN 与 Online ISSN 对应的期刊不一致，请检查后重试。".to_string());
    }
    if requires_unknown_pair_confirmation(print.is_some() && online.is_some(), relation, confirm_unknown.unwrap_or(false)) {
        return Err(UNKNOWN_ISSN_PAIR_CONFIRMATION.to_string());
    }

    let meta_print = metas.iter().find_map(|m| m.print_issn.as_deref().and_then(crate::util::normalize_issn));
    let meta_online = metas.iter().find_map(|m| m.online_issn.as_deref().and_then(crate::util::normalize_issn));
    let meta_issn_l = metas.iter().find_map(|m| m.issn_l.as_deref().and_then(crate::util::normalize_issn));
    let id = if let Some(id) = targets.first().copied() {
        id
    } else {
        let title = metas.first().map(|m| m.title.as_str())
            .or_else(|| name.as_deref().map(str::trim).filter(|s| !s.is_empty()))
            .ok_or_else(|| "无法解析期刊名称，请填写期刊名称后重试。".to_string())?;
        db::insert_journal(
            &conn,
            title,
            print.as_deref().or(meta_print.as_deref()),
            online.as_deref().or(meta_online.as_deref()),
            metas.first().and_then(|m| m.publisher.as_deref()),
            oa_id.as_deref(),
        ).map_err(|e| e.to_string())?
    };

    // Crossref and OpenAlex provide reliable extra identifiers; explicit user inputs
    // are then bound with their declared print/online type.
    if let Some(value) = meta_print.as_deref() { db::bind_journal_identifier(&conn, id, models::IDT_PRINT, value, Some("crossref")).map_err(|e| e.to_string())?; }
    if let Some(value) = meta_online.as_deref() { db::bind_journal_identifier(&conn, id, models::IDT_ONLINE, value, Some("crossref")).map_err(|e| e.to_string())?; }
    if let Some(value) = print.as_deref() { db::bind_journal_identifier(&conn, id, models::IDT_PRINT, value, Some("manual")).map_err(|e| e.to_string())?; }
    if let Some(value) = online.as_deref() { db::bind_journal_identifier(&conn, id, models::IDT_ONLINE, value, Some("manual")).map_err(|e| e.to_string())?; }
    for value in evidence.openalex_family() {
        // Inputs retain their print/online type.  A supplemental OpenAlex
        // family identifier may only be added when it is not already owned by
        // another canonical Journal; never turn it into an implicit merge.
        if print.as_deref() == Some(value.as_str()) || online.as_deref() == Some(value.as_str()) {
            continue;
        }
        match db::resolve_journal_by_identifier(&conn, &value).map_err(|e| e.to_string())? {
            Some(owner) if owner != id => {
                return Err("Print ISSN 与 Online ISSN 对应的期刊不一致，请检查后重试。".to_string());
            }
            Some(_) => {}
            None => db::insert_identifier(&conn, id, models::IDT_OTHER, &value, Some("openalex")).map_err(|e| e.to_string())?,
        }
    }
    db::fill_journal_issn_columns(&conn, id, print.as_deref().or(meta_print.as_deref()), online.as_deref().or(meta_online.as_deref())).map_err(|e| e.to_string())?;
    if db::get_journal_issn_l(&conn, id).map_err(|e| e.to_string())?.is_none() {
        if let Some(value) = meta_issn_l.as_deref() { db::set_journal_issn_l(&conn, id, Some(value)).map_err(|e| e.to_string())?; }
    }
    if db::get_journal_openalex_source(&conn, id).map_err(|e| e.to_string())?.is_none() {
        if let Some(value) = oa_id.as_deref() { db::set_journal_openalex_source(&conn, id, Some(value)).map_err(|e| e.to_string())?; }
    }
    let journal = db::get_journal(&conn, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "写入后读取失败".to_string())?;
    let note = if targets.is_empty() {
        if relation == IssnIdentityRelation::Unknown && print.is_some() && online.is_some() {
            Some("已按确认添加；公开元数据尚未确认两个 ISSN 的关联".to_string())
        } else { None }
    } else {
        Some("已补充到已有期刊，未创建重复".to_string())
    };
    Ok(models::AddJournalResult { journal, note })
}

fn normalize_manual_issn(input: Option<&str>, label: &str) -> Result<Option<String>, String> {
    match input.map(str::trim).filter(|s| !s.is_empty()) {
        Some(value) => crate::util::normalize_issn(value).map(Some).ok_or_else(|| format!("{} 格式无效", label)),
        None => Ok(None),
    }
}

const UNKNOWN_ISSN_PAIR_CONFIRMATION: &str = "ISSN_PAIR_UNKNOWN_CONFIRMATION";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IssnIdentityRelation { Same, Conflict, Unknown }

#[derive(Default)]
pub(crate) struct IssnIdentityEvidence {
    pub print_crossref: Option<api::crossref::JournalMeta>,
    pub online_crossref: Option<api::crossref::JournalMeta>,
    pub print_openalex: Option<api::openalex::OpenAlexSourceIdentity>,
    pub online_openalex: Option<api::openalex::OpenAlexSourceIdentity>,
}

impl IssnIdentityEvidence {
    fn crossref_metas(&self) -> Vec<api::crossref::JournalMeta> {
        [self.print_crossref.clone(), self.online_crossref.clone()].into_iter().flatten().collect()
    }

    fn openalex_id(&self) -> Option<String> {
        self.print_openalex.as_ref().or(self.online_openalex.as_ref()).map(|identity| identity.source_id.clone())
    }

    fn openalex_family(&self) -> Vec<String> {
        let mut family = self.print_openalex.iter().chain(self.online_openalex.iter())
            .flat_map(|identity| identity.issns.iter().cloned()).collect::<Vec<_>>();
        family.sort();
        family.dedup();
        family
    }
}

/// Resolve a supplied print/online pair without treating absent or incomplete
/// public metadata as evidence of a different journal.
pub(crate) fn resolve_paired_issn_identity(
    print: &str,
    online: &str,
    evidence: &IssnIdentityEvidence,
) -> IssnIdentityRelation {
    let meta_contains_pair = |meta: &api::crossref::JournalMeta| {
        let identifiers = [meta.print_issn.as_deref(), meta.online_issn.as_deref()]
            .into_iter().flatten().filter_map(crate::util::normalize_issn).collect::<Vec<_>>();
        identifiers.contains(&print.to_string()) && identifiers.contains(&online.to_string())
    };
    if evidence.print_crossref.iter().chain(evidence.online_crossref.iter()).any(meta_contains_pair) {
        return IssnIdentityRelation::Same;
    }
    let same_crossref_issn_l = match (&evidence.print_crossref, &evidence.online_crossref) {
        (Some(left), Some(right)) => left.issn_l.as_deref().and_then(crate::util::normalize_issn)
            .zip(right.issn_l.as_deref().and_then(crate::util::normalize_issn))
            .is_some_and(|(left, right)| left == right),
        _ => false,
    };
    if same_crossref_issn_l { return IssnIdentityRelation::Same; }

    let source_has_pair = |identity: &api::openalex::OpenAlexSourceIdentity| {
        identity.issns.iter().any(|value| value == print) && identity.issns.iter().any(|value| value == online)
    };
    if evidence.print_openalex.iter().chain(evidence.online_openalex.iter()).any(source_has_pair) {
        return IssnIdentityRelation::Same;
    }
    let (Some(left), Some(right)) = (&evidence.print_openalex, &evidence.online_openalex) else {
        return IssnIdentityRelation::Unknown;
    };
    if left.source_id == right.source_id {
        return IssnIdentityRelation::Same;
    }
    if left.issn_l.is_some() && left.issn_l == right.issn_l {
        return IssnIdentityRelation::Same;
    }
    let has_shared_family = left.issns.iter().any(|value| right.issns.contains(value));
    if has_shared_family { return IssnIdentityRelation::Same; }
    // Both source records resolve successfully and declare distinct canonical
    // identities without a shared family: this is positive conflict evidence.
    IssnIdentityRelation::Conflict
}

pub(crate) fn requires_unknown_pair_confirmation(
    has_pair: bool,
    relation: IssnIdentityRelation,
    confirmed: bool,
) -> bool {
    has_pair && relation == IssnIdentityRelation::Unknown && !confirmed
}

#[tauri::command]
fn set_journal_enabled(id: i64, enabled: bool, state: State<Db>) -> Result<(), String> {
    let conn = state.inner().lock().unwrap();
    db::set_journal_enabled(&conn, id, enabled).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_journal(id: i64, state: State<Db>) -> Result<(), String> {
    let conn = state.inner().lock().unwrap();
    db::delete_journal(&conn, id).map_err(|e| e.to_string())
}

// ---------- Round 5A：Journal Collections（Round 5C 真实目录导入的基础命令） ----------

#[tauri::command]
fn list_collections(state: State<Db>) -> Result<Vec<models::JournalCollection>, String> {
    let conn = state.inner().lock().unwrap();
    db::list_collections(&conn).map_err(|e| e.to_string())
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
fn create_collection(
    code: String,
    name: String,
    version: Option<String>,
    effective_from: Option<String>,
    source_name: Option<String>,
    source_url: Option<String>,
    state: State<Db>,
) -> Result<i64, String> {
    let conn = state.inner().lock().unwrap();
    db::create_collection(
        &conn,
        &code,
        &name,
        version.as_deref(),
        effective_from.as_deref(),
        source_name.as_deref(),
        source_url.as_deref(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn add_collection_member(collection_id: i64, journal_id: i64, state: State<Db>) -> Result<bool, String> {
    let conn = state.inner().lock().unwrap();
    db::add_collection_member(&conn, collection_id, journal_id).map_err(|e| e.to_string())
}

/// Paper → journal → collections 的派生查询（Paper 不冗余存集合）。
#[tauri::command]
fn get_journal_collections(journal_id: i64, state: State<Db>) -> Result<Vec<models::JournalCollection>, String> {
    let conn = state.inner().lock().unwrap();
    db::collections_for_journal(&conn, journal_id).map_err(|e| e.to_string())
}

// ---------- Round 5C：Verified Journal Catalog ----------

fn load_catalog() -> Result<catalog::CatalogFile, String> {
    serde_json::from_str(catalog::CATALOG_JSON).map_err(|e| format!("catalog 解析失败: {}", e))
}

#[tauri::command]
fn list_catalog_collections(state: State<Db>) -> Result<Vec<models::CatalogCollectionView>, String> {
    let data = load_catalog()?;
    let conn = state.inner().lock().unwrap();
    let mut out = Vec::new();
    for c in &data.collections {
        let count = db::count_collection_members(&conn, &c.code).unwrap_or(0);
        out.push(models::CatalogCollectionView {
            code: c.code.clone(),
            name: c.name.clone(),
            version: c.version.clone(),
            effective_from: c.effective_from.clone(),
            source_name: c.source_name.clone(),
            source_url: c.source_url.clone(),
            count,
        });
    }
    Ok(out)
}

fn resolve_catalog_journal(conn: &rusqlite::Connection, j: &catalog::CatalogJournalDef) -> Result<Option<i64>, String> {
    let print_norm = j.print_issn.as_deref().and_then(crate::util::normalize_issn);
    let online_norm = j.online_issn.as_deref().and_then(crate::util::normalize_issn);
    let issn_l_norm = j.issn_l.as_deref().and_then(crate::util::normalize_issn);
    for n in [&print_norm, &online_norm].into_iter().flatten() {
        if let Some(id) = db::resolve_journal_by_identifier(conn, n).map_err(|e| e.to_string())? {
            return Ok(Some(id));
        }
    }
    if let Some(il) = &issn_l_norm {
        if let Some(id) = db::find_journal_by_issn_l(conn, il).map_err(|e| e.to_string())? {
            return Ok(Some(id));
        }
    }
    let mut alias_list = j.aliases.clone();
    alias_list.push(j.canonical_title.clone());
    db::find_journal_by_aliases(conn, &alias_list).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_catalog_journals(code: String, state: State<Db>) -> Result<Vec<models::CatalogJournalView>, String> {
    let data = load_catalog()?;
    let conn = state.inner().lock().unwrap();
    let mut out = Vec::new();
    for j in &data.journals {
        if !j.collections.iter().any(|c| c == &code) {
            continue;
        }
        let jid = resolve_catalog_journal(&conn, j)?;
        let subscribed = match jid {
            Some(id) => db::get_journal(&conn, id)
                .map_err(|e| e.to_string())?
                .map(|j| j.enabled)
                .unwrap_or(false),
            None => false,
        };
        out.push(models::CatalogJournalView {
            catalog_id: j.catalog_id.clone(),
            canonical_title: j.canonical_title.clone(),
            publisher: j.publisher.clone(),
            print_issn: j.print_issn.clone(),
            online_issn: j.online_issn.clone(),
            issn_l: j.issn_l.clone(),
            collections: j.collections.clone(),
            metadata_needs_review: j.metadata_needs_review,
            journal_id: jid,
            subscribed,
        });
    }
    Ok(out)
}

/// 批量订阅逻辑（命令与测试共用）：只做订阅记录（enabled=1），不同步；
/// 已订阅期刊计入 already；无效 id 计入 failed，不整体失败。
pub(crate) fn subscribe_journals_logic(
    conn: &rusqlite::Connection,
    ids: Vec<i64>,
) -> Result<models::BulkSubscribeResult, String> {
    let mut r = models::BulkSubscribeResult::default();
    for id in ids {
        match db::get_journal(conn, id).map_err(|e| e.to_string())? {
            Some(j) => {
                if j.enabled {
                    r.already += 1;
                    continue;
                }
                // Syncability 防护（Round 5C.1）：没有任何 discovery identifier 的 Journal
                // 不得静默 enabled=1（订阅后必然无法同步）。当前 51 本 catalog 全部可同步。
                let has_identifier = !db::list_journal_identifiers(conn, id)
                    .map_err(|e| e.to_string())?
                    .is_empty()
                    || j.print_issn.is_some()
                    || j.online_issn.is_some();
                if !has_identifier {
                    r.failed += 1;
                    continue;
                }
                db::set_journal_enabled(conn, id, true).map_err(|e| e.to_string())?;
                r.subscribed += 1;
            }
            None => r.failed += 1,
        }
    }
    Ok(r)
}

/// 批量订阅：只做订阅记录（enabled=1），不同步；不重复订阅已订阅期刊。
#[tauri::command]
fn subscribe_journals(ids: Vec<i64>, state: State<Db>) -> Result<models::BulkSubscribeResult, String> {
    let conn = state.inner().lock().unwrap();
    subscribe_journals_logic(&conn, ids)
}

// ---------- Round 6：每日推荐时间线与历史 ----------

fn current_daily_check_time(conn: &rusqlite::Connection) -> String {
    db::get_setting(conn, "settings.daily_sync_time")
        .unwrap_or_else(|| "09:00".into())
}

/// 当前推荐周期 + 内容（items 含 Paper 当前内容；rank/score 用 snapshot）。
#[tauri::command]
fn get_current_recommendation_run(state: State<Db>) -> Result<models::RecommendationRunView, String> {
    let now = chrono::Local::now();
    let conn = state.inner().lock().unwrap();
    let dtime = current_daily_check_time(&conn);
    let run_id = recommendation::ensure_current_recommendation_cycle(&conn, &now, &dtime)?;
    let run = db::get_recommendation_run(&conn, run_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "推荐周期不存在".to_string())?;
    let items = recommendation::run_items_with_papers(&conn, run_id)?;
    Ok(models::RecommendationRunView { run, items })
}

// ---------- Round 6.4：User Collections ----------

/// 创建用户集合（code=USER-<unique>，source_name=user）；复用 journal_collections 表。
#[tauri::command]
fn create_user_collection(name: String, state: State<Db>) -> Result<i64, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("集合名称不能为空".to_string());
    }
    if name.chars().count() > 60 {
        return Err("集合名称过长（最多 60 字）".to_string());
    }
    let conn = state.inner().lock().unwrap();
    // 同名校验（用户集合范围内，source_name='user'）
    let existing: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM journal_collections WHERE name = ?1 AND source_name = 'user'",
            rusqlite::params![name],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if existing > 0 {
        return Err("同名集合已存在".to_string());
    }
    let code = format!(
        "USER-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );
    db::create_collection(&conn, &code, &name, None, None, Some("user"), None)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn rename_collection(id: i64, name: String, state: State<Db>) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("集合名称不能为空".to_string());
    }
    let conn = state.inner().lock().unwrap();
    let code = db::collection_code_by_id(&conn, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "集合不存在".to_string())?;
    if db::is_builtin_collection_code(&code) {
        return Err("内置集合（UTD24 / FT50）不可重命名".to_string());
    }
    db::rename_collection(&conn, id, &name).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_collection(id: i64, state: State<Db>) -> Result<(), String> {
    let conn = state.inner().lock().unwrap();
    let code = db::collection_code_by_id(&conn, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "集合不存在".to_string())?;
    if db::is_builtin_collection_code(&code) {
        return Err("内置集合（UTD24 / FT50）不可删除".to_string());
    }
    db::delete_collection(&conn, id).map_err(|e| e.to_string())
}

/// 从集合移除期刊（只删 membership，不取消订阅、不删 journal/paper）。
#[tauri::command]
fn remove_collection_member(collection_id: i64, journal_id: i64, state: State<Db>) -> Result<(), String> {
    let conn = state.inner().lock().unwrap();
    let code = db::collection_code_by_id(&conn, collection_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "集合不存在".to_string())?;
    if db::is_builtin_collection_code(&code) {
        return Err("内置集合（UTD24 / FT50）成员不可修改".to_string());
    }
    db::remove_collection_member(&conn, collection_id, journal_id).map_err(|e| e.to_string())
}

/// 某集合的 journals（DB 视角，支持用户集合；含手动添加期刊）。
#[tauri::command]
fn get_collection_journals(code: String, state: State<Db>) -> Result<Vec<models::Journal>, String> {
    let conn = state.inner().lock().unwrap();
    db::list_collection_journals(&conn, &code).map_err(|e| e.to_string())
}

// ---------- Round 6.5：Versioned Tag Configuration ----------

/// 当前 active tag 配置（对比 candidate 计算 diff 用）。
#[tauri::command]
fn get_active_tag_config(state: State<Db>) -> Result<Vec<models::TagDraftItem>, String> {
    let conn = state.inner().lock().unwrap();
    let tags = db::list_tags(&conn).map_err(|e| e.to_string())?;
    Ok(tags
        .into_iter()
        .map(|t| models::TagDraftItem {
            id: t.id,
            name: t.name,
            description: t.description,
            enabled: t.enabled,
            deleted: false,
        })
        .collect())
}

/// Tags 页 baseline：scheduled 优先（继续编辑将生效的版本），否则 active。
#[tauri::command]
fn get_tag_config_baseline(state: State<Db>) -> Result<models::TagBaseline, String> {
    let conn = state.inner().lock().unwrap();
    if let Some(sched) = db::scheduled_tag_config(&conn).map_err(|e| e.to_string())? {
        let items = db::list_tag_config_items(&conn, sched.id).map_err(|e| e.to_string())?;
        let draft: Vec<models::TagDraftItem> = items
            .iter()
            .map(|it| models::TagDraftItem {
                id: it.tag_id,
                name: it.name.clone(),
                description: it.description.clone(),
                enabled: it.enabled,
                deleted: it.deleted,
            })
            .collect();
        return Ok(models::TagBaseline {
            items: draft,
            source: "scheduled".to_string(),
            scheduled_effective_cycle_key: sched.effective_cycle_key,
        });
    }
    let tags = db::list_tags(&conn).map_err(|e| e.to_string())?;
    let items: Vec<models::TagDraftItem> = tags
        .into_iter()
        .map(|t| models::TagDraftItem {
            id: t.id,
            name: t.name,
            description: t.description,
            enabled: t.enabled,
            deleted: false,
        })
        .collect();
    Ok(models::TagBaseline {
        items,
        source: "active".to_string(),
        scheduled_effective_cycle_key: None,
    })
}

/// 保存 Tag 配置。
/// mode="scheduled"：持久化为 scheduled（下个周期生效；不调 AI、不改 tags、不重排）。
/// mode="immediate"：写入 active → diff → 本地重算；AI-needed 启动 tag-only batch。
#[tauri::command]
fn save_tag_config(items: Vec<models::TagDraftItem>, mode: String, state: State<Db>, queue: State<AiQueue>) -> Result<models::SaveTagConfigResult, String> {
    if mode == "scheduled" {
        let conn = state.inner().lock().unwrap();
        let now = chrono::Local::now();
        let dtime = current_daily_check_time(&conn);
        // 下一推荐周期 key（当前 cycle 的下一天 cutoff 后）
        let cur_key = recommendation::cycle_key_for(&now, &dtime);
        let next_key = next_cycle_key(&cur_key);
        return tag_config::save_scheduled_config(&conn, &items, &next_key);
    }
    if mode != "immediate" {
        return Err("未知保存模式".to_string());
    }
    // immediate：先持久化（diff 需要新 active 生成）
    let conn = state.inner().lock().unwrap();
    let mut res = tag_config::save_immediate_config(&conn, &items)?;
    // AI-needed：added + semanticChanged（active tags 语义）
    let need_ai: Vec<String> = res
        .diff
        .added
        .iter()
        .chain(res.diff.semantic_changed.iter())
        .cloned()
        .collect();
    if !need_ai.is_empty() {
        // 目标 tags（active 中匹配名称）
        let active = tag_config::active_tags(&conn)?;
        let targets: Vec<(i64, String, String)> = active
            .iter()
            .filter(|(_, name, _)| need_ai.iter().any(|n| n == name))
            .cloned()
            .collect();
        // eligible papers：有摘要且非已分析成功（或 hash stale）—— 简化：所有有摘要且 tag 缺失/stale
        let paper_ids = db::papers_needing_tag_scores(&conn, &targets).map_err(|e| e.to_string())?;
        res.ai_needed_papers = paper_ids.len() as i64;
        if !paper_ids.is_empty() && !targets.is_empty() {
            queue.cmd_tx
                .send(crate::ai_queue::QueueCommand::TagOnlyBatch {
                    paper_ids,
                    tags: targets,
                    model: get_model_default(),
                    parent_batch_id: None,
                })
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(res)
}

/// 到下一推荐周期时激活 scheduled Tag 配置：写 tags 表 → 新 active version → diff → 本地重算 + tag-only batch。
fn activate_scheduled_tag_config_if_due(conn: &rusqlite::Connection, queue: &AiQueue, current_cycle_key: &str) -> Result<(), String> {
    let Some(sched) = db::scheduled_tag_config(conn).map_err(|e| e.to_string())? else {
        return Ok(());
    };
    let due = sched
        .effective_cycle_key
        .as_deref()
        .map(|k| k <= current_cycle_key)
        .unwrap_or(false);
    if !due {
        return Ok(());
    }
    let items = db::list_tag_config_items(conn, sched.id).map_err(|e| e.to_string())?;
    // old = 激活前 tags 表（active 语义）
    let old_tags = db::list_tags(conn).map_err(|e| e.to_string())?;
    let old_items: Vec<models::TagConfigItem> = old_tags
        .iter()
        .map(|t| models::TagConfigItem {
            version_id: 0,
            tag_id: t.id,
            name: t.name.clone(),
            description: t.description.clone(),
            enabled: t.enabled,
            deleted: false,
        })
        .collect();
    // 应用 scheduled items → tags 表
    for it in &items {
        if it.deleted {
            if it.tag_id > 0 {
                db::delete_tag(conn, it.tag_id).map_err(|e| e.to_string())?;
            }
        } else if it.tag_id > 0 {
            db::update_tag(conn, it.tag_id, &it.name, it.description.as_deref(), it.enabled)
                .map_err(|e| e.to_string())?;
        } else {
            match db::find_tag_by_name(conn, &it.name).map_err(|e| e.to_string())? {
                Some(id) => {
                    db::update_tag(conn, id, &it.name, it.description.as_deref(), it.enabled)
                        .map_err(|e| e.to_string())?;
                }
                None => {
                    db::add_tag(conn, &it.name, it.description.as_deref()).map_err(|e| e.to_string())?;
                }
            }
        }
    }
    // 删除激活前存在但 scheduled items 中已移除的 tag
    for o in &old_items {
        let in_items = items.iter().any(|it| it.tag_id > 0 && it.tag_id == o.tag_id);
        if !in_items {
            db::delete_tag(conn, o.tag_id).map_err(|e| e.to_string())?;
        }
    }
    db::create_active_tag_version(conn).map_err(|e| e.to_string())?;
    db::delete_scheduled_tag_config(conn).map_err(|e| e.to_string())?;
    // diff（scheduled items 作为 new draft 语义）
    let draft: Vec<models::TagDraftItem> = items
        .iter()
        .map(|it| models::TagDraftItem {
            id: it.tag_id,
            name: it.name.clone(),
            description: it.description.clone(),
            enabled: it.enabled,
            deleted: it.deleted,
        })
        .collect();
    let diff = tag_config::compute_diff(&old_items, &draft);
    let need_ai: Vec<String> = diff
        .added
        .iter()
        .chain(diff.semantic_changed.iter())
        .cloned()
        .collect();
    // 本地重算 removed/disabled
    if !diff.removed.is_empty() || !diff.disabled.is_empty() {
        let local = db::paper_ids_with_tag_names(conn, &diff.removed, &diff.disabled)
            .map_err(|e| e.to_string())?;
        let active = tag_config::active_tags(conn)?;
        for pid in local {
            let _ = tag_config::recompute_paper_total_score(conn, pid, &active);
        }
    }
    // tag-only batch
    if !need_ai.is_empty() {
        let active = tag_config::active_tags(conn)?;
        let targets: Vec<(i64, String, String)> = active
            .iter()
            .filter(|(_, name, _)| need_ai.iter().any(|n| n == name))
            .cloned()
            .collect();
        let paper_ids = db::papers_needing_tag_scores(conn, &targets).map_err(|e| e.to_string())?;
        if !paper_ids.is_empty() && !targets.is_empty() {
            let _ = queue.cmd_tx.send(crate::ai_queue::QueueCommand::TagOnlyBatch {
                paper_ids,
                tags: targets,
                model: get_model_default(),
                parent_batch_id: None,
            });
        }
    }
    Ok(())
}

fn get_model_default() -> String {
    // 前端通过 localStorage 保存模型；Rust 侧用默认
    "deepseek-v4-flash".to_string()
}

/// 下一推荐周期 key（按 YYYY-MM-DD 加一天）。
fn next_cycle_key(cur: &str) -> String {
    if let Ok(d) = chrono::NaiveDate::parse_from_str(cur, "%Y-%m-%d") {
        (d + chrono::Days::new(1)).format("%Y-%m-%d").to_string()
    } else {
        chrono::Local::now().format("%Y-%m-%d").to_string()
    }
}

/// 重算当前 open run（幂等；finalized 冻结不动）。
#[tauri::command]
fn refresh_current_recommendations(state: State<Db>) -> Result<i64, String> {
    let now = chrono::Local::now();
    let conn = state.inner().lock().unwrap();
    let dtime = current_daily_check_time(&conn);
    recommendation::refresh_current_recommendations(&conn, &now, &dtime)
}

/// 历史周期列表（按日期倒序）。
#[tauri::command]
fn list_recommendation_runs(state: State<Db>) -> Result<Vec<models::RecommendationRun>, String> {
    let conn = state.inner().lock().unwrap();
    db::list_recommendation_runs(&conn).map_err(|e| e.to_string())
}

/// 指定周期内容（历史快照：rank/score 用 snapshot）。
#[tauri::command]
fn get_recommendation_run(id: i64, state: State<Db>) -> Result<models::RecommendationRunView, String> {
    let conn = state.inner().lock().unwrap();
    let run = db::get_recommendation_run(&conn, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "推荐周期不存在".to_string())?;
    let items = recommendation::run_items_with_papers(&conn, id)?;
    Ok(models::RecommendationRunView { run, items })
}

#[tauri::command]
fn list_papers(journal_id: Option<i64>, state: State<Db>) -> Result<Vec<models::Paper>, String> {
    let conn = state.inner().lock().unwrap();
    db::list_papers(&conn, journal_id, 1000).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_library_papers(view: String, state: State<Db>) -> Result<Vec<models::LibraryPaper>, String> {
    let conn = state.inner().lock().unwrap();
    db::list_library_papers(&conn, &view, 1000).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_library_membership(paper_id: i64, state: State<Db>) -> Result<Option<models::LibraryMembership>, String> {
    let conn = state.inner().lock().unwrap();
    db::get_library_membership(&conn, paper_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn add_paper_to_library(
    paper_id: i64,
    collection_ids: Vec<i64>,
    tag_ids: Vec<i64>,
    added_source: Option<String>,
    state: State<Db>,
) -> Result<models::LibraryMembership, String> {
    let conn = state.inner().lock().unwrap();
    db::add_paper_to_library(
        &conn,
        paper_id,
        &collection_ids,
        &tag_ids,
        added_source.as_deref().unwrap_or("manual"),
    ).map_err(|e| e.to_string())
}

#[tauri::command]
fn remove_paper_from_library(paper_id: i64, state: State<Db>) -> Result<bool, String> {
    let conn = state.inner().lock().unwrap();
    db::remove_paper_from_library(&conn, paper_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_paper_collections(paper_id: i64, collection_ids: Vec<i64>, state: State<Db>) -> Result<(), String> {
    let conn = state.inner().lock().unwrap();
    db::set_paper_collections(&conn, paper_id, &collection_ids).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_paper_library_tags(paper_id: i64, tag_ids: Vec<i64>, state: State<Db>) -> Result<(), String> {
    let conn = state.inner().lock().unwrap();
    db::set_paper_library_tags(&conn, paper_id, &tag_ids).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_library_collections(state: State<Db>) -> Result<Vec<models::LibraryCollection>, String> {
    let conn = state.inner().lock().unwrap();
    db::list_library_collections(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_library_collection(name: String, parent_id: Option<i64>, state: State<Db>) -> Result<models::LibraryCollection, String> {
    let conn = state.inner().lock().unwrap();
    db::create_library_collection(&conn, &name, parent_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn rename_library_collection(id: i64, name: String, state: State<Db>) -> Result<(), String> {
    let conn = state.inner().lock().unwrap();
    db::rename_library_collection(&conn, id, &name).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_library_collection(id: i64, state: State<Db>) -> Result<bool, String> {
    let conn = state.inner().lock().unwrap();
    db::delete_library_collection(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_library_tags(state: State<Db>) -> Result<Vec<models::LibraryTag>, String> {
    let conn = state.inner().lock().unwrap();
    db::list_library_tags(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_library_tag(name: String, color: Option<String>, state: State<Db>) -> Result<models::LibraryTag, String> {
    let conn = state.inner().lock().unwrap();
    db::create_library_tag(&conn, &name, color.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
fn rename_library_tag(id: i64, name: String, state: State<Db>) -> Result<(), String> {
    let conn = state.inner().lock().unwrap();
    db::rename_library_tag(&conn, id, &name).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_library_tag(id: i64, state: State<Db>) -> Result<bool, String> {
    let conn = state.inner().lock().unwrap();
    db::delete_library_tag(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_library_item_metadata(
    paper_id: i64,
    state: State<Db>,
) -> Result<Option<models::LibraryItemMetadata>, String> {
    let conn = state.inner().lock().unwrap();
    db::get_library_item_metadata(&conn, paper_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_library_item_metadata(
    paper_id: i64,
    metadata: models::LibraryItemMetadataInput,
    state: State<Db>,
) -> Result<models::LibraryItemMetadata, String> {
    let conn = state.inner().lock().unwrap();
    db::set_library_item_metadata(&conn, paper_id, &metadata).map_err(|e| e.to_string())
}

#[tauri::command]
fn update_library_item_metadata(
    paper_id: i64,
    metadata: models::LibraryItemMetadataInput,
    state: State<Db>,
) -> Result<models::LibraryItemMetadata, String> {
    set_library_item_metadata(paper_id, metadata, state)
}

#[tauri::command]
fn set_library_item_note(
    paper_id: i64,
    note: Option<String>,
    state: State<Db>,
) -> Result<models::LibraryItemMetadata, String> {
    let conn = state.inner().lock().unwrap();
    db::set_library_item_note(&conn, paper_id, note.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
fn clear_library_item_overrides(
    paper_id: i64,
    state: State<Db>,
) -> Result<Option<models::LibraryItemMetadata>, String> {
    let conn = state.inner().lock().unwrap();
    db::clear_library_item_overrides(&conn, paper_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_paper_attachments(
    paper_id: i64,
    state: State<Db>,
) -> Result<Vec<models::PaperAttachment>, String> {
    let conn = state.inner().lock().unwrap();
    db::list_paper_attachments(&conn, paper_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn attach_pdf(
    paper_id: i64,
    path: String,
    state: State<Db>,
) -> Result<models::PaperAttachment, String> {
    let conn = state.inner().lock().unwrap();
    db::attach_pdf_to_paper(&conn, paper_id, &path).map_err(|e| e.to_string())
}

#[tauri::command]
fn attach_discovery_pdf(
    paper_id: i64,
    path: String,
    state: State<Db>,
) -> Result<models::PaperAttachment, String> {
    let conn = state.inner().lock().unwrap();
    db::attach_discovery_pdf(&conn, paper_id, &path).map_err(|e| e.to_string())
}

#[tauri::command]
fn detach_pdf(
    attachment_id: i64,
    state: State<Db>,
) -> Result<bool, String> {
    let conn = state.inner().lock().unwrap();
    db::detach_pdf(&conn, attachment_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn relink_pdf(
    attachment_id: i64,
    path: String,
    state: State<Db>,
) -> Result<models::PaperAttachment, String> {
    let conn = state.inner().lock().unwrap();
    db::relink_pdf(&conn, attachment_id, &path).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_pdf(
    attachment_id: i64,
    state: State<Db>,
) -> Result<(), String> {
    let conn = state.inner().lock().unwrap();
    db::open_pdf(&conn, attachment_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn reveal_pdf(
    attachment_id: i64,
    state: State<Db>,
) -> Result<(), String> {
    let conn = state.inner().lock().unwrap();
    db::reveal_pdf(&conn, attachment_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn import_pdf(
    path: String,
    confirmed_paper_id: Option<i64>,
    state: State<Db>,
) -> Result<models::ExternalPdfImportResult, String> {
    let conn = state.inner().lock().unwrap();
    db::import_external_pdf(&conn, &path, confirmed_paper_id).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_today_missing_papers(state: State<Db>) -> Result<Vec<models::Paper>, String> {
    let c = state.inner().lock().unwrap();
    db::list_current_missing_papers_for_cycle(&c, &chrono::Local::now().format("%Y-%m-%d").to_string()).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_daily_paper_summaries(state: State<Db>) -> Result<Vec<models::DailyPaperSummary>, String> {
    let c = state.inner().lock().unwrap();
    db::list_daily_paper_summaries(&c).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_daily_papers(cycle_key: String, missing_only: Option<bool>, state: State<Db>) -> Result<Vec<models::Paper>, String> {
    let c = state.inner().lock().unwrap();
    db::list_papers_for_first_seen_cycle(&c, &cycle_key, missing_only.unwrap_or(false)).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_daily_recommendation_run(cycle_key: String, state: State<Db>) -> Result<Option<models::RecommendationRunView>, String> {
    let c = state.inner().lock().unwrap();
    let Some(id) = db::find_recommendation_run_by_cycle_key(&c, &cycle_key).map_err(|e| e.to_string())? else { return Ok(None) };
    let run = db::get_recommendation_run(&c, id).map_err(|e| e.to_string())?.ok_or_else(|| "推荐周期不存在".to_string())?;
    Ok(Some(models::RecommendationRunView { run, items: recommendation::run_items_with_papers(&c, id)? }))
}

#[tauri::command]
fn set_paper_flag(id: i64, flag: String, value: bool, state: State<Db>) -> Result<(), String> {
    let conn = state.inner().lock().unwrap();
    db::set_paper_flag(&conn, id, &flag, value).map_err(|e| e.to_string())
}

// ---------- 同步（统一 SyncCoordinator，禁止重入） ----------

/// 实际同步工作（与 AI 队列完全解耦，§三十四）。
/// 每次被 coordinator 接受的同步都创建一个持久化 SyncBatch。
fn sync_task<R: Runtime>(app: &AppHandle<R>, db: &Db, ids: Option<Vec<i64>>, trigger: &str) {
    let batch_id = {
        let c = db.lock().unwrap();
        db::create_sync_batch(&c, trigger).unwrap_or(0)
    };
    let mut batch_finalizer = SyncBatchFinalizer::new(db.clone(), batch_id);
    let _ = app.emit("sync://start", ());
    let mut report = sync::run_sync(db, ids, app, MAILTO, batch_id, trigger);
    report.batch_id = batch_id;
    report.trigger = trigger.to_string();
    // finalize：部分期刊失败 → completedWithErrors；否则 completed
    {
        let c = db.lock().unwrap();
        let status = if report.failed_journals > 0 {
            crate::models::SBC_COMPLETED_WITH_ERRORS
        } else {
            crate::models::SBC_COMPLETED
        };
        let err = if report.failed_journals > 0 {
            Some(format!("{} 本期刊同步失败", report.failed_journals))
        } else {
            None
        };
        let _ = db::finalize_sync_batch(&c, batch_id, status, err.as_deref());
        let _ = db::set_setting(&c, "sync.last_auto_sync_at", &db::now_utc());
    }
    batch_finalizer.mark_finalized();
    if report.new_papers > 0 {
        let _ = app
            .notification()
            .builder()
            .title("CowPaper 发现新论文")
            .body(format!(
                "新增 {} 篇论文，共检查 {} 本期刊",
                report.new_papers, report.checked_journals
            ))
            .show();
    }
    let _ = app.emit("sync://done", &report);
}

/// worker 线程启动器类型（可注入，便于测试模拟 spawn 失败）。
type WorkerSpawner = fn(Box<dyn FnOnce() + Send + 'static>) -> Result<(), String>;

/// 默认启动器：使用 std::thread::Builder::spawn（显式返回 Result，失败不 panic）。
fn default_spawner(worker: Box<dyn FnOnce() + Send + 'static>) -> Result<(), String> {
    std::thread::Builder::new()
        .name("cowpaper-sync".to_string())
        .spawn(move || worker())
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// 所有同步入口的唯一通道：经 SyncCoordinator 获取全局锁。
/// 已运行 → 返回 syncAlreadyRunning，不启动第二个线程。
fn start_sync_task<R: Runtime>(
    app: &AppHandle<R>,
    db: &Db,
    sync: &Arc<SyncCoordinator>,
    trigger: SyncTrigger,
    ids: Option<Vec<i64>>,
) -> SyncStartResult {
    start_sync_task_with(app, db, sync, trigger, ids, default_spawner)
}

/// 带可注入 spawner 的实现（测试用 double 模拟 spawn 失败）。
/// panic-safe 生命周期：try_acquire 成功 → 立即创建 SyncGuard（调用方作用域）→
/// move 进 worker 闭包。因此：
/// - spawn 失败：worker 闭包（含 guard）被丢弃 → guard Drop → release，返回 syncWorkerStartFailed；
/// - worker panic：guard 在 unwind 时 Drop → release；
/// - worker 正常结束：guard 在闭包结束时 Drop → release。
/// 无需手写多个 release 分支。
fn start_sync_task_with<R: Runtime>(
    app: &AppHandle<R>,
    db: &Db,
    sync: &Arc<SyncCoordinator>,
    trigger: SyncTrigger,
    ids: Option<Vec<i64>>,
    spawner: WorkerSpawner,
) -> SyncStartResult {
    match sync.try_acquire(trigger) {
        Some(started_at) => {
            // guard 在 spawn 之前创建：覆盖 try_acquire 之后的整个生命周期
            let guard = SyncGuard::new(sync.clone());
            let app2 = app.clone();
            let db2 = db.clone();
            let app_err = app2.clone(); // spawn 失败分支使用（app2 将被 move 进 worker）
            let worker: Box<dyn FnOnce() + Send + 'static> = Box::new(move || {
                let _guard = guard; // move 进 worker：闭包结束 / panic 时 Drop → release
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    sync_task(&app2, &db2, ids, trigger.as_str());
                }));
                if let Err(payload) = result {
                    // 记录安全错误并发出同步失败事件；不静默吞掉 panic
                    let msg = panic_summary(&payload);
                    let _ = app2.emit("sync://error", format!("同步任务异常终止：{}", msg));
                    std::panic::resume_unwind(payload); // 触发 _guard Drop → release
                }
            });
            match spawner(worker) {
                Ok(()) => SyncStartResult {
                    started: true,
                    reason: "started".to_string(),
                    trigger: Some(trigger.as_str().to_string()),
                    started_at: Some(started_at),
                },
                Err(e) => {
                    // spawn 失败：worker 闭包（含 guard）被丢弃 → guard Drop → release。
                    // coordinator 恢复 idle，返回明确错误，不影响后续再次同步。
                    let _ = app_err.emit("sync://error", format!("同步 worker 启动失败：{}", e));
                    SyncStartResult {
                        started: false,
                        reason: "syncWorkerStartFailed".to_string(),
                        trigger: None,
                        started_at: None,
                    }
                }
            }
        }
        None => SyncStartResult {
            started: false,
            reason: "syncAlreadyRunning".to_string(),
            trigger: None,
            started_at: None,
        },
    }
}

/// 从 panic payload 提取安全摘要（不包含敏感数据；未知类型返回通用文本）。
fn panic_summary(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "未知 panic".to_string()
    }
}

/// daily 标记：仅当 coordinator 真正接受 daily 任务（started=true）才写入 last_daily_sync_date。
/// syncAlreadyRunning 不算 daily 已执行，不标记，下一 tick 可重试。
fn mark_daily_if_started(started: bool, db: &Db, today: &str) -> bool {
    if started {
        let c = db.lock().unwrap();
        let _ = db::set_setting(&c, "sync.last_daily_sync_date", today);
    }
    started
}

fn start_sync_global<R: Runtime>(app: &AppHandle<R>, trigger: SyncTrigger, ids: Option<Vec<i64>>) -> SyncStartResult {
    let db = app.state::<Db>().inner().clone();
    let sync = app.state::<Arc<SyncCoordinator>>().inner().clone();
    start_sync_task(app, &db, &sync, trigger, ids)
}

#[tauri::command]
fn sync_journals(
    trigger: SyncTrigger,
    ids: Option<Vec<i64>>,
    app: AppHandle,
    state: State<Db>,
    sync: State<Arc<SyncCoordinator>>,
) -> Result<SyncStartResult, String> {
    Ok(start_sync_task(&app, state.inner(), sync.inner(), trigger, ids))
}

/// 启动时调用：若「启动自动检查」开启且距上次同步超过阈值，则后台同步。
#[tauri::command]
fn maybe_auto_sync(
    app: AppHandle,
    state: State<Db>,
    sync: State<Arc<SyncCoordinator>>,
) -> Result<bool, String> {
    let conn = state.inner().lock().unwrap();
    let auto =
        db::get_setting(&conn, "settings.startup_auto_sync").unwrap_or_else(|| "1".into()) == "1";
    let last = db::get_setting(&conn, "sync.last_auto_sync_at").unwrap_or_default();
    let need = if last.is_empty() {
        true
    } else {
        chrono::DateTime::parse_from_rfc3339(&last)
            .map(|t| chrono::Utc::now() - t.with_timezone(&chrono::Utc) > AUTO_SYNC_MIN_INTERVAL)
            .unwrap_or(true)
    };
    drop(conn);
    if !auto || !need {
        return Ok(false);
    }
    let result = start_sync_task(&app, state.inner(), sync.inner(), SyncTrigger::Startup, None);
    Ok(result.started)
}

/// 每日自动同步调度（进程存活期间每 30s 检查一次，每天最多一次）。
fn scheduler_loop(db: Db, app: AppHandle, sync: Arc<SyncCoordinator>) {
    loop {
        std::thread::sleep(Duration::from_secs(30));
        let (daily, time, last_date) = {
            let c = db.lock().unwrap();
            let daily = db::get_setting(&c, "settings.daily_auto_sync")
                .unwrap_or_else(|| "1".into())
                == "1";
            let time = db::get_setting(&c, "settings.daily_sync_time")
                .unwrap_or_else(|| "09:00".into());
            let last_date = db::get_setting(&c, "sync.last_daily_sync_date").unwrap_or_default();
            (daily, time, last_date)
        };
        if !daily {
            continue;
        }
        let now_local = chrono::Local::now();
        let today = now_local.format("%Y-%m-%d").to_string();
        if last_date == today {
            continue;
        }
        let now_hm = now_local.format("%H:%M").to_string();
        if now_hm >= time {
            // 每日推荐周期前滚（Round 6）：finalize 昨日 open run、确保今日 open run；
            // 推荐内容由 sync://done / ai://finished 触发前端 refresh 填充。
            {
                let c = db.lock().unwrap();
                let _ = recommendation::ensure_current_recommendation_cycle(&c, &chrono::Local::now(), &time);
            }
            // 只有 daily 被 coordinator 接受（started=true）才标记"今日已计划"；
            // syncAlreadyRunning 不算执行，不标记，下一 tick 若空闲会再次尝试。
            let started = start_sync_task(&app, &db, &sync, SyncTrigger::Daily, None).started;
            mark_daily_if_started(started, &db, &today);
        }
    }
}

// ---------- 标签 ----------

#[tauri::command]
fn list_tags(state: State<Db>) -> Result<Vec<models::Tag>, String> {
    let conn = state.inner().lock().unwrap();
    db::list_tags(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn add_tag(name: String, description: Option<String>, state: State<Db>) -> Result<models::Tag, String> {
    let conn = state.inner().lock().unwrap();
    db::add_tag(&conn, &name, description.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
fn update_tag(
    id: i64,
    name: String,
    description: Option<String>,
    enabled: bool,
    state: State<Db>,
) -> Result<(), String> {
    let conn = state.inner().lock().unwrap();
    db::update_tag(&conn, id, &name, description.as_deref(), enabled).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_tag(id: i64, state: State<Db>) -> Result<(), String> {
    let conn = state.inner().lock().unwrap();
    db::delete_tag(&conn, id).map_err(|e| e.to_string())
}

// ---------- API Key（本地 secret 文件，经 SecureStore；无 get 命令暴露给前端） ----------

#[tauri::command]
fn save_api_key(key: String, store: State<Secure>) -> Result<(), String> {
    store.save(&key)
}

#[tauri::command]
fn has_api_key(store: State<Secure>) -> bool {
    store.has()
}

#[tauri::command]
fn delete_api_key(store: State<Secure>) -> Result<(), String> {
    store.delete()
}

// ---------- AI 队列 ----------

#[tauri::command]
fn start_ai(
    paper_ids: Option<Vec<i64>>,
    model: String,
    trigger: String,
    source_sync_batch_id: Option<i64>,
    queue: State<AiQueue>,
    store: State<Secure>,
) -> Result<(), String> {
    if !store.has() {
        return Err("未保存 API Key，请先在设置中保存".to_string());
    }
    queue
        .cmd_tx
        .send(QueueCommand::Start {
            paper_ids,
            model,
            trigger,
            source_sync_batch_id,
        })
        .map_err(|e| e.to_string())
}

/// Schedule title-only translations for missing-abstract papers. This is kept
/// outside the full AnalysisBatch state machine because those papers must stay
/// waitingForAbstract and ineligible for recommendation.
#[tauri::command]
fn translate_missing_titles(
    app: AppHandle,
    paper_ids: Option<Vec<i64>>,
    model: String,
    state: State<Db>,
    store: State<Secure>,
    gate: State<TitleTranslationGate>,
) -> Result<i64, String> {
    // Acquire before selecting candidates so a manual click cannot race an
    // automatic drain between selection and worker startup.
    let permit = gate.acquire()?;
    let api_key = store.get().map_err(|e| e.to_string())?
        .filter(|key| !key.is_empty())
        .ok_or_else(|| "未保存 API Key，请先在设置中保存".to_string())?;
    let candidates = {
        let conn = state.inner().lock().unwrap();
        db::list_missing_title_translation_candidates(&conn, paper_ids.as_deref())
            .map_err(|e| e.to_string())?
    };
    let scheduled = candidates.len() as i64;
    if candidates.is_empty() { return Ok(0); }
    let candidate_ids: Vec<i64> = candidates.iter().map(|(id, _)| *id).collect();
    if let Err(error) = app.emit("title-translation://started", serde_json::json!({
        "scheduled": scheduled,
        "paperIds": candidate_ids,
    })) {
        eprintln!("title translation emit failed: event=title-translation://started; error={error}");
        return Err(error.to_string());
    }
    let worker_db = state.inner().clone();
    std::thread::spawn(move || {
        let _permit = permit;
        let worker = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let batch_started = Instant::now();
            emit_title_progress(&app, "batch_start", None, None, 0, None);
            let client = api::deepseek::DeepSeek::new();
            let mut translated = 0_i64;
            let mut failed = 0_i64;
            let mut translated_ids: Vec<i64> = Vec::new();
            let mut errors: Vec<String> = Vec::new();
            for (id, title) in candidates {
                let paper_started = Instant::now();
                emit_title_progress(&app, "paper_start", Some(id), None, 0, None);
                let request_app = app.clone();
                match client.translate_title_observed(&api_key, &model, &title, |stage, attempt, elapsed_ms| {
                    emit_title_progress(&request_app, stage.as_str(), Some(id), Some(attempt), elapsed_ms, None);
                }) {
                    Ok(chinese_title) => {
                        emit_title_progress(&app, "db_write_start", Some(id), None, paper_started.elapsed().as_millis(), None);
                        let saved = match worker_db.lock() {
                            Ok(conn) => {
                                emit_title_progress(&app, "db_write_acquired", Some(id), None, paper_started.elapsed().as_millis(), None);
                                db::save_title_translation(&conn, id, &chinese_title).map_err(|e| e.to_string())
                            }
                            Err(_) => Err("数据库锁定".to_string()),
                        };
                        match saved {
                            Ok(true) => {
                                translated += 1;
                                translated_ids.push(id);
                                emit_title_progress(&app, "db_write_complete", Some(id), None, paper_started.elapsed().as_millis(), None);
                                emit_title_progress(&app, "paper_success", Some(id), None, paper_started.elapsed().as_millis(), None);
                            }
                            Ok(false) => {
                                // A stale candidate was already translated by an earlier operation;
                                // it is terminal but not a translation failure.
                                emit_title_progress(&app, "db_write_complete", Some(id), None, paper_started.elapsed().as_millis(), None);
                                emit_title_progress(&app, "paper_success", Some(id), None, paper_started.elapsed().as_millis(), None);
                            }
                            Err(err) => {
                                failed += 1;
                                let message = format!("论文 {} 保存标题失败：{}", id, err);
                                emit_title_progress(&app, "paper_failure", Some(id), None, paper_started.elapsed().as_millis(), Some(&message));
                                errors.push(message);
                            }
                        }
                    }
                    Err(err) => {
                        failed += 1;
                        let message = format!("论文 {} 标题翻译失败：{}", id, err);
                        emit_title_progress(&app, "paper_failure", Some(id), None, paper_started.elapsed().as_millis(), Some(&message));
                        errors.push(message);
                    }
                }
            }
            let payload = serde_json::json!({
                "translated": translated,
                "failed": failed,
                "translatedIds": translated_ids,
                "errors": errors,
            });
            emit_title_progress(&app, "batch_done", None, None, batch_started.elapsed().as_millis(), None);
            payload
        }));
        match worker {
            Ok(payload) => { let _ = emit_title_event(&app, "title-translation://done", payload); }
            Err(_) => {
                let message = "标题翻译 worker 异常终止";
                emit_title_progress(&app, "batch_fatal", None, None, 0, Some(message));
                let _ = emit_title_event(&app, "title-translation://fatal", serde_json::json!({ "error": message }));
            }
        }
    });
    Ok(scheduled)
}

#[tauri::command]
fn pause_ai(queue: State<AiQueue>) -> Result<(), String> {
    queue.cmd_tx.send(QueueCommand::Pause).map_err(|e| e.to_string())
}

#[tauri::command]
fn resume_ai(model: String, queue: State<AiQueue>, store: State<Secure>) -> Result<(), String> {
    if !store.has() {
        return Err("未保存 API Key，请先在设置中保存".to_string());
    }
    queue
        .cmd_tx
        .send(QueueCommand::Resume { model })
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn stop_ai(queue: State<AiQueue>) -> Result<(), String> {
    queue.cmd_tx.send(QueueCommand::Stop).map_err(|e| e.to_string())
}

#[tauri::command]
fn retry_failed_ai(
    model: String,
    parent_batch_id: Option<i64>,
    queue: State<AiQueue>,
    store: State<Secure>,
) -> Result<(), String> {
    if !store.has() {
        return Err("未保存 API Key，请先在设置中保存".to_string());
    }
    queue
        .cmd_tx
        .send(QueueCommand::RetryFailed { model, parent_batch_id })
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_ai_status(state: State<Db>) -> Result<models::AiStatus, String> {
    Ok(ai_queue::status_from_db(state.inner()))
}

/// 历史积压（待分析）数量。
#[tauri::command]
fn get_pending_ai_count(state: State<Db>) -> Result<i64, String> {
    let conn = state.inner().lock().unwrap();
    db::count_pending_papers(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_failed_ai_count(state: State<Db>) -> Result<i64, String> {
    let conn = state.inner().lock().unwrap();
    db::count_by_status(&conn, "analysisFailed").map_err(|e| e.to_string())
}

#[tauri::command]
fn get_waiting_abstract_count(state: State<Db>) -> Result<i64, String> {
    let conn = state.inner().lock().unwrap();
    db::count_waiting_for_abstract(&conn).map_err(|e| e.to_string())
}

fn start_abstract_recovery(app: AppHandle, db_arc: Db, paper_ids: Vec<i64>) -> Result<models::AbstractRecoveryBatch, String> {
    if paper_ids.is_empty() { return Err("没有需要补全摘要的论文".into()); }
    let batch = {
        let c = db_arc.lock().unwrap();
        let paper_ids = db::list_recoverable_paper_ids(&c, &paper_ids).map_err(|e| e.to_string())?;
        if paper_ids.is_empty() { return Err("当前范围内没有需要补全摘要的论文".into()); }
        if let Some(running) = db::latest_abstract_recovery_batch(&c).map_err(|e| e.to_string())? {
            if running.status == "running" { return Err("摘要补全正在进行中".into()); }
        }
        let id = db::create_abstract_recovery_batch(&c, &paper_ids).map_err(|e| e.to_string())?;
        db::get_abstract_recovery_batch(&c, id).map_err(|e| e.to_string())?.ok_or_else(|| "无法创建摘要补全批次".to_string())?
    };
    let worker_db = db_arc.clone();
    std::thread::spawn(move || {
        let result = abstract_recovery::run_batch(worker_db.clone(), batch.id, MAILTO, |progress| { let _ = app.emit("abstract://progress", &progress); });
        if let Err(err) = result {
            if let Ok(c) = worker_db.lock() { let _ = db::finalize_abstract_recovery_batch(&c, batch.id, "failed", Some(&err)); }
            let _ = app.emit("abstract://error", err);
        } else { let _ = app.emit("abstract://done", batch.id); }
    });
    Ok(batch)
}

#[tauri::command]
fn recover_paper_abstract(app: AppHandle, paper_id: i64, state: State<Db>) -> Result<models::AbstractRecoveryBatch, String> {
    start_abstract_recovery(app, state.inner().clone(), vec![paper_id])
}

#[tauri::command]
fn recover_scoped_abstracts(app: AppHandle, paper_ids: Vec<i64>, state: State<Db>) -> Result<models::AbstractRecoveryBatch, String> {
    start_abstract_recovery(app, state.inner().clone(), paper_ids)
}

#[tauri::command]
fn get_abstract_recovery_batch(id: i64, state: State<Db>) -> Result<(models::AbstractRecoveryBatch, Vec<models::AbstractRecoveryItem>), String> {
    let c = state.inner().lock().unwrap();
    let batch = db::get_abstract_recovery_batch(&c, id).map_err(|e| e.to_string())?.ok_or_else(|| "摘要补全批次不存在".to_string())?;
    let items = db::list_abstract_recovery_items(&c, id).map_err(|e| e.to_string())?;
    Ok((batch, items))
}

#[tauri::command]
fn list_abstract_recovery_batches(limit: Option<i64>, state: State<Db>) -> Result<Vec<models::AbstractRecoveryBatch>, String> {
    let c = state.inner().lock().unwrap();
    db::list_abstract_recovery_batches(&c, limit.unwrap_or(25)).map_err(|e| e.to_string())
}

/// 测试 DeepSeek 连接：Key 由 Rust 从 Keychain 读取，前端不传 Key。
#[tauri::command]
fn test_api_connection(model: String, store: State<Secure>) -> Result<models::ConnectionTestResult, String> {
    let ds = api::deepseek::DeepSeek::new();
    let result = match store.get() {
        Ok(Some(key)) if !key.is_empty() => match ds.test_connection(&key, &model) {
            Ok(msg) => models::ConnectionTestResult { ok: true, message: msg },
            Err(e) => models::ConnectionTestResult {
                ok: false,
                message: e.to_string(),
            },
        },
        _ => models::ConnectionTestResult {
            ok: false,
            message: "未保存 API Key".to_string(),
        },
    };
    Ok(result)
}

// ---------- 设置 ----------

fn read_settings(conn: &Connection) -> models::Settings {
    models::Settings {
        startup_auto_sync: db::get_setting(conn, "settings.startup_auto_sync")
            .unwrap_or_else(|| "1".into())
            == "1",
        daily_auto_sync: db::get_setting(conn, "settings.daily_auto_sync")
            .unwrap_or_else(|| "1".into())
            == "1",
        daily_sync_time: db::get_setting(conn, "settings.daily_sync_time")
            .unwrap_or_else(|| "09:00".into()),
        auto_analyze_new: db::get_setting(conn, "settings.auto_analyze_new")
            .unwrap_or_else(|| "1".into())
            == "1",
        default_abstract_lang: db::get_setting(conn, "settings.default_abstract_lang")
            .unwrap_or_else(|| "zh".into()),
    }
}

#[tauri::command]
fn get_settings(state: State<Db>) -> Result<models::Settings, String> {
    let conn = state.inner().lock().unwrap();
    Ok(read_settings(&conn))
}

#[tauri::command]
fn set_settings(s: models::Settings, state: State<Db>) -> Result<(), String> {
    if !valid_daily_sync_time(&s.daily_sync_time) {
        return Err("每日检查时间必须为 HH:MM".to_string());
    }
    let conn = state.inner().lock().unwrap();
    db::set_setting(
        &conn,
        "settings.startup_auto_sync",
        if s.startup_auto_sync { "1" } else { "0" },
    ).map_err(|e| e.to_string())?;
    db::set_setting(
        &conn,
        "settings.daily_auto_sync",
        if s.daily_auto_sync { "1" } else { "0" },
    ).map_err(|e| e.to_string())?;
    db::set_setting(&conn, "settings.daily_sync_time", &s.daily_sync_time)
        .map_err(|e| e.to_string())?;
    db::set_setting(
        &conn,
        "settings.auto_analyze_new",
        if s.auto_analyze_new { "1" } else { "0" },
    ).map_err(|e| e.to_string())?;
    db::set_setting(&conn, "settings.default_abstract_lang", &s.default_abstract_lang)
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn valid_daily_sync_time(value: &str) -> bool {
    value.len() == 5
        && value.as_bytes()[2] == b':'
        && value.as_bytes()[..2].iter().all(u8::is_ascii_digit)
        && value.as_bytes()[3..].iter().all(u8::is_ascii_digit)
        && chrono::NaiveTime::parse_from_str(value, "%H:%M").is_ok()
}

// ---------- Round 4：Activity 查询 ----------

/// 聚合全局 Activity 状态（get_activity_state 命令与一致性测试共用）。
/// pending_analysis / analysis_failed / waiting_for_abstract 为实时 DB 计数，
/// 与 last_analysis（上一次批次的 total）严格区分，杜绝"上次 7 篇"被误读成"待处理 7 篇"。
pub(crate) fn build_activity_state(conn: &Connection) -> Result<models::ActivityState, String> {
    let retry_waiting = db::get_setting(conn, "queue.retry_waiting").unwrap_or_default() == "1";
    Ok(models::ActivityState {
        sync_batch: db::get_running_sync_batch(conn).map_err(|e| e.to_string())?,
        analysis_batch: db::get_current_analysis_batch(conn).map_err(|e| e.to_string())?,
        last_sync: db::last_finished_sync_batch(conn).map_err(|e| e.to_string())?,
        last_analysis: db::last_finished_analysis_batch(conn).map_err(|e| e.to_string())?,
        retry_waiting,
        pending_analysis: db::count_pending_papers(conn).unwrap_or(0),
        analysis_failed: db::count_by_status(conn, "analysisFailed").unwrap_or(0),
        waiting_for_abstract: db::count_waiting_for_abstract(conn).unwrap_or(0),
    })
}

#[tauri::command]
fn get_activity_state(state: State<Db>) -> Result<models::ActivityState, String> {
    let conn = state.inner().lock().unwrap();
    build_activity_state(&conn)
}

#[tauri::command]
fn list_sync_batches(limit: Option<i64>, state: State<Db>) -> Result<Vec<models::SyncBatch>, String> {
    let conn = state.inner().lock().unwrap();
    db::list_sync_batches(&conn, limit.unwrap_or(50)).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_sync_batch(id: i64, state: State<Db>) -> Result<(models::SyncBatch, Vec<models::SyncBatchPaper>), String> {
    let conn = state.inner().lock().unwrap();
    let batch = db::get_sync_batch(&conn, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "SyncBatch 不存在".to_string())?;
    let papers = db::list_sync_batch_papers(&conn, id).map_err(|e| e.to_string())?;
    Ok((batch, papers))
}

#[tauri::command]
fn list_analysis_batches(limit: Option<i64>, state: State<Db>) -> Result<Vec<models::AnalysisBatch>, String> {
    let conn = state.inner().lock().unwrap();
    db::list_analysis_batches(&conn, limit.unwrap_or(50)).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_analysis_batch(id: i64, state: State<Db>) -> Result<(models::AnalysisBatch, Vec<models::AnalysisBatchItem>), String> {
    let conn = state.inner().lock().unwrap();
    let batch = db::get_analysis_batch(&conn, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "AnalysisBatch 不存在".to_string())?;
    let items = db::list_analysis_batch_items(&conn, id).map_err(|e| e.to_string())?;
    Ok((batch, items))
}

#[cfg(target_os = "macos")]
fn restore_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_minimized().unwrap_or(false) {
            let _ = window.unminimize();
        }
        if !window.is_visible().unwrap_or(false) {
            let _ = window.show();
        }
        let _ = window.set_focus();
    }
}


#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("cowpaper.db");
            let conn = db::open(&db_path)?;
            db::init(&conn)?;
            // 上次进程若在同步期间退出，持久化的 running batch 已不可能继续；
            // 先收尾，避免它永久遮蔽随后完成的同步进度。
            let _ = db::recover_interrupted_sync_batches(&conn);
            let _ = db::recover_interrupted_abstract_recovery_batches(&conn);
            // 启动恢复：中断的 analyzing 论文退回 queued（可作为剩余任务继续）
            let _ = db::recover_analyzing_to_queued(&conn);
            // 有剩余任务 → 队列显示为「已暂停」，可继续
            let active = db::count_active_queue(&conn).unwrap_or(0);
            let _ = db::set_setting(
                &conn,
                "queue.state",
                if active > 0 { "paused" } else { "idle" },
            );
            let db_arc = Arc::new(Mutex::new(conn));
            app.manage(db_arc.clone());
            app.manage(TitleTranslationGate::default());

            // 全局同步协调器（禁止重入）
            let sync_arc = Arc::new(SyncCoordinator::new());
            app.manage(sync_arc.clone());

            // 安全存储：本地 secret 文件（不再使用 macOS Keychain，避免系统授权弹窗）。
            // 路径：Application Support/CowPaper/secrets.json，目录 0700 / 文件 0600。
            let store_arc: Secure = Arc::new(LocalFileSecretStore::new(&data_dir));
            app.manage(store_arc.clone());

            // Verified Journal Catalog（Round 5C）：安装 UTD24 / FT50-2026 metadata。
            // 幂等导入；只 enrich collection membership 与 identifiers，不自动订阅任何期刊。
            {
                let c = db_arc.lock().unwrap();
                let rep = catalog::import_catalog(&c).unwrap_or_default();
                let _ = db::set_setting(
                    &c,
                    "catalog.last_import",
                    &format!(
                        "created={} merged={} members={} ids={}",
                        rep.journals_created,
                        rep.journals_merged,
                        rep.memberships_added,
                        rep.identifiers_added
                    ),
                );
                // 每日推荐周期（Round 6）：启动时前滚周期并填充当前推荐（仅本地计算，不触发 AI/Sync）
                let now = chrono::Local::now();
                let dtime = db::get_setting(&c, "settings.daily_sync_time")
                    .unwrap_or_else(|| "09:00".into());
                let _ = recommendation::refresh_current_recommendations(&c, &now, &dtime);
            }

            // AI 队列协调器（全局唯一，§三十五）
            let (cmd_tx, cmd_rx) = mpsc::channel();
            let queue_handle = AiQueue { cmd_tx };
            app.manage(queue_handle.clone_state());
            // 激活到期 scheduled Tag 配置（需 queue 已 manage；仅本地 + 队列，不调旧周期重排）
            {
                let c = db_arc.lock().unwrap();
                let now = chrono::Local::now();
                let dtime = db::get_setting(&c, "settings.daily_sync_time")
                    .unwrap_or_else(|| "09:00".into());
                let key = recommendation::cycle_key_for(&now, &dtime);
                let _ = activate_scheduled_tag_config_if_due(&c, &queue_handle, &key);
            }
            {
                let conn2 = db_arc.clone();
                let app2 = app.handle().clone();
                let store2 = store_arc.clone();
                std::thread::spawn(move || {
                    ai_queue::coordinator_loop(conn2, cmd_rx, app2, store2)
                });
            }

            // 每日同步调度
            {
                let conn3 = db_arc.clone();
                let app3 = app.handle().clone();
                let sync3 = sync_arc.clone();
                std::thread::spawn(move || scheduler_loop(conn3, app3, sync3));
            }

            // 菜单栏托盘
            let show = MenuItemBuilder::with_id("show", "显示工作台").build(app)?;
            let sync_item = MenuItemBuilder::with_id("tray_sync", "检查新论文").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;
            let menu = MenuBuilder::new(app).items(&[&show, &sync_item, &quit]).build()?;
            let icon = app
                .default_window_icon()
                .cloned()
                .expect("默认窗口图标缺失");
            let tray = TrayIconBuilder::new()
                .icon(icon)
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "tray_sync" => {
                        let _ = start_sync_global(app, SyncTrigger::Tray, None);
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            app.manage(tray);

            // 关闭窗口时隐藏到托盘，而非退出
            if let Some(win) = app.get_webview_window("main") {
                let win_clone = win.clone();
                let _ = win.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = win_clone.hide();
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_journals,
            add_journal,
            set_journal_enabled,
            delete_journal,
            list_papers,
            list_library_papers,
            get_library_membership,
            add_paper_to_library,
            remove_paper_from_library,
            set_paper_collections,
            set_paper_library_tags,
            list_library_collections,
            create_library_collection,
            rename_library_collection,
            delete_library_collection,
            list_library_tags,
            create_library_tag,
            rename_library_tag,
            delete_library_tag,
            get_library_item_metadata,
            set_library_item_metadata,
            update_library_item_metadata,
            set_library_item_note,
            clear_library_item_overrides,
            list_paper_attachments,
            attach_pdf,
            attach_discovery_pdf,
            detach_pdf,
            relink_pdf,
            open_pdf,
            reveal_pdf,
            import_pdf,
            list_today_missing_papers,
            list_daily_paper_summaries,
            list_daily_papers,
            get_daily_recommendation_run,
            set_paper_flag,
            sync_journals,
            maybe_auto_sync,
            list_tags,
            add_tag,
            update_tag,
            delete_tag,
            save_api_key,
            has_api_key,
            delete_api_key,
            start_ai,
            translate_missing_titles,
            pause_ai,
            resume_ai,
            stop_ai,
            retry_failed_ai,
            get_ai_status,
            get_pending_ai_count,
            get_failed_ai_count,
            get_waiting_abstract_count,
            recover_paper_abstract,
            recover_scoped_abstracts,
            get_abstract_recovery_batch,
            list_abstract_recovery_batches,
            test_api_connection,
            get_settings,
            set_settings,
            get_activity_state,
            list_sync_batches,
            get_sync_batch,
            list_analysis_batches,
            get_analysis_batch,
            list_collections,
            create_collection,
            add_collection_member,
            get_journal_collections,
            list_catalog_collections,
            list_catalog_journals,
            subscribe_journals,
            get_current_recommendation_run,
            refresh_current_recommendations,
            list_recommendation_runs,
            get_recommendation_run,
            create_user_collection,
            rename_collection,
            delete_collection,
            remove_collection_member,
            get_collection_journals,
            save_tag_config,
            get_tag_config_baseline,
            get_active_tag_config
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if matches!(event, tauri::RunEvent::Reopen { .. }) {
                restore_main_window(app);
            }
        });
}

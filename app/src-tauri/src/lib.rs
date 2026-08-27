mod abstract_quality;
mod catalog;
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
use std::time::Duration;

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
    issn: Option<String>,
    state: State<Db>,
) -> Result<models::AddJournalResult, String> {
    let crossref = api::crossref::Crossref::new(MAILTO);
    let openalex = api::openalex::OpenAlex::new(MAILTO);

    let issn_str = match issn.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(i) => i.to_string(),
        None => {
            let cands = crossref
                .search_issns(name.as_deref().unwrap_or(""))
                .ok_or_else(|| "按名称检索期刊失败，请改用 ISSN".to_string())?;
            cands
                .first()
                .cloned()
                .ok_or_else(|| "未找到匹配期刊，请改用 ISSN".to_string())?
        }
    };
    // 统一 normalize + checksum；非法 ISSN 不得进入 canonical identifiers
    let norm = crate::util::normalize_issn(&issn_str).ok_or_else(|| "ISSN 格式无效".to_string())?;

    let conn = state.inner().lock().unwrap();
    // 1) 已存在的 identifier 映射 → 返回已有 Journal，不创建重复
    if let Some(jid) = db::resolve_journal_by_identifier(&conn, &norm).map_err(|e| e.to_string())? {
        let journal = db::get_journal(&conn, jid)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "期刊不存在".to_string())?;
        return Ok(models::AddJournalResult {
            journal,
            note: Some("该 ISSN 已对应已有期刊，未创建重复".to_string()),
        });
    }

    let meta = crossref
        .journal_meta(&norm)
        .ok_or_else(|| "Crossref 未收录该 ISSN".to_string())?;
    // Failure to enrich with OpenAlex must not prevent a valid Crossref-backed
    // manual subscription; the sync path will retry and cache it later.
    let oa_id = openalex.source_by_issn(&norm).ok().flatten();

    // 2) ISSN-L 归并：meta 的 ISSN-L 若命中已有期刊（issn_l 列或既有 identifier），
    //    把输入 ISSN 归入该 canonical Journal，不创建新 Journal。
    let issn_l_norm = meta.issn_l.as_deref().and_then(crate::util::normalize_issn);
    if let Some(il) = &issn_l_norm {
        let merged = db::find_journal_by_issn_l(&conn, il)
            .map_err(|e| e.to_string())?
            .or(db::resolve_journal_by_identifier(&conn, il).map_err(|e| e.to_string())?);
        if let Some(jid) = merged {
            let _ = db::insert_identifier(&conn, jid, models::IDT_OTHER, &norm, Some("manual"));
            let journal = db::get_journal(&conn, jid)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "期刊不存在".to_string())?;
            return Ok(models::AddJournalResult {
                journal,
                note: Some("通过 ISSN-L 归并到已有期刊，未创建重复".to_string()),
            });
        }
    }

    // 3) 创建新 Journal + identifiers（保守合并：Crossref 明确给出的 print/online 才入库）
    let id = db::insert_journal(
        &conn,
        &meta.title,
        meta.print_issn.as_deref(),
        meta.online_issn.as_deref(),
        meta.publisher.as_deref(),
        oa_id.as_deref(),
    )
    .map_err(|e| e.to_string())?;
    if let Some(p) = meta.print_issn.as_deref().and_then(crate::util::normalize_issn) {
        let _ = db::insert_identifier(&conn, id, models::IDT_PRINT, &p, Some("crossref"));
    }
    if let Some(o) = meta.online_issn.as_deref().and_then(crate::util::normalize_issn) {
        let _ = db::insert_identifier(&conn, id, models::IDT_ONLINE, &o, Some("crossref"));
    }
    // 输入 ISSN 若不在 print/online 中，以 other 补充（用户明确给出的 identifier）
    let covered = meta
        .print_issn
        .as_deref()
        .and_then(crate::util::normalize_issn)
        .map(|x| x == norm)
        .unwrap_or(false)
        || meta
            .online_issn
            .as_deref()
            .and_then(crate::util::normalize_issn)
            .map(|x| x == norm)
            .unwrap_or(false);
    if !covered {
        let _ = db::insert_identifier(&conn, id, models::IDT_OTHER, &norm, Some("manual"));
    }
    if let Some(il) = &issn_l_norm {
        let _ = db::set_journal_issn_l(&conn, id, Some(il));
    }
    let journal = db::get_journal(&conn, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "插入后读取失败".to_string())?;
    Ok(models::AddJournalResult { journal, note: None })
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
    chrono::NaiveTime::parse_from_str(value, "%H:%M").is_ok()
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


#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("cowpaper.db");
            let conn = db::open(&db_path)?;
            db::init(&conn)?;
            // 上次进程若在同步期间退出，持久化的 running batch 已不可能继续；
            // 先收尾，避免它永久遮蔽随后完成的同步进度。
            let _ = db::recover_interrupted_sync_batches(&conn);
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
            pause_ai,
            resume_ai,
            stop_ai,
            retry_failed_ai,
            get_ai_status,
            get_pending_ai_count,
            get_failed_ai_count,
            get_waiting_abstract_count,
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

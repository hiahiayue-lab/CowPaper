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
use crate::secure_store::{KeychainStore, SecureStore};
use crate::sync_coordinator::{SyncCoordinator, SyncGuard};

const MAILTO: &str = "dev@cowpaper.local";
/// 启动自动同步的最小间隔（避免频繁重启触发大量请求）。
const AUTO_SYNC_MIN_INTERVAL: chrono::Duration = chrono::Duration::minutes(30);

type Db = Arc<Mutex<Connection>>;
type Secure = Arc<dyn SecureStore>;

// ---------- 期刊 ----------

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
    let oa_id = openalex.source_by_issn(&norm);

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
fn add_collection_member(collection_id: i64, journal_id: i64, state: State<Db>) -> Result<(), String> {
    let conn = state.inner().lock().unwrap();
    db::add_collection_member(&conn, collection_id, journal_id).map_err(|e| e.to_string())
}

/// Paper → journal → collections 的派生查询（Paper 不冗余存集合）。
#[tauri::command]
fn get_journal_collections(journal_id: i64, state: State<Db>) -> Result<Vec<models::JournalCollection>, String> {
    let conn = state.inner().lock().unwrap();
    db::collections_for_journal(&conn, journal_id).map_err(|e| e.to_string())
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

// ---------- API Key（macOS Keychain，经 SecureStore） ----------

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
    let conn = state.inner().lock().unwrap();
    let _ = db::set_setting(
        &conn,
        "settings.startup_auto_sync",
        if s.startup_auto_sync { "1" } else { "0" },
    );
    let _ = db::set_setting(
        &conn,
        "settings.daily_auto_sync",
        if s.daily_auto_sync { "1" } else { "0" },
    );
    let _ = db::set_setting(&conn, "settings.daily_sync_time", &s.daily_sync_time);
    let _ = db::set_setting(
        &conn,
        "settings.auto_analyze_new",
        if s.auto_analyze_new { "1" } else { "0" },
    );
    let _ = db::set_setting(&conn, "settings.default_abstract_lang", &s.default_abstract_lang);
    Ok(())
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

            // 安全存储（macOS Keychain）
            let store_arc: Secure = Arc::new(KeychainStore::production());
            app.manage(store_arc.clone());

            // AI 队列协调器（全局唯一，§三十五）
            let (cmd_tx, cmd_rx) = mpsc::channel();
            app.manage(AiQueue { cmd_tx });
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
            get_journal_collections
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

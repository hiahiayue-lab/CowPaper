use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde_json::json;
use tauri::{AppHandle, Emitter, Runtime};

use crate::analyze::{self, AnalyzeContext};
use crate::api::deepseek::{AiError, DeepSeek};
use crate::db;
use crate::models::{
    AiStatus, LastAiRun, ST_ANALYZING, QS_IDLE, QS_PAUSED, QS_PAUSING, QS_RUNNING, QS_STOPPING,
};
use crate::secure_store::SecureStore;

/// 最大并发 DeepSeek 请求数（保守，§八）。
pub const MAX_CONCURRENCY: usize = 2;
/// 单篇最大自动重试次数（§九）。
pub const MAX_RETRIES: u32 = 3;
/// 指数退避基准（毫秒）。
const BASE_BACKOFF_MS: u64 = 2000;

pub enum QueueCommand {
    Start {
        paper_ids: Option<Vec<i64>>,
        model: String,
    },
    Pause,
    Resume {
        model: String,
    },
    Stop,
    RetryFailed {
        model: String,
    },
}

/// 全局唯一 AI 队列句柄（单一 coordinator，§三十五）。
pub struct AiQueue {
    pub cmd_tx: Sender<QueueCommand>,
}

enum WorkerMsg {
    Retrying {
        paper_id: i64,
        wait_ms: u64,
        attempt: u32,
    },
    Done {
        paper_id: i64,
        outcome: Result<bool, AiError>,
    },
}

struct Batch {
    size: i64,
    success: i64,
    failed: i64,
    skipped: i64,
    started_at: Instant,
    batch_started_at_iso: String,
    last_progress_at_iso: String,
    current: Option<(i64, String, String)>, // (paper_id, started_iso, title)
    in_flight: usize,
    worker_rx: Receiver<WorkerMsg>,
    worker_tx: Sender<WorkerMsg>,
    ctx: Arc<AnalyzeContext>,
    creds: (String, String),
    done: bool,
    final_state: String,
    last_error: Option<String>,
    retry_paper: Option<i64>,
    retry_until_iso: Option<String>,
}

impl Batch {
    fn new(
        size: i64,
        worker_tx: Sender<WorkerMsg>,
        worker_rx: Receiver<WorkerMsg>,
        ctx: Arc<AnalyzeContext>,
        creds: (String, String),
    ) -> Self {
        let now = now_iso();
        Batch {
            size,
            success: 0,
            failed: 0,
            skipped: 0,
            started_at: Instant::now(),
            batch_started_at_iso: now.clone(),
            last_progress_at_iso: now,
            current: None,
            in_flight: 0,
            worker_rx,
            worker_tx,
            ctx,
            creds,
            done: false,
            final_state: QS_IDLE.to_string(),
            last_error: None,
            retry_paper: None,
            retry_until_iso: None,
        }
    }
}

/// 全局唯一的 AI Queue 协调器主循环（应用生命周期内常驻）。
/// API Key 由 SecureStore（macOS Keychain）读取，前端不传 Key。
pub fn coordinator_loop<R: Runtime>(
    conn: Arc<Mutex<Connection>>,
    cmd_rx: Receiver<QueueCommand>,
    app: AppHandle<R>,
    store: Arc<dyn SecureStore>,
) {
    let mut state = QS_IDLE.to_string();
    let pick_new = Arc::new(AtomicBool::new(false));
    let mut batch: Option<Batch> = None;

    loop {
        match cmd_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(cmd) => handle_command(&conn, &app, &mut state, &pick_new, &mut batch, cmd, &store),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if let Some(b) = &mut batch {
            step_batch(&conn, &app, b, &mut state, &pick_new);
            if b.done {
                let status = build_status(&conn, &b.final_state, b);
                let _ = app.emit("ai://progress", &status);
                let _ = app.emit("ai://finished", &status);
                // 记录上一次 AI 运行摘要（保留到下一次运行完成，供 idle 展示）
                {
                    let c = conn.lock().unwrap();
                    let set = |k: &str, v: &str| {
                        let _ = db::set_setting(&c, k, v);
                    };
                    set("ai.last_total", &b.size.to_string());
                    set("ai.last_success", &b.success.to_string());
                    set("ai.last_failed", &b.failed.to_string());
                    set("ai.last_skipped", &b.skipped.to_string());
                    set("ai.last_started_at", &b.batch_started_at_iso);
                    set("ai.last_finished_at", &now_iso());
                    match &b.last_error {
                        Some(e) => set("ai.last_error_summary", e),
                        None => {
                            let _ = db::set_setting(&c, "ai.last_error_summary", "");
                        }
                    }
                }
                // 清空批次计数，避免残留旧批次数字误导 UI
                {
                    let c = conn.lock().unwrap();
                    let _ = db::set_setting(&c, "queue.state", &b.final_state);
                    let _ = db::set_setting(&c, "queue.batch_size", "0");
                    let _ = db::set_setting(&c, "queue.success", "0");
                    let _ = db::set_setting(&c, "queue.failed", "0");
                    let _ = db::set_setting(&c, "queue.skipped", "0");
                    let _ = db::set_setting(&c, "queue.current_paper_id", "");
                    let _ = db::set_setting(&c, "queue.current_paper_started_at", "");
                    let _ = db::set_setting(&c, "queue.retry_waiting", "0");
                }
                state = b.final_state.clone(); // 复位内存状态
                batch = None;
                pick_new.store(false, Ordering::SeqCst);
            }
        }
    }
}

fn handle_command<R: Runtime>(
    conn: &Arc<Mutex<Connection>>,
    app: &AppHandle<R>,
    state: &mut String,
    pick_new: &AtomicBool,
    batch: &mut Option<Batch>,
    cmd: QueueCommand,
    store: &Arc<dyn SecureStore>,
) {
    match cmd {
        QueueCommand::Start { paper_ids, model } => {
            if state.as_str() != QS_IDLE {
                return; // 单一队列：已有批次时不重复启动
            }
            let Some(api_key) = read_api_key(store, app) else {
                return;
            };
            let ctx = match analyze::build_context(conn) {
                Some(c) => Arc::new(c),
                None => {
                    let _ = app.emit("ai://error", "没有启用的标签，无法分析");
                    return;
                }
            };
            {
                let c = conn.lock().unwrap();
                match &paper_ids {
                    Some(ids) => {
                        for id in ids {
                            let _ = db::enqueue_paper(&c, *id);
                        }
                    }
                    None => {
                        let pending = db::list_pending_papers(&c, None).unwrap_or_default();
                        for p in &pending {
                            let _ = db::enqueue_paper(&c, p.id);
                        }
                    }
                }
            }
            let size = {
                let c = conn.lock().unwrap();
                db::count_active_queue(&c).unwrap_or(0)
            };
            if size == 0 {
                return;
            }
            let (worker_tx, worker_rx) = mpsc::channel();
            *batch = Some(Batch::new(size, worker_tx, worker_rx, ctx, (api_key, model)));
            *state = QS_RUNNING.to_string();
            pick_new.store(true, Ordering::SeqCst);
            persist_queue_state(conn, state, batch.as_ref().unwrap());
            emit_progress(app, conn, state, batch.as_ref().unwrap());
        }
        QueueCommand::Pause => {
            if state.as_str() == QS_RUNNING || state.as_str() == QS_PAUSING {
                pick_new.store(false, Ordering::SeqCst);
                *state = QS_PAUSING.to_string();
                if let Some(b) = batch {
                    persist_queue_state(conn, state, b);
                    emit_progress(app, conn, state, b);
                }
            }
        }
        QueueCommand::Resume { model } => {
            if state.as_str() == QS_PAUSED || state.as_str() == QS_PAUSING {
                let Some(api_key) = read_api_key(store, app) else {
                    return;
                };
                pick_new.store(true, Ordering::SeqCst);
                *state = QS_RUNNING.to_string();
                if let Some(b) = batch {
                    b.creds = (api_key, model);
                    persist_queue_state(conn, state, b);
                    emit_progress(app, conn, state, b);
                }
            } else if state.as_str() == QS_IDLE {
                // 重启恢复：从 queued 残留重建批次
                let Some(api_key) = read_api_key(store, app) else {
                    return;
                };
                let ctx = match analyze::build_context(conn) {
                    Some(c) => Arc::new(c),
                    None => return,
                };
                let size = {
                    let c = conn.lock().unwrap();
                    db::count_active_queue(&c).unwrap_or(0)
                };
                if size > 0 {
                    let (worker_tx, worker_rx) = mpsc::channel();
                    *batch = Some(Batch::new(
                        size,
                        worker_tx,
                        worker_rx,
                        ctx,
                        (api_key, model),
                    ));
                    *state = QS_RUNNING.to_string();
                    pick_new.store(true, Ordering::SeqCst);
                    persist_queue_state(conn, state, batch.as_ref().unwrap());
                    emit_progress(app, conn, state, batch.as_ref().unwrap());
                }
            }
        }
        QueueCommand::Stop => match state.as_str() {
            QS_RUNNING | QS_PAUSING => {
                pick_new.store(false, Ordering::SeqCst);
                *state = QS_STOPPING.to_string();
                if let Some(b) = batch {
                    persist_queue_state(conn, state, b);
                    emit_progress(app, conn, state, b);
                }
            }
            QS_PAUSED => {
                let c = conn.lock().unwrap();
                let _ = db::revert_active_to_pending(&c);
                drop(c);
                if let Some(b) = batch {
                    b.done = true;
                    b.final_state = QS_IDLE.to_string();
                }
                *state = QS_IDLE.to_string();
            }
            _ => {}
        },
        QueueCommand::RetryFailed { model } => {
            if state.as_str() != QS_IDLE {
                return;
            }
            let Some(api_key) = read_api_key(store, app) else {
                return;
            };
            let failed_ids = {
                let c = conn.lock().unwrap();
                db::list_failed_ids(&c).unwrap_or_default()
            };
            if failed_ids.is_empty() {
                return;
            }
            let ctx = match analyze::build_context(conn) {
                Some(c) => Arc::new(c),
                None => return,
            };
            {
                let c = conn.lock().unwrap();
                let _ = db::reset_failed_to_pending(&c);
                for id in &failed_ids {
                    let _ = db::enqueue_paper(&c, *id);
                }
            }
            let size = failed_ids.len() as i64;
            let (worker_tx, worker_rx) = mpsc::channel();
            *batch = Some(Batch::new(
                size,
                worker_tx,
                worker_rx,
                ctx,
                (api_key, model),
            ));
            *state = QS_RUNNING.to_string();
            pick_new.store(true, Ordering::SeqCst);
            persist_queue_state(conn, state, batch.as_ref().unwrap());
            emit_progress(app, conn, state, batch.as_ref().unwrap());
        }
    }
}

/// 从 SecureStore 读取 API Key；缺失/失败时发错误事件并返回 None。
fn read_api_key<R: Runtime>(store: &Arc<dyn SecureStore>, app: &AppHandle<R>) -> Option<String> {
    match store.get() {
        Ok(Some(k)) if !k.is_empty() => Some(k),
        Ok(_) => {
            let _ = app.emit("ai://error", "未保存 API Key，请先在设置中保存");
            None
        }
        Err(e) => {
            let _ = app.emit("ai://error", format!("读取 API Key 失败：{}", e));
            None
        }
    }
}

fn step_batch<R: Runtime>(
    conn: &Arc<Mutex<Connection>>,
    app: &AppHandle<R>,
    b: &mut Batch,
    state: &mut String,
    pick_new: &AtomicBool,
) {
    // 1) 收取工作线程消息
    while let Ok(msg) = b.worker_rx.try_recv() {
        match msg {
            WorkerMsg::Retrying {
                paper_id,
                wait_ms,
                attempt,
            } => {
                let c = conn.lock().unwrap();
                let _ = db::set_retry_count(&c, paper_id, attempt as i64);
                drop(c);
                b.last_progress_at_iso = now_iso();
                b.retry_paper = Some(paper_id);
                b.retry_until_iso = Some(
                    (chrono::Utc::now() + chrono::Duration::milliseconds(wait_ms as i64))
                        .to_rfc3339(),
                );
                emit_progress(app, conn, state, b);
                let _ = app.emit(
                    "ai://retry",
                    json!({ "paperId": paper_id, "waitMs": wait_ms }),
                );
            }
            WorkerMsg::Done { paper_id, outcome } => {
                b.in_flight = b.in_flight.saturating_sub(1);
                b.last_progress_at_iso = now_iso();
                b.retry_paper = None;
                b.retry_until_iso = None;
                match outcome {
                    Ok(true) => {
                        b.success += 1;
                        // 状态写回 DB（生产路径 save_analysis 已写；此处幂等，保证 mock/异常路径一致）
                        let c = conn.lock().unwrap();
                        let _ = db::set_paper_status(&c, paper_id, "analysisSucceeded");
                        drop(c);
                    }
                    Ok(false) => {
                        b.skipped += 1;
                        let c = conn.lock().unwrap();
                        let _ = db::set_paper_status(&c, paper_id, "analysisSucceeded");
                        drop(c);
                    }
                    Err(e) => {
                        b.failed += 1;
                        b.last_error = Some(e.to_string());
                        let c = conn.lock().unwrap();
                        let _ = db::mark_analysis_failed(&c, paper_id);
                        drop(c);
                        // 全局配置错误（无效 Key / 模型 / 请求 schema）：暂停整个队列并提示。
                        if e.is_global_config() {
                            pick_new.store(false, Ordering::SeqCst);
                            *state = QS_PAUSING.to_string();
                            let _ = app.emit(
                                "ai://error",
                                format!("AI 服务配置错误，分析已暂停：{}", e),
                            );
                        }
                    }
                }
                b.current = None;
                emit_progress(app, conn, state, b);
            }
        }
    }

    // 2) 领取新任务（仅 running 且允许）
    if state.as_str() == QS_RUNNING {
        while b.in_flight < MAX_CONCURRENCY {
            let pid = {
                let c = conn.lock().unwrap();
                db::list_queued_ids(&c, 1).unwrap_or_default().first().copied()
            };
            let Some(pid) = pid else { break };
            let (title, _abs) = {
                let c = conn.lock().unwrap();
                db::get_paper_title_abstract(&c, pid)
                    .unwrap_or_default()
                    .unwrap_or_default()
            };
            {
                let c = conn.lock().unwrap();
                let _ = db::set_paper_status(&c, pid, ST_ANALYZING);
            }
            b.in_flight += 1;
            b.current = Some((pid, now_iso(), title));
            let wtx = b.worker_tx.clone();
            let conn2 = conn.clone();
            let ctx2 = b.ctx.clone();
            let creds2 = b.creds.clone();
            std::thread::spawn(move || {
                worker_run(conn2, wtx, creds2, pid, ctx2);
            });
            emit_progress(app, conn, state, b);
        }
    }

    // 3) 完成 / 状态迁移
    if b.in_flight == 0 {
        let remaining = {
            let c = conn.lock().unwrap();
            db::count_active_queue(&c).unwrap_or(0)
        };
        match state.as_str() {
            QS_RUNNING if remaining == 0 => {
                b.done = true;
                b.final_state = QS_IDLE.to_string();
            }
            QS_PAUSING => {
                pick_new.store(false, Ordering::SeqCst);
                *state = QS_PAUSED.to_string();
                persist_queue_state(conn, state, b);
                emit_progress(app, conn, state, b);
            }
            QS_STOPPING => {
                let c = conn.lock().unwrap();
                let _ = db::revert_active_to_pending(&c);
                drop(c);
                b.done = true;
                b.final_state = QS_IDLE.to_string();
            }
            _ => {}
        }
    }
}

/// 测试钩子：安装后 worker 用 mock 分析器替代真实 DeepSeek（仅测试构建）。
#[cfg(test)]
pub static MOCK_ANALYZE: std::sync::Mutex<
    Option<Arc<dyn Fn(i64) -> Result<bool, AiError> + Send + Sync>>,
> = std::sync::Mutex::new(None);

#[cfg(test)]
pub fn set_mock_analyzer(f: Option<Arc<dyn Fn(i64) -> Result<bool, AiError> + Send + Sync>>) {
    *MOCK_ANALYZE.lock().unwrap() = f;
}

/// 带有限重试的调用封装（429/5xx/网络 可重试，配置错误不重试，最多 MAX_RETRIES 次）。
pub(crate) fn run_with_retry(
    mut analyze: impl FnMut() -> Result<bool, AiError>,
    mut on_retry: impl FnMut(u32, u64),
) -> Result<bool, AiError> {
    let mut attempt: u32 = 0;
    loop {
        match analyze() {
            Ok(v) => return Ok(v),
            Err(e) => {
                if e.retryable() && attempt < MAX_RETRIES {
                    attempt += 1;
                    let wait_ms = match &e {
                        AiError::RateLimited(Some(s)) => s * 1000,
                        _ => BASE_BACKOFF_MS * 2u64.pow(attempt.min(5)),
                    };
                    on_retry(attempt, wait_ms);
                    std::thread::sleep(Duration::from_millis(wait_ms));
                } else {
                    return Err(e);
                }
            }
        }
    }
}

fn worker_run(
    conn: Arc<Mutex<Connection>>,
    tx: Sender<WorkerMsg>,
    creds: (String, String),
    paper_id: i64,
    ctx: Arc<AnalyzeContext>,
) {
    let (api_key, model) = creds;
    let (title, abstract_text) = {
        let c = conn.lock().unwrap();
        db::get_paper_title_abstract(&c, paper_id)
            .unwrap_or_default()
            .unwrap_or_default()
    };
    let ds = DeepSeek::new();
    let outcome = run_with_retry(
        || {
            // 测试钩子：每次尝试前检查 mock（可模拟限流/失败/成功序列）
            #[cfg(test)]
            {
                let mock = MOCK_ANALYZE.lock().unwrap();
                if let Some(f) = mock.as_ref() {
                    return f(paper_id);
                }
            }
            analyze::analyze_paper_once(
                &conn,
                &ds,
                &api_key,
                &model,
                paper_id,
                &title,
                &abstract_text,
                &ctx,
            )
        },
        |attempt, wait_ms| {
            let _ = tx.send(WorkerMsg::Retrying {
                paper_id,
                wait_ms,
                attempt,
            });
        },
    );
    let _ = tx.send(WorkerMsg::Done { paper_id, outcome });
}

// ---------- 状态持久化与查询 ----------

fn persist_queue_state(conn: &Arc<Mutex<Connection>>, state: &str, b: &Batch) {
    let c = conn.lock().unwrap();
    let set = |k: &str, v: &str| {
        let _ = db::set_setting(&c, k, v);
    };
    set("queue.state", state);
    set("queue.batch_size", &b.size.to_string());
    set("queue.success", &b.success.to_string());
    set("queue.failed", &b.failed.to_string());
    set("queue.skipped", &b.skipped.to_string());
    set("queue.batch_started_at", &b.batch_started_at_iso);
    set("queue.last_progress_at", &b.last_progress_at_iso);
    match &b.current {
        Some((id, started, _t)) => {
            set("queue.current_paper_id", &id.to_string());
            set("queue.current_paper_started_at", started);
        }
        None => {
            let _ = db::set_setting(&c, "queue.current_paper_id", "");
            let _ = db::set_setting(&c, "queue.current_paper_started_at", "");
        }
    }
    set(
        "queue.retry_waiting",
        if b.retry_paper.is_some() { "1" } else { "0" },
    );
    match &b.retry_until_iso {
        Some(u) => set("queue.retry_until", u),
        None => {
            let _ = db::set_setting(&c, "queue.retry_until", "");
        }
    }
    match &b.last_error {
        Some(e) => set("queue.last_error", e),
        None => {
            let _ = db::set_setting(&c, "queue.last_error", "");
        }
    }
}

fn build_status(conn: &Arc<Mutex<Connection>>, state: &str, b: &Batch) -> AiStatus {
    let (remaining, last_run) = {
        let c = conn.lock().unwrap();
        (
            db::count_active_queue(&c).unwrap_or(0),
            last_run_from(&c),
        )
    };
    let completed = b.size - remaining;
    let elapsed = b.started_at.elapsed().as_secs() as i64;
    let done_count = b.success + b.failed + b.skipped;
    let eta = if done_count >= 3 && remaining > 0 && elapsed > 0 {
        Some(((elapsed as f64 / done_count as f64) * remaining as f64).round() as i64)
    } else {
        None
    };
    AiStatus {
        state: state.to_string(),
        batch_size: b.size,
        completed: completed.max(0),
        success: b.success,
        failed: b.failed,
        skipped: b.skipped,
        remaining,
        current_paper_id: b.current.as_ref().map(|(id, _, _)| *id),
        current_paper_title: b.current.as_ref().map(|(_, _, t)| t.clone()),
        batch_started_at: Some(b.batch_started_at_iso.clone()),
        last_progress_at: Some(b.last_progress_at_iso.clone()),
        current_paper_started_at: b.current.as_ref().map(|(_, s, _)| s.clone()),
        retry_waiting: b.retry_paper.is_some(),
        retry_until: b.retry_until_iso.clone(),
        last_error: b.last_error.clone(),
        elapsed_seconds: elapsed,
        eta_seconds: eta,
        last_run,
    }
}

/// 从 app_state 读取上一次 AI 运行摘要。
fn last_run_from(conn: &Connection) -> Option<LastAiRun> {
    let g = |k: &str| db::get_setting(conn, k).unwrap_or_default();
    let total: i64 = g("ai.last_total").parse().unwrap_or(0);
    let success: i64 = g("ai.last_success").parse().unwrap_or(0);
    let failed: i64 = g("ai.last_failed").parse().unwrap_or(0);
    let skipped: i64 = g("ai.last_skipped").parse().unwrap_or(0);
    let started = g("ai.last_started_at");
    let finished = g("ai.last_finished_at");
    let err = g("ai.last_error_summary");
    if total == 0 && success == 0 && failed == 0 && skipped == 0 && started.is_empty() {
        return None;
    }
    Some(LastAiRun {
        total,
        success,
        failed,
        skipped,
        started_at: if started.is_empty() { None } else { Some(started) },
        finished_at: if finished.is_empty() { None } else { Some(finished) },
        error_summary: if err.is_empty() { None } else { Some(err) },
    })
}

fn emit_progress<R: Runtime>(
    app: &AppHandle<R>,
    conn: &Arc<Mutex<Connection>>,
    state: &str,
    b: &Batch,
) {
    // 先持久化，再发事件（get_ai_status 读 app_state，必须与事件同步）
    persist_queue_state(conn, state, b);
    let status = build_status(conn, state, b);
    let _ = app.emit("ai://progress", &status);
}

/// 供 get_ai_status 命令：从持久化状态 + 数据库重建 AiStatus。
pub fn status_from_db(conn: &Arc<Mutex<Connection>>) -> AiStatus {
    let c = conn.lock().unwrap();
    let g = |k: &str| db::get_setting(&c, k).unwrap_or_default();

    let state = g("queue.state");
    let state = if state.is_empty() { QS_IDLE.to_string() } else { state };
    let batch_size: i64 = g("queue.batch_size").parse().unwrap_or(0);
    let success: i64 = g("queue.success").parse().unwrap_or(0);
    let failed: i64 = g("queue.failed").parse().unwrap_or(0);
    let skipped: i64 = g("queue.skipped").parse().unwrap_or(0);
    let remaining = db::count_active_queue(&c).unwrap_or(0);
    let completed = batch_size - remaining;
    let current_paper_id: Option<i64> = g("queue.current_paper_id").parse().ok().filter(|i| *i > 0);
    let current_paper_title = current_paper_id
        .and_then(|id| db::get_paper_title_abstract(&c, id).ok().flatten())
        .map(|(t, _)| t);
    let retry_waiting = g("queue.retry_waiting") == "1";
    let bsa = g("queue.batch_started_at");
    let elapsed = if !bsa.is_empty() {
        chrono::DateTime::parse_from_rfc3339(&bsa)
            .map(|t| (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds())
            .unwrap_or(0)
    } else {
        0
    };
    let done_count = success + failed + skipped;
    let eta = if done_count >= 3 && remaining > 0 && elapsed > 0 {
        Some(((elapsed as f64 / done_count as f64) * remaining as f64).round() as i64)
    } else {
        None
    };
    let csa = g("queue.current_paper_started_at");
    let ru = g("queue.retry_until");
    let le = g("queue.last_error");
    let last_run = last_run_from(&c);

    AiStatus {
        state: state.clone(),
        batch_size,
        completed: completed.max(0),
        success,
        failed,
        skipped,
        remaining,
        current_paper_id,
        current_paper_title,
        batch_started_at: if bsa.is_empty() { None } else { Some(bsa) },
        last_progress_at: Some(g("queue.last_progress_at")).filter(|s| !s.is_empty()),
        current_paper_started_at: if csa.is_empty() { None } else { Some(csa) },
        retry_waiting,
        retry_until: if ru.is_empty() { None } else { Some(ru) },
        last_error: if le.is_empty() { None } else { Some(le) },
        elapsed_seconds: elapsed,
        eta_seconds: eta,
        last_run,
    }
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

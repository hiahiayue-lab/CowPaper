use rusqlite::{params, Connection, OptionalExtension};

use crate::db;
use crate::models::{Author, PaperCandidate, UpsertOutcome};
use crate::util::normalize_doi;

fn mem_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    db::init(&conn).unwrap();
    conn
}

fn candidate(
    doi: Option<&str>,
    title: &str,
    abstract_text: Option<&str>,
    abstract_source: Option<&str>,
) -> PaperCandidate {
    PaperCandidate {
        normalized_doi: doi.and_then(normalize_doi),
        original_doi: doi.map(str::to_string),
        title: Some(title.to_string()),
        authors: vec![Author {
            given: Some("A".into()),
            family: Some("B".into()),
            name: None,
        }],
        published_date: Some("2025-08-01".into()),
        year: Some(2025),
        abstract_text: abstract_text.map(str::to_string),
        abstract_source: abstract_source.map(str::to_string),
        url: doi.map(|d| format!("https://doi.org/{}", d)),
        publisher_article_id: None,
        openalex_work_id: None,
        discovery_source: "crossref".to_string(),
        source_id: doi.map(str::to_string),
        raw_json: None,
    }
}

#[test]
fn test_normalize_doi() {
    assert_eq!(
        normalize_doi("https://doi.org/10.1086/734873").as_deref(),
        Some("10.1086/734873")
    );
    assert_eq!(
        normalize_doi("doi:10.1086/734873").as_deref(),
        Some("10.1086/734873")
    );
    assert_eq!(
        normalize_doi(" 10.1086/734873?foo=bar ").as_deref(),
        Some("10.1086/734873")
    );
    assert_eq!(normalize_doi(""), None);
}

#[test]
fn test_dedup_by_doi() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();

    let c1 = candidate(Some("https://doi.org/10.1000/abc"), "Title", Some("abs"), Some("crossref"));
    assert!(matches!(db::upsert_paper(&conn, jid, &c1).unwrap(), UpsertOutcome::New(_)));

    let r2 = db::upsert_paper(&conn, jid, &c1).unwrap();
    match r2 {
        UpsertOutcome::Existing { abstract_filled, .. } => assert!(!abstract_filled),
        _ => panic!("expected existing"),
    }

    let papers = db::list_papers(&conn, Some(jid), 100).unwrap();
    assert_eq!(papers.len(), 1);
}

#[test]
fn test_abstract_fill_from_second_source() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();

    // Crossref：无摘要
    let c1 = candidate(Some("10.1000/abc"), "Title", None, None);
    db::upsert_paper(&conn, jid, &c1).unwrap();

    // OpenAlex：同 DOI，有摘要
    let c2 = candidate(Some("10.1000/abc"), "Title", Some("full abstract"), Some("openalex"));
    match db::upsert_paper(&conn, jid, &c2).unwrap() {
        UpsertOutcome::Existing { abstract_filled, .. } => assert!(abstract_filled),
        _ => panic!("expected existing"),
    }

    let papers = db::list_papers(&conn, Some(jid), 100).unwrap();
    assert_eq!(papers.len(), 1);
    assert_eq!(papers[0].abstract_text.as_deref(), Some("full abstract"));
    assert_eq!(papers[0].analysis_status, "pendingAnalysis");
}

#[test]
fn test_waiting_for_abstract_status() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let c = candidate(Some("10.1000/none"), "No abstract paper", None, None);
    db::upsert_paper(&conn, jid, &c).unwrap();
    let papers = db::list_papers(&conn, Some(jid), 100).unwrap();
    assert_eq!(papers[0].analysis_status, "waitingForAbstract");
}

#[test]
fn test_paper_flags() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let c = candidate(Some("10.1000/flag"), "Flag paper", Some("abs"), Some("crossref"));
    let id = match db::upsert_paper(&conn, jid, &c).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new"),
    };
    db::set_paper_flag(&conn, id, "favorite", true).unwrap();
    db::set_paper_flag(&conn, id, "read", true).unwrap();
    db::set_paper_flag(&conn, id, "ignored", true).unwrap();
    let papers = db::list_papers(&conn, Some(jid), 100).unwrap();
    assert!(papers[0].is_favorite);
    assert!(papers[0].is_read);
    assert!(papers[0].is_ignored);

    db::set_paper_flag(&conn, id, "ignored", false).unwrap();
    let papers = db::list_papers(&conn, Some(jid), 100).unwrap();
    assert!(!papers[0].is_ignored);
}

#[test]
fn test_dedup_by_title_year() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let c1 = candidate(None, "Some Paper Title", Some("abs"), Some("crossref"));
    let c2 = candidate(None, "Some Paper Title", None, None);
    db::upsert_paper(&conn, jid, &c1).unwrap();
    assert!(matches!(db::upsert_paper(&conn, jid, &c2).unwrap(), UpsertOutcome::Existing { .. }));
    assert_eq!(db::list_papers(&conn, Some(jid), 100).unwrap().len(), 1);
}

/// 联网冒烟测试（需网络，默认跳过）：真实拉取 Management Science 近 30 天并入库。
#[test]
#[ignore]
fn live_sync_smoke() {
    let conn = mem_db();
    let jid = db::insert_journal(
        &conn,
        "Management Science",
        Some("0025-1909"),
        Some("1526-5501"),
        None,
        Some("S33323087"),
    )
    .unwrap();

    let crossref = crate::api::crossref::Crossref::new("dev@cowpaper.local");
    let openalex = crate::api::openalex::OpenAlex::new("dev@cowpaper.local");
    let to = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let from = (chrono::Utc::now() - chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();

    let mut candidates = vec![];
    if let Some(w) = crossref.works("0025-1909", &from, &to) {
        candidates.extend(w.candidates);
    }
    if let Some(oa) = openalex.works("S33323087", &from, &to) {
        candidates.extend(oa);
    }
    assert!(!candidates.is_empty(), "未发现任何候选论文");

    let mut new = 0;
    let with_abstract;
    for c in &candidates {
        if let Ok(UpsertOutcome::New(_)) = db::upsert_paper(&conn, jid, c) {
            new += 1;
        }
    }
    let papers = db::list_papers(&conn, Some(jid), 500).unwrap();
    with_abstract = papers.iter().filter(|p| p.abstract_text.is_some()).count();

    println!(
        "live: candidates={}, new_papers={}, total_papers={}, with_abstract={}",
        candidates.len(),
        new,
        papers.len(),
        with_abstract
    );
    assert!(new > 0, "应至少新增一篇论文");
}

// ================= AI 队列测试 =================

#[test]
fn test_queue_db_mechanics() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let mut ids = Vec::new();
    for i in 0..5 {
        let c = candidate(
            Some(&format!("10.1000/qdb{}", i)),
            &format!("T{}", i),
            Some("abs"),
            Some("crossref"),
        );
        match db::upsert_paper(&conn, jid, &c).unwrap() {
            UpsertOutcome::New(id) => ids.push(id),
            _ => panic!("expected new"),
        }
    }
    assert_eq!(db::count_pending_papers(&conn).unwrap(), 5);

    // 入队 → queued
    for id in &ids {
        db::enqueue_paper(&conn, *id).unwrap();
    }
    assert_eq!(db::count_active_queue(&conn).unwrap(), 5);
    // 幂等：重复入队不重复
    db::enqueue_paper(&conn, ids[0]).unwrap();
    assert_eq!(db::count_active_queue(&conn).unwrap(), 5);

    // 出队（analyzing）+ 数量
    let picked = db::list_queued_ids(&conn, 2).unwrap();
    assert_eq!(picked.len(), 2);
    for p in &picked {
        db::set_paper_status(&conn, *p, "analyzing").unwrap();
    }
    assert_eq!(db::count_active_queue(&conn).unwrap(), 5); // queued+analyzing

    // 停止回退 → pendingAnalysis（不标失败）
    db::revert_active_to_pending(&conn).unwrap();
    assert_eq!(db::count_active_queue(&conn).unwrap(), 0);
    assert_eq!(db::count_pending_papers(&conn).unwrap(), 5);

    // 中断恢复：analyzing → queued
    for id in &ids {
        db::enqueue_paper(&conn, *id).unwrap();
    }
    for p in &db::list_queued_ids(&conn, 2).unwrap() {
        db::set_paper_status(&conn, *p, "analyzing").unwrap();
    }
    db::recover_analyzing_to_queued(&conn).unwrap();
    assert_eq!(db::count_by_status(&conn, "queued").unwrap(), 5);
}

#[test]
fn test_retry_logic() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use crate::ai_queue::{run_with_retry, MAX_RETRIES};
    use crate::api::deepseek::AiError;

    // 第一次 429、第二次 5xx、第三次成功 → Ok，2 次 retry
    let attempts = Arc::new(AtomicUsize::new(0));
    let retries = Arc::new(AtomicUsize::new(0));
    let a = attempts.clone();
    let r = retries.clone();
    let result = run_with_retry(
        move || {
            let n = a.fetch_add(1, Ordering::SeqCst);
            match n {
                0 => Err(AiError::RateLimited(Some(0))),
                1 => Err(AiError::Server(503)),
                _ => Ok(true),
            }
        },
        move |_, _| {
            r.fetch_add(1, Ordering::SeqCst);
        },
    );
    assert!(matches!(result, Ok(true)));
    assert_eq!(retries.load(Ordering::SeqCst), 2);

    // 一直网络失败 → 最多 MAX_RETRIES 次重试后返回 Err
    let attempts = Arc::new(AtomicUsize::new(0));
    let retries = Arc::new(AtomicUsize::new(0));
    let a = attempts.clone();
    let r = retries.clone();
    let result = run_with_retry(
        move || {
            a.fetch_add(1, Ordering::SeqCst);
            Err(AiError::Network("x".into()))
        },
        move |_, _| {
            r.fetch_add(1, Ordering::SeqCst);
        },
    );
    assert!(result.is_err());
    assert_eq!(attempts.load(Ordering::SeqCst), 1 + MAX_RETRIES as usize);
    assert_eq!(retries.load(Ordering::SeqCst), MAX_RETRIES as usize);

    // 配置错误不重试（只尝试 1 次）
    let attempts = Arc::new(AtomicUsize::new(0));
    let a = attempts.clone();
    let result = run_with_retry(
        move || {
            a.fetch_add(1, Ordering::SeqCst);
            Err(AiError::GlobalConfig { status: 401, code: None, message: "bad key".into() })
        },
        |_, _| {},
    );
    assert!(result.is_err());
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

/// 完整协调器集成测试（mock 分析器，无需真实 Key）：
/// A 正常 20 篇 / B 20 篇暂停-继续 / D 停止 / E 单篇失败 / F 429 重试。
#[test]
fn test_ai_queue_scenarios() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use crate::ai_queue::{self, AiQueue, QueueCommand};
    use crate::api::deepseek::AiError;
    use crate::models::AiStatus;
    use tauri::{Listener, Manager};

    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let handle = app.handle().clone();
    let conn = Arc::new(Mutex::new(mem_db()));
    {
        let c = conn.lock().unwrap();
        let jid = db::insert_journal(&c, "J", Some("0025-1909"), None, None, None).unwrap();
        for i in 0..20 {
            let cand = candidate(
                Some(&format!("10.1000/q{}", i)),
                &format!("Title {}", i),
                Some("abs"),
                Some("crossref"),
            );
            db::upsert_paper(&c, jid, &cand).unwrap();
        }
    }
    handle.manage(conn.clone());
    let (cmd_tx, cmd_rx) = mpsc::channel();
    handle.manage(AiQueue {
        cmd_tx: cmd_tx.clone(),
    });
    let (retry_tx, retry_rx) = mpsc::channel::<i64>();
    let _un = handle.listen("ai://retry", move |_e| {
        let _ = retry_tx.send(1);
    });

    let h2 = handle.clone();
    let c2 = conn.clone();
    let store = Arc::new(crate::secure_store::MockStore::with_key("test-key"));
    let _coord = std::thread::spawn(move || ai_queue::coordinator_loop(c2, cmd_rx, h2, store));

    let wait = |conn: &Arc<Mutex<Connection>>,
                timeout: Duration,
                pred: &dyn Fn(&AiStatus, &Arc<Mutex<Connection>>) -> bool|
     -> AiStatus {
        let deadline = Instant::now() + timeout;
        let mut last;
        loop {
            last = ai_queue::status_from_db(conn);
            if pred(&last, conn) || Instant::now() > deadline {
                return last;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let reset_pending = |conn: &Arc<Mutex<Connection>>| {
        let c = conn.lock().unwrap();
        let _ = c.execute("UPDATE papers SET analysis_status='pendingAnalysis'", []);
        drop(c);
    };
    let analyzing_count = |conn: &Arc<Mutex<Connection>>| -> i64 {
        let c = conn.lock().unwrap();
        db::count_by_status(&c, "analyzing").unwrap_or(0)
    };

    // ===== 场景 A：20 篇正常分析 =====
    ai_queue::set_mock_analyzer(Some(Arc::new(|_id| Ok(true))));
    cmd_tx
        .send(QueueCommand::Start {
            paper_ids: None,
            model: "m".into(),
        })
        .unwrap();
    let s = wait(&conn, Duration::from_secs(10), &|s, conn| {
        if s.state != "idle" {
            return false;
        }
        let c = conn.lock().unwrap();
        db::count_by_status(&c, "analysisSucceeded").unwrap() == 20
    });
    assert_eq!(s.state, "idle");
    {
        let c = conn.lock().unwrap();
        assert_eq!(db::count_by_status(&c, "analysisSucceeded").unwrap(), 20);
        assert_eq!(db::count_active_queue(&c).unwrap(), 0);
    }
    // 上次运行摘要应保留（§七：批次结束后不清零历史统计）；自然完成 → completed
    {
        let lr = ai_queue::status_from_db(&conn).last_run.expect("应有 last_run 摘要");
        assert_eq!(lr.total, 20, "last_run.total");
        assert_eq!(lr.success, 20, "last_run.success");
        assert_eq!(lr.failed, 0, "last_run.failed");
        assert_eq!(lr.remaining, 0, "last_run.remaining（自然完成为 0）");
        assert_eq!(lr.final_status, "completed", "last_run.final_status");
        assert!(lr.finished_at.is_some(), "last_run.finished_at");
    }

    // ===== 场景 B：暂停 / 继续（慢速 mock 300ms/篇） =====
    reset_pending(&conn);
    let cnt = Arc::new(AtomicUsize::new(0));
    let cnt2 = cnt.clone();
    ai_queue::set_mock_analyzer(Some(Arc::new(move |_id| {
        cnt2.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(300));
        Ok(true)
    })));
    cmd_tx
        .send(QueueCommand::Start {
            paper_ids: None,
            model: "m".into(),
        })
        .unwrap();
    let dl = Instant::now() + Duration::from_secs(10);
    while analyzing_count(&conn) < 2 && Instant::now() < dl {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(analyzing_count(&conn) >= 2, "B: 应有 2 个并发任务在跑");
    cmd_tx.send(QueueCommand::Pause).unwrap();
    let s = wait(&conn, Duration::from_secs(10), &|s, _| s.state == "paused");
    assert_eq!(s.state, "paused", "B: 应进入 paused（state={}）", s.state);
    assert_eq!(s.success, 2, "B: 暂停时已完成 2 篇（success={}）", s.success);
    assert_eq!(s.remaining, 18, "B: 剩余 18 篇（remaining={}）", s.remaining);
    // pause 不生成新的 completed last-run（仍是场景 A 的摘要）
    {
        let lr = ai_queue::status_from_db(&conn).last_run.expect("应有 last_run");
        assert_eq!(lr.final_status, "completed", "B: pause 不得写 completed/部分摘要");
        assert_eq!(lr.total, 20, "B: last_run 仍为上一轮（A）摘要");
    }
    cmd_tx
        .send(QueueCommand::Resume {
            model: "m".into(),
        })
        .unwrap();
    let s = wait(&conn, Duration::from_secs(15), &|s, conn| {
        if s.state != "idle" {
            return false;
        }
        let c = conn.lock().unwrap();
        db::count_by_status(&c, "analysisSucceeded").unwrap() == 20
    });
    assert_eq!(s.state, "idle", "B: 继续后应全部完成（state={}）", s.state);
    {
        let lr = ai_queue::status_from_db(&conn).last_run.expect("应有 last_run");
        assert_eq!(lr.final_status, "completed", "B: resume 后自然结束应为 completed");
        assert_eq!(lr.total, 20);
    }

    // ===== 场景 D：停止 =====
    reset_pending(&conn);
    ai_queue::set_mock_analyzer(Some(Arc::new(|_id| {
        std::thread::sleep(Duration::from_millis(300));
        Ok(true)
    })));
    cmd_tx
        .send(QueueCommand::Start {
            paper_ids: None,
            model: "m".into(),
        })
        .unwrap();
    let dl = Instant::now() + Duration::from_secs(10);
    while analyzing_count(&conn) < 2 && Instant::now() < dl {
        std::thread::sleep(Duration::from_millis(20));
    }
    cmd_tx.send(QueueCommand::Stop).unwrap();
    let _ = wait(&conn, Duration::from_secs(10), &|s, conn| {
        if s.state != "idle" {
            return false;
        }
        let c = conn.lock().unwrap();
        db::count_pending_papers(&c).unwrap() == 18
    });
    {
        let c = conn.lock().unwrap();
        let succeeded = db::count_by_status(&c, "analysisSucceeded").unwrap();
        let pending = db::count_pending_papers(&c).unwrap();
        let failed = db::count_by_status(&c, "analysisFailed").unwrap();
        assert_eq!(succeeded, 2, "D: 已完成结果保留（succeeded={}）", succeeded);
        assert_eq!(pending, 18, "D: 未完成回退 pending（pending={}）", pending);
        assert_eq!(failed, 0, "D: 不得标记失败（failed={}）", failed);
        assert_eq!(db::count_active_queue(&c).unwrap(), 0);
    }
    // D: stop 终态 → stopped + remaining 正确（未执行 18 篇不算 failed）
    {
        let lr = ai_queue::status_from_db(&conn).last_run.expect("应有 last_run");
        assert_eq!(lr.final_status, "stopped", "D: 停止后终态应为 stopped（实际 {}）", lr.final_status);
        assert_eq!(lr.total, 20);
        assert_eq!(lr.success, 2, "D: 已完成结果保留");
        assert_eq!(lr.remaining, 18, "D: 未执行论文数应计为 remaining=18（实际 {}）", lr.remaining);
    }

    // ===== 场景 E：单篇失败不影响后续 =====
    reset_pending(&conn);
    let first_id = {
        let c = conn.lock().unwrap();
        let mut stmt = c.prepare("SELECT id FROM papers ORDER BY id LIMIT 1").unwrap();
        stmt.query_row([], |r| r.get::<_, i64>(0)).unwrap()
    };
    ai_queue::set_mock_analyzer(Some(Arc::new(move |id| {
        if id == first_id {
            Err(AiError::Paper("mock 单篇失败".into()))
        } else {
            Ok(true)
        }
    })));
    cmd_tx
        .send(QueueCommand::Start {
            paper_ids: None,
            model: "m".into(),
        })
        .unwrap();
    let _ = wait(&conn, Duration::from_secs(10), &|s, conn| {
        if s.state != "idle" {
            return false;
        }
        let c = conn.lock().unwrap();
        db::count_by_status(&c, "analysisFailed").unwrap() == 1
            && db::count_by_status(&c, "analysisSucceeded").unwrap() == 19
    });
    {
        let c = conn.lock().unwrap();
        assert_eq!(db::count_by_status(&c, "analysisFailed").unwrap(), 1, "E: 1 篇失败");
        assert_eq!(db::count_by_status(&c, "analysisSucceeded").unwrap(), 19, "E: 其余 19 篇成功");
    }

    // ===== 场景 F：429 限流 → 等待重试 → 成功 =====
    reset_pending(&conn);
    let attempts = Arc::new(AtomicUsize::new(0));
    let at = attempts.clone();
    ai_queue::set_mock_analyzer(Some(Arc::new(move |_id| {
        let n = at.fetch_add(1, Ordering::SeqCst);
        if n < 2 {
            Err(AiError::RateLimited(Some(0)))
        } else {
            Ok(true)
        }
    })));
    let one_id = {
        let c = conn.lock().unwrap();
        let mut stmt = c.prepare("SELECT id FROM papers ORDER BY id LIMIT 1").unwrap();
        stmt.query_row([], |r| r.get::<_, i64>(0)).unwrap()
    };
    cmd_tx
        .send(QueueCommand::Start {
            paper_ids: Some(vec![one_id]),
            model: "m".into(),
        })
        .unwrap();
    let s = wait(&conn, Duration::from_secs(10), &|s, conn| {
        if s.state != "idle" {
            return false;
        }
        let c = conn.lock().unwrap();
        db::count_by_status(&c, "analysisSucceeded").unwrap() == 1
    });
    assert_eq!(s.state, "idle", "F: 应完成（state={}）", s.state);
    {
        let c = conn.lock().unwrap();
        let rc: i64 = c
            .query_row(
                "SELECT retry_count FROM papers WHERE id=?1",
                params![one_id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(rc >= 2, "F: retry_count 应 ≥2（实际 {}）", rc);
    }
    let retries = {
        let mut n = 0;
        while let Ok(_) = retry_rx.try_recv() {
            n += 1;
        }
        n
    };
    assert!(retries >= 2, "F: 应收到 ≥2 次 ai://retry 事件（实际 {}）", retries);

    // ===== 场景 G：全局配置错误 → 暂停整队（不得逐篇重复失败） =====
    reset_pending(&conn);
    ai_queue::set_mock_analyzer(Some(Arc::new(|_id| {
        Err(AiError::GlobalConfig {
            status: 401,
            code: Some("invalid_api_key".into()),
            message: "invalid api key".into(),
        })
    })));
    cmd_tx
        .send(QueueCommand::Start {
            paper_ids: None,
            model: "m".into(),
        })
        .unwrap();
    let s = wait(&conn, Duration::from_secs(10), &|s, _| s.state == "paused");
    assert_eq!(s.state, "paused", "G: 全局配置错误应暂停整队（state={}）", s.state);
    {
        let c = conn.lock().unwrap();
        let failed = db::count_by_status(&c, "analysisFailed").unwrap();
        // 并发 2：最多 2 篇 in-flight 触发配置错误；其余不得逐篇失败
        assert!(failed <= 2, "G: 只有 in-flight 的少数篇标 failed（实际 {}）", failed);
        let active = db::count_active_queue(&c).unwrap();
        assert!(active >= 1, "G: 未执行论文应保留在队列（queued/analyzing），实际 {}", active);
    }

    ai_queue::set_mock_analyzer(None);
}

/// 真实 DeepSeek 冒烟测试（需真实 API Key，默认跳过；不用 mock）。
/// 用法：
///   COWPAPER_KEY=<key> COWPAPER_MODEL=<model> cargo test live_deepseek_smoke -- --ignored --nocapture
/// 第一步：真实分析 1 篇，校验 5 个输出字段。
/// 第二步：真实分析 3–5 篇 batch，校验连续推进、成功计数、不重复分析。
#[test]
#[ignore]
fn live_deepseek_smoke() {
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use crate::ai_queue::{self, AiQueue, QueueCommand};
    use tauri::Manager;

    // 关键：确保不命中 mock（本测试必须走真实 DeepSeek）
    ai_queue::set_mock_analyzer(None);

    let key = std::env::var("COWPAPER_KEY").expect("需要 COWPAPER_KEY 环境变量");
    let model = std::env::var("COWPAPER_MODEL").unwrap_or_else(|_| "deepseek-v4-flash".into());
    assert!(!key.is_empty(), "COWPAPER_KEY 不能为空");
    println!("[live] model={}", model);

    // 真实 macOS Keychain（独立 test namespace，绝不触碰 production）：
    // 保存 → 队列经 Keychain 读取 → 用后删除（TestCleanup 兜底）
    let test_service = format!(
        "com.cowpaper.test.live.{}",
        std::process::id()
    );
    let store: Arc<dyn crate::secure_store::SecureStore> = Arc::new(
        crate::secure_store::KeychainStore::with_namespace(&test_service, "live-test-credential"),
    );
    store.save(&key).expect("真实 Keychain 写入失败");

    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let handle = app.handle().clone();
    let conn = Arc::new(Mutex::new(mem_db()));
    let jid = {
        let c = conn.lock().unwrap();
        let jid = db::insert_journal(&c, "Live Test", Some("0025-1909"), None, None, None).unwrap();
        let cand = candidate(
            Some("10.1000/live1"),
            "Pricing in Two-Sided Platforms with Network Effects",
            Some("We study how a platform should set prices for buyers and sellers in a two-sided market with network effects, and derive equilibrium pricing rules that internalize cross-side externalities. We show that optimal prices depend on the relative elasticities and the strength of cross-side network effects."),
            Some("crossref"),
        );
        db::upsert_paper(&c, jid, &cand).unwrap();
        jid
    };
    handle.manage(conn.clone());
    let (cmd_tx, cmd_rx) = mpsc::channel();
    handle.manage(AiQueue {
        cmd_tx: cmd_tx.clone(),
    });
    let h2 = handle.clone();
    let c2 = conn.clone();
    let store2 = store.clone();
    let _coord = std::thread::spawn(move || ai_queue::coordinator_loop(c2, cmd_rx, h2, store2));

    // ---------- 第一步：1 篇真实分析 ----------
    cmd_tx
        .send(QueueCommand::Start {
            paper_ids: None,
            model: model.clone(),
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let s = ai_queue::status_from_db(&conn);
        let succ = {
            let c = conn.lock().unwrap();
            db::count_by_status(&c, "analysisSucceeded").unwrap()
        };
        if succ >= 1 && s.state == "idle" {
            break;
        }
        if Instant::now() > deadline {
            panic!("1 篇真实分析超时（120s），state={}", s.state);
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    let (ct, ca, oss, tm, ts) = {
        let c = conn.lock().unwrap();
        let row: (Option<String>, Option<String>, Option<String>, Option<String>, Option<f64>) = c
            .query_row(
                "SELECT chinese_title, chinese_abstract, one_sentence_summary, tag_matches_json, total_score FROM papers WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        row
    };
    println!("[live-1] chineseTitle={:?}", ct);
    assert!(
        ct.as_deref().map(|s| !s.is_empty()).unwrap_or(false),
        "chinese_title 缺失"
    );
    assert!(
        ca.as_deref().map(|s| !s.is_empty()).unwrap_or(false),
        "chinese_abstract 缺失"
    );
    assert!(
        oss.as_deref().map(|s| !s.is_empty()).unwrap_or(false),
        "one_sentence_summary 缺失"
    );
    assert!(
        tm.as_deref().map(|s| s != "[]").unwrap_or(false),
        "tag_matches 为空"
    );
    assert!(ts.is_some(), "total_score 缺失");
    println!("[live-1] OK: 5 字段齐备, totalScore={:?}", ts);

    // ---------- 第二步：3–5 篇真实 batch ----------
    {
        let c = conn.lock().unwrap();
        for i in 2..=5 {
            let cand = candidate(
                Some(&format!("10.1000/live{}", i)),
                &format!("Real Supply Chain Paper Number {}", i),
                Some("We examine the effect of information asymmetry on supply chain contracting using a game-theoretic model with private demand information. We compare optimal contracts under symmetric and asymmetric information and characterize the efficiency loss."),
                Some("crossref"),
            );
            db::upsert_paper(&c, jid, &cand).unwrap();
        }
        let _ = c
            .execute("UPDATE papers SET analysis_status='pendingAnalysis' WHERE id IN (2,3,4,5)", [])
            .unwrap();
    }
    cmd_tx
        .send(QueueCommand::Start {
            paper_ids: Some(vec![2, 3, 4, 5]),
            model,
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        let s = ai_queue::status_from_db(&conn);
        let succ = {
            let c = conn.lock().unwrap();
            db::count_by_status(&c, "analysisSucceeded").unwrap()
        };
        if succ >= 5 && s.state == "idle" {
            break;
        }
        if Instant::now() > deadline {
            panic!("3-5 篇真实 batch 超时（300s），state={}", s.state);
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    let (succ, fail, dup) = {
        let c = conn.lock().unwrap();
        let succ = db::count_by_status(&c, "analysisSucceeded").unwrap();
        let fail = db::count_by_status(&c, "analysisFailed").unwrap();
        let dup: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM (SELECT evidence_hash, COUNT(*) c FROM papers WHERE analysis_status='analysisSucceeded' GROUP BY evidence_hash HAVING c>1)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        (succ, fail, dup)
    };
    println!("[live-2] succeeded={} failed={} duplicate_evidence={}", succ, fail, dup);
    assert!(succ >= 5, "应有 ≥5 篇成功（含第 1 篇），实际 {}", succ);
    assert_eq!(dup, 0, "不应有重复 evidence 的成功论文（未重复分析）");
    println!("[live-2] OK: batch 连续推进、成功计数正确、未重复分析");

    // 清理：删除测试写入的 Keychain 条目，恢复环境原状
    store.delete().expect("真实 Keychain 清理失败");
    println!("[live] Keychain 清理完成");
}

// ================= Round 3.5 hardening 测试 =================

#[test]
fn test_sync_coordinator_no_reentry() {
    use crate::models::SyncTrigger;
    use crate::sync_coordinator::SyncCoordinator;

    let sc = SyncCoordinator::new();
    assert!(!sc.is_running());
    assert!(sc.try_acquire(SyncTrigger::Manual).is_some());
    assert!(sc.is_running());
    // 任意其他 trigger 都不得重入
    assert!(sc.try_acquire(SyncTrigger::Startup).is_none());
    assert!(sc.try_acquire(SyncTrigger::Daily).is_none());
    assert!(sc.try_acquire(SyncTrigger::Tray).is_none());
    assert!(sc.try_acquire(SyncTrigger::JournalTest).is_none());
    let st = sc.status();
    assert!(st.started);
    assert_eq!(st.reason, "running");
    assert_eq!(st.trigger.as_deref(), Some("manual"));
    // 释放后恢复
    sc.release();
    assert!(!sc.is_running());
    assert!(sc.try_acquire(SyncTrigger::Manual).is_some());
    sc.release();
}

#[test]
fn test_duplicate_tag_normalization() {
    use crate::analyze::normalize_tag_matches;
    use crate::models::TagMatch;
    let pairs: Vec<(String, String)> = vec![
        ("平台经济".into(), "".into()),
        ("博弈论".into(), "".into()),
        ("定价".into(), "".into()),
    ];
    let t = |tag: &str, s: f64| TagMatch {
        tag: tag.into(),
        score: s,
    };
    let total = |out: &Vec<TagMatch>| out.iter().map(|m| m.score).sum::<f64>();

    // 1) 重复 tag：平台经济 x2 → 只保留一个，取最高分，totalScore 不得翻倍
    let out = normalize_tag_matches(vec![t("平台经济", 0.8), t("平台经济", 0.8)], &pairs);
    assert_eq!(out.len(), 3);
    assert_eq!(out.iter().find(|m| m.tag == "平台经济").unwrap().score, 0.8);
    assert_eq!(total(&out), 0.8, "重复 tag 不得求和");

    // 2) 重复且分数不同 → 取最高合法分
    let out = normalize_tag_matches(vec![t("平台经济", 0.4), t("平台经济", 1.0)], &pairs);
    assert_eq!(out.iter().find(|m| m.tag == "平台经济").unwrap().score, 1.0);

    // 3) 未知标签 → 丢弃
    let out = normalize_tag_matches(vec![t("不存在的标签", 1.0), t("定价", 0.6)], &pairs);
    assert!(!out.iter().any(|m| m.tag == "不存在的标签"));
    assert_eq!(total(&out), 0.6);

    // 4) 已禁用标签（不在 canonical pairs）→ 丢弃
    let out = normalize_tag_matches(vec![t("已禁用标签", 0.8)], &pairs);
    assert!(!out.iter().any(|m| m.tag == "已禁用标签"));

    // 5) 非法 score → 钳制到合法档位
    let out = normalize_tag_matches(
        vec![t("博弈论", 5.0), t("定价", -1.0), t("平台经济", 0.75)],
        &pairs,
    );
    let by = |tag: &str| out.iter().find(|m| m.tag == tag).unwrap().score;
    assert_eq!(by("博弈论"), 1.0);
    assert_eq!(by("定价"), 0.0);
    assert_eq!(by("平台经济"), 0.8);

    // 6) 正常列表
    let out = normalize_tag_matches(vec![t("平台经济", 0.8), t("定价", 0.6)], &pairs);
    assert_eq!(total(&out), 1.4);
}

#[test]
fn test_global_config_no_retry() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use crate::ai_queue::run_with_retry;
    use crate::api::deepseek::AiError;

    // 无效模型（404 model_not_found）→ GlobalConfig → 只尝试 1 次，不逐篇重试
    let attempts = Arc::new(AtomicUsize::new(0));
    let a = attempts.clone();
    let result = run_with_retry(
        move || {
            a.fetch_add(1, Ordering::SeqCst);
            Err(AiError::GlobalConfig {
                status: 404,
                code: Some("model_not_found".into()),
                message: "model not found".into(),
            })
        },
        |_, _| {},
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 1, "GlobalConfig 不得重试");
    assert!(result.err().unwrap().is_global_config());

    // 429 仍然允许 retry
    let attempts = Arc::new(AtomicUsize::new(0));
    let a = attempts.clone();
    let result = run_with_retry(
        move || {
            let n = a.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                Err(AiError::RateLimited(Some(0)))
            } else {
                Ok(true)
            }
        },
        |_, _| {},
    );
    assert!(matches!(result, Ok(true)));
    assert_eq!(attempts.load(Ordering::SeqCst), 3, "429 应重试到成功");

    // 5xx 仍有限 retry（最多 MAX_RETRIES 次后失败）
    let attempts = Arc::new(AtomicUsize::new(0));
    let a = attempts.clone();
    let result = run_with_retry(
        move || {
            a.fetch_add(1, Ordering::SeqCst);
            Err(AiError::Server(503))
        },
        |_, _| {},
    );
    assert!(result.is_err());
    assert_eq!(attempts.load(Ordering::SeqCst), 1 + crate::ai_queue::MAX_RETRIES as usize);
}

#[test]
fn test_secure_store_mock_save_has_delete() {
    use crate::secure_store::{MockStore, SecureStore};
    let store = MockStore::new();
    assert!(!store.has());
    assert!(store.get().unwrap().is_none());
    store.save("sk-abc").unwrap();
    assert!(store.has());
    assert_eq!(store.get().unwrap().unwrap(), "sk-abc");
    store.delete().unwrap();
    assert!(!store.has());
    assert!(store.get().unwrap().is_none());
}

#[test]
fn test_api_key_not_in_sqlite() {
    use crate::secure_store::{MockStore, SecureStore};
    let conn = mem_db();
    let store = MockStore::with_key("sk-test-secret-12345");
    store.save("sk-test-secret-12345").unwrap();
    assert!(store.has());

    // app_state 不得出现 key 相关键
    let keys: Vec<String> = conn
        .prepare("SELECT key FROM app_state")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        !keys.iter().any(|k| k.contains("api_key") || k.contains("keychain") || k.contains("secret")),
        "app_state 不得保存 API Key：{:?}",
        keys
    );

    // 任何表的文本列不得包含该 Key
    let needle = "sk-test-secret-12345";
    let mut found = false;
    let mut tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    tables.push("app_state".into());
    for t in &tables {
        let cols: Vec<String> = conn
            .prepare(&format!("PRAGMA table_info({})", t))
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for col in cols {
            let q = format!(
                "SELECT 1 FROM \"{}\" WHERE COALESCE(CAST(\"{}\" AS TEXT), '') LIKE ?1 LIMIT 1",
                t, col
            );
            let hit = conn
                .query_row(&q, params![&format!("%{}%", needle)], |_| Ok(()))
                .optional()
                .unwrap();
            if hit.is_some() {
                found = true;
            }
        }
    }
    assert!(!found, "API Key 不得出现在任何 SQLite 表中");
}

#[test]
fn test_migration_upgrade_preserves_data() {
    // 构造 round-2 时代旧 schema（无 chinese_*/is_favorite 等列）
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE journals (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, print_issn TEXT, online_issn TEXT, publisher TEXT, enabled INTEGER NOT NULL DEFAULT 1, priority INTEGER NOT NULL DEFAULT 0, rss_url TEXT, openalex_source_id TEXT, publisher_adapter TEXT, last_successful_sync_at TEXT, last_paper_date TEXT, coverage_status TEXT, abstract_coverage_rate REAL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
        CREATE TABLE papers (id INTEGER PRIMARY KEY AUTOINCREMENT, journal_id INTEGER NOT NULL, normalized_doi TEXT, original_doi TEXT, title TEXT, title_norm TEXT, authors_json TEXT, published_date TEXT, year INTEGER, abstract TEXT, abstract_source TEXT, abstract_retrieved_at TEXT, url TEXT, publisher_article_id TEXT, openalex_work_id TEXT, discovery_source TEXT, analysis_status TEXT NOT NULL DEFAULT 'pending', created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
        CREATE TABLE source_records (id INTEGER PRIMARY KEY AUTOINCREMENT, paper_id INTEGER NOT NULL, source TEXT NOT NULL, source_id TEXT, raw_json TEXT, retrieved_at TEXT NOT NULL);
        CREATE TABLE tags (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE, description TEXT, enabled INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
        INSERT INTO journals (name, created_at, updated_at) VALUES ('Old J', 't', 't');
        INSERT INTO papers (journal_id, title, abstract, analysis_status, created_at, updated_at) VALUES (1, 'Old Paper', 'old abstract', 'pending', 't', 't');
        INSERT INTO tags (name, created_at, updated_at) VALUES ('旧标签', 't', 't');
        "#,
    )
    .unwrap();

    // 升级
    db::init(&conn).unwrap();

    // 数据保留
    let title: String = conn
        .query_row("SELECT title FROM papers WHERE id=1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(title, "Old Paper");
    let tag: String = conn
        .query_row("SELECT name FROM tags WHERE id=1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(tag, "旧标签");
    let st: String = conn
        .query_row("SELECT analysis_status FROM papers WHERE id=1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(st, "pendingAnalysis", "旧状态值应重命名");

    // 新列存在
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(papers)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for need in ["chinese_title", "is_favorite", "retry_count", "total_score"] {
        assert!(cols.contains(&need.to_string()), "缺少列 {}", need);
    }

    // user_version 推进且重复 init 幂等
    let v: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, db::SCHEMA_VERSION);
    db::init(&conn).unwrap();
    let v2: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v2, db::SCHEMA_VERSION);
}

/// 真实 macOS Keychain 冒烟（ignored）：save/get/has/delete 真实值。
#[test]
#[ignore]
fn keychain_real_smoke() {
    let msg = crate::secure_store::keychain_smoke().expect("真实 Keychain 验证失败");
    println!("{}", msg);
}

// ================= Round 3.6 hardening 测试 =================

/// Test 1：SyncGuard 正常释放（normal return / drop）。
#[test]
fn test_sync_guard_normal_release() {
    use std::sync::Arc;

    use crate::models::SyncTrigger;
    use crate::sync_coordinator::{SyncCoordinator, SyncGuard};

    let sc = Arc::new(SyncCoordinator::new());
    assert!(sc.try_acquire(SyncTrigger::Manual).is_some());
    {
        let _g = SyncGuard::new(sc.clone());
    } // drop → release
    assert!(!sc.is_running(), "guard drop 后必须释放");
    assert!(sc.try_acquire(SyncTrigger::Manual).is_some(), "释放后可再次 acquire");
    sc.release();
}

/// Test 2：SyncGuard panic 释放（unwind 后 running=false，可再次 acquire）。
#[test]
fn test_sync_guard_panic_release() {
    use std::sync::Arc;

    use crate::models::SyncTrigger;
    use crate::sync_coordinator::{SyncCoordinator, SyncGuard};

    let sc = Arc::new(SyncCoordinator::new());
    assert!(sc.try_acquire(SyncTrigger::Manual).is_some());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _g = SyncGuard::new(sc.clone());
        panic!("sync 任务模拟 panic");
    }));
    assert!(result.is_err(), "panic 应被 catch_unwind 捕获");
    assert!(!sc.is_running(), "panic/unwind 后 running 必须被释放");
    assert!(sc.try_acquire(SyncTrigger::Manual).is_some(), "panic 后可再次 acquire");
    sc.release();
}

/// daily 标记语义：仅 coordinator 接受（started=true）才写 last_daily_sync_date。
#[test]
fn test_daily_mark_only_on_accept() {
    use std::sync::{Arc, Mutex};

    let conn = mem_db();
    let db = Arc::new(Mutex::new(conn));
    // started=false（syncAlreadyRunning / 被拒）→ 不标记
    assert!(!crate::mark_daily_if_started(false, &db, "2026-08-25"));
    {
        let c = db.lock().unwrap();
        assert_eq!(
            db::get_setting(&c, "sync.last_daily_sync_date"),
            None,
            "被拒时不得标记今日已执行"
        );
    }
    // started=true（accepted）→ 标记
    assert!(crate::mark_daily_if_started(true, &db, "2026-08-25"));
    {
        let c = db.lock().unwrap();
        assert_eq!(
            db::get_setting(&c, "sync.last_daily_sync_date").as_deref(),
            Some("2026-08-25")
        );
    }
}

/// daily 冲突：coordinator busy 时 daily 被拒；释放后下一 evaluation 可再次尝试。
#[test]
fn test_daily_rejected_when_busy_then_retryable() {
    use std::sync::Arc;

    use crate::models::SyncTrigger;
    use crate::sync_coordinator::SyncCoordinator;

    let sc = Arc::new(SyncCoordinator::new());
    assert!(sc.try_acquire(SyncTrigger::Manual).is_some());
    // busy → daily 被拒（语义上 started=false → 不标记）
    assert!(sc.try_acquire(SyncTrigger::Daily).is_none());
    sc.release();
    // 释放后下一 evaluation 允许 daily 启动
    assert!(sc.try_acquire(SyncTrigger::Daily).is_some());
    sc.release();
}

/// Keychain 命名空间隔离：test namespace 与 production namespace 必须完全不同。
#[test]
fn test_keychain_test_namespace_isolation() {
    use crate::secure_store::{
        KeychainStore, PRODUCTION_ACCOUNT, PRODUCTION_SERVICE,
    };

    let prod = KeychainStore::production();
    let test = KeychainStore::with_namespace("com.cowpaper.test.isolation", "test-credential");
    assert_ne!(prod.service, test.service, "service 必须不同");
    assert_ne!(prod.account, test.account, "account 必须不同");
    assert_ne!(test.service, PRODUCTION_SERVICE);
    assert_ne!(test.account, PRODUCTION_ACCOUNT);
}

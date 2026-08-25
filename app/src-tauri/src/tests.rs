use rusqlite::{params, Connection};

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
            Err(AiError::Config("bad key".into()))
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
    let _coord = std::thread::spawn(move || ai_queue::coordinator_loop(c2, cmd_rx, h2));

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
            api_key: "k".into(),
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
            api_key: "k".into(),
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
    cmd_tx
        .send(QueueCommand::Resume {
            api_key: "k".into(),
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

    // ===== 场景 D：停止 =====
    reset_pending(&conn);
    ai_queue::set_mock_analyzer(Some(Arc::new(|_id| {
        std::thread::sleep(Duration::from_millis(300));
        Ok(true)
    })));
    cmd_tx
        .send(QueueCommand::Start {
            paper_ids: None,
            api_key: "k".into(),
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

    // ===== 场景 E：单篇失败不影响后续 =====
    reset_pending(&conn);
    let first_id = {
        let c = conn.lock().unwrap();
        let mut stmt = c.prepare("SELECT id FROM papers ORDER BY id LIMIT 1").unwrap();
        stmt.query_row([], |r| r.get::<_, i64>(0)).unwrap()
    };
    ai_queue::set_mock_analyzer(Some(Arc::new(move |id| {
        if id == first_id {
            Err(AiError::Empty)
        } else {
            Ok(true)
        }
    })));
    cmd_tx
        .send(QueueCommand::Start {
            paper_ids: None,
            api_key: "k".into(),
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
            api_key: "k".into(),
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
    use tauri::{Listener, Manager};

    // 关键：确保不命中 mock（本测试必须走真实 DeepSeek）
    ai_queue::set_mock_analyzer(None);

    let key = std::env::var("COWPAPER_KEY").expect("需要 COWPAPER_KEY 环境变量");
    let model = std::env::var("COWPAPER_MODEL").unwrap_or_else(|_| "deepseek-v4-flash".into());
    assert!(!key.is_empty(), "COWPAPER_KEY 不能为空");
    println!("[live] model={}", model);

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
    let _coord = std::thread::spawn(move || ai_queue::coordinator_loop(c2, cmd_rx, h2));

    // ---------- 第一步：1 篇真实分析 ----------
    cmd_tx
        .send(QueueCommand::Start {
            paper_ids: None,
            api_key: key.clone(),
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
            api_key: key,
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
}

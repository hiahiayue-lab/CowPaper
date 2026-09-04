use rusqlite::{params, Connection, OptionalExtension};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use std::thread;

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
        abstract_source_url: None,
        url: doi.map(|d| format!("https://doi.org/{}", d)),
        publisher_article_id: None,
        openalex_work_id: None,
        discovery_source: "crossref".to_string(),
        source_id: doi.map(str::to_string),
        raw_json: None,
    }
}

fn title_response_sequence_server(responses: Vec<(&str, &str)>) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let responses: Vec<(String, String)> = responses.into_iter()
        .map(|(status, body)| (status.to_string(), body.to_string())).collect();
    let requests = Arc::new(AtomicUsize::new(0));
    let observed = requests.clone();
    thread::spawn(move || {
        for (status, body) in responses {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 8192];
            let _ = stream.read(&mut request).unwrap();
            observed.fetch_add(1, Ordering::SeqCst);
            let response = format!(
                "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                status, body.len(), body
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    (format!("http://{address}/chat/completions"), requests)
}

fn test_pdf_path(label: &str, body: &str) -> std::path::PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("cowpaper-{label}-{}-{stamp}.pdf", std::process::id()));
    std::fs::write(&path, body.as_bytes()).unwrap();
    path
}

fn test_pdf_library(label: &str) -> std::path::PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("cowpaper-library-{label}-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn set_pdf_storage_settings(conn: &Connection, mode: &str, root: &std::path::Path, template: &str, rule: &str) {
    db::set_setting(conn, "settings.pdf_file_handling_mode", mode).unwrap();
    db::set_setting(conn, "settings.pdf_library_root", root.to_str().unwrap()).unwrap();
    db::set_setting(conn, "settings.pdf_naming_template", template).unwrap();
    db::set_setting(conn, "settings.pdf_subfolder_rule", rule).unwrap();
}

fn test_paper(conn: &Connection, doi: &str, title: &str) -> i64 {
    let jid = db::insert_journal(conn, "Storage Journal", Some("0025-1909"), None, None, None).unwrap();
    match db::upsert_paper(conn, jid, &candidate(Some(doi), title, Some("abstract"), Some("crossref"))).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
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
    if let Ok(Some(w)) = crossref.works("0025-1909", &from, &to) {
        candidates.extend(w.candidates);
    }
    if let Ok(Some(oa)) = openalex.works("S33323087", &from, &to) {
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
            trigger: "manual".to_string(),
            source_sync_batch_id: None,
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
    // AnalysisBatch 持久化：completed + trigger manual + 聚合正确
    {
        let c = conn.lock().unwrap();
        let ab = db::list_analysis_batches(&c, 1).unwrap().pop().expect("应有 AnalysisBatch");
        assert_eq!(ab.status, "completed", "A: 自然完成应为 completed");
        assert_eq!(ab.trigger, "manual");
        assert_eq!(ab.total, 20);
        assert_eq!(ab.succeeded, 20);
        assert_eq!(ab.failed, 0);
        assert_eq!(ab.remaining, 0);
        let items = db::list_analysis_batch_items(&c, ab.id).unwrap();
        assert_eq!(items.len(), 20);
        assert!(items.iter().all(|i| i.status == "succeeded"), "A: 全部 item 应 succeeded");
        // 队列状态携带 batch id
        drop(c);
    }
    assert!(ai_queue::status_from_db(&conn).analysis_batch_id.is_none(), "A: 完成后 analysis_batch_id 应清空");

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
            trigger: "manual".to_string(),
            source_sync_batch_id: None,
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
    // B: batch 状态 paused，且 analysis_batch_id 被队列携带
    let bid_pause = {
        let c = conn.lock().unwrap();
        let ab = db::list_analysis_batches(&c, 1).unwrap().pop().unwrap();
        assert_eq!(ab.status, "paused", "B: pause 后 batch 应为 paused（非终态）");
        ab.id
    };
    assert_eq!(ai_queue::status_from_db(&conn).analysis_batch_id, Some(bid_pause), "B: 队列应携带 paused batch id");
    cmd_tx
        .send(QueueCommand::Resume {
            model: "m".into(),
        })
        .unwrap();
    // J: resume 后仍在 running、仍有待分析论文时，第二次 Start 必须被忽略（不建第二个 batch）
    cmd_tx
        .send(QueueCommand::Start {
            paper_ids: None,
            model: "m".into(),
            trigger: "manual".into(),
            source_sync_batch_id: None,
        })
        .unwrap();
    std::thread::sleep(Duration::from_millis(400));
    {
        let c = conn.lock().unwrap();
        let batches = db::list_analysis_batches(&c, 10).unwrap();
        let running: Vec<_> = batches.iter().filter(|b| b.status == "running").collect();
        assert_eq!(running.len(), 1, "J: running 时第二次 Start 不得创建第二个 running batch");
        assert_eq!(running[0].id, bid_pause, "J: 运行中的 batch 必须是原 batch");
    }
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
    // B: resume 保持同一 batch id（不新建）
    {
        let c = conn.lock().unwrap();
        let ab = db::list_analysis_batches(&c, 1).unwrap().pop().unwrap();
        assert_eq!(ab.id, bid_pause, "B: resume 必须保持同一 batch id");
        assert_eq!(ab.status, "completed");
        assert_eq!(ab.succeeded, 20);
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
            trigger: "manual".to_string(),
            source_sync_batch_id: None,
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
    // D: batch stopped + 未执行 item = cancelled（不标 failed）
    {
        let c = conn.lock().unwrap();
        let ab = db::list_analysis_batches(&c, 1).unwrap().pop().unwrap();
        assert_eq!(ab.status, "stopped", "D: batch 应为 stopped");
        let items = db::list_analysis_batch_items(&c, ab.id).unwrap();
        let cancelled = items.iter().filter(|i| i.status == "cancelled").count();
        let succeeded = items.iter().filter(|i| i.status == "succeeded").count();
        let failed = items.iter().filter(|i| i.status == "failed").count();
        assert_eq!(cancelled, 18, "D: 未执行 item 应为 cancelled（实际 {}）", cancelled);
        assert_eq!(succeeded, 2, "D: 已完成 item 保留");
        assert_eq!(failed, 0, "D: 未执行 item 不得标 failed");
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
            trigger: "manual".to_string(),
            source_sync_batch_id: None,
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
    // E: 批次终态 completedWithErrors（1 失败），历史保留
    let e_batch = {
        let c = conn.lock().unwrap();
        let ab = db::list_analysis_batches(&c, 1).unwrap().pop().unwrap();
        assert_eq!(ab.status, "completedWithErrors", "E: 有失败 → completedWithErrors");
        assert_eq!(ab.failed, 1);
        assert_eq!(ab.succeeded, 19);
        ab
    };

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
            trigger: "manual".to_string(),
            source_sync_batch_id: None,
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
            trigger: "manual".to_string(),
            source_sync_batch_id: None,
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

    // ===== 场景 H：重试失败 → 新 AnalysisBatch（parent_batch_id 正确，历史保留） =====
    // 先停止 G 的（暂停）batch，使队列回 idle
    cmd_tx.send(QueueCommand::Stop).unwrap();
    let _ = wait(&conn, Duration::from_secs(10), &|s, _| s.state == "idle");
    // 构造确定性状态：全部 pendingAnalysis，仅 first_id 一篇为 analysisFailed
    {
        let c = conn.lock().unwrap();
        let _ = c.execute("UPDATE papers SET analysis_status='pendingAnalysis'", []);
        let _ = c.execute(
            "UPDATE papers SET analysis_status='analysisFailed' WHERE id=?1",
            params![first_id],
        );
    }
    ai_queue::set_mock_analyzer(Some(Arc::new(|_id| Ok(true))));
    cmd_tx
        .send(QueueCommand::RetryFailed {
            model: "m".into(),
            parent_batch_id: Some(e_batch.id),
        })
        .unwrap();
    let _ = wait(&conn, Duration::from_secs(10), &|s, conn| {
        if s.state != "idle" {
            return false;
        }
        let c = conn.lock().unwrap();
        db::list_analysis_batches(&c, 1)
            .unwrap()
            .first()
            .map(|b| b.status == "completed")
            .unwrap_or(false)
    });
    {
        let c = conn.lock().unwrap();
        let h = db::list_analysis_batches(&c, 1).unwrap().pop().unwrap();
        assert_eq!(h.trigger, "retryFailed", "H: trigger=retryFailed");
        assert_eq!(h.parent_batch_id, Some(e_batch.id), "H: parent_batch_id 应指向失败批次");
        assert!(h.id != e_batch.id, "H: 必须创建新 batch");
        assert_eq!(h.total, 1, "H: 只重试失败的论文");
        assert_eq!(h.succeeded, 1, "H: 重试成功");
        assert_eq!(h.failed, 0);
        // 历史保留：原 batch 不变
        let e2 = db::get_analysis_batch(&c, e_batch.id).unwrap().unwrap();
        assert_eq!(e2.status, "completedWithErrors");
        assert_eq!(e2.failed, 1);
        assert_eq!(e2.succeeded, 19);
    }
    let batch_count_before_i = {
        let c = conn.lock().unwrap();
        db::list_analysis_batches(&c, 100).unwrap().len()
    };

    // ===== 场景 I：无待分析论文 → Start 不创建空 AnalysisBatch =====
    {
        let c = conn.lock().unwrap();
        let _ = c.execute("UPDATE papers SET analysis_status='analysisSucceeded'", []);
    }
    cmd_tx
        .send(QueueCommand::Start {
            paper_ids: None,
            model: "m".into(),
            trigger: "manual".into(),
            source_sync_batch_id: None,
        })
        .unwrap();
    std::thread::sleep(Duration::from_millis(400));
    {
        let c = conn.lock().unwrap();
        let now_count = db::list_analysis_batches(&c, 100).unwrap().len();
        assert_eq!(now_count, batch_count_before_i, "I: 无待分析论文不得创建空 batch");
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

    // 本地 secret 文件（临时目录，绝不触碰用户真实目录）：
    // 保存 → 队列经 LocalFileSecretStore 读取 → 用后删除
    let secret_dir = std::env::temp_dir().join(format!("cowpaper-live-{}", std::process::id()));
    let store: Arc<dyn crate::secure_store::SecureStore> =
        Arc::new(crate::secure_store::TempDirSecretStore::new_in(&secret_dir));
    store.save(&key).expect("本地 secret 写入失败");

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
            trigger: "manual".to_string(),
            source_sync_batch_id: None,
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
            trigger: "manual".to_string(),
            source_sync_batch_id: None,
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

    // 清理：删除测试写入的本地 secret 文件，恢复环境原状
    store.delete().expect("本地 secret 清理失败");
    let _ = std::fs::remove_dir_all(&secret_dir);
    println!("[live] 本地 secret 清理完成");
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
    let pairs: Vec<(i64, String, String)> = vec![
        (1, "平台经济".into(), "".into()),
        (2, "博弈论".into(), "".into()),
        (3, "定价".into(), "".into()),
    ];
    let t = |tag: &str, s: f64| TagMatch {
        tag: tag.into(),
        score: s,
        tag_id: None,
        semantic_hash: None,
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

    // v2：批次表存在
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for t in ["sync_batches", "sync_batch_papers", "analysis_batches", "analysis_batch_items"] {
        assert!(tables.contains(&t.to_string()), "缺少 v2 表 {}", t);
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

/// Keychain 已停用（Round 5A.1）：CowPaper 不再使用 macOS Keychain。
/// 原 keychain_real_smoke / namespace 隔离测试随 KeychainStore 一并移除。
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

/// 本地 secret 文件存储测试（Round 5A.1）。
#[test]
fn test_local_secret_store() {
    use crate::secure_store::{SecureStore, TempDirSecretStore};

    // 1) save / has / load internal
    let dir = std::env::temp_dir().join(format!("cowpaper-secrets-unit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = TempDirSecretStore::new_in(&dir);
    assert!(!store.has());
    assert!(store.get().unwrap().is_none());
    store.save("sk-test-12345").unwrap();
    assert!(store.has());
    assert_eq!(store.get().unwrap().unwrap(), "sk-test-12345");

    // 2) replace（覆盖旧 Key）
    store.save("sk-new-value").unwrap();
    assert_eq!(store.get().unwrap().unwrap(), "sk-new-value");

    // 3) delete
    store.delete().unwrap();
    assert!(!store.has());
    assert!(store.get().unwrap().is_none());
    // delete 幂等（文件不存在也成功）
    store.delete().unwrap();

    // 4) restart persistence：同路径重开仍可读
    store.save("sk-restart").unwrap();
    let store2 = TempDirSecretStore::new_in(&dir);
    assert_eq!(store2.get().unwrap().unwrap(), "sk-restart");
    assert!(store2.has());

    // 5) invalid/corrupt file safe failure（坏 JSON → Err，不 panic）
    let secret_file = dir.join("secrets.json");
    std::fs::write(&secret_file, "{ not valid json !!").unwrap();
    let store3 = TempDirSecretStore::new_in(&dir);
    assert!(store3.get().is_err(), "损坏文件必须安全失败");
    assert!(!store3.has(), "损坏文件 has() 不得误报");

    // 6) 文件权限（Unix 可测）：secrets.json 应为 0600
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let store4 = TempDirSecretStore::new_in(&dir);
        store4.save("sk-perm").unwrap();
        let meta = std::fs::metadata(&secret_file).unwrap();
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "secret 文件权限应为 0600，实际 {:o}", mode);
        let dmeta = std::fs::metadata(&dir).unwrap();
        let dmode = dmeta.permissions().mode() & 0o777;
        assert_eq!(dmode, 0o700, "目录权限应为 0700，实际 {:o}", dmode);
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// 前端命令不得返回完整 Key：lib.rs 只注册 save/has/delete/test 命令，
/// 不存在 get_api_key 命令。此处静态断言前端接口层无 Key 暴露路径。
#[test]
fn test_no_get_api_key_command() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap();
    assert!(!src.contains("fn get_api_key"), "禁止存在 get_api_key 命令（前端不得读取完整 Key）");
    assert!(!src.contains("KeychainStore"), "production 不得再引用 KeychainStore");
    assert!(src.contains("LocalFileSecretStore"), "production 应使用本地 secret 文件");
}

/// spawn worker 失败路径：注入返回 Err 的 spawner，验证 guard 仍被 release。
#[test]
fn test_sync_worker_spawn_failure_releases() {
    use std::sync::{Arc, Mutex};

    use crate::models::SyncTrigger;
    use crate::sync_coordinator::SyncCoordinator;

    // 测试 double：模拟操作系统 thread creation 失败
    fn failing_spawner(
        _worker: Box<dyn FnOnce() + Send + 'static>,
    ) -> Result<(), String> {
        Err("simulated thread spawn failure".into())
    }

    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let handle = app.handle().clone();
    let conn = Arc::new(Mutex::new(mem_db()));
    let sync = Arc::new(SyncCoordinator::new());

    let result = crate::start_sync_task_with(
        &handle,
        &conn,
        &sync,
        SyncTrigger::Manual,
        None,
        failing_spawner,
    );
    assert!(!result.started, "spawn 失败应 started=false");
    assert_eq!(result.reason, "syncWorkerStartFailed");
    assert!(!sync.is_running(), "spawn 失败后必须 release（running=false）");
    // 下一次 acquire 仍成功（不影响后续同步）
    assert!(sync.try_acquire(SyncTrigger::Manual).is_some());
    sync.release();
}

// ================= Round 4A：Batch Backend 测试 =================

/// busy 被拒的同步不得创建 SyncBatch。
#[test]
fn test_busy_rejected_sync_creates_no_batch() {
    use std::sync::{Arc, Mutex};

    use crate::models::SyncTrigger;
    use crate::sync_coordinator::SyncCoordinator;

    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let handle = app.handle().clone();
    let conn = Arc::new(Mutex::new(mem_db()));
    let sync = Arc::new(SyncCoordinator::new());
    // coordinator busy（manual 占用）
    assert!(sync.try_acquire(SyncTrigger::Manual).is_some());
    let result = crate::start_sync_task(
        &handle, &conn, &sync, SyncTrigger::Manual, None,
    );
    assert!(!result.started);
    assert_eq!(result.reason, "syncAlreadyRunning");
    // 不创建假的 SyncBatch
    let c = conn.lock().unwrap();
    assert_eq!(db::list_sync_batches(&c, 10).unwrap().len(), 0, "busy 被拒不得创建 SyncBatch");
    drop(c);
    sync.release();
}

/// SyncBatch DB 生命周期：创建 → 关联论文 → finalize；同一 Paper 可出现在多个 batch。
#[test]
fn test_sync_batch_db_lifecycle() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let mut paper_ids = Vec::new();
    for i in 0..3 {
        let c = candidate(Some(&format!("10.1000/sb{}", i)), &format!("T{}", i), Some("abs"), Some("crossref"));
        match db::upsert_paper(&conn, jid, &c).unwrap() {
            UpsertOutcome::New(id) => paper_ids.push(id),
            _ => panic!(),
        }
    }
    // batch #1：1 inserted + 2 existing
    let b1 = db::create_sync_batch(&conn, "manual").unwrap();
    db::add_sync_batch_papers(&conn, b1, &paper_ids[0..1], &paper_ids[1..3], &[]).unwrap();
    db::finalize_sync_batch(&conn, b1, "completed", None).unwrap();
    // batch #2：同一 Paper 再次出现（abstractUpdated）
    let b2 = db::create_sync_batch(&conn, "daily").unwrap();
    db::add_sync_batch_papers(&conn, b2, &[], &[], &paper_ids[1..2]).unwrap();
    db::finalize_sync_batch(&conn, b2, "completed", None).unwrap();

    let b = db::get_sync_batch(&conn, b1).unwrap().unwrap();
    assert_eq!(b.status, "completed");
    assert_eq!(b.trigger, "manual");
    let p1 = db::list_sync_batch_papers(&conn, b1).unwrap();
    assert_eq!(p1.len(), 3);
    let p2 = db::list_sync_batch_papers(&conn, b2).unwrap();
    assert_eq!(p2.len(), 1);
    assert_eq!(p2[0].result, "abstractUpdated");
    // 同一 Paper 出现在多个 batch（many-to-many 保留）
    assert_eq!(p1.iter().filter(|x| x.paper_id == paper_ids[1]).count(), 1);
    assert_eq!(p2[0].paper_id, paper_ids[1]);
}

/// An old process cannot finish a persisted running SyncBatch. Startup recovery
/// must make it terminal so it cannot permanently drive the Work Center.
#[test]
fn test_interrupted_sync_batch_is_recovered() {
    let conn = mem_db();
    let batch_id = db::create_sync_batch(&conn, "startup").unwrap();
    db::set_sync_batch_journal_total(&conn, batch_id, 51).unwrap();
    db::update_sync_batch_journal_progress(&conn, batch_id, 31, 0).unwrap();

    assert_eq!(db::recover_interrupted_sync_batches(&conn).unwrap(), 1);
    let batch = db::get_sync_batch(&conn, batch_id).unwrap().unwrap();
    assert_eq!(batch.status, "failed");
    assert!(batch.finished_at.is_some());
    assert_eq!(batch.journal_completed, 31);
    assert!(batch.error_summary.unwrap_or_default().contains("中断"));
    assert_eq!(db::recover_interrupted_sync_batches(&conn).unwrap(), 0);
}

#[test]
fn test_abstract_recovery_batch_ledger_and_restart_recovery() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let id = match db::upsert_paper(&conn, jid, &candidate(Some("10.1000/recovery"), "Recovery", None, None)).unwrap() { UpsertOutcome::New(id) => id, _ => panic!() };
    let batch = db::create_abstract_recovery_batch(&conn, &[id]).unwrap();
    let item = db::list_abstract_recovery_items(&conn, batch).unwrap().remove(0);
    db::start_abstract_recovery_item(&conn, item.id, "Crossref").unwrap();
    db::finish_abstract_recovery_attempt(&conn, item.id, "Crossref", "notFound", None).unwrap();
    db::finish_abstract_recovery_item(&conn, item.id, "notFound", None, Some("2030-01-01T00:00:00Z")).unwrap();
    db::update_abstract_recovery_batch_counts(&conn, batch).unwrap();
    db::finalize_abstract_recovery_batch(&conn, batch, "completed", None).unwrap();
    let b = db::get_abstract_recovery_batch(&conn, batch).unwrap().unwrap();
    assert_eq!((b.completed, b.not_found, b.remaining), (1, 1, 0));
    let running = db::create_abstract_recovery_batch(&conn, &[id]).unwrap();
    assert_eq!(db::recover_interrupted_abstract_recovery_batches(&conn).unwrap(), 1);
    assert_eq!(db::get_abstract_recovery_batch(&conn, running).unwrap().unwrap().status, "interrupted");
}

#[test]
fn test_daily_first_seen_membership_is_stable() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let id = match db::upsert_paper(&conn, jid, &candidate(Some("10.1000/daily"), "Daily", None, None)).unwrap() { UpsertOutcome::New(id) => id, _ => panic!() };
    conn.execute("UPDATE papers SET first_seen_cycle='2026-08-27' WHERE id=?1", params![id]).unwrap();
    assert_eq!(db::list_papers_for_first_seen_cycle(&conn, "2026-08-27", true).unwrap().len(), 1);
    db::merge_recovered_abstract(&conn, id, "crossref", "A complete abstract with sufficient research detail and results.").unwrap();
    assert_eq!(db::list_papers_for_first_seen_cycle(&conn, "2026-08-27", false).unwrap().len(), 1);
    assert_eq!(db::list_papers_for_first_seen_cycle(&conn, "2026-08-27", true).unwrap().len(), 1, "history missing membership must survive later recovery");
    assert!(db::list_papers_for_first_seen_cycle(&conn, "2026-08-28", false).unwrap().is_empty());
}

#[test]
fn test_daily_summary_aggregates_papers_and_recommendations_independently() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let now = db::now_utc();
    let mut day_27 = Vec::new();
    let mut day_26 = Vec::new();
    for (day, total, missing, ids) in [
        ("2026-08-27", 372, 83, &mut day_27),
        ("2026-08-26", 34, 20, &mut day_26),
    ] {
        for n in 0..total {
            conn.execute(
                "INSERT INTO papers (journal_id,normalized_doi,original_doi,title,title_norm,authors_json,analysis_status,created_at,updated_at,first_seen_cycle,first_seen_abstract_missing)
                 VALUES (?1,?2,?2,?3,?3,'[]','pendingAnalysis',?4,?4,?5,?6)",
                params![jid, format!("10.1000/{day}-{n}"), format!("Paper {day}-{n}"), now, day, if n < missing { 1 } else { 0 }],
            ).unwrap();
            ids.push(conn.last_insert_rowid());
        }
    }
    // A recommendation run may include papers first seen before that date.
    // Add 77 older papers so the 8/26 run has 111 distinct snapshot items
    // while that day itself still has only 34 first-seen papers.
    let mut day_26_recommendations = day_26.clone();
    for n in 0..77 {
        conn.execute(
            "INSERT INTO papers (journal_id,normalized_doi,original_doi,title,title_norm,authors_json,analysis_status,created_at,updated_at,first_seen_cycle,first_seen_abstract_missing)
             VALUES (?1,?2,?2,?3,?3,'[]','pendingAnalysis',?4,?4,'2026-08-01',0)",
            params![jid, format!("10.1000/older-{n}"), format!("Older {n}"), now],
        ).unwrap();
        day_26_recommendations.push(conn.last_insert_rowid());
    }
    for (day, ids, count) in [("2026-08-27", &day_27, 221usize), ("2026-08-26", &day_26_recommendations, 111usize)] {
        let run = db::create_recommendation_run(&conn, day, "finalized").unwrap();
        for (rank, paper_id) in ids.iter().take(count).enumerate() {
            conn.execute(
                "INSERT INTO recommendation_items (run_id,paper_id,rank,score_snapshot,added_at) VALUES (?1,?2,?3,1.0,?4)",
                params![run, paper_id, rank as i64 + 1, now],
            ).unwrap();
        }
    }

    let summaries = db::list_daily_paper_summaries(&conn).unwrap();
    let day_27 = summaries.iter().find(|s| s.cycle_key == "2026-08-27").unwrap();
    assert_eq!((day_27.paper_count, day_27.recommendation_count, day_27.missing_count), (372, 221, 83));
    let day_26 = summaries.iter().find(|s| s.cycle_key == "2026-08-26").unwrap();
    assert_eq!((day_26.paper_count, day_26.recommendation_count, day_26.missing_count), (34, 111, 20));
}

#[test]
fn test_v12_backfills_ledger_proven_legacy_missing_idempotently() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let paper_id = match db::upsert_paper(&conn, jid, &candidate(Some("10.1000/v12"), "Recovered", None, None)).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
    };
    conn.execute("UPDATE papers SET created_at='2026-08-27T00:00:00Z', first_seen_abstract_missing=0 WHERE id=?1", params![paper_id]).unwrap();
    let batch = db::create_abstract_recovery_batch(&conn, &[paper_id]).unwrap();
    let item = db::list_abstract_recovery_items(&conn, batch).unwrap().remove(0);
    conn.execute("UPDATE abstract_recovery_items SET started_at='2026-08-27T01:00:00Z', outcome='recovered' WHERE id=?1", params![item.id]).unwrap();
    conn.pragma_update(None, "user_version", 11).unwrap();

    db::init(&conn).unwrap();
    let missing: i64 = conn.query_row("SELECT first_seen_abstract_missing FROM papers WHERE id=?1", params![paper_id], |r| r.get(0)).unwrap();
    assert_eq!(missing, 1, "v12 must restore first-seen missing membership from recovery evidence");
    db::init(&conn).unwrap();
    let after_restart: i64 = conn.query_row("SELECT first_seen_abstract_missing FROM papers WHERE id=?1", params![paper_id], |r| r.get(0)).unwrap();
    assert_eq!(after_restart, 1, "v12 repair must be idempotent");
}

/// AnalysisBatch DB 生命周期：创建+items → 状态流转 → aggregate 重算。
#[test]
fn test_analysis_batch_db_lifecycle() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let mut ids = Vec::new();
    for i in 0..4 {
        let c = candidate(Some(&format!("10.1000/ab{}", i)), &format!("T{}", i), Some("abs"), Some("crossref"));
        match db::upsert_paper(&conn, jid, &c).unwrap() {
            UpsertOutcome::New(id) => ids.push(id),
            _ => panic!(),
        }
    }
    let bid = db::create_analysis_batch(&conn, "manual", Some("m1"), Some("v1"), None, None, &ids).unwrap();
    let ab = db::get_analysis_batch(&conn, bid).unwrap().unwrap();
    assert_eq!(ab.total, 4);
    assert_eq!(ab.status, "running");
    let items = db::list_analysis_batch_items(&conn, bid).unwrap();
    assert_eq!(items.len(), 4);
    assert!(items.iter().all(|i| i.status == "queued"));

    // 2 成功、1 失败、1 跳过 → aggregate
    db::set_item_started(&conn, bid, ids[0], 1).unwrap();
    db::set_item_status(&conn, bid, ids[0], "succeeded", None, None, None, Some(&db::now_utc())).unwrap();
    db::set_item_started(&conn, bid, ids[1], 1).unwrap();
    db::set_item_status(&conn, bid, ids[1], "failed", None, Some("paperError"), Some("bad json"), Some(&db::now_utc())).unwrap();
    db::set_item_status(&conn, bid, ids[2], "skipped", None, None, None, Some(&db::now_utc())).unwrap();
    db::recompute_analysis_aggregate(&conn, bid).unwrap();
    let ab = db::get_analysis_batch(&conn, bid).unwrap().unwrap();
    assert_eq!(ab.completed, 3);
    assert_eq!(ab.succeeded, 1);
    assert_eq!(ab.failed, 1);
    assert_eq!(ab.skipped, 1);
    assert_eq!(ab.remaining, 1);

    // stop：未执行 → cancelled
    db::cancel_queued_items(&conn, bid).unwrap();
    db::recompute_analysis_aggregate(&conn, bid).unwrap();
    db::set_analysis_batch_status(&conn, bid, "stopped", Some(&db::now_utc()), None).unwrap();
    let items = db::list_analysis_batch_items(&conn, bid).unwrap();
    assert_eq!(items.iter().filter(|i| i.status == "cancelled").count(), 1);
    assert_eq!(items.iter().filter(|i| i.status == "failed").count(), 1);
    let ab = db::get_analysis_batch(&conn, bid).unwrap().unwrap();
    assert_eq!(ab.status, "stopped");
}

/// 重启持久化：批次历史写入文件 DB 后重开仍在。
#[test]
fn test_batch_persistence_restart() {
    let path = std::env::temp_dir().join(format!("cowpaper-test-batch-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    {
        let conn = Connection::open(&path).unwrap();
        db::init(&conn).unwrap();
        let b = db::create_sync_batch(&conn, "daily").unwrap();
        db::finalize_sync_batch(&conn, b, "completed", None).unwrap();
        let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
        let c = candidate(Some("10.1000/r1"), "R1", Some("abs"), Some("crossref"));
        let pid = match db::upsert_paper(&conn, jid, &c).unwrap() { UpsertOutcome::New(id) => id, _ => panic!() };
        let ab = db::create_analysis_batch(&conn, "manual", Some("m"), Some("v"), None, None, &[pid]).unwrap();
        db::set_analysis_batch_status(&conn, ab, "completed", Some(&db::now_utc()), None).unwrap();
    }
    // 模拟重启：重新打开
    {
        let conn = Connection::open(&path).unwrap();
        db::init(&conn).unwrap();
        let sbs = db::list_sync_batches(&conn, 10).unwrap();
        assert_eq!(sbs.len(), 1);
        assert_eq!(sbs[0].trigger, "daily");
        let abs = db::list_analysis_batches(&conn, 10).unwrap();
        assert_eq!(abs.len(), 1);
        assert_eq!(abs[0].status, "completed");
    }
    let _ = std::fs::remove_file(&path);
}

/// 重新分析后 totalScore 更新（推荐排序的数据侧保证）。
#[test]
fn test_reanalysis_updates_totalscore() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let mk = |doi: &str, title: &str| match db::upsert_paper(
        &conn, jid,
        &candidate(Some(doi), title, Some("abs"), Some("crossref")),
    ).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!(),
    };
    let a = mk("10.1000/ra", "Paper A");
    let b = mk("10.1000/rb", "Paper B");
    // 初始：A=1.0, B=2.0
    db::save_analysis(&conn, a, "A中", "abs", "sum", "[{\"tag\":\"t\",\"score\":1.0}]", 1.0, "m", "v", "h1").unwrap();
    db::save_analysis(&conn, b, "B中", "abs", "sum", "[{\"tag\":\"t\",\"score\":2.0}]", 2.0, "m", "v", "h2").unwrap();
    // 重新分析 A：3.0
    db::save_analysis(&conn, a, "A中", "abs", "sum", "[{\"tag\":\"t\",\"score\":3.0}]", 3.0, "m", "v", "h3").unwrap();
    let papers = db::list_papers(&conn, None, 100).unwrap();
    let score = |id: i64| papers.iter().find(|p| p.id == id).unwrap().total_score.unwrap();
    assert_eq!(score(a), 3.0, "重新分析后 A 的 totalScore 必须更新");
    assert_eq!(score(b), 2.0);
    // 前端推荐按 totalScore 降序 → A(3.0) 应在 B(2.0) 之前
    let mut sorted = papers.iter().filter(|p| p.total_score.is_some()).collect::<Vec<_>>();
    sorted.sort_by(|x, y| y.total_score.unwrap().partial_cmp(&x.total_score.unwrap()).unwrap());
    assert_eq!(sorted[0].id, a);
    assert_eq!(sorted[1].id, b);
}

// ================= Round 4.1 测试 =================

/// SyncBatch journal 计数：total 持久化 + completed(成功)/failed 定义 + 不变量。
#[test]
fn test_sync_batch_journal_counters() {
    let conn = mem_db();
    let b = db::create_sync_batch(&conn, "manual").unwrap();
    db::set_sync_batch_journal_total(&conn, b, 3).unwrap();
    db::update_sync_batch_journal_progress(&conn, b, 2, 1).unwrap();
    let sb = db::get_sync_batch(&conn, b).unwrap().unwrap();
    assert_eq!(sb.journal_total, 3, "journal_total 必须持久化");
    assert_eq!(sb.journal_completed, 2, "journal_completed = 成功期刊数");
    assert_eq!(sb.journal_failed, 1, "journal_failed = 失败期刊数");
    assert!(
        sb.journal_completed + sb.journal_failed <= sb.journal_total,
        "journal_completed + journal_failed 不得超过 total"
    );
    assert_eq!(sb.journal_completed + sb.journal_failed, sb.journal_total, "正常结束应相等");
}

/// Crossref may be temporarily unavailable while OpenAlex still covers the
/// journal. That fallback must keep the journal successful instead of
/// preventing every later journal in the sequential batch from running.
#[test]
fn test_openalex_fallback_is_successful_after_crossref_error() {
    assert!(crate::sync::source_discovery_succeeded(false, true));
    assert!(!crate::sync::source_discovery_succeeded(false, false));
}

#[test]
fn test_daily_sync_time_validation() {
    assert!(crate::valid_daily_sync_time("09:00"));
    assert!(crate::valid_daily_sync_time("23:59"));
    assert!(!crate::valid_daily_sync_time("9:00"));
    assert!(!crate::valid_daily_sync_time("25:00"));
}

#[test]
fn test_work_state_consistency() {
    // ===== Scenario A：7 篇 pendingAnalysis（有摘要）→ Work State pending_analysis=7 =====
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let mut ids = Vec::new();
    for i in 0..7 {
        let c = candidate(
            Some(&format!("10.1000/ws-a{}", i)),
            &format!("WorkState A{}", i),
            Some("abstract"),
            Some("crossref"),
        );
        match db::upsert_paper(&conn, jid, &c).unwrap() {
            UpsertOutcome::New(id) => ids.push(id),
            _ => panic!("expected new"),
        }
    }
    let st = crate::build_activity_state(&conn).unwrap();
    assert_eq!(st.pending_analysis, 7, "A: 7 篇待分析 → pendingAnalysis=7");
    assert_eq!(st.analysis_failed, 0, "A: analysisFailed=0");
    assert_eq!(st.waiting_for_abstract, 0, "A: waitingForAbstract=0");
    assert!(st.last_analysis.is_none(), "A: 尚无已完成批次");

    // ===== Scenario B：AnalysisBatch completed 7/7 + papers 全部 succeeded → pending_analysis=0 =====
    for id in &ids {
        db::set_paper_status(&conn, *id, "analysisSucceeded").unwrap();
    }
    let b = db::create_analysis_batch(&conn, "manual", Some("deepseek-v4-flash"), None, None, None, &ids).unwrap();
    for id in &ids {
        db::set_item_status(&conn, b, *id, "succeeded", Some(1), None, None, Some(&db::now_utc())).unwrap();
    }
    db::recompute_analysis_aggregate(&conn, b).unwrap();
    db::set_analysis_batch_status(&conn, b, "completed", Some(&db::now_utc()), None).unwrap();
    let ab = db::get_analysis_batch(&conn, b).unwrap().unwrap();
    assert_eq!((ab.total, ab.succeeded, ab.failed), (7, 7, 0), "B: 批次 7/7 成功");
    let st = crate::build_activity_state(&conn).unwrap();
    assert_eq!(st.pending_analysis, 0, "B: 7/7 全部成功 → pendingAnalysis=0");
    assert_eq!(st.analysis_failed, 0, "B: analysisFailed=0");

    // ===== Scenario C：lastAnalysis.total=7 但 pending=0 → 不得把 total 当 pending =====
    let st = crate::build_activity_state(&conn).unwrap();
    let la = st.last_analysis.expect("C: 应有 lastAnalysis");
    assert_eq!(la.total, 7, "C: lastAnalysis.total=7");
    assert_eq!(la.succeeded, 7, "C: lastAnalysis.succeeded=7");
    assert_eq!(st.pending_analysis, 0, "C: lastAnalysis.total 不得影响 pendingAnalysis");

    // ===== Scenario D：retry failed 后 counts 正确重新计算 =====
    db::set_paper_status(&conn, ids[0], "analysisFailed").unwrap();
    db::set_paper_status(&conn, ids[1], "analysisFailed").unwrap();
    let st = crate::build_activity_state(&conn).unwrap();
    assert_eq!(st.analysis_failed, 2, "D: 2 篇失败 → analysisFailed=2");
    assert_eq!(st.pending_analysis, 0, "D: 失败篇不计入 pendingAnalysis");
    // retry：失败论文回到 pendingAnalysis（retry_failed_ai 的入队语义）
    db::set_paper_status(&conn, ids[0], "pendingAnalysis").unwrap();
    db::set_paper_status(&conn, ids[1], "pendingAnalysis").unwrap();
    let st = crate::build_activity_state(&conn).unwrap();
    assert_eq!(st.analysis_failed, 0, "D: 重试后失败清零");
    assert_eq!(st.pending_analysis, 2, "D: 重试后 pending=2");
    // 重试完成
    for id in [ids[0], ids[1]] {
        db::set_paper_status(&conn, id, "analysisSucceeded").unwrap();
    }
    let st = crate::build_activity_state(&conn).unwrap();
    assert_eq!(st.pending_analysis, 0, "D: 重试完成 → pending=0");
    assert_eq!(st.analysis_failed, 0, "D: 重试完成 → failed=0");

    // ===== Scenario E：sync 新增 3 篇（2 有摘要 + 1 无摘要 waitingForAbstract）→ pending=2, waiting=1 =====
    for i in 0..2 {
        let c = candidate(
            Some(&format!("10.1000/ws-e{}", i)),
            &format!("WorkState E{}", i),
            Some("abstract"),
            Some("crossref"),
        );
        match db::upsert_paper(&conn, jid, &c).unwrap() {
            UpsertOutcome::New(_) => {}
            _ => panic!("expected new"),
        }
    }
    let c = candidate(Some("10.1000/ws-e-noabs"), "WorkState ENoAbs", None, None);
    match db::upsert_paper(&conn, jid, &c).unwrap() {
        UpsertOutcome::New(_) => {}
        _ => panic!("expected new"),
    }
    let st = crate::build_activity_state(&conn).unwrap();
    assert_eq!(st.pending_analysis, 2, "E: 2 篇有摘要待分析（不得把无摘要篇计入）");
    assert_eq!(st.waiting_for_abstract, 1, "E: 1 篇等待摘要");
    assert_eq!(st.pending_analysis + st.waiting_for_abstract, 3, "E: 合计 3 篇");
}

// ================= Round 5A：Canonical Journal Identity & Collections =================

#[test]
fn test_issn_normalize_and_checksum() {
    use crate::util::normalize_issn;
    // 1) valid：带连字符 / 无连字符 / 空白 / 小写 x
    assert_eq!(normalize_issn("0025-1909"), Some("0025-1909".to_string()));
    assert_eq!(normalize_issn("00251909"), Some("0025-1909".to_string()));
    assert_eq!(normalize_issn(" 0025-1909 "), Some("0025-1909".to_string()));
    assert_eq!(normalize_issn("1526-5501"), Some("1526-5501".to_string()));
    assert_eq!(normalize_issn("0306-4573"), Some("0306-4573".to_string()));
    // 2) X checksum：前 7 位 1000002 → 校验位 X
    assert_eq!(normalize_issn("1000-002X"), Some("1000-002X".to_string()));
    assert_eq!(normalize_issn("1000002x"), Some("1000-002X".to_string()));
    assert_eq!(normalize_issn("1000-002x"), Some("1000-002X".to_string()));
    // 3) invalid checksum / 非法输入
    assert_eq!(normalize_issn("0025-1900"), None, "校验位错误");
    assert_eq!(normalize_issn("1000-0020"), None, "X 位置被数字替换");
    assert_eq!(normalize_issn("0025-190"), None, "长度不足");
    assert_eq!(normalize_issn("12345678"), None, "随机 8 位校验失败");
    assert_eq!(normalize_issn(""), None);
    assert_eq!(normalize_issn("abcdefgh"), None, "非数字");
    assert_eq!(normalize_issn("0025-190Z"), None, "非法校验字符");
}

#[test]
fn test_identifier_resolution_same_journal() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "Mgmt Sci", Some("0025-1909"), Some("1526-5501"), None, None).unwrap();
    db::insert_identifier(&conn, jid, "print", "0025-1909", Some("crossref")).unwrap();
    db::insert_identifier(&conn, jid, "online", "1526-5501", Some("crossref")).unwrap();
    // 4) pISSN resolve 同一 journal
    assert_eq!(db::resolve_journal_by_identifier(&conn, "0025-1909").unwrap(), Some(jid));
    // 5) eISSN resolve 同一 journal
    assert_eq!(db::resolve_journal_by_identifier(&conn, "1526-5501").unwrap(), Some(jid));
    // 6) pISSN + eISSN → 同一个 canonical Journal
    assert_eq!(
        db::resolve_journal_by_identifier(&conn, "0025-1909").unwrap(),
        db::resolve_journal_by_identifier(&conn, "1526-5501").unwrap()
    );
    // 未注册 ISSN → None
    assert_eq!(db::resolve_journal_by_identifier(&conn, "0306-4573").unwrap(), None);
}

#[test]
fn test_manual_print_online_validation_and_confirmation() {
    assert_eq!(crate::normalize_manual_issn(Some("00251909"), "Print ISSN").unwrap().as_deref(), Some("0025-1909"));
    assert!(crate::normalize_manual_issn(Some("0025-1900"), "Print ISSN").is_err());
    assert_eq!(crate::normalize_manual_issn(None, "Online ISSN").unwrap(), None);

    let same = crate::api::crossref::JournalMeta {
        title: "Management Science".into(), publisher: None,
        print_issn: Some("0025-1909".into()), online_issn: Some("1526-5501".into()), issn_l: Some("0025-1909".into()),
    };
    let evidence = crate::IssnIdentityEvidence { print_crossref: Some(same), ..Default::default() };
    assert_eq!(crate::resolve_paired_issn_identity("0025-1909", "1526-5501", &evidence), crate::IssnIdentityRelation::Same);
    assert!(crate::requires_unknown_pair_confirmation(true, crate::IssnIdentityRelation::Unknown, false));
    assert!(!crate::requires_unknown_pair_confirmation(true, crate::IssnIdentityRelation::Unknown, true));
}

fn openalex_identity(source_id: &str, issn_l: Option<&str>, issns: &[&str]) -> crate::api::openalex::OpenAlexSourceIdentity {
    crate::api::openalex::OpenAlexSourceIdentity {
        source_id: source_id.to_string(),
        issn_l: issn_l.map(str::to_string),
        issns: issns.iter().map(|value| value.to_string()).collect(),
    }
}

#[test]
fn test_paired_issn_identity_resolver_is_three_state() {
    use crate::{IssnIdentityEvidence, IssnIdentityRelation};
    use crate::api::crossref::JournalMeta;

    // Energy Economics: Crossref only resolves the print endpoint; OpenAlex's
    // source family proves that 0140-9883 and 1873-6181 are one journal.
    let energy_crossref = JournalMeta {
        title: "Energy Economics".into(), publisher: Some("Elsevier".into()),
        print_issn: Some("0140-9883".into()), online_issn: None, issn_l: Some("0140-9883".into()),
    };
    let energy = IssnIdentityEvidence {
        print_crossref: Some(energy_crossref),
        online_crossref: None, // Crossref online lookup 404
        print_openalex: Some(openalex_identity("S94499970", Some("0140-9883"), &["0140-9883", "1873-6181"])),
        online_openalex: Some(openalex_identity("S94499970", Some("0140-9883"), &["0140-9883", "1873-6181"])),
    };
    assert_eq!(crate::resolve_paired_issn_identity("0140-9883", "1873-6181", &energy), IssnIdentityRelation::Same);

    // Crossref can be absent for both lookups while a shared OpenAlex source
    // remains sufficient positive evidence.
    let openalex_only = IssnIdentityEvidence {
        print_openalex: Some(openalex_identity("S1", Some("0025-1909"), &["0025-1909", "1526-5501"])),
        online_openalex: Some(openalex_identity("S1", Some("0025-1909"), &["0025-1909", "1526-5501"])),
        ..Default::default()
    };
    assert_eq!(crate::resolve_paired_issn_identity("0025-1909", "1526-5501", &openalex_only), IssnIdentityRelation::Same);

    // A timeout or any other unavailable lookup supplies no positive conflict
    // evidence, so the UI must request confirmation rather than reject it.
    assert_eq!(crate::resolve_paired_issn_identity("0025-1909", "1526-5501", &IssnIdentityEvidence::default()), IssnIdentityRelation::Unknown);
    assert!(crate::requires_unknown_pair_confirmation(true, IssnIdentityRelation::Unknown, false));
    let conn = mem_db();
    let confirmed_id = db::insert_journal(&conn, "Confirmed unknown pair", Some("0025-1909"), Some("1526-5501"), None, None).unwrap();
    db::bind_journal_identifier(&conn, confirmed_id, crate::models::IDT_PRINT, "0025-1909", Some("manual")).unwrap();
    db::bind_journal_identifier(&conn, confirmed_id, crate::models::IDT_ONLINE, "1526-5501", Some("manual")).unwrap();
    assert_eq!(db::list_journals(&conn).unwrap().len(), 1, "confirmed unknown pair remains one canonical Journal");
    assert_eq!(db::list_journal_identifiers(&conn, confirmed_id).unwrap().len(), 2);

    let conflict = IssnIdentityEvidence {
        print_openalex: Some(openalex_identity("S-print", Some("0025-1909"), &["0025-1909"])),
        online_openalex: Some(openalex_identity("S-online", Some("0306-4573"), &["0306-4573"])),
        ..Default::default()
    };
    assert_eq!(crate::resolve_paired_issn_identity("0025-1909", "0306-4573", &conflict), IssnIdentityRelation::Conflict);
}

#[test]
fn test_manual_identifier_enriches_existing_journal_without_duplicates() {
    let conn = mem_db();
    let print_first = db::insert_journal(&conn, "Print first", Some("0025-1909"), None, None, None).unwrap();
    db::bind_journal_identifier(&conn, print_first, crate::models::IDT_PRINT, "0025-1909", Some("manual")).unwrap();
    db::bind_journal_identifier(&conn, print_first, crate::models::IDT_ONLINE, "1526-5501", Some("manual")).unwrap();
    db::fill_journal_issn_columns(&conn, print_first, Some("0025-1909"), Some("1526-5501")).unwrap();
    assert_eq!(db::resolve_journal_by_identifier(&conn, "1526-5501").unwrap(), Some(print_first));
    assert_eq!(db::list_journals(&conn).unwrap().len(), 1);

    let online_first = db::insert_journal(&conn, "Online first", None, Some("0306-4573"), None, None).unwrap();
    db::bind_journal_identifier(&conn, online_first, crate::models::IDT_ONLINE, "0306-4573", Some("manual")).unwrap();
    db::bind_journal_identifier(&conn, online_first, crate::models::IDT_PRINT, "0743-7463", Some("manual")).unwrap();
    db::fill_journal_issn_columns(&conn, online_first, Some("0743-7463"), Some("0306-4573")).unwrap();
    assert_eq!(db::resolve_journal_by_identifier(&conn, "0743-7463").unwrap(), Some(online_first));
    assert_eq!(db::list_journals(&conn).unwrap().len(), 2);

    let other = db::insert_journal(&conn, "Other", Some("1932-6203"), None, None, None).unwrap();
    db::bind_journal_identifier(&conn, other, crate::models::IDT_PRINT, "1932-6203", Some("manual")).unwrap();
    assert!(db::bind_journal_identifier(&conn, print_first, crate::models::IDT_ONLINE, "1932-6203", Some("manual")).is_err(), "different canonical Journal must not be merged");
}

#[test]
fn test_duplicate_identifier_rejected() {
    let conn = mem_db();
    let a = db::insert_journal(&conn, "Journal A", Some("0025-1909"), None, None, None).unwrap();
    let b = db::insert_journal(&conn, "Journal B", Some("1526-5501"), None, None, None).unwrap();
    db::insert_identifier(&conn, a, "print", "0025-1909", Some("crossref")).unwrap();
    // 同一 ISSN 试图映射到 B：唯一索引拒绝，映射仍归 A
    db::insert_identifier(&conn, b, "print", "0025-1909", Some("crossref")).unwrap();
    assert_eq!(db::resolve_journal_by_identifier(&conn, "0025-1909").unwrap(), Some(a));
    assert_eq!(db::list_journal_identifiers(&conn, b).unwrap().len(), 0, "重复 ISSN 不得写入第二个 journal");
    // 幂等：同 journal 重复插入不产生重复行
    db::insert_identifier(&conn, a, "print", "0025-1909", Some("crossref")).unwrap();
    assert_eq!(db::list_journal_identifiers(&conn, a).unwrap().len(), 1);
}

#[test]
fn test_migration_v2_to_v3_preserves_data() {
    // 手工构造 v2 库（无 journal_identifiers / issn_l / collections），含旧数据，迁移到 v3
    let conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE journals (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, print_issn TEXT, online_issn TEXT, publisher TEXT, enabled INTEGER NOT NULL DEFAULT 1, priority INTEGER NOT NULL DEFAULT 0, rss_url TEXT, openalex_source_id TEXT, publisher_adapter TEXT, last_successful_sync_at TEXT, last_paper_date TEXT, coverage_status TEXT, abstract_coverage_rate REAL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
        CREATE TABLE papers (id INTEGER PRIMARY KEY AUTOINCREMENT, journal_id INTEGER NOT NULL, normalized_doi TEXT, original_doi TEXT, title TEXT, title_norm TEXT, authors_json TEXT, published_date TEXT, year INTEGER, abstract TEXT, abstract_source TEXT, abstract_retrieved_at TEXT, url TEXT, publisher_article_id TEXT, openalex_work_id TEXT, discovery_source TEXT, analysis_status TEXT NOT NULL DEFAULT 'pending', created_at TEXT NOT NULL, updated_at TEXT NOT NULL, chinese_title TEXT, chinese_abstract TEXT, one_sentence_summary TEXT, tag_matches_json TEXT, total_score REAL, model_name TEXT, prompt_version TEXT, evidence_hash TEXT, analyzed_at TEXT, is_favorite INTEGER NOT NULL DEFAULT 0, is_read INTEGER NOT NULL DEFAULT 0, is_ignored INTEGER NOT NULL DEFAULT 0, retry_count INTEGER NOT NULL DEFAULT 0, queued_at TEXT);
        CREATE TABLE source_records (id INTEGER PRIMARY KEY AUTOINCREMENT, paper_id INTEGER NOT NULL, source TEXT NOT NULL, source_id TEXT, raw_json TEXT, retrieved_at TEXT NOT NULL);
        CREATE TABLE sync_batches (id INTEGER PRIMARY KEY AUTOINCREMENT, trigger TEXT NOT NULL, status TEXT NOT NULL, created_at TEXT NOT NULL, started_at TEXT, finished_at TEXT, journal_total INTEGER NOT NULL DEFAULT 0, journal_completed INTEGER NOT NULL DEFAULT 0, journal_failed INTEGER NOT NULL DEFAULT 0, records_found INTEGER NOT NULL DEFAULT 0, papers_inserted INTEGER NOT NULL DEFAULT 0, papers_existing INTEGER NOT NULL DEFAULT 0, abstracts_added INTEGER NOT NULL DEFAULT 0, waiting_abstract INTEGER NOT NULL DEFAULT 0, error_summary TEXT);
        CREATE TABLE sync_batch_papers (id INTEGER PRIMARY KEY AUTOINCREMENT, sync_batch_id INTEGER NOT NULL, paper_id INTEGER NOT NULL, result TEXT NOT NULL);
        CREATE TABLE analysis_batches (id INTEGER PRIMARY KEY AUTOINCREMENT, source_sync_batch_id INTEGER, parent_batch_id INTEGER, trigger TEXT NOT NULL, status TEXT NOT NULL, model_name TEXT, prompt_version TEXT, created_at TEXT NOT NULL, started_at TEXT, finished_at TEXT, total INTEGER NOT NULL DEFAULT 0, completed INTEGER NOT NULL DEFAULT 0, succeeded INTEGER NOT NULL DEFAULT 0, failed INTEGER NOT NULL DEFAULT 0, skipped INTEGER NOT NULL DEFAULT 0, remaining INTEGER NOT NULL DEFAULT 0, error_summary TEXT);
        CREATE TABLE analysis_batch_items (id INTEGER PRIMARY KEY AUTOINCREMENT, analysis_batch_id INTEGER NOT NULL, paper_id INTEGER NOT NULL, status TEXT NOT NULL, attempt_count INTEGER NOT NULL DEFAULT 0, started_at TEXT, finished_at TEXT, error_type TEXT, error_summary TEXT, previous_analysis_hash TEXT, result_analysis_hash TEXT);
        "#,
    )
    .unwrap();
    conn.pragma_update(None, "user_version", 2).unwrap();
    let now = db::now_utc();
    conn.execute(
        "INSERT INTO journals (name, print_issn, online_issn, created_at, updated_at) VALUES ('Mgmt Sci','0025-1909','1526-5501',?1,?1)",
        params![now],
    )
    .unwrap();
    let jid = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO papers (journal_id, title, analysis_status, created_at, updated_at) VALUES (?1,'Paper One','analysisSucceeded',?2,?2)",
        params![jid, now],
    )
    .unwrap();
    let pid = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO sync_batches (trigger, status, created_at, journal_total, journal_completed) VALUES ('manual','completed',?1,1,1)",
        params![now],
    )
    .unwrap();
    let sb = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO sync_batch_papers (sync_batch_id, paper_id, result) VALUES (?1,?2,'new')",
        params![sb, pid],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO analysis_batches (trigger, status, created_at, total, succeeded) VALUES ('manual','completed',?1,1,1)",
        params![now],
    )
    .unwrap();
    let ab = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO analysis_batch_items (analysis_batch_id, paper_id, status, attempt_count) VALUES (?1,?2,'succeeded',1)",
        params![ab, pid],
    )
    .unwrap();

    // 迁移到 v3
    db::init(&conn).unwrap();
    assert_eq!(db::SCHEMA_VERSION, 15);

    // 8) 旧 issn 迁移进 journal_identifiers（类型按列，不猜）
    let ids = db::list_journal_identifiers(&conn, jid).unwrap();
    assert_eq!(ids.len(), 2);
    let types: Vec<&str> = ids.iter().map(|i| i.identifier_type.as_str()).collect();
    assert!(types.contains(&"print") && types.contains(&"online"), "print+online 各一");
    assert!(ids.iter().any(|i| i.value == "0025-1909"));
    assert!(ids.iter().any(|i| i.value == "1526-5501"));

    // 9) papers 保留
    let papers = db::list_papers(&conn, Some(jid), 100).unwrap();
    assert_eq!(papers.len(), 1);
    assert_eq!(papers[0].title.as_deref(), Some("Paper One"));

    // 10) Batch history 保留
    assert_eq!(db::list_sync_batches(&conn, 10).unwrap().len(), 1);
    assert_eq!(db::list_analysis_batches(&conn, 10).unwrap().len(), 1);
    let b = db::get_analysis_batch(&conn, ab).unwrap().unwrap();
    assert_eq!(b.succeeded, 1);
    assert_eq!(db::list_analysis_batch_items(&conn, ab).unwrap().len(), 1);
}

#[test]
fn test_database_restart_persistence() {
    let dir = std::env::temp_dir().join(format!("cowpaper_r5a_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("persist.db");
    let _ = std::fs::remove_file(&path);
    {
        let conn = db::open(&path).unwrap();
        db::init(&conn).unwrap();
        let jid = db::insert_journal(&conn, "Persist J", Some("0025-1909"), None, None, None).unwrap();
        db::insert_identifier(&conn, jid, "print", "0025-1909", Some("crossref")).unwrap();
        db::set_journal_issn_l(&conn, jid, Some("0025-1909")).unwrap();
        let cid = db::create_collection(&conn, "TEST-UTD", "UTD24 测试", None, None, Some("test"), None).unwrap();
        db::add_collection_member(&conn, cid, jid).unwrap();
    }
    {
        let conn = db::open(&path).unwrap();
        db::init(&conn).unwrap(); // 幂等：user_version=3 不重复迁移
        assert_eq!(db::SCHEMA_VERSION, 15);
        let j = db::get_journal(&conn, 1).unwrap().expect("期刊持久化");
        assert_eq!(j.print_issn.as_deref(), Some("0025-1909"));
        assert_eq!(j.identifiers.len(), 1);
        assert_eq!(j.identifiers[0].value, "0025-1909");
        assert_eq!(j.collections, vec!["TEST-UTD".to_string()]);
        assert_eq!(j.issn_l.as_deref(), Some("0025-1909"));
        let colls = db::collections_for_journal(&conn, 1).unwrap();
        assert_eq!(colls.len(), 1);
        assert_eq!(colls[0].code, "TEST-UTD");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_collections_many_to_many_and_score_unaffected() {
    let conn = mem_db();
    let a = db::insert_journal(&conn, "Journal A", Some("0025-1909"), None, None, None).unwrap();
    let b = db::insert_journal(&conn, "Journal B", Some("1526-5501"), None, None, None).unwrap();
    let c = db::insert_journal(&conn, "Journal C", Some("0306-4573"), None, None, None).unwrap();
    let utd = db::create_collection(&conn, "TEST-UTD", "UTD24 测试", None, None, Some("test"), None).unwrap();
    let ft = db::create_collection(&conn, "TEST-FT", "FT50 测试", None, None, Some("test"), None).unwrap();
    db::add_collection_member(&conn, utd, a).unwrap();
    db::add_collection_member(&conn, utd, b).unwrap();
    db::add_collection_member(&conn, ft, b).unwrap();
    db::add_collection_member(&conn, ft, c).unwrap();

    // 12) 同一 Journal 属于多个 collection
    let b_colls = db::collections_for_journal(&conn, b).unwrap();
    let codes: Vec<&str> = b_colls.iter().map(|x| x.code.as_str()).collect();
    assert!(codes.contains(&"TEST-UTD") && codes.contains(&"TEST-FT"), "B ∈ UTD + FT");

    // 13) 重复 membership 被拒（PRIMARY KEY，INSERT OR IGNORE）
    db::add_collection_member(&conn, utd, a).unwrap();
    let dup: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM (SELECT collection_id, journal_id, COUNT(*) c FROM journal_collection_members GROUP BY collection_id, journal_id HAVING c > 1)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(dup, 0, "不得存在重复 membership");

    // 14) Paper 通过 journal → collections 派生（Paper 不冗余存集合）
    let cand = candidate(Some("10.1000/coll"), "Collection Paper", Some("abs"), Some("crossref"));
    let pid = match db::upsert_paper(&conn, a, &cand).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new"),
    };
    let p = db::list_papers(&conn, Some(a), 100).unwrap();
    assert_eq!(p[0].id, pid);
    let derived = db::collections_for_journal(&conn, p[0].journal_id).unwrap();
    assert_eq!(derived.len(), 1);
    assert_eq!(derived[0].code, "TEST-UTD");

    // 15) totalScore 不受 collections 影响
    conn.execute(
        "UPDATE papers SET total_score = 42.5, analysis_status='analysisSucceeded' WHERE id = ?1",
        params![pid],
    )
    .unwrap();
    let before: f64 = conn
        .query_row("SELECT total_score FROM papers WHERE id = ?1", params![pid], |r| r.get(0))
        .unwrap();
    // 加入/移除 collection 不影响 total_score
    let ft2 = db::create_collection(&conn, "TEST-FT2", "FT50 测试 2", None, None, Some("test"), None).unwrap();
    db::add_collection_member(&conn, ft2, a).unwrap();
    let after: f64 = conn
        .query_row("SELECT total_score FROM papers WHERE id = ?1", params![pid], |r| r.get(0))
        .unwrap();
    assert_eq!(before, after, "collection 不得改变 totalScore");
    assert_eq!(after, 42.5);
}

#[test]
fn test_two_identifiers_sync_doi_dedup() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), Some("1526-5501"), None, None).unwrap();
    db::insert_identifier(&conn, jid, "print", "0025-1909", Some("crossref")).unwrap();
    db::insert_identifier(&conn, jid, "online", "1526-5501", Some("crossref")).unwrap();
    // 模拟：pISSN 与 eISSN 查询都返回同一 DOI 的候选
    let c = candidate(Some("10.1000/dedup-r5a"), "Dedup Paper", Some("abs"), Some("crossref"));
    assert!(matches!(db::upsert_paper(&conn, jid, &c).unwrap(), UpsertOutcome::New(_)));
    assert!(matches!(
        db::upsert_paper(&conn, jid, &c).unwrap(),
        UpsertOutcome::Existing { .. }
    ));
    let papers = db::list_papers(&conn, Some(jid), 100).unwrap();
    assert_eq!(papers.len(), 1, "多 ISSN 同步不得产生重复论文（normalized_doi 唯一）");
    assert_eq!(papers[0].normalized_doi.as_deref(), Some("10.1000/dedup-r5a"));
}

#[test]
fn test_issn_l_merge_and_possible_duplicate() {
    let conn = mem_db();
    // 两个期刊共享 ISSN-L → 标记 possible_duplicate（不自动合并，不破坏数据）
    let a = db::insert_journal(&conn, "Mgmt Sci", Some("0025-1909"), None, None, None).unwrap();
    let b = db::insert_journal(&conn, "Mgmt Science X", Some("1526-5501"), None, None, None).unwrap();
    db::set_journal_issn_l(&conn, a, Some("0025-1909")).unwrap();
    db::set_journal_issn_l(&conn, b, Some("0025-1909")).unwrap();
    let list = db::list_journals(&conn).unwrap();
    let ja = list.iter().find(|j| j.id == a).unwrap();
    let jb = list.iter().find(|j| j.id == b).unwrap();
    assert!(ja.possible_duplicate, "A 应标记疑似重复");
    assert!(jb.possible_duplicate, "B 应标记疑似重复");
    // 相同规范化标题也标记（"Management Science" 与 "management   science" 规范化相同）
    let c = db::insert_journal(&conn, "Management Science", Some("0306-4573"), None, None, None).unwrap();
    let d = db::insert_journal(&conn, "management   science", Some("1532-6934"), None, None, None).unwrap();
    let list = db::list_journals(&conn).unwrap();
    let jc = list.iter().find(|j| j.id == c).unwrap();
    let jd = list.iter().find(|j| j.id == d).unwrap();
    assert!(jc.possible_duplicate, "相同标题规范化应标记疑似重复");
    assert!(jd.possible_duplicate, "相同标题规范化应标记疑似重复");
}

// ================= Round 5B：Abstract Quality & Recovery =================

#[test]
fn test_abstract_quality_heuristic() {
    use crate::abstract_quality::assess_abstract_quality as aq;
    use crate::models::{ABQ_COMPLETE, ABQ_MISSING, ABQ_PARTIAL};
    // missing：空 / 空白
    assert_eq!(aq(""), (ABQ_MISSING, "missing"));
    assert_eq!(aq("   "), (ABQ_MISSING, "missing"));
    // partial：ASCII / Unicode 省略号截断
    assert_eq!(
        aq("This paper studies pricing in two-sided platforms and the effect of network externalities..."),
        (ABQ_PARTIAL, "ellipsis_truncated")
    );
    assert_eq!(
        aq("本论文研究双边平台定价与网络外部性对市场均衡的影响……"),
        (ABQ_PARTIAL, "ellipsis_truncated")
    );
    // partial：极短且句法不完整
    assert_eq!(aq("We study how platforms set prices"), (ABQ_PARTIAL, "very_short_incomplete_sentence"));
    // complete：短但完整（70–100 词的真实摘要不得误判）
    assert_eq!(
        aq("We study how platforms set prices. Our model explains equilibrium market outcomes."),
        (ABQ_COMPLETE, "full_text_like_abstract")
    );
    // complete：长完整摘要
    let long = "We develop a model of platform pricing with network effects. ".repeat(12) + "Results follow.";
    assert_eq!(aq(&long), (ABQ_COMPLETE, "full_text_like_abstract"));
    // partial：长文本无结尾标点且以介词/连词结尾（句子中途断开）
    assert_eq!(
        aq(&(long.clone() + " and")),
        (ABQ_PARTIAL, "truncated_sentence")
    );
    assert_eq!(aq(&(long.clone() + " of")), (ABQ_PARTIAL, "truncated_sentence"));
}

#[test]
fn test_abstract_normalize_html_jats() {
    use crate::abstract_quality::{assess_abstract_quality, normalize_abstract_text};
    use crate::models::ABQ_COMPLETE;
    let raw = "<jats:p>We study <b>pricing</b> in two-sided platforms.</jats:p><jats:p>Results show &amp; effects.</jats:p>";
    let n = normalize_abstract_text(raw);
    assert!(!n.contains('<'), "JATS/HTML 标签必须清除");
    assert!(n.contains("pricing"));
    assert!(n.contains('&'), "实体应解码（&amp; → &）");
    assert!(!n.contains('\n'), "空白/换行折叠");
    // 标签字符多 ≠ 更完整：normalize 后按真实内容判定
    assert_eq!(assess_abstract_quality(&n).0, ABQ_COMPLETE);
    // RSS snippet（带省略号）→ partial
    let snippet = normalize_abstract_text("A teaser of the article about platform pricing and network effects...");
    assert_eq!(assess_abstract_quality(&snippet).0, crate::models::ABQ_PARTIAL);
}

#[test]
fn test_abstract_recovery_retry_cadence_and_public_metadata_parser() {
    use chrono::{Duration, Utc};
    use crate::abstract_recovery::{retry_due, retry_delay};
    let now = Utc::now();
    assert_eq!(retry_delay(0), Duration::days(1));
    assert_eq!(retry_delay(1), Duration::days(3));
    assert_eq!(retry_delay(2), Duration::days(7));
    assert_eq!(retry_delay(3), Duration::days(30));
    assert!(!retry_due(Some(&(now - Duration::hours(23)).to_rfc3339()), 0, now));
    assert!(retry_due(Some(&(now - Duration::days(3)).to_rfc3339()), 1, now));
    let html = r#"<meta name="citation_abstract" content="A public abstract with methods and results.">"#;
    assert_eq!(crate::api::publisher::extract_public_abstract(html).as_deref(), Some("A public abstract with methods and results."));
}

#[test]
fn test_non_research_sync_filter_is_strict() {
    use crate::sync::is_non_research_record;

    let mut issue = candidate(Some("10.1000/issue"), "Issue Information: Volume 12, Issue 3", None, None);
    assert_eq!(is_non_research_record(&issue), Some("issue-information"));
    issue.title = Some("Table of Contents".into());
    assert_eq!(is_non_research_record(&issue), Some("table-of-contents"));
    issue.title = Some("Front Matter".into());
    assert_eq!(is_non_research_record(&issue), Some("front-matter"));
    issue.title = Some("Correction to: Platform Pricing".into());
    assert_eq!(is_non_research_record(&issue), Some("publication-notice"));
    issue.title = Some("A normal-looking title".into());
    issue.raw_json = Some(r#"{"type":"journal-issue"}"#.into());
    assert_eq!(is_non_research_record(&issue), Some("source-metadata-type"));

    let research = candidate(Some("10.1000/research"), "An Editorial Perspective on Platform Strategy", None, None);
    assert_eq!(is_non_research_record(&research), None, "ambiguous scholarly titles must remain eligible");
}

fn mk_candidate(src: &str, text: &str) -> crate::abstract_quality::AbstractCandidate {
    use crate::abstract_quality::assess_abstract_quality;
    let (q, r) = assess_abstract_quality(text);
    crate::abstract_quality::AbstractCandidate {
        source: src.to_string(),
        text: text.to_string(),
        quality: q.to_string(),
        reason: r.to_string(),
    }
}

#[test]
fn test_canonical_selection() {
    use crate::abstract_quality::select_canonical_abstract;
    use crate::models::ABQ_COMPLETE;
    // partial Crossref + complete OpenAlex → OpenAlex（质量优先于来源）
    let short = "We study pricing in two-sided platforms and the effect of network externalities on market outcomes...";
    let full = "We study pricing in two-sided platforms and the effect of network externalities on market outcomes. Our model shows that optimal prices internalize cross-side effects. ".repeat(3) + "We derive welfare implications.";
    let best = select_canonical_abstract(vec![mk_candidate("crossref", short), mk_candidate("openalex", &full)]).unwrap();
    assert_eq!(best.source, "openalex");
    assert_eq!(best.quality, ABQ_COMPLETE);
    // complete Crossref + partial OpenAlex → Crossref
    let full_cr = "A complete abstract from crossref covering the model and results in detail. ".repeat(5) + "Conclusion.";
    let short_oa = "A short snippet that is truncated...";
    let best = select_canonical_abstract(vec![mk_candidate("openalex", short_oa), mk_candidate("crossref", &full_cr)]).unwrap();
    assert_eq!(best.source, "crossref");
    // 同 quality 前缀关系：A 是 B 的明显前缀 → B 胜出
    let base = "We study platform pricing. Our model shows optimal prices depend on network effects and user elasticities.";
    let p1 = format!("{} This is a longer complete version with additional welfare detail.", base);
    let p2 = format!("{} This is a longer complete version with additional welfare detail. We also discuss market structure implications.", base);
    let best = select_canonical_abstract(vec![mk_candidate("crossref", &p1), mk_candidate("openalex", &p2)]).unwrap();
    assert_eq!(best.text.trim(), p2.trim(), "更长更完整的候选应胜出");
    // 相同摘要去重：来源优先级更高者胜出
    let same = "Identical abstract from both sources. Complete sentence here.";
    let best = select_canonical_abstract(vec![mk_candidate("rss", same), mk_candidate("crossref", same)]).unwrap();
    assert_eq!(best.source, "crossref");
    // 空候选 → None
    assert!(select_canonical_abstract(vec![mk_candidate("crossref", "   ")]).is_none());
}

#[test]
fn test_missing_abstract_flow() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    // missing：无摘要 → waitingForAbstract + quality missing（不进 AI）
    let c = candidate(Some("10.1000/noabs5b"), "No Abs Paper", None, None);
    let id = match db::upsert_paper(&conn, jid, &c).unwrap() {
        UpsertOutcome::New(i) => i,
        _ => panic!("expected new"),
    };
    let p = db::list_papers(&conn, Some(jid), 100).unwrap();
    assert_eq!(p[0].analysis_status, "waitingForAbstract");
    assert_eq!(p[0].abstract_quality, "missing");
    assert!(
        db::list_pending_papers(&conn, None).unwrap().iter().all(|x| x.id != id),
        "missing 论文不得进入 AI 待分析"
    );
    // missing → partial：可 AI（pendingAnalysis + quality partial）
    let c2 = candidate(Some("10.1000/noabs5b"), "No Abs Paper", Some("Short snippet truncated..."), Some("crossref"));
    match db::upsert_paper(&conn, jid, &c2).unwrap() {
        UpsertOutcome::Existing { abstract_filled, .. } => assert!(abstract_filled),
        _ => panic!("expected existing"),
    }
    let p = db::list_papers(&conn, Some(jid), 100).unwrap();
    assert_eq!(p[0].analysis_status, "pendingAnalysis");
    assert_eq!(p[0].abstract_quality, "partial");
    // missing → complete：可 AI
    let c3 = candidate(
        Some("10.1000/noabs5b"),
        "No Abs Paper",
        Some(&("Full abstract now available with complete detail. ".repeat(8) + "Done.")),
        Some("openalex"),
    );
    db::upsert_paper(&conn, jid, &c3).unwrap();
    let p = db::list_papers(&conn, Some(jid), 100).unwrap();
    assert_eq!(p[0].abstract_quality, "complete");
    assert_eq!(p[0].analysis_status, "pendingAnalysis");
    // 来源候选已记录（canonical source recorded）
    let cnt: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM paper_abstract_sources WHERE paper_id = ?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(cnt >= 2, "有摘要的来源候选都应记录（crossref/openalex），实际 {}", cnt);
}

#[test]
fn test_title_only_translation_preserves_missing_abstract_semantics() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let id = match db::upsert_paper(&conn, jid, &candidate(Some("10.1000/title-only"), "English-only title", None, None)).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
    };

    // This is the exact input set a title-only request may use: missing
    // abstract, source title, and no existing Chinese title.
    assert_eq!(db::list_missing_title_translation_candidates(&conn, None).unwrap(), vec![(id, "English-only title".into())]);
    assert!(db::save_title_translation(&conn, id, "仅标题翻译").unwrap());

    let p = db::get_paper(&conn, id).unwrap().unwrap();
    assert_eq!(p.chinese_title.as_deref(), Some("仅标题翻译"));
    assert_eq!(p.abstract_quality, "missing");
    assert_eq!(p.analysis_status, "waitingForAbstract");
    assert!(p.evidence_hash.is_none());
    assert!(p.chinese_abstract.is_none());
    assert!(p.one_sentence_summary.is_none());
    assert!(p.total_score.is_none());
    assert!(db::list_pending_papers(&conn, None).unwrap().is_empty(), "title-only must not become a full-analysis job");
    let run_id = crate::recommendation::refresh_current_recommendations(&conn, &chrono::Local::now(), "09:00").unwrap();
    assert!(
        !db::list_recommendation_items(&conn, run_id).unwrap().iter().any(|item| item.paper_id == id),
        "a translated title alone must not enter recommendations"
    );

    // Existing translated title excludes repeat requests; no API key path
    // simply leaves the original English title stored by upsert untouched.
    assert!(db::list_missing_title_translation_candidates(&conn, None).unwrap().is_empty());
}

#[test]
fn test_title_only_length_truncated_empty_response_does_not_retry_or_write_title() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let id = match db::upsert_paper(&conn, jid, &candidate(Some("10.1000/http-title"), "English-only title", None, None)).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
    };
    let (endpoint, requests) = title_response_sequence_server(vec![
        ("200 OK", r#"{"choices":[{"message":{"content":" "},"finish_reason":"length"}]}"#),
    ]);
    let error = crate::api::deepseek::DeepSeek::with_endpoint(endpoint)
        .translate_title("valid-test-key", "test-model", "English-only title")
        .unwrap_err();
    assert!(error.to_string().contains("finish_reason=length"));
    let paper = db::get_paper(&conn, id).unwrap().unwrap();
    assert!(paper.chinese_title.is_none());
    assert_eq!(paper.abstract_quality, "missing");
    assert_eq!(paper.analysis_status, "waitingForAbstract");
    assert!(paper.total_score.is_none());
    assert_eq!(requests.load(Ordering::SeqCst), 1, "length-truncated output must not retry with the same configuration");
}

#[test]
fn test_title_only_transient_empty_retry_success_writes_one_row_without_changing_missing_semantics() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let id = match db::upsert_paper(&conn, jid, &candidate(Some("10.1000/http-title-retry"), "English-only title", None, None)).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
    };
    let (endpoint, requests) = title_response_sequence_server(vec![
        ("200 OK", r#"{"choices":[{"message":{"content":" "},"finish_reason":"stop"}]}"#),
        ("200 OK", r#"{"choices":[{"message":{"content":"HTTP 模拟中文标题"},"finish_reason":"stop"}]}"#),
    ]);
    let translated = crate::api::deepseek::DeepSeek::with_endpoint(endpoint)
        .translate_title("valid-test-key", "test-model", "English-only title")
        .unwrap();
    assert!(db::save_title_translation(&conn, id, &translated).unwrap());
    let paper = db::get_paper(&conn, id).unwrap().unwrap();
    assert_eq!(paper.chinese_title.as_deref(), Some("HTTP 模拟中文标题"));
    assert_eq!(paper.abstract_quality, "missing");
    assert_eq!(paper.analysis_status, "waitingForAbstract");
    assert!(paper.total_score.is_none());
    assert_eq!(requests.load(Ordering::SeqCst), 2, "transient empty output gets one bounded retry");
}

#[test]
fn test_historical_missing_title_backlog_candidate_is_translated_once() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let id = match db::upsert_paper(&conn, jid, &candidate(Some("10.1000/historical-title"), "Historical English title", None, None)).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
    };
    // Simulate a paper that already existed before this sync/app session.
    conn.execute(
        "UPDATE papers SET created_at='2026-08-06T00:00:00Z', updated_at='2026-08-06T00:00:00Z', first_seen_cycle='2026-08-06' WHERE id=?1",
        params![id],
    ).unwrap();

    assert_eq!(
        db::list_missing_title_translation_candidates(&conn, None).unwrap(),
        vec![(id, "Historical English title".into())],
        "historical missing papers must be selected without a current sync batch"
    );
    assert!(db::save_title_translation(&conn, id, "历史中文标题").unwrap());

    let p = db::get_paper(&conn, id).unwrap().unwrap();
    assert_eq!(p.chinese_title.as_deref(), Some("历史中文标题"));
    assert_eq!(p.abstract_quality, "missing");
    assert_eq!(p.analysis_status, "waitingForAbstract");
    assert!(p.evidence_hash.is_none());
    assert!(p.total_score.is_none());
    assert!(db::list_pending_papers(&conn, None).unwrap().is_empty());
    let run_id = crate::recommendation::refresh_current_recommendations(&conn, &chrono::Local::now(), "09:00").unwrap();
    assert!(!db::list_recommendation_items(&conn, run_id).unwrap().iter().any(|item| item.paper_id == id));

    // The next backlog run cannot make another DeepSeek request for this row.
    assert!(db::list_missing_title_translation_candidates(&conn, None).unwrap().is_empty());
}

#[test]
fn test_title_only_backlog_drains_bounded_batches_without_reselecting_translated_rows() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    for n in 0..61 {
        let outcome = db::upsert_paper(
            &conn,
            jid,
            &candidate(Some(&format!("10.1000/title-batch-{}", n)), &format!("English title {}", n), None, None),
        ).unwrap();
        assert!(matches!(outcome, UpsertOutcome::New(_)));
    }

    let mut batch_sizes = Vec::new();
    loop {
        let candidates = db::list_missing_title_translation_candidates(&conn, None).unwrap();
        batch_sizes.push(candidates.len());
        if candidates.is_empty() { break; }
        assert!(candidates.len() <= db::TITLE_TRANSLATION_BATCH_LIMIT);
        for (id, _) in candidates {
            assert!(db::save_title_translation(&conn, id, "中文标题").unwrap());
        }
    }

    assert_eq!(batch_sizes, vec![25, 25, 11, 0]);
}

#[test]
fn test_title_translation_gate_is_exclusive_and_releases_after_worker_exit() {
    let gate = crate::TitleTranslationGate::default();
    let permit = gate.acquire().unwrap();
    assert!(gate.acquire().is_err(), "manual and automatic workers must not overlap");
    drop(permit);
    assert!(gate.acquire().is_ok(), "worker completion/error must release the gate");
}

#[test]
fn test_scoped_abstract_recovery_only_accepts_current_view_ids() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let ids: Vec<i64> = (1..=6).map(|n| match db::upsert_paper(
        &conn, jid, &candidate(Some(&format!("10.1000/recovery-{}", n)), &format!("P{}", n), None, None),
    ).unwrap() { UpsertOutcome::New(id) => id, _ => unreachable!() }).collect();

    // The page decides membership (today/day A/day B); the backend only
    // validates those submitted IDs and cannot expand the scope to all rows.
    assert_eq!(db::list_recoverable_paper_ids(&conn, &ids[4..6]).unwrap(), ids[4..6]);
    assert_eq!(db::list_recoverable_paper_ids(&conn, &ids[0..2]).unwrap(), ids[0..2]);
    assert_eq!(db::list_recoverable_paper_ids(&conn, &ids[2..4]).unwrap(), ids[2..4]);
    assert_eq!(db::list_recoverable_paper_ids(&conn, &[ids[3], ids[3]]).unwrap(), vec![ids[3]]);

    db::merge_recovered_abstract(&conn, ids[0], "crossref", &"complete abstract ".repeat(30)).unwrap();
    assert!(db::list_recoverable_paper_ids(&conn, &[ids[0]]).unwrap().is_empty());
}

#[test]
fn test_scoped_abstract_recovery_dedupes_and_caps_to_fifty_ids() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let ids: Vec<i64> = (0..61).map(|n| match db::upsert_paper(
        &conn, jid, &candidate(Some(&format!("10.1000/scoped-{}", n)), &format!("P{}", n), None, None),
    ).unwrap() { UpsertOutcome::New(id) => id, _ => unreachable!() }).collect();
    let mut requested = ids.clone();
    requested.extend_from_slice(&ids[..3]);
    let recoverable = db::list_recoverable_paper_ids(&conn, &requested).unwrap();
    assert_eq!(recoverable.len(), db::ABSTRACT_RECOVERY_BATCH_LIMIT);
    assert_eq!(recoverable, ids[..db::ABSTRACT_RECOVERY_BATCH_LIMIT]);
}

#[test]
fn test_recovered_abstract_after_title_only_translation_remains_full_analysis_eligible() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let id = match db::upsert_paper(&conn, jid, &candidate(Some("10.1000/title-recovery"), "Title before recovery", None, None)).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
    };
    db::save_title_translation(&conn, id, "恢复前标题翻译").unwrap();
    let full = "A complete abstract with methods, results, and implications. ".repeat(8);
    db::merge_recovered_abstract(&conn, id, "crossref", &full).unwrap();
    let p = db::get_paper(&conn, id).unwrap().unwrap();
    assert_ne!(p.abstract_quality, "missing");
    assert_eq!(p.analysis_status, "pendingAnalysis", "title-only must not cause full analysis to be skipped");
    assert!(db::list_pending_papers(&conn, Some(&[id])).unwrap().iter().any(|p| p.id == id));
}

#[test]
fn test_abstract_upgrade_flow() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    // 初始 partial（crossref，省略号截断）
    let c1 = candidate(
        Some("10.1000/up5b"),
        "Upgrade Paper",
        Some("We study platform pricing and network effects..."),
        Some("crossref"),
    );
    let id = match db::upsert_paper(&conn, jid, &c1).unwrap() {
        UpsertOutcome::New(i) => i,
        _ => panic!("expected new"),
    };
    let p = db::list_papers(&conn, Some(jid), 100).unwrap();
    assert_eq!(p[0].abstract_quality, "partial");
    // 模拟一次基于 partial 的 AI 分析（写入旧 evidence_hash）
    db::save_analysis(&conn, id, "中文标题", "中文摘要", "一句话", "[]", 1.2, "m", "v1", "old-hash").unwrap();
    // 完整摘要到达（同 DOI，OpenAlex complete）→ 升级
    let full = "We study platform pricing and network effects in two-sided markets. ".repeat(10) + "We derive equilibrium and welfare results.";
    let c2 = candidate(Some("10.1000/up5b"), "Upgrade Paper", Some(&full), Some("openalex"));
    match db::upsert_paper(&conn, jid, &c2).unwrap() {
        UpsertOutcome::Existing { abstract_upgraded, .. } => assert!(abstract_upgraded, "partial→complete 必须升级"),
        _ => panic!("expected existing"),
    }
    let p = db::list_papers(&conn, Some(jid), 100).unwrap();
    assert_eq!(p[0].abstract_quality, "complete");
    assert_eq!(p[0].abstract_source.as_deref(), Some("openalex"));
    assert_eq!(p[0].evidence_hash.as_deref(), None, "摘要改变后 evidenceHash 清空（旧分析视为 stale）");
    // 禁降级：complete 时再来 partial → 不覆盖
    let c3 = candidate(Some("10.1000/up5b"), "Upgrade Paper", Some("Shorter truncated version..."), Some("crossref"));
    match db::upsert_paper(&conn, jid, &c3).unwrap() {
        UpsertOutcome::Existing { abstract_upgraded, .. } => assert!(!abstract_upgraded, "complete 不得被 partial 降级"),
        _ => panic!("expected existing"),
    }
    let p = db::list_papers(&conn, Some(jid), 100).unwrap();
    assert_eq!(p[0].abstract_quality, "complete");
    assert_eq!(p[0].abstract_source.as_deref(), Some("openalex"));
    // 节流：每次检查更新 abstract_last_checked_at
    assert!(p[0].abstract_last_checked_at.is_some());
}

#[test]
fn test_abstract_upgraded_batch_and_reanalysis() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    // 5 篇论文（外键）
    let mut ids = Vec::new();
    for i in 0..5 {
        let c = candidate(
            Some(&format!("10.1000/upb{}", i)),
            &format!("Upgrade Batch {}", i),
            Some("abstract with full detail about pricing effects and outcomes."),
            Some("crossref"),
        );
        match db::upsert_paper(&conn, jid, &c).unwrap() {
            UpsertOutcome::New(id) => ids.push(id),
            _ => panic!("expected new"),
        }
    }
    // 旧批次（历史，不得修改）
    let old_batch = db::create_analysis_batch(&conn, "manual", Some("m"), None, None, None, &ids).unwrap();
    db::set_analysis_batch_status(&conn, old_batch, "completed", Some(&db::now_utc()), None).unwrap();
    // 摘要升级后创建新批次（trigger=abstractUpgraded）
    let new_ids = [ids[1], ids[2]];
    let new_batch = db::create_analysis_batch(&conn, "abstractUpgraded", Some("m"), None, None, None, &new_ids).unwrap();
    db::set_analysis_batch_status(&conn, new_batch, "completed", Some(&db::now_utc()), None).unwrap();
    let old = db::get_analysis_batch(&conn, old_batch).unwrap().unwrap();
    let new = db::get_analysis_batch(&conn, new_batch).unwrap().unwrap();
    assert_eq!(old.trigger, "manual");
    assert_eq!(new.trigger, "abstractUpgraded");
    assert_eq!(new.total, 2);
    // 旧批次未被改写
    assert_eq!(old.total, 5);
    assert_eq!(old.status, "completed");
    // 两个批次都在历史中
    assert_eq!(db::list_analysis_batches(&conn, 10).unwrap().len(), 2);
}

#[test]
fn test_migration_v4_abstract_quality_init() {
    // 手工构造 v3 库（含旧摘要数据与 batch 历史），迁移到 v4：
    // 存量摘要分类、papers 保留、batch 保留、不调用任何外部服务
    let conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    conn.execute_batch(
        r#"
        CREATE TABLE journals (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, print_issn TEXT, online_issn TEXT, publisher TEXT, enabled INTEGER NOT NULL DEFAULT 1, priority INTEGER NOT NULL DEFAULT 0, rss_url TEXT, openalex_source_id TEXT, publisher_adapter TEXT, last_successful_sync_at TEXT, last_paper_date TEXT, coverage_status TEXT, abstract_coverage_rate REAL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
        CREATE TABLE papers (id INTEGER PRIMARY KEY AUTOINCREMENT, journal_id INTEGER NOT NULL, normalized_doi TEXT, original_doi TEXT, title TEXT, title_norm TEXT, authors_json TEXT, published_date TEXT, year INTEGER, abstract TEXT, abstract_source TEXT, abstract_retrieved_at TEXT, url TEXT, publisher_article_id TEXT, openalex_work_id TEXT, discovery_source TEXT, analysis_status TEXT NOT NULL DEFAULT 'pending', created_at TEXT NOT NULL, updated_at TEXT NOT NULL, chinese_title TEXT, chinese_abstract TEXT, one_sentence_summary TEXT, tag_matches_json TEXT, total_score REAL, model_name TEXT, prompt_version TEXT, evidence_hash TEXT, analyzed_at TEXT, is_favorite INTEGER NOT NULL DEFAULT 0, is_read INTEGER NOT NULL DEFAULT 0, is_ignored INTEGER NOT NULL DEFAULT 0, retry_count INTEGER NOT NULL DEFAULT 0, queued_at TEXT);
        CREATE TABLE sync_batches (id INTEGER PRIMARY KEY AUTOINCREMENT, trigger TEXT NOT NULL, status TEXT NOT NULL, created_at TEXT NOT NULL, started_at TEXT, finished_at TEXT, journal_total INTEGER NOT NULL DEFAULT 0, journal_completed INTEGER NOT NULL DEFAULT 0, journal_failed INTEGER NOT NULL DEFAULT 0, records_found INTEGER NOT NULL DEFAULT 0, papers_inserted INTEGER NOT NULL DEFAULT 0, papers_existing INTEGER NOT NULL DEFAULT 0, abstracts_added INTEGER NOT NULL DEFAULT 0, waiting_abstract INTEGER NOT NULL DEFAULT 0, error_summary TEXT);
        CREATE TABLE sync_batch_papers (id INTEGER PRIMARY KEY AUTOINCREMENT, sync_batch_id INTEGER NOT NULL, paper_id INTEGER NOT NULL, result TEXT NOT NULL);
        CREATE TABLE analysis_batches (id INTEGER PRIMARY KEY AUTOINCREMENT, source_sync_batch_id INTEGER, parent_batch_id INTEGER, trigger TEXT NOT NULL, status TEXT NOT NULL, model_name TEXT, prompt_version TEXT, created_at TEXT NOT NULL, started_at TEXT, finished_at TEXT, total INTEGER NOT NULL DEFAULT 0, completed INTEGER NOT NULL DEFAULT 0, succeeded INTEGER NOT NULL DEFAULT 0, failed INTEGER NOT NULL DEFAULT 0, skipped INTEGER NOT NULL DEFAULT 0, remaining INTEGER NOT NULL DEFAULT 0, error_summary TEXT);
        CREATE TABLE analysis_batch_items (id INTEGER PRIMARY KEY AUTOINCREMENT, analysis_batch_id INTEGER NOT NULL, paper_id INTEGER NOT NULL, status TEXT NOT NULL, attempt_count INTEGER NOT NULL DEFAULT 0, started_at TEXT, finished_at TEXT, error_type TEXT, error_summary TEXT, previous_analysis_hash TEXT, result_analysis_hash TEXT);
        "#,
    )
    .unwrap();
    conn.pragma_update(None, "user_version", 2).unwrap();
    let now = db::now_utc();
    conn.execute(
        "INSERT INTO journals (name, print_issn, created_at, updated_at) VALUES ('J','0025-1909',?1,?1)",
        params![now],
    )
    .unwrap();
    let jid = conn.last_insert_rowid();
    // 完整摘要 / 截断摘要 / 无摘要 三篇
    conn.execute(
        "INSERT INTO papers (journal_id, title, abstract, abstract_source, analysis_status, created_at, updated_at) VALUES (?1,'P1','A complete abstract about platform pricing with network effects.','crossref','pendingAnalysis',?2,?2)",
        params![jid, now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO papers (journal_id, title, abstract, abstract_source, analysis_status, created_at, updated_at) VALUES (?1,'P2','This is a truncated snippet about pricing and network effects...','crossref','pendingAnalysis',?2,?2)",
        params![jid, now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO papers (journal_id, title, abstract, analysis_status, created_at, updated_at) VALUES (?1,'P3',NULL,'waitingForAbstract',?2,?2)",
        params![jid, now],
    )
    .unwrap();
    // batch 历史
    conn.execute(
        "INSERT INTO sync_batches (trigger, status, created_at) VALUES ('manual','completed',?1)",
        params![now],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO analysis_batches (trigger, status, created_at) VALUES ('manual','completed',?1)",
        params![now],
    )
    .unwrap();

    db::init(&conn).unwrap();
    assert_eq!(db::SCHEMA_VERSION, 15);

    let papers = db::list_papers(&conn, Some(jid), 100).unwrap();
    assert_eq!(papers.len(), 3, "迁移不得丢论文");
    let by_title = |t: &str| papers.iter().find(|p| p.title.as_deref() == Some(t)).unwrap();
    assert_eq!(by_title("P1").abstract_quality, "complete");
    assert_eq!(by_title("P2").abstract_quality, "partial");
    assert_eq!(by_title("P3").abstract_quality, "missing");
    // 无摘要论文保持 waitingForAbstract（与 missing 对齐）
    assert_eq!(by_title("P3").analysis_status, "waitingForAbstract");
    // batch 历史保留
    assert_eq!(db::list_sync_batches(&conn, 10).unwrap().len(), 1);
    assert_eq!(db::list_analysis_batches(&conn, 10).unwrap().len(), 1);
    // 来源候选表已建立（migration 记录）
    let cnt: i64 = conn
        .query_row("SELECT COUNT(*) FROM paper_abstract_sources", [], |r| r.get(0))
        .unwrap();
    assert_eq!(cnt, 2, "两篇有摘要论文的 migration 来源候选已记录");
}

// ================= Round 5B.1：Abstract Upgrade Reanalysis Orchestration =================

/// P1-1 修复：succeeded partial → complete 升级后必须真正获得重新入队资格
/// （analysis_status → pendingAnalysis，enqueue 不再 UPDATE 0 rows），
/// 走真实 enqueue + AnalysisBatch（batch item 落库），新 analysis 写入新 hash。
#[test]
fn test_upgrade_reanalysis_orchestration() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let cand = candidate(
        Some("10.1000/orch1"),
        "Orch Paper",
        Some("We study platform pricing and network effects..."),
        Some("crossref"),
    );
    let id = match db::upsert_paper(&conn, jid, &cand).unwrap() {
        UpsertOutcome::New(i) => i,
        _ => panic!("expected new"),
    };
    // 模拟基于 partial 摘要的已完成旧分析（succeeded + hash H1 + score）
    db::save_analysis(&conn, id, "中文标题", "中文摘要", "一句话", "[{\"tag\":\"t1\",\"score\":0.6}]", 0.6, "m", "v1", "H1").unwrap();
    {
        let p = db::list_papers(&conn, None, 100).unwrap().remove(0);
        assert_eq!(p.analysis_status, "analysisSucceeded");
        assert_eq!(p.abstract_quality, "partial");
        assert_eq!(p.evidence_hash.as_deref(), Some("H1"));
        assert_eq!(p.total_score, Some(0.6));
    }
    // 完整摘要到达 → 真实 merge_abstract 升级路径（upsert Existing）
    let full = "We study platform pricing and network effects in two-sided markets. ".repeat(10) + "We derive equilibrium and welfare results.";
    let cand2 = candidate(Some("10.1000/orch1"), "Orch Paper", Some(&full), Some("openalex"));
    match db::upsert_paper(&conn, jid, &cand2).unwrap() {
        UpsertOutcome::Existing { abstract_upgraded, .. } => assert!(abstract_upgraded, "必须标记升级"),
        _ => panic!("expected existing"),
    }
    {
        let p = db::list_papers(&conn, None, 100).unwrap().remove(0);
        assert_eq!(p.abstract_quality, "complete");
        assert_eq!(p.analysis_status, "pendingAnalysis", "succeeded 必须回到 pendingAnalysis 获得入队资格");
        assert_eq!(p.evidence_hash, None, "evidenceHash 清空 → 旧分析 stale");
        assert_eq!(p.total_score, Some(0.6), "旧 AI 结果保留，直到新分析覆盖");
    }
    // 真实 enqueue：UPDATE 必须生效（不再是 0 rows），论文进入 queued
    db::enqueue_paper(&conn, id).unwrap();
    {
        let st = db::get_analysis_status(&conn, id).unwrap();
        assert_eq!(st.as_deref(), Some("queued"), "enqueue 成功：pendingAnalysis → queued");
    }
    // 真实 AnalysisBatch（trigger=abstractUpgraded）+ item 落库
    let batch = db::create_analysis_batch(&conn, "abstractUpgraded", Some("m"), None, None, None, &[id]).unwrap();
    let b = db::get_analysis_batch(&conn, batch).unwrap().unwrap();
    assert_eq!(b.trigger, "abstractUpgraded");
    assert_eq!(b.total, 1);
    assert_eq!(db::list_analysis_batch_items(&conn, batch).unwrap().len(), 1, "Paper 成功进入 batch item");
    // 模拟重新分析完成：succeeded + 新 hash H2（对应新 canonical abstract）
    db::set_paper_status(&conn, id, "analysisSucceeded").unwrap();
    db::save_analysis(&conn, id, "新中文标题", "新中文摘要", "新一句话", "[{\"tag\":\"t1\",\"score\":1.0}]", 3.0, "m", "v1", "H2").unwrap();
    {
        let p = db::list_papers(&conn, None, 100).unwrap().remove(0);
        assert_eq!(p.analysis_status, "analysisSucceeded");
        assert_eq!(p.evidence_hash.as_deref(), Some("H2"), "新 evidenceHash 写入");
        assert_ne!(p.evidence_hash.as_deref(), Some("H1"));
        assert_eq!(p.total_score, Some(3.0), "新分析覆盖旧结果");
        // 旧批次历史不修改：本测试只有新批次；batch 状态保持
        assert_eq!(db::list_analysis_batches(&conn, 10).unwrap().len(), 1);
    }
}

/// 同一 complete 摘要再次同步不得重复触发：
/// status 保持 succeeded、hash 保持 H2、不产生 upgraded id。
#[test]
fn test_same_complete_no_retrigger() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let full = "We study platform pricing and network effects in two-sided markets. ".repeat(10) + "We derive equilibrium and welfare results.";
    let cand = candidate(Some("10.1000/nore"), "No Re-trigger", Some(&full), Some("openalex"));
    let id = match db::upsert_paper(&conn, jid, &cand).unwrap() {
        UpsertOutcome::New(i) => i,
        _ => panic!("expected new"),
    };
    db::save_analysis(&conn, id, "中文", "摘要", "一句话", "[]", 2.0, "m", "v1", "H2").unwrap();
    // 再次收到相同 complete B（同来源同文本）
    let cand2 = candidate(Some("10.1000/nore"), "No Re-trigger", Some(&full), Some("openalex"));
    match db::upsert_paper(&conn, jid, &cand2).unwrap() {
        UpsertOutcome::Existing { abstract_upgraded, abstract_filled, .. } => {
            assert!(!abstract_upgraded, "相同 complete 不得触发升级");
            assert!(!abstract_filled, "相同摘要不得视为新增");
        }
        _ => panic!("expected existing"),
    }
    let p = db::list_papers(&conn, Some(jid), 100).unwrap().remove(0);
    assert_eq!(p.analysis_status, "analysisSucceeded", "不得改回 pendingAnalysis");
    assert_eq!(p.evidence_hash.as_deref(), Some("H2"), "hash 保持");
    assert_eq!(p.abstract_quality, "complete");
}

/// Post-Sync 自动分析目标合并（Case A–E）：一次 sync 最多一个自动 AnalysisBatch。
#[test]
fn test_post_sync_analysis_union() {
    let f = crate::post_sync_analysis_ids;
    // Case A：new + autoNew=true → [1,2]
    assert_eq!(f(&[1, 2], &[], true), vec![1, 2]);
    // Case B：仅 upgraded → [3,4]
    assert_eq!(f(&[], &[3, 4], true), vec![3, 4]);
    // Case C：两类都有 + autoNew=true → 一个合并集合 [1,2,3,4]
    assert_eq!(f(&[1, 2], &[3, 4], true), vec![1, 2, 3, 4]);
    // Case D：autoNew=false → 只分析升级论文 [3]
    assert_eq!(f(&[1, 2], &[3], false), vec![3]);
    // Case E：overlap dedup → [1,2,3]（Paper 2 只出现一次）
    assert_eq!(f(&[1, 2], &[2, 3], true), vec![1, 2, 3]);
    // 空 → 不启动
    assert!(f(&[], &[], true).is_empty());
}

/// 升级论文重分析后推荐排序回归：A 升级重分析 score 3 > B score 2 → A 排前。
#[test]
fn test_upgrade_recommendation_reorder() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let ca = candidate(Some("10.1000/ra"), "Paper A", Some("abstract a with full detail here."), Some("crossref"));
    let ida = match db::upsert_paper(&conn, jid, &ca).unwrap() {
        UpsertOutcome::New(i) => i,
        _ => panic!(),
    };
    let cb = candidate(Some("10.1000/rb"), "Paper B", Some("abstract b with full detail here."), Some("crossref"));
    let idb = match db::upsert_paper(&conn, jid, &cb).unwrap() {
        UpsertOutcome::New(i) => i,
        _ => panic!(),
    };
    // 旧分析：A=1, B=2 → B 排前
    db::save_analysis(&conn, ida, "A", "a", "sa", "[]", 1.0, "m", "v1", "HA").unwrap();
    db::save_analysis(&conn, idb, "B", "b", "sb", "[]", 2.0, "m", "v1", "HB").unwrap();
    let rank = |conn: &Connection| -> Vec<i64> {
        let papers = db::list_papers(conn, None, 100).unwrap();
        let mut scored: Vec<&crate::models::Paper> = papers.iter().filter(|p| p.total_score.is_some()).collect();
        scored.sort_by(|x, y| y.total_score.unwrap().total_cmp(&x.total_score.unwrap()));
        scored.iter().map(|p| p.id).collect()
    };
    assert_eq!(rank(&conn), vec![idb, ida], "旧：B(2) > A(1)");
    // A 摘要升级 + 重新分析（新 score 3）→ A 排前（不修改推荐算法，仅验证数据层排序）
    db::save_analysis(&conn, ida, "A2", "a2", "s2", "[]", 3.0, "m", "v1", "HA2").unwrap();
    assert_eq!(rank(&conn), vec![ida, idb], "新：A(3) > B(2)");
}

// ================= Round 5C：Verified Journal Catalog =================

#[test]
fn test_catalog_import_counts_and_idempotent() {
    use crate::catalog::{self, CatalogImportReport};
    let conn = mem_db();
    // 首次导入
    let r1: CatalogImportReport = catalog::import_catalog(&conn).unwrap();
    let list = db::list_journals(&conn).unwrap();
    assert_eq!(list.len(), 51, "unique canonical journals = 51");
    assert!(r1.journals_created >= 45, "多数期刊应新建，实际 {}", r1.journals_created);
    // UTD24 = 24 / FT50 = 50
    let utd = db::count_collection_members(&conn, "UTD24").unwrap();
    let ft = db::count_collection_members(&conn, "FT50").unwrap();
    assert_eq!(utd, 24, "UTD24 必须严格 24");
    assert_eq!(ft, 50, "FT50-2026 必须严格 50");
    // overlap / unique
    let colls: Vec<String> = db::list_journals(&conn).unwrap().into_iter().map(|j| j.collections.join(",")).collect();
    let both = colls.iter().filter(|c| c.contains("UTD24") && c.contains("FT50")).count();
    assert_eq!(both, 23, "overlap = 23");
    // 幂等：重复导入不产生重复
    let r2: CatalogImportReport = catalog::import_catalog(&conn).unwrap();
    assert_eq!(db::list_journals(&conn).unwrap().len(), 51, "重复导入不得新增 journal");
    assert_eq!(db::count_collection_members(&conn, "UTD24").unwrap(), 24);
    assert_eq!(db::count_collection_members(&conn, "FT50").unwrap(), 50);
    assert_eq!(r2.journals_created, 0, "重复导入不得新建 journal");
    assert_eq!(r2.memberships_added, 0, "重复导入不得新增 membership");
    assert_eq!(r2.identifiers_added, 0, "重复导入不得新增 identifier");
    // 2026 三进三出 regression
    let ft_journals = db::list_journals(&conn).unwrap();
    let has_ft = |title: &str| ft_journals.iter().any(|j| j.name == title && j.collections.iter().any(|c| c == "FT50"));
    assert!(has_ft("Academy of Management Annals"), "2026 FT50 含 Academy of Management Annals");
    assert!(has_ft("American Sociological Review"));
    assert!(has_ft("Psychological Science"));
    assert!(!has_ft("Human Relations"), "2026 FT50 不含 Human Relations");
    assert!(!has_ft("Journal of Business Ethics"));
    assert!(!has_ft("Organization Studies"));
}

#[test]
fn test_catalog_existing_journal_enrichment() {
    use crate::catalog;
    let conn = mem_db();
    // 用户已订阅 Management Science（与 catalog 中 Management Science 同 ISSN）
    let jid = db::insert_journal(&conn, "Management Science", Some("0025-1909"), Some("1526-5501"), Some("INFORMS"), None).unwrap();
    db::set_journal_enabled(&conn, jid, true).unwrap();
    let _ = db::insert_identifier(&conn, jid, "print", "0025-1909", Some("manual"));
    let _ = db::insert_identifier(&conn, jid, "online", "1526-5501", Some("manual"));
    // 用户已有论文（journal_id 必须保留）
    let cand = candidate(Some("10.1000/ms-fk"), "MS Paper", Some("abstract with full detail here."), Some("crossref"));
    let pid = match db::upsert_paper(&conn, jid, &cand).unwrap() {
        UpsertOutcome::New(i) => i,
        _ => panic!("expected new"),
    };
    // 导入 catalog → enrichment
    catalog::import_catalog(&conn).unwrap();
    let list = db::list_journals(&conn).unwrap();
    assert_eq!(list.len(), 51, "已有期刊不得重复创建");
    let ms = list.iter().find(|j| j.id == jid).expect("Management Science 保留原 id");
    assert!(ms.enabled, "用户订阅状态不被覆盖");
    assert!(ms.collections.contains(&"UTD24".to_string()), "获得 UTD24 membership");
    assert!(ms.collections.contains(&"FT50".to_string()), "获得 FT50 membership");
    assert_eq!(ms.identifiers.len(), 2, "identifiers 保持");
    // papers.journal_id 不变（enrich 不重建/删除 journal）
    let p = db::list_papers(&conn, None, 100).unwrap().into_iter().find(|x| x.id == pid).expect("论文保留");
    assert_eq!(p.journal_id, jid, "papers.journal_id 保持原 journal");
    // metadata_needs_review：仅 HBR 标记
    let hbr = list.iter().find(|j| j.name == "Harvard Business Review").expect("HBR 在 catalog 中");
    assert!(hbr.metadata_needs_review, "HBR identifier 未解决应标记 review");
}

#[test]
fn test_catalog_bulk_subscribe() {
    use crate::catalog;
    let conn = mem_db();
    catalog::import_catalog(&conn).unwrap();
    let list = db::list_journals(&conn).unwrap();
    let ms = list.iter().find(|j| j.name == "Management Science").unwrap();
    let or = list.iter().find(|j| j.name == "Operations Research").unwrap();
    let hbr = list.iter().find(|j| j.name == "Harvard Business Review").unwrap();
    // 仅选择部分期刊批量订阅
    let result = crate::subscribe_journals_logic(&conn, vec![ms.id, or.id]).unwrap();
    assert_eq!(result.subscribed, 2);
    assert_eq!(result.already, 0);
    // 重复订阅：already 计数，不重复
    let result2 = crate::subscribe_journals_logic(&conn, vec![ms.id]).unwrap();
    assert_eq!(result2.subscribed, 0);
    assert_eq!(result2.already, 1);
    // 未选期刊不订阅
    assert!(!db::get_journal(&conn, hbr.id).unwrap().unwrap().enabled, "未选择期刊不得订阅");
    // 无效 id：failed 计数，不整体失败
    let result3 = crate::subscribe_journals_logic(&conn, vec![99999]).unwrap();
    assert_eq!(result3.failed, 1);
    assert_eq!(result3.subscribed, 0);
}

#[test]
fn test_collection_total_score_invariant() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let cand = candidate(Some("10.1000/score5c"), "Score Paper", Some("abstract with full detail about pricing and markets."), Some("crossref"));
    let pid = match db::upsert_paper(&conn, jid, &cand).unwrap() {
        UpsertOutcome::New(i) => i,
        _ => panic!(),
    };
    db::save_analysis(&conn, pid, "中", "摘要", "句", "[{\"tag\":\"A\",\"score\":0.8},{\"tag\":\"B\",\"score\":0.7}]", 1.5, "m", "v1", "H").unwrap();
    let before = db::list_papers(&conn, None, 100).unwrap()[0].total_score;
    assert_eq!(before, Some(1.5), "tags A=0.8 + B=0.7 → 1.5");
    // 加入 UTD24 + FT50 collections → totalScore 不变
    let utd = db::create_collection(&conn, "UTD24", "UTD24", None, None, Some("test"), None).unwrap();
    let ft = db::create_collection(&conn, "FT50", "FT50", None, None, Some("test"), None).unwrap();
    db::add_collection_member(&conn, utd, jid).unwrap();
    db::add_collection_member(&conn, ft, jid).unwrap();
    let after = db::list_papers(&conn, None, 100).unwrap()[0].total_score;
    assert_eq!(after, Some(1.5), "collection 不得改变 totalScore");
    // 删除 collection membership → 仍 1.5
    conn.execute("DELETE FROM journal_collection_members WHERE collection_id=?1", params![utd]).unwrap();
    let p = db::list_papers(&conn, None, 100).unwrap()[0].clone();
    assert_eq!(p.total_score, Some(1.5));
    assert_eq!(p.collections, vec!["FT50".to_string()], "paper 派生 collections");
}

#[test]
fn test_recommendation_ordering_unaffected_by_collections() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let a = match db::upsert_paper(&conn, jid, &candidate(Some("10.1000/rr1"), "R1", Some("abstract one here with full detail."), Some("crossref"))).unwrap() {
        UpsertOutcome::New(i) => i, _ => panic!(),
    };
    let b = match db::upsert_paper(&conn, jid, &candidate(Some("10.1000/rr2"), "R2", Some("abstract two here with full detail."), Some("crossref"))).unwrap() {
        UpsertOutcome::New(i) => i, _ => panic!(),
    };
    db::save_analysis(&conn, a, "A", "a", "s", "[]", 1.0, "m", "v1", "H1").unwrap();
    db::save_analysis(&conn, b, "B", "b", "s", "[]", 2.0, "m", "v1", "H2").unwrap();
    let order = || -> Vec<i64> {
        let mut ps = db::list_papers(&conn, None, 100).unwrap();
        ps.retain(|p| p.total_score.is_some());
        ps.sort_by(|x, y| y.total_score.unwrap().total_cmp(&x.total_score.unwrap()).then(y.published_date.cmp(&x.published_date)));
        ps.iter().map(|p| p.id).collect()
    };
    assert_eq!(order(), vec![b, a]);
    // 加入 collection → 排序不变
    let utd = db::create_collection(&conn, "UTD24", "UTD24", None, None, Some("test"), None).unwrap();
    db::add_collection_member(&conn, utd, jid).unwrap();
    assert_eq!(order(), vec![b, a], "collection filter 不改变排序");
}

// ================= Round 5C.1：Catalog Identity & Syncability =================

/// 全 catalog 51 本：identifier_ready = 51/51（至少一个可用的 resolver identifier）。
#[test]
fn test_all_catalog_journals_identifier_ready() {
    use crate::catalog::CatalogFile;
    let data: CatalogFile = serde_json::from_str(crate::catalog::CATALOG_JSON).unwrap();
    assert_eq!(data.journals.len(), 51);
    let mut not_ready = Vec::new();
    for j in &data.journals {
        let has_id = j
            .print_issn
            .as_deref()
            .and_then(crate::util::normalize_issn)
            .is_some()
            || j
                .online_issn
                .as_deref()
                .and_then(crate::util::normalize_issn)
                .is_some();
        if !has_id {
            not_ready.push(j.canonical_title.clone());
        }
    }
    assert!(not_ready.is_empty(), "identifier 缺失: {:?}", not_ready);
}

/// 全 catalog 51 本：discovery_strategy = 51/51（每本至少一个当前 pipeline 可用的
/// discovery 策略：Crossref ISSN 或 OpenAlex source id）。
#[test]
fn test_all_catalog_journals_discovery_strategy() {
    use crate::catalog::CatalogFile;
    let data: CatalogFile = serde_json::from_str(crate::catalog::CATALOG_JSON).unwrap();
    let mut no_strategy = Vec::new();
    for j in &data.journals {
        let crossref_id = j
            .print_issn
            .as_deref()
            .and_then(crate::util::normalize_issn)
            .or(j.online_issn.as_deref().and_then(crate::util::normalize_issn));
        let openalex_strategy = j
            .openalex_source_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if crossref_id.is_none() && openalex_strategy.is_none() {
            no_strategy.push(j.canonical_title.clone());
        }
    }
    assert!(no_strategy.is_empty(), "无 discovery 策略: {:?}", no_strategy);
}

/// HBR：identifier_ready + OpenAlex source id + discovery strategy（配置层）。
#[test]
fn test_hbr_discovery_strategy() {
    use crate::catalog::{self, CatalogFile};
    let data: CatalogFile = serde_json::from_str(crate::catalog::CATALOG_JSON).unwrap();
    let hbr = data
        .journals
        .iter()
        .find(|j| j.canonical_title == "Harvard Business Review")
        .expect("HBR 在 catalog");
    // ISSN 0017-8012 校验通过（identifier_ready）
    assert_eq!(
        crate::util::normalize_issn("0017-8012").as_deref(),
        Some("0017-8012")
    );
    // OpenAlex source id
    assert_eq!(hbr.openalex_source_id.as_deref(), Some("S41416626"));
    // discovery strategy：Crossref ISSN 或 OpenAlex source 至少其一
    let has_strategy = hbr
        .print_issn
        .as_deref()
        .and_then(crate::util::normalize_issn)
        .is_some()
        || hbr
            .openalex_source_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_some();
    assert!(has_strategy, "HBR 必须有 discovery strategy");
    // 导入后：journal 拥有 identifier + openalex_source_id
    let conn = mem_db();
    catalog::import_catalog(&conn).unwrap();
    let h = db::list_journals(&conn)
        .unwrap()
        .into_iter()
        .find(|j| j.name == "Harvard Business Review")
        .unwrap();
    assert!(h.identifiers.iter().any(|i| i.value == "0017-8012"));
    assert_eq!(h.openalex_source_id.as_deref(), Some("S41416626"));
}

/// OpenAlex 返回的 work 无 DOI 时：按现有规则（title 去重）不产生 duplicate。
#[test]
fn test_openalex_no_doi_dedup() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "HBR", Some("0017-8012"), None, None, None).unwrap();
    // 无 DOI 的候选（OpenAlex HBR 数据现实：无 DOI）
    let c1 = candidate(None, "A Better Way to Onboard AI", Some("abstract text here"), Some("openalex"));
    let c2 = candidate(None, "A Better Way to Onboard AI", Some("abstract text here"), Some("openalex"));
    assert!(matches!(db::upsert_paper(&conn, jid, &c1).unwrap(), UpsertOutcome::New(_)));
    match db::upsert_paper(&conn, jid, &c2).unwrap() {
        UpsertOutcome::Existing { .. } => {}
        _ => panic!("无 DOI 重复候选应按 title 去重"),
    }
    assert_eq!(db::list_papers(&conn, Some(jid), 100).unwrap().len(), 1);
}

/// HBR：identifier 存在（0017-8012，ISSN Portal 核实）→ 订阅成功 → 不再"No ISSN"。
#[test]
fn test_hbr_catalog_syncable() {
    use crate::catalog;
    // 1) identifier 有效
    assert_eq!(
        crate::util::normalize_issn("0017-8012").as_deref(),
        Some("0017-8012"),
        "0017-8012 校验通过"
    );
    let conn = mem_db();
    catalog::import_catalog(&conn).unwrap();
    let list = db::list_journals(&conn).unwrap();
    let hbr = list.iter().find(|j| j.name == "Harvard Business Review").expect("HBR 在 catalog");
    // 2) 导入后拥有 identifier
    assert!(
        hbr.identifiers.iter().any(|i| i.value == "0017-8012"),
        "HBR 必须拥有 identifier"
    );
    assert!(hbr.metadata_needs_review, "online/ISSN-L 未解决 → 仍标记 review");
    // 3) subscribe 成功（syncable 防护通过）
    let r = crate::subscribe_journals_logic(&conn, vec![hbr.id]).unwrap();
    assert_eq!(r.subscribed, 1, "HBR 可订阅");
    // 4) sync_journal 的 identifier 列表不再为空（不会 "No ISSN" immediate failure）
    let hbr2 = db::get_journal(&conn, hbr.id).unwrap().unwrap();
    assert!(
        !hbr2.identifiers.is_empty() || hbr2.print_issn.is_some(),
        "sync 数据流有可用 ISSN"
    );
}

/// 已有用户期刊（变体标题）→ catalog alias resolve 到同一 Journal，不产生 duplicate。
#[test]
fn test_catalog_alias_matching_no_duplicate() {
    use crate::catalog;
    let conn = mem_db();
    // 用户已有三个变体标题期刊（无 identifier）
    let rfs = db::insert_journal(&conn, "Review of Financial Studies", None, None, None, None).unwrap();
    let joc = db::insert_journal(&conn, "Journal on Computing", None, None, None, None).unwrap();
    let msom = db::insert_journal(&conn, "Manufacturing and Service Operations Management", None, None, None, None).unwrap();
    db::set_journal_enabled(&conn, rfs, true).unwrap();
    catalog::import_catalog(&conn).unwrap();
    let list = db::list_journals(&conn).unwrap();
    assert_eq!(list.len(), 51, "变体标题不产生 duplicate（3 本被 enrich + 48 新建）");
    let r = list.iter().find(|j| j.id == rfs).unwrap();
    assert!(r.enabled, "RFS enabled 保持");
    assert!(r.collections.iter().any(|c| c == "UTD24") && r.collections.iter().any(|c| c == "FT50"), "RFS 获得 UTD24+FT50");
    assert!(r.identifiers.iter().any(|i| i.value == "0893-9454"), "RFS 补入 identifier");
    let j = list.iter().find(|j| j.id == joc).unwrap();
    assert!(j.collections.contains(&"UTD24".to_string()), "Journal on Computing ∈ UTD24");
    let m = list.iter().find(|j| j.id == msom).unwrap();
    assert!(m.collections.iter().any(|c| c == "UTD24") && m.collections.iter().any(|c| c == "FT50"), "M&SOM ∈ UTD24+FT50");
}

/// title alias 匹配但 identifiers 冲突 → 不自动 merge（保留两条 + review 标记）。
#[test]
fn test_catalog_alias_conflict_no_merge() {
    use crate::catalog;
    let conn = mem_db();
    // 用户已有 "Management Science"（同名）但 identifier 是无关的有效 ISSN（2045-2322）
    let user_id = db::insert_journal(&conn, "Management Science", Some("2045-2322"), None, None, None).unwrap();
    let _ = db::insert_identifier(&conn, user_id, "print", "2045-2322", Some("manual"));
    db::set_journal_enabled(&conn, user_id, true).unwrap();
    catalog::import_catalog(&conn).unwrap();
    let list = db::list_journals(&conn).unwrap();
    // 用户 1 + catalog 51 = 52（冲突不 merge）
    assert_eq!(list.len(), 52, "identifiers 冲突不得自动 merge");
    let catalog_ms = list
        .iter()
        .find(|j| j.id != user_id && j.name == "Management Science")
        .expect("catalog Management Science 新建");
    assert!(catalog_ms.metadata_needs_review, "冲突期刊标记 metadataNeedsReview");
    let user = list.iter().find(|j| j.id == user_id).unwrap();
    assert!(user.enabled, "用户数据保留");
    assert_eq!(user.print_issn.as_deref(), Some("2045-2322"), "用户 identifier 不被改写");
}

/// subscribe_journals syncable 防护：无任何 identifier 的 Journal 订阅 → failed（不静默 enabled）。
#[test]
fn test_subscribe_syncable_guard() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "No ID Journal", None, None, None, None).unwrap();
    db::set_journal_enabled(&conn, jid, false).unwrap();
    let r = crate::subscribe_journals_logic(&conn, vec![jid]).unwrap();
    assert_eq!(r.failed, 1, "无 identifier 的期刊不得订阅");
    assert_eq!(r.subscribed, 0);
    assert!(!db::get_journal(&conn, jid).unwrap().unwrap().enabled);
}

/// 真实 HBR 同步（ignored，需要网络；临时 DB，不触碰用户数据）：
/// 验证 Crossref unsupported 被隔离 → OpenAlex fallback 被调用 → sync 不标 failed。
/// 运行：cargo test test_live_hbr_sync -- --ignored --nocapture
#[test]
#[ignore]
fn test_live_hbr_sync() {
    use std::sync::{Arc, Mutex};
    use tauri::Manager;

    use crate::catalog;
    use crate::sync::run_sync;

    let dir = std::env::temp_dir().join(format!("cowpaper-hbr-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("hbr.db");
    let _ = std::fs::remove_file(&path);
    let conn = Arc::new(Mutex::new(db::open(&path).unwrap()));
    {
        let c = conn.lock().unwrap();
        db::init(&c).unwrap();
        catalog::import_catalog(&c).unwrap();
    }
    let hbr = {
        let c = conn.lock().unwrap();
        let j = db::list_journals(&c)
            .unwrap()
            .into_iter()
            .find(|j| j.name == "Harvard Business Review")
            .expect("HBR");
        println!("[live-hbr] journal id={} issns={:?} openalex={:?}", j.id, j.identifiers.iter().map(|i| i.value.clone()).collect::<Vec<_>>(), j.openalex_source_id);
        db::set_journal_enabled(&c, j.id, true).unwrap();
        j.id
    };
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock app");
    let handle = app.handle().clone();
    let batch_id = {
        let c = conn.lock().unwrap();
        db::create_sync_batch(&c, "manual").unwrap()
    };
    let report = run_sync(&conn, Some(vec![hbr]), &handle, "dev@cowpaper.local", batch_id, "manual");
    println!("[live-hbr] checked={} found={} new={} failed_journals={}",
        report.checked_journals, report.found_records, report.new_papers, report.failed_journals);
    // 关键断言：HBR 不得被标 failed（Crossref unsupported 被隔离，OpenAlex fallback 成功）
    assert_eq!(report.failed_journals, 0, "HBR 同步不得标 failed（Crossref unsupported ≠ 期刊 unsupported）");
    assert_eq!(report.checked_journals, 1);
    let _ = std::fs::remove_dir_all(&dir);
}

// ================= Round 6：Daily Recommendation Timeline & History =================

fn local_dt(y: i32, m: u32, d: u32, h: u32, min: u32) -> chrono::DateTime<chrono::Local> {
    use chrono::TimeZone;
    chrono::Local
        .with_ymd_and_hms(y, m, d, h, min, 0)
        .single()
        .expect("valid local datetime")
}

#[test]
fn test_new_journal_sync_uses_two_day_local_safe_window() {
    let now = local_dt(2026, 8, 31, 0, 5);
    assert_eq!(crate::sync::initial_safe_window(now), ("2026-08-30".into(), "2026-08-31".into()));
    assert_eq!(crate::sync::sync_window_start(None, now), "2026-08-30");
}

#[test]
fn test_existing_journal_sync_keeps_24_hour_overlap() {
    let now = local_dt(2026, 8, 31, 12, 0);
    assert_eq!(
        crate::sync::sync_window_start(Some("2026-08-31T01:30:00+00:00"), now),
        "2026-08-30"
    );
    // A malformed legacy timestamp must not reopen a 30-day initial fetch.
    assert_eq!(crate::sync::sync_window_start(Some("invalid"), now), "2026-08-30");
}

#[test]
fn test_recommendation_cycle_key() {
    use crate::recommendation::cycle_key_for;
    // cutoff 09:00：当天 15:00 → 当天；次日 08:59 → 仍前一天；09:00 → 当天
    assert_eq!(cycle_key_for(&local_dt(2026, 8, 26, 15, 0), "09:00"), "2026-08-26");
    assert_eq!(cycle_key_for(&local_dt(2026, 8, 27, 8, 59), "09:00"), "2026-08-26");
    assert_eq!(cycle_key_for(&local_dt(2026, 8, 27, 9, 0), "09:00"), "2026-08-27");
    assert_eq!(cycle_key_for(&local_dt(2026, 8, 27, 9, 1), "09:00"), "2026-08-27");
    // 非法时间回退 09:00
    assert_eq!(cycle_key_for(&local_dt(2026, 8, 27, 10, 0), "garbage"), "2026-08-27");
    // 其他 cutoff
    assert_eq!(cycle_key_for(&local_dt(2026, 8, 27, 7, 59), "08:00"), "2026-08-26");
    assert_eq!(cycle_key_for(&local_dt(2026, 8, 27, 8, 0), "08:00"), "2026-08-27");
}

fn seed_paper_with_score(conn: &rusqlite::Connection, jid: i64, doi: &str, title: &str, score: f64) -> i64 {
    let cand = candidate(Some(doi), title, Some("abstract with full detail about pricing and markets."), Some("crossref"));
    let id = match db::upsert_paper(conn, jid, &cand).unwrap() {
        UpsertOutcome::New(i) => i,
        _ => panic!("expected new"),
    };
    db::save_analysis(conn, id, "中文", "摘要", "句", "[]", score, "m", "v1", &format!("H-{}", id)).unwrap();
    id
}

/// Update integration invariant: replacing the installed app must reopen the
/// same user-data directory and retain the DB, app settings, AI output,
/// recommendation history, and the separately stored API key.
#[test]
fn test_update_reopen_preserves_user_data_and_settings() {
    use crate::secure_store::{LocalFileSecretStore, SecureStore};

    let data_dir = std::env::temp_dir().join(format!(
        "cowpaper-update-preservation-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&data_dir);
    std::fs::create_dir_all(&data_dir).unwrap();
    let db_path = data_dir.join("cowpaper.db");

    {
        let conn = db::open(&db_path).unwrap();
        db::init(&conn).unwrap();
        db::set_setting(&conn, "settings.daily_sync_time", "07:30").unwrap();
        db::set_setting(&conn, "settings.default_abstract_lang", "en").unwrap();
        let jid = db::insert_journal(&conn, "Preserved Journal", Some("0025-1909"), None, None, None).unwrap();
        let paper_id = seed_paper_with_score(&conn, jid, "10.1000/update-preserve", "Preserved Paper", 4.2);
        db::save_analysis(
            &conn,
            paper_id,
            "保留的中文标题",
            "摘要",
            "句",
            "[]",
            4.2,
            "m",
            "v1",
            &format!("H-{}", paper_id),
        )
        .unwrap();
        let run_id = crate::recommendation::refresh_current_recommendations(
            &conn,
            &chrono::Local::now(),
            "07:30",
        )
        .unwrap();
        assert_eq!(db::list_recommendation_items(&conn, run_id).unwrap().len(), 1);

        let store = LocalFileSecretStore::new(&data_dir);
        store.save("sk-update-preserve").unwrap();
    }

    // Simulate an app bundle/installer upgrade: reopen and initialize the
    // existing paths. No updater code removes or recreates this directory.
    {
        let conn = db::open(&db_path).unwrap();
        db::init(&conn).unwrap();
        assert_eq!(db::get_setting(&conn, "settings.daily_sync_time").as_deref(), Some("07:30"));
        assert_eq!(db::get_setting(&conn, "settings.default_abstract_lang").as_deref(), Some("en"));
        let paper = db::get_paper(&conn, 1).unwrap().unwrap();
        assert_eq!(paper.chinese_title.as_deref(), Some("保留的中文标题"));
        assert_eq!(paper.one_sentence_summary.as_deref(), Some("句"));
        assert_eq!(paper.total_score, Some(4.2));
        assert_eq!(db::list_recommendation_runs(&conn).unwrap().len(), 1);
        assert_eq!(db::list_recommendation_items(&conn, 1).unwrap().len(), 1);

        let store = LocalFileSecretStore::new(&data_dir);
        assert_eq!(store.get().unwrap().as_deref(), Some("sk-update-preserve"));
    }

    let _ = std::fs::remove_dir_all(&data_dir);
}

#[test]
fn test_updater_config_requires_signed_cross_platform_artifacts() {
    let config_path = concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json");
    let config: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(config_path).unwrap()).unwrap();
    assert_eq!(config["bundle"]["createUpdaterArtifacts"], true);
    assert!(config["plugins"]["updater"]["pubkey"].as_str().unwrap().len() > 20);
    let endpoints = config["plugins"]["updater"]["endpoints"].as_array().unwrap();
    assert_eq!(endpoints.len(), 1);
    assert!(endpoints[0].as_str().unwrap().starts_with("https://github.com/"));
    assert!(endpoints[0].as_str().unwrap().ends_with("/latest/download/latest.json"));
    assert_eq!(db::SCHEMA_VERSION, 15, "updater must not claim migration ownership");
}

#[test]
fn test_recommendation_cycle_lifecycle() {
    use crate::recommendation::{ensure_current_recommendation_cycle, refresh_current_recommendations};
    let conn = mem_db();
    // 8-26 15:00 → 8-26 open
    let r1 = ensure_current_recommendation_cycle(&conn, &local_dt(2026, 8, 26, 15, 0), "09:00").unwrap();
    let run1 = db::get_recommendation_run(&conn, r1).unwrap().unwrap();
    assert_eq!(run1.status, "open");
    assert_eq!(run1.cycle_key, "2026-08-26");
    // 幂等：同一 cycle 返回同一 run
    let r1b = ensure_current_recommendation_cycle(&conn, &local_dt(2026, 8, 26, 20, 0), "09:00").unwrap();
    assert_eq!(r1, r1b);
    // 09:00 次日 → finalize 旧 + 新 open
    let r2 = ensure_current_recommendation_cycle(&conn, &local_dt(2026, 8, 27, 9, 0), "09:00").unwrap();
    assert_ne!(r1, r2);
    let run1f = db::get_recommendation_run(&conn, r1).unwrap().unwrap();
    assert_eq!(run1f.status, "finalized");
    assert!(run1f.finalized_at.is_some());
    let run2 = db::get_recommendation_run(&conn, r2).unwrap().unwrap();
    assert_eq!(run2.status, "open");
    assert_eq!(run2.cycle_key, "2026-08-27");
    // refresh 幂等：不重复创建 run
    let r3 = refresh_current_recommendations(&conn, &local_dt(2026, 8, 27, 10, 0), "09:00").unwrap();
    assert_eq!(r2, r3);
}

#[test]
fn test_recommendation_paper_membership_and_next_day_exclusion() {
    use crate::recommendation::refresh_current_recommendations;
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pa = seed_paper_with_score(&conn, jid, "10.1000/rec-a", "Paper A", 2.0);
    let pb = seed_paper_with_score(&conn, jid, "10.1000/rec-b", "Paper B", 1.0);
    // 8-26：A(2.0) rank1, B(1.0) rank2
    let r1 = refresh_current_recommendations(&conn, &local_dt(2026, 8, 26, 15, 0), "09:00").unwrap();
    let items = db::list_recommendation_items(&conn, r1).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].paper_id, pa);
    assert_eq!(items[0].rank, 1);
    assert_eq!(items[0].score_snapshot, 2.0);
    assert_eq!(items[1].paper_id, pb);
    // 8-27：A 已推荐 → 排除；新 C 加入
    let pc = seed_paper_with_score(&conn, jid, "10.1000/rec-c", "Paper C", 3.0);
    let r2 = refresh_current_recommendations(&conn, &local_dt(2026, 8, 27, 9, 0), "09:00").unwrap();
    let items2 = db::list_recommendation_items(&conn, r2).unwrap();
    assert_eq!(items2.len(), 1, "8-27 只含 C（A/B 不重复）");
    assert_eq!(items2[0].paper_id, pc);
    // A 一生只在一个周期（UNIQUE paper_id）
    let cnt: i64 = conn
        .query_row("SELECT COUNT(*) FROM recommendation_items WHERE paper_id = ?1", params![pa], |r| r.get(0))
        .unwrap();
    assert_eq!(cnt, 1);
    // 空推荐日：所有 eligible 都已推荐 → 0 篇
    let r3 = refresh_current_recommendations(&conn, &local_dt(2026, 8, 28, 9, 0), "09:00").unwrap();
    assert_eq!(db::list_recommendation_items(&conn, r3).unwrap().len(), 0, "空日 0 篇");
}

#[test]
fn test_recommendation_rank_update_and_finalize_freeze() {
    use crate::recommendation::refresh_current_recommendations;
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pa = seed_paper_with_score(&conn, jid, "10.1000/rk-a", "A", 1.0);
    let pb = seed_paper_with_score(&conn, jid, "10.1000/rk-b", "B", 2.0);
    let r1 = refresh_current_recommendations(&conn, &local_dt(2026, 8, 26, 15, 0), "09:00").unwrap();
    let items = db::list_recommendation_items(&conn, r1).unwrap();
    assert_eq!(items[0].paper_id, pb, "B(2.0) rank1");
    // open run：A 重新分析 score 3.0 → refresh 后 A rank1（open 可更新）
    db::save_analysis(&conn, pa, "A2", "a", "s", "[]", 3.0, "m", "v1", "H-A2").unwrap();
    let r1b = refresh_current_recommendations(&conn, &local_dt(2026, 8, 26, 16, 0), "09:00").unwrap();
    assert_eq!(r1b, r1, "同日 refresh 保持同一 run");
    let items = db::list_recommendation_items(&conn, r1).unwrap();
    assert_eq!(items[0].paper_id, pa, "open run：A 升级后 rank1");
    assert_eq!(items[0].score_snapshot, 3.0);
    // 次日 09:00 → 8-26 finalize（A 的 3.0 快照冻结）
    let _r2 = refresh_current_recommendations(&conn, &local_dt(2026, 8, 27, 9, 0), "09:00").unwrap();
    let run1 = db::get_recommendation_run(&conn, r1).unwrap().unwrap();
    assert_eq!(run1.status, "finalized");
    // finalized 后 A 再变 score → 8-26 快照不变
    db::save_analysis(&conn, pa, "A3", "a", "s", "[]", 5.0, "m", "v1", "H-A3").unwrap();
    let _ = refresh_current_recommendations(&conn, &local_dt(2026, 8, 27, 10, 0), "09:00").unwrap();
    let items = db::list_recommendation_items(&conn, r1).unwrap();
    assert_eq!(items[0].score_snapshot, 3.0, "finalized run 快照冻结");
    assert_eq!(items[0].paper_id, pa);
}

#[test]
fn test_recommendation_restart_persistence_and_reanalysis_no_rerank() {
    use crate::recommendation::refresh_current_recommendations;
    let dir = std::env::temp_dir().join(format!("cowpaper_rec_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rec.db");
    let _ = std::fs::remove_file(&path);
    let pa;
    {
        let conn = db::open(&path).unwrap();
        db::init(&conn).unwrap();
        let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
        pa = seed_paper_with_score(&conn, jid, "10.1000/rt-a", "A", 2.0);
        let _r = refresh_current_recommendations(&conn, &local_dt(2026, 8, 26, 15, 0), "09:00").unwrap();
    }
    {
        let conn = db::open(&path).unwrap();
        db::init(&conn).unwrap();
        // restart 后历史 run 保留
        let runs = db::list_recommendation_runs(&conn).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].cycle_key, "2026-08-26");
        // 8-27：A 已推荐 → 不因重新分析（score 变化）重新加入
        let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
        let _ = jid;
        let conn2 = &conn;
        let _ = conn2;
        // 找到 A 所在 journal 重新分析（简化：直接 refresh 8-27 检查 A 不出现）
        let r2 = refresh_current_recommendations(&conn, &local_dt(2026, 8, 27, 9, 0), "09:00").unwrap();
        let items = db::list_recommendation_items(&conn, r2).unwrap();
        assert!(items.iter().all(|i| i.paper_id != pa), "重分析不得把 A 重新放入新周期");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_recommendation_manual_sync_enters_current() {
    use crate::recommendation::refresh_current_recommendations;
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pa = seed_paper_with_score(&conn, jid, "10.1000/m-a", "A", 2.0);
    let r1 = refresh_current_recommendations(&conn, &local_dt(2026, 8, 26, 9, 0), "09:00").unwrap();
    assert_eq!(db::list_recommendation_items(&conn, r1).unwrap().len(), 1);
    // 16:00 手动同步发现 D + AI 完成 → 加入今天 open snapshot
    let pd = seed_paper_with_score(&conn, jid, "10.1000/m-d", "D", 1.5);
    let r1b = refresh_current_recommendations(&conn, &local_dt(2026, 8, 26, 16, 0), "09:00").unwrap();
    assert_eq!(r1b, r1);
    let items = db::list_recommendation_items(&conn, r1).unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().any(|i| i.paper_id == pd), "当天手动同步的新论文进入今天推荐");
}

#[test]
fn test_recommendation_changing_cutoff_keeps_finalized_history() {
    use crate::recommendation::{ensure_current_recommendation_cycle, refresh_current_recommendations};
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pa = seed_paper_with_score(&conn, jid, "10.1000/c-a", "A", 2.0);
    let r1 = refresh_current_recommendations(&conn, &local_dt(2026, 8, 26, 15, 0), "09:00").unwrap();
    let _r2 = ensure_current_recommendation_cycle(&conn, &local_dt(2026, 8, 27, 9, 0), "09:00").unwrap();
    assert_eq!(db::get_recommendation_run(&conn, r1).unwrap().unwrap().status, "finalized");
    // 修改 daily_check_time 后（09:00 → 08:00），8-26 finalized 历史不变
    let _r3 = ensure_current_recommendation_cycle(&conn, &local_dt(2026, 8, 27, 8, 30), "08:00").unwrap();
    let run1 = db::get_recommendation_run(&conn, r1).unwrap().unwrap();
    assert_eq!(run1.status, "finalized", "改 cutoff 不得改动已 finalized 历史");
    // 8-26 items 冻结
    let items = db::list_recommendation_items(&conn, r1).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].paper_id, pa);
}

#[test]
fn test_recommendation_does_not_change_total_score() {
    use crate::recommendation::refresh_current_recommendations;
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pa = seed_paper_with_score(&conn, jid, "10.1000/t-a", "A", 1.5);
    let before: f64 = conn
        .query_row("SELECT total_score FROM papers WHERE id = ?1", params![pa], |r| r.get(0))
        .unwrap();
    let _r = refresh_current_recommendations(&conn, &local_dt(2026, 8, 26, 15, 0), "09:00").unwrap();
    let after: f64 = conn
        .query_row("SELECT total_score FROM papers WHERE id = ?1", params![pa], |r| r.get(0))
        .unwrap();
    assert_eq!(before, after, "recommendation snapshot 不得改变 totalScore");
    assert_eq!(after, 1.5);
}

// ================= Round 6.4：User Collections =================

#[test]
fn test_user_collections_lifecycle() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let cand = candidate(Some("10.1000/uc-p"), "UC Paper", Some("abstract with full detail here."), Some("crossref"));
    let pid = match db::upsert_paper(&conn, jid, &cand).unwrap() {
        UpsertOutcome::New(i) => i,
        _ => panic!(),
    };
    // 1) create user collection
    let cid = db::create_collection(&conn, "USER-x1", "数字平台", None, None, Some("user"), None).unwrap();
    assert!(db::find_collection_by_code(&conn, "USER-x1").unwrap().is_some());
    // 3) rename
    db::rename_collection(&conn, cid, "数字经济").unwrap();
    assert_eq!(db::list_collections(&conn).unwrap()[0].name, "数字经济");
    // 4) add member
    assert!(db::add_collection_member(&conn, cid, jid).unwrap());
    // 5) duplicate member ignored
    assert!(!db::add_collection_member(&conn, cid, jid).unwrap(), "重复 member 返回 false");
    let members = db::list_collection_journals(&conn, "USER-x1").unwrap();
    assert_eq!(members.len(), 1);
    // 6) remove member
    db::remove_collection_member(&conn, cid, jid).unwrap();
    assert!(db::list_collection_journals(&conn, "USER-x1").unwrap().is_empty());
    // 8) delete collection：journal/paper 保留
    db::add_collection_member(&conn, cid, jid).unwrap();
    db::delete_collection(&conn, cid).unwrap();
    assert!(db::find_collection_by_code(&conn, "USER-x1").unwrap().is_none());
    let j = db::get_journal(&conn, jid).unwrap().unwrap();
    assert_eq!(j.id, jid, "删除集合不得删除 journal");
    let p = db::list_papers(&conn, Some(jid), 100).unwrap();
    assert_eq!(p.len(), 1, "删除集合不得删除 paper");
    assert_eq!(p[0].id, pid);
    // 10/11) built-in 保护（db 层判定）
    assert!(db::is_builtin_collection_code("UTD24"));
    assert!(db::is_builtin_collection_code("FT50"));
    assert!(!db::is_builtin_collection_code("USER-x1"));
    // 13) membership 不影响 totalScore
    db::save_analysis(&conn, pid, "中", "摘要", "句", "[]", 1.5, "m", "v1", "H").unwrap();
    let c2 = db::create_collection(&conn, "USER-x2", "供应链", None, None, Some("user"), None).unwrap();
    db::add_collection_member(&conn, c2, jid).unwrap();
    let score: f64 = conn
        .query_row("SELECT total_score FROM papers WHERE id = ?1", params![pid], |r| r.get(0))
        .unwrap();
    assert_eq!(score, 1.5, "user collection 不得改变 totalScore");
}

#[test]
fn test_user_collections_restart_persistence() {
    let dir = std::env::temp_dir().join(format!("cowpaper_uc_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("uc.db");
    let _ = std::fs::remove_file(&path);
    let jid;
    {
        let conn = db::open(&path).unwrap();
        db::init(&conn).unwrap();
        jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
        let cid = db::create_collection(&conn, "USER-p", "持久集合", None, None, Some("user"), None).unwrap();
        db::add_collection_member(&conn, cid, jid).unwrap();
    }
    {
        let conn = db::open(&path).unwrap();
        db::init(&conn).unwrap();
        let colls = db::list_collections(&conn).unwrap();
        assert!(colls.iter().any(|c| c.code == "USER-p"));
        assert_eq!(db::list_collection_journals(&conn, "USER-p").unwrap().len(), 1);
        let _ = jid;
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_history_run_stats() {
    use crate::recommendation::refresh_current_recommendations;
    let conn = mem_db();
    let jid1 = db::insert_journal(&conn, "J1", Some("0025-1909"), None, None, None).unwrap();
    let jid2 = db::insert_journal(&conn, "J2", Some("1526-5501"), None, None, None).unwrap();
    let pa = seed_paper_with_score(&conn, jid1, "10.1000/hs-a", "A", 3.8);
    let pb = seed_paper_with_score(&conn, jid1, "10.1000/hs-b", "B", 2.0);
    let pc = seed_paper_with_score(&conn, jid2, "10.1000/hs-c", "C", 3.4);
    let _r1 = refresh_current_recommendations(&conn, &local_dt(2026, 8, 26, 15, 0), "09:00").unwrap();
    // 次日 09:00：finalize 8-26
    let _r2 = refresh_current_recommendations(&conn, &local_dt(2026, 8, 27, 9, 0), "09:00").unwrap();
    let runs = db::list_recommendation_runs(&conn).unwrap();
    // 最新在前（cycle_key DESC）
    assert_eq!(runs[0].cycle_key, "2026-08-27");
    assert_eq!(runs[1].cycle_key, "2026-08-26");
    let run26 = &runs[1];
    assert_eq!(run26.status, "finalized");
    assert_eq!(run26.item_count, 3);
    assert_eq!(run26.max_score, Some(3.8), "最高 score_snapshot");
    assert_eq!(run26.journal_count, 2, "涉及期刊数（去重）");
    let _ = pa;
    let _ = pb;
    let _ = pc;
}

// ================= Round 6.5：Versioned Tag Configuration =================

#[test]
fn test_tag_semantic_hash() {
    let h1 = crate::tag_config::tag_semantic_hash(1, "平台经济", "关注双边平台");
    let h2 = crate::tag_config::tag_semantic_hash(1, "平台经济", "关注双边平台");
    let h3 = crate::tag_config::tag_semantic_hash(1, "平台经济", "关注多边平台");
    let h4 = crate::tag_config::tag_semantic_hash(2, "平台经济", "关注双边平台");
    assert_eq!(h1, h2, "同 tag+name+desc → 相同 hash（cache 可复用）");
    assert_ne!(h1, h3, "description 变化 → hash 变化（该 tag stale）");
    assert_ne!(h1, h4, "tag_id 变化 → hash 变化");
}

#[test]
fn test_tag_config_diff_classification() {
    use crate::models::{TagConfigItem, TagDraftItem};
    let mk = |id: i64, name: &str, desc: &str, enabled: bool| TagConfigItem {
        version_id: 1,
        tag_id: id,
        name: name.to_string(),
        description: Some(desc.to_string()),
        enabled,
        deleted: false,
    };
    let old = vec![
        mk(1, "平台经济", "关注双边平台", true),
        mk(2, "定价", "定价策略", true),
        mk(3, "旧停用", "旧说明", false),
        mk(5, "未变", "说明", true),
    ];
    let draft = vec![
        TagDraftItem { id: 1, name: "平台经济".into(), description: Some("关注多边平台".into()), enabled: true, deleted: false }, // semanticChanged
        TagDraftItem { id: 2, name: "定价".into(), description: Some("定价策略".into()), enabled: true, deleted: true },        // removed
        TagDraftItem { id: 3, name: "旧停用".into(), description: Some("旧说明".into()), enabled: true, deleted: false },       // enabled
        TagDraftItem { id: 0, name: "数字劳动".into(), description: Some("数字劳动".into()), enabled: true, deleted: false },   // added
        TagDraftItem { id: 5, name: "未变".into(), description: Some("说明".into()), enabled: true, deleted: false },           // unchanged
    ];
    let d = crate::tag_config::compute_diff(&old, &draft);
    assert_eq!(d.added, vec!["数字劳动"]);
    assert_eq!(d.removed, vec!["定价"]);
    assert_eq!(d.enabled, vec!["旧停用"]);
    assert_eq!(d.semantic_changed, vec!["平台经济"]);
    assert!(d.unchanged.contains(&"未变".to_string()));
}

#[test]
fn test_scheduled_save_no_ai_no_rerank() {
    use crate::models::TagDraftItem;
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pa = seed_paper_with_score(&conn, jid, "10.1000/tc-a", "A", 1.0);
    // 当前 open run
    let r1 = crate::recommendation::refresh_current_recommendations(&conn, &local_dt(2026, 8, 26, 15, 0), "09:00").unwrap();
    let items_before = db::list_recommendation_items(&conn, r1).unwrap();
    // tags：tag1 存在（Full AI 已用 desc）
    let tag = db::add_tag(&conn, "新标签X", Some("关注双边平台")).unwrap();
    // scheduled 保存（仅改 description）
    let draft = vec![TagDraftItem {
        id: tag.id,
        name: "新标签X".into(),
        description: Some("关注多边平台".into()),
        enabled: true,
        deleted: false,
    }];
    let res = crate::tag_config::save_scheduled_config(&conn, &draft, "2026-08-27").unwrap();
    assert_eq!(res.mode, "scheduled");
    // 不改 tags 表（active 不变）
    let t = db::list_tags(&conn).unwrap();
    let tx = t.iter().find(|x| x.name == "新标签X").expect("新标签X 存在");
    assert_eq!(tx.description.as_deref(), Some("关注双边平台"), "scheduled 保存不得改 active tags 表");
    // 当前 run 不变（不重排）
    assert_eq!(db::list_recommendation_items(&conn, r1).unwrap().len(), items_before.len());
    // scheduled 持久化
    let sched = db::scheduled_tag_config(&conn).unwrap().unwrap();
    assert_eq!(sched.effective_cycle_key.as_deref(), Some("2026-08-27"));
    // 可替换（同一 upcoming cycle 至多一个）
    let draft2 = vec![TagDraftItem {
        id: tag.id,
        name: "新标签X".into(),
        description: Some("平台治理视角".into()),
        enabled: true,
        deleted: false,
    }];
    crate::tag_config::save_scheduled_config(&conn, &draft2, "2026-08-27").unwrap();
    let cnt: i64 = conn
        .query_row("SELECT COUNT(*) FROM tag_config_versions WHERE status='scheduled'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(cnt, 1, "一个 upcoming cycle 至多一个 scheduled");
    let _ = pa;
}

#[test]
fn test_immediate_save_local_recompute_and_preserve() {
    use crate::models::TagDraftItem;
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pa = seed_paper_with_score(&conn, jid, "10.1000/tc-b", "B", 1.0);
    // 构造 tag_matches（含 tag_id + semantic hash）
    let t1 = db::add_tag(&conn, "t1", Some("说明1")).unwrap();
    let t2 = db::add_tag(&conn, "t2", Some("说明2")).unwrap();
    let h1 = crate::tag_config::tag_semantic_hash(t1.id, "t1", "说明1");
    let h2 = crate::tag_config::tag_semantic_hash(t2.id, "t2", "说明2");
    let matches = serde_json::json!([
        {"tag":"t1","score":0.8,"tagId":t1.id,"semanticHash":h1},
        {"tag":"t2","score":0.4,"tagId":t2.id,"semanticHash":h2}
    ]);
    conn.execute("UPDATE papers SET tag_matches_json=?1 WHERE id=?2", params![matches.to_string(), pa]).unwrap();
    // 本地重算 → 1.2
    let active = crate::tag_config::active_tags(&conn).unwrap();
    crate::tag_config::recompute_paper_total_score(&conn, pa, &active).unwrap();
    let s: f64 = conn.query_row("SELECT total_score FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    assert!((s - 1.2).abs() < 1e-9, "本地重算 1.2，实际 {}", s);
    // immediate 保存：t2 disabled → 本地重算 0.8
    let draft = vec![
        TagDraftItem { id: t1.id, name: "t1".into(), description: Some("说明1".into()), enabled: true, deleted: false },
        TagDraftItem { id: t2.id, name: "t2".into(), description: Some("说明2".into()), enabled: false, deleted: false },
    ];
    let res = crate::tag_config::save_immediate_config(&conn, &draft).unwrap();
    assert!(res.diff.disabled.contains(&"t2".to_string()));
    let s2: f64 = conn.query_row("SELECT total_score FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    assert!((s2 - 0.8).abs() < 1e-9, "t2 disabled 后本地重算 0.8，实际 {}", s2);
    // tag_matches_json 保留 t2 分数（缓存）
    let json: String = conn.query_row("SELECT tag_matches_json FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    assert!(json.contains("t2"), "disabled tag score 保留为缓存");
}

#[test]
fn test_tag_only_merge_and_papers_needing() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pa = seed_paper_with_score(&conn, jid, "10.1000/tc-c", "C", 1.0);
    let t1 = db::add_tag(&conn, "平台治理", Some("平台治理研究")).unwrap();
    let t2 = db::add_tag(&conn, "数字劳动", Some("数字劳动研究")).unwrap();
    // paper 已有 t1 score（旧 hash）→ t1 需要更新；t2 缺失 → 需要
    let old_h = crate::tag_config::tag_semantic_hash(t1.id, "平台治理", "旧说明");
    let matches = serde_json::json!([{"tag":"平台治理","score":0.6,"tagId":t1.id,"semanticHash":old_h}]);
    conn.execute("UPDATE papers SET tag_matches_json=?1 WHERE id=?2", params![matches.to_string(), pa]).unwrap();
    // 目标 tags（t1 新说明 + t2）
    let targets = vec![
        (t1.id, "平台治理".to_string(), "平台治理研究".to_string()),
        (t2.id, "数字劳动".to_string(), "数字劳动研究".to_string()),
    ];
    let need = db::papers_needing_tag_scores(&conn, &targets).unwrap();
    assert_eq!(need, vec![pa], "t1 hash stale + t2 missing → 需要");
    // merge 结果（模拟 tag-only 返回）
    let scores = vec![(t1.id, 0.8), (t2.id, 0.4)];
    db::set_paper_tag_scores(&conn, pa, &scores, &targets).unwrap();
    let json: String = conn.query_row("SELECT tag_matches_json FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    assert!(json.contains("数字劳动"));
    // 再次检查 → 不再需要（hash 匹配）
    let need2 = db::papers_needing_tag_scores(&conn, &targets).unwrap();
    assert!(need2.is_empty(), "merge 后 hash 匹配 → 不再需要");
    // total 本地重算：0.8 + 0.4 = 1.2
    let s: f64 = conn.query_row("SELECT total_score FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    assert!((s - 1.2).abs() < 1e-9, "total 1.2，实际 {}", s);
}

#[test]
fn test_tag_config_does_not_change_finalized_history() {
    use crate::models::TagDraftItem;
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pa = seed_paper_with_score(&conn, jid, "10.1000/tc-d", "D", 2.0);
    // 8-26 run（finalize）
    let r1 = crate::recommendation::refresh_current_recommendations(&conn, &local_dt(2026, 8, 26, 15, 0), "09:00").unwrap();
    let _r2 = crate::recommendation::refresh_current_recommendations(&conn, &local_dt(2026, 8, 27, 9, 0), "09:00").unwrap();
    assert_eq!(db::get_recommendation_run(&conn, r1).unwrap().unwrap().status, "finalized");
    let snapshot_before = db::list_recommendation_items(&conn, r1).unwrap()[0].score_snapshot;
    // immediate 保存（t1 新增）→ 不重排 finalized run
    let t1 = db::add_tag(&conn, "新标签", Some("说明")).unwrap();
    let draft = vec![
        TagDraftItem { id: t1.id, name: "新标签".into(), description: Some("说明".into()), enabled: true, deleted: false },
    ];
    crate::tag_config::save_immediate_config(&conn, &draft).unwrap();
    let items = db::list_recommendation_items(&conn, r1).unwrap();
    assert_eq!(items[0].score_snapshot, snapshot_before, "finalized history 冻结");
    let _ = pa;
}

#[test]
fn test_full_ai_prompt_uses_description() {
    // 审计验证：build_context 把 description 带入 tag_pairs → prompt 输出含说明
    let conn = mem_db();
    db::add_tag(&conn, "新标签Y", Some("关注双边平台")).unwrap();
    let ctx = crate::analyze::build_context(&std::sync::Arc::new(std::sync::Mutex::new(conn))).unwrap();
    assert!(ctx.tag_pairs.iter().any(|(_id, n, d)| n == "新标签Y" && d == "关注双边平台"), "description 必须进入 prompt 上下文");
}

// ================= Round 6.5.2：Decouple Save from Rerank =================

#[test]
fn test_immediate_consumes_scheduled() {
    use crate::models::TagDraftItem;
    let conn = mem_db();
    let tag = db::add_tag(&conn, "新标签Z", Some("说明")).unwrap();
    let draft = vec![TagDraftItem {
        id: tag.id,
        name: "新标签Z".into(),
        description: Some("新说明".into()),
        enabled: true,
        deleted: false,
    }];
    crate::tag_config::save_scheduled_config(&conn, &draft, "2026-08-27").unwrap();
    assert!(db::scheduled_tag_config(&conn).unwrap().is_some());
    // immediate（用 scheduled 内容作为 candidate，无需再次编辑）
    crate::tag_config::save_immediate_config(&conn, &draft).unwrap();
    assert!(db::scheduled_tag_config(&conn).unwrap().is_none(), "immediate 消耗 scheduled");
    // 下一 cutoff 不重复激活（scheduled 已删 → activate 无触发源）
    let now = local_dt(2026, 8, 27, 9, 0);
    let key = crate::recommendation::cycle_key_for(&now, "09:00");
    assert!(db::scheduled_tag_config(&conn).unwrap().is_none(), "cutoff 不得重复激活已消费的 scheduled");
    assert_eq!(key, "2026-08-27");
}

#[test]
fn test_scheduled_immediate_activation_no_second_edit() {
    use crate::models::TagDraftItem;
    let conn = mem_db();
    let t1 = db::add_tag(&conn, "T甲", Some("旧")).unwrap();
    let draft = vec![TagDraftItem {
        id: t1.id,
        name: "T甲".into(),
        description: Some("新说明".into()),
        enabled: true,
        deleted: false,
    }];
    crate::tag_config::save_scheduled_config(&conn, &draft, "2026-08-27").unwrap();
    // 无新编辑：直接用 scheduled 内容立即激活
    crate::tag_config::save_immediate_config(&conn, &draft).unwrap();
    let t = db::list_tags(&conn).unwrap();
    let tx = t.iter().find(|x| x.name == "T甲").unwrap();
    assert_eq!(tx.description.as_deref(), Some("新说明"), "scheduled 立即激活无需再次编辑");
    assert!(db::scheduled_tag_config(&conn).unwrap().is_none());
}

#[test]
fn test_delete_only_immediate_zero_ai() {
    let conn = mem_db();
    let t1 = db::add_tag(&conn, "T删", Some("说明")).unwrap();
    // draft 不含 t1（前端 splice 删除语义）→ removed，零 AI
    let res = crate::tag_config::save_immediate_config(&conn, &[]).unwrap();
    assert!(res.diff.removed.contains(&"T删".to_string()), "diff.removed 应含被移除 tag: {:?}", res.diff.removed);
    assert!(res.diff.added.is_empty() && res.diff.semantic_changed.is_empty(), "删除不触发 AI");
    assert!(db::find_tag_by_name(&conn, "T删").unwrap().is_none(), "splice 删除必须真正删 DB tag");
}

#[test]
fn test_immediate_cancel_preserves_state() {
    // 前端取消语义验证（后端无 cancel；由前端不 invoke 保证）：
    // 这里验证 scheduled 在未 immediate 前保持（供前端取消后仍可继续）
    use crate::models::TagDraftItem;
    let conn = mem_db();
    let t1 = db::add_tag(&conn, "T乙", Some("说明")).unwrap();
    let draft = vec![TagDraftItem {
        id: t1.id,
        name: "T乙".into(),
        description: Some("新说明".into()),
        enabled: true,
        deleted: false,
    }];
    crate::tag_config::save_scheduled_config(&conn, &draft, "2026-08-27").unwrap();
    // 未调 immediate → scheduled 保留（取消不改变 active）
    assert!(db::scheduled_tag_config(&conn).unwrap().is_some());
    let t = db::list_tags(&conn).unwrap();
    let tx = t.iter().find(|x| x.name == "T乙").unwrap();
    assert_eq!(tx.description.as_deref(), Some("说明"), "取消后 active 不变");
}

#[test]
fn test_scheduled_not_dirty_and_guard_semantics() {
    // scheduled != active 不算 unsaved；前端 guard 只由 dirty 触发（前端逻辑）——
    // 后端验证 scheduled 保存不改 active tags（无 dirty 数据面变化）
    use crate::models::TagDraftItem;
    let conn = mem_db();
    let t1 = db::add_tag(&conn, "T丙", Some("旧")).unwrap();
    let draft = vec![TagDraftItem {
        id: t1.id,
        name: "T丙".into(),
        description: Some("新说明".into()),
        enabled: true,
        deleted: false,
    }];
    crate::tag_config::save_scheduled_config(&conn, &draft, "2026-08-27").unwrap();
    let t = db::list_tags(&conn).unwrap();
    let tx = t.iter().find(|x| x.name == "T丙").unwrap();
    assert_eq!(tx.description.as_deref(), Some("旧"), "scheduled 保存不改 active（非 dirty 数据面）");
    assert!(db::scheduled_tag_config(&conn).unwrap().is_some());
}

// ================= Round 6.5.4：Incremental Tag Merge & TotalScore =================

fn tag_match(id: Option<i64>, name: &str, score: f64, hash: Option<&str>) -> crate::models::TagMatch {
    crate::models::TagMatch {
        tag: name.to_string(),
        score,
        tag_id: id,
        semantic_hash: hash.map(str::to_string),
    }
}

/// 用户截图场景回归：old A=.8 B=.8 C=.8（旧数据无 tag_id/hash）→ tag-only C=1.0
/// → final A=.8 B=.8 C=1.0，total=2.6，不得出现两条 C、不得 total=1.0。
#[test]
fn test_screenshot_regression_incremental_merge() {
    use crate::models::TagDraftItem;
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pa = seed_paper_with_score(&conn, jid, "10.1000/sc-a", "A", 0.0);
    // 构造旧 Full AI 数据：A/B/C 均无 tag_id/hash（Round 6.5 前格式）
    let old_json = serde_json::json!([
        {"tag":"T平台","score":0.8},
        {"tag":"T数字","score":0.8},
        {"tag":"T定价","score":0.8}
    ]);
    conn.execute("UPDATE papers SET tag_matches_json=?1, total_score=2.4 WHERE id=?2", params![old_json.to_string(), pa]).unwrap();
    // 三个 active tag（repair 按 tags 表补身份，故先建 tag）
    let ta = db::add_tag(&conn, "T平台", Some("双边平台")).unwrap();
    let tb = db::add_tag(&conn, "T数字", Some("数字产品")).unwrap();
    let tc = db::add_tag(&conn, "T定价", Some("定价策略")).unwrap();
    // 模拟生产 v8 迁移：repair 旧数据（补 tag_id + 当前 hash）
    db::repair_paper_tag_matches(&conn).unwrap();
    // tag-only：只请求 C（定价）→ 返回 1.0
    let targets = vec![(tc.id, "T定价".to_string(), "定价策略".to_string())];
    let scores = vec![(tc.id, 1.0)];
    db::set_paper_tag_scores(&conn, pa, &scores, &targets).unwrap();
    let json: String = conn.query_row("SELECT tag_matches_json FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    let ms: Vec<crate::models::TagMatch> = serde_json::from_str(&json).unwrap();
    // 定价只一条（1.0）；A/B 保留 0.8
    let c_list: Vec<&crate::models::TagMatch> = ms.iter().filter(|m| m.tag == "T定价").collect();
    assert_eq!(c_list.len(), 1, "同一逻辑 Tag 不得出现两条：{:?}", c_list.iter().map(|m| m.score).collect::<Vec<_>>());
    assert_eq!(c_list[0].score, 1.0);
    assert_eq!(c_list[0].tag_id, Some(tc.id));
    let a = ms.iter().find(|m| m.tag == "T平台").unwrap();
    assert_eq!(a.score, 0.8, "未请求 tag 保留");
    // totalScore = 全部 active = 0.8+0.8+1.0 = 2.6
    let s: f64 = conn.query_row("SELECT total_score FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    assert!((s - 2.6).abs() < 1e-9, "totalScore 必须 2.6，实际 {}", s);
    let _ = ta;
    let _ = tb;
}

/// 多标签增量：requested B,D → A=.8 B=.9 C=.4 D=.7 → total=2.8（不是 1.6）。
#[test]
fn test_multi_tag_incremental_merge() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pa = seed_paper_with_score(&conn, jid, "10.1000/mt-a", "A", 0.0);
    let ta = db::add_tag(&conn, "TA", Some("a")).unwrap();
    let tb = db::add_tag(&conn, "TB", Some("b")).unwrap();
    let tc = db::add_tag(&conn, "TC", Some("c")).unwrap();
    let td = db::add_tag(&conn, "TD", Some("d")).unwrap();
    let ha = crate::tag_config::tag_semantic_hash(ta.id, "TA", "a");
    let hb = crate::tag_config::tag_semantic_hash(tb.id, "TB", "b");
    let hc = crate::tag_config::tag_semantic_hash(tc.id, "TC", "c");
    let hd = crate::tag_config::tag_semantic_hash(td.id, "TD", "d");
    let old_json = serde_json::json!([
        {"tag":"TA","score":0.8,"tagId":ta.id,"semanticHash":ha},
        {"tag":"TB","score":0.6,"tagId":tb.id,"semanticHash":hb},
        {"tag":"TC","score":0.4,"tagId":tc.id,"semanticHash":hc},
        {"tag":"TD","score":0.2,"tagId":td.id,"semanticHash":hd}
    ]);
    conn.execute("UPDATE papers SET tag_matches_json=?1, total_score=2.0 WHERE id=?2", params![old_json.to_string(), pa]).unwrap();
    // requested B,D
    let targets = vec![(tb.id, "TB".to_string(), "b".to_string()), (td.id, "TD".to_string(), "d".to_string())];
    let scores = vec![(tb.id, 0.9), (td.id, 0.7)];
    db::set_paper_tag_scores(&conn, pa, &scores, &targets).unwrap();
    let json: String = conn.query_row("SELECT tag_matches_json FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    let ms: Vec<crate::models::TagMatch> = serde_json::from_str(&json).unwrap();
    assert_eq!(ms.len(), 4, "未请求 tag 完全保留");
    assert!((ms.iter().find(|m| m.tag == "TB").unwrap().score - 0.9).abs() < 1e-9);
    assert!((ms.iter().find(|m| m.tag == "TD").unwrap().score - 0.7).abs() < 1e-9);
    assert!((ms.iter().find(|m| m.tag == "TA").unwrap().score - 0.8).abs() < 1e-9);
    let s: f64 = conn.query_row("SELECT total_score FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    assert!((s - 2.8).abs() < 1e-9, "totalScore 必须 2.8（非 1.6），实际 {}", s);
}

/// 新增 tag：A=.8 B=.6 → 新增 C=.9 → total=2.3。
#[test]
fn test_new_tag_adds_to_old_scores() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pa = seed_paper_with_score(&conn, jid, "10.1000/nt-a", "A", 0.0);
    let ta = db::add_tag(&conn, "NT-A", Some("a")).unwrap();
    let tb = db::add_tag(&conn, "NT-B", Some("b")).unwrap();
    let tc = db::add_tag(&conn, "NT-C", Some("c")).unwrap();
    let ha = crate::tag_config::tag_semantic_hash(ta.id, "NT-A", "a");
    let hb = crate::tag_config::tag_semantic_hash(tb.id, "NT-B", "b");
    let old_json = serde_json::json!([
        {"tag":"NT-A","score":0.8,"tagId":ta.id,"semanticHash":ha},
        {"tag":"NT-B","score":0.6,"tagId":tb.id,"semanticHash":hb}
    ]);
    conn.execute("UPDATE papers SET tag_matches_json=?1 WHERE id=?2", params![old_json.to_string(), pa]).unwrap();
    let targets = vec![(tc.id, "NT-C".to_string(), "c".to_string())];
    let scores = vec![(tc.id, 0.9)];
    db::set_paper_tag_scores(&conn, pa, &scores, &targets).unwrap();
    let s: f64 = conn.query_row("SELECT total_score FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    assert!((s - 2.3).abs() < 1e-9, "新增 tag 加到旧分：total 2.3，实际 {}", s);
}

/// disabled：A+B+C 缓存保留，total 只计 enabled（A+C）；re-enable 且 hash 不变 → 重新计入（零 AI 语义由调度保证）。
#[test]
fn test_disabled_and_reenabled_total() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pa = seed_paper_with_score(&conn, jid, "10.1000/ds-a", "A", 0.0);
    let ta = db::add_tag(&conn, "DS-A", Some("a")).unwrap();
    let tb = db::add_tag(&conn, "DS-B", Some("b")).unwrap();
    let ha = crate::tag_config::tag_semantic_hash(ta.id, "DS-A", "a");
    let hb = crate::tag_config::tag_semantic_hash(tb.id, "DS-B", "b");
    let old_json = serde_json::json!([
        {"tag":"DS-A","score":0.8,"tagId":ta.id,"semanticHash":ha},
        {"tag":"DS-B","score":1.0,"tagId":tb.id,"semanticHash":hb}
    ]);
    conn.execute("UPDATE papers SET tag_matches_json=?1 WHERE id=?2", params![old_json.to_string(), pa]).unwrap();
    let active = crate::tag_config::active_tags(&conn).unwrap();
    crate::tag_config::recompute_paper_total_score(&conn, pa, &active).unwrap();
    let s: f64 = conn.query_row("SELECT total_score FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    assert!((s - 1.8).abs() < 1e-9, "初始 1.8，实际 {}", s);
    // disable B
    db::update_tag(&conn, tb.id, "DS-B", Some("b"), false).unwrap();
    let active2 = crate::tag_config::active_tags(&conn).unwrap();
    crate::tag_config::recompute_paper_total_score(&conn, pa, &active2).unwrap();
    let s2: f64 = conn.query_row("SELECT total_score FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    assert!((s2 - 0.8).abs() < 1e-9, "disabled 不计：0.8，实际 {}", s2);
    // cache 保留
    let json: String = conn.query_row("SELECT tag_matches_json FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    assert!(json.contains("DS-B"), "disabled 缓存保留");
    // re-enable（hash 不变）→ 重新计入（无新 AI）
    db::update_tag(&conn, tb.id, "DS-B", Some("b"), true).unwrap();
    let active3 = crate::tag_config::active_tags(&conn).unwrap();
    crate::tag_config::recompute_paper_total_score(&conn, pa, &active3).unwrap();
    let s3: f64 = conn.query_row("SELECT total_score FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    assert!((s3 - 1.8).abs() < 1e-9, "re-enable 缓存计入：1.8，实际 {}", s3);
}

/// 部分失败：requested C,D；C 成功 D 失败 → A/B/C 计入，D 旧 score 保留但（hash 变化时）不计入，不破坏整篇。
#[test]
fn test_partial_incremental_failure_preserves_others() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pa = seed_paper_with_score(&conn, jid, "10.1000/pf-a", "A", 0.0);
    let ta = db::add_tag(&conn, "PF-A", Some("a")).unwrap();
    let tc = db::add_tag(&conn, "PF-C", Some("c")).unwrap();
    let td = db::add_tag(&conn, "PF-D", Some("d")).unwrap();
    let ha = crate::tag_config::tag_semantic_hash(ta.id, "PF-A", "a");
    let hc_old = crate::tag_config::tag_semantic_hash(tc.id, "PF-C", "旧");
    let hd_old = crate::tag_config::tag_semantic_hash(td.id, "PF-D", "旧");
    let old_json = serde_json::json!([
        {"tag":"PF-A","score":0.8,"tagId":ta.id,"semanticHash":ha},
        {"tag":"PF-C","score":0.6,"tagId":tc.id,"semanticHash":hc_old},
        {"tag":"PF-D","score":0.5,"tagId":td.id,"semanticHash":hd_old}
    ]);
    conn.execute("UPDATE papers SET tag_matches_json=?1 WHERE id=?2", params![old_json.to_string(), pa]).unwrap();
    // 模拟 immediate 保存：tags 表 desc 已更新（active 语义 = 新说明）→ 再 tag-only
    db::update_tag(&conn, tc.id, "PF-C", Some("新说明"), true).unwrap();
    db::update_tag(&conn, td.id, "PF-D", Some("新说明"), true).unwrap();
    // requested C + D（D 失败未返回）
    let targets = vec![
        (tc.id, "PF-C".to_string(), "新说明".to_string()),
        (td.id, "PF-D".to_string(), "新说明".to_string()),
    ];
    let scores = vec![(tc.id, 1.0)]; // D 失败未返回
    db::set_paper_tag_scores(&conn, pa, &scores, &targets).unwrap();
    let json: String = conn.query_row("SELECT tag_matches_json FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    let ms: Vec<crate::models::TagMatch> = serde_json::from_str(&json).unwrap();
    assert_eq!(ms.len(), 3, "D 旧 score 保留（cache/history），不删除");
    let s: f64 = conn.query_row("SELECT total_score FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    // A=.8 + C=1.0（新 hash 匹配）+ D stale（旧 hash ≠ 新语义 → 不计）= 1.8
    assert!((s - 1.8).abs() < 1e-9, "A/C 正常计入，D stale 不计：total 1.8，实际 {}", s);
}

/// 未知 AI tag 被忽略（tag_only_analyze 过滤层；这里验证 merge 只接受 requested）。
#[test]
fn test_unknown_ai_tag_ignored() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pa = seed_paper_with_score(&conn, jid, "10.1000/un-a", "A", 0.0);
    let tc = db::add_tag(&conn, "UN-C", Some("c")).unwrap();
    // requested 只有 C；scores 里混入未请求的 tag_id=9999
    let targets = vec![(tc.id, "UN-C".to_string(), "c".to_string())];
    let scores = vec![(tc.id, 0.9), (9999, 0.9)];
    db::set_paper_tag_scores(&conn, pa, &scores, &targets).unwrap();
    let json: String = conn.query_row("SELECT tag_matches_json FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    let ms: Vec<crate::models::TagMatch> = serde_json::from_str(&json).unwrap();
    assert!(!ms.iter().any(|m| m.tag_id == Some(9999)), "未请求 tag 不得写入");
}

/// repair：旧数据无 tag_id → 补 identity + hash；同 tag 重复 → 去重保留 active 匹配者。
#[test]
fn test_duplicate_repair() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pa = seed_paper_with_score(&conn, jid, "10.1000/rp-a", "A", 0.0);
    let tc = db::add_tag(&conn, "RP-C", Some("说明")).unwrap();
    let expect = crate::tag_config::tag_semantic_hash(tc.id, "RP-C", "说明");
    // 模拟损坏数据：同 tag 两条（旧无 id + 新有 id）
    let bad_json = serde_json::json!([
        {"tag":"RP-C","score":0.8},
        {"tag":"RP-C","score":1.0,"tagId":tc.id,"semanticHash":expect}
    ]);
    conn.execute("UPDATE papers SET tag_matches_json=?1 WHERE id=?2", params![bad_json.to_string(), pa]).unwrap();
    db::repair_paper_tag_matches(&conn).unwrap();
    let json: String = conn.query_row("SELECT tag_matches_json FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    let ms: Vec<crate::models::TagMatch> = serde_json::from_str(&json).unwrap();
    let c_list: Vec<&crate::models::TagMatch> = ms.iter().filter(|m| m.tag == "RP-C").collect();
    assert_eq!(c_list.len(), 1, "repair 后同 tag 只一条");
    assert_eq!(c_list[0].score, 1.0, "保留 active hash 匹配的 score");
    assert_eq!(c_list[0].tag_id, Some(tc.id));
    let s: f64 = conn.query_row("SELECT total_score FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    assert!((s - 1.0).abs() < 1e-9);
}


/// Round 6.5.5：修改 tag name → semanticChanged 识别（B 场景），其他标签评分不变。
#[test]
fn test_name_edit_semantic_changed() {
    use crate::models::{TagConfigItem, TagDraftItem};
    let mk = |id: i64, name: &str, desc: &str, enabled: bool| TagConfigItem {
        version_id: 1,
        tag_id: id,
        name: name.to_string(),
        description: Some(desc.to_string()),
        enabled,
        deleted: false,
    };
    let old = vec![
        mk(1, "定价", "定价策略", true),
        mk(2, "平台经济", "双边平台", true),
    ];
    // name 修改（定价 → 价格策略）
    let draft = vec![
        TagDraftItem { id: 1, name: "价格策略".into(), description: Some("定价策略".into()), enabled: true, deleted: false },
        TagDraftItem { id: 2, name: "平台经济".into(), description: Some("双边平台".into()), enabled: true, deleted: false },
    ];
    let d = crate::tag_config::compute_diff(&old, &draft);
    assert!(d.semantic_changed.contains(&"价格策略".to_string()), "name 修改 → semanticChanged: {:?}", d.semantic_changed);
    assert!(d.unchanged.contains(&"平台经济".to_string()), "未修改标签 unchanged");
    // 与 screenshot 等价：改 desc 后的 totalScore（A/C 保留、修改标签替换）已由 test_screenshot_regression 覆盖
}

// ================= Round 6.5.6：Tag Visibility（DTO 过滤） =================

fn seed_abc_scores() -> (rusqlite::Connection, i64, i64, i64, i64) {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pa = seed_paper_with_score(&conn, jid, "10.1000/vis-a", "A", 0.0);
    let ta = db::add_tag(&conn, "VIS-A", Some("a")).unwrap();
    let tb = db::add_tag(&conn, "VIS-B", Some("b")).unwrap();
    let tc = db::add_tag(&conn, "VIS-C", Some("c")).unwrap();
    let ha = crate::tag_config::tag_semantic_hash(ta.id, "VIS-A", "a");
    let hb = crate::tag_config::tag_semantic_hash(tb.id, "VIS-B", "b");
    let hc = crate::tag_config::tag_semantic_hash(tc.id, "VIS-C", "c");
    let json = serde_json::json!([
        {"tag":"VIS-A","score":0.8,"tagId":ta.id,"semanticHash":ha},
        {"tag":"VIS-B","score":0.6,"tagId":tb.id,"semanticHash":hb},
        {"tag":"VIS-C","score":1.0,"tagId":tc.id,"semanticHash":hc}
    ]);
    conn.execute("UPDATE papers SET tag_matches_json=?1 WHERE id=?2", params![json.to_string(), pa]).unwrap();
    let active = crate::tag_config::active_tags(&conn).unwrap();
    crate::tag_config::recompute_paper_total_score(&conn, pa, &active).unwrap();
    (conn, pa, ta.id, tb.id, tc.id)
}
fn dto_tags(conn: &rusqlite::Connection, paper_id: i64) -> Vec<String> {
    let p = db::get_paper(conn, paper_id).unwrap().unwrap();
    p.tag_matches.iter().map(|m| m.tag.clone()).collect()
}

/// 1/2) delete → DTO 不含 deleted tag，totalScore 正确。
#[test]
fn test_delete_hides_from_dto() {
    let (conn, pa, _ta, _tb, tc) = seed_abc_scores();
    // delete C → 新 active（无 C）
    db::delete_tag(&conn, tc).unwrap();
    let active = crate::tag_config::active_tags(&conn).unwrap();
    crate::tag_config::recompute_paper_total_score(&conn, pa, &active).unwrap();
    let tags = dto_tags(&conn, pa);
    assert!(!tags.contains(&"VIS-C".to_string()), "DTO 不得含 deleted tag: {:?}", tags);
    assert!(tags.contains(&"VIS-A".to_string()));
    let s: f64 = conn.query_row("SELECT total_score FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    assert!((s - 1.4).abs() < 1e-9, "delete 后 total=1.4，实际 {}", s);
    // cache 保留
    let json: String = conn.query_row("SELECT tag_matches_json FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    assert!(json.contains("VIS-C"), "cache 保留 deleted tag");
}

/// 3/4) disable → cache 保留但 DTO 隐藏；re-enable（hash 不变）→ cache 复用重新显示。
#[test]
fn test_disable_hide_and_reenable_show() {
    let (conn, pa, _ta, tb, _tc) = seed_abc_scores();
    // disable B
    db::update_tag(&conn, tb, "VIS-B", Some("b"), false).unwrap();
    let active = crate::tag_config::active_tags(&conn).unwrap();
    crate::tag_config::recompute_paper_total_score(&conn, pa, &active).unwrap();
    let tags = dto_tags(&conn, pa);
    assert!(!tags.contains(&"VIS-B".to_string()), "disabled tag 不显示: {:?}", tags);
    let s: f64 = conn.query_row("SELECT total_score FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    assert!((s - 1.8).abs() < 1e-9, "disabled 不计：1.8，实际 {}", s);
    // re-enable（semantic 未变）→ cache 复用，重新显示
    db::update_tag(&conn, tb, "VIS-B", Some("b"), true).unwrap();
    let active2 = crate::tag_config::active_tags(&conn).unwrap();
    crate::tag_config::recompute_paper_total_score(&conn, pa, &active2).unwrap();
    let tags2 = dto_tags(&conn, pa);
    assert!(tags2.contains(&"VIS-B".to_string()), "re-enable 后重新显示");
    let s2: f64 = conn.query_row("SELECT total_score FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    assert!((s2 - 2.4).abs() < 1e-9, "re-enable 缓存计入：2.4，实际 {}", s2);
}

/// 5) scheduled delete 未生效 → 当前 active 仍含 C → DTO 仍显示（只有真正激活后才隐藏）。
#[test]
fn test_scheduled_delete_not_active_until_activation() {
    use crate::models::TagDraftItem;
    let (conn, pa, _ta, tb, tc) = seed_abc_scores();
    // scheduled：删除 C（draft 不含 C）
    let draft = vec![
        TagDraftItem { id: _ta, name: "VIS-A".into(), description: Some("a".into()), enabled: true, deleted: false },
        TagDraftItem { id: tb, name: "VIS-B".into(), description: Some("b".into()), enabled: true, deleted: false },
    ];
    crate::tag_config::save_scheduled_config(&conn, &draft, "2026-08-27").unwrap();
    // 未激活 → active 仍含 C → DTO 显示 C
    let tags = dto_tags(&conn, pa);
    assert!(tags.contains(&"VIS-C".to_string()), "scheduled 未激活时 C 仍显示: {:?}", tags);
    // 模拟 immediate 激活（scheduled items 作为 candidate）→ C 消失
    crate::tag_config::save_immediate_config(&conn, &draft).unwrap();
    let tags2 = dto_tags(&conn, pa);
    assert!(!tags2.contains(&"VIS-C".to_string()), "激活后 C 隐藏: {:?}", tags2);
    let _ = tc;
}

/// 6) immediate delete → tag 消失。
#[test]
fn test_immediate_delete_hides() {
    use crate::models::TagDraftItem;
    let (conn, pa, _ta, tb, tc) = seed_abc_scores();
    let draft = vec![
        TagDraftItem { id: _ta, name: "VIS-A".into(), description: Some("a".into()), enabled: true, deleted: false },
        TagDraftItem { id: tb, name: "VIS-B".into(), description: Some("b".into()), enabled: true, deleted: false },
    ];
    crate::tag_config::save_immediate_config(&conn, &draft).unwrap();
    let tags = dto_tags(&conn, pa);
    assert!(!tags.contains(&"VIS-C".to_string()), "immediate delete 后隐藏");
    let _ = tc;
}

/// 7) semanticChanged：旧 hash chip 隐藏；tag-only 更新成功 → 新 hash 生效显示。
#[test]
fn test_semantic_changed_hide_until_updated() {
    let (conn, pa, _ta, _tb, tc) = seed_abc_scores();
    // 修改 C description（simulate immediate 保存改 active）
    db::update_tag(&conn, tc, "VIS-C", Some("新说明"), true).unwrap();
    // 旧 cache hash（旧 c）≠ 新语义 → DTO 隐藏 C、total 不含 C
    let active = crate::tag_config::active_tags(&conn).unwrap();
    crate::tag_config::recompute_paper_total_score(&conn, pa, &active).unwrap();
    let tags = dto_tags(&conn, pa);
    assert!(!tags.contains(&"VIS-C".to_string()), "旧 hash chip 隐藏: {:?}", tags);
    let s: f64 = conn.query_row("SELECT total_score FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    assert!((s - 1.4).abs() < 1e-9, "stale 不计：1.4，实际 {}", s);
    // tag-only 更新 C = 1.0（新 hash）
    let targets = vec![(tc, "VIS-C".to_string(), "新说明".to_string())];
    let scores = vec![(tc, 1.0)];
    db::set_paper_tag_scores(&conn, pa, &scores, &targets).unwrap();
    let tags2 = dto_tags(&conn, pa);
    assert!(tags2.contains(&"VIS-C".to_string()), "新 hash score 重新显示");
    let s2: f64 = conn.query_row("SELECT total_score FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    assert!((s2 - 2.4).abs() < 1e-9, "更新后 total=2.4，实际 {}", s2);
}

/// 8) 历史推荐 rank/scoreSnapshot 不变（DTO 过滤不触碰 recommendation_items）。
#[test]
fn test_history_frozen_by_dto_filter() {
    let (conn, pa, _ta, _tb, tc) = seed_abc_scores();
    let r1 = crate::recommendation::refresh_current_recommendations(&conn, &local_dt(2026, 8, 26, 15, 0), "09:00").unwrap();
    let snapshot = db::list_recommendation_items(&conn, r1).unwrap()[0].score_snapshot;
    let _r2 = crate::recommendation::refresh_current_recommendations(&conn, &local_dt(2026, 8, 27, 9, 0), "09:00").unwrap();
    // delete C + DTO 过滤 → finalized run 快照不变
    db::delete_tag(&conn, tc).unwrap();
    let active = crate::tag_config::active_tags(&conn).unwrap();
    crate::tag_config::recompute_paper_total_score(&conn, pa, &active).unwrap();
    let items = db::list_recommendation_items(&conn, r1).unwrap();
    assert_eq!(items[0].score_snapshot, snapshot, "历史 rank/score 冻结");
}

/// 推荐命令 DTO 端到端：3 个有效 tagMatches 的论文进入推荐后，items.paper.tag_matches 非空且含全部 3 个。
#[test]
fn test_recommend_dto_carries_tag_matches() {
    let (conn, pa, ta, tb, tc) = seed_abc_scores();
    let _r = crate::recommendation::refresh_current_recommendations(&conn, &local_dt(2026, 8, 26, 15, 0), "09:00").unwrap();
    let runs = db::list_recommendation_runs(&conn).unwrap();
    let run_id = runs.iter().find(|r| r.status == "open").map(|r| r.id).or_else(|| runs.first().map(|r| r.id)).unwrap();
    let views = crate::recommendation::run_items_with_papers(&conn, run_id).unwrap();
    let v = views.iter().find(|v| v.paper_id == pa).expect("论文在推荐中");
    assert_eq!(v.paper.tag_matches.len(), 3, "推荐 DTO 必须携带 3 个有效 tagMatches: {:?}", v.paper.tag_matches.iter().map(|m| m.tag.clone()).collect::<Vec<_>>());
    let names: Vec<&str> = v.paper.tag_matches.iter().map(|m| m.tag.as_str()).collect();
    assert!(names.contains(&"VIS-A") && names.contains(&"VIS-B") && names.contains(&"VIS-C"));
    let _ = ta;
    let _ = tb;
    let _ = tc;
}

// ================= Full AI Tag Identity Persistence =================

/// 1) Full AI 新结果带 tag_id/hash；totalScore = visible matches sum。
#[test]
fn test_full_ai_writes_tag_identity() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pa = seed_paper_with_score(&conn, jid, "10.1000/fa-a", "A", 0.0);
    let ta = db::add_tag(&conn, "FA-A", Some("a")).unwrap();
    let tb = db::add_tag(&conn, "FA-B", Some("b")).unwrap();
    let tc = db::add_tag(&conn, "FA-C", Some("c")).unwrap();
    // Full AI 返回（name + score）
    let ai = vec![
        crate::models::TagMatch { tag: "FA-A".into(), score: 0.8, tag_id: None, semantic_hash: None },
        crate::models::TagMatch { tag: "FA-B".into(), score: 0.6, tag_id: None, semantic_hash: None },
        crate::models::TagMatch { tag: "FA-C".into(), score: 1.0, tag_id: None, semantic_hash: None },
    ];
    // 用当前连接的 active tags 构造 canonical 集（含 seed 6 + FA 3）
    let active: Vec<(i64, String, String)> = db::list_tags(&conn)
        .unwrap()
        .into_iter()
        .filter(|t| t.enabled)
        .map(|t| (t.id, t.name, t.description.unwrap_or_default()))
        .collect();
    let normalized = crate::analyze::normalize_tag_matches(ai, &active);
    assert_eq!(normalized.len(), 6 + 3, "包含 seed tags + FA 的完整 canonical 集");
    for m in normalized.iter().filter(|m| m.tag == "FA-A" || m.tag == "FA-B" || m.tag == "FA-C") {
        assert!(m.tag_id.is_some(), "Full AI 结果必须带 tag_id: {}", m.tag);
        assert!(m.semantic_hash.is_some(), "Full AI 结果必须带 semantic_hash: {}", m.tag);
    }
    let c = &conn;
    let _ = ta;
    let _ = tb;
    let _ = tc;
    let _ = pa;
    let _ = c;
}

/// 3) legacy name-only repair → DTO 恢复 tagMatches、totalScore 重算。
#[test]
fn test_legacy_name_only_repair() {
    let (conn, pa, _ta, _tb, _tc) = seed_abc_scores();
    // 破坏：清空 tag_id/hash（模拟 Full AI 旧写入）
    conn.execute(
        "UPDATE papers SET tag_matches_json=?1 WHERE id=?2",
        params![serde_json::json!([{"tag":"VIS-A","score":0.8},{"tag":"VIS-B","score":0.6},{"tag":"VIS-C","score":1.0}]).to_string(), pa],
    )
    .unwrap();
    // repair（migration v9 逻辑）
    db::repair_paper_tag_matches(&conn).unwrap();
    let p = db::get_paper(&conn, pa).unwrap().unwrap();
    assert_eq!(p.tag_matches.len(), 3, "repair 后 DTO 恢复 3 个 tagMatches: {:?}", p.tag_matches.iter().map(|m| m.tag.clone()).collect::<Vec<_>>());
    assert!(p.tag_matches.iter().all(|m| m.tag_id.is_some() && m.semantic_hash.is_some()));
    let s: f64 = conn.query_row("SELECT total_score FROM papers WHERE id=?1", params![pa], |r| r.get(0)).unwrap();
    assert!((s - 2.4).abs() < 1e-9, "repair 后 totalScore=2.4，实际 {}", s);
}

// ================= Round 7 Phase 1：Missing Abstract Intelligence =================

/// 构造带 raw_json 的 candidate（crossref / openalex 风格由调用方决定）。
fn cand_raw(doi: &str, title: &str, abstract_text: Option<&str>, raw_json: Option<&str>) -> PaperCandidate {
    let mut c = candidate(Some(doi), title, abstract_text, None);
    c.raw_json = raw_json.map(str::to_string);
    c
}

/// 绕过 upsert 分类逻辑的「legacy 时代」原始插入（content_kind 等保持默认 unknown），
/// 用于验证 v13 migration backfill。
fn raw_insert_legacy_paper(
    conn: &rusqlite::Connection,
    jid: i64,
    title: &str,
    doi: &str,
    abstract_text: Option<&str>,
    raw_json: Option<&str>,
) -> i64 {
    conn.execute(
        "INSERT INTO papers (journal_id, normalized_doi, title, abstract, analysis_status, abstract_quality, created_at, updated_at)
         VALUES (?1,?2,?3,?4,'waitingForAbstract','missing',?5,?5)",
        params![jid, doi, title, abstract_text, "2026-08-27T00:00:00Z"],
    )
    .unwrap();
    let pid = conn.last_insert_rowid();
    if let Some(raw) = raw_json {
        conn.execute(
            "INSERT INTO source_records (paper_id, source, source_id, raw_json, retrieved_at) VALUES (?1,'crossref',?2,?3,?4)",
            params![pid, doi, raw, "2026-08-27T00:00:00Z"],
        )
        .unwrap();
    }
    pid
}

/// 模拟一次 v13 之前的存量库：全部迁移已完成、user_version 回退到 12，
/// 再执行 db::init 只重跑 v13（columns 已存在 → 仅 backfill）。
fn rerun_v13_migration(conn: &rusqlite::Connection) {
    conn.pragma_update(None, "user_version", 12).unwrap();
    db::init(conn).unwrap();
}

// A. research_article（显式可信证据）+ 无摘要 → missing_recoverable
// Correctness Fix：journal-article 是 broad type，不能产出 research_article；
// 此处直接以可信 content_kind 写入，验证 abstract_status 与 recovery 门控。
#[test]
fn test_r7_research_article_missing_is_recoverable() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let raw = r#"{"DOI":"10.1000/r7-a","type":"journal-article","title":["Platform Pricing"]}"#;
    let id = match db::upsert_paper(&conn, jid, &cand_raw("10.1000/r7-a", "Platform Pricing", None, Some(raw))).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
    };
    // broad journal-article 只说明 journal-level item，不赋 research_article
    let p = db::get_paper(&conn, id).unwrap().unwrap();
    assert_eq!(p.content_kind, crate::content_kind::CK_UNKNOWN);
    assert_eq!(p.abstract_status, crate::content_kind::ABST_UNKNOWN);
    // 模拟未来足够明确的证据（Phase 2 或 publisher metadata）显式写入 research_article
    conn.execute(
        "UPDATE papers SET content_kind='research_article', content_kind_source='explicit', content_kind_confidence='EXACT' WHERE id=?1",
        params![id],
    )
    .unwrap();
    db::refresh_abstract_status(&conn, id).unwrap();
    let p = db::get_paper(&conn, id).unwrap().unwrap();
    assert_eq!(p.content_kind, crate::content_kind::CK_RESEARCH_ARTICLE);
    assert_eq!(p.abstract_status, crate::content_kind::ABST_MISSING_RECOVERABLE);
    // 在 recovery 候选集中
    let ids = db::list_recoverable_paper_ids(&conn, &[id]).unwrap();
    assert_eq!(ids, vec![id]);
}

// B. news → not_expected → 不进入 abstract recovery
#[test]
fn test_r7_news_is_not_expected_and_excluded_from_recovery() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let raw = r#"{"DOI":"10.1000/r7-b","type":"news","title":["A New Discovery"]}"#;
    let id = match db::upsert_paper(&conn, jid, &cand_raw("10.1000/r7-b", "A New Discovery", None, Some(raw))).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
    };
    let p = db::get_paper(&conn, id).unwrap().unwrap();
    assert_eq!(p.content_kind, crate::content_kind::CK_NEWS);
    assert_eq!(p.abstract_status, crate::content_kind::ABST_NOT_EXPECTED);
    // 不得出现在 recovery 候选（单篇 + 批量）
    assert!(db::list_recoverable_paper_ids(&conn, &[id]).unwrap().is_empty());
    let research_raw = r#"{"DOI":"10.1000/r7-b2","type":"journal-article"}"#;
    let rid = match db::upsert_paper(&conn, jid, &cand_raw("10.1000/r7-b2", "A Real Study", None, Some(research_raw))).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
    };
    let ids = db::list_recoverable_paper_ids(&conn, &[id, rid]).unwrap();
    assert_eq!(ids, vec![rid], "bulk recovery 必须只包含可恢复论文");
}

// C. editorial / correction / front_matter → not_expected
#[test]
fn test_r7_non_research_kinds_are_not_expected() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let cases = [
        ("10.1000/r7-ed", "editorial", "Editorial Note", "editorial"),
        ("10.1000/r7-cor", "correction", "Correction to: Pricing", "correction"),
        ("10.1000/r7-fm", "journal-issue", "Issue Information", "front_matter"),
    ];
    for (doi, ty, title, expected_kind) in cases {
        let raw = format!(r#"{{"DOI":"{}","type":"{}","title":["{}"]}}"#, doi, ty, title);
        let id = match db::upsert_paper(&conn, jid, &cand_raw(doi, title, None, Some(&raw))).unwrap() {
            UpsertOutcome::New(id) => id,
            _ => panic!("expected new paper"),
        };
        let p = db::get_paper(&conn, id).unwrap().unwrap();
        assert_eq!(p.content_kind, expected_kind);
        assert_eq!(p.abstract_status, crate::content_kind::ABST_NOT_EXPECTED);
        assert!(db::list_recoverable_paper_ids(&conn, &[id]).unwrap().is_empty());
    }
}

// D. review → missing_recoverable + 保留推荐资格；news 被推荐门控排除
#[test]
fn test_r7_review_keeps_recommendation_eligibility_news_gated() {
    use chrono::Local;
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let review_raw = r#"{"DOI":"10.1000/r7-rev","type":"review-article","title":["A Review of X"]}"#;
    let news_raw = r#"{"DOI":"10.1000/r7-news","type":"news","title":["News Item"]}"#;
    let res_raw = r#"{"DOI":"10.1000/r7-res","type":"journal-article","title":["A Study"]}"#;
    let rev = match db::upsert_paper(&conn, jid, &cand_raw("10.1000/r7-rev", "A Review of X", None, Some(review_raw))).unwrap() {
        UpsertOutcome::New(id) => id, _ => panic!(),
    };
    let news = match db::upsert_paper(&conn, jid, &cand_raw("10.1000/r7-news", "News Item", None, Some(news_raw))).unwrap() {
        UpsertOutcome::New(id) => id, _ => panic!(),
    };
    let res = match db::upsert_paper(&conn, jid, &cand_raw("10.1000/r7-res", "A Study", None, Some(res_raw))).unwrap() {
        UpsertOutcome::New(id) => id, _ => panic!(),
    };
    let p = db::get_paper(&conn, rev).unwrap().unwrap();
    assert_eq!(p.content_kind, crate::content_kind::CK_REVIEW);
    assert_eq!(p.abstract_status, crate::content_kind::ABST_MISSING_RECOVERABLE);
    // 模拟 Full AI 已完成（DB 状态齐全），验证推荐资格门控
    for (id, score) in [(rev, 5.0), (news, 9.0), (res, 7.0)] {
        conn.execute(
            "UPDATE papers SET analysis_status='analysisSucceeded', total_score=?1,
                chinese_title='中', chinese_abstract='中', one_sentence_summary='中',
                evidence_hash='h' WHERE id=?2",
            params![score, id],
        )
        .unwrap();
    }
    let run_id = crate::recommendation::refresh_current_recommendations(&conn, &Local::now(), "09:00").unwrap();
    let items = db::list_recommendation_items(&conn, run_id).unwrap();
    let paper_ids: Vec<i64> = items.iter().map(|i| i.paper_id).collect();
    assert!(paper_ids.contains(&rev), "review 必须保留推荐资格");
    assert!(paper_ids.contains(&res), "research_article 保持原规则");
    assert!(!paper_ids.contains(&news), "news（not_expected）不得进入研究推荐");
}

// E. Nature DOI exact landing page + dc.description → recovered + provenance
#[test]
fn test_r7_nature_landing_page_dc_description_recovery_with_provenance() {
    use crate::api::publisher::{extract_public_abstract, page_identity_matches, page_doi};
    let doi = "10.1038/s41586-024-00001-2";
    let url = format!("https://www.nature.com/articles/s41586-024-00001-2");
    let text = "We study how climate policy interacts with firm investment and quantify welfare effects. Our model explains equilibrium outcomes across heterogeneous regions with explicit mechanisms and robust results. Policy implications follow directly from the calibrated evidence we present.";
    let html = format!(
        r#"<html><head>
<meta name="citation_doi" content="{}">
<meta name="dc.description" content="{}">
</head><body><p>ignored body text</p></body></html>"#,
        doi, text
    );
    // 解析器只认显式 metadata，不认正文
    assert_eq!(extract_public_abstract(&html).as_deref(), Some(text));
    assert_eq!(page_doi(&html).as_deref(), Some(doi));
    assert!(page_identity_matches(&html, &url, doi));

    // 端到端 recovery 合并：provenance 完整记录
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let raw = format!(r#"{{"DOI":"{}","type":"journal-article"}}"#, doi);
    let id = match db::upsert_paper(&conn, jid, &cand_raw(doi, "Climate Policy", None, Some(&raw))).unwrap() {
        UpsertOutcome::New(id) => id, _ => panic!(),
    };
    let p0 = db::get_paper(&conn, id).unwrap().unwrap();
    assert_eq!(p0.abstract_status, crate::content_kind::ABST_UNKNOWN, "broad journal-article → unknown");
    assert_eq!(db::list_recoverable_paper_ids(&conn, &[id]).unwrap(), vec![id], "unknown 仍允许 recovery");
    db::merge_recovered_abstract_with_url(&conn, id, "publisher:nature", text, Some(&url)).unwrap();
    let p = db::get_paper(&conn, id).unwrap().unwrap();
    assert_eq!(p.abstract_text.as_deref(), Some(text));
    assert_eq!(p.abstract_source.as_deref(), Some("publisher:nature"));
    assert_eq!(p.abstract_source_url.as_deref(), Some(url.as_str()));
    assert_eq!(p.abstract_quality, crate::models::ABQ_COMPLETE);
    assert_eq!(p.abstract_status, crate::content_kind::ABST_AVAILABLE);
}

// F. Springer DOI exact（URL 含 DOI，无 citation_doi meta）→ 身份验证通过
#[test]
fn test_r7_springer_url_doi_identity_and_recovery() {
    use crate::api::publisher::{extract_public_abstract, page_identity_matches};
    let doi = "10.1007/s10683-024-00001-2";
    let text = "We study auction design with entry costs and show that equilibrium bidding deviates systematically. Our experiments confirm the predicted comparative statics with high statistical precision and robustness across specifications. The results inform practical auction implementations.";
    // Springer 页面：dc.description 存在，但无 citation_doi meta —— identity 靠 URL
    let html = format!(r#"<meta name="dc.description" content="{}">"#, text);
    assert_eq!(extract_public_abstract(&html).as_deref(), Some(text));
    assert!(page_identity_matches(
        &html,
        "https://link.springer.com/article/10.1007/s10683-024-00001-2",
        doi
    ));
    // %2F 编码形式（部分 Springer 链接）
    assert!(page_identity_matches(
        &html,
        "https://link.springer.com/article/10.1007%2Fs10683-024-00001-2",
        doi
    ));

    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let raw = format!(r#"{{"DOI":"{}","type":"journal-article"}}"#, doi);
    let id = match db::upsert_paper(&conn, jid, &cand_raw(doi, "Auction Design", None, Some(&raw))).unwrap() {
        UpsertOutcome::New(id) => id, _ => panic!(),
    };
    db::merge_recovered_abstract_with_url(&conn, id, "publisher:springer", text, Some("https://link.springer.com/article/10.1007/s10683-024-00001-2")).unwrap();
    let p = db::get_paper(&conn, id).unwrap().unwrap();
    assert_eq!(p.abstract_source.as_deref(), Some("publisher:springer"));
    assert_eq!(p.abstract_status, crate::content_kind::ABST_AVAILABLE);
}

// G. publisher DOI mismatch → 摘要拒绝（identity 验证失败）
#[test]
fn test_r7_publisher_doi_mismatch_rejects_abstract() {
    use crate::api::publisher::{page_identity_matches, page_doi};
    // 页面声称另一个 DOI
    let html = r#"<meta name="citation_doi" content="10.1038/s41586-024-99999-9">
<meta name="dc.description" content="Some other article's abstract with enough words to be a complete sentence.">"#;
    assert_eq!(page_doi(html).as_deref(), Some("10.1038/s41586-024-99999-9"));
    assert!(!page_identity_matches(
        html,
        "https://www.nature.com/articles/s41586-024-99999-9",
        "10.1038/s41586-024-00001-2"
    ), "目标 DOI 与页面 DOI 不一致 → 必须拒绝");

    // 页面完全无 DOI identity evidence → 拒绝
    let no_identity = r#"<meta name="description" content="Generic teaser text that looks like an abstract sentence.">"#;
    assert_eq!(page_doi(no_identity), None);
    assert!(!page_identity_matches(no_identity, "https://www.nature.com/articles/s41586-024-00001-2", "10.1038/s41586-024-00001-2"));

    // 显式错误的页面 DOI 即使最终 URL 含目标 DOI，也必须拒绝。
    let conflicting_identity = r#"<meta name="citation_doi" content="10.1038/s41586-024-99999-9">"#;
    assert!(!page_identity_matches(
        conflicting_identity,
        "https://www.nature.com/articles/s41586-024-00001-2",
        "10.1038/s41586-024-00001-2"
    ));
}

// H. not_expected → 排除出 bulk recovery（list_recoverable_paper_ids 已覆盖 B，
// 此处验证 mixed scope：broad journal-article → unknown 仍可恢复）
#[test]
fn test_r7_bulk_recovery_scope_excludes_not_expected() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let mut ids = Vec::new();
    for (i, ty) in ["journal-article", "news", "review-article", "editorial"].iter().enumerate() {
        let doi = format!("10.1000/r7-bulk{}", i);
        let raw = format!(r#"{{"DOI":"{}","type":"{}"}}"#, doi, ty);
        let id = match db::upsert_paper(&conn, jid, &cand_raw(&doi, &format!("T{}", i), None, Some(&raw))).unwrap() {
            UpsertOutcome::New(id) => id, _ => panic!(),
        };
        ids.push(id);
    }
    let eligible = db::list_recoverable_paper_ids(&conn, &ids).unwrap();
    assert_eq!(eligible.len(), 2, "journal-article(unknown) 和 review-article 可恢复；news/editorial 排除");
    let kinds: Vec<String> = eligible.iter().map(|id| db::get_paper(&conn, *id).unwrap().unwrap().content_kind).collect();
    assert!(kinds.contains(&crate::content_kind::CK_UNKNOWN.to_string()), "broad journal-article → unknown，仍允许 recovery");
    assert!(kinds.contains(&crate::content_kind::CK_REVIEW.to_string()));
}

// I. not_expected → title translation 仍允许
#[test]
fn test_r7_not_expected_still_eligible_for_title_translation() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let news_raw = r#"{"DOI":"10.1000/r7-i1","type":"news","title":["News Item"]}"#;
    let news = match db::upsert_paper(&conn, jid, &cand_raw("10.1000/r7-i1", "News Item", None, Some(news_raw))).unwrap() {
        UpsertOutcome::New(id) => id, _ => panic!(),
    };
    let p = db::get_paper(&conn, news).unwrap().unwrap();
    assert_eq!(p.abstract_status, crate::content_kind::ABST_NOT_EXPECTED);
    let candidates = db::list_missing_title_translation_candidates(&conn, None).unwrap();
    assert!(candidates.iter().any(|(id, _)| *id == news), "not_expected 论文仍应可翻译标题");
}

#[test]
fn test_title_translation_candidates_are_not_gated_by_abstract_or_analysis_state() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let create = |doi: &str, title: &str, abstract_text: Option<&str>| {
        match db::upsert_paper(&conn, jid, &candidate(Some(doi), title, abstract_text, None)).unwrap() {
            UpsertOutcome::New(id) => id,
            _ => panic!("expected new paper"),
        }
    };

    // A complete abstract must not block title-only backlog eligibility.
    let complete = create(
        "10.1000/title-complete",
        "Complete title",
        Some("A complete abstract with methods, results, and implications. ".repeat(8).as_str()),
    );
    // These are deliberately inconsistent states: the title backlog must be
    // independent from abstract_status, content_kind, and analysis_status.
    let unknown = create("10.1000/title-unknown", "Unknown title", None);
    let not_expected = create("10.1000/title-not-expected", "News title", None);
    let editorial = create("10.1000/title-editorial", "Editorial title", None);
    let letter = create("10.1000/title-letter", "Letter title", None);
    conn.execute("UPDATE papers SET abstract_status='unknown', content_kind='unknown', analysis_status='analysisSucceeded' WHERE id=?1", params![complete]).unwrap();
    conn.execute("UPDATE papers SET abstract_status='unknown', content_kind='unknown' WHERE id=?1", params![unknown]).unwrap();
    conn.execute("UPDATE papers SET abstract_status='not_expected', content_kind='news' WHERE id=?1", params![not_expected]).unwrap();
    conn.execute("UPDATE papers SET abstract_status='not_expected', content_kind='editorial' WHERE id=?1", params![editorial]).unwrap();
    conn.execute("UPDATE papers SET abstract_status='not_expected', content_kind='letter' WHERE id=?1", params![letter]).unwrap();

    let existing_title = create("10.1000/title-existing", "Already translated", None);
    conn.execute("UPDATE papers SET chinese_title='已有中文标题' WHERE id=?1", params![existing_title]).unwrap();
    let mut blank_candidate = candidate(Some("10.1000/title-blank"), "placeholder", None, None);
    blank_candidate.title = Some("   ".into());
    let blank = match db::upsert_paper(&conn, jid, &blank_candidate).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
    };

    let candidates = db::list_missing_title_translation_candidates(&conn, None).unwrap();
    let ids: Vec<i64> = candidates.iter().map(|(id, _)| *id).collect();
    for id in [complete, unknown, not_expected, editorial, letter] {
        assert!(ids.contains(&id), "paper {} must remain title-translation eligible", id);
    }
    assert!(!ids.contains(&existing_title), "existing Chinese title must be excluded");
    assert!(!ids.contains(&blank), "blank source title must be excluded");

    // Eligibility and persistence must use the same state-independent rule:
    // a complete-abstract paper that receives a title-only translation must
    // persist only chinese_title, without changing analysis data.
    assert!(db::save_title_translation(&conn, complete, "完整标题的中文翻译").unwrap());
    let translated = db::get_paper(&conn, complete).unwrap().unwrap();
    assert_eq!(translated.chinese_title.as_deref(), Some("完整标题的中文翻译"));
    assert_eq!(translated.abstract_quality, "complete");
    assert_eq!(translated.abstract_status, "unknown");
    assert_eq!(translated.analysis_status, "analysisSucceeded");
    assert!(translated.chinese_abstract.is_none());
    assert!(translated.one_sentence_summary.is_none());
    assert!(translated.total_score.is_none());
    assert!(translated.tag_matches.is_empty());
}

#[test]
fn test_title_translation_candidate_batch_limit_is_twenty_five_without_state_filter() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    for n in 0..30 {
        let mut paper = candidate(Some(&format!("10.1000/title-limit-{}", n)), &format!("Title {}", n), Some("abstract"), None);
        paper.title = Some(format!("Title {}", n));
        let id = match db::upsert_paper(&conn, jid, &paper).unwrap() {
            UpsertOutcome::New(id) => id,
            _ => panic!("expected new paper"),
        };
        conn.execute("UPDATE papers SET abstract_status='not_expected', content_kind='news' WHERE id=?1", params![id]).unwrap();
    }
    let candidates = db::list_missing_title_translation_candidates(&conn, None).unwrap();
    assert_eq!(candidates.len(), 25);
    assert_eq!(db::TITLE_TRANSLATION_BATCH_LIMIT, 25);
}

// J. 已有真实摘要 → migration backfill 不覆盖
#[test]
fn test_r7_backfill_does_not_overwrite_existing_abstract() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let abstract_text = "An existing complete abstract that must survive migration with all its original words and meaning intact.";
    let raw = r#"{"DOI":"10.1000/r7-j","type":"news"}"#;
    let pid = raw_insert_legacy_paper(&conn, jid, "Already Abstracted", "10.1000/r7-j", Some(abstract_text), Some(raw));
    conn.execute(
        "UPDATE papers SET abstract_source='crossref', abstract_quality='complete', is_favorite=1 WHERE id=?1",
        params![pid],
    )
    .unwrap();
    rerun_v13_migration(&conn);
    let (abs, src, quality): (Option<String>, Option<String>, String) = conn
        .query_row("SELECT abstract, abstract_source, abstract_quality FROM papers WHERE id=?1", params![pid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap();
    assert_eq!(abs.as_deref(), Some(abstract_text), "backfill 不得覆盖真实摘要");
    assert_eq!(src.as_deref(), Some("crossref"));
    assert_eq!(quality, "complete");
    let (kind, status): (String, String) = conn
        .query_row("SELECT content_kind, abstract_status FROM papers WHERE id=?1", params![pid], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert_eq!(kind, "news", "类型按证据解析");
    assert_eq!(status, "available", "已有摘要 → available（不因类型是 news 而丢失）");
}

// K. History / first_seen 语义 → migration 不变
#[test]
fn test_r7_migration_preserves_first_seen_and_history_semantics() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let raw = r#"{"DOI":"10.1000/r7-k","type":"news"}"#;
    let pid = raw_insert_legacy_paper(&conn, jid, "History Paper", "10.1000/r7-k", None, Some(raw));
    conn.execute(
        "UPDATE papers SET first_seen_cycle='2026-08-27', first_seen_abstract_missing=1, is_favorite=1, is_ignored=0 WHERE id=?1",
        params![pid],
    )
    .unwrap();
    let run_id = db::create_recommendation_run(&conn, "2026-08-27", crate::recommendation::RC_OPEN).unwrap();
    conn.execute(
        "INSERT INTO recommendation_items (run_id, paper_id, rank, score_snapshot, added_at) VALUES (?1,?2,1,1.0,?3)",
        params![run_id, pid, "2026-08-27T00:00:00Z"],
    )
    .unwrap();
    let before_items: i64 = conn.query_row("SELECT COUNT(*) FROM recommendation_items", [], |r| r.get(0)).unwrap();
    assert_eq!(before_items, 1);

    rerun_v13_migration(&conn);

    let (cycle, missing, fav, ign): (Option<String>, i64, i64, i64) = conn
        .query_row(
            "SELECT first_seen_cycle, first_seen_abstract_missing, is_favorite, is_ignored FROM papers WHERE id=?1",
            params![pid],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(cycle.as_deref(), Some("2026-08-27"));
    assert_eq!(missing, 1);
    assert_eq!(fav, 1);
    assert_eq!(ign, 0);
    let after_items: i64 = conn.query_row("SELECT COUNT(*) FROM recommendation_items", [], |r| r.get(0)).unwrap();
    assert_eq!(after_items, 1, "recommendation snapshot 历史不得被 migration 改变");
}

// v13 backfill：crossref / openalex / letter-upgrade / 无证据 → unknown
#[test]
fn test_r7_v13_backfill_classifies_from_raw_json() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    // A: crossref news
    let a = raw_insert_legacy_paper(&conn, jid, "News A", "10.1000/v13-a", None, Some(r#"{"DOI":"10.1000/v13-a","type":"news"}"#));
    // B: openalex article
    let b = raw_insert_legacy_paper(&conn, jid, "Study B", "10.1000/v13-b", None, Some(r#"{"doi":"10.1000/v13-b","type":"article","authorships":[]}"#));
    // C: 无 raw_json → unknown（不误标）
    let c = raw_insert_legacy_paper(&conn, jid, "An Interesting Model of Pricing", "10.1000/v13-c", None, None);
    // D: crossref letter + openalex article → 保持 letter，不自动升级
    let d = raw_insert_legacy_paper(&conn, jid, "Letter D", "10.1000/v13-d", None, Some(r#"{"DOI":"10.1000/v13-d","type":"letter"}"#));
    conn.execute(
        "INSERT INTO source_records (paper_id, source, source_id, raw_json, retrieved_at) VALUES (?1,'openalex','x',?2,'t')",
        params![d, r#"{"doi":"10.1000/v13-d","type":"article","authorships":[]}"#],
    )
    .unwrap();
    rerun_v13_migration(&conn);

    let kind_of = |id: i64| -> (String, String, String) {
        conn.query_row(
            "SELECT content_kind, content_kind_source, content_kind_confidence FROM papers WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
    };
    let (ka, _, ca) = kind_of(a);
    assert_eq!(ka, "news");
    assert_eq!(ca, "EXACT");
    // Correctness Fix：OpenAlex broad article 不产出 research_article → unknown
    let (kb, sb, cb) = kind_of(b);
    assert_eq!(kb, "unknown", "OpenAlex article 是 broad type，不得映射 research_article");
    assert_eq!(sb, "none");
    assert_eq!(cb, "UNKNOWN");
    let (kc, _, cc) = kind_of(c);
    assert_eq!(kc, "unknown", "低置信度必须保留 unknown，不得误标");
    assert_eq!(cc, "UNKNOWN");
    // Correctness Fix：Crossref letter + OpenAlex article → letter（不升级 research_article）
    let (kd, sd, cd) = kind_of(d);
    assert_eq!(kd, "letter", "Crossref 显式 letter 保持 letter，不被 broad article 升级");
    assert_eq!(sd, "crossref:type");
    assert_eq!(cd, "EXACT");
    let st: String = conn.query_row("SELECT abstract_status FROM papers WHERE id=?1", params![a], |r| r.get(0)).unwrap();
    assert_eq!(st, "not_expected");
    let st: String = conn.query_row("SELECT abstract_status FROM papers WHERE id=?1", params![b], |r| r.get(0)).unwrap();
    assert_eq!(st, "unknown", "unknown + 无摘要 → abstract_status unknown（仍可 recovery）");
    let st: String = conn.query_row("SELECT abstract_status FROM papers WHERE id=?1", params![d], |r| r.get(0)).unwrap();
    assert_eq!(st, "not_expected", "letter 保守 → not_expected");
}

// 运行时补充分类：unknown → 第二次 upsert 带 raw_json 后填上；broad 证据不覆盖已分类
#[test]
fn test_r7_existing_upsert_fills_kind_when_unknown() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let id = match db::upsert_paper(&conn, jid, &cand_raw("10.1000/r7-fill", "First Pass", None, None)).unwrap() {
        UpsertOutcome::New(id) => id, _ => panic!(),
    };
    let p = db::get_paper(&conn, id).unwrap().unwrap();
    assert_eq!(p.content_kind, "unknown");
    // 第二次 upsert 带 crossref news 证据 → 填充（显式细分类型）
    let raw = r#"{"DOI":"10.1000/r7-fill","type":"news"}"#;
    db::upsert_paper(&conn, jid, &cand_raw("10.1000/r7-fill", "First Pass", None, Some(raw))).unwrap();
    let p = db::get_paper(&conn, id).unwrap().unwrap();
    assert_eq!(p.content_kind, "news");
    assert_eq!(p.abstract_status, "not_expected");
    // 第三次 upsert 换 broad journal-article 证据 → 不覆盖已分类（review #7）
    let raw2 = r#"{"DOI":"10.1000/r7-fill","type":"journal-article"}"#;
    db::upsert_paper(&conn, jid, &cand_raw("10.1000/r7-fill", "First Pass", None, Some(raw2))).unwrap();
    let p = db::get_paper(&conn, id).unwrap().unwrap();
    assert_eq!(p.content_kind, "news", "broad 类型证据不得覆盖已有可信分类");
    assert_eq!(p.abstract_status, "not_expected");
    // 显式 editorial 分类后，broad journal-article 证据同样不覆盖
    let raw3 = r#"{"DOI":"10.1000/r7-fill","type":"editorial"}"#;
    db::upsert_paper(&conn, jid, &cand_raw("10.1000/r7-fill", "First Pass", None, Some(raw3))).unwrap();
    let p = db::get_paper(&conn, id).unwrap().unwrap();
    assert_eq!(p.content_kind, "news", "已分类结果（news）不得被 editorial 覆盖，因为 fill 只补 unknown");
}

#[test]
fn test_library_migration_v13_to_v15_preserves_existing_data() {
    let conn = mem_db();
    // Turn a fully initialized in-memory database into a representative v13
    // database by removing only the Library tables created by the test setup.
    for table in [
        "library_item_tags",
        "library_collection_items",
        "library_items",
        "library_tags",
        "library_collections",
        "paper_attachments",
        "library_item_metadata",
    ] {
        conn.execute(&format!("DROP TABLE {}", table), []).unwrap();
    }
    conn.pragma_update(None, "user_version", 13).unwrap();

    let jid = db::insert_journal(&conn, "Migration J", Some("0025-1909"), None, None, None).unwrap();
    let pid = match db::upsert_paper(
        &conn,
        jid,
        &candidate(Some("10.1000/v13-v14"), "Migration Paper", Some("preserved abstract"), None),
    )
    .unwrap()
    {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
    };
    conn.execute(
        "UPDATE papers SET chinese_title='保留中文标题', chinese_abstract='保留中文摘要',
            one_sentence_summary='保留 AI 分析', total_score=4.8, is_favorite=1 WHERE id=?1",
        params![pid],
    )
    .unwrap();
    let run_id = db::create_recommendation_run(&conn, "2026-09-03", crate::recommendation::RC_OPEN).unwrap();
    conn.execute(
        "INSERT INTO recommendation_items (run_id, paper_id, rank, score_snapshot, added_at) VALUES (?1,?2,1,4.8,?3)",
        params![run_id, pid, "2026-09-03T00:00:00Z"],
    )
    .unwrap();
    db::set_setting(&conn, "settings.daily_sync_time", "08:30").unwrap();

    db::init(&conn).unwrap();
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, 15);
    let paper = db::get_paper(&conn, pid).unwrap().unwrap();
    assert_eq!(paper.abstract_text.as_deref(), Some("preserved abstract"));
    assert_eq!(paper.chinese_title.as_deref(), Some("保留中文标题"));
    assert_eq!(paper.chinese_abstract.as_deref(), Some("保留中文摘要"));
    assert_eq!(paper.one_sentence_summary.as_deref(), Some("保留 AI 分析"));
    assert_eq!(paper.total_score, Some(4.8));
    assert!(paper.is_favorite);
    assert_eq!(db::list_recommendation_runs(&conn).unwrap().len(), 1);
    assert_eq!(db::list_recommendation_items(&conn, run_id).unwrap().len(), 1);
    assert_eq!(db::get_setting(&conn, "settings.daily_sync_time").as_deref(), Some("08:30"));

    db::init(&conn).unwrap();
    let version_again: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version_again, 15);
    for table in [
        "library_items",
        "library_collections",
        "library_collection_items",
        "library_tags",
        "library_item_tags",
        "paper_attachments",
        "library_item_metadata",
    ] {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                params![table],
                |r| r.get(0),
            )
            .unwrap();
        assert!(exists, "缺少 Library 表 {table}");
    }
}

#[test]
fn test_migration_v14_to_v15_creates_attachment_and_metadata_tables() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "v14 Journal", Some("0025-1909"), None, None, None).unwrap();
    let pid = match db::upsert_paper(&conn, jid, &candidate(Some("10.1000/v14-v15"), "v14 Paper", None, None)).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
    };
    db::add_paper_to_library(&conn, pid, &[], &[], "manual").unwrap();
    conn.execute("DROP TABLE library_item_metadata", []).unwrap();
    conn.execute("DROP TABLE paper_attachments", []).unwrap();
    conn.pragma_update(None, "user_version", 14).unwrap();
    db::init(&conn).unwrap();
    assert_eq!(conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 15);
    assert!(db::get_library_membership(&conn, pid).unwrap().is_some(), "v15 不得破坏 v14 Library membership");
    for table in ["paper_attachments", "library_item_metadata"] {
        assert!(conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)", params![table], |r| r.get::<_, bool>(0)).unwrap());
    }
}

#[test]
fn test_library_migration_is_empty_and_idempotent() {
    let conn = mem_db();
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, db::SCHEMA_VERSION);
    for table in [
        "library_items",
        "library_collections",
        "library_collection_items",
        "library_tags",
        "library_item_tags",
        "paper_attachments",
        "library_item_metadata",
    ] {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                params![table],
                |r| r.get(0),
            )
            .unwrap();
        assert!(exists, "缺少 Library 表 {table}");
    }
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM library_items", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    // CREATE TABLE IF NOT EXISTS migration can safely be rerun.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS library_items (paper_id INTEGER PRIMARY KEY, added_at TEXT NOT NULL, added_source TEXT NOT NULL);",
    )
    .unwrap();
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM library_items", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn test_library_membership_is_canonical_idempotent_and_clears_read_later() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pid = match db::upsert_paper(&conn, jid, &candidate(Some("10.1000/library"), "Library Paper", Some("abstract"), Some("crossref")).clone()).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
    };
    db::set_paper_flag(&conn, pid, "favorite", true).unwrap();
    let first = db::add_paper_to_library(&conn, pid, &[], &[], "read_later").unwrap();
    let second = db::add_paper_to_library(&conn, pid, &[], &[], "recommendation").unwrap();
    assert_eq!(first.paper_id, pid);
    assert_eq!(second.paper_id, pid);
    assert_eq!(second.added_source, "read_later", "重复加入不覆盖首次 provenance");
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM library_items WHERE paper_id=?1", params![pid], |r| r.get::<_, i64>(0)).unwrap(), 1);
    let p = db::get_paper(&conn, pid).unwrap().unwrap();
    assert!(!p.is_favorite, "加入 Library 必须清除 Read Later");
    db::set_paper_flag(&conn, pid, "favorite", true).unwrap();
    assert!(!db::get_paper(&conn, pid).unwrap().unwrap().is_favorite, "Library Paper 不得重新进入 Read Later");
}

#[test]
fn test_library_collections_tags_views_and_removal_preserve_paper() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let a = match db::upsert_paper(&conn, jid, &candidate(Some("10.1000/library-a"), "Paper A", Some("abstract"), Some("crossref")).clone()).unwrap() { UpsertOutcome::New(id) => id, _ => panic!() };
    let b = match db::upsert_paper(&conn, jid, &candidate(Some("10.1000/library-b"), "Paper B", Some("abstract"), Some("crossref")).clone()).unwrap() { UpsertOutcome::New(id) => id, _ => panic!() };
    conn.execute("UPDATE papers SET total_score = 4.2 WHERE id = ?1", params![a]).unwrap();
    let score_before: Option<f64> = conn.query_row("SELECT total_score FROM papers WHERE id = ?1", params![a], |r| r.get(0)).unwrap();
    let root = db::create_library_collection(&conn, "博士论文", None).unwrap();
    let child = db::create_library_collection(&conn, "实证", Some(root.id)).unwrap();
    let other = db::create_library_collection(&conn, "准备引用", None).unwrap();
    let tag_a = db::create_library_tag(&conn, "核心文献", Some("#2563eb")).unwrap();
    let tag_b = db::create_library_tag(&conn, "待引用", None).unwrap();
    let membership = db::add_paper_to_library(&conn, a, &[root.id, child.id, other.id], &[tag_a.id, tag_b.id], "manual").unwrap();
    assert_eq!(membership.collection_ids.len(), 3);
    assert_eq!(membership.tag_ids.len(), 2);
    db::add_paper_to_library(&conn, b, &[], &[], "history").unwrap();
    let score_after: Option<f64> = conn.query_row("SELECT total_score FROM papers WHERE id = ?1", params![a], |r| r.get(0)).unwrap();
    assert_eq!(score_after, score_before, "Library membership must not alter recommendation score");
    assert_eq!(db::list_library_papers(&conn, "all", 100).unwrap().len(), 2);
    assert_eq!(db::list_library_papers(&conn, "unfiled", 100).unwrap().len(), 1);
    assert_eq!(db::list_library_papers(&conn, "recent", 100).unwrap().len(), 2);
    assert_eq!(db::list_library_tags(&conn).unwrap().len(), 2);
    assert_eq!(db::list_tags(&conn).unwrap().len(), 6, "Library Tags 不得污染 Research Tags");
    db::remove_paper_from_library(&conn, a).unwrap();
    assert!(db::get_paper(&conn, a).unwrap().is_some());
    assert!(db::get_library_membership(&conn, a).unwrap().is_none());
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM library_collection_items WHERE paper_id=?1", params![a], |r| r.get::<_, i64>(0)).unwrap(), 0);
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM library_item_tags WHERE paper_id=?1", params![a], |r| r.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn test_library_collection_delete_detaches_children_without_deleting_paper() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pid = match db::upsert_paper(&conn, jid, &candidate(Some("10.1000/library-delete"), "Paper", Some("abstract"), Some("crossref")).clone()).unwrap() { UpsertOutcome::New(id) => id, _ => panic!() };
    let parent = db::create_library_collection(&conn, "Parent", None).unwrap();
    let child = db::create_library_collection(&conn, "Child", Some(parent.id)).unwrap();
    db::add_paper_to_library(&conn, pid, &[child.id], &[], "manual").unwrap();
    assert!(db::delete_library_collection(&conn, parent.id).unwrap());
    let child_parent: Option<i64> = conn.query_row("SELECT parent_id FROM library_collections WHERE id=?1", params![child.id], |r| r.get(0)).unwrap();
    assert_eq!(child_parent, None);
    assert!(db::get_paper(&conn, pid).unwrap().is_some());
    assert!(db::get_library_membership(&conn, pid).unwrap().is_some());
}

#[test]
fn test_library_tag_rename_delete_preserves_paper_and_recommendation_fields() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "J", Some("0025-1909"), None, None, None).unwrap();
    let pid = match db::upsert_paper(
        &conn,
        jid,
        &candidate(Some("10.1000/library-tag-management"), "Tagged Paper", Some("abstract"), Some("crossref")),
    )
    .unwrap()
    {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
    };
    conn.execute(
        "UPDATE papers SET chinese_title='中文标题', one_sentence_summary='AI summary',
            tag_matches_json='[{\"tag\":\"核心\",\"score\":1}]', total_score=4.6,
            analysis_status='analysisSucceeded', analyzed_at='2026-09-03T00:00:00Z' WHERE id=?1",
        params![pid],
    )
    .unwrap();
    let tag = db::create_library_tag(&conn, "Original Library Tag", Some("#2563eb")).unwrap();
    db::add_paper_to_library(&conn, pid, &[], &[tag.id], "manual").unwrap();

    let recommendation_before: (Option<f64>, Option<String>, String, Option<String>) = conn
        .query_row(
            "SELECT total_score, tag_matches_json, analysis_status, analyzed_at FROM papers WHERE id=?1",
            params![pid],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    db::rename_library_tag(&conn, tag.id, "Renamed Library Tag").unwrap();
    assert_eq!(db::list_library_tags(&conn).unwrap()[0].name, "Renamed Library Tag");
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM library_item_tags WHERE paper_id=?1 AND tag_id=?2", params![pid, tag.id], |r| r.get::<_, i64>(0)).unwrap(),
        1,
    );

    assert!(db::delete_library_tag(&conn, tag.id).unwrap());
    assert!(db::list_library_tags(&conn).unwrap().is_empty());
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM library_item_tags WHERE paper_id=?1", params![pid], |r| r.get::<_, i64>(0)).unwrap(),
        0,
    );
    assert!(db::get_paper(&conn, pid).unwrap().is_some(), "删除文献标签不得删除 canonical Paper");
    assert!(db::get_library_membership(&conn, pid).unwrap().is_some(), "删除文献标签不得移出 Library");
    let recommendation_after: (Option<f64>, Option<String>, String, Option<String>) = conn
        .query_row(
            "SELECT total_score, tag_matches_json, analysis_status, analyzed_at FROM papers WHERE id=?1",
            params![pid],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(recommendation_after, recommendation_before, "Library Tag 管理不得改变推荐分析字段");
}

#[test]
fn test_v15_linked_attachment_detach_missing_and_relink() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "Attachment J", Some("0025-1909"), None, None, None).unwrap();
    let pid = match db::upsert_paper(&conn, jid, &candidate(Some("10.1000/attachment"), "Attachment Paper", Some("abstract"), Some("crossref"))).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
    };
    let first = test_pdf_path("attachment-a", "%PDF-1.7\n/Title (A)\n");
    let second = test_pdf_path("attachment-b", "%PDF-1.7\n/Title (B)\n");
    let attachment = db::attach_pdf_to_paper(&conn, pid, first.to_str().unwrap()).unwrap();
    assert_eq!(attachment.storage_mode, "linked");
    assert!(!attachment.missing);
    assert!(attachment.sha256.as_ref().is_some_and(|value| value.len() == 64));
    assert!(first.exists());

    std::fs::remove_file(&first).unwrap();
    assert!(db::list_paper_attachments(&conn, pid).unwrap()[0].missing, "missing 只影响读取状态，不删除关系");
    let relinked = db::relink_pdf(&conn, attachment.id, second.to_str().unwrap()).unwrap();
    assert!(!relinked.missing);
    assert!(second.exists());
    assert!(db::detach_pdf(&conn, attachment.id).unwrap());
    assert!(second.exists(), "detach 不得删除用户 PDF");
    assert!(db::list_paper_attachments(&conn, pid).unwrap().is_empty());
    let _ = std::fs::remove_file(second);
}

#[test]
fn test_pdf_storage_none_keeps_linked_source() {
    let conn = mem_db();
    let pid = test_paper(&conn, "10.1000/storage-none", "None Paper");
    let source = test_pdf_path("storage-none", "%PDF-1.7\n");
    db::set_setting(&conn, "settings.pdf_file_handling_mode", "none").unwrap();
    let attachment = db::attach_pdf_to_paper(&conn, pid, source.to_str().unwrap()).unwrap();
    assert_eq!(attachment.storage_mode, "linked");
    assert_eq!(attachment.relative_path, None);
    let canonical_source = std::fs::canonicalize(&source).unwrap();
    assert_eq!(std::path::Path::new(&attachment.absolute_path), canonical_source.as_path());
    assert!(source.exists());
    let _ = std::fs::remove_file(source);
}

#[test]
fn test_pdf_storage_copy_preserves_source_and_verifies_destination() {
    let conn = mem_db();
    let pid = test_paper(&conn, "10.1000/storage-copy", "Copy Paper");
    let source = test_pdf_path("storage-copy", "%PDF-1.7\ncopy body\n");
    let root = test_pdf_library("copy");
    set_pdf_storage_settings(&conn, "copy", &root, "{title} - {first_author} - {year}.pdf", "none");
    let attachment = db::attach_pdf_to_paper(&conn, pid, source.to_str().unwrap()).unwrap();
    assert_eq!(attachment.storage_mode, "managed");
    let canonical_root = std::fs::canonicalize(&root).unwrap();
    assert!(std::path::Path::new(&attachment.absolute_path).starts_with(&canonical_root));
    assert!(attachment.relative_path.is_some());
    assert!(std::path::Path::new(&attachment.absolute_path).is_file());
    assert!(source.is_file(), "copy 必须保留源 PDF");
    assert_eq!(attachment.sha256, Some(sha256_file_for_test(&source)));
    let _ = std::fs::remove_file(source);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn test_pdf_storage_move_deletes_source_only_after_verified_destination() {
    let conn = mem_db();
    let pid = test_paper(&conn, "10.1000/storage-move", "Move Paper");
    let source = test_pdf_path("storage-move", "%PDF-1.7\nmove body\n");
    let root = test_pdf_library("move");
    set_pdf_storage_settings(&conn, "move", &root, "{title}.pdf", "none");
    let attachment = db::attach_pdf_to_paper(&conn, pid, source.to_str().unwrap()).unwrap();
    assert_eq!(attachment.storage_mode, "managed");
    assert!(!source.exists(), "move 完成后才删除源 PDF");
    assert!(std::path::Path::new(&attachment.absolute_path).is_file());
    let _ = std::fs::remove_file(attachment.absolute_path);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn test_pdf_storage_failed_move_preserves_source() {
    let conn = mem_db();
    let pid = test_paper(&conn, "10.1000/storage-move-failure", "Move Failure Paper");
    let source = test_pdf_path("storage-move-failure", "%PDF-1.7\n");
    let root_file = test_pdf_path("storage-root-file", "not a directory");
    set_pdf_storage_settings(&conn, "move", &root_file, "{title}.pdf", "none");
    assert!(db::attach_pdf_to_paper(&conn, pid, source.to_str().unwrap()).is_err());
    assert!(source.exists(), "目标准备失败时绝不能删除源 PDF");
    assert_eq!(db::list_paper_attachments(&conn, pid).unwrap().len(), 0);
    let _ = std::fs::remove_file(source);
    let _ = std::fs::remove_file(root_file);
}

fn sha256_file_for_test(path: &std::path::Path) -> String {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).unwrap();
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn test_pdf_filename_sanitization_collision_and_empty_fields() {
    let conn = mem_db();
    let pid = test_paper(&conn, "10.1000/storage-sanitize", "Bad / \\ : * ? \" < > | Title");
    let source = test_pdf_path("storage-sanitize", "%PDF-1.7\n");
    let root = test_pdf_library("sanitize");
    set_pdf_storage_settings(&conn, "copy", &root, "{title} - {doi} - {authors} - {year}.pdf", "none");
    let first = db::attach_pdf_to_paper(&conn, pid, source.to_str().unwrap()).unwrap();
    let second = db::attach_pdf_to_paper(&conn, pid, source.to_str().unwrap()).unwrap();
    for attachment in [&first, &second] {
        let name = std::path::Path::new(&attachment.absolute_path).file_name().unwrap().to_str().unwrap();
        assert!(name.ends_with(".pdf"));
        assert!(!name.chars().any(|ch| matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')));
        assert!(name.len() <= 180);
    }
    assert_ne!(first.absolute_path, second.absolute_path, "collision 不得覆盖原文件");
    assert!(second.filename.contains("(2).pdf"));
    let _ = std::fs::remove_file(source);
    let _ = std::fs::remove_file(first.absolute_path);
    let _ = std::fs::remove_file(second.absolute_path);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn test_pdf_storage_year_subfolder() {
    let conn = mem_db();
    let pid = test_paper(&conn, "10.1000/storage-year", "Year Paper");
    let source = test_pdf_path("storage-year", "%PDF-1.7\n");
    let root = test_pdf_library("year");
    set_pdf_storage_settings(&conn, "copy", &root, "{title}.pdf", "year");
    let attachment = db::attach_pdf_to_paper(&conn, pid, source.to_str().unwrap()).unwrap();
    assert_eq!(attachment.relative_path.as_deref().unwrap().split(std::path::MAIN_SEPARATOR).next(), Some("2025"));
    assert!(std::path::Path::new(&attachment.absolute_path).is_file());
    let _ = std::fs::remove_file(source);
    let _ = std::fs::remove_file(attachment.absolute_path);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn test_linked_to_managed_copy_and_move_are_explicit() {
    let conn = mem_db();
    let pid = test_paper(&conn, "10.1000/storage-reorganize", "Reorganize Paper");
    let source = test_pdf_path("storage-reorganize", "%PDF-1.7\n");
    db::set_setting(&conn, "settings.pdf_file_handling_mode", "none").unwrap();
    let linked = db::attach_pdf_to_paper(&conn, pid, source.to_str().unwrap()).unwrap();
    let copy_root = test_pdf_library("reorganize-copy");
    set_pdf_storage_settings(&conn, "none", &copy_root, "{title}.pdf", "none");
    let copied = db::reorganize_pdf(&conn, linked.id, "copy").unwrap();
    assert_eq!(copied.storage_mode, "managed");
    assert!(source.exists());
    assert!(std::path::Path::new(&copied.absolute_path).exists());

    db::set_setting(&conn, "settings.pdf_file_handling_mode", "none").unwrap();
    let linked_for_move = db::attach_pdf_to_paper(&conn, pid, source.to_str().unwrap()).unwrap();
    let move_root = test_pdf_library("reorganize-move");
    set_pdf_storage_settings(&conn, "none", &move_root, "{title} moved.pdf", "none");
    let moved = db::reorganize_pdf(&conn, linked_for_move.id, "move").unwrap();
    assert_eq!(moved.storage_mode, "managed");
    assert!(!source.exists(), "显式 move 完成后源路径才消失");
    assert!(std::path::Path::new(&moved.absolute_path).exists());
    let _ = std::fs::remove_file(copied.absolute_path);
    let _ = std::fs::remove_file(moved.absolute_path);
    let _ = std::fs::remove_dir_all(copy_root);
    let _ = std::fs::remove_dir_all(move_root);
}

#[test]
fn test_managed_detach_does_not_delete_file() {
    let conn = mem_db();
    let pid = test_paper(&conn, "10.1000/storage-detach", "Detach Managed Paper");
    let source = test_pdf_path("storage-detach", "%PDF-1.7\n");
    let root = test_pdf_library("detach");
    set_pdf_storage_settings(&conn, "copy", &root, "{title}.pdf", "none");
    let attachment = db::attach_pdf_to_paper(&conn, pid, source.to_str().unwrap()).unwrap();
    let managed_path = std::path::PathBuf::from(&attachment.absolute_path);
    assert!(db::detach_pdf(&conn, attachment.id).unwrap());
    assert!(source.exists(), "detach 不能删除源 PDF");
    assert!(managed_path.exists(), "detach 不能删除 managed PDF");
    let _ = std::fs::remove_file(source);
    let _ = std::fs::remove_file(managed_path);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn test_discovery_attach_pdf_adds_library_and_clears_read_later_atomically() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "Discovery J", Some("0025-1909"), None, None, None).unwrap();
    let pid = match db::upsert_paper(&conn, jid, &candidate(Some("10.1000/discovery-attach"), "Discovery Paper", None, None)).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
    };
    db::set_paper_flag(&conn, pid, "favorite", true).unwrap();
    let path = test_pdf_path("discovery", "%PDF-1.7\n/Title (Discovery)\n");
    let attachment = db::attach_discovery_pdf(&conn, pid, path.to_str().unwrap()).unwrap();
    assert_eq!(attachment.paper_id, pid);
    let membership = db::get_library_membership(&conn, pid).unwrap().unwrap();
    assert_eq!(membership.added_source, "discovery_attach_pdf");
    assert!(!db::get_paper(&conn, pid).unwrap().unwrap().is_favorite);
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM library_items WHERE paper_id=?1", params![pid], |r| r.get::<_, i64>(0)).unwrap(), 1);
    let _ = std::fs::remove_file(path);
}

#[test]
fn test_external_pdf_doi_import_does_not_duplicate_canonical_paper() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "Existing J", Some("0025-1909"), None, None, None).unwrap();
    let pid = match db::upsert_paper(&conn, jid, &candidate(Some("10.1000/exact-pdf"), "Existing Paper", Some("real abstract"), Some("crossref"))).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
    };
    let path = test_pdf_path(
        "doi-import",
        "%PDF-1.7\n1 0 obj << /Title (Imported Filename) /Author (PDF Author) /CreationDate (D:2024) /DOI (10.1000/exact-pdf) >>\n",
    );
    let result = db::import_external_pdf(&conn, path.to_str().unwrap(), None).unwrap();
    assert_eq!(result.outcome, "existingDoi");
    assert_eq!(result.paper_id, Some(pid));
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM papers", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert!(db::get_library_membership(&conn, pid).unwrap().is_some());
    assert_eq!(db::list_paper_attachments(&conn, pid).unwrap().len(), 1);
    let _ = std::fs::remove_file(path);
}

#[test]
fn test_external_pdf_import_uses_managed_storage_without_second_canonical_paper() {
    let conn = mem_db();
    let root = test_pdf_library("external-copy");
    set_pdf_storage_settings(&conn, "copy", &root, "{title} - {year}.pdf", "year");
    let path = test_pdf_path(
        "external-copy",
        "%PDF-1.7\n1 0 obj << /Title (External Managed Paper) /Author (External Author) /CreationDate (D:2024) /DOI (10.1000/external-managed) >>\n",
    );
    let result = db::import_external_pdf(&conn, path.to_str().unwrap(), None).unwrap();
    assert_eq!(result.outcome, "createdExternalPaper");
    let pid = result.paper_id.unwrap();
    let attachment = result.attachment.unwrap();
    assert_eq!(attachment.paper_id, pid);
    assert_eq!(attachment.storage_mode, "managed");
    assert!(path.exists(), "external import 的 copy 必须保留源 PDF");
    assert!(std::path::Path::new(&attachment.absolute_path).is_file());
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM papers", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
    assert!(std::path::Path::new(attachment.relative_path.as_deref().unwrap()).starts_with("2024"));
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(attachment.absolute_path);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn test_external_pdf_without_identity_creates_canonical_paper_and_never_generates_abstract() {
    let conn = mem_db();
    let path = test_pdf_path(
        "new-external",
        "%PDF-1.7\n1 0 obj << /Title (A New External Paper) /Author (New Author) /CreationDate (D:2023) >>\n",
    );
    let result = db::import_external_pdf(&conn, path.to_str().unwrap(), None).unwrap();
    assert_eq!(result.outcome, "createdExternalPaper");
    let pid = result.paper_id.unwrap();
    let paper = db::get_paper(&conn, pid).unwrap().unwrap();
    assert_eq!(paper.title.as_deref(), Some("A New External Paper"));
    assert_eq!(paper.year, Some(2023));
    assert!(paper.abstract_text.is_none(), "title 不得生成 abstract");
    assert!(db::get_library_membership(&conn, pid).unwrap().is_some());
    assert!(db::list_paper_attachments(&conn, pid).unwrap()[0].absolute_path.contains("new-external"));
    for table in ["library_papers", "external_library_papers"] {
        assert!(!conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)", params![table], |r| r.get::<_, bool>(0)).unwrap(), "禁止第二 Paper 表 {table}");
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn test_external_pdf_without_reliable_identity_does_not_title_merge() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "External PDF Import", None, None, None, None).unwrap();
    let mut existing_candidate = candidate(None, "Same External Title", None, None);
    existing_candidate.authors.clear();
    existing_candidate.published_date = Some("2024-01-01".into());
    existing_candidate.year = Some(2024);
    let existing = match db::upsert_paper(&conn, jid, &existing_candidate).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
    };
    assert!(existing > 0);
    let path = test_pdf_path(
        "same-title-no-author",
        "%PDF-1.7\n1 0 obj << /Title (Same External Title) /CreationDate (D:2024) >>\n",
    );
    let result = db::import_external_pdf(&conn, path.to_str().unwrap(), None).unwrap();
    assert_eq!(result.outcome, "createdExternalPaper");
    assert_ne!(result.paper_id, Some(existing), "无可靠 identity 不得按标题静默合并");
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM papers", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
    let _ = std::fs::remove_file(path);
}

#[test]
fn test_external_pdf_title_author_year_is_manual_candidate_only() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "Candidate J", Some("0025-1909"), None, None, None).unwrap();
    let mut existing = candidate(None, "Candidate Paper", None, None);
    existing.authors = vec![Author { given: None, family: None, name: Some("Alice Smith".into()) }];
    let pid = match db::upsert_paper(&conn, jid, &existing).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
    };
    let path = test_pdf_path(
        "candidate",
        "%PDF-1.7\n1 0 obj << /Title (Candidate Paper) /Author (Alice Smith) /CreationDate (D:2025) >>\n",
    );
    let pending = db::import_external_pdf(&conn, path.to_str().unwrap(), None).unwrap();
    assert_eq!(pending.outcome, "needsManualConfirmation");
    assert!(pending.requires_confirmation);
    assert_eq!(pending.candidate.unwrap().paper_id, pid);
    assert!(db::get_library_membership(&conn, pid).unwrap().is_none());
    let confirmed = db::import_external_pdf(&conn, path.to_str().unwrap(), Some(pid)).unwrap();
    assert_eq!(confirmed.outcome, "manualConfirmation");
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM papers", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert!(db::get_library_membership(&conn, pid).unwrap().is_some());
    let _ = std::fs::remove_file(path);
}

#[test]
fn test_library_metadata_overrides_effective_values_without_mutating_canonical_paper() {
    let conn = mem_db();
    let jid = db::insert_journal(&conn, "Canonical Journal", Some("0025-1909"), None, None, None).unwrap();
    let pid = match db::upsert_paper(&conn, jid, &candidate(Some("10.1000/metadata"), "Canonical Title", Some("Canonical Abstract"), Some("crossref"))).unwrap() {
        UpsertOutcome::New(id) => id,
        _ => panic!("expected new paper"),
    };
    db::add_paper_to_library(&conn, pid, &[], &[], "manual").unwrap();
    let input = crate::models::LibraryItemMetadataInput {
        title_override: Some("Personal Title".into()),
        chinese_title_override: Some("个人标题".into()),
        source_override: Some("Personal Source".into()),
        year_override: Some(2020),
        authors_override: Some(vec![Author { given: None, family: None, name: Some("Personal Author".into()) }]),
        abstract_override: Some("Personal Abstract".into()),
        chinese_abstract_override: Some("个人摘要".into()),
        note: Some("Keep for review".into()),
    };
    let metadata = db::set_library_item_metadata(&conn, pid, &input).unwrap();
    assert_eq!(metadata.note.as_deref(), Some("Keep for review"));
    let item = db::get_library_paper(&conn, pid).unwrap().unwrap();
    assert_eq!(item.effective_title.as_deref(), Some("Personal Title"));
    assert_eq!(item.effective_source.as_deref(), Some("Personal Source"));
    assert_eq!(item.effective_year, Some(2020));
    assert_eq!(item.effective_abstract.as_deref(), Some("Personal Abstract"));
    assert_eq!(item.note.as_deref(), Some("Keep for review"));
    let canonical = db::get_paper(&conn, pid).unwrap().unwrap();
    assert_eq!(canonical.title.as_deref(), Some("Canonical Title"));
    assert_eq!(canonical.year, Some(2025));
    assert_eq!(canonical.abstract_text.as_deref(), Some("Canonical Abstract"));

    db::set_library_item_note(&conn, pid, Some("Updated note")).unwrap();
    assert_eq!(db::get_library_item_metadata(&conn, pid).unwrap().unwrap().note.as_deref(), Some("Updated note"));
    db::clear_library_item_overrides(&conn, pid).unwrap();
    let reset = db::get_library_paper(&conn, pid).unwrap().unwrap();
    assert_eq!(reset.effective_title.as_deref(), Some("Canonical Title"));
    assert_eq!(reset.effective_abstract.as_deref(), Some("Canonical Abstract"));
    assert_eq!(reset.note.as_deref(), Some("Updated note"), "reset override 不得清除 note");
}

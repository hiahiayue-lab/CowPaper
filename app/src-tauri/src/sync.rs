use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use tauri::{AppHandle, Emitter, Runtime};

use crate::api::{crossref::Crossref, openalex::OpenAlex};
use crate::db;
use crate::models::{
    Journal, PaperCandidate, SyncProgress, SyncReport, UpsertOutcome,
};

/// 单期刊同步结果：本次涉及论文及其结果（用于 SyncBatch 关联）。
pub struct JournalSyncResult {
    pub inserted: Vec<i64>,
    pub existing: Vec<i64>,
    pub abstract_updated: Vec<i64>,
}

/// 在后台线程中运行同步（由命令触发）。同步一个持久化 SyncBatch，
/// 通过事件回报进度（sync://progress 携带 SyncProgress）。
pub fn run_sync<R: Runtime>(
    conn: &Arc<Mutex<Connection>>,
    ids: Option<Vec<i64>>,
    app: &AppHandle<R>,
    mailto: &str,
    batch_id: i64,
    trigger: &str,
) -> SyncReport {
    let start = std::time::Instant::now();
    let mut report = SyncReport::default();
    let started_at = db::now_utc();

    let journals = {
        let c = conn.lock().unwrap();
        db::list_journals(&c).unwrap_or_default()
    };
    let journals: Vec<Journal> = journals
        .into_iter()
        .filter(|j| j.enabled && ids.as_ref().map_or(true, |ids| ids.contains(&j.id)))
        .collect();
    report.checked_journals = journals.len() as i64;
    {
        let c = conn.lock().unwrap();
        // journal_total 必须持久化（Activity 历史显示 期刊 x/y）
        let _ = db::set_sync_batch_journal_total(&c, batch_id, journals.len() as i64);
        let _ = db::update_sync_batch_journal_progress(&c, batch_id, 0, 0);
    }

    let crossref = Crossref::new(mailto);
    let openalex = OpenAlex::new(mailto);
    let to = chrono::Utc::now().format("%Y-%m-%d").to_string();

    let mut journal_completed: i64 = 0;
    let mut journal_failed: i64 = 0;

    let emit_progress = |current: Option<&str>,
                         jc: i64,
                         jf: i64,
                         records: i64,
                         inserted: i64,
                         existing: i64,
                         added: i64| {
        let prog = SyncProgress {
            batch_id,
            trigger: trigger.to_string(),
            journal_total: journals.len() as i64,
            journal_completed: jc,
            journal_failed: jf,
            current_journal: current.map(str::to_string),
            records_found: records,
            papers_inserted: inserted,
            papers_existing: existing,
            abstracts_added: added,
            started_at: started_at.clone(),
        };
        let _ = app.emit("sync://progress", &prog);
    };

    for j in &journals {
        let journal_started = std::time::Instant::now();
        let _ = app.emit("sync://journal-start", j.name.clone());
        emit_progress(
            Some(&j.name),
            journal_completed,
            journal_failed,
            report.found_records,
            report.new_papers,
            report.existing_papers,
            report.abstracts_added,
        );
        match sync_journal(conn, &crossref, &openalex, j, &to, &mut report) {
            Ok(res) => {
                journal_completed += 1;
                report.new_paper_ids.extend(res.inserted.iter().copied());
                {
                    let c = conn.lock().unwrap();
                    let _ = db::add_sync_batch_papers(
                        &c,
                        batch_id,
                        &res.inserted,
                        &res.existing,
                        &res.abstract_updated,
                    );
                }
                eprintln!(
                    "sync journal completed journal={:?} elapsed_ms={} papers={}",
                    j.name,
                    journal_started.elapsed().as_millis(),
                    res.inserted.len() + res.existing.len()
                );
                let _ = app.emit("sync://journal-done", j.name.clone());
            }
            Err(e) => {
                journal_failed += 1;
                report.failed_journals += 1;
                eprintln!(
                    "sync journal failed journal={:?} elapsed_ms={} error={}",
                    j.name,
                    journal_started.elapsed().as_millis(),
                    e
                );
                let _ = app.emit("sync://journal-error", format!("{}: {}", j.name, e));
            }
        }
        // Persist progress after *every* terminal journal result. Otherwise a
        // failed journal leaves Activity showing stale counts while the worker
        // correctly proceeds to the next journal.
        {
            let c = conn.lock().unwrap();
            let _ = db::update_sync_batch_journal_progress(
                &c,
                batch_id,
                journal_completed,
                journal_failed,
            );
        }
        emit_progress(
            None,
            journal_completed,
            journal_failed,
            report.found_records,
            report.new_papers,
            report.existing_papers,
            report.abstracts_added,
        );
    }

    {
        let c = conn.lock().unwrap();
        report.waiting_for_abstract = db::count_waiting_for_abstract(&c).unwrap_or(0);
        let _ = db::update_sync_batch_counts(
            &c,
            batch_id,
            report.found_records,
            report.new_papers,
            report.existing_papers,
            report.abstracts_added,
            report.abstracts_upgraded,
            report.waiting_for_abstract,
        );
    }
    // 后端保证 disjoint：本次 discovery 中新建的 Paper 归为 new（不重复视为历史升级）。
    // 同一次 sync 里从较差 candidate 换成更好 candidate 的论文，升级归类让位给 new。
    report
        .abstract_upgraded_ids
        .retain(|id| !report.new_paper_ids.contains(id));
    report.duration_ms = start.elapsed().as_millis() as i64;
    report
}

fn sync_journal(
    conn: &Arc<Mutex<Connection>>,
    crossref: &Crossref,
    openalex: &OpenAlex,
    j: &Journal,
    to: &str,
    report: &mut SyncReport,
) -> Result<JournalSyncResult, String> {
    // 多 ISSN：收集该 canonical Journal 的全部 identifiers（print/online/other），
    // normalize + 去重，同一 ISSN 只发起一次 API 查询。
    let mut issns: Vec<String> = Vec::new();
    for idf in &j.identifiers {
        if let Some(n) = crate::util::normalize_issn(&idf.value) {
            if !issns.contains(&n) {
                issns.push(n);
            }
        }
    }
    for raw in [&j.print_issn, &j.online_issn].into_iter().flatten() {
        if let Some(n) = crate::util::normalize_issn(raw) {
            if !issns.contains(&n) {
                issns.push(n);
            }
        }
    }
    if issns.is_empty() {
        return Err("缺少 ISSN".to_string());
    }

    // 增量起点：上次成功同步 - 24h（首次回溯 30 天），满足 §7.2。
    let from = {
        let c = conn.lock().unwrap();
        let last = db::get_last_successful_sync_at(&c, j.id).map_err(|e| e.to_string())?;
        match last {
            Some(iso) => chrono::DateTime::parse_from_rfc3339(&iso)
                .map(|dt| (dt - chrono::Duration::hours(24)).format("%Y-%m-%d").to_string())
                .unwrap_or_else(|_| thirty_days_ago()),
            None => thirty_days_ago(),
        }
    };

    // 发现：Crossref 为主力（多 ISSN 各自查询，DOI 去重由 upsert 的 normalized_doi 唯一索引保证），
    // OpenAlex 为 fallback/补漏（尤其 Crossref 无记录时，如 HBR）。
    // "Crossref unsupported ≠ Journal unsupported"：只要任一 configured source 提供数据，sync 即成功。
    let mut candidates: Vec<PaperCandidate> = Vec::new();
    let mut crossref_ok = false;
    let mut crossref_err: Option<String> = None;
    for i in &issns {
        let source_started = std::time::Instant::now();
        eprintln!("sync source start journal={:?} source=crossref issn={}", j.name, i);
        match crossref.works(i, &from, to) {
            Ok(Some(w)) => {
                crossref_ok = true;
                eprintln!("sync source end journal={:?} source=crossref elapsed_ms={} result=ok candidates={}", j.name, source_started.elapsed().as_millis(), w.candidates.len());
                candidates.extend(w.candidates);
            }
            Ok(None) => eprintln!("sync source end journal={:?} source=crossref elapsed_ms={} result=unsupported", j.name, source_started.elapsed().as_millis()),
            Err(e) => {
                eprintln!("sync source end journal={:?} source=crossref elapsed_ms={} result=error error={}", j.name, source_started.elapsed().as_millis(), e);
                crossref_err = Some(e);
            }
        }
    }
    // OpenAlex fallback：无论 Crossref 结果如何都尝试补漏
    // Catalog imports may not yet have a cached OpenAlex source id. Resolve it
    // once from the first configured ISSN and persist it; later syncs reuse the
    // stored id and therefore avoid this metadata request.
    let mut openalex_ok = false;
    let mut openalex_err: Option<String> = None;
    let source_resolve_started = std::time::Instant::now();
    let openalex_source_id = match &j.openalex_source_id {
        Some(sid) => {
            eprintln!("sync source end journal={:?} source=openalex-resolve elapsed_ms=0 result=cached", j.name);
            Some(sid.clone())
        }
        None => match openalex.source_by_issn(&issns[0]) {
            Ok(Some(sid)) => {
                eprintln!("sync source end journal={:?} source=openalex-resolve elapsed_ms={} result=ok", j.name, source_resolve_started.elapsed().as_millis());
                if let Ok(c) = conn.lock() {
                    let _ = db::set_journal_openalex_source(&c, j.id, Some(&sid));
                }
                Some(sid)
            }
            Ok(None) => {
                eprintln!("sync source end journal={:?} source=openalex-resolve elapsed_ms={} result=unsupported", j.name, source_resolve_started.elapsed().as_millis());
                None
            }
            Err(e) => {
                eprintln!("sync source end journal={:?} source=openalex-resolve elapsed_ms={} result=error error={}", j.name, source_resolve_started.elapsed().as_millis(), e);
                openalex_err = Some(e);
                None
            }
        },
    };
    if let Some(sid) = &openalex_source_id {
        let source_started = std::time::Instant::now();
        eprintln!("sync source start journal={:?} source=openalex", j.name);
        match openalex.works(sid, &from, to) {
            Ok(Some(oa)) => {
                openalex_ok = true;
                eprintln!("sync source end journal={:?} source=openalex elapsed_ms={} result=ok candidates={}", j.name, source_started.elapsed().as_millis(), oa.len());
                candidates.extend(oa);
            }
            Ok(None) => eprintln!("sync source end journal={:?} source=openalex elapsed_ms={} result=unsupported", j.name, source_started.elapsed().as_millis()),
            Err(e) => {
                eprintln!("sync source end journal={:?} source=openalex elapsed_ms={} result=error error={}", j.name, source_started.elapsed().as_millis(), e);
                openalex_err = Some(e);
            }
        }
    }
    // Overall coverage：所有 configured source 都 unsupported（非网络错误）→ overall unsupported；
    // 任一 source 成功（即使 0 篇）→ sync 成功。
    let any_discovery = source_discovery_succeeded(crossref_ok, openalex_ok);
    if !any_discovery {
        if crossref_err.is_none() && openalex_err.is_none() {
            let c = conn.lock().unwrap();
            let _ = db::update_journal_sync_state(
                &c,
                j.id,
                &crate::db::now_utc(),
                None,
                "unsupported",
                None,
            );
            drop(c);
            return Ok(JournalSyncResult {
                inserted: Vec::new(),
                existing: Vec::new(),
                abstract_updated: Vec::new(),
            });
        }
        let msg = openalex_err
            .as_ref()
            .or(crossref_err.as_ref())
            .cloned()
            .unwrap_or_else(|| "所有数据源均不可用".to_string());
        return Err(format!("同步失败: {}", msg));
    }
    report.found_records += candidates.len() as i64;

    // 合并入库（DOI 去重 + 缺失字段补全）。
    let mut c = conn.lock().unwrap();
    let tx = c.transaction().map_err(|e| e.to_string())?;
    let mut res = JournalSyncResult {
        inserted: Vec::new(),
        existing: Vec::new(),
        abstract_updated: Vec::new(),
    };
    for cand in &candidates {
        match db::upsert_paper(&tx, j.id, cand) {
            Ok(UpsertOutcome::New(id)) => {
                report.new_papers += 1;
                res.inserted.push(id);
                let _ = db::insert_source_record(
                    &tx,
                    id,
                    &cand.discovery_source,
                    cand.source_id.as_deref(),
                    cand.raw_json.as_deref(),
                );
            }
            Ok(UpsertOutcome::Existing { id, abstract_filled, abstract_upgraded }) => {
                report.existing_papers += 1;
                res.existing.push(id);
                if abstract_filled {
                    report.abstracts_added += 1;
                    res.abstract_updated.push(id);
                }
                if abstract_upgraded {
                    report.abstracts_upgraded += 1;
                    report.abstract_upgraded_ids.push(id);
                }
                let _ = db::insert_source_record(
                    &tx,
                    id,
                    &cand.discovery_source,
                    cand.source_id.as_deref(),
                    cand.raw_json.as_deref(),
                );
            }
            Err(_) => {}
        }
    }

    // 更新期刊同步状态与摘要覆盖率。
    let (paper_count, abstract_count, last_paper_date) =
        db::journal_stats(&tx, j.id).map_err(|e| e.to_string())?;
    let rate = if paper_count > 0 {
        Some(abstract_count as f64 / paper_count as f64)
    } else {
        None
    };
    let status = match rate {
        None => "unsupported",
        Some(r) if r >= 0.7 => "fullySupported",
        Some(_) => "supportedWithMissingAbstracts",
    };
    db::update_journal_sync_state(
        &tx,
        j.id,
        &db::now_utc(),
        last_paper_date.as_deref(),
        status,
        rate,
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;

    Ok(res)
}

/// A working fallback makes the journal sync successful even if the primary
/// source had a transient failure. Only fail the journal when no configured
/// source completed a request.
pub(crate) fn source_discovery_succeeded(crossref_ok: bool, openalex_ok: bool) -> bool {
    crossref_ok || openalex_ok
}

fn thirty_days_ago() -> String {
    (chrono::Utc::now() - chrono::Duration::days(30)).format("%Y-%m-%d").to_string()
}

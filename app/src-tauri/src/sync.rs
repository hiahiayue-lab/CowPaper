use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use tauri::{AppHandle, Emitter, Runtime};

use crate::api::{crossref::Crossref, openalex::OpenAlex};
use crate::db;
use crate::models::{Journal, PaperCandidate, SyncReport, UpsertOutcome};

/// 在后台线程中运行同步（由命令触发）。通过事件回报进度与结果。
pub fn run_sync<R: Runtime>(
    conn: &Arc<Mutex<Connection>>,
    ids: Option<Vec<i64>>,
    app: &AppHandle<R>,
    mailto: &str,
) -> SyncReport {
    let start = std::time::Instant::now();
    let mut report = SyncReport::default();

    let journals = {
        let c = conn.lock().unwrap();
        db::list_journals(&c).unwrap_or_default()
    };
    let journals: Vec<Journal> = journals
        .into_iter()
        .filter(|j| j.enabled && ids.as_ref().map_or(true, |ids| ids.contains(&j.id)))
        .collect();
    report.checked_journals = journals.len() as i64;

    let crossref = Crossref::new(mailto);
    let openalex = OpenAlex::new(mailto);
    let to = chrono::Utc::now().format("%Y-%m-%d").to_string();

    let mut new_ids: Vec<i64> = Vec::new();
    for j in &journals {
        let _ = app.emit("sync://journal-start", j.name.clone());
        match sync_journal(conn, &crossref, &openalex, j, &to, &mut report) {
            Ok(ids) => {
                new_ids.extend(ids);
                let _ = app.emit("sync://journal-done", j.name.clone());
            }
            Err(e) => {
                report.failed_journals += 1;
                let _ = app.emit("sync://journal-error", format!("{}: {}", j.name, e));
            }
        }
    }
    report.new_paper_ids = new_ids;

    {
        let c = conn.lock().unwrap();
        report.waiting_for_abstract = db::count_waiting_for_abstract(&c).unwrap_or(0);
    }
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
) -> Result<Vec<i64>, String> {
    let issn = j
        .print_issn
        .as_deref()
        .or(j.online_issn.as_deref())
        .ok_or_else(|| "缺少 ISSN".to_string())?;

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

    // 发现：Crossref 为主力，OpenAlex 补漏 + 补摘要。
    let mut candidates: Vec<PaperCandidate> = Vec::new();
    match crossref.works(issn, &from, to) {
        Some(w) => candidates.extend(w.candidates),
        None => return Err("Crossref 获取失败".into()),
    }
    if let Some(sid) = &j.openalex_source_id {
        if let Some(oa) = openalex.works(sid, &from, to) {
            candidates.extend(oa);
        }
    }
    report.found_records += candidates.len() as i64;

    // 合并入库（DOI 去重 + 缺失字段补全）。
    let mut c = conn.lock().unwrap();
    let tx = c.transaction().map_err(|e| e.to_string())?;
    let mut new_ids: Vec<i64> = Vec::new();
    for cand in &candidates {
        match db::upsert_paper(&tx, j.id, cand) {
            Ok(UpsertOutcome::New(id)) => {
                report.new_papers += 1;
                new_ids.push(id);
                let _ = db::insert_source_record(
                    &tx,
                    id,
                    &cand.discovery_source,
                    cand.source_id.as_deref(),
                    cand.raw_json.as_deref(),
                );
            }
            Ok(UpsertOutcome::Existing { id, abstract_filled }) => {
                report.existing_papers += 1;
                if abstract_filled {
                    report.abstracts_filled += 1;
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

    Ok(new_ids)
}

fn thirty_days_ago() -> String {
    (chrono::Utc::now() - chrono::Duration::days(30)).format("%Y-%m-%d").to_string()
}

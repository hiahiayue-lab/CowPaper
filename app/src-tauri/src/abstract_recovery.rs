//! Bounded, on-demand recovery for missing/partial abstracts. It is called by
//! normal sync entry points or explicit UI commands; it is not a daemon and it
//! never calls AI services.
use chrono::{DateTime, Duration, Utc};
use rusqlite::Connection;

use crate::{api::{crossref::Crossref, openalex::OpenAlex, publisher::PublisherMetadata}, db, models::Paper};

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryReport { pub checked: i64, pub recovered: i64, pub upgraded: i64, pub remaining: i64, pub failed: i64, pub recovered_ids: Vec<i64>, pub upgraded_ids: Vec<i64> }

/// 24h → 3d → 7d → 30d; later attempts stay at a 30-day cadence rather than
/// issuing daily publisher requests forever.
pub(crate) fn retry_delay(retry_count: i64) -> Duration {
    Duration::days(match retry_count { 0 => 1, 1 => 3, 2 => 7, _ => 30 })
}

pub(crate) fn retry_due(last_checked: Option<&str>, retry_count: i64, now: DateTime<Utc>) -> bool {
    match last_checked.and_then(|s| DateTime::parse_from_rfc3339(s).ok()) {
        None => true,
        Some(last) => now >= last.with_timezone(&Utc) + retry_delay(retry_count),
    }
}

pub fn recover_due(conn: &Connection, mailto: &str, limit: usize) -> Result<RecoveryReport, String> {
    let papers = db::list_papers(conn, None, 10_000).map_err(|e| e.to_string())?;
    let now = Utc::now();
    let due = papers.into_iter().filter(|p| p.abstract_quality != "complete" && retry_due(p.abstract_last_checked_at.as_deref(), p.abstract_retry_count, now)).take(limit);
    recover_many(conn, mailto, due.collect(), |_| {})
}

pub fn recover_paper(conn: &Connection, mailto: &str, paper_id: i64) -> Result<RecoveryReport, String> {
    let paper = db::get_paper(conn, paper_id).map_err(|e| e.to_string())?.ok_or_else(|| "论文不存在".to_string())?;
    recover_many(conn, mailto, vec![paper], |_| {})
}

/// Explicit user action: retry every non-complete paper now, ignoring the
/// automatic cadence. It still uses the same bounded source sequence and never
/// starts AI work.
pub fn recover_all_with_progress<F>(conn: &Connection, mailto: &str, progress: F) -> Result<RecoveryReport, String>
where F: FnMut(RecoveryProgress) {
    let papers = db::list_papers(conn, None, 10_000).map_err(|e| e.to_string())?
        .into_iter().filter(|p| p.abstract_quality != "complete").collect();
    recover_many(conn, mailto, papers, progress)
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryProgress { pub completed: i64, pub total: i64 }

fn recover_many<F>(conn: &Connection, mailto: &str, papers: Vec<Paper>, mut progress: F) -> Result<RecoveryReport, String>
where F: FnMut(RecoveryProgress) {
    let crossref = Crossref::new(mailto);
    let openalex = OpenAlex::new(mailto);
    let publisher = PublisherMetadata::new();
    let mut report = RecoveryReport::default();
    let total = papers.iter().filter(|p| p.abstract_quality != "complete").count() as i64;
    progress(RecoveryProgress { completed: 0, total });
    for paper in papers {
        if paper.abstract_quality == "complete" { continue; }
        report.checked += 1;
        db::mark_abstract_recovery_attempt(conn, paper.id).map_err(|e| e.to_string())?;
        let before = paper.abstract_quality.clone();
        let mut source_failed = false;
        if let Some(doi) = paper.normalized_doi.as_deref() {
            match crossref.work_by_doi(doi) {
                Ok(Some(c)) => if let Some(text) = c.abstract_text { let _ = db::merge_recovered_abstract(conn, paper.id, "crossref", &text); },
                Ok(None) => {}, Err(_) => source_failed = true,
            }
            match openalex.work_by_doi(doi) {
                Ok(Some(c)) => if let Some(text) = c.abstract_text { let _ = db::merge_recovered_abstract(conn, paper.id, "openalex", &text); },
                Ok(None) => {}, Err(_) => source_failed = true,
            }
            let current = db::get_paper(conn, paper.id).map_err(|e| e.to_string())?.ok_or_else(|| "论文不存在".to_string())?;
            if current.abstract_quality != "complete" {
                match publisher.abstract_by_doi(doi) {
                    Ok(Some(text)) => { let _ = db::merge_recovered_abstract(conn, paper.id, "publisher", &text); },
                    Ok(None) => {}, Err(_) => source_failed = true,
                }
            }
        }
        let after = db::get_paper(conn, paper.id).map_err(|e| e.to_string())?.ok_or_else(|| "论文不存在".to_string())?;
        if after.abstract_quality != before { report.recovered += 1; report.recovered_ids.push(paper.id); }
        if before == "partial" && after.abstract_quality == "complete" { report.upgraded += 1; report.upgraded_ids.push(paper.id); }
        if after.abstract_quality != "complete" { report.remaining += 1; }
        if after.abstract_quality != "complete" && source_failed { report.failed += 1; }
        progress(RecoveryProgress { completed: report.checked, total });
    }
    Ok(report)
}

//! Durable public-metadata recovery. HTTP never runs under the SQLite mutex;
//! this module never invokes AI.
use std::sync::{Arc, Mutex};
use chrono::{DateTime, Duration, Utc};
use rusqlite::Connection;
use crate::{api::{crossref::Crossref, openalex::OpenAlex, publisher::PublisherMetadata}, db, models::AbstractRecoveryProgress};

pub(crate) fn retry_delay(retry_count: i64) -> Duration { Duration::days(match retry_count { 0 => 1, 1 => 3, 2 => 7, _ => 30 }) }
pub(crate) fn retry_due(last_checked: Option<&str>, retry_count: i64, now: DateTime<Utc>) -> bool {
    last_checked.and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map_or(true, |last| now >= last.with_timezone(&Utc) + retry_delay(retry_count))
}

/// Executes a persisted batch. Every database scope is deliberately short;
/// source requests happen after that scope has dropped.
pub fn run_batch<F>(db_arc: Arc<Mutex<Connection>>, batch_id: i64, mailto: &str, mut emit: F) -> Result<(), String>
where F: FnMut(AbstractRecoveryProgress) {
    let items = { let c = db_arc.lock().unwrap(); db::list_abstract_recovery_items(&c, batch_id).map_err(|e| e.to_string())? };
    let total = items.len() as i64;
    let crossref = Crossref::new(mailto); let openalex = OpenAlex::new(mailto); let publisher = PublisherMetadata::new();
    let (mut completed, mut recovered, mut not_found, mut failed) = (0, 0, 0, 0);
    emit(progress(batch_id, 0, total, None, None, "started", 0, 0, 0));
    for item in items {
        let paper = { let c = db_arc.lock().unwrap(); db::get_paper(&c, item.paper_id).map_err(|e| e.to_string())? };
        let Some(paper) = paper else { continue };
        if paper.abstract_quality == "complete" { continue; }
        { let c = db_arc.lock().unwrap(); db::mark_abstract_recovery_attempt(&c, paper.id).map_err(|e| e.to_string())?; }
        let (title, before) = (paper.title.clone(), paper.abstract_quality.clone());
        let mut network_failure = false;
        let unsupported = paper.normalized_doi.is_none();
        if let Some(doi) = paper.normalized_doi.as_deref() {
            for source in ["Crossref", "OpenAlex"] {
                { let c = db_arc.lock().unwrap(); db::start_abstract_recovery_item(&c, item.id, source).map_err(|e| e.to_string())?; }
                emit(progress(batch_id, completed, total, title.clone(), Some(source.into()), "sourceStarted", recovered, not_found, failed));
                let result = if source == "Crossref" { crossref.work_by_doi(doi).map(|v| v.and_then(|p| p.abstract_text)) } else { openalex.work_by_doi(doi).map(|v| v.and_then(|p| p.abstract_text)) };
                let (outcome, error) = match result {
                    Ok(Some(text)) => { let c = db_arc.lock().unwrap(); db::merge_recovered_abstract(&c, paper.id, &source.to_ascii_lowercase(), &text).map_err(|e| e.to_string())?; ("recovered", None) }
                    Ok(None) => ("notFound", None), Err(err) => { network_failure = true; ("networkFailure", Some(err)) }
                };
                { let c = db_arc.lock().unwrap(); db::finish_abstract_recovery_attempt(&c, item.id, source, outcome, error.as_deref()).map_err(|e| e.to_string())?; }
                emit(progress(batch_id, completed, total, title.clone(), Some(source.into()), "sourceFinished", recovered, not_found, failed));
            }
            let complete = { let c = db_arc.lock().unwrap(); db::get_paper(&c, paper.id).map_err(|e| e.to_string())?.map(|p| p.abstract_quality == "complete").unwrap_or(false) };
            if !complete {
                let source = "Publisher";
                { let c = db_arc.lock().unwrap(); db::start_abstract_recovery_item(&c, item.id, source).map_err(|e| e.to_string())?; }
                emit(progress(batch_id, completed, total, title.clone(), Some(source.into()), "sourceStarted", recovered, not_found, failed));
                let (outcome, error) = match publisher.abstract_by_doi(doi) {
                    Ok(Some(text)) => { let c = db_arc.lock().unwrap(); db::merge_recovered_abstract(&c, paper.id, "publisher", &text).map_err(|e| e.to_string())?; ("recovered", None) }
                    Ok(None) => ("notFound", None), Err(err) => { network_failure = true; ("networkFailure", Some(err)) }
                };
                { let c = db_arc.lock().unwrap(); db::finish_abstract_recovery_attempt(&c, item.id, source, outcome, error.as_deref()).map_err(|e| e.to_string())?; }
                emit(progress(batch_id, completed, total, title.clone(), Some(source.into()), "sourceFinished", recovered, not_found, failed));
            }
        }
        let after = { let c = db_arc.lock().unwrap(); db::get_paper(&c, paper.id).map_err(|e| e.to_string())?.ok_or_else(|| "论文不存在".to_string())? };
        let outcome = if after.abstract_quality != before { "recovered" } else if unsupported { "unsupported" } else if network_failure { "networkFailure" } else { "notFound" };
        let retry = (outcome != "recovered").then(|| (Utc::now() + retry_delay(paper.abstract_retry_count + 1)).to_rfc3339());
        { let c = db_arc.lock().unwrap(); db::finish_abstract_recovery_item(&c, item.id, outcome, (outcome == "networkFailure").then_some("One or more public sources were unavailable"), retry.as_deref()).map_err(|e| e.to_string())?; db::update_abstract_recovery_batch_counts(&c, batch_id).map_err(|e| e.to_string())?; }
        completed += 1; match outcome { "recovered" => recovered += 1, "networkFailure" => failed += 1, _ => not_found += 1 }
        emit(progress(batch_id, completed, total, title, None, "paperFinished", recovered, not_found, failed));
    }
    let c = db_arc.lock().unwrap();
    db::finalize_abstract_recovery_batch(&c, batch_id, if failed > 0 { "completedWithErrors" } else { "completed" }, None).map_err(|e| e.to_string())
}

fn progress(batch_id: i64, completed: i64, total: i64, title: Option<String>, source: Option<String>, phase: &str, recovered: i64, not_found: i64, failed: i64) -> AbstractRecoveryProgress {
    AbstractRecoveryProgress { batch_id, completed, total, current_paper_title: title, current_source: source, phase: phase.into(), recovered, not_found, failed, remaining: total - completed }
}

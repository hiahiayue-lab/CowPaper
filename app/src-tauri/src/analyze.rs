use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::api::deepseek::{AiError, DeepSeek};
use crate::db;
use crate::models::{Tag, TagMatch, ST_SUCCEEDED};
use crate::util::hash64;

pub const PROMPT_VERSION: &str = "v1";

/// 标签上下文：入队时快照一次，整批复用。
#[derive(Debug, Clone)]
pub struct AnalyzeContext {
    pub tag_pairs: Vec<(String, String)>,
    pub known: HashSet<String>,
}

pub fn build_context(conn: &Arc<Mutex<Connection>>) -> Option<AnalyzeContext> {
    let c = conn.lock().unwrap();
    let tags: Vec<Tag> = db::list_tags(&c).unwrap_or_default();
    let tags: Vec<Tag> = tags.into_iter().filter(|t| t.enabled).collect();
    if tags.is_empty() {
        return None;
    }
    let tag_pairs: Vec<(String, String)> = tags
        .iter()
        .map(|t| (t.name.clone(), t.description.clone().unwrap_or_default()))
        .collect();
    let known: HashSet<String> = tag_pairs.iter().map(|(n, _)| n.clone()).collect();
    Some(AnalyzeContext { tag_pairs, known })
}

/// 单篇论文的一次分析尝试（不含重试）。
/// Ok(true) = 已分析并保存；Ok(false) = 证据未变且已成功，跳过；
/// Err = 单次请求失败（是否重试由调用方决定）。
pub fn analyze_paper_once(
    conn: &Arc<Mutex<Connection>>,
    ds: &DeepSeek,
    api_key: &str,
    model: &str,
    paper_id: i64,
    title: &str,
    abstract_text: &str,
    ctx: &AnalyzeContext,
) -> Result<bool, AiError> {
    if title.is_empty() || abstract_text.is_empty() {
        return Err(AiError::Empty);
    }
    let tag_names: String = ctx
        .tag_pairs
        .iter()
        .map(|(n, _)| n.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let evidence_hash = hash64(&format!(
        "{}|{}|{}|{}",
        title, abstract_text, tag_names, PROMPT_VERSION
    ));

    // 幂等性：证据未变且已成功 → 跳过（不得重复调用 DeepSeek）。
    {
        let c = conn.lock().unwrap();
        let existing_hash = db::get_evidence_hash(&c, paper_id).unwrap_or(None);
        let status = db::get_analysis_status(&c, paper_id).unwrap_or(None);
        if existing_hash.as_deref() == Some(&evidence_hash) && status.as_deref() == Some(ST_SUCCEEDED) {
            return Ok(false);
        }
    }

    let out = ds.analyze(api_key, model, title, abstract_text, &ctx.tag_pairs)?;

    let mut tag_matches: Vec<TagMatch> = out
        .tag_matches
        .into_iter()
        .filter(|m| ctx.known.contains(&m.tag))
        .map(|m| TagMatch {
            tag: m.tag,
            score: clamp_score(m.score),
        })
        .collect();
    for (name, _) in &ctx.tag_pairs {
        if !tag_matches.iter().any(|m| &m.tag == name) {
            tag_matches.push(TagMatch {
                tag: name.clone(),
                score: 0.0,
            });
        }
    }
    let total: f64 = tag_matches.iter().map(|m| m.score).sum();
    let tag_matches_json = serde_json::to_string(&tag_matches).unwrap_or_else(|_| "[]".to_string());

    {
        let c = conn.lock().unwrap();
        db::save_analysis(
            &c,
            paper_id,
            &out.chinese_title,
            &out.chinese_abstract,
            &out.one_sentence_summary,
            &tag_matches_json,
            total,
            model,
            PROMPT_VERSION,
            &evidence_hash,
        )
        .map_err(|e| AiError::Parse(e.to_string()))?;
    }
    Ok(true)
}

fn clamp_score(s: f64) -> f64 {
    let buckets = [0.0_f64, 0.2, 0.4, 0.6, 0.8, 1.0];
    let mut best = 0.0;
    let mut best_d = f64::MAX;
    for &x in &buckets {
        let d = (x - s).abs();
        if d < best_d {
            best_d = d;
            best = x;
        }
    }
    best
}

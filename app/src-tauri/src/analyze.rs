use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::api::deepseek::{AiError, DeepSeek};
use crate::db;
use crate::models::{Tag, TagMatch, ST_SUCCEEDED};
use crate::util::hash64;

pub const PROMPT_VERSION: &str = "v1";

// ================= 不变式（Round 7 Phase 1，Section 15）=================
// CowPaper 永远不能：title → DeepSeek → AI 生成 abstract → 保存成真实 abstract。
// - 两个 AI 入口（analyze_paper_once / tag_only_analyze）都要求真实摘要非空，
//   否则直接拒绝 —— 缺少摘要的论文只能走 title-only 翻译（只写 chinese_title）。
// - AI 未来只允许：classification / parsing assistance / version matching assistance，
//   绝不生成缺失摘要。title-only 翻译永远不写 abstract / chinese_abstract。
// ======================================================================

/// 标签上下文：入队时快照一次，整批复用（仅包含当前启用标签 = canonical set）。
/// 每项 (tag_id, name, description)——Full AI 保存时必须写 tag identity（Round small fix）。
#[derive(Debug, Clone)]
pub struct AnalyzeContext {
    pub tag_pairs: Vec<(i64, String, String)>,
}

pub fn build_context(conn: &Arc<Mutex<Connection>>) -> Option<AnalyzeContext> {
    let c = conn.lock().unwrap();
    let tags: Vec<Tag> = db::list_tags(&c).unwrap_or_default();
    let tags: Vec<Tag> = tags.into_iter().filter(|t| t.enabled).collect();
    if tags.is_empty() {
        return None;
    }
    let tag_pairs: Vec<(i64, String, String)> = tags
        .iter()
        .map(|t| (t.id, t.name.clone(), t.description.clone().unwrap_or_default()))
        .collect();
    Some(AnalyzeContext { tag_pairs })
}

/// 以本地启用的标签集合（canonical set）规范化 AI 返回的 tagMatches：
/// 1. 只保留存在于 canonical 集合的标签（未知 / 已禁用标签被丢弃，不信任 AI 返回列表）；
/// 2. 相同标签出现多次时取最高合法分（绝不对重复项求和）；
/// 3. score 钳制到合法档位 {0.0, 0.2, 0.4, 0.6, 0.8, 1.0}；
/// 4. canonical 中 AI 未返回的标签补 0.0，保证集合完整。
/// totalScore 必须由 Rust 基于该规范化结果求和。
pub fn normalize_tag_matches(
    ai_matches: Vec<TagMatch>,
    tag_pairs: &[(i64, String, String)],
) -> Vec<TagMatch> {
    tag_pairs
        .iter()
        .map(|(id, name, desc)| {
            let score = ai_matches
                .iter()
                .filter(|m| m.tag == *name)
                .map(|m| clamp_score(m.score))
                .fold(0.0_f64, |acc, s| acc.max(s));
            TagMatch {
                tag: name.clone(),
                score,
                tag_id: Some(*id),
                semantic_hash: Some(crate::tag_config::tag_semantic_hash(*id, name, desc)),
            }
        })
        .collect()
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
    abstract_quality: &str,
    ctx: &AnalyzeContext,
) -> Result<bool, AiError> {
    if title.trim().is_empty() || abstract_text.trim().is_empty() {
        return Err(AiError::Paper("缺少标题或摘要".to_string()));
    }
    let tag_names: String = ctx
        .tag_pairs
        .iter()
        .map(|(_, n, _)| n.as_str())
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

    let prompt_tags: Vec<(String, String)> = ctx
        .tag_pairs
        .iter()
        .map(|(_, n, d)| (n.clone(), d.clone()))
        .collect();
    let out = ds.analyze(api_key, model, title, abstract_text, abstract_quality, &prompt_tags)?;

    // 规范化：canonical 唯一标签集 + 最高合法分（重复 tag 不求和）。
    let tag_matches = normalize_tag_matches(out.tag_matches, &ctx.tag_pairs);
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
        .map_err(|e| AiError::Paper(e.to_string()))?;
        // totalScore 必须与当前有效（active+hash 匹配）评分一致：以统一规则本地重算
        if let Ok(active) = crate::tag_config::active_tags(&c) {
            let _ = crate::tag_config::recompute_paper_total_score(&c, paper_id, &active);
        }
    }
    Ok(true)
}

/// Tag-only 增量评分（多 tag 一次请求；只更新 requested tags，其余保留）。
/// 返回 (tag_id, score)。
pub fn tag_only_analyze(
    conn: &Arc<Mutex<Connection>>,
    ds: &DeepSeek,
    api_key: &str,
    model: &str,
    paper_id: i64,
    title: &str,
    abstract_text: &str,
    abstract_quality: &str,
    tags: &[(i64, String, String)],
) -> Result<Vec<(i64, f64)>, AiError> {
    // 不变式：没有真实摘要绝不调用 AI（防止任何 title→AI→abstract 路径）。
    if title.trim().is_empty() || abstract_text.trim().is_empty() {
        return Err(AiError::Paper("缺少标题或摘要".to_string()));
    }
    let id_strs: Vec<(String, String, String)> = tags
        .iter()
        .map(|(id, n, d)| (id.to_string(), n.clone(), d.clone()))
        .collect();
    let out = ds.analyze_tags(api_key, model, title, abstract_text, abstract_quality, &id_strs)?;
    // 只保留 requested tags（trust only requested set），clamp 到合法档位
    let mut scores: Vec<(i64, f64)> = Vec::new();
    for (id_str, score) in out {
        if let Ok(id) = id_str.parse::<i64>() {
            if tags.iter().any(|(tid, _, _)| *tid == id) {
                scores.push((id, clamp_score(score)));
            }
        }
    }
    {
        let c = conn.lock().unwrap();
        db::set_paper_tag_scores(&c, paper_id, &scores, tags).map_err(|e| AiError::Paper(e.to_string()))?;
    }
    Ok(scores)
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

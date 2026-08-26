//! Round 5B：Abstract Quality 判定与 Canonical Abstract 选择。
//!
//! 全部为本地确定性规则（不调用任何 LLM / 网络）：
//! - `assess_abstract_quality`：判定 complete / partial / missing，并给出可解释 reason。
//! - `select_canonical_abstract`：在多来源候选中选择 canonical 摘要。
//!   质量优先于来源优先级；同质量内来源优先级 + 更长更完整者胜出；
//!   禁止"complete 被 partial 降级"。
//! - 比较/判定一律基于 normalized plain text（HTML/JATS 已清洗、空白折叠），
//!   避免"HTML 字符多 = 更完整"的误判。

use crate::models::{ABQ_COMPLETE, ABQ_MISSING, ABQ_PARTIAL};
use crate::util::strip_html;

/// 摘要质量 reason（第一版保持简单、可解释，不做 ML confidence）。
pub const REASON_MISSING: &str = "missing";
pub const REASON_ELLIPSIS: &str = "ellipsis_truncated";
pub const REASON_VERY_SHORT: &str = "very_short_incomplete_sentence";
pub const REASON_TRUNCATED_SENTENCE: &str = "truncated_sentence";
pub const REASON_FULL_TEXT: &str = "full_text_like_abstract";
pub const REASON_MULTI_SOURCE: &str = "multi_source_agreement";

/// 清洗为可比较的纯文本：去 HTML/JATS 标签、解码实体、折叠空白与重复换行。
/// 判定与比较前必须经过本函数，避免标签字符数误导质量判断。
pub fn normalize_abstract_text(raw: &str) -> String {
    strip_html(raw)
}

fn ends_with_punct(t: &str) -> bool {
    let c = t.trim_end().chars().last().unwrap_or(' ');
    matches!(c, '.' | '。' | '!' | '！' | '?' | '？' | ';' | '；')
}

/// 本地确定性质量判定。输入应为 normalized 纯文本。
/// 返回 (quality, reason)。
///
/// missing：空 / 清洗后无正文。
/// partial：以省略号（ASCII 或 Unicode）截断；极短且句法明显不完整（<25 词且无结尾标点）；
///          较长文本无结尾标点且以介词/连词等"半句"结尾。
/// complete：无截断证据且结构正常。短但完整的摘要（70–100 词）不被误判为 partial。
pub fn assess_abstract_quality(normalized: &str) -> (&'static str, &'static str) {
    let t = normalized.trim();
    if t.is_empty() {
        return (ABQ_MISSING, REASON_MISSING);
    }
    // 省略号截断（ASCII / Unicode / 中文省略号）
    if t.ends_with("...") || t.ends_with("…") || t.ends_with("⋯") {
        return (ABQ_PARTIAL, REASON_ELLIPSIS);
    }
    let words = t.split_whitespace().count();
    if words < 25 {
        if !ends_with_punct(t) {
            return (ABQ_PARTIAL, REASON_VERY_SHORT);
        }
        return (ABQ_COMPLETE, REASON_FULL_TEXT);
    }
    if !ends_with_punct(t) {
        let last = t
            .split_whitespace()
            .last()
            .unwrap_or("")
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_ascii_lowercase();
        if matches!(
            last.as_str(),
            "of" | "to" | "in" | "on" | "for" | "with" | "and" | "the" | "a" | "an" | "at" | "by"
                | "from" | "than" | "that" | "which" | "while"
        ) {
            return (ABQ_PARTIAL, REASON_TRUNCATED_SENTENCE);
        }
    }
    (ABQ_COMPLETE, REASON_FULL_TEXT)
}

/// 来源优先级（次级规则，仅在 quality 相同时使用）。
/// quality 永远优先于来源优先级；publisher 若只是营销 teaser 会被 quality 规则排除。
pub fn source_priority(source: &str) -> u8 {
    match source.to_ascii_lowercase().as_str() {
        "publisher" => 0,
        "crossref" => 1,
        "openalex" => 2,
        "rss" => 3,
        _ => 4,
    }
}

/// 单来源摘要候选（text 已 normalized）。
#[derive(Debug, Clone)]
pub struct AbstractCandidate {
    pub source: String,
    pub text: String,
    pub quality: String,
    pub reason: String,
}

/// 在多个来源候选中选择 canonical 摘要：
/// 1. 剔除空候选；
/// 2. quality 最高者优先（complete > partial > missing）；
/// 3. 同 quality：若 A 是 B 的明显前缀（normalized 后），移除 A（B 更完整）；
/// 4. 同 quality 且无前缀关系：来源优先级更可靠者优先，其次更长。
/// 绝不从 high quality 降级到 low quality。
pub fn select_canonical_abstract(candidates: Vec<AbstractCandidate>) -> Option<AbstractCandidate> {
    let rank = |q: &str| -> u8 {
        match q {
            ABQ_COMPLETE => 2,
            ABQ_PARTIAL => 1,
            _ => 0,
        }
    };
    let mut pool: Vec<AbstractCandidate> = candidates
        .into_iter()
        .filter(|c| !c.text.trim().is_empty())
        .collect();
    if pool.is_empty() {
        return None;
    }
    pool.sort_by(|a, b| rank(&b.quality).cmp(&rank(&a.quality)));
    let best_rank = rank(&pool[0].quality);
    pool.retain(|c| rank(&c.quality) == best_rank);
    // 前缀消解：normalized 后 A 是 B 的严格前缀 → 移除 A（B 更完整）
    let mut i = 0;
    while i < pool.len() {
        let a = pool[i].text.trim();
        let is_prefix = pool.iter().enumerate().any(|(j, b)| {
            j != i && {
                let bt = b.text.trim();
                a != bt && bt.starts_with(a) && a.len() < bt.len()
            }
        });
        if is_prefix {
            pool.remove(i);
        } else {
            i += 1;
        }
    }
    if pool.is_empty() {
        return None;
    }
    // 同 quality：来源优先级，其次更长
    pool.sort_by(|a, b| {
        source_priority(&a.source)
            .cmp(&source_priority(&b.source))
            .then(b.text.len().cmp(&a.text.len()))
    });
    let mut best = pool.remove(0);
    if best.reason == REASON_FULL_TEXT && !pool.is_empty() {
        best.reason = REASON_MULTI_SOURCE.to_string();
    }
    Some(best)
}

//! Round 7 Phase 1：Missing Abstract Intelligence —— 内容类型与摘要语义状态。
//!
//! 目标：把「没有摘要」从单一状态拆分为语义状态：
//! - `content_kind`：论文内容类型（research_article / review / editorial / ... / unknown）
//! - `abstract_status`：available / missing_recoverable / not_expected / unknown
//!
//! 全部为本地确定性规则（不调用任何 LLM / 网络）：
//! - 证据优先级：provider explicit publication type（Crossref）→ trusted structured
//!   metadata（OpenAlex）→ 可靠 title heuristic（弱证据）→ unknown。
//! - **关键语义（Correctness Fix）**：Crossref `journal-article` 与 OpenAlex `article`
//!   都是 broad container type —— 它们同时覆盖 news / editorial / letter / correction
//!   等真实内容，**绝不能作为 research_article 的证据**（否则 Nature News 等会被错误
//!   提升为 research_article）。只有 provider 明确细分的类型（review-article、editorial、
//!   letter、correction、journal-issue、paratext …）才贡献 content_kind。
//! - 原则：wrong high-confidence classification 比 temporary unknown 更糟。
//!   Phase 1 允许大量 unknown；research_article 只有存在足够明确证据时才赋值。
//! - title heuristic 只允许返回 LOW confidence，绝不作为不可逆最终事实。
//! - letter 保守处理：Crossref 显式 letter 就标 letter（EXACT）→ abstract_status
//!   not_expected；OpenAlex broad article 不构成「研究型 letter」升级证据。
//!   真正的「研究型 letter」识别留待 Phase 2（Semantic Scholar publicationTypes）。

/// 内容类型常量（存储值 = 小写英文，UI 自行中文化，不直接暴露技术字段）。
pub const CK_RESEARCH_ARTICLE: &str = "research_article";
pub const CK_REVIEW: &str = "review";
pub const CK_EDITORIAL: &str = "editorial";
pub const CK_COMMENTARY: &str = "commentary";
pub const CK_LETTER: &str = "letter";
pub const CK_CORRECTION: &str = "correction";
pub const CK_NEWS: &str = "news";
pub const CK_BOOK_REVIEW: &str = "book_review";
pub const CK_FRONT_MATTER: &str = "front_matter";
pub const CK_UNKNOWN: &str = "unknown";

/// 摘要语义状态（abstract_status）。
pub const ABST_AVAILABLE: &str = "available";
pub const ABST_MISSING_RECOVERABLE: &str = "missing_recoverable";
pub const ABST_NOT_EXPECTED: &str = "not_expected";
pub const ABST_UNKNOWN: &str = "unknown";

/// 分类置信度。
pub const CONF_EXACT: &str = "EXACT";
pub const CONF_HIGH: &str = "HIGH";
pub const CONF_LOW: &str = "LOW";
pub const CONF_UNKNOWN: &str = "UNKNOWN";

/// 默认「不期待研究摘要」的内容类型（recovery 与 recommendation 均排除）。
pub const NOT_EXPECTED_KINDS: &[&str] = &[
    CK_NEWS,
    CK_EDITORIAL,
    CK_COMMENTARY,
    CK_CORRECTION,
    CK_FRONT_MATTER,
    CK_BOOK_REVIEW,
];

/// 单次解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentKindResolution {
    pub kind: String,
    pub source: String,
    pub confidence: String,
}

impl ContentKindResolution {
    pub fn unknown() -> Self {
        Self {
            kind: CK_UNKNOWN.to_string(),
            source: "none".to_string(),
            confidence: CONF_UNKNOWN.to_string(),
        }
    }
}

/// 从 raw discovery JSON 中提取 provider explicit type。
/// Crossref work message 用大写 "DOI" 键；OpenAlex work 用 "authorships" / "abstract_inverted_index"。
/// 返回 (provider, type)。
pub fn provider_type_from_raw_json(raw: &str) -> Option<(&'static str, String)> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let ty = v.get("type").and_then(|t| t.as_str())?;
    if v.get("DOI").is_some() {
        Some(("crossref", ty.to_string()))
    } else if v.get("authorships").is_some() || v.get("abstract_inverted_index").is_some() {
        Some(("openalex", ty.to_string()))
    } else {
        Some(("unknown", ty.to_string()))
    }
}

/// Crossref provider type → content kind。
/// **只有明确细分类型才贡献分类**：`journal-article` 是 broad container type，
/// 同时覆盖 news / editorial / letter 等，绝不映射 research_article。
/// 其余未列出的 broad type（proceedings-article / posted-content / dissertation …）
/// 同样不贡献 —— 宁可 unknown。
pub fn crossref_kind(ty: &str) -> Option<&'static str> {
    Some(match ty {
        "review-article" => CK_REVIEW,
        "book-review" => CK_BOOK_REVIEW,
        "news" => CK_NEWS,
        "editorial" => CK_EDITORIAL,
        "letter" => CK_LETTER,
        "correction" | "erratum" | "retraction" => CK_CORRECTION,
        "journal-issue" | "journal-volume" => CK_FRONT_MATTER,
        _ => return None,
    })
}

/// OpenAlex provider type → content kind。
/// `article` 与 Crossref `journal-article` 同理是 broad type，不单独等价
/// research_article；只有明确细分类型贡献分类。
pub fn openalex_kind(ty: &str) -> Option<&'static str> {
    Some(match ty {
        "review" => CK_REVIEW,
        "editorial" => CK_EDITORIAL,
        "letter" => CK_LETTER,
        "news" => CK_NEWS,
        "correction" | "erratum" | "retraction" => CK_CORRECTION,
        "book-review" => CK_BOOK_REVIEW,
        "paratext" => CK_FRONT_MATTER,
        _ => return None,
    })
}

/// 统一解析：provider 显式细分类型优先，title heuristic 仅作弱证据。
/// - Crossref 显式细分类型（EXACT）。
/// - 否则 OpenAlex 显式细分类型（HIGH）。
/// - 否则可靠 title heuristic（LOW）。
/// - 否则 unknown。
/// broad type（journal-article / article）永远不产生 research_article。
pub fn resolve_content_kind(
    crossref_type: Option<&str>,
    openalex_type: Option<&str>,
    title: Option<&str>,
) -> ContentKindResolution {
    // 1) Crossref 显式细分类型（EXACT）。letter 保持 letter：
    //    OpenAlex broad article 不构成「研究型 letter」升级证据。
    if let Some(t) = crossref_type {
        if let Some(kind) = crossref_kind(t) {
            return ContentKindResolution {
                kind: kind.to_string(),
                source: "crossref:type".to_string(),
                confidence: CONF_EXACT.to_string(),
            };
        }
    }
    // 2) OpenAlex 显式细分类型（HIGH）。
    if let Some(t) = openalex_type {
        if let Some(kind) = openalex_kind(t) {
            return ContentKindResolution {
                kind: kind.to_string(),
                source: "openalex:type".to_string(),
                confidence: CONF_HIGH.to_string(),
            };
        }
    }
    // 3) 可靠 title heuristic（弱证据，LOW）。
    if let Some(kind) = title_kind(title) {
        return ContentKindResolution {
            kind: kind.to_string(),
            source: "title-heuristic".to_string(),
            confidence: CONF_LOW.to_string(),
        };
    }
    ContentKindResolution::unknown()
}

/// 弱 title heuristic。只识别几乎无歧义的期刊栏目/通知标题；
/// 避免把 "introduction" / "news" 之类可能出现在研究标题中的词误分类。
/// 低置信度不覆盖任何 provider 证据。
fn title_kind(title: Option<&str>) -> Option<&'static str> {
    let t = title?.trim().to_ascii_lowercase();
    let t = t.trim_end_matches(|c| matches!(c, '.' | '!' | '?' | '。'));
    let exact_or_colon = |patterns: &[&str]| {
        patterns
            .iter()
            .any(|p| t == *p || t.starts_with(&format!("{}:", p)))
    };
    if exact_or_colon(&["editorial", "editorials"]) {
        return Some(CK_EDITORIAL);
    }
    if t.starts_with("correction to:")
        || exact_or_colon(&["correction", "erratum", "retraction", "retraction notice", "addendum"])
    {
        return Some(CK_CORRECTION);
    }
    if t.starts_with("book review")
        || t.starts_with("book reviews")
        || t.starts_with("review of the book")
        || t.starts_with("review of the edited book")
    {
        return Some(CK_BOOK_REVIEW);
    }
    if exact_or_colon(&[
        "front matter",
        "back matter",
        "table of contents",
        "issue information",
        "masthead",
        "cover",
        "index",
        "author index",
        "subject index",
        "publication information",
    ]) {
        return Some(CK_FRONT_MATTER);
    }
    if exact_or_colon(&["letter to the editor", "letters to the editor"]) {
        return Some(CK_LETTER);
    }
    if t.starts_with("commentary")
        || t.starts_with("commentaries")
        || t.starts_with("a comment on")
        || t.starts_with("comments on")
        || t.starts_with("comment on")
    {
        return Some(CK_COMMENTARY);
    }
    if t == "news" || t == "news and views" {
        return Some(CK_NEWS);
    }
    None
}

/// 该内容类型是否「默认不期待研究摘要」。
pub fn is_not_expected_kind(kind: &str) -> bool {
    NOT_EXPECTED_KINDS.contains(&kind)
}

/// 由 content_kind + 是否已有真实摘要推导 abstract_status。
/// - 已有摘要 → available（无论类型）。
/// - 无摘要：
///   research_article / review → missing_recoverable
///   news / editorial / commentary / correction / front_matter / book_review → not_expected
///   letter → not_expected（保守；研究型 letter 的进一步识别留待 Phase 2）
///   unknown → unknown（保持旧行为：仍可尝试 recovery，不误标）
pub fn abstract_status_for(kind: &str, has_abstract: bool) -> &'static str {
    if has_abstract {
        return ABST_AVAILABLE;
    }
    match kind {
        CK_RESEARCH_ARTICLE | CK_REVIEW => ABST_MISSING_RECOVERABLE,
        CK_LETTER => ABST_NOT_EXPECTED,
        CK_NEWS | CK_EDITORIAL | CK_COMMENTARY | CK_CORRECTION | CK_FRONT_MATTER | CK_BOOK_REVIEW => {
            ABST_NOT_EXPECTED
        }
        _ => ABST_UNKNOWN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crossref_explicit_type_wins() {
        let r = resolve_content_kind(Some("news"), None, Some("A Deep Dive Into Pricing"));
        assert_eq!(r.kind, CK_NEWS);
        assert_eq!(r.confidence, CONF_EXACT);
    }

    #[test]
    fn test_openalex_fallback() {
        let r = resolve_content_kind(None, Some("review"), Some("Platform Economics"));
        assert_eq!(r.kind, CK_REVIEW);
        assert_eq!(r.confidence, CONF_HIGH);
    }

    #[test]
    fn test_crossref_journal_article_is_not_research_evidence() {
        // Correctness Fix：journal-article 是 broad container type，绝不能 → research_article
        let r = resolve_content_kind(Some("journal-article"), None, None);
        assert_eq!(r.kind, CK_UNKNOWN, "journal-article 不得映射 research_article");
        assert_eq!(r.confidence, CONF_UNKNOWN);
        let r2 = resolve_content_kind(Some("journal-article"), Some("article"), None);
        assert_eq!(r2.kind, CK_UNKNOWN, "journal-article + article 双 broad 证据仍为 unknown");
    }

    #[test]
    fn test_openalex_article_is_not_research_evidence() {
        let r = resolve_content_kind(None, Some("article"), None);
        assert_eq!(r.kind, CK_UNKNOWN, "OpenAlex article 不得映射 research_article");
        assert_eq!(r.confidence, CONF_UNKNOWN);
    }

    #[test]
    fn test_letter_stays_letter_with_broad_openalex_article() {
        // Correctness Fix：Crossref letter + OpenAlex article 不再自动升级 research_article
        let r = resolve_content_kind(Some("letter"), Some("article"), None);
        assert_eq!(r.kind, CK_LETTER, "Crossref 显式 letter 保持 letter");
        assert_eq!(r.confidence, CONF_EXACT);
        assert_eq!(abstract_status_for(&r.kind, false), ABST_NOT_EXPECTED);
    }

    #[test]
    fn test_letter_stays_conservative() {
        let r = resolve_content_kind(Some("letter"), Some("letter"), None);
        assert_eq!(r.kind, CK_LETTER);
        assert_eq!(abstract_status_for(&r.kind, false), ABST_NOT_EXPECTED);
    }

    #[test]
    fn test_explicit_editorial_and_review() {
        let r = resolve_content_kind(Some("editorial"), Some("article"), None);
        assert_eq!(r.kind, CK_EDITORIAL);
        assert_eq!(r.confidence, CONF_EXACT);
        let r2 = resolve_content_kind(Some("review-article"), Some("article"), None);
        assert_eq!(r2.kind, CK_REVIEW);
        let r3 = resolve_content_kind(None, Some("review"), Some("A Study"));
        assert_eq!(r3.kind, CK_REVIEW);
        assert_eq!(r3.confidence, CONF_HIGH);
    }

    #[test]
    fn test_title_heuristic_is_low_confidence() {
        let r = resolve_content_kind(None, None, Some("Editorial: A New Era"));
        assert_eq!(r.kind, CK_EDITORIAL);
        assert_eq!(r.confidence, CONF_LOW);
    }

    #[test]
    fn test_ambiguous_title_stays_unknown() {
        let r = resolve_content_kind(None, None, Some("An Introduction to Platform Pricing"));
        assert_eq!(r.kind, CK_UNKNOWN, "introduction 不能通过 heuristic 误分类");
        assert_eq!(r.confidence, CONF_UNKNOWN);
    }

    #[test]
    fn test_abstract_status_matrix() {
        assert_eq!(abstract_status_for(CK_RESEARCH_ARTICLE, false), ABST_MISSING_RECOVERABLE);
        assert_eq!(abstract_status_for(CK_REVIEW, false), ABST_MISSING_RECOVERABLE);
        assert_eq!(abstract_status_for(CK_NEWS, false), ABST_NOT_EXPECTED);
        assert_eq!(abstract_status_for(CK_EDITORIAL, false), ABST_NOT_EXPECTED);
        assert_eq!(abstract_status_for(CK_CORRECTION, false), ABST_NOT_EXPECTED);
        assert_eq!(abstract_status_for(CK_FRONT_MATTER, false), ABST_NOT_EXPECTED);
        assert_eq!(abstract_status_for(CK_BOOK_REVIEW, false), ABST_NOT_EXPECTED);
        assert_eq!(abstract_status_for(CK_LETTER, false), ABST_NOT_EXPECTED);
        assert_eq!(abstract_status_for(CK_UNKNOWN, false), ABST_UNKNOWN);
        // 有真实摘要 → 一律 available
        for k in [CK_NEWS, CK_EDITORIAL, CK_RESEARCH_ARTICLE, CK_UNKNOWN] {
            assert_eq!(abstract_status_for(k, true), ABST_AVAILABLE);
        }
    }
}

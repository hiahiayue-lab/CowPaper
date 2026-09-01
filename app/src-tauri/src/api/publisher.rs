//! Public publisher landing-page metadata only. This never downloads article
//! PDFs, authenticates, or attempts to bypass a paywall / CAPTCHA / bot
//! protection. Requests are rate-limited (≥ 1s between landing-page fetches).
//!
//! Round 7 Phase 1 additions:
//! - nature.com / link.springer.com 加入合法 public-metadata fallback
//!   （Audit：6/6 与 3/3 成功，均来自公开结构化 dc.description）。
//! - 摘要身份验证：只有当页面明确携带与目标 DOI 一致的 identity evidence
//!   （citation_doi / dc.identifier meta，或最终落地页 URL 含目标 DOI）时才采用摘要；
//!   否则拒绝 —— 绝不允许「标题相似 → 把网页摘要写给目标 paper」。
use std::sync::Mutex;
use std::time::Instant;

use reqwest::blocking::{Client, Response};
use reqwest::redirect::Policy;

const ALLOWED_HOSTS: &[&str] = &[
    "informs.org", "onlinelibrary.wiley.com", "sciencedirect.com", "elsevier.com",
    "journals.sagepub.com", "academic.oup.com", "journals.aom.org",
    "nature.com", "link.springer.com",
];

/// 单次 publisher recovery 结果：来源标识（publisher:nature / publisher:springer /
/// publisher）+ 落地页 URL（provenance）+ 摘要文本。
#[derive(Debug, Clone)]
pub struct PublisherMetadataResult {
    pub source: String,
    pub url: String,
    pub abstract_text: String,
}

pub struct PublisherMetadata {
    client: Client,
    last_request: Mutex<Option<Instant>>,
}

impl PublisherMetadata {
    pub fn new() -> Self {
        Self {
            client: Client::builder().user_agent("CowPaper/0.1 public-metadata")
                .connect_timeout(std::time::Duration::from_secs(10)).timeout(std::time::Duration::from_secs(20))
                .redirect(Policy::limited(5)).build().expect("build publisher http client"),
            last_request: Mutex::new(None),
        }
    }

    /// 访问 DOI 落地页并尝试提取验证过的公开摘要。
    /// - Ok(Some(meta))：找到并已通过 DOI identity 验证。
    /// - Ok(None)：host 不在白名单 / 非成功响应 / 无可用元数据 / identity 不匹配。
    /// - Err：网络/传输错误（真实失败，应视为 networkFailure）。
    pub fn abstract_by_doi(&self, doi: &str) -> Result<Option<PublisherMetadataResult>, String> {
        self.rate_limit();
        let resp = self.client.get(format!("https://doi.org/{}", doi)).send()
            .map_err(|e| format!("DOI landing page 请求失败: {}", e))?;
        self.extract_from_response(resp, doi)
    }

    /// 相邻 landing-page 请求至少间隔 1s（限速，遵守 publisher 公开页面礼貌要求）。
    fn rate_limit(&self) {
        let mut last = self.last_request.lock().unwrap();
        if let Some(prev) = *last {
            let elapsed = prev.elapsed();
            if elapsed < std::time::Duration::from_secs(1) {
                std::thread::sleep(std::time::Duration::from_secs(1) - elapsed);
            }
        }
        *last = Some(Instant::now());
    }

    fn extract_from_response(&self, resp: Response, target_doi: &str) -> Result<Option<PublisherMetadataResult>, String> {
        let host = resp.url().host_str().unwrap_or("").to_ascii_lowercase();
        if !ALLOWED_HOSTS.iter().any(|suffix| host == *suffix || host.ends_with(&format!(".{}", suffix))) {
            return Ok(None);
        }
        if !resp.status().is_success() { return Ok(None); }
        let final_url = resp.url().to_string();
        let html = resp.text().map_err(|e| format!("publisher 页面读取失败: {}", e))?;
        // 摘要身份验证：页面 DOI identity 必须与目标 DOI 一致，否则拒绝。
        if !page_identity_matches(&html, &final_url, target_doi) {
            return Ok(None);
        }
        let source = match host.as_str() {
            h if h == "nature.com" || h.ends_with(".nature.com") => "publisher:nature".to_string(),
            h if h == "link.springer.com" || h.ends_with(".link.springer.com") => "publisher:springer".to_string(),
            _ => "publisher".to_string(),
        };
        let text = match extract_public_abstract(&html) {
            Some(t) => t,
            None => return Ok(None),
        };
        Ok(Some(PublisherMetadataResult {
            source,
            url: final_url,
            abstract_text: text,
        }))
    }
}

/// DOI identity evidence：页面 meta 中明确声明的 DOI（citation_doi / dc.identifier 等）。
/// 只接受显式 meta 声明 —— 正文/链接中的 doi.org URL 可能是参考文献，不是身份证据。
pub(crate) fn page_doi(html: &str) -> Option<String> {
    for key in [
        "citation_doi",
        "dc.identifier",
        "dcterms.identifier",
        "dcsext.publicationDOI",
        "bepress_citation_doi",
    ] {
        if let Some(v) = meta_content(html, key) {
            if let Some(doi) = normalize_doi_candidate(&v) {
                return Some(doi);
            }
        }
    }
    None
}

/// 页面 DOI identity 是否与目标 DOI 一致：
/// 1. page_doi(html) 与 target 完全一致；
/// 2. 或最终落地页 URL 包含 target DOI（或其 URL 编码形式，如 Springer 路径式 DOI）。
pub(crate) fn page_identity_matches(html: &str, final_url: &str, target_doi: &str) -> bool {
    if let Some(page) = page_doi(html) {
        // Explicit page metadata is authoritative: a mismatch must reject the
        // page even when a redirect URL happens to contain the target DOI.
        return page == target_doi.to_ascii_lowercase();
    }
    let target = target_doi.to_ascii_lowercase();
    if final_url.to_ascii_lowercase().contains(&target) {
        return true;
    }
    // Springer 等使用 %2F 编码斜杠的路径式 DOI
    let encoded = target.replace('/', "%2f");
    let encoded_upper = target.replace('/', "%2F");
    let lower_url = final_url.to_ascii_lowercase();
    lower_url.contains(&encoded) || lower_url.contains(&encoded_upper)
}

fn normalize_doi_candidate(v: &str) -> Option<String> {
    let s = v.trim();
    let s = s.strip_prefix("doi:").unwrap_or(s).trim();
    crate::util::normalize_doi(s)
}

/// Conservative generic parser for explicit metadata fields only. It does not
/// use title/body text and therefore cannot manufacture an abstract.
/// 优先级：citation_abstract → dc.description / dcterms.description → 通用 description
/// （通用 description 是弱候选，仅在页面 DOI identity 已验证后才会被采用 ——
/// 身份验证由调用方 page_identity_matches 负责）。
pub(crate) fn extract_public_abstract(html: &str) -> Option<String> {
    for key in ["citation_abstract", "dc.description", "dcterms.description", "description"] {
        if let Some(value) = meta_content(html, key) {
            let text = crate::abstract_quality::normalize_abstract_text(&value);
            if !text.is_empty() { return Some(text); }
        }
    }
    // JSON-LD commonly exposes a top-level \"abstract\" string.
    for marker in ["\"abstract\"", "\"description\""] {
        if let Some(start) = html.find(marker) {
            let tail = &html[start + marker.len()..];
            if let Some(colon) = tail.find(':') {
                let rest = tail[colon + 1..].trim_start();
                if let Some(rest) = rest.strip_prefix('\"') {
                    if let Some(end) = rest.find('\"') {
                        let text = crate::abstract_quality::normalize_abstract_text(&rest[..end]);
                        if !text.is_empty() { return Some(text); }
                    }
                }
            }
        }
    }
    None
}

fn meta_content(html: &str, key: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let needle = key.to_ascii_lowercase();
    let pos = lower.find(&needle)?;
    let tag_start = lower[..pos].rfind("<meta")?;
    let tag_end = lower[pos..].find('>')? + pos;
    let tag = &html[tag_start..=tag_end];
    let low = tag.to_ascii_lowercase();
    let content = low.find("content=")? + "content=".len();
    let quote = tag[content..].chars().next()?;
    if quote != '\'' && quote != '\"' { return None; }
    let value = &tag[content + quote.len_utf8()..];
    let end = value.find(quote)?;
    Some(value[..end].to_string())
}

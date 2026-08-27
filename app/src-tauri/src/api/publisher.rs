//! Public publisher landing-page metadata only. This never downloads article
//! PDFs, authenticates, or attempts to bypass a paywall.
use reqwest::blocking::{Client, Response};
use reqwest::redirect::Policy;

const ALLOWED_HOSTS: &[&str] = &[
    "informs.org", "onlinelibrary.wiley.com", "sciencedirect.com", "elsevier.com",
    "journals.sagepub.com", "academic.oup.com", "journals.aom.org",
];

pub struct PublisherMetadata { client: Client }

impl PublisherMetadata {
    pub fn new() -> Self {
        Self { client: Client::builder().user_agent("CowPaper/0.1 public-metadata")
            .connect_timeout(std::time::Duration::from_secs(10)).timeout(std::time::Duration::from_secs(20))
            .redirect(Policy::limited(5)).build().expect("build publisher http client") }
    }

    pub fn abstract_by_doi(&self, doi: &str) -> Result<Option<String>, String> {
        let resp = self.client.get(format!("https://doi.org/{}", doi)).send()
            .map_err(|e| format!("DOI landing page 请求失败: {}", e))?;
        self.extract_from_response(resp)
    }

    fn extract_from_response(&self, resp: Response) -> Result<Option<String>, String> {
        let host = resp.url().host_str().unwrap_or("").to_ascii_lowercase();
        if !ALLOWED_HOSTS.iter().any(|suffix| host == *suffix || host.ends_with(&format!(".{}", suffix))) {
            return Ok(None);
        }
        if !resp.status().is_success() { return Ok(None); }
        let html = resp.text().map_err(|e| format!("publisher 页面读取失败: {}", e))?;
        Ok(extract_public_abstract(&html))
    }
}

/// Conservative generic parser for explicit metadata fields only. It does not
/// use title/body text and therefore cannot manufacture an abstract.
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

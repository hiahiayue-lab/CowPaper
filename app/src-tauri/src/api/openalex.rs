use reqwest::blocking::Client;
use serde_json::{Map, Value};

use crate::models::{Author, PaperCandidate};
use crate::util::{extract_year, normalize_doi};

pub struct OpenAlex {
    client: Client,
    mailto: String,
}

/// Stable journal identity returned by OpenAlex for an ISSN lookup.  The ISSN
/// family is intentionally retained: a source can represent both print and
/// online editions even when Crossref only exposes one of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAlexSourceIdentity {
    pub source_id: String,
    pub issn_l: Option<String>,
    pub issns: Vec<String>,
}

impl OpenAlex {
    pub fn new(mailto: &str) -> Self {
        let client = Client::builder()
            .user_agent(format!("CowPaper/0.1 (mailto:{})", mailto))
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .expect("build openalex http client");
        OpenAlex {
            client,
            mailto: mailto.to_string(),
        }
    }

    /// 通过 ISSN 查找期刊 Source ID（去掉 https://openalex.org/ 前缀）。
    pub fn source_by_issn(&self, issn: &str) -> Result<Option<String>, String> {
        Ok(self.source_identity_by_issn(issn)?.map(|identity| identity.source_id))
    }

    /// Resolve the complete OpenAlex journal identity for one ISSN.  `Ok(None)`
    /// means no matching source; transport and service failures stay errors so
    /// callers never mistake an unavailable source for an identity conflict.
    pub fn source_identity_by_issn(&self, issn: &str) -> Result<Option<OpenAlexSourceIdentity>, String> {
        let url = format!("https://api.openalex.org/sources?filter=issn:{}&mailto={}", issn, self.mailto);
        let resp = self
            .client
            .get(&url)
            .send()
            .map_err(|e| format!("OpenAlex source 查询失败: {}", e))?;
        if !resp.status().is_success() {
            return Err(format!("OpenAlex source HTTP {}", resp.status().as_u16()));
        }
        let v: Value = resp
            .json()
            .map_err(|e| format!("OpenAlex source 响应解析失败: {}", e))?;
        let Some(source) = v
            .get("results")
            .and_then(|r| r.as_array())
            .and_then(|r| r.first()) else {
            return Ok(None);
        };
        let Some(source_id) = source
            .get("id")
            .and_then(|id| id.as_str())
            .map(|id| id.replace("https://openalex.org/", "")) else {
            return Ok(None);
        };
        let mut issns = source
            .get("issn")
            .and_then(|values| values.as_array())
            .map(|values| values.iter().filter_map(|value| value.as_str())
                .filter_map(crate::util::normalize_issn).collect::<Vec<_>>())
            .unwrap_or_default();
        issns.sort();
        issns.dedup();
        Ok(Some(OpenAlexSourceIdentity {
            source_id,
            issn_l: source.get("issn_l").and_then(|value| value.as_str())
                .and_then(crate::util::normalize_issn),
            issns,
        }))
    }

    /// 按 source_id 查询近期 works。
    /// Ok(Some(v)) = 调用成功（可能为空列表：该窗口无数据，如 OpenAlex 对 HBR 覆盖停止于 2021）；
    /// Ok(None) = source 不存在/无记录；Err = 网络/服务错误（视为真实失败，不得伪装成 unsupported）。
    pub fn works(&self, source_id: &str, from: &str, to: &str) -> Result<Option<Vec<PaperCandidate>>, String> {
        let url = format!(
            "https://api.openalex.org/works?filter=primary_location.source.id:{},from_publication_date:{},to_publication_date:{}&sort=publication_date:desc&per-page=200&mailto={}",
            source_id, from, to, self.mailto
        );
        let resp = self
            .client
            .get(&url)
            .send()
            .map_err(|e| format!("OpenAlex 请求失败: {}", e))?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            return Err(format!("OpenAlex HTTP {}", resp.status().as_u16()));
        }
        let v: Value = resp.json().map_err(|e| format!("OpenAlex 响应解析失败: {}", e))?;
        let Some(items) = v.get("results").and_then(|a| a.as_array()).cloned() else {
            return Ok(None);
        };
        Ok(Some(items.iter().filter_map(parse_work).collect()))
    }

    /// Re-check one DOI for a later OpenAlex abstract without a journal-wide query.
    pub fn work_by_doi(&self, doi: &str) -> Result<Option<PaperCandidate>, String> {
        let resp = self.client.get("https://api.openalex.org/works")
            .query(&[("filter", format!("doi:https://doi.org/{}", doi)), ("per-page", "1".to_string()), ("mailto", self.mailto.clone())])
            .send().map_err(|e| format!("OpenAlex DOI 请求失败: {}", e))?;
        if !resp.status().is_success() { return Err(format!("OpenAlex DOI HTTP {}", resp.status().as_u16())); }
        let v: Value = resp.json().map_err(|e| format!("OpenAlex DOI 响应解析失败: {}", e))?;
        Ok(v.get("results").and_then(|x| x.as_array()).and_then(|x| x.first()).and_then(parse_work).filter(|c| c.normalized_doi == normalize_doi(doi)))
    }
}

pub(crate) fn parse_work(item: &Value) -> Option<PaperCandidate> {
    let original_doi = item.get("doi").and_then(|d| d.as_str()).map(str::to_string);
    let normalized_doi = original_doi.as_deref().and_then(normalize_doi);
    let title = item.get("title").and_then(|t| t.as_str()).map(str::to_string);
    let published_date = item
        .get("publication_date")
        .and_then(|d| d.as_str())
        .map(str::to_string);
    let year = published_date.as_deref().and_then(extract_year);
    let abstract_text = item
        .get("abstract_inverted_index")
        .and_then(|a| a.as_object())
        .map(reconstruct_abstract)
        .filter(|s| !s.is_empty());
    let url = item
        .get("primary_location")
        .and_then(|pl| pl.get("landing_page_url"))
        .and_then(|u| u.as_str())
        .map(str::to_string);
    let openalex_work_id = item
        .get("id")
        .and_then(|i| i.as_str())
        .map(|s| s.replace("https://openalex.org/", ""));
    let authors = item
        .get("authorships")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    let name = a
                        .get("author")
                        .and_then(|au| au.get("display_name"))
                        .and_then(|n| n.as_str())
                        .map(str::to_string);
                    name.map(|n| Author {
                        given: None,
                        family: None,
                        name: Some(n),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Some(PaperCandidate {
        normalized_doi,
        original_doi: original_doi.clone(),
        title,
        authors,
        published_date,
        year,
        abstract_text: abstract_text.clone(),
        abstract_source: abstract_text.map(|_| "openalex".to_string()),
        abstract_source_url: None,
        url,
        publisher_article_id: None,
        openalex_work_id: openalex_work_id.clone(),
        discovery_source: "openalex".to_string(),
        source_id: openalex_work_id,
        raw_json: Some(item.to_string()),
    })
}

fn reconstruct_abstract(inv: &Map<String, Value>) -> String {
    let mut positions: Vec<(i64, &str)> = Vec::new();
    for (word, idxs) in inv {
        if let Some(arr) = idxs.as_array() {
            for i in arr {
                if let Some(p) = i.as_i64() {
                    positions.push((p, word.as_str()));
                }
            }
        }
    }
    positions.sort_by_key(|(p, _)| *p);
    positions
        .into_iter()
        .map(|(_, w)| w)
        .collect::<Vec<_>>()
        .join(" ")
}

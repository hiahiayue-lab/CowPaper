use reqwest::blocking::Client;
use serde_json::Value;

use crate::models::{Author, PaperCandidate};
use crate::util::{extract_year, normalize_doi, strip_html};

#[derive(Debug, Clone)]
pub struct JournalMeta {
    pub title: String,
    pub publisher: Option<String>,
    pub print_issn: Option<String>,
    pub online_issn: Option<String>,
    /// ISSN-L（linking ISSN），Crossref journal 端点提供
    pub issn_l: Option<String>,
}

pub struct CrossrefWorks {
    #[allow(dead_code)] // 预留用于 rows 截断检测
    pub total: i64,
    pub candidates: Vec<PaperCandidate>,
}

pub struct Crossref {
    client: Client,
    mailto: String,
}

impl Crossref {
    pub fn new(mailto: &str) -> Self {
        let client = Client::builder()
            .user_agent(format!("CowPaper/0.1 (mailto:{})", mailto))
            // A source outage must not hold the whole (sequential) journal batch
            // indefinitely. `timeout` covers the complete request/response body;
            // `connect_timeout` makes a broken route fail promptly.
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .expect("build crossref http client");
        Crossref {
            client,
            mailto: mailto.to_string(),
        }
    }

    pub fn journal_meta(&self, issn: &str) -> Option<JournalMeta> {
        let url = format!("https://api.crossref.org/journals/{}", issn);
        let v: Value = self.client.get(&url).send().ok()?.json().ok()?;
        let m = v.get("message")?;
        let title = m.get("title")?.as_str()?.to_string();
        let publisher = m.get("publisher").and_then(|p| p.as_str()).map(str::to_string);
        let mut print_issn = None;
        let mut online_issn = None;
        if let Some(arr) = m.get("issn-type").and_then(|a| a.as_array()) {
            for t in arr {
                let ty = t.get("type").and_then(|x| x.as_str());
                let val = t.get("value").and_then(|x| x.as_str()).map(str::to_string);
                match ty {
                    Some("print") => print_issn = val,
                    Some("electronic") => online_issn = val,
                    _ => {}
                }
            }
        }
        Some(JournalMeta {
            title,
            publisher,
            print_issn,
            online_issn,
            // Crossref 字段名为大写 "ISSN-L"；兼容小写变体
            issn_l: m
                .get("ISSN-L")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .or_else(|| m.get("issn-l").and_then(|v| v.as_str()).map(str::to_string)),
        })
    }

    /// 按期刊名检索候选 ISSN（优先 print ISSN）。
    pub fn search_issns(&self, name: &str) -> Option<Vec<String>> {
        let v: Value = self
            .client
            .get("https://api.crossref.org/journals")
            .query(&[("query", name), ("rows", "5")])
            .send()
            .ok()?
            .json()
            .ok()?;
        let items = v.get("message")?.get("items")?.as_array()?;
        let mut out = Vec::new();
        for it in items {
            let mut print = None;
            let mut elec = None;
            if let Some(arr) = it.get("issn-type").and_then(|a| a.as_array()) {
                for t in arr {
                    let ty = t.get("type").and_then(|x| x.as_str());
                    let val = t.get("value").and_then(|x| x.as_str()).map(str::to_string);
                    match ty {
                        Some("print") => print = val,
                        Some("electronic") => elec = val,
                        _ => {}
                    }
                }
            }
            if let Some(p) = print.or(elec) {
                if !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    /// 按 ISSN 查询期刊 works。
    /// Ok(Some(w)) = 有记录；Ok(None) = 该 ISSN 在 Crossref 无期刊记录（404 / 无数据，如 HBR）；
    /// Err = 网络/服务错误（应视为同步失败，而不是"无记录"）。
    pub fn works(&self, issn: &str, from: &str, to: &str) -> Result<Option<CrossrefWorks>, String> {
        let url = format!(
            "https://api.crossref.org/journals/{}/works?filter=from-pub-date:{},until-pub-date:{}&sort=published&order=desc&rows=200&mailto={}",
            issn, from, to, self.mailto
        );
        let resp = self
            .client
            .get(&url)
            .send()
            .map_err(|e| format!("Crossref 请求失败: {}", e))?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None); // 无该 ISSN 的期刊记录（HBR 等）
        }
        if !resp.status().is_success() {
            return Err(format!("Crossref HTTP {}", resp.status().as_u16()));
        }
        let v: Value = resp.json().map_err(|e| format!("Crossref 响应解析失败: {}", e))?;
        let Some(msg) = v.get("message") else {
            return Ok(None);
        };
        let total = msg.get("total-results").and_then(|t| t.as_i64()).unwrap_or(0);
        let items = msg
            .get("items")
            .and_then(|a| a.as_array())
            .cloned()
            .unwrap_or_default();
        let candidates = items.iter().filter_map(parse_work).collect();
        Ok(Some(CrossrefWorks { total, candidates }))
    }

    /// Re-check one already discovered work without widening a journal sync.
    pub fn work_by_doi(&self, doi: &str) -> Result<Option<PaperCandidate>, String> {
        let url = format!("https://api.crossref.org/works/{}", doi);
        let resp = self.client.get(&url).send().map_err(|e| format!("Crossref DOI 请求失败: {}", e))?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND { return Ok(None); }
        if !resp.status().is_success() { return Err(format!("Crossref DOI HTTP {}", resp.status().as_u16())); }
        let v: Value = resp.json().map_err(|e| format!("Crossref DOI 响应解析失败: {}", e))?;
        Ok(v.get("message").and_then(parse_work).filter(|c| c.normalized_doi == normalize_doi(doi)))
    }
}

pub(crate) fn parse_work(item: &Value) -> Option<PaperCandidate> {
    let original_doi = item.get("DOI").and_then(|d| d.as_str()).map(str::to_string);
    let normalized_doi = original_doi.as_deref().and_then(normalize_doi);
    let title = item
        .get("title")
        .and_then(|t| t.as_array())
        .and_then(|a| a.first())
        .and_then(|t| t.as_str())
        .map(str::to_string);
    let authors = parse_authors(item);
    let (published_date, year) = parse_date(item);
    let abstract_text = item
        .get("abstract")
        .and_then(|a| a.as_str())
        .map(strip_html)
        .filter(|s| !s.is_empty());
    let url = item
        .get("URL")
        .and_then(|u| u.as_str())
        .map(str::to_string)
        .or_else(|| {
            item.get("link")
                .and_then(|l| l.as_array())
                .and_then(|a| a.first())
                .and_then(|l| l.get("URL"))
                .and_then(|u| u.as_str())
                .map(str::to_string)
        });
    let publisher_article_id = item
        .get("alternative-id")
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .and_then(|a| a.as_str())
        .map(str::to_string);

    Some(PaperCandidate {
        normalized_doi,
        original_doi: original_doi.clone(),
        title,
        authors,
        published_date,
        year,
        abstract_text: abstract_text.clone(),
        abstract_source: abstract_text.map(|_| "crossref".to_string()),
        abstract_source_url: None,
        url,
        publisher_article_id,
        openalex_work_id: None,
        discovery_source: "crossref".to_string(),
        source_id: original_doi,
        raw_json: Some(item.to_string()),
    })
}

fn parse_authors(item: &Value) -> Vec<Author> {
    item.get("author")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    let given = a.get("given").and_then(|g| g.as_str()).map(str::to_string);
                    let family = a.get("family").and_then(|f| f.as_str()).map(str::to_string);
                    let name = a.get("name").and_then(|n| n.as_str()).map(str::to_string);
                    if given.is_none() && family.is_none() && name.is_none() {
                        None
                    } else {
                        Some(Author { given, family, name })
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_date(item: &Value) -> (Option<String>, Option<i32>) {
    for key in ["published-print", "published-online", "issued", "published"] {
        if let Some(d) = item.get(key) {
            if let Some(dp) = d
                .get("date-parts")
                .and_then(|x| x.as_array())
                .and_then(|a| a.first())
                .and_then(|p| p.as_array())
            {
                if let Some(y) = dp.first().and_then(|y| y.as_i64()) {
                    let year = y as i32;
                    let month = dp.get(1).and_then(|m| m.as_i64()).unwrap_or(1);
                    let day = dp.get(2).and_then(|dd| dd.as_i64()).unwrap_or(1);
                    let date = match dp.len() {
                        1 => format!("{year:04}"),
                        2 => format!("{year:04}-{month:02}"),
                        _ => format!("{year:04}-{month:02}-{day:02}"),
                    };
                    return (Some(date), Some(year));
                }
            } else if let Some(ds) = d.get("date").and_then(|x| x.as_str()) {
                let year = extract_year(ds);
                return (Some(ds.to_string()), year);
            }
        }
    }
    (None, None)
}

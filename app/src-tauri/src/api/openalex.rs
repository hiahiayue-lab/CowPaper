use reqwest::blocking::Client;
use serde_json::{Map, Value};

use crate::models::{Author, PaperCandidate};
use crate::util::{extract_year, normalize_doi};

pub struct OpenAlex {
    client: Client,
    mailto: String,
}

impl OpenAlex {
    pub fn new(mailto: &str) -> Self {
        let client = Client::builder()
            .user_agent(format!("CowPaper/0.1 (mailto:{})", mailto))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("build openalex http client");
        OpenAlex {
            client,
            mailto: mailto.to_string(),
        }
    }

    /// 通过 ISSN 查找期刊 Source ID（去掉 https://openalex.org/ 前缀）。
    pub fn source_by_issn(&self, issn: &str) -> Option<String> {
        let url = format!("https://api.openalex.org/sources?filter=issn:{}&mailto={}", issn, self.mailto);
        let v: Value = self.client.get(&url).send().ok()?.json().ok()?;
        let id = v.get("results")?.as_array()?.first()?.get("id")?.as_str()?;
        Some(id.replace("https://openalex.org/", ""))
    }

    pub fn works(&self, source_id: &str, from: &str, to: &str) -> Option<Vec<PaperCandidate>> {
        let url = format!(
            "https://api.openalex.org/works?filter=primary_location.source.id:{},from_publication_date:{},to_publication_date:{}&sort=publication_date:desc&per-page=200&mailto={}",
            source_id, from, to, self.mailto
        );
        let v: Value = self.client.get(&url).send().ok()?.json().ok()?;
        let items = v.get("results")?.as_array()?.clone();
        Some(items.iter().filter_map(parse_work).collect())
    }
}

fn parse_work(item: &Value) -> Option<PaperCandidate> {
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

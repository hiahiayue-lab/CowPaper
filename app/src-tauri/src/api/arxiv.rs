use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::blocking::Client;

use crate::models::{Author, PaperCandidate};
use crate::util::{extract_year, normalize_doi};

/// Exact arXiv metadata lookup. The identifier is already normalized by the
/// PDF recovery layer, so this endpoint is never queried from a fuzzy title
/// match.
pub struct Arxiv {
    client: Client,
}

impl Arxiv {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .user_agent("CowPaper/0.4.2")
                .connect_timeout(std::time::Duration::from_secs(10))
                .timeout(std::time::Duration::from_secs(20))
                .build()
                .expect("build arXiv http client"),
        }
    }

    pub fn work_by_id(&self, id: &str) -> Option<PaperCandidate> {
        let url = format!("https://export.arxiv.org/api/query?id_list={id}");
        let body = self.client.get(url).send().ok()?.text().ok()?;
        parse_entry(&body, id)
    }
}

fn parse_entry(xml: &str, id: &str) -> Option<PaperCandidate> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut current: Option<String> = None;
    let mut title = None;
    let mut summary = None;
    let mut published = None;
    let mut doi = None;
    let mut journal_ref = None;
    let mut authors = Vec::new();
    let mut in_author = false;
    let mut author_name = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                let name =
                    String::from_utf8_lossy(event.local_name().as_ref()).to_ascii_lowercase();
                if name == "entry" || name == "feed" {
                    continue;
                }
                if name == "author" {
                    in_author = true;
                    author_name = None;
                } else {
                    current = Some(name);
                }
            }
            Ok(Event::Text(event)) => {
                let value = String::from_utf8_lossy(event.as_ref()).trim().to_string();
                if value.is_empty() {
                    continue;
                }
                if in_author && current.as_deref() == Some("name") {
                    author_name = Some(value);
                } else {
                    match current.as_deref() {
                        Some("title") => title = Some(value),
                        Some("summary") => summary = Some(value),
                        Some("published") => published = Some(value),
                        Some("doi") => doi = Some(value),
                        Some("journal_ref") => journal_ref = Some(value),
                        _ => {}
                    }
                }
            }
            Ok(Event::End(event)) => {
                let name =
                    String::from_utf8_lossy(event.local_name().as_ref()).to_ascii_lowercase();
                if name == "author" {
                    if let Some(name) = author_name.take() {
                        authors.push(author_from_display_name(&name));
                    }
                    in_author = false;
                }
                current = None;
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    let title = title?.trim().to_string();
    if title.is_empty() {
        return None;
    }
    let normalized_doi = doi.as_deref().and_then(normalize_doi);
    let raw_json = journal_ref.map(|journal| {
        serde_json::json!({
            "DOI": normalized_doi.clone().unwrap_or_default(),
            "container-title": [journal],
        })
        .to_string()
    });
    Some(PaperCandidate {
        normalized_doi: normalized_doi.clone(),
        original_doi: doi,
        title: Some(title),
        authors,
        published_date: published.clone(),
        year: published.as_deref().and_then(extract_year),
        abstract_text: summary,
        abstract_source: Some("arxiv".to_string()),
        abstract_source_url: Some(format!("https://arxiv.org/abs/{id}")),
        url: Some(format!("https://arxiv.org/abs/{id}")),
        publisher_article_id: Some(format!("arxiv:{id}")),
        openalex_work_id: None,
        discovery_source: "arxiv".to_string(),
        source_id: Some(id.to_string()),
        raw_json,
    })
}

fn author_from_display_name(name: &str) -> Author {
    let mut parts = name.split_whitespace();
    let family = parts.next_back().map(str::to_string);
    let given = {
        let value = parts.collect::<Vec<_>>().join(" ");
        (!value.is_empty()).then_some(value)
    };
    Author {
        given,
        family,
        name: Some(name.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_entry;

    #[test]
    fn parses_exact_arxiv_metadata() {
        let xml = r#"<feed xmlns="http://www.w3.org/2005/Atom"><entry><title> A deterministic paper </title><author><name>Ada Lovelace</name></author><published>2025-01-02T00:00:00Z</published><summary>Provider abstract.</summary><arxiv:doi xmlns:arxiv="http://arxiv.org/schemas/atom">10.5555/example</arxiv:doi><arxiv:journal_ref xmlns:arxiv="http://arxiv.org/schemas/atom">Research Journal 1</arxiv:journal_ref></entry></feed>"#;
        let candidate = parse_entry(xml, "2501.12345").unwrap();
        assert_eq!(candidate.title.as_deref(), Some("A deterministic paper"));
        assert_eq!(candidate.year, Some(2025));
        assert_eq!(candidate.abstract_source.as_deref(), Some("arxiv"));
        assert_eq!(
            candidate.publisher_article_id.as_deref(),
            Some("arxiv:2501.12345")
        );
        assert_eq!(candidate.normalized_doi.as_deref(), Some("10.5555/example"));
        assert!(candidate
            .raw_json
            .as_deref()
            .unwrap()
            .contains("Research Journal 1"));
    }
}

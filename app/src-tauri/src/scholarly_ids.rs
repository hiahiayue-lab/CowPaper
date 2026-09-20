//! Deterministic identifiers embedded in working-paper PDFs.
//!
//! These helpers intentionally recognize only explicit identifiers.  They are
//! used for exact identity reconciliation and never infer identity from a
//! title or an author list.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScholarlyIds {
    pub arxiv: Option<String>,
    pub ssrn: Option<String>,
}

fn trim_identifier(value: &str) -> &str {
    value
        .trim()
        .trim_matches(|ch: char| ".,;:!?)]}>\"'".contains(ch))
}

fn is_digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_arxiv_id(value: &str) -> bool {
    let Some((prefix, suffix)) = value.split_once('.') else {
        let Some((category, number)) = value.split_once('/') else {
            return false;
        };
        return !category.is_empty()
            && category
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
            && number.len() == 7
            && is_digits(number);
    };
    prefix.len() == 4
        && is_digits(prefix)
        && (suffix.len() == 4 || suffix.len() == 5)
        && is_digits(suffix)
}

/// Return the versionless canonical arXiv identity.
pub fn normalize_arxiv_id(value: &str) -> Option<String> {
    let lower = value.trim().to_ascii_lowercase();
    let mut candidate = lower.as_str();
    for marker in ["arxiv.org/abs/", "arxiv.org/pdf/", "arxiv:"] {
        if let Some(index) = candidate.find(marker) {
            candidate = &candidate[index + marker.len()..];
            break;
        }
    }
    candidate = candidate.trim_start_matches(['/', ' ']);
    let end = candidate
        .find(|ch: char| ch.is_whitespace() || matches!(ch, '?' | '#' | ')' | ']' | '}' | '>'))
        .unwrap_or(candidate.len());
    let candidate = trim_identifier(&candidate[..end]).trim_end_matches(".pdf");
    let base = candidate
        .rsplit_once('v')
        .filter(|(_, version)| !version.is_empty() && is_digits(version))
        .map(|(base, _)| base)
        .unwrap_or(candidate);
    valid_arxiv_id(base).then(|| base.to_string())
}

/// Return a namespaced exact SSRN identity.  Namespacing prevents an arXiv
/// number and an SSRN number from ever colliding in the shared paper column.
pub fn normalize_ssrn_id(value: &str) -> Option<String> {
    let lower = value.trim().to_ascii_lowercase();
    let marker_candidates = [
        "10.2139/ssrn.",
        "ssrn-id",
        "ssrn id",
        "abstract_id=",
        "abstractid=",
    ];
    for marker in marker_candidates {
        if let Some(index) = lower.find(marker) {
            let start = index + marker.len();
            let digits: String = lower[start..]
                .chars()
                .skip_while(|ch| !ch.is_ascii_digit())
                .take_while(|ch| ch.is_ascii_digit())
                .collect();
            if !digits.is_empty() {
                return Some(format!("ssrn:{digits}"));
            }
        }
    }
    None
}

pub fn extract_ids(text: &str) -> ScholarlyIds {
    let mut ids = ScholarlyIds::default();
    let lower = text.to_ascii_lowercase();
    for marker in ["arxiv:", "arxiv.org/abs/", "arxiv.org/pdf/"] {
        let mut offset = 0;
        while let Some(found) = lower[offset..].find(marker) {
            let start = offset + found;
            let tail = &text[start..];
            if let Some(id) = normalize_arxiv_id(tail) {
                ids.arxiv = Some(id);
                break;
            }
            offset = start + marker.len();
        }
        if ids.arxiv.is_some() {
            break;
        }
    }
    for marker in ["10.2139/ssrn.", "ssrn-id", "ssrn id", "abstract_id="] {
        if lower.contains(marker) && normalize_ssrn_id(text).is_some() {
            ids.ssrn = normalize_ssrn_id(text);
            break;
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::{extract_ids, normalize_arxiv_id, normalize_ssrn_id};

    #[test]
    fn arxiv_versions_share_one_identity() {
        assert_eq!(
            normalize_arxiv_id("arXiv:2501.12345v2").as_deref(),
            Some("2501.12345")
        );
        assert_eq!(
            normalize_arxiv_id("https://arxiv.org/pdf/2501.12345v1.pdf").as_deref(),
            Some("2501.12345")
        );
        assert_eq!(
            normalize_arxiv_id("hep-th/9901001v3").as_deref(),
            Some("hep-th/9901001")
        );
    }

    #[test]
    fn ssrn_explicit_identifiers_are_namespaced() {
        assert_eq!(
            normalize_ssrn_id("https://papers.ssrn.com/sol3/papers.cfm?abstract_id=1234567")
                .as_deref(),
            Some("ssrn:1234567")
        );
        assert_eq!(
            normalize_ssrn_id("10.2139/ssrn.1234567").as_deref(),
            Some("ssrn:1234567")
        );
    }

    #[test]
    fn extraction_does_not_guess_from_titles() {
        assert_eq!(
            extract_ids("A working paper about 2501.12345 without an arXiv marker").arxiv,
            None
        );
        assert_eq!(
            extract_ids("arXiv:2501.12345v2 SSRN-id1234567")
                .arxiv
                .as_deref(),
            Some("2501.12345")
        );
        assert_eq!(
            extract_ids("arXiv:2501.12345v2 SSRN-id1234567")
                .ssrn
                .as_deref(),
            Some("ssrn:1234567")
        );
    }
}

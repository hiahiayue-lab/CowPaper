/// DOI 规范化（需求书 §8.1）：转小写、去前缀、去跟踪参数。
pub fn normalize_doi(raw: &str) -> Option<String> {
    let mut s = raw.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    for p in [
        "https://doi.org/",
        "http://doi.org/",
        "https://dx.doi.org/",
        "http://dx.doi.org/",
    ] {
        if let Some(rest) = s.strip_prefix(p) {
            s = rest.to_string();
            break;
        }
    }
    if let Some(rest) = s.strip_prefix("doi:") {
        s = rest.trim().to_string();
    }
    if let Some(i) = s.find(|c| c == '?' || c == '#') {
        s.truncate(i);
    }
    let s = s.trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// 去掉 XML/HTML 标签并解码常见实体，折叠空白（用于摘要展示）。
pub fn strip_html(input: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in input.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let decoded = out
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 从日期字符串提取年份（YYYY 前缀）。
pub fn extract_year(date: &str) -> Option<i32> {
    date.trim().chars().take(4).collect::<String>().parse().ok()
}

/// FNV-1a 64 位哈希，用于「证据指纹」（幂等性），返回 16 位十六进制字符串。
pub fn hash64(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", h)
}

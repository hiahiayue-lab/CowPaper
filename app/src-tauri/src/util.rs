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

/// ISSN 规范化（Round 5A）：接受 `0025-1909` / `00251909` / 带空格 / 小写 x 等，
/// 统一为内部 canonical 形式 `NNNN-NNNX`（校验位大写 X）。
/// 校验 ISSN-8 checksum（前 7 位加权 8..2，mod 11，余 10 → 'X'）。
/// 非法/校验失败的输入返回 None（不得进入 canonical identifiers）。
pub fn normalize_issn(raw: &str) -> Option<String> {
    let digits: Vec<char> = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if digits.len() != 8 {
        return None;
    }
    for (i, c) in digits.iter().enumerate() {
        if i < 7 {
            if !c.is_ascii_digit() {
                return None;
            }
        } else if !(c.is_ascii_digit() || *c == 'X') {
            return None;
        }
    }
    let mut sum: u32 = 0;
    for (i, c) in digits[..7].iter().enumerate() {
        sum += c.to_digit(10).unwrap() * (8 - i as u32);
    }
    let check = (11 - (sum % 11)) % 11;
    let expect = if check == 10 { 'X' } else { char::from_digit(check, 10).unwrap() };
    if digits[7] != expect {
        return None;
    }
    Some(format!(
        "{}{}{}{}-{}{}{}{}",
        digits[0], digits[1], digits[2], digits[3], digits[4], digits[5], digits[6], digits[7]
    ))
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

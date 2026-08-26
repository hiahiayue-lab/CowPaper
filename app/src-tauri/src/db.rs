use rusqlite::{params, Connection, OptionalExtension, Result};
use std::path::Path;

use crate::models::{
    AnalysisBatch, AnalysisBatchItem, Author, Journal, Paper, PaperCandidate, SyncBatch,
    SyncBatchPaper, Tag, TagMatch, UpsertOutcome, IDT_ONLINE, IDT_PRINT, ST_PENDING,
    ST_SUCCEEDED, ST_WAITING_ABSTRACT,
};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS journals (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    print_issn TEXT,
    online_issn TEXT,
    publisher TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    priority INTEGER NOT NULL DEFAULT 0,
    rss_url TEXT,
    openalex_source_id TEXT,
    publisher_adapter TEXT,
    last_successful_sync_at TEXT,
    last_paper_date TEXT,
    coverage_status TEXT,
    abstract_coverage_rate REAL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS papers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    journal_id INTEGER NOT NULL REFERENCES journals(id) ON DELETE CASCADE,
    normalized_doi TEXT,
    original_doi TEXT,
    title TEXT,
    title_norm TEXT,
    authors_json TEXT,
    published_date TEXT,
    year INTEGER,
    abstract TEXT,
    abstract_source TEXT,
    abstract_retrieved_at TEXT,
    url TEXT,
    publisher_article_id TEXT,
    openalex_work_id TEXT,
    discovery_source TEXT,
    analysis_status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_papers_journal ON papers(journal_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_papers_norm_doi ON papers(normalized_doi) WHERE normalized_doi IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_papers_openalex ON papers(openalex_work_id) WHERE openalex_work_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_papers_title_norm ON papers(journal_id, year, title_norm);

CREATE TABLE IF NOT EXISTS source_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    paper_id INTEGER NOT NULL REFERENCES papers(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    source_id TEXT,
    raw_json TEXT,
    retrieved_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    description TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS app_state (
    key TEXT PRIMARY KEY,
    value TEXT
);
"#;

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

/// 当前 schema 版本（Round 5B：abstract_quality / paper_abstract_sources 为 v4）。
/// 生产构建中仅由迁移系统隐式使用；测试中直接断言。
#[allow(dead_code)]
pub const SCHEMA_VERSION: i64 = 5;

pub fn init(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)?;
    run_migrations(conn)?;
    seed_default_tags(conn)?;
    Ok(())
}

pub fn now_utc() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn normalize_title(t: &str) -> String {
    t.chars()
        .filter_map(|c| {
            if c.is_alphanumeric() {
                Some(c.to_ascii_lowercase())
            } else {
                None
            }
        })
        .collect()
}

// ---------- Journals ----------

pub fn insert_journal(
    conn: &Connection,
    name: &str,
    print_issn: Option<&str>,
    online_issn: Option<&str>,
    publisher: Option<&str>,
    openalex_source_id: Option<&str>,
) -> Result<i64> {
    let now = now_utc();
    conn.execute(
        "INSERT INTO journals (name, print_issn, online_issn, publisher, enabled, priority, openalex_source_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 1, 0, ?5, ?6, ?6)",
        params![name, print_issn, online_issn, publisher, openalex_source_id, now],
    )?;
    Ok(conn.last_insert_rowid())
}

fn row_to_journal(row: &rusqlite::Row) -> Result<Journal> {
    Ok(Journal {
        id: row.get("id")?,
        name: row.get("name")?,
        print_issn: row.get("print_issn")?,
        online_issn: row.get("online_issn")?,
        issn_l: row.get("issn_l")?,
        publisher: row.get("publisher")?,
        enabled: row.get::<_, i64>("enabled")? != 0,
        priority: row.get("priority")?,
        rss_url: row.get("rss_url")?,
        openalex_source_id: row.get("openalex_source_id")?,
        publisher_adapter: row.get("publisher_adapter")?,
        last_successful_sync_at: row.get("last_successful_sync_at")?,
        last_paper_date: row.get("last_paper_date")?,
        coverage_status: row.get("coverage_status")?,
        abstract_coverage_rate: row.get("abstract_coverage_rate")?,
        paper_count: row.get("paper_count")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        identifiers: Vec::new(),
        collections: Vec::new(),
        possible_duplicate: false,
        metadata_needs_review: row.get("metadata_needs_review").unwrap_or(false),
    })
}

pub fn list_journals(conn: &Connection) -> Result<Vec<Journal>> {
    let mut stmt = conn.prepare(
        "SELECT j.*, (SELECT COUNT(*) FROM papers p WHERE p.journal_id = j.id) AS paper_count
         FROM journals j ORDER BY j.enabled DESC, j.priority DESC, j.name ASC",
    )?;
    let mut journals: Vec<Journal> = stmt.query_map([], row_to_journal)?.collect::<Result<Vec<_>>>()?;
    enrich_journals(conn, &mut journals)?;
    Ok(journals)
}

pub fn get_journal(conn: &Connection, id: i64) -> Result<Option<Journal>> {
    let mut j = conn
        .query_row(
            "SELECT j.*, (SELECT COUNT(*) FROM papers p WHERE p.journal_id = j.id) AS paper_count
             FROM journals j WHERE j.id = ?1",
            params![id],
            row_to_journal,
        )
        .optional()?;
    if let Some(j) = j.as_mut() {
        let mut v = vec![j.clone()];
        enrich_journals(conn, &mut v)?;
        *j = v.remove(0);
    }
    Ok(j)
}

/// 给 journals 填充 identifiers / collections / possible_duplicate（一次查询，避免 N+1）。
fn enrich_journals(conn: &Connection, journals: &mut [Journal]) -> Result<()> {
    {
        let mut stmt = conn.prepare(
            "SELECT id, journal_id, identifier_type, value, source, created_at, updated_at
             FROM journal_identifiers ORDER BY identifier_type, value",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(crate::models::JournalIdentifier {
                id: r.get(0)?,
                journal_id: r.get(1)?,
                identifier_type: r.get(2)?,
                value: r.get(3)?,
                source: r.get(4)?,
                created_at: r.get(5)?,
                updated_at: r.get(6)?,
            })
        })?;
        for idf in rows {
            let idf = idf?;
            if let Some(j) = journals.iter_mut().find(|j| j.id == idf.journal_id) {
                j.identifiers.push(idf);
            }
        }
    }
    {
        let mut stmt = conn.prepare(
            "SELECT m.journal_id, c.code FROM journal_collection_members m
             JOIN journal_collections c ON c.id = m.collection_id ORDER BY c.code",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (jid, code) = row?;
            if let Some(j) = journals.iter_mut().find(|j| j.id == jid) {
                j.collections.push(code);
            }
        }
    }
    let dup_ids = possible_duplicate_journal_ids(conn)?;
    for j in journals.iter_mut() {
        j.possible_duplicate = dup_ids.contains(&j.id);
    }
    Ok(())
}

/// 疑似重复期刊：共享 ISSN-L 或相同规范化标题的期刊组（只标记，不自动合并）。
fn possible_duplicate_journal_ids(conn: &Connection) -> Result<std::collections::HashSet<i64>> {
    use std::collections::{HashMap, HashSet};
    let mut out: HashSet<i64> = HashSet::new();
    {
        let mut stmt = conn.prepare(
            "SELECT issn_l FROM journals WHERE issn_l IS NOT NULL AND issn_l != '' GROUP BY issn_l HAVING COUNT(*) > 1",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for issn_l in rows {
            let issn_l = issn_l?;
            let mut stmt2 = conn.prepare("SELECT id FROM journals WHERE issn_l = ?1")?;
            let ids = stmt2.query_map(params![issn_l], |r| r.get::<_, i64>(0))?;
            for id in ids {
                out.insert(id?);
            }
        }
    }
    {
        let mut stmt = conn.prepare("SELECT id, name FROM journals")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        let mut by_title: HashMap<String, Vec<i64>> = HashMap::new();
        for row in rows {
            let (id, name) = row?;
            let key: String = name
                .chars()
                .filter(|c| c.is_alphanumeric())
                .map(|c| c.to_ascii_lowercase())
                .collect();
            if key.is_empty() {
                continue;
            }
            by_title.entry(key).or_default().push(id);
        }
        for v in by_title.values() {
            if v.len() > 1 {
                for id in v {
                    out.insert(*id);
                }
            }
        }
    }
    Ok(out)
}

pub fn set_journal_enabled(conn: &Connection, id: i64, enabled: bool) -> Result<()> {
    conn.execute(
        "UPDATE journals SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
        params![enabled as i64, now_utc(), id],
    )?;
    Ok(())
}

#[allow(dead_code)] // 阶段四 RSS 发现时启用
pub fn set_journal_rss(conn: &Connection, id: i64, rss_url: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE journals SET rss_url = ?1, updated_at = ?2 WHERE id = ?3",
        params![rss_url, now_utc(), id],
    )?;
    Ok(())
}

pub fn delete_journal(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM journals WHERE id = ?1", params![id])?;
    Ok(())
}

// ---------- Round 5A：Canonical Journal Identity ----------

/// 设置 ISSN-L（linking ISSN）。调用方需保证传入值已 normalize 或为 None。
pub fn set_journal_issn_l(conn: &Connection, id: i64, issn_l: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE journals SET issn_l = ?1, updated_at = ?2 WHERE id = ?3",
        params![issn_l, now_utc(), id],
    )?;
    Ok(())
}

/// 写入规范化 identifier（幂等：INSERT OR IGNORE，value 唯一索引保证一个 ISSN 只映射一个 Journal）。
/// 调用方必须传入已 normalize 的 value（canonical NNNN-NNNX）。
pub fn insert_identifier(
    conn: &Connection,
    journal_id: i64,
    identifier_type: &str,
    value: &str,
    source: Option<&str>,
) -> Result<()> {
    let now = now_utc();
    conn.execute(
        "INSERT OR IGNORE INTO journal_identifiers (journal_id, identifier_type, value, source, created_at, updated_at)
         VALUES (?1,?2,?3,?4,?5,?5)",
        params![journal_id, identifier_type, value, source, now],
    )?;
    Ok(())
}

/// 输入任意已知 ISSN（规范化后），返回其映射的 canonical Journal id（如有）。
pub fn resolve_journal_by_identifier(conn: &Connection, value: &str) -> Result<Option<i64>> {
    let id = conn
        .query_row(
            "SELECT journal_id FROM journal_identifiers WHERE value = ?1",
            params![value],
            |r| r.get::<_, i64>(0),
        )
        .optional()?;
    Ok(id)
}

/// 按集合 code 查找 collection id。
pub fn find_collection_by_code(conn: &Connection, code: &str) -> Result<Option<i64>> {
    let id = conn
        .query_row(
            "SELECT id FROM journal_collections WHERE code = ?1",
            params![code],
            |r| r.get::<_, i64>(0),
        )
        .optional()?;
    Ok(id)
}

/// 读取 journals.issn_l（用于只填空、不覆盖）。
pub fn get_journal_issn_l(conn: &Connection, id: i64) -> Result<Option<String>> {
    let v: Option<String> = conn
        .query_row("SELECT issn_l FROM journals WHERE id = ?1", params![id], |r| r.get(0))
        .optional()?
        .flatten();
    Ok(v)
}

/// 设置 metadata_needs_review（幂等：仅置 true）。
pub fn set_journal_review_flag(conn: &Connection, id: i64, review: bool) -> Result<()> {
    if review {
        conn.execute(
            "UPDATE journals SET metadata_needs_review = 1, updated_at = ?1 WHERE id = ?2",
            params![now_utc(), id],
        )?;
    }
    Ok(())
}

/// 显式别名匹配（Round 5C.1）：只接受 catalog.json 明确列出的 alias（含 canonical_title），
/// 对 journals.name 做规范化 key 精确比较。禁止模糊字符串（contains/编辑距离）自动合并。
/// 返回 id 最小的命中期刊。
pub fn find_journal_by_aliases(conn: &Connection, aliases: &[String]) -> Result<Option<i64>> {
    let keys: Vec<String> = aliases
        .iter()
        .filter_map(|a| {
            let k: String = a
                .chars()
                .filter(|c| c.is_alphanumeric())
                .map(|c| c.to_ascii_lowercase())
                .collect();
            if k.is_empty() {
                None
            } else {
                Some(k)
            }
        })
        .collect();
    if keys.is_empty() {
        return Ok(None);
    }
    let mut stmt = conn.prepare("SELECT id, name FROM journals ORDER BY id")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows {
        let (id, name) = row?;
        let nk: String = name
            .chars()
            .filter(|c| c.is_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        if keys.contains(&nk) {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// 已有 Journal 的 identifiers 是否与 catalog 候选 identifier 冲突：
/// existing 有 identifier，且没有一个与 catalog 的 print/online 相同 → 冲突（禁止 alias 合并）。
pub fn journal_has_conflicting_identifiers(
    conn: &Connection,
    id: i64,
    catalog_print: Option<&str>,
    catalog_online: Option<&str>,
) -> Result<bool> {
    let existing = list_journal_identifiers(conn, id)?;
    if existing.is_empty() {
        return Ok(false);
    }
    let set: std::collections::HashSet<&str> =
        existing.iter().map(|i| i.value.as_str()).collect();
    let any_match = [catalog_print, catalog_online]
        .into_iter()
        .flatten()
        .any(|c| set.contains(c));
    Ok(!any_match)
}

/// 按 ISSN-L（规范化后）查找 canonical Journal（journals.issn_l 列）。
pub fn find_journal_by_issn_l(conn: &Connection, issn_l: &str) -> Result<Option<i64>> {
    let id = conn
        .query_row(
            "SELECT id FROM journals WHERE issn_l = ?1",
            params![issn_l],
            |r| r.get::<_, i64>(0),
        )
        .optional()?;
    Ok(id)
}

/// 某 Journal 的全部 identifiers。
pub fn list_journal_identifiers(conn: &Connection, journal_id: i64) -> Result<Vec<crate::models::JournalIdentifier>> {
    let mut stmt = conn.prepare(
        "SELECT id, journal_id, identifier_type, value, source, created_at, updated_at
         FROM journal_identifiers WHERE journal_id = ?1 ORDER BY identifier_type, value",
    )?;
    let rows = stmt.query_map(params![journal_id], |r| {
        Ok(crate::models::JournalIdentifier {
            id: r.get(0)?,
            journal_id: r.get(1)?,
            identifier_type: r.get(2)?,
            value: r.get(3)?,
            source: r.get(4)?,
            created_at: r.get(5)?,
            updated_at: r.get(6)?,
        })
    })?;
    rows.collect()
}

// ---------- Round 5A：Journal Collections ----------

pub fn create_collection(
    conn: &Connection,
    code: &str,
    name: &str,
    version: Option<&str>,
    effective_from: Option<&str>,
    source_name: Option<&str>,
    source_url: Option<&str>,
) -> Result<i64> {
    let now = now_utc();
    conn.execute(
        "INSERT INTO journal_collections (code, name, version, effective_from, source_name, source_url, created_at, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?7)",
        params![code, name, version, effective_from, source_name, source_url, now],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 幂等加入集合成员（PRIMARY KEY (collection_id, journal_id) 拒绝重复）。
/// 返回是否实际新增（false = 已存在）。
pub fn add_collection_member(conn: &Connection, collection_id: i64, journal_id: i64) -> Result<bool> {
    let n = conn.execute(
        "INSERT OR IGNORE INTO journal_collection_members (collection_id, journal_id) VALUES (?1,?2)",
        params![collection_id, journal_id],
    )?;
    Ok(n > 0)
}

pub fn list_collections(conn: &Connection) -> Result<Vec<crate::models::JournalCollection>> {
    let mut stmt = conn.prepare(
        "SELECT id, code, name, version, effective_from, source_name, source_url, last_verified_at, created_at, updated_at
         FROM journal_collections ORDER BY code",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(crate::models::JournalCollection {
            id: r.get(0)?,
            code: r.get(1)?,
            name: r.get(2)?,
            version: r.get(3)?,
            effective_from: r.get(4)?,
            source_name: r.get(5)?,
            source_url: r.get(6)?,
            last_verified_at: r.get(7)?,
            created_at: r.get(8)?,
            updated_at: r.get(9)?,
        })
    })?;
    rows.collect()
}

/// 某集合的 membership 数（按 code）。
pub fn count_collection_members(conn: &Connection, code: &str) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM journal_collection_members m JOIN journal_collections c ON c.id = m.collection_id WHERE c.code = ?1",
        params![code],
        |r| r.get(0),
    )
}

/// Journal 所属集合（Paper → journal → collections 的派生路径）。
pub fn collections_for_journal(conn: &Connection, journal_id: i64) -> Result<Vec<crate::models::JournalCollection>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.code, c.name, c.version, c.effective_from, c.source_name, c.source_url, c.last_verified_at, c.created_at, c.updated_at
         FROM journal_collection_members m
         JOIN journal_collections c ON c.id = m.collection_id
         WHERE m.journal_id = ?1 ORDER BY c.code",
    )?;
    let rows = stmt.query_map(params![journal_id], |r| {
        Ok(crate::models::JournalCollection {
            id: r.get(0)?,
            code: r.get(1)?,
            name: r.get(2)?,
            version: r.get(3)?,
            effective_from: r.get(4)?,
            source_name: r.get(5)?,
            source_url: r.get(6)?,
            last_verified_at: r.get(7)?,
            created_at: r.get(8)?,
            updated_at: r.get(9)?,
        })
    })?;
    rows.collect()
}

pub fn update_journal_sync_state(
    conn: &Connection,
    id: i64,
    last_successful_sync_at: &str,
    last_paper_date: Option<&str>,
    coverage_status: &str,
    abstract_coverage_rate: Option<f64>,
) -> Result<()> {
    conn.execute(
        "UPDATE journals SET last_successful_sync_at = ?1, last_paper_date = ?2, coverage_status = ?3, abstract_coverage_rate = ?4, updated_at = ?5 WHERE id = ?6",
        params![last_successful_sync_at, last_paper_date, coverage_status, abstract_coverage_rate, now_utc(), id],
    )?;
    Ok(())
}

pub fn get_last_successful_sync_at(conn: &Connection, id: i64) -> Result<Option<String>> {
    let v: Option<String> = conn
        .query_row(
            "SELECT last_successful_sync_at FROM journals WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    Ok(v)
}

// ---------- Papers ----------

/// 依据需求书 §8.2 的去重优先级查找已有论文。
pub fn find_paper_id(conn: &Connection, journal_id: i64, c: &PaperCandidate) -> Result<Option<i64>> {
    if let Some(doi) = &c.normalized_doi {
        let id = conn
            .query_row(
                "SELECT id FROM papers WHERE normalized_doi = ?1",
                params![doi],
                |r| r.get::<_, i64>(0),
            )
            .optional()?;
        if id.is_some() {
            return Ok(id);
        }
    }
    if let Some(paid) = &c.publisher_article_id {
        let id = conn
            .query_row(
                "SELECT id FROM papers WHERE publisher_article_id = ?1",
                params![paid],
                |r| r.get::<_, i64>(0),
            )
            .optional()?;
        if id.is_some() {
            return Ok(id);
        }
    }
    if let Some(wid) = &c.openalex_work_id {
        let id = conn
            .query_row(
                "SELECT id FROM papers WHERE openalex_work_id = ?1",
                params![wid],
                |r| r.get::<_, i64>(0),
            )
            .optional()?;
        if id.is_some() {
            return Ok(id);
        }
    }
    if let (Some(title), Some(year)) = (&c.title, c.year) {
        let norm = normalize_title(title);
        let id = conn
            .query_row(
                "SELECT id FROM papers WHERE journal_id = ?1 AND year = ?2 AND title_norm = ?3",
                params![journal_id, year, norm],
                |r| r.get::<_, i64>(0),
            )
            .optional()?;
        if id.is_some() {
            return Ok(id);
        }
    }
    Ok(None)
}

/// 记录某来源的摘要候选（UNIQUE(paper_id, source)，upsert 保留最新版本）。
fn record_abstract_source(
    conn: &Connection,
    paper_id: i64,
    source: &str,
    text: &str,
    quality: &str,
    reason: &str,
) -> Result<()> {
    let now = now_utc();
    conn.execute(
        "INSERT INTO paper_abstract_sources (paper_id, source, abstract_text, quality, quality_reason, fetched_at, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?6)
         ON CONFLICT(paper_id, source) DO UPDATE SET
            abstract_text = excluded.abstract_text,
            quality = excluded.quality,
            quality_reason = excluded.quality_reason,
            updated_at = excluded.updated_at",
        params![paper_id, source, text, quality, reason, now],
    )?;
    Ok(())
}

fn quality_rank(q: &str) -> i8 {
    match q {
        crate::models::ABQ_COMPLETE => 2,
        crate::models::ABQ_PARTIAL => 1,
        _ => 0,
    }
}

/// 合并摘要：记录新来源候选 → canonical selection → 升级（禁降级）→ 节流时间戳。
/// 返回 (abstract_filled, abstract_upgraded)。
fn merge_abstract(conn: &Connection, paper_id: i64, c: &PaperCandidate) -> Result<(bool, bool)> {
    let now = now_utc();
    let cand_source = c.abstract_source.clone().unwrap_or_else(|| "unknown".to_string());

    // 1) 记录该来源候选（normalized）
    let mut new_cand: Option<(String, &'static str, &'static str)> = None;
    if let Some(ct) = &c.abstract_text {
        let n = crate::abstract_quality::normalize_abstract_text(ct);
        if !n.trim().is_empty() {
            let (q, r) = crate::abstract_quality::assess_abstract_quality(&n);
            record_abstract_source(conn, paper_id, &cand_source, &n, q, r)?;
            new_cand = Some((n, q, r));
        }
    }

    // 2) 读取当前 canonical
    let (cur_text, cur_source, cur_quality): (Option<String>, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT abstract, abstract_source, abstract_quality FROM papers WHERE id = ?1",
            params![paper_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .unwrap_or((None, None, None));

    // 3) canonical selection：当前 + 新候选
    let mut candidates: Vec<crate::abstract_quality::AbstractCandidate> = Vec::new();
    if let Some(t) = &cur_text {
        if !t.trim().is_empty() {
            let q = cur_quality.as_deref().unwrap_or(crate::models::ABQ_MISSING).to_string();
            let (cq, cr): (String, String) = if q.as_str() == crate::models::ABQ_MISSING {
                let (x, y) = crate::abstract_quality::assess_abstract_quality(t);
                (x.to_string(), y.to_string())
            } else if q.as_str() == crate::models::ABQ_COMPLETE {
                (q, "full_text_like_abstract".to_string())
            } else {
                (q, "prefix_of_longer_source".to_string())
            };
            candidates.push(crate::abstract_quality::AbstractCandidate {
                source: cur_source.clone().unwrap_or_else(|| "unknown".to_string()),
                text: t.clone(),
                quality: cq,
                reason: cr,
            });
        }
    }
    if let Some((t, q, r)) = &new_cand {
        candidates.push(crate::abstract_quality::AbstractCandidate {
            source: cand_source.clone(),
            text: t.clone(),
            quality: q.to_string(),
            reason: r.to_string(),
        });
    }

    let mut filled = false;
    let mut upgraded = false;
    let prev_quality = cur_quality.as_deref().unwrap_or(crate::models::ABQ_MISSING);

    if let Some(best) = crate::abstract_quality::select_canonical_abstract(candidates) {
        let cur_norm = cur_text.as_deref().map(crate::abstract_quality::normalize_abstract_text);
        let same_text = cur_norm.as_deref().map(|t| t.trim() == best.text.trim()).unwrap_or(false);
        let best_rank = quality_rank(&best.quality);
        let cur_rank = quality_rank(prev_quality);
        let should_update = if best_rank > cur_rank {
            true // 升级：partial→complete / missing→(partial|complete)
        } else if best_rank < cur_rank {
            false // 禁降级：complete → partial 一律不覆盖
        } else if same_text {
            false
        } else if best.quality == crate::models::ABQ_COMPLETE {
            // 同 complete：来源优先级更可靠 或 明显更长（≥1.5x）才替换
            let better_source = crate::abstract_quality::source_priority(&best.source)
                < crate::abstract_quality::source_priority(cur_source.as_deref().unwrap_or(""));
            let much_longer = best.text.len() >= cur_norm.as_deref().map(|t| t.len() * 15 / 10).unwrap_or(usize::MAX);
            better_source || much_longer
        } else {
            // 同 partial：仅当来源优先级更可靠且文本不同才替换
            crate::abstract_quality::source_priority(&best.source)
                < crate::abstract_quality::source_priority(cur_source.as_deref().unwrap_or(""))
        };

        if should_update {
            let had_abstract = cur_text.as_deref().map(|t| !t.trim().is_empty()).unwrap_or(false);
            conn.execute(
                "UPDATE papers SET abstract = ?1, abstract_source = ?2, abstract_quality = ?3,
                    abstract_retrieved_at = ?4, abstract_last_checked_at = ?4, updated_at = ?4,
                    evidence_hash = NULL
                 WHERE id = ?5",
                params![best.text, best.source, best.quality, now, paper_id],
            )?;
            if !had_abstract {
                filled = true;
            } else {
                upgraded = true;
            }
            // 摘要补全/升级后获得重新入队资格：final/stale 状态 → pendingAnalysis。
            // 正在执行中的分析（queued/analyzing）不覆盖，避免重复请求；pendingAnalysis 保持不变。
            conn.execute(
                "UPDATE papers SET analysis_status = 'pendingAnalysis'
                 WHERE id = ?1 AND analysis_status IN ('waitingForAbstract','analysisSucceeded','analysisFailed')",
                params![paper_id],
            )?;
        } else {
            // 未升级也记录检查时间（节流依据）
            conn.execute(
                "UPDATE papers SET abstract_last_checked_at = ?1 WHERE id = ?2",
                params![now, paper_id],
            )?;
        }
    }

    // 填充其他缺失字段（保持原有行为）
    fill_other_fields(conn, paper_id, c)?;

    Ok((filled, upgraded))
}

/// 填充非摘要缺失字段（从 fill_missing_fields 拆出，保持原有 §8.3 行为）。
fn fill_other_fields(conn: &Connection, id: i64, c: &PaperCandidate) -> Result<()> {
    let authors_json = serde_json::to_string(&c.authors).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "UPDATE papers SET
            url = COALESCE(url, ?1),
            title = COALESCE(title, ?2),
            authors_json = CASE WHEN authors_json IS NULL OR authors_json = '[]' THEN ?3 ELSE authors_json END,
            published_date = COALESCE(published_date, ?4),
            year = COALESCE(year, ?5),
            publisher_article_id = COALESCE(publisher_article_id, ?6),
            openalex_work_id = COALESCE(openalex_work_id, ?7),
            updated_at = ?8
         WHERE id = ?9",
        params![
            c.url,
            c.title,
            authors_json,
            c.published_date,
            c.year,
            c.publisher_article_id,
            c.openalex_work_id,
            now_utc(),
            id
        ],
    )?;
    Ok(())
}

pub fn upsert_paper(conn: &Connection, journal_id: i64, c: &PaperCandidate) -> Result<UpsertOutcome> {
    if let Some(existing_id) = find_paper_id(conn, journal_id, c)? {
        let (abstract_filled, abstract_upgraded) = merge_abstract(conn, existing_id, c)?;
        return Ok(UpsertOutcome::Existing {
            id: existing_id,
            abstract_filled,
            abstract_upgraded,
        });
    }

    let authors_json = serde_json::to_string(&c.authors).unwrap_or_else(|_| "[]".to_string());
    let title_norm = c.title.as_deref().map(normalize_title);
    // 摘要质量（本地判定）决定初始 analysis_status：
    // missing → waitingForAbstract；partial/complete → pendingAnalysis
    let (abs_norm, abs_quality) = match &c.abstract_text {
        Some(a) => {
            let n = crate::abstract_quality::normalize_abstract_text(a);
            let (q, _r) = crate::abstract_quality::assess_abstract_quality(&n);
            (Some(n), q)
        }
        None => (None, crate::models::ABQ_MISSING),
    };
    let analysis_status = if abs_quality == crate::models::ABQ_MISSING {
        ST_WAITING_ABSTRACT
    } else {
        ST_PENDING
    };
    let now = now_utc();

    conn.execute(
        "INSERT INTO papers (
            journal_id, normalized_doi, original_doi, title, title_norm, authors_json,
            published_date, year, abstract, abstract_source, abstract_retrieved_at,
            url, publisher_article_id, openalex_work_id, discovery_source,
            analysis_status, abstract_quality, abstract_last_checked_at, created_at, updated_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?19)",
        params![
            journal_id,
            c.normalized_doi,
            c.original_doi,
            c.title,
            title_norm,
            authors_json,
            c.published_date,
            c.year,
            abs_norm,
            c.abstract_source,
            now.clone(),
            c.url,
            c.publisher_article_id,
            c.openalex_work_id,
            c.discovery_source,
            analysis_status,
            abs_quality,
            now.clone(),
            now
        ],
    )?;
    let id = conn.last_insert_rowid();
    // 记录初始来源候选
    if let (Some(t), Some(src)) = (&abs_norm, &c.abstract_source) {
        let (q, r) = crate::abstract_quality::assess_abstract_quality(t);
        let _ = record_abstract_source(conn, id, src, t, q, r);
    }
    Ok(UpsertOutcome::New(id))
}

pub fn insert_source_record(
    conn: &Connection,
    paper_id: i64,
    source: &str,
    source_id: Option<&str>,
    raw_json: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO source_records (paper_id, source, source_id, raw_json, retrieved_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![paper_id, source, source_id, raw_json, now_utc()],
    )?;
    Ok(())
}

fn row_to_paper(row: &rusqlite::Row) -> Result<Paper> {
    let authors_json: Option<String> = row.get("authors_json")?;
    let authors: Vec<Author> = authors_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    let tag_matches_json: Option<String> = row.get("tag_matches_json")?;
    let tag_matches: Vec<TagMatch> = tag_matches_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    Ok(Paper {
        id: row.get("id")?,
        journal_id: row.get("journal_id")?,
        journal_name: row.get("journal_name")?,
        normalized_doi: row.get("normalized_doi")?,
        original_doi: row.get("original_doi")?,
        title: row.get("title")?,
        authors,
        published_date: row.get("published_date")?,
        year: row.get("year")?,
        abstract_text: row.get("abstract")?,
        abstract_source: row.get("abstract_source")?,
        abstract_retrieved_at: row.get("abstract_retrieved_at")?,
        abstract_quality: row.get("abstract_quality")?,
        abstract_last_checked_at: row.get("abstract_last_checked_at")?,
        abstract_retry_count: row.get("abstract_retry_count")?,
        url: row.get("url")?,
        publisher_article_id: row.get("publisher_article_id")?,
        openalex_work_id: row.get("openalex_work_id")?,
        discovery_source: row.get("discovery_source")?,
        is_favorite: row.get::<_, i64>("is_favorite")? != 0,
        is_read: row.get::<_, i64>("is_read")? != 0,
        is_ignored: row.get::<_, i64>("is_ignored")? != 0,
        analysis_status: row.get("analysis_status")?,
        chinese_title: row.get("chinese_title")?,
        chinese_abstract: row.get("chinese_abstract")?,
        one_sentence_summary: row.get("one_sentence_summary")?,
        tag_matches,
        total_score: row.get("total_score")?,
        model_name: row.get("model_name")?,
        prompt_version: row.get("prompt_version")?,
        evidence_hash: row.get("evidence_hash")?,
        analyzed_at: row.get("analyzed_at")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        collections: Vec::new(),
    })
}

pub fn list_papers(conn: &Connection, journal_id: Option<i64>, limit: i64) -> Result<Vec<Paper>> {
    let sql = format!(
        "SELECT p.*, j.name AS journal_name FROM papers p
         JOIN journals j ON j.id = p.journal_id
         {} ORDER BY p.published_date DESC, p.id DESC LIMIT ?1",
        if journal_id.is_some() { "WHERE p.journal_id = ?2" } else { "" }
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = if let Some(jid) = journal_id {
        stmt.query_map(params![limit, jid], row_to_paper)?
    } else {
        stmt.query_map(params![limit], row_to_paper)?
    };
    let mut papers: Vec<Paper> = rows.collect::<Result<Vec<_>>>()?;
    enrich_papers_collections(conn, &mut papers)?;
    Ok(papers)
}

/// 一次查询填充全部 papers 的 collection codes（避免 N+1）。
fn enrich_papers_collections(conn: &Connection, papers: &mut [Paper]) -> Result<()> {
    if papers.is_empty() {
        return Ok(());
    }
    // 简化实现：为每篇 paper 查询其 journal 的 collections（论文量通常 < 1000，一次查询足够）
    let mut all: Vec<(i64, String)> = Vec::new();
    {
        let mut stmt2 = conn.prepare(
            "SELECT m.journal_id, c.code FROM journal_collection_members m
             JOIN journal_collections c ON c.id = m.collection_id ORDER BY c.code",
        )?;
        let rows = stmt2.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            all.push(row?);
        }
    }
    for p in papers.iter_mut() {
        p.collections = all
            .iter()
            .filter(|(jid, _)| *jid == p.journal_id)
            .map(|(_, code)| code.clone())
            .collect();
    }
    Ok(())
}

pub fn count_waiting_for_abstract(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM papers WHERE analysis_status = 'waitingForAbstract'",
        [],
        |r| r.get(0),
    )
}

/// 待分析（历史积压）数量：有摘要且尚未分析。
pub fn count_pending_papers(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM papers WHERE analysis_status = 'pendingAnalysis' AND abstract IS NOT NULL AND abstract != ''",
        [],
        |r| r.get(0),
    )
}

/// 返回某期刊 (论文总数, 有摘要数, 最近论文日期)。
pub fn journal_stats(conn: &Connection, journal_id: i64) -> Result<(i64, i64, Option<String>)> {
    conn.query_row(
        "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN abstract IS NOT NULL AND abstract != '' THEN 1 ELSE 0 END), 0),
                MAX(published_date)
         FROM papers WHERE journal_id = ?1",
        params![journal_id],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        },
    )
}

// ---------- 迁移 ----------

fn column_exists(conn: &Connection, table: &str, col: &str) -> bool {
    let sql = format!("PRAGMA table_info({})", table);
    if let Ok(mut stmt) = conn.prepare(&sql) {
        if let Ok(cols) = stmt.query_map([], |r| r.get::<_, String>(1)) {
            return cols.filter_map(|c| c.ok()).any(|c| c == col);
        }
    }
    false
}

/// 版本化迁移：按 user_version 顺序执行，每个迁移在事务中完成，
/// 失败即回滚且不推进版本，杜绝半迁移状态。已成功的迁移不会重复执行。
fn run_migrations(conn: &Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (version, _name, up) in migrations() {
        if version <= current {
            continue;
        }
        let tx = conn.unchecked_transaction()?;
        up(&tx)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}

fn migrations() -> Vec<(i64, &'static str, fn(&Connection) -> Result<()>)> {
    vec![
        (1, "round3-baseline", migrate_to_v1),
        (2, "round4-batches", migrate_to_v2),
        (3, "round5a-identity", migrate_to_v3),
        (4, "round5b-abstract-quality", migrate_to_v4),
        (5, "round5c-catalog", migrate_to_v5),
    ]
}

/// v5：Verified Journal Catalog 支持。
/// - journals.metadata_needs_review：identifier 未可靠解决时标记（不阻塞导入）
fn migrate_to_v5(conn: &Connection) -> Result<()> {
    if !column_exists(conn, "journals", "metadata_needs_review") {
        conn.execute(
            "ALTER TABLE journals ADD COLUMN metadata_needs_review INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    Ok(())
}

/// v4：Abstract Quality & Recovery。
/// - papers 新增 abstract_quality（complete/partial/missing）、abstract_last_checked_at、
///   abstract_retry_count
/// - paper_abstract_sources：多来源摘要候选（UNIQUE(paper_id, source)，保留来源差异）
/// - 存量摘要本地评估（normalize + heuristic），只标记质量、回写 normalized 文本；
///   绝不调用 DeepSeek / 不触发 AI / 不收费
fn migrate_to_v4(conn: &Connection) -> Result<()> {
    for (name, ty) in [
        ("abstract_quality", "TEXT NOT NULL DEFAULT 'missing'"),
        ("abstract_last_checked_at", "TEXT"),
        ("abstract_retry_count", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        if !column_exists(conn, "papers", name) {
            conn.execute(&format!("ALTER TABLE papers ADD COLUMN {} {}", name, ty), [])?;
        }
    }
    if !column_exists(conn, "sync_batches", "abstracts_upgraded") {
        conn.execute(
            "ALTER TABLE sync_batches ADD COLUMN abstracts_upgraded INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS paper_abstract_sources (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            paper_id INTEGER NOT NULL REFERENCES papers(id) ON DELETE CASCADE,
            source TEXT NOT NULL,
            abstract_text TEXT NOT NULL,
            quality TEXT NOT NULL,
            quality_reason TEXT,
            fetched_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE (paper_id, source)
        );
        CREATE INDEX IF NOT EXISTS idx_pas_paper ON paper_abstract_sources(paper_id);
        "#,
    )?;
    initialize_abstract_quality(conn)?;
    Ok(())
}

/// 存量摘要质量初始化：本地 normalize + heuristic 判定。
/// missing → quality=missing（waitingForAbstract 对齐在正常同步流程完成）；
/// 非空 → 回写 normalized 纯文本并评估 complete/partial。
fn initialize_abstract_quality(conn: &Connection) -> Result<()> {
    let rows: Vec<(i64, Option<String>)> = {
        let mut stmt = conn.prepare("SELECT id, abstract FROM papers")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?))
        })?;
        rows.collect::<Result<Vec<_>>>()?
    };
    let now = now_utc();
    for (id, abstract_text) in rows {
        match abstract_text {
            None => {
                conn.execute(
                    "UPDATE papers SET abstract_quality = ?1, abstract_last_checked_at = ?2 WHERE id = ?3",
                    params![crate::models::ABQ_MISSING, now, id],
                )?;
            }
            Some(raw) => {
                let normalized = crate::abstract_quality::normalize_abstract_text(&raw);
                if normalized.trim().is_empty() {
                    conn.execute(
                        "UPDATE papers SET abstract = NULL, abstract_quality = ?1, abstract_last_checked_at = ?2 WHERE id = ?3",
                        params![crate::models::ABQ_MISSING, now, id],
                    )?;
                    continue;
                }
                let (q, r) = crate::abstract_quality::assess_abstract_quality(&normalized);
                conn.execute(
                    "UPDATE papers SET abstract = ?1, abstract_quality = ?2, abstract_last_checked_at = ?3 WHERE id = ?4",
                    params![normalized, q, now, id],
                )?;
                let _ = conn.execute(
                    "INSERT OR IGNORE INTO paper_abstract_sources (paper_id, source, abstract_text, quality, quality_reason, fetched_at, updated_at)
                     VALUES (?1, 'migration', ?2, ?3, ?4, ?5, ?5)",
                    params![id, normalized, q, r, now],
                );
            }
        }
    }
    Ok(())
}

/// v3：Canonical Journal Identity + Journal Collection 基础。
/// - journals.issn_l（linking ISSN，nullable）
/// - journal_identifiers：一个 canonical Journal 可有多 ISSN（print/online/other），
///   规范化 value 全库唯一（一个 ISSN 只能映射一个 Journal）
/// - journal_collections + journal_collection_members：many-to-many，集合是 Journal metadata
///   不参与 AI 评分
/// - 旧 print_issn/online_issn 按列可靠迁移进 journal_identifiers（不猜类型）
fn migrate_to_v3(conn: &Connection) -> Result<()> {
    if !column_exists(conn, "journals", "issn_l") {
        conn.execute("ALTER TABLE journals ADD COLUMN issn_l TEXT", [])?;
    }
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS journal_identifiers (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            journal_id INTEGER NOT NULL REFERENCES journals(id) ON DELETE CASCADE,
            identifier_type TEXT NOT NULL,
            value TEXT NOT NULL,
            source TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_ji_value ON journal_identifiers(value);
        CREATE INDEX IF NOT EXISTS idx_ji_journal ON journal_identifiers(journal_id);

        CREATE TABLE IF NOT EXISTS journal_collections (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            code TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            version TEXT,
            effective_from TEXT,
            source_name TEXT,
            source_url TEXT,
            last_verified_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS journal_collection_members (
            collection_id INTEGER NOT NULL REFERENCES journal_collections(id) ON DELETE CASCADE,
            journal_id INTEGER NOT NULL REFERENCES journals(id) ON DELETE CASCADE,
            PRIMARY KEY (collection_id, journal_id)
        );
        "#,
    )?;
    migrate_legacy_issns(conn)?;
    Ok(())
}

/// 旧 issn 迁移：现有 journals.print_issn / online_issn 列已明确类型，按列迁移（不猜）；
/// normalize 校验通过才入库；与已有 identifiers 冲突时保留已有映射（INSERT OR IGNORE）。
fn migrate_legacy_issns(conn: &Connection) -> Result<()> {
    let now = now_utc();
    let rows: Vec<(i64, Option<String>, Option<String>)> = {
        let mut stmt = conn.prepare("SELECT id, print_issn, online_issn FROM journals")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>>>()?
    };
    for (jid, print, online) in rows {
        if let Some(p) = print {
            if let Some(n) = crate::util::normalize_issn(&p) {
                let _ = conn.execute(
                    "INSERT OR IGNORE INTO journal_identifiers (journal_id, identifier_type, value, source, created_at, updated_at)
                     VALUES (?1,?2,?3,'migration',?4,?4)",
                    params![jid, IDT_PRINT, n, now],
                );
            }
        }
        if let Some(o) = online {
            if let Some(n) = crate::util::normalize_issn(&o) {
                let _ = conn.execute(
                    "INSERT OR IGNORE INTO journal_identifiers (journal_id, identifier_type, value, source, created_at, updated_at)
                     VALUES (?1,?2,?3,'migration',?4,?4)",
                    params![jid, IDT_ONLINE, n, now],
                );
            }
        }
    }
    Ok(())
}

/// v2：新增 Batch & Activity 表（事务内执行，由 run_migrations 统一驱动）。
fn migrate_to_v2(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS sync_batches (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            trigger TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            started_at TEXT,
            finished_at TEXT,
            journal_total INTEGER NOT NULL DEFAULT 0,
            journal_completed INTEGER NOT NULL DEFAULT 0,
            journal_failed INTEGER NOT NULL DEFAULT 0,
            records_found INTEGER NOT NULL DEFAULT 0,
            papers_inserted INTEGER NOT NULL DEFAULT 0,
            papers_existing INTEGER NOT NULL DEFAULT 0,
            abstracts_added INTEGER NOT NULL DEFAULT 0,
            waiting_abstract INTEGER NOT NULL DEFAULT 0,
            error_summary TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_sb_started ON sync_batches(started_at);

        CREATE TABLE IF NOT EXISTS sync_batch_papers (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            sync_batch_id INTEGER NOT NULL REFERENCES sync_batches(id) ON DELETE CASCADE,
            paper_id INTEGER NOT NULL REFERENCES papers(id) ON DELETE CASCADE,
            result TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_sbp_batch ON sync_batch_papers(sync_batch_id);
        CREATE INDEX IF NOT EXISTS idx_sbp_paper ON sync_batch_papers(paper_id);

        CREATE TABLE IF NOT EXISTS analysis_batches (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source_sync_batch_id INTEGER,
            parent_batch_id INTEGER,
            trigger TEXT NOT NULL,
            status TEXT NOT NULL,
            model_name TEXT,
            prompt_version TEXT,
            created_at TEXT NOT NULL,
            started_at TEXT,
            finished_at TEXT,
            total INTEGER NOT NULL DEFAULT 0,
            completed INTEGER NOT NULL DEFAULT 0,
            succeeded INTEGER NOT NULL DEFAULT 0,
            failed INTEGER NOT NULL DEFAULT 0,
            skipped INTEGER NOT NULL DEFAULT 0,
            remaining INTEGER NOT NULL DEFAULT 0,
            error_summary TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_ab_started ON analysis_batches(started_at);
        CREATE INDEX IF NOT EXISTS idx_ab_status ON analysis_batches(status);

        CREATE TABLE IF NOT EXISTS analysis_batch_items (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            analysis_batch_id INTEGER NOT NULL REFERENCES analysis_batches(id) ON DELETE CASCADE,
            paper_id INTEGER NOT NULL REFERENCES papers(id) ON DELETE CASCADE,
            status TEXT NOT NULL,
            attempt_count INTEGER NOT NULL DEFAULT 0,
            started_at TEXT,
            finished_at TEXT,
            error_type TEXT,
            error_summary TEXT,
            previous_analysis_hash TEXT,
            result_analysis_hash TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_abi_batch ON analysis_batch_items(analysis_batch_id);
        CREATE INDEX IF NOT EXISTS idx_abi_paper ON analysis_batch_items(paper_id);
        CREATE INDEX IF NOT EXISTS idx_abi_status ON analysis_batch_items(status);
        "#,
    )?;
    Ok(())
}

/// v1：把任意旧库升级到 round-3 基线（幂等：列已存在则跳过）。
fn migrate_to_v1(conn: &Connection) -> Result<()> {
    let add_cols: &[(&str, &str)] = &[
        ("chinese_title", "TEXT"),
        ("chinese_abstract", "TEXT"),
        ("one_sentence_summary", "TEXT"),
        ("tag_matches_json", "TEXT"),
        ("total_score", "REAL"),
        ("model_name", "TEXT"),
        ("prompt_version", "TEXT"),
        ("evidence_hash", "TEXT"),
        ("analyzed_at", "TEXT"),
        ("is_favorite", "INTEGER NOT NULL DEFAULT 0"),
        ("is_read", "INTEGER NOT NULL DEFAULT 0"),
        ("is_ignored", "INTEGER NOT NULL DEFAULT 0"),
        ("retry_count", "INTEGER NOT NULL DEFAULT 0"),
        ("queued_at", "TEXT"),
    ];
    for (name, ty) in add_cols {
        if !column_exists(conn, "papers", name) {
            conn.execute(&format!("ALTER TABLE papers ADD COLUMN {} {}", name, ty), [])?;
        }
    }
    // 状态枚举重命名（旧值 → 新值）
    conn.execute(
        "UPDATE papers SET analysis_status = ?1 WHERE analysis_status = 'pending'",
        params![ST_PENDING],
    )?;
    conn.execute(
        "UPDATE papers SET analysis_status = ?1 WHERE analysis_status = 'analyzed'",
        params![ST_SUCCEEDED],
    )?;
    Ok(())
}

// ---------- Tags ----------

fn row_to_tag(row: &rusqlite::Row) -> Result<Tag> {
    Ok(Tag {
        id: row.get("id")?,
        name: row.get("name")?,
        description: row.get("description")?,
        enabled: row.get::<_, i64>("enabled")? != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub fn list_tags(conn: &Connection) -> Result<Vec<Tag>> {
    let mut stmt = conn.prepare("SELECT * FROM tags ORDER BY id ASC")?;
    let rows = stmt.query_map([], row_to_tag)?;
    rows.collect()
}

pub fn get_tag(conn: &Connection, id: i64) -> Result<Option<Tag>> {
    conn.query_row("SELECT * FROM tags WHERE id = ?1", params![id], row_to_tag)
        .optional()
}

pub fn add_tag(conn: &Connection, name: &str, description: Option<&str>) -> Result<Tag> {
    let now = now_utc();
    conn.execute(
        "INSERT INTO tags (name, description, enabled, created_at, updated_at) VALUES (?1, ?2, 1, ?3, ?3)",
        params![name, description, now],
    )?;
    let id = conn.last_insert_rowid();
    get_tag(conn, id).map(|t| t.unwrap())
}

pub fn update_tag(
    conn: &Connection,
    id: i64,
    name: &str,
    description: Option<&str>,
    enabled: bool,
) -> Result<()> {
    conn.execute(
        "UPDATE tags SET name = ?1, description = ?2, enabled = ?3, updated_at = ?4 WHERE id = ?5",
        params![name, description, enabled as i64, now_utc(), id],
    )?;
    Ok(())
}

pub fn delete_tag(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM tags WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn seed_default_tags(conn: &Connection) -> Result<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM tags", [], |r| r.get(0))?;
    if count > 0 {
        return Ok(());
    }
    let defaults = ["平台经济", "博弈论", "信息不对称", "供应链", "定价", "数字产品"];
    for name in defaults {
        let _ = add_tag(conn, name, None);
    }
    Ok(())
}

// ---------- AI 分析 ----------

pub fn list_pending_papers(conn: &Connection, paper_ids: Option<&[i64]>) -> Result<Vec<Paper>> {
    let mut sql = String::from(
        "SELECT p.*, j.name AS journal_name FROM papers p JOIN journals j ON j.id = p.journal_id \
         WHERE p.analysis_status IN ('pendingAnalysis','analysisFailed') AND p.abstract IS NOT NULL AND p.abstract != ''",
    );
    if let Some(ids) = paper_ids {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        sql.push_str(" AND p.id IN (");
        sql.push_str(&ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(","));
        sql.push_str(")");
    }
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_paper)?;
    rows.collect()
}

#[allow(clippy::too_many_arguments)]
pub fn save_analysis(
    conn: &Connection,
    id: i64,
    chinese_title: &str,
    chinese_abstract: &str,
    one_sentence_summary: &str,
    tag_matches_json: &str,
    total_score: f64,
    model: &str,
    prompt_version: &str,
    evidence_hash: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE papers SET chinese_title=?1, chinese_abstract=?2, one_sentence_summary=?3, tag_matches_json=?4, \
         total_score=?5, model_name=?6, prompt_version=?7, evidence_hash=?8, analyzed_at=?9, analysis_status='analysisSucceeded', updated_at=?10 \
         WHERE id=?11",
        params![
            chinese_title,
            chinese_abstract,
            one_sentence_summary,
            tag_matches_json,
            total_score,
            model,
            prompt_version,
            evidence_hash,
            now_utc(),
            now_utc(),
            id
        ],
    )?;
    Ok(())
}

pub fn mark_analysis_failed(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE papers SET analysis_status='analysisFailed', updated_at=?1 WHERE id=?2",
        params![now_utc(), id],
    )?;
    Ok(())
}

pub fn set_retry_count(conn: &Connection, id: i64, count: i64) -> Result<()> {
    conn.execute(
        "UPDATE papers SET retry_count = ?1, updated_at = ?2 WHERE id = ?3",
        params![count, now_utc(), id],
    )?;
    Ok(())
}

pub fn get_evidence_hash(conn: &Connection, id: i64) -> Result<Option<String>> {
    let v: Option<String> = conn
        .query_row(
            "SELECT evidence_hash FROM papers WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    Ok(v)
}

pub fn set_paper_flag(conn: &Connection, id: i64, flag: &str, value: bool) -> Result<()> {
    let col = match flag {
        "favorite" => "is_favorite",
        "read" => "is_read",
        "ignored" => "is_ignored",
        _ => return Err(rusqlite::Error::InvalidParameterName(flag.to_string())),
    };
    conn.execute(
        &format!("UPDATE papers SET {} = ?1, updated_at = ?2 WHERE id = ?3", col),
        params![value as i64, now_utc(), id],
    )?;
    Ok(())
}

pub fn get_analysis_status(conn: &Connection, id: i64) -> Result<Option<String>> {
    let v: Option<String> = conn
        .query_row(
            "SELECT analysis_status FROM papers WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    Ok(v)
}

/// 取单篇论文的 (标题, 摘要)，用于 AI 队列。
/// 返回 (title, abstract, abstract_quality)。
pub fn get_paper_title_abstract(conn: &Connection, id: i64) -> Result<Option<(String, String, String)>> {
    conn.query_row(
        "SELECT COALESCE(title,''), COALESCE(abstract,''), COALESCE(abstract_quality,'missing') FROM papers WHERE id=?1",
        params![id],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        },
    )
    .optional()
}

// ---------- app_state（键值持久化） ----------

pub fn get_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM app_state WHERE key = ?1", params![key], |r| r.get(0))
        .optional()
        .ok()
        .flatten()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO app_state (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

// ---------- AI 队列 ----------

#[allow(dead_code)] // 预留：按状态统计（AI 状态筛选）
pub fn count_by_status(conn: &Connection, status: &str) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM papers WHERE analysis_status = ?1",
        params![status],
        |r| r.get(0),
    )
}

/// 队列中尚未完成的论文数（queued + analyzing）。
pub fn count_active_queue(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM papers WHERE analysis_status IN ('queued','analyzing')",
        [],
        |r| r.get(0),
    )
}

pub fn list_queued_ids(conn: &Connection, limit: i64) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM papers WHERE analysis_status = 'queued' ORDER BY queued_at ASC, id ASC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], |r| r.get::<_, i64>(0))?;
    rows.collect()
}

/// 仅把 pendingAnalysis 论文入队（已成功/已入队的不重复入队）。
pub fn enqueue_paper(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE papers SET analysis_status = 'queued', queued_at = ?1, retry_count = 0, updated_at = ?1 WHERE id = ?2 AND analysis_status = 'pendingAnalysis'",
        params![now_utc(), id],
    )?;
    Ok(())
}

pub fn set_paper_status(conn: &Connection, id: i64, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE papers SET analysis_status = ?1, updated_at = ?2 WHERE id = ?3",
        params![status, now_utc(), id],
    )?;
    Ok(())
}

/// 停止：未完成的 queued/analyzing 论文退回 pendingAnalysis（不得标为失败）。
pub fn revert_active_to_pending(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE papers SET analysis_status = 'pendingAnalysis', updated_at = ?1 WHERE analysis_status IN ('queued','analyzing')",
        params![now_utc()],
    )?;
    Ok(())
}

/// 启动恢复：中断的 analyzing 论文退回 queued（作为剩余任务继续）。
pub fn recover_analyzing_to_queued(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE papers SET analysis_status = 'queued', queued_at = ?1, updated_at = ?1 WHERE analysis_status = 'analyzing'",
        params![now_utc()],
    )?;
    Ok(())
}

/// 重试失败：analysisFailed → pendingAnalysis。
pub fn reset_failed_to_pending(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE papers SET analysis_status = 'pendingAnalysis', retry_count = 0, updated_at = ?1 WHERE analysis_status = 'analysisFailed'",
        params![now_utc()],
    )?;
    Ok(())
}

/// 列出所有 analysisFailed 论文 id。
pub fn list_failed_ids(conn: &Connection) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM papers WHERE analysis_status = 'analysisFailed'")?;
    let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
    rows.collect()
}

// ================= Round 4：Batch CRUD =================

// ---------- SyncBatch ----------

pub fn create_sync_batch(conn: &Connection, trigger: &str) -> Result<i64> {
    let now = now_utc();
    conn.execute(
        "INSERT INTO sync_batches (trigger, status, created_at, started_at) VALUES (?1, 'running', ?2, ?2)",
        params![trigger, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn set_sync_batch_journal_total(conn: &Connection, id: i64, total: i64) -> Result<()> {
    conn.execute(
        "UPDATE sync_batches SET journal_total=?1 WHERE id=?2",
        params![total, id],
    )?;
    Ok(())
}

pub fn update_sync_batch_journal_progress(
    conn: &Connection,
    id: i64,
    journal_completed: i64,
    journal_failed: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE sync_batches SET journal_completed=?1, journal_failed=?2 WHERE id=?3",
        params![journal_completed, journal_failed, id],
    )?;
    Ok(())
}

pub fn update_sync_batch_counts(
    conn: &Connection,
    id: i64,
    records_found: i64,
    papers_inserted: i64,
    papers_existing: i64,
    abstracts_added: i64,
    abstracts_upgraded: i64,
    waiting_abstract: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE sync_batches SET records_found=?1, papers_inserted=?2, papers_existing=?3, abstracts_added=?4, abstracts_upgraded=?5, waiting_abstract=?6 WHERE id=?7",
        params![
            records_found,
            papers_inserted,
            papers_existing,
            abstracts_added,
            abstracts_upgraded,
            waiting_abstract,
            id
        ],
    )?;
    Ok(())
}

pub fn add_sync_batch_papers(
    conn: &Connection,
    batch_id: i64,
    inserted: &[i64],
    existing: &[i64],
    abstract_updated: &[i64],
) -> Result<()> {
    for id in inserted {
        conn.execute(
            "INSERT INTO sync_batch_papers (sync_batch_id, paper_id, result) VALUES (?1,?2,'inserted')",
            params![batch_id, id],
        )?;
    }
    for id in existing {
        conn.execute(
            "INSERT INTO sync_batch_papers (sync_batch_id, paper_id, result) VALUES (?1,?2,'existing')",
            params![batch_id, id],
        )?;
    }
    for id in abstract_updated {
        conn.execute(
            "INSERT INTO sync_batch_papers (sync_batch_id, paper_id, result) VALUES (?1,?2,'abstractUpdated')",
            params![batch_id, id],
        )?;
    }
    Ok(())
}

pub fn finalize_sync_batch(
    conn: &Connection,
    id: i64,
    status: &str,
    error_summary: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE sync_batches SET status=?1, finished_at=?2, error_summary=?3 WHERE id=?4",
        params![status, now_utc(), error_summary, id],
    )?;
    Ok(())
}

fn row_to_sync_batch(row: &rusqlite::Row) -> Result<SyncBatch> {
    Ok(SyncBatch {
        id: row.get("id")?,
        trigger: row.get("trigger")?,
        status: row.get("status")?,
        created_at: row.get("created_at")?,
        started_at: row.get("started_at")?,
        finished_at: row.get("finished_at")?,
        journal_total: row.get("journal_total")?,
        journal_completed: row.get("journal_completed")?,
        journal_failed: row.get("journal_failed")?,
        records_found: row.get("records_found")?,
        papers_inserted: row.get("papers_inserted")?,
        papers_existing: row.get("papers_existing")?,
        abstracts_added: row.get("abstracts_added")?,
        waiting_abstract: row.get("waiting_abstract")?,
        error_summary: row.get("error_summary")?,
    })
}

pub fn get_sync_batch(conn: &Connection, id: i64) -> Result<Option<SyncBatch>> {
    conn.query_row("SELECT * FROM sync_batches WHERE id=?1", params![id], row_to_sync_batch)
        .optional()
}

pub fn list_sync_batches(conn: &Connection, limit: i64) -> Result<Vec<SyncBatch>> {
    let mut stmt = conn.prepare("SELECT * FROM sync_batches ORDER BY id DESC LIMIT ?1")?;
    let rows = stmt.query_map(params![limit], row_to_sync_batch)?;
    rows.collect()
}

pub fn get_running_sync_batch(conn: &Connection) -> Result<Option<SyncBatch>> {
    conn.query_row(
        "SELECT * FROM sync_batches WHERE status='running' ORDER BY id DESC LIMIT 1",
        [],
        row_to_sync_batch,
    )
    .optional()
}

pub fn last_finished_sync_batch(conn: &Connection) -> Result<Option<SyncBatch>> {
    conn.query_row(
        "SELECT * FROM sync_batches WHERE status != 'running' ORDER BY id DESC LIMIT 1",
        [],
        row_to_sync_batch,
    )
    .optional()
}

pub fn list_sync_batch_papers(conn: &Connection, batch_id: i64) -> Result<Vec<SyncBatchPaper>> {
    let mut stmt = conn.prepare(
        "SELECT sbp.sync_batch_id, sbp.paper_id, sbp.result, p.title AS title
         FROM sync_batch_papers sbp LEFT JOIN papers p ON p.id = sbp.paper_id
         WHERE sbp.sync_batch_id=?1 ORDER BY sbp.id ASC",
    )?;
    let rows = stmt.query_map(params![batch_id], |r| {
        Ok(SyncBatchPaper {
            sync_batch_id: r.get("sync_batch_id")?,
            paper_id: r.get("paper_id")?,
            result: r.get("result")?,
            title: r.get("title")?,
        })
    })?;
    rows.collect()
}

// ---------- AnalysisBatch ----------

#[allow(clippy::too_many_arguments)]
pub fn create_analysis_batch(
    conn: &Connection,
    trigger: &str,
    model: Option<&str>,
    prompt_version: Option<&str>,
    source_sync_batch_id: Option<i64>,
    parent_batch_id: Option<i64>,
    paper_ids: &[i64],
) -> Result<i64> {
    let now = now_utc();
    let total = paper_ids.len() as i64;
    conn.execute(
        "INSERT INTO analysis_batches (source_sync_batch_id, parent_batch_id, trigger, status, model_name, prompt_version, created_at, started_at, total, remaining)
         VALUES (?1,?2,?3,'running',?4,?5,?6,?6,?7,?7)",
        params![source_sync_batch_id, parent_batch_id, trigger, model, prompt_version, now, total],
    )?;
    let batch_id = conn.last_insert_rowid();
    for pid in paper_ids {
        conn.execute(
            "INSERT INTO analysis_batch_items (analysis_batch_id, paper_id, status, attempt_count) VALUES (?1,?2,'queued',0)",
            params![batch_id, pid],
        )?;
    }
    Ok(batch_id)
}

pub fn set_analysis_batch_status(
    conn: &Connection,
    id: i64,
    status: &str,
    finished_at: Option<&str>,
    error_summary: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE analysis_batches SET status=?1, finished_at=?2, error_summary=?3 WHERE id=?4",
        params![status, finished_at, error_summary, id],
    )?;
    Ok(())
}

/// 由 items 表重算聚合计数（completed/succeeded/failed/skipped/remaining）。
pub fn recompute_analysis_aggregate(conn: &Connection, batch_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE analysis_batches SET
            completed = (SELECT COUNT(*) FROM analysis_batch_items WHERE analysis_batch_id=?1 AND status IN ('succeeded','failed','skipped','cancelled')),
            succeeded  = (SELECT COUNT(*) FROM analysis_batch_items WHERE analysis_batch_id=?1 AND status='succeeded'),
            failed     = (SELECT COUNT(*) FROM analysis_batch_items WHERE analysis_batch_id=?1 AND status='failed'),
            skipped    = (SELECT COUNT(*) FROM analysis_batch_items WHERE analysis_batch_id=?1 AND status='skipped'),
            remaining  = (SELECT COUNT(*) FROM analysis_batch_items WHERE analysis_batch_id=?1 AND status IN ('queued','running'))
         WHERE id=?1",
        params![batch_id],
    )?;
    Ok(())
}

pub fn set_item_status(
    conn: &Connection,
    batch_id: i64,
    paper_id: i64,
    status: &str,
    attempt_count: Option<i64>,
    error_type: Option<&str>,
    error_summary: Option<&str>,
    finished_at: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE analysis_batch_items SET status=?1, attempt_count=COALESCE(?2, attempt_count), error_type=?3, error_summary=?4, finished_at=COALESCE(?5, finished_at) WHERE analysis_batch_id=?6 AND paper_id=?7",
        params![status, attempt_count, error_type, error_summary, finished_at, batch_id, paper_id],
    )?;
    Ok(())
}

pub fn set_item_started(conn: &Connection, batch_id: i64, paper_id: i64, attempt_count: i64) -> Result<()> {
    conn.execute(
        "UPDATE analysis_batch_items SET status='running', attempt_count=?1, started_at=?2, finished_at=NULL WHERE analysis_batch_id=?3 AND paper_id=?4",
        params![attempt_count, now_utc(), batch_id, paper_id],
    )?;
    Ok(())
}

pub fn cancel_queued_items(conn: &Connection, batch_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE analysis_batch_items SET status='cancelled', finished_at=?1 WHERE analysis_batch_id=?2 AND status IN ('queued','running')",
        params![now_utc(), batch_id],
    )?;
    Ok(())
}

/// 最近一个有失败的 AnalysisBatch（作为 retry 的 parent）。
pub fn last_analysis_batch_with_failures(conn: &Connection) -> Result<Option<i64>> {
    let v: Option<i64> = conn
        .query_row(
            "SELECT id FROM analysis_batches WHERE failed > 0 ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(v)
}

pub fn get_analysis_batch(conn: &Connection, id: i64) -> Result<Option<AnalysisBatch>> {
    conn.query_row("SELECT * FROM analysis_batches WHERE id=?1", params![id], row_to_analysis_batch)
        .optional()
}

pub fn list_analysis_batches(conn: &Connection, limit: i64) -> Result<Vec<AnalysisBatch>> {
    let mut stmt = conn.prepare("SELECT * FROM analysis_batches ORDER BY id DESC LIMIT ?1")?;
    let rows = stmt.query_map(params![limit], row_to_analysis_batch)?;
    rows.collect()
}

pub fn get_current_analysis_batch(conn: &Connection) -> Result<Option<AnalysisBatch>> {
    conn.query_row(
        "SELECT * FROM analysis_batches WHERE status IN ('running','paused') ORDER BY id DESC LIMIT 1",
        [],
        row_to_analysis_batch,
    )
    .optional()
}

pub fn last_finished_analysis_batch(conn: &Connection) -> Result<Option<AnalysisBatch>> {
    conn.query_row(
        "SELECT * FROM analysis_batches WHERE status NOT IN ('running','paused') ORDER BY id DESC LIMIT 1",
        [],
        row_to_analysis_batch,
    )
    .optional()
}

fn row_to_analysis_batch(row: &rusqlite::Row) -> Result<AnalysisBatch> {
    Ok(AnalysisBatch {
        id: row.get("id")?,
        source_sync_batch_id: row.get("source_sync_batch_id")?,
        parent_batch_id: row.get("parent_batch_id")?,
        trigger: row.get("trigger")?,
        status: row.get("status")?,
        model_name: row.get("model_name")?,
        prompt_version: row.get("prompt_version")?,
        created_at: row.get("created_at")?,
        started_at: row.get("started_at")?,
        finished_at: row.get("finished_at")?,
        total: row.get("total")?,
        completed: row.get("completed")?,
        succeeded: row.get("succeeded")?,
        failed: row.get("failed")?,
        skipped: row.get("skipped")?,
        remaining: row.get("remaining")?,
        error_summary: row.get("error_summary")?,
    })
}

pub fn list_analysis_batch_items(conn: &Connection, batch_id: i64) -> Result<Vec<AnalysisBatchItem>> {
    let mut stmt = conn.prepare(
        "SELECT abi.id, abi.analysis_batch_id, abi.paper_id, abi.status, abi.attempt_count, abi.started_at, abi.finished_at, abi.error_type, abi.error_summary, p.title AS title
         FROM analysis_batch_items abi LEFT JOIN papers p ON p.id = abi.paper_id
         WHERE abi.analysis_batch_id=?1 ORDER BY abi.id ASC",
    )?;
    let rows = stmt.query_map(params![batch_id], |r| {
        Ok(AnalysisBatchItem {
            id: r.get("id")?,
            analysis_batch_id: r.get("analysis_batch_id")?,
            paper_id: r.get("paper_id")?,
            status: r.get("status")?,
            attempt_count: r.get("attempt_count")?,
            started_at: r.get("started_at")?,
            finished_at: r.get("finished_at")?,
            error_type: r.get("error_type")?,
            error_summary: r.get("error_summary")?,
            title: r.get("title")?,
        })
    })?;
    rows.collect()
}

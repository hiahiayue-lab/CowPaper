use rusqlite::{params, Connection, OptionalExtension, Result};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::models::{
    AnalysisBatch, AnalysisBatchItem, Author, Journal, Paper, PaperCandidate,
    AbstractRecoveryBatch, AbstractRecoveryItem, RecommendationItem, RecommendationRun, SyncBatch, SyncBatchPaper, Tag, TagMatch,
    UpsertOutcome, IDT_ONLINE, IDT_PRINT, SBC_FAILED, ST_PENDING, ST_SUCCEEDED,
    ST_WAITING_ABSTRACT,
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

/// 当前 schema 版本（Round 5B：abstract_quality / paper_abstract_sources 为 v4；
/// Round 7 Phase 1：content_kind / abstract_status 为 v13；
/// Literature Workspace 为 v14；v15 为 Library Attachments + User Metadata。
/// 生产构建中仅由迁移系统隐式使用；测试中直接断言。
#[allow(dead_code)]
pub const SCHEMA_VERSION: i64 = 15;

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

/// Bind a known normalized identifier to its canonical Journal. Unlike the
/// migration-oriented `insert_identifier`, this checks ownership first and
/// updates the identifier type when a user explicitly supplies print/online.
/// It never silently moves an identifier between journals.
pub fn bind_journal_identifier(
    conn: &Connection,
    journal_id: i64,
    identifier_type: &str,
    value: &str,
    source: Option<&str>,
) -> Result<()> {
    if let Some(owner) = resolve_journal_by_identifier(conn, value)? {
        if owner != journal_id {
            return Err(rusqlite::Error::InvalidQuery);
        }
        conn.execute(
            "UPDATE journal_identifiers SET identifier_type=?1, source=COALESCE(source,?2), updated_at=?3 WHERE value=?4",
            params![identifier_type, source, now_utc(), value],
        )?;
        return Ok(());
    }
    insert_identifier(conn, journal_id, identifier_type, value, source)
}

/// Enrich only empty legacy display columns. Canonical identity remains in
/// journal_identifiers; this keeps existing rows backward-compatible without
/// overwriting known values.
pub fn fill_journal_issn_columns(
    conn: &Connection,
    journal_id: i64,
    print_issn: Option<&str>,
    online_issn: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE journals
         SET print_issn=COALESCE(print_issn,?1), online_issn=COALESCE(online_issn,?2), updated_at=?3
         WHERE id=?4",
        params![print_issn, online_issn, now_utc(), journal_id],
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

/// 读取 journals.openalex_source_id（仅填空用）。
pub fn get_journal_openalex_source(conn: &Connection, id: i64) -> Result<Option<String>> {
    let v: Option<String> = conn
        .query_row(
            "SELECT openalex_source_id FROM journals WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    Ok(v)
}

/// 设置 journals.openalex_source_id（仅当调用方判断为空时）。
pub fn set_journal_openalex_source(conn: &Connection, id: i64, sid: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE journals SET openalex_source_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![sid, now_utc(), id],
    )?;
    Ok(())
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
        "SELECT c.id, c.code, c.name, c.version, c.effective_from, c.source_name, c.source_url, c.last_verified_at, c.created_at, c.updated_at,
            (SELECT COUNT(*) FROM journal_collection_members m WHERE m.collection_id = c.id) AS member_count
         FROM journal_collections c ORDER BY c.code",
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
            member_count: r.get(10)?,
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
        "SELECT c.id, c.code, c.name, c.version, c.effective_from, c.source_name, c.source_url, c.last_verified_at, c.created_at, c.updated_at,
            0 AS member_count
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
            member_count: r.get(10)?,
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
                    abstract_source_url = ?5, evidence_hash = NULL
                 WHERE id = ?6",
                params![best.text, best.source, best.quality, now, c.abstract_source_url, paper_id],
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

    // Round 7 Phase 1：补充分类（unknown → 有证据才填）+ 重算 abstract_status。
    // 摘要被填/升级/清空后，语义状态必须与 content_kind + 摘要有无保持一致。
    fill_content_kind_if_unknown(conn, paper_id, c)?;
    refresh_abstract_status(conn, paper_id)?;

    Ok((filled, upgraded))
}

/// Round 7 Phase 1：若 content_kind 仍为 unknown，用候选中的 raw JSON
/// （provider explicit type）+ title 补充分类。绝不覆盖已分类结果。
fn fill_content_kind_if_unknown(conn: &Connection, paper_id: i64, c: &PaperCandidate) -> Result<()> {
    let current: String = conn
        .query_row(
            "SELECT content_kind FROM papers WHERE id = ?1",
            params![paper_id],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| crate::content_kind::CK_UNKNOWN.to_string());
    if current != crate::content_kind::CK_UNKNOWN {
        return Ok(());
    }
    let raw = match c.raw_json.as_deref() {
        Some(raw) => raw,
        None => return Ok(()),
    };
    let (provider, ty) = match crate::content_kind::provider_type_from_raw_json(raw) {
        Some(v) => v,
        None => return Ok(()),
    };
    let (crossref_type, openalex_type) = match provider {
        "crossref" => (Some(ty.as_str()), None),
        "openalex" => (None, Some(ty.as_str())),
        _ => return Ok(()),
    };
    let res = crate::content_kind::resolve_content_kind(crossref_type, openalex_type, c.title.as_deref());
    if res.kind != crate::content_kind::CK_UNKNOWN {
        conn.execute(
            "UPDATE papers SET content_kind=?1, content_kind_source=?2, content_kind_confidence=?3 WHERE id=?4",
            params![res.kind, res.source, res.confidence, paper_id],
        )?;
    }
    Ok(())
}

/// Round 7 Phase 1：重算 abstract_status（content_kind + 摘要有无 的纯函数）。
/// 摘要被填/升级/清空后调用，保证语义状态始终一致。
pub fn refresh_abstract_status(conn: &Connection, paper_id: i64) -> Result<()> {
    let (kind, has_abstract): (String, i64) = conn.query_row(
        "SELECT content_kind,
                CASE WHEN abstract IS NOT NULL AND abstract != '' THEN 1 ELSE 0 END
         FROM papers WHERE id = ?1",
        params![paper_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let status = crate::content_kind::abstract_status_for(&kind, has_abstract != 0);
    conn.execute(
        "UPDATE papers SET abstract_status=?1 WHERE id=?2",
        params![status, paper_id],
    )?;
    Ok(())
}

/// Feed a public recovery result back through the same source ledger and
/// canonical selector used by normal sync; never creates a second abstract
/// field or bypasses quality/upgraded semantics.
pub fn merge_recovered_abstract(
    conn: &Connection,
    paper_id: i64,
    source: &str,
    abstract_text: &str,
) -> Result<(bool, bool)> {
    merge_recovered_abstract_with_url(conn, paper_id, source, abstract_text, None)
}

/// Recovery 变体：记录摘要来源落地页 URL（provenance，如 publisher:nature 页面地址）。
pub fn merge_recovered_abstract_with_url(
    conn: &Connection,
    paper_id: i64,
    source: &str,
    abstract_text: &str,
    abstract_source_url: Option<&str>,
) -> Result<(bool, bool)> {
    merge_abstract(conn, paper_id, &PaperCandidate {
        normalized_doi: None, original_doi: None, title: None, authors: Vec::new(),
        published_date: None, year: None, abstract_text: Some(abstract_text.to_string()),
        abstract_source: Some(source.to_string()),
        abstract_source_url: abstract_source_url.map(str::to_string),
        url: None, publisher_article_id: None,
        openalex_work_id: None, discovery_source: source.to_string(), source_id: None, raw_json: None,
    })
}

/// A recovery attempt is recorded even when public sources have no abstract.
/// This timestamp/count drives the bounded retry cadence and is never applied
/// to complete papers.
pub fn mark_abstract_recovery_attempt(conn: &Connection, paper_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE papers SET abstract_last_checked_at=?1, abstract_retry_count=abstract_retry_count+1, updated_at=?1
         WHERE id=?2 AND abstract_quality != 'complete'",
        params![now_utc(), paper_id],
    )?;
    Ok(())
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

    Ok(UpsertOutcome::New(insert_paper_without_identity_merge(conn, journal_id, c)?))
}

/// Insert a canonical Paper without running identity matching first. This is
/// used only after an external PDF import has already checked exact DOI and
/// scholarly identity and has handled any title/author/year candidate through
/// explicit UI confirmation. It prevents the generic title/year fallback in
/// `upsert_paper` from silently merging an unconfirmed PDF.
fn insert_paper_without_identity_merge(conn: &Connection, journal_id: i64, c: &PaperCandidate) -> Result<i64> {
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
    // Round 7 Phase 1：内容类型解析（provider explicit type → title heuristic → unknown）
    let mut crossref_type: Option<String> = None;
    let mut openalex_type: Option<String> = None;
    if let Some(raw) = c.raw_json.as_deref() {
        if let Some((provider, ty)) = crate::content_kind::provider_type_from_raw_json(raw) {
            match provider {
                "crossref" => crossref_type = Some(ty),
                "openalex" => openalex_type = Some(ty),
                _ => {}
            }
        }
    }
    let ck = crate::content_kind::resolve_content_kind(
        crossref_type.as_deref(),
        openalex_type.as_deref(),
        c.title.as_deref(),
    );
    let has_abstract = abs_quality != crate::models::ABQ_MISSING;
    let abstract_status = crate::content_kind::abstract_status_for(&ck.kind, has_abstract);
    let now = now_utc();
    let first_seen_cycle = chrono::Local::now().format("%Y-%m-%d").to_string();

    conn.execute(
        "INSERT INTO papers (
            journal_id, normalized_doi, original_doi, title, title_norm, authors_json,
            published_date, year, abstract, abstract_source, abstract_retrieved_at, abstract_source_url,
            url, publisher_article_id, openalex_work_id, discovery_source,
            analysis_status, abstract_quality, abstract_last_checked_at, first_seen_cycle, first_seen_abstract_missing,
            content_kind, content_kind_source, content_kind_confidence, abstract_status,
            created_at, updated_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?26)",
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
            None::<String>, // abstract_source_url：初始来源由 abstract_source 标识（crossref/openalex API）
            c.url,
            c.publisher_article_id,
            c.openalex_work_id,
            c.discovery_source,
            analysis_status,
            abs_quality,
            now.clone(),
            first_seen_cycle,
            if abs_quality == crate::models::ABQ_MISSING { 1 } else { 0 },
            ck.kind,
            ck.source,
            ck.confidence,
            abstract_status,
            now
        ],
    )?;
    let id = conn.last_insert_rowid();
    // 记录初始来源候选
    if let (Some(t), Some(src)) = (&abs_norm, &c.abstract_source) {
        let (q, r) = crate::abstract_quality::assess_abstract_quality(t);
        let _ = record_abstract_source(conn, id, src, t, q, r);
    }
    Ok(id)
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
        abstract_source_url: row.get("abstract_source_url")?,
        content_kind: row.get("content_kind")?,
        content_kind_source: row.get("content_kind_source")?,
        content_kind_confidence: row.get("content_kind_confidence")?,
        abstract_status: row.get("abstract_status")?,
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
    filter_current_tag_matches(conn, &mut papers)?;
    Ok(papers)
}

pub fn list_papers_for_first_seen_cycle(conn: &Connection, cycle_key: &str, missing_only: bool) -> Result<Vec<Paper>> {
    let sql = if missing_only {
        "SELECT p.*,j.name AS journal_name FROM papers p JOIN journals j ON j.id=p.journal_id WHERE p.first_seen_cycle=?1 AND p.first_seen_abstract_missing=1 ORDER BY p.id DESC"
    } else {
        "SELECT p.*,j.name AS journal_name FROM papers p JOIN journals j ON j.id=p.journal_id WHERE p.first_seen_cycle=?1 ORDER BY p.id DESC"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![cycle_key], row_to_paper)?;
    let mut papers: Vec<Paper> = rows.collect::<Result<Vec<_>>>()?;
    enrich_papers_collections(conn, &mut papers)?;
    filter_current_tag_matches(conn, &mut papers)?;
    Ok(papers)
}

pub fn list_current_missing_papers_for_cycle(conn: &Connection, cycle_key: &str) -> Result<Vec<Paper>> {
    let mut stmt = conn.prepare("SELECT p.*,j.name AS journal_name FROM papers p JOIN journals j ON j.id=p.journal_id WHERE p.first_seen_cycle=?1 AND p.abstract_quality='missing' ORDER BY p.id DESC")?;
    let rows = stmt.query_map(params![cycle_key], row_to_paper)?;
    let mut papers: Vec<Paper> = rows.collect::<Result<Vec<_>>>()?;
    enrich_papers_collections(conn, &mut papers)?;
    filter_current_tag_matches(conn, &mut papers)?;
    Ok(papers)
}

pub fn list_daily_paper_summaries(conn: &Connection) -> Result<Vec<crate::models::DailyPaperSummary>> {
    let mut stmt = conn.prepare(
        "WITH days AS (
             SELECT first_seen_cycle AS cycle_key FROM papers WHERE first_seen_cycle IS NOT NULL
             UNION SELECT cycle_key FROM recommendation_runs
         ), paper_counts AS (
             SELECT first_seen_cycle AS cycle_key, COUNT(DISTINCT id) AS paper_count,
                    COUNT(DISTINCT CASE WHEN first_seen_abstract_missing=1 THEN id END) AS missing_count
             FROM papers WHERE first_seen_cycle IS NOT NULL GROUP BY first_seen_cycle
         ), recommendation_counts AS (
             SELECT r.cycle_key, r.id AS run_id, COUNT(DISTINCT ri.paper_id) AS recommendation_count
             FROM recommendation_runs r LEFT JOIN recommendation_items ri ON ri.run_id=r.id GROUP BY r.id,r.cycle_key
         )
         SELECT d.cycle_key,COALESCE(p.paper_count,0),COALESCE(p.missing_count,0),rc.run_id,COALESCE(rc.recommendation_count,0)
         FROM days d LEFT JOIN paper_counts p ON p.cycle_key=d.cycle_key
         LEFT JOIN recommendation_counts rc ON rc.cycle_key=d.cycle_key ORDER BY d.cycle_key DESC"
    )?;
    let rows = stmt.query_map([], |r| Ok(crate::models::DailyPaperSummary { cycle_key:r.get(0)?, paper_count:r.get(1)?, missing_count:r.get(2)?, recommendation_run_id:r.get(3)?, recommendation_count:r.get(4)? }))?;
    rows.collect()
}

/// Paper DTO 过滤：只保留当前有效 tag score（active + enabled + tag_id 精确匹配 + hash 一致）。
/// 不修改 tag_matches_json（cache 完整保留）。
fn filter_current_tag_matches(conn: &Connection, papers: &mut [Paper]) -> Result<()> {
    if papers.is_empty() {
        return Ok(());
    }
    let active = crate::tag_config::active_tags(conn).unwrap_or_default();
    for p in papers.iter_mut() {
        p.tag_matches
            .retain(|m| crate::tag_config::is_current_tag_match_valid(m, &active));
    }
    Ok(())
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

/// 单篇论文（含 collections 派生）。
pub fn get_paper(conn: &Connection, id: i64) -> Result<Option<Paper>> {
    let p = conn
        .query_row(
            "SELECT p.*, j.name AS journal_name FROM papers p LEFT JOIN journals j ON j.id = p.journal_id WHERE p.id = ?1",
            params![id],
            row_to_paper,
        )
        .optional()?;
    let mut v = Vec::new();
    if let Some(p) = p {
        v.push(p);
        enrich_papers_collections(conn, &mut v)?;
        filter_current_tag_matches(conn, &mut v)?;
        Ok(v.pop())
    } else {
        Ok(None)
    }
}

// ---------- Literature Workspace ----------

fn library_collection_from_row(row: &rusqlite::Row) -> Result<crate::models::LibraryCollection> {
    Ok(crate::models::LibraryCollection {
        id: row.get("id")?,
        parent_id: row.get("parent_id")?,
        name: row.get("name")?,
        sort_order: row.get("sort_order")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn library_tag_from_row(row: &rusqlite::Row) -> Result<crate::models::LibraryTag> {
    Ok(crate::models::LibraryTag {
        id: row.get("id")?,
        name: row.get("name")?,
        color: row.get("color")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn paper_exists(conn: &Connection, paper_id: i64) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM papers WHERE id = ?1)",
        params![paper_id],
        |r| r.get(0),
    )
}

fn library_item_exists(conn: &Connection, paper_id: i64) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM library_items WHERE paper_id = ?1)",
        params![paper_id],
        |r| r.get(0),
    )
}

fn validate_collection_ids(conn: &Connection, ids: &[i64]) -> Result<()> {
    for id in ids {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM library_collections WHERE id = ?1)",
            params![id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
    }
    Ok(())
}

fn validate_library_tag_ids(conn: &Connection, ids: &[i64]) -> Result<()> {
    for id in ids {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM library_tags WHERE id = ?1)",
            params![id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
    }
    Ok(())
}

fn library_metadata(
    conn: &Connection,
    paper_id: i64,
) -> Result<(String, String, Vec<crate::models::LibraryCollection>, Vec<crate::models::LibraryTag>)> {
    let (added_at, added_source): (String, String) = conn.query_row(
        "SELECT added_at, added_source FROM library_items WHERE paper_id = ?1",
        params![paper_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let mut collections_stmt = conn.prepare(
        "SELECT c.* FROM library_collections c
         JOIN library_collection_items i ON i.collection_id = c.id
         WHERE i.paper_id = ?1 ORDER BY c.parent_id IS NOT NULL, c.sort_order, c.name, c.id",
    )?;
    let collections = collections_stmt
        .query_map(params![paper_id], library_collection_from_row)?
        .collect::<Result<Vec<_>>>()?;
    let mut tags_stmt = conn.prepare(
        "SELECT t.* FROM library_tags t
         JOIN library_item_tags i ON i.tag_id = t.id
         WHERE i.paper_id = ?1 ORDER BY t.name, t.id",
    )?;
    let tags = tags_stmt
        .query_map(params![paper_id], library_tag_from_row)?
        .collect::<Result<Vec<_>>>()?;
    Ok((added_at, added_source, collections, tags))
}

fn library_paper(conn: &Connection, paper: Paper) -> Result<crate::models::LibraryPaper> {
    let (added_at, added_source, collections, tags) = library_metadata(conn, paper.id)?;
    let metadata = get_library_item_metadata(conn, paper.id)?;
    let effective_title = metadata.as_ref().and_then(|m| m.title_override.clone()).or_else(|| paper.title.clone());
    let effective_chinese_title = metadata
        .as_ref()
        .and_then(|m| m.chinese_title_override.clone())
        .or_else(|| paper.chinese_title.clone());
    let effective_source = metadata
        .as_ref()
        .and_then(|m| m.source_override.clone())
        .or_else(|| paper.journal_name.clone());
    let effective_year = metadata.as_ref().and_then(|m| m.year_override).or(paper.year);
    let effective_authors = metadata
        .as_ref()
        .and_then(|m| m.authors_override.clone())
        .unwrap_or_else(|| paper.authors.clone());
    let effective_abstract = metadata
        .as_ref()
        .and_then(|m| m.abstract_override.clone())
        .or_else(|| paper.abstract_text.clone());
    let effective_chinese_abstract = metadata
        .as_ref()
        .and_then(|m| m.chinese_abstract_override.clone())
        .or_else(|| paper.chinese_abstract.clone());
    let note = metadata.as_ref().and_then(|m| m.note.clone());
    let attachments = list_paper_attachments(conn, paper.id)?;
    Ok(crate::models::LibraryPaper {
        paper,
        added_at,
        added_source,
        collections,
        tags,
        metadata,
        effective_title,
        effective_chinese_title,
        effective_source,
        effective_year,
        effective_authors,
        effective_abstract,
        effective_chinese_abstract,
        note,
        attachments,
    })
}

fn clean_optional_text(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|value| !value.is_empty()).map(str::to_string)
}

fn library_item_metadata_from_row(row: &rusqlite::Row) -> Result<crate::models::LibraryItemMetadata> {
    let authors_override = row
        .get::<_, Option<String>>("authors_override")?
        .and_then(|value| serde_json::from_str::<Vec<crate::models::Author>>(&value).ok());
    Ok(crate::models::LibraryItemMetadata {
        paper_id: row.get("paper_id")?,
        title_override: row.get("title_override")?,
        chinese_title_override: row.get("chinese_title_override")?,
        source_override: row.get("source_override")?,
        year_override: row.get("year_override")?,
        authors_override,
        abstract_override: row.get("abstract_override")?,
        chinese_abstract_override: row.get("chinese_abstract_override")?,
        note: row.get("note")?,
        updated_at: row.get("updated_at")?,
    })
}

/// Read the optional Library metadata row. A missing row means all fields use
/// canonical Paper values; callers should not materialize an empty row merely
/// to display a Library item.
pub fn get_library_item_metadata(
    conn: &Connection,
    paper_id: i64,
) -> Result<Option<crate::models::LibraryItemMetadata>> {
    conn.query_row(
        "SELECT * FROM library_item_metadata WHERE paper_id = ?1",
        params![paper_id],
        library_item_metadata_from_row,
    )
    .optional()
}

/// Replace the Library-only metadata layer. `None` clears an override and
/// therefore restores the canonical value without mutating `papers`.
pub fn set_library_item_metadata(
    conn: &Connection,
    paper_id: i64,
    input: &crate::models::LibraryItemMetadataInput,
) -> Result<crate::models::LibraryItemMetadata> {
    let tx = conn.unchecked_transaction()?;
    if !library_item_exists(&tx, paper_id)? {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    let authors_json = input
        .authors_override
        .as_ref()
        .map(|authors| serde_json::to_string(authors).unwrap_or_else(|_| "[]".to_string()));
    let now = now_utc();
    tx.execute(
        "INSERT INTO library_item_metadata (
            paper_id, title_override, chinese_title_override, source_override,
            year_override, authors_override, abstract_override,
            chinese_abstract_override, note, updated_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
         ON CONFLICT(paper_id) DO UPDATE SET
            title_override=excluded.title_override,
            chinese_title_override=excluded.chinese_title_override,
            source_override=excluded.source_override,
            year_override=excluded.year_override,
            authors_override=excluded.authors_override,
            abstract_override=excluded.abstract_override,
            chinese_abstract_override=excluded.chinese_abstract_override,
            note=excluded.note,
            updated_at=excluded.updated_at",
        params![
            paper_id,
            clean_optional_text(input.title_override.as_deref()),
            clean_optional_text(input.chinese_title_override.as_deref()),
            clean_optional_text(input.source_override.as_deref()),
            input.year_override,
            authors_json,
            clean_optional_text(input.abstract_override.as_deref()),
            clean_optional_text(input.chinese_abstract_override.as_deref()),
            clean_optional_text(input.note.as_deref()),
            now,
        ],
    )?;
    tx.commit()?;
    get_library_item_metadata(conn, paper_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

/// Update only the personal note while retaining all other overrides.
pub fn set_library_item_note(
    conn: &Connection,
    paper_id: i64,
    note: Option<&str>,
) -> Result<crate::models::LibraryItemMetadata> {
    let tx = conn.unchecked_transaction()?;
    if !library_item_exists(&tx, paper_id)? {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    tx.execute(
        "INSERT INTO library_item_metadata (paper_id, note, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(paper_id) DO UPDATE SET note=excluded.note, updated_at=excluded.updated_at",
        params![paper_id, clean_optional_text(note), now_utc()],
    )?;
    tx.commit()?;
    get_library_item_metadata(conn, paper_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

/// Reset all editable metadata fields while preserving the personal note.
pub fn clear_library_item_overrides(
    conn: &Connection,
    paper_id: i64,
) -> Result<Option<crate::models::LibraryItemMetadata>> {
    if !library_item_exists(conn, paper_id)? {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    conn.execute(
        "UPDATE library_item_metadata SET
            title_override=NULL, chinese_title_override=NULL, source_override=NULL,
            year_override=NULL, authors_override=NULL, abstract_override=NULL,
            chinese_abstract_override=NULL, updated_at=?1 WHERE paper_id=?2",
        params![now_utc(), paper_id],
    )?;
    get_library_item_metadata(conn, paper_id)
}

struct LinkedFile {
    absolute_path: PathBuf,
    filename: String,
    sha256: String,
    metadata: crate::models::ExternalPdfMetadata,
}

fn resolve_linked_pdf_path(input: &str) -> Result<PathBuf> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err(rusqlite::Error::InvalidParameterName("path".into()));
    }
    let path = PathBuf::from(raw);
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|_| rusqlite::Error::InvalidQuery)?
            .join(path)
    };
    if !absolute.is_file() {
        return Err(rusqlite::Error::InvalidParameterName("path".into()));
    }
    std::fs::canonicalize(absolute).map_err(|_| rusqlite::Error::InvalidParameterName("path".into()))
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|_| rusqlite::Error::InvalidQuery)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn is_pdf_file(path: &Path) -> Result<bool> {
    if path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("pdf")) {
        return Ok(true);
    }
    let mut file = File::open(path).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let mut header = [0_u8; 5];
    let count = file.read(&mut header).map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(count == 5 && &header == b"%PDF-")
}

fn linked_file(input: &str) -> Result<LinkedFile> {
    let absolute_path = resolve_linked_pdf_path(input)?;
    if !is_pdf_file(&absolute_path)? {
        return Err(rusqlite::Error::InvalidParameterName("pdf_path".into()));
    }
    let filename = absolute_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("document.pdf")
        .to_string();
    let metadata = parse_external_pdf_metadata(&absolute_path, &filename)?;
    let sha256 = sha256_file(&absolute_path)?;
    Ok(LinkedFile { absolute_path, filename, sha256, metadata })
}

fn paper_attachment_from_row(row: &rusqlite::Row) -> Result<crate::models::PaperAttachment> {
    let absolute_path: String = row.get("absolute_path")?;
    Ok(crate::models::PaperAttachment {
        id: row.get("id")?,
        paper_id: row.get("paper_id")?,
        kind: row.get("kind")?,
        storage_mode: row.get("storage_mode")?,
        missing: !Path::new(&absolute_path).is_file(),
        absolute_path,
        relative_path: row.get("relative_path")?,
        url: row.get("url")?,
        filename: row.get("filename")?,
        mime_type: row.get("mime_type")?,
        sha256: row.get("sha256")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub fn get_paper_attachment(conn: &Connection, attachment_id: i64) -> Result<Option<crate::models::PaperAttachment>> {
    conn.query_row(
        "SELECT * FROM paper_attachments WHERE id=?1",
        params![attachment_id],
        paper_attachment_from_row,
    )
    .optional()
}

pub fn list_paper_attachments(conn: &Connection, paper_id: i64) -> Result<Vec<crate::models::PaperAttachment>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM paper_attachments WHERE paper_id=?1 ORDER BY created_at DESC, id DESC",
    )?;
    let rows = stmt.query_map(params![paper_id], paper_attachment_from_row)?;
    rows.collect()
}

fn insert_linked_attachment(
    conn: &Connection,
    paper_id: i64,
    file: &LinkedFile,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO paper_attachments (
            paper_id, kind, storage_mode, absolute_path, relative_path, url,
            filename, mime_type, sha256, created_at, updated_at
         ) VALUES (?1,'pdf','linked',?2,NULL,NULL,?3,'application/pdf',?4,?5,?5)",
        params![paper_id, file.absolute_path.to_string_lossy().as_ref(), file.filename, file.sha256, now_utc()],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Attach an existing local PDF to a canonical Paper. The source file is
/// read only for metadata/hash purposes and is never copied or removed.
pub fn attach_pdf_to_paper(
    conn: &Connection,
    paper_id: i64,
    path: &str,
) -> Result<crate::models::PaperAttachment> {
    let file = linked_file(path)?;
    let tx = conn.unchecked_transaction()?;
    if !paper_exists(&tx, paper_id)? {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    let id = insert_linked_attachment(&tx, paper_id, &file)?;
    tx.commit()?;
    get_paper_attachment(conn, id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

/// Discovery's Attach PDF action is a durable Library action. It atomically
/// creates membership (when absent), clears Read Later, and inserts the link.
pub fn attach_discovery_pdf(
    conn: &Connection,
    paper_id: i64,
    path: &str,
) -> Result<crate::models::PaperAttachment> {
    let file = linked_file(path)?;
    let tx = conn.unchecked_transaction()?;
    if !paper_exists(&tx, paper_id)? {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    let now = now_utc();
    tx.execute(
        "INSERT INTO library_items (paper_id, added_at, added_source)
         VALUES (?1,?2,'discovery_attach_pdf') ON CONFLICT(paper_id) DO NOTHING",
        params![paper_id, now],
    )?;
    tx.execute(
        "UPDATE papers SET is_favorite=0, updated_at=?1 WHERE id=?2",
        params![now, paper_id],
    )?;
    let id = insert_linked_attachment(&tx, paper_id, &file)?;
    tx.commit()?;
    get_paper_attachment(conn, id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

/// Detach only removes the CowPaper relation. It deliberately does not touch
/// the linked source file.
pub fn detach_pdf(conn: &Connection, attachment_id: i64) -> Result<bool> {
    Ok(conn.execute("DELETE FROM paper_attachments WHERE id=?1", params![attachment_id])? == 1)
}

pub fn relink_pdf(
    conn: &Connection,
    attachment_id: i64,
    path: &str,
) -> Result<crate::models::PaperAttachment> {
    let file = linked_file(path)?;
    let current = get_paper_attachment(conn, attachment_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    if current.storage_mode != "linked" {
        return Err(rusqlite::Error::InvalidParameterName("storage_mode".into()));
    }
    conn.execute(
        "UPDATE paper_attachments SET absolute_path=?1, relative_path=NULL,
            url=NULL, filename=?2, mime_type='application/pdf', sha256=?3, updated_at=?4
         WHERE id=?5",
        params![file.absolute_path.to_string_lossy().as_ref(), file.filename, file.sha256, now_utc(), attachment_id],
    )?;
    get_paper_attachment(conn, attachment_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

fn launch_file_action(path: &Path, reveal: bool) -> Result<()> {
    if !path.is_file() {
        return Err(rusqlite::Error::InvalidParameterName("missing_attachment".into()));
    }
    #[cfg(target_os = "macos")]
    let status = if reveal {
        Command::new("open").arg("-R").arg(path).status()
    } else {
        Command::new("open").arg(path).status()
    };
    #[cfg(target_os = "windows")]
    let status = if reveal {
        Command::new("explorer").arg(format!("/select,{}", path.display())).status()
    } else {
        let path_string = path.to_string_lossy().into_owned();
        Command::new("cmd").args(["/C", "start", ""]).arg(path_string).status()
    };
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let status = if reveal {
        Command::new("xdg-open").arg(path.parent().unwrap_or(path)).status()
    } else {
        Command::new("xdg-open").arg(path).status()
    };
    match status {
        Ok(status) if status.success() => Ok(()),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

pub fn open_pdf(conn: &Connection, attachment_id: i64) -> Result<()> {
    let attachment = get_paper_attachment(conn, attachment_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    launch_file_action(Path::new(&attachment.absolute_path), false)
}

pub fn reveal_pdf(conn: &Connection, attachment_id: i64) -> Result<()> {
    let attachment = get_paper_attachment(conn, attachment_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    launch_file_action(Path::new(&attachment.absolute_path), true)
}

fn decode_pdf_literal(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('b') => out.push('\u{0008}'),
            Some('f') => out.push('\u{000c}'),
            Some('\n') => {}
            Some(next @ '0'..='7') => {
                let mut octal = String::from(next);
                for _ in 0..2 {
                    if chars.peek().is_some_and(|c| matches!(c, '0'..='7')) {
                        octal.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                if let Ok(value) = u8::from_str_radix(&octal, 8) {
                    out.push(value as char);
                }
            }
            Some(next) => out.push(next),
            None => out.push('\\'),
        }
    }
    out
}

fn pdf_info_value(text: &str, key: &str) -> Option<String> {
    let marker = format!("/{}", key);
    let start = text.find(&marker)? + marker.len();
    let rest = &text[start..];
    let first = rest.char_indices().find(|(_, ch)| !ch.is_whitespace())?;
    let value = &rest[first.0..];
    if value.starts_with('(') {
        let mut depth = 1_i32;
        let mut escaped = false;
        let mut body = String::new();
        for ch in value[1..].chars() {
            if escaped {
                body.push('\\');
                body.push(ch);
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '(' {
                depth += 1;
                body.push(ch);
            } else if ch == ')' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                body.push(ch);
            } else {
                body.push(ch);
            }
        }
        let value = decode_pdf_literal(&body);
        return clean_optional_text(Some(&value));
    }
    if value.starts_with('<') {
        let end = value.find('>')?;
        return clean_optional_text(Some(&value[1..end]));
    }
    let end = value.find(|ch: char| ch.is_whitespace() || ch == '/' || ch == '>').unwrap_or(value.len());
    clean_optional_text(Some(&value[..end]))
}

fn xml_metadata_value(text: &str, tags: &[&str]) -> Option<String> {
    for tag in tags {
        let open = format!("<{}", tag);
        let Some(start) = text.find(&open) else { continue; };
        let Some(open_end) = text[start..].find('>') else { continue; };
        let content_start = open_end + start + 1;
        let close = format!("</{}>", tag);
        let Some(close_offset) = text[content_start..].find(&close) else { continue; };
        let end = close_offset + content_start;
        let value = crate::util::strip_html(&text[content_start..end]);
        if let Some(value) = clean_optional_text(Some(&value)) {
            return Some(value);
        }
    }
    None
}

fn first_doi(value: Option<&str>) -> Option<String> {
    let value = value?;
    let mut offset = 0;
    while let Some(found) = value[offset..].to_ascii_lowercase().find("10.") {
        let start = offset + found;
        let candidate = value[start..]
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '/' | '-' | '_' | ':' | ';' | '(' | ')'))
            .collect::<String>();
        let candidate = candidate.trim_end_matches(|ch: char| matches!(ch, '.' | ',' | ';' | ':' | ')' | ']'));
        if candidate.contains('/') {
            if let Some(doi) = crate::util::normalize_doi(candidate) {
                if doi.starts_with("10.") && doi.contains('/') {
                    return Some(doi);
                }
            }
        }
        offset = start + 3;
        if offset >= value.len() {
            break;
        }
    }
    None
}

fn author_key(author: &crate::models::Author) -> String {
    author
        .name
        .as_deref()
        .or_else(|| author.family.as_deref())
        .unwrap_or("")
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

fn parse_author_metadata(value: Option<&str>) -> Vec<crate::models::Author> {
    let Some(value) = value else { return Vec::new(); };
    value
        .split(|ch| ch == ';' || ch == '\n' || ch == '\r')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| crate::models::Author { given: None, family: None, name: Some(name.to_string()) })
        .collect()
}

fn parse_year_metadata(value: Option<&str>) -> Option<i32> {
    let value = value?;
    let bytes = value.as_bytes();
    for start in 0..=bytes.len().saturating_sub(4) {
        let part = &bytes[start..start + 4];
        if part.iter().all(|byte| byte.is_ascii_digit()) {
            let year = i32::from(part[0] - b'0') * 1000
                + i32::from(part[1] - b'0') * 100
                + i32::from(part[2] - b'0') * 10
                + i32::from(part[3] - b'0');
            if (1500..=2200).contains(&year) {
                return Some(year);
            }
        }
    }
    None
}

/// Parse only lightweight PDF Info/XMP metadata. This intentionally does not
/// extract PDF text, annotations, or inferred academic facts.
pub fn parse_external_pdf_metadata(path: &Path, filename: &str) -> Result<crate::models::ExternalPdfMetadata> {
    let bytes = std::fs::read(path).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let text = String::from_utf8_lossy(&bytes);
    let title = pdf_info_value(&text, "Title")
        .or_else(|| xml_metadata_value(&text, &["dc:title", "title"]))
        .or_else(|| Path::new(filename).file_stem().and_then(|value| value.to_str()).map(str::to_string));
    let author_value = pdf_info_value(&text, "Author")
        .or_else(|| xml_metadata_value(&text, &["dc:creator", "creator", "Author"]));
    let xmp_doi = xml_metadata_value(&text, &["prism:doi", "bibo:doi", "doi"]);
    let doi = first_doi(pdf_info_value(&text, "DOI").as_deref().or(xmp_doi.as_deref()));
    // A DOI-looking filename is useful only as a deterministic exact identity
    // hint; arbitrary title text is never treated as an identity.
    let doi = doi.or_else(|| first_doi(Some(filename)));
    let scholarly_id = pdf_info_value(&text, "OpenAlex")
        .or_else(|| pdf_info_value(&text, "PMID"))
        .or_else(|| pdf_info_value(&text, "PMCID"))
        .or_else(|| pdf_info_value(&text, "arXiv"))
        .or_else(|| xml_metadata_value(&text, &["openalex", "pmid", "pmcid", "arXiv"]));
    let abstract_text = pdf_info_value(&text, "Abstract")
        .or_else(|| xml_metadata_value(&text, &["dc:description", "abstract"]));
    let creation_date = pdf_info_value(&text, "CreationDate");
    let mod_date = pdf_info_value(&text, "ModDate");
    let year = parse_year_metadata(creation_date.as_deref().or(mod_date.as_deref()))
        .or_else(|| parse_year_metadata(Some(filename)));
    Ok(crate::models::ExternalPdfMetadata {
        filename: filename.to_string(),
        title,
        authors: parse_author_metadata(author_value.as_deref()),
        year,
        doi,
        scholarly_id: clean_optional_text(scholarly_id.as_deref()),
        abstract_text,
    })
}

fn title_author_year_candidates(
    conn: &Connection,
    metadata: &crate::models::ExternalPdfMetadata,
) -> Result<Vec<crate::models::ExternalPdfCandidate>> {
    let (Some(title), Some(year)) = (metadata.title.as_deref(), metadata.year) else {
        return Ok(Vec::new());
    };
    let title_norm = normalize_title(title);
    if title_norm.is_empty() || metadata.authors.is_empty() {
        return Ok(Vec::new());
    }
    let imported_authors: std::collections::HashSet<String> = metadata
        .authors
        .iter()
        .map(author_key)
        .filter(|value| !value.is_empty())
        .collect();
    if imported_authors.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT id, title, authors_json, year FROM papers WHERE title_norm=?1 AND year=?2 ORDER BY id",
    )?;
    let rows = stmt.query_map(params![title_norm, year], |row| {
        let paper_id: i64 = row.get(0)?;
        let title: Option<String> = row.get(1)?;
        let authors_json: Option<String> = row.get(2)?;
        let authors = authors_json
            .as_deref()
            .and_then(|value| serde_json::from_str::<Vec<crate::models::Author>>(value).ok())
            .unwrap_or_default();
        Ok((paper_id, title, authors, row.get::<_, Option<i32>>(3)?))
    })?;
    let mut candidates = Vec::new();
    for row in rows {
        let (paper_id, title, authors, candidate_year) = row?;
        let matches_author = authors.iter().map(author_key).any(|key| imported_authors.contains(&key));
        if matches_author {
            candidates.push(crate::models::ExternalPdfCandidate { paper_id, title, authors, year: candidate_year });
        }
    }
    Ok(candidates)
}

fn find_paper_by_exact_scholarly_id(conn: &Connection, id: &str) -> Result<Option<i64>> {
    conn.query_row(
        "SELECT id FROM papers
         WHERE lower(openalex_work_id)=lower(?1) OR lower(publisher_article_id)=lower(?1)
         ORDER BY id LIMIT 1",
        params![id.trim()],
        |row| row.get(0),
    )
    .optional()
}

fn ensure_external_pdf_journal(conn: &Connection) -> Result<i64> {
    const NAME: &str = "External PDF Import";
    if let Some(id) = conn
        .query_row("SELECT id FROM journals WHERE name=?1 ORDER BY id LIMIT 1", params![NAME], |row| row.get(0))
        .optional()?
    {
        return Ok(id);
    }
    let now = now_utc();
    conn.execute(
        "INSERT INTO journals (name, enabled, priority, created_at, updated_at)
         VALUES (?1,0,-100,?2,?2)",
        params![NAME, now],
    )?;
    Ok(conn.last_insert_rowid())
}

fn add_library_and_attach(
    conn: &Connection,
    paper_id: i64,
    file: &LinkedFile,
    added_source: &str,
) -> Result<crate::models::PaperAttachment> {
    let tx = conn.unchecked_transaction()?;
    if !paper_exists(&tx, paper_id)? {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    let now = now_utc();
    tx.execute(
        "INSERT INTO library_items (paper_id, added_at, added_source)
         VALUES (?1,?2,?3) ON CONFLICT(paper_id) DO NOTHING",
        params![paper_id, now, added_source],
    )?;
    tx.execute("UPDATE papers SET is_favorite=0, updated_at=?1 WHERE id=?2", params![now, paper_id])?;
    let id = insert_linked_attachment(&tx, paper_id, file)?;
    tx.commit()?;
    get_paper_attachment(conn, id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

/// Import a local PDF into the canonical Paper graph. Exact DOI and exact
/// scholarly IDs merge immediately; title+authors+year is a candidate only
/// and remains pending until the caller supplies explicit confirmation.
pub fn import_external_pdf(
    conn: &Connection,
    path: &str,
    confirmed_paper_id: Option<i64>,
) -> Result<crate::models::ExternalPdfImportResult> {
    let file = linked_file(path)?;
    let metadata = file.metadata.clone();

    if let Some(doi) = metadata.doi.as_deref() {
        if let Some(paper_id) = conn
            .query_row("SELECT id FROM papers WHERE normalized_doi=?1", params![doi], |row| row.get(0))
            .optional()?
        {
            let attachment = add_library_and_attach(conn, paper_id, &file, "external_pdf_import")?;
            return Ok(crate::models::ExternalPdfImportResult {
                outcome: "existingDoi".to_string(),
                paper_id: Some(paper_id),
                attachment: Some(attachment),
                metadata,
                candidate: None,
                candidates: Vec::new(),
                requires_confirmation: false,
            });
        }
    }

    if let Some(scholarly_id) = metadata.scholarly_id.as_deref() {
        if let Some(paper_id) = find_paper_by_exact_scholarly_id(conn, scholarly_id)? {
            let attachment = add_library_and_attach(conn, paper_id, &file, "external_pdf_import")?;
            return Ok(crate::models::ExternalPdfImportResult {
                outcome: "existingScholarlyId".to_string(),
                paper_id: Some(paper_id),
                attachment: Some(attachment),
                metadata,
                candidate: None,
                candidates: Vec::new(),
                requires_confirmation: false,
            });
        }
    }

    let candidates = title_author_year_candidates(conn, &metadata)?;
    if let Some(paper_id) = confirmed_paper_id {
        if !paper_exists(conn, paper_id)? {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        let attachment = add_library_and_attach(conn, paper_id, &file, "external_pdf_manual_confirmation")?;
        return Ok(crate::models::ExternalPdfImportResult {
            outcome: "manualConfirmation".to_string(),
            paper_id: Some(paper_id),
            attachment: Some(attachment),
            metadata,
            candidate: candidates.iter().find(|candidate| candidate.paper_id == paper_id).cloned(),
            candidates,
            requires_confirmation: false,
        });
    }
    if let Some(candidate) = candidates.first().cloned() {
        return Ok(crate::models::ExternalPdfImportResult {
            outcome: "needsManualConfirmation".to_string(),
            paper_id: None,
            attachment: None,
            metadata,
            candidate: Some(candidate),
            candidates,
            requires_confirmation: true,
        });
    }

    let journal_id = ensure_external_pdf_journal(conn)?;
    let doi = metadata.doi.clone();
    let abstract_source = metadata.abstract_text.as_ref().map(|_| "pdf_metadata".to_string());
    let candidate = crate::models::PaperCandidate {
        normalized_doi: doi.clone(),
        original_doi: doi.clone(),
        title: metadata.title.clone(),
        authors: metadata.authors.clone(),
        published_date: metadata.year.map(|year| format!("{}-01-01", year)),
        year: metadata.year,
        abstract_text: metadata.abstract_text.clone(),
        abstract_source,
        abstract_source_url: None,
        url: doi.as_deref().map(|doi| format!("https://doi.org/{}", doi)),
        publisher_article_id: metadata.scholarly_id.clone(),
        openalex_work_id: None,
        discovery_source: "external_pdf_import".to_string(),
        source_id: doi,
        raw_json: None,
    };
    let paper_id = insert_paper_without_identity_merge(conn, journal_id, &candidate)?;
    let attachment = add_library_and_attach(conn, paper_id, &file, "external_pdf_import")?;
    Ok(crate::models::ExternalPdfImportResult {
        outcome: "createdExternalPaper".to_string(),
        paper_id: Some(paper_id),
        attachment: Some(attachment),
        metadata,
        candidate: None,
        candidates: Vec::new(),
        requires_confirmation: false,
    })
}

pub fn get_library_membership(
    conn: &Connection,
    paper_id: i64,
) -> Result<Option<crate::models::LibraryMembership>> {
    let base: Option<(String, String)> = conn
        .query_row(
            "SELECT added_at, added_source FROM library_items WHERE paper_id = ?1",
            params![paper_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((added_at, added_source)) = base else {
        return Ok(None);
    };
    let collection_ids = conn
        .prepare("SELECT collection_id FROM library_collection_items WHERE paper_id = ?1 ORDER BY collection_id")?
        .query_map(params![paper_id], |r| r.get(0))?
        .collect::<Result<Vec<i64>>>()?;
    let tag_ids = conn
        .prepare("SELECT tag_id FROM library_item_tags WHERE paper_id = ?1 ORDER BY tag_id")?
        .query_map(params![paper_id], |r| r.get(0))?
        .collect::<Result<Vec<i64>>>()?;
    Ok(Some(crate::models::LibraryMembership {
        paper_id,
        added_at,
        added_source,
        collection_ids,
        tag_ids,
    }))
}

pub fn add_paper_to_library(
    conn: &Connection,
    paper_id: i64,
    collection_ids: &[i64],
    tag_ids: &[i64],
    added_source: &str,
) -> Result<crate::models::LibraryMembership> {
    if added_source.trim().is_empty() {
        return Err(rusqlite::Error::InvalidParameterName("added_source".into()));
    }
    let tx = conn.unchecked_transaction()?;
    if !paper_exists(&tx, paper_id)? {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    validate_collection_ids(&tx, collection_ids)?;
    validate_library_tag_ids(&tx, tag_ids)?;
    let now = now_utc();
    tx.execute(
        "INSERT INTO library_items (paper_id, added_at, added_source)
         VALUES (?1, ?2, ?3) ON CONFLICT(paper_id) DO NOTHING",
        params![paper_id, now, added_source],
    )?;
    // The caller's selection is authoritative for this membership.
    tx.execute("DELETE FROM library_collection_items WHERE paper_id = ?1", params![paper_id])?;
    tx.execute("DELETE FROM library_item_tags WHERE paper_id = ?1", params![paper_id])?;
    for collection_id in collection_ids.iter().copied() {
        tx.execute(
            "INSERT OR IGNORE INTO library_collection_items (collection_id, paper_id, added_at)
             VALUES (?1, ?2, ?3)",
            params![collection_id, paper_id, now],
        )?;
    }
    for tag_id in tag_ids.iter().copied() {
        tx.execute(
            "INSERT OR IGNORE INTO library_item_tags (paper_id, tag_id, added_at)
             VALUES (?1, ?2, ?3)",
            params![paper_id, tag_id, now],
        )?;
    }
    tx.execute(
        "UPDATE papers SET is_favorite = 0, updated_at = ?1 WHERE id = ?2",
        params![now, paper_id],
    )?;
    tx.commit()?;
    get_library_membership(conn, paper_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

pub fn remove_paper_from_library(conn: &Connection, paper_id: i64) -> Result<bool> {
    let tx = conn.unchecked_transaction()?;
    let changed = tx.execute("DELETE FROM library_items WHERE paper_id = ?1", params![paper_id])?;
    // Keep membership cleanup explicit so this invariant also holds for
    // test/legacy connections that do not enable SQLite foreign keys.
    tx.execute("DELETE FROM library_collection_items WHERE paper_id = ?1", params![paper_id])?;
    tx.execute("DELETE FROM library_item_tags WHERE paper_id = ?1", params![paper_id])?;
    tx.commit()?;
    Ok(changed == 1)
}

pub fn set_paper_collections(conn: &Connection, paper_id: i64, collection_ids: &[i64]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    if !library_item_exists(&tx, paper_id)? {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    validate_collection_ids(&tx, collection_ids)?;
    let now = now_utc();
    tx.execute("DELETE FROM library_collection_items WHERE paper_id = ?1", params![paper_id])?;
    for collection_id in collection_ids.iter().copied() {
        tx.execute(
            "INSERT OR IGNORE INTO library_collection_items (collection_id, paper_id, added_at)
             VALUES (?1, ?2, ?3)",
            params![collection_id, paper_id, now],
        )?;
    }
    tx.commit()
}

pub fn set_paper_library_tags(conn: &Connection, paper_id: i64, tag_ids: &[i64]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    if !library_item_exists(&tx, paper_id)? {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    validate_library_tag_ids(&tx, tag_ids)?;
    let now = now_utc();
    tx.execute("DELETE FROM library_item_tags WHERE paper_id = ?1", params![paper_id])?;
    for tag_id in tag_ids.iter().copied() {
        tx.execute(
            "INSERT OR IGNORE INTO library_item_tags (paper_id, tag_id, added_at)
             VALUES (?1, ?2, ?3)",
            params![paper_id, tag_id, now],
        )?;
    }
    tx.commit()
}

pub fn list_library_papers(conn: &Connection, view: &str, limit: i64) -> Result<Vec<crate::models::LibraryPaper>> {
    let order = match view {
        "recent" => "li.added_at DESC, p.id DESC",
        "all" | "unfiled" => "COALESCE(p.published_date, p.created_at) DESC, p.id DESC",
        _ => return Err(rusqlite::Error::InvalidParameterName("view".into())),
    };
    let filter = if view == "unfiled" {
        "AND NOT EXISTS (SELECT 1 FROM library_collection_items ci WHERE ci.paper_id = p.id)"
    } else {
        ""
    };
    let sql = format!(
        "SELECT p.*, j.name AS journal_name FROM papers p
         JOIN journals j ON j.id = p.journal_id
         JOIN library_items li ON li.paper_id = p.id
         WHERE 1=1 {} ORDER BY {} LIMIT ?1",
        filter, order
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![limit], row_to_paper)?;
    let mut papers = rows.collect::<Result<Vec<_>>>()?;
    enrich_papers_collections(conn, &mut papers)?;
    filter_current_tag_matches(conn, &mut papers)?;
    papers.into_iter().map(|p| library_paper(conn, p)).collect()
}

pub fn get_library_paper(conn: &Connection, paper_id: i64) -> Result<Option<crate::models::LibraryPaper>> {
    let paper = conn
        .query_row(
            "SELECT p.*, j.name AS journal_name FROM papers p
             JOIN journals j ON j.id = p.journal_id
             JOIN library_items li ON li.paper_id = p.id WHERE p.id = ?1",
            params![paper_id],
            row_to_paper,
        )
        .optional()?;
    let Some(mut paper) = paper else { return Ok(None); };
    enrich_papers_collections(conn, std::slice::from_mut(&mut paper))?;
    filter_current_tag_matches(conn, std::slice::from_mut(&mut paper))?;
    Ok(Some(library_paper(conn, paper)?))
}

pub fn list_library_collections(conn: &Connection) -> Result<Vec<crate::models::LibraryCollection>> {
    let mut stmt = conn.prepare(
        "SELECT * FROM library_collections
         ORDER BY parent_id IS NOT NULL, parent_id, sort_order, name, id",
    )?;
    let rows = stmt.query_map([], library_collection_from_row)?;
    rows.collect()
}

pub fn create_library_collection(conn: &Connection, name: &str, parent_id: Option<i64>) -> Result<crate::models::LibraryCollection> {
    let name = name.trim();
    if name.is_empty() {
        return Err(rusqlite::Error::InvalidParameterName("name".into()));
    }
    if let Some(parent_id) = parent_id {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM library_collections WHERE id = ?1)",
            params![parent_id],
            |r| r.get(0),
        )?;
        if !exists { return Err(rusqlite::Error::QueryReturnedNoRows); }
    }
    let now = now_utc();
    conn.execute(
        "INSERT INTO library_collections (parent_id, name, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?3)",
        params![parent_id, name, now],
    )?;
    conn.query_row(
        "SELECT * FROM library_collections WHERE id = ?1",
        params![conn.last_insert_rowid()],
        library_collection_from_row,
    )
}

pub fn rename_library_collection(conn: &Connection, id: i64, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() { return Err(rusqlite::Error::InvalidParameterName("name".into())); }
    conn.execute(
        "UPDATE library_collections SET name = ?1, updated_at = ?2 WHERE id = ?3",
        params![name, now_utc(), id],
    )?;
    Ok(())
}

pub fn delete_library_collection(conn: &Connection, id: i64) -> Result<bool> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("UPDATE library_collections SET parent_id = NULL, updated_at = ?1 WHERE parent_id = ?2", params![now_utc(), id])?;
    let changed = tx.execute("DELETE FROM library_collections WHERE id = ?1", params![id])?;
    tx.commit()?;
    Ok(changed == 1)
}

pub fn list_library_tags(conn: &Connection) -> Result<Vec<crate::models::LibraryTag>> {
    let mut stmt = conn.prepare("SELECT * FROM library_tags ORDER BY name, id")?;
    let rows = stmt.query_map([], library_tag_from_row)?;
    rows.collect()
}

pub fn create_library_tag(conn: &Connection, name: &str, color: Option<&str>) -> Result<crate::models::LibraryTag> {
    let name = name.trim();
    if name.is_empty() { return Err(rusqlite::Error::InvalidParameterName("name".into())); }
    let now = now_utc();
    conn.execute(
        "INSERT INTO library_tags (name, color, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
        params![name, color, now],
    )?;
    conn.query_row("SELECT * FROM library_tags WHERE id = ?1", params![conn.last_insert_rowid()], library_tag_from_row)
}

pub fn rename_library_tag(conn: &Connection, id: i64, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() { return Err(rusqlite::Error::InvalidParameterName("name".into())); }
    conn.execute("UPDATE library_tags SET name = ?1, updated_at = ?2 WHERE id = ?3", params![name, now_utc(), id])?;
    Ok(())
}

pub fn delete_library_tag(conn: &Connection, id: i64) -> Result<bool> {
    Ok(conn.execute("DELETE FROM library_tags WHERE id = ?1", params![id])? == 1)
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
        (6, "round6-recommendation-history", migrate_to_v6),
        (7, "round6.5-tag-config-versions", migrate_to_v7),
        (8, "round6.5.4-tag-score-repair", migrate_to_v8),
        (9, "full-ai-tag-identity-repair", migrate_to_v9),
        (10, "abstract-recovery-batches", migrate_to_v10),
        (11, "daily-paper-first-seen-cycle", migrate_to_v11),
        (12, "backfill-first-seen-missing-from-recovery", migrate_to_v12),
        (13, "round7-content-kind", migrate_to_v13),
        (14, "literature-workspace-library", migrate_to_v14),
        (15, "literature-library-attachments-metadata", migrate_to_v15),
    ]
}

/// v13：Round 7 Phase 1 —— Missing Abstract Intelligence。
/// - papers 新增 content_kind / content_kind_source / content_kind_confidence /
///   abstract_status / abstract_source_url（全部 nullable 或带默认值，兼容存量行）。
/// - 安全 backfill：只写这 5 个新列；绝不覆盖已有 abstract / favorite / ignore /
///   first_seen_* / recommendation 历史。幂等（重复执行结果一致）。
fn migrate_to_v13(conn: &Connection) -> Result<()> {
    for (name, ty) in [
        ("content_kind", "TEXT NOT NULL DEFAULT 'unknown'"),
        ("content_kind_source", "TEXT"),
        ("content_kind_confidence", "TEXT NOT NULL DEFAULT 'UNKNOWN'"),
        ("abstract_status", "TEXT NOT NULL DEFAULT 'unknown'"),
        ("abstract_source_url", "TEXT"),
    ] {
        if !column_exists(conn, "papers", name) {
            conn.execute(&format!("ALTER TABLE papers ADD COLUMN {} {}", name, ty), [])?;
        }
    }
    backfill_content_kind_and_abstract_status(conn)?;
    Ok(())
}

/// 存量 backfill：优先根据 source_records 中已保存的 Crossref / OpenAlex raw JSON
/// （provider explicit type）+ 可靠 title heuristic 推导 content_kind / abstract_status。
/// - 低置信度必须保留 unknown，不批量误标 news/editorial。
/// - 只写 v13 新增列；不触碰 abstract、favorite/ignore、first_seen、recommendation。
/// - 已分类（非 unknown）的行不覆盖（尊重运行时解析结果），但 abstract_status 恒重算
///   （它是 content_kind + 摘要有无的纯函数，重算幂等）。
fn backfill_content_kind_and_abstract_status(conn: &Connection) -> Result<()> {
    let papers: Vec<(i64, bool, Option<String>)> = {
        let mut stmt = conn.prepare(
            "SELECT id,
                    CASE WHEN abstract IS NOT NULL AND abstract != '' THEN 1 ELSE 0 END,
                    title
             FROM papers",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? != 0, r.get::<_, Option<String>>(2)?))
        })?;
        rows.collect::<Result<Vec<_>>>()?
    };
    // 一次读全部 source_records，按 paper 分组（论文量通常 < 数千，内存足够）。
    let mut raw_by_paper: std::collections::HashMap<i64, Vec<String>> =
        std::collections::HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT paper_id, raw_json FROM source_records WHERE raw_json IS NOT NULL AND raw_json != ''",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (pid, raw) = row?;
            raw_by_paper.entry(pid).or_default().push(raw);
        }
    }
    for (id, has_abstract, title) in papers {
        let mut crossref_type: Option<String> = None;
        let mut openalex_type: Option<String> = None;
        if let Some(raws) = raw_by_paper.get(&id) {
            for raw in raws {
                if let Some((provider, ty)) = crate::content_kind::provider_type_from_raw_json(raw) {
                    match provider {
                        "crossref" if crossref_type.is_none() => crossref_type = Some(ty),
                        "openalex" if openalex_type.is_none() => openalex_type = Some(ty),
                        _ => {}
                    }
                }
            }
        }
        let kind: String = conn
            .query_row(
                "SELECT content_kind FROM papers WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| crate::content_kind::CK_UNKNOWN.to_string());
        // 未分类 → 解析；已分类 → 尊重现有结果。
        let effective_kind = if kind == crate::content_kind::CK_UNKNOWN {
            let res = crate::content_kind::resolve_content_kind(
                crossref_type.as_deref(),
                openalex_type.as_deref(),
                title.as_deref(),
            );
            conn.execute(
                "UPDATE papers SET content_kind=?1, content_kind_source=?2, content_kind_confidence=?3 WHERE id=?4",
                params![res.kind, res.source, res.confidence, id],
            )?;
            res.kind
        } else {
            kind
        };
        let status = crate::content_kind::abstract_status_for(&effective_kind, has_abstract);
        conn.execute(
            "UPDATE papers SET abstract_status=?1 WHERE id=?2",
            params![status, id],
        )?;
    }
    Ok(())
}

/// v14：Literature Workspace core.
/// Library membership references the canonical papers row; existing papers
/// are never auto-added during migration.
fn migrate_to_v14(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS library_items (
            paper_id INTEGER PRIMARY KEY REFERENCES papers(id) ON DELETE CASCADE,
            added_at TEXT NOT NULL,
            added_source TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_library_items_added_at ON library_items(added_at DESC);

        CREATE TABLE IF NOT EXISTS library_collections (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            parent_id INTEGER REFERENCES library_collections(id) ON DELETE SET NULL,
            name TEXT NOT NULL,
            sort_order INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_library_collections_parent ON library_collections(parent_id, sort_order, id);

        CREATE TABLE IF NOT EXISTS library_collection_items (
            collection_id INTEGER NOT NULL REFERENCES library_collections(id) ON DELETE CASCADE,
            paper_id INTEGER NOT NULL REFERENCES papers(id) ON DELETE CASCADE,
            added_at TEXT NOT NULL,
            PRIMARY KEY (collection_id, paper_id)
        );
        CREATE INDEX IF NOT EXISTS idx_library_collection_items_paper ON library_collection_items(paper_id);

        CREATE TABLE IF NOT EXISTS library_tags (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            color TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS library_item_tags (
            paper_id INTEGER NOT NULL REFERENCES papers(id) ON DELETE CASCADE,
            tag_id INTEGER NOT NULL REFERENCES library_tags(id) ON DELETE CASCADE,
            added_at TEXT NOT NULL,
            PRIMARY KEY (paper_id, tag_id)
        );
        CREATE INDEX IF NOT EXISTS idx_library_item_tags_tag ON library_item_tags(tag_id);
        "#,
    )?;
    Ok(())
}

/// v15：linked PDF relations and Library-only metadata overrides.
///
/// `managed` remains a valid storage mode for forward schema compatibility,
/// but no v15 operation copies, moves, or deletes a user file.
fn migrate_to_v15(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS paper_attachments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            paper_id INTEGER NOT NULL REFERENCES papers(id) ON DELETE CASCADE,
            kind TEXT NOT NULL DEFAULT 'pdf',
            storage_mode TEXT NOT NULL DEFAULT 'linked'
                CHECK (storage_mode IN ('linked', 'managed')),
            absolute_path TEXT NOT NULL,
            relative_path TEXT,
            url TEXT,
            filename TEXT NOT NULL,
            mime_type TEXT NOT NULL DEFAULT 'application/pdf',
            sha256 TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_paper_attachments_paper
            ON paper_attachments(paper_id, created_at DESC, id DESC);

        CREATE TABLE IF NOT EXISTS library_item_metadata (
            paper_id INTEGER PRIMARY KEY REFERENCES papers(id) ON DELETE CASCADE,
            title_override TEXT,
            chinese_title_override TEXT,
            source_override TEXT,
            year_override INTEGER,
            authors_override TEXT,
            abstract_override TEXT,
            chinese_abstract_override TEXT,
            note TEXT,
            updated_at TEXT NOT NULL
        );
        "#,
    )?;
    Ok(())
}

/// v12 data-only repair: v11 could initialise a recovered paper as non-missing
/// because recovery had already completed. A persisted successful v10 recovery
/// after the paper was created is reliable evidence that it was originally
/// lacking an abstract. No network/AI/snapshot data is touched.
fn migrate_to_v12(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE papers SET first_seen_abstract_missing=1
         WHERE first_seen_abstract_missing=0 AND EXISTS (
           SELECT 1 FROM abstract_recovery_items i
           WHERE i.paper_id=papers.id AND i.outcome='recovered'
             AND i.started_at IS NOT NULL AND i.started_at >= papers.created_at
         )", []
    )?;
    Ok(())
}

/// Immutable local calendar-day membership. Existing records retain their
/// original created date; new papers are assigned only at first insertion.
fn migrate_to_v11(conn: &Connection) -> Result<()> {
    if !column_exists(conn, "papers", "first_seen_cycle") {
        conn.execute("ALTER TABLE papers ADD COLUMN first_seen_cycle TEXT", [])?;
        conn.execute("UPDATE papers SET first_seen_cycle=substr(created_at,1,10) WHERE first_seen_cycle IS NULL", [])?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_papers_first_seen_cycle ON papers(first_seen_cycle)", [])?;
    }
    if !column_exists(conn, "papers", "first_seen_abstract_missing") {
        conn.execute("ALTER TABLE papers ADD COLUMN first_seen_abstract_missing INTEGER NOT NULL DEFAULT 0", [])?;
        conn.execute("UPDATE papers SET first_seen_abstract_missing=CASE WHEN abstract_quality='missing' THEN 1 ELSE 0 END", [])?;
    }
    Ok(())
}

/// v10: durable, inspectable abstract-recovery attempt ledger.
fn migrate_to_v10(conn: &Connection) -> Result<()> {
    conn.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS abstract_recovery_batches (
            id INTEGER PRIMARY KEY AUTOINCREMENT, status TEXT NOT NULL, created_at TEXT NOT NULL,
            started_at TEXT, finished_at TEXT, total INTEGER NOT NULL DEFAULT 0, completed INTEGER NOT NULL DEFAULT 0,
            recovered INTEGER NOT NULL DEFAULT 0, not_found INTEGER NOT NULL DEFAULT 0, failed INTEGER NOT NULL DEFAULT 0,
            remaining INTEGER NOT NULL DEFAULT 0, error_summary TEXT
        );
        CREATE TABLE IF NOT EXISTS abstract_recovery_items (
            id INTEGER PRIMARY KEY AUTOINCREMENT, batch_id INTEGER NOT NULL REFERENCES abstract_recovery_batches(id) ON DELETE CASCADE,
            paper_id INTEGER NOT NULL REFERENCES papers(id) ON DELETE CASCADE, status TEXT NOT NULL DEFAULT 'pending',
            current_source TEXT, outcome TEXT, started_at TEXT, completed_at TEXT, next_retry_at TEXT, error_summary TEXT,
            UNIQUE(batch_id, paper_id)
        );
        CREATE INDEX IF NOT EXISTS idx_arb_started ON abstract_recovery_batches(started_at);
        CREATE INDEX IF NOT EXISTS idx_ari_batch ON abstract_recovery_items(batch_id, status);
        CREATE TABLE IF NOT EXISTS abstract_recovery_attempts (
            id INTEGER PRIMARY KEY AUTOINCREMENT, item_id INTEGER NOT NULL REFERENCES abstract_recovery_items(id) ON DELETE CASCADE,
            source TEXT NOT NULL, outcome TEXT, started_at TEXT NOT NULL, completed_at TEXT, error_summary TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_ara_item ON abstract_recovery_attempts(item_id);
    "#)?;
    Ok(())
}

/// v9：修复 Full AI 历史 name-only tagMatches（无 tag_id/hash）。
/// repair_paper_tag_matches 幂等：按 name 匹配 tags 表补 tag_id + 当前 semantic hash，
/// 按 tag_id 去重并重算 total_score；不调用 DeepSeek、不删除 Paper。
fn migrate_to_v9(conn: &Connection) -> Result<()> {
    repair_paper_tag_matches(conn)?;
    Ok(())
}

/// v8：修复历史 tag score 数据——为无 tag_id 的记录补 identity + 当前 semantic hash，
/// 按 tag_id 去重并重算 totalScore（不调用 AI、不删除 Paper）。
fn migrate_to_v8(conn: &Connection) -> Result<()> {
    repair_paper_tag_matches(conn)?;
    Ok(())
}

/// v7：Versioned Tag Configuration。
/// - tag_config_versions（active/scheduled/retired）+ tag_config_version_items（name/desc 快照）
/// - 一个 upcoming cycle 至多一个 scheduled config
fn migrate_to_v7(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS tag_config_versions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            status TEXT NOT NULL,
            effective_cycle_key TEXT,
            created_at TEXT NOT NULL,
            activated_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_tcv_status ON tag_config_versions(status);

        CREATE TABLE IF NOT EXISTS tag_config_version_items (
            version_id INTEGER NOT NULL REFERENCES tag_config_versions(id) ON DELETE CASCADE,
            tag_id INTEGER NOT NULL,
            name TEXT NOT NULL,
            description TEXT,
            enabled INTEGER NOT NULL DEFAULT 1,
            deleted INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (version_id, tag_id)
        );
        "#,
    )?;
    // 初始化：若从未有版本 → 创建 v1 active（当前 tags 表快照）
    let has: i64 = conn
        .query_row("SELECT COUNT(*) FROM tag_config_versions", [], |r| r.get(0))
        .unwrap_or(0);
    if has == 0 {
        let now = now_utc();
        conn.execute(
            "INSERT INTO tag_config_versions (status, created_at, activated_at) VALUES ('active', ?1, ?1)",
            params![now],
        )?;
        let vid = conn.last_insert_rowid();
        conn.execute(
            "INSERT OR IGNORE INTO tag_config_version_items (version_id, tag_id, name, description, enabled, deleted)
             SELECT ?1, id, name, description, enabled, 0 FROM tags",
            params![vid],
        )?;
    }
    Ok(())
}

/// v6：每日推荐时间线与历史。
/// - recommendation_runs：每日周期（cycle_key=本地时区日期，open/finalized）
/// - recommendation_items：rank + score_snapshot；UNIQUE(run_id, paper_id)；
///   UNIQUE(paper_id) 硬约束——同一 Paper 一生只进入一个推荐周期
fn migrate_to_v6(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS recommendation_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            cycle_key TEXT NOT NULL UNIQUE,
            cycle_start TEXT NOT NULL,
            cycle_end TEXT,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            finalized_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_rr_cycle ON recommendation_runs(cycle_key);

        CREATE TABLE IF NOT EXISTS recommendation_items (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id INTEGER NOT NULL REFERENCES recommendation_runs(id) ON DELETE CASCADE,
            paper_id INTEGER NOT NULL REFERENCES papers(id) ON DELETE CASCADE,
            rank INTEGER NOT NULL,
            score_snapshot REAL NOT NULL,
            added_at TEXT NOT NULL,
            UNIQUE (run_id, paper_id),
            UNIQUE (paper_id)
        );
        CREATE INDEX IF NOT EXISTS idx_ri_run ON recommendation_items(run_id, rank);
        CREATE INDEX IF NOT EXISTS idx_ri_paper ON recommendation_items(paper_id);
        "#,
    )?;
    Ok(())
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

/// Papers with a valid source title may receive a title-only translation. This
/// query intentionally excludes only an existing Chinese title or an invalid
/// source title; abstract/content/analysis state must not gate this backlog.
/// It deliberately has no sync-batch or first-seen predicate: historical
/// papers are backlog candidates too. A bounded, newest-first batch lets the
/// papers currently visible after a sync get their titles in this session,
/// without turning one launch or sync into an unbounded API run.
pub const TITLE_TRANSLATION_BATCH_LIMIT: usize = 25;
pub fn list_missing_title_translation_candidates(
    conn: &Connection,
    paper_ids: Option<&[i64]>,
) -> Result<Vec<(i64, String)>> {
    let mut sql = String::from(
        "SELECT id, title FROM papers WHERE title IS NOT NULL AND TRIM(title) != '' \
         AND (chinese_title IS NULL OR TRIM(chinese_title) = '')",
    );
    if let Some(ids) = paper_ids {
        if ids.is_empty() { return Ok(vec![]); }
        sql.push_str(" AND id IN (");
        sql.push_str(&ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(","));
        sql.push(')');
    }
    sql.push_str(" ORDER BY created_at DESC, id DESC LIMIT ");
    sql.push_str(&TITLE_TRANSLATION_BATCH_LIMIT.to_string());
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let candidates = rows.collect::<Result<Vec<_>>>()?;
    Ok(candidates)
}

/// Validate a caller-provided recovery scope.  Recovery is deliberately never
/// allowed to discover its own database-wide target set: the current UI view
/// owns the scope, while this query protects against stale, duplicate, or
/// already-complete IDs.
pub const ABSTRACT_RECOVERY_BATCH_LIMIT: usize = 50;
pub fn list_recoverable_paper_ids(conn: &Connection, paper_ids: &[i64]) -> Result<Vec<i64>> {
    if paper_ids.is_empty() { return Ok(vec![]); }
    let mut ids = paper_ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let mut sql = String::from(
        "SELECT id FROM papers WHERE id IN (",
    );
    sql.push_str(&ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(","));
    sql.push_str(") AND abstract_quality != 'complete' AND abstract_status != 'not_expected' ORDER BY id ASC LIMIT ");
    sql.push_str(&ABSTRACT_RECOVERY_BATCH_LIMIT.to_string());
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    rows.collect()
}

/// Persist only a translated title. In particular, this must never create
/// evidence, scores, summaries, or a completed-analysis status.
pub fn save_title_translation(conn: &Connection, id: i64, chinese_title: &str) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE papers SET chinese_title = ?1, updated_at = ?2
         WHERE id = ?3
           AND (chinese_title IS NULL OR TRIM(chinese_title) = '')",
        params![chinese_title, now_utc(), id],
    )?;
    Ok(changed == 1)
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
    // Library membership wins over the legacy Read Later flag. Keep this as an
    // application-level invariant; no cross-table trigger is required.
    let value = if flag == "favorite" && value && library_item_exists(conn, id)? {
        false
    } else {
        value
    };
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

/// A sync batch cannot survive a process exit: there is no worker after the
/// next launch that could legitimately complete it.  Mark such persisted
/// `running` rows terminal before Activity state is exposed again.
pub fn recover_interrupted_sync_batches(conn: &Connection) -> Result<usize> {
    let changed = conn.execute(
        "UPDATE sync_batches
         SET status=?1, finished_at=?2,
             error_summary=COALESCE(error_summary, '应用在同步完成前中断')
         WHERE status='running'",
        params![SBC_FAILED, now_utc()],
    )?;
    Ok(changed)
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

// ---------- AbstractRecoveryBatch ----------

pub fn create_abstract_recovery_batch(conn: &Connection, paper_ids: &[i64]) -> Result<i64> {
    let now = now_utc();
    conn.execute("INSERT INTO abstract_recovery_batches (status, created_at, started_at, total, remaining) VALUES ('running', ?1, ?1, ?2, ?2)", params![now, paper_ids.len() as i64])?;
    let id = conn.last_insert_rowid();
    for paper_id in paper_ids {
        conn.execute("INSERT INTO abstract_recovery_items (batch_id, paper_id, status) VALUES (?1, ?2, 'pending')", params![id, paper_id])?;
    }
    Ok(id)
}

fn row_to_abstract_recovery_batch(row: &rusqlite::Row) -> Result<AbstractRecoveryBatch> {
    Ok(AbstractRecoveryBatch { id: row.get(0)?, status: row.get(1)?, created_at: row.get(2)?, started_at: row.get(3)?, finished_at: row.get(4)?, total: row.get(5)?, completed: row.get(6)?, recovered: row.get(7)?, not_found: row.get(8)?, failed: row.get(9)?, remaining: row.get(10)?, error_summary: row.get(11)? })
}
pub fn get_abstract_recovery_batch(conn: &Connection, id: i64) -> Result<Option<AbstractRecoveryBatch>> {
    conn.query_row("SELECT id,status,created_at,started_at,finished_at,total,completed,recovered,not_found,failed,remaining,error_summary FROM abstract_recovery_batches WHERE id=?1", params![id], row_to_abstract_recovery_batch).optional()
}
pub fn latest_abstract_recovery_batch(conn: &Connection) -> Result<Option<AbstractRecoveryBatch>> {
    conn.query_row("SELECT id,status,created_at,started_at,finished_at,total,completed,recovered,not_found,failed,remaining,error_summary FROM abstract_recovery_batches ORDER BY id DESC LIMIT 1", [], row_to_abstract_recovery_batch).optional()
}
pub fn list_abstract_recovery_batches(conn: &Connection, limit: i64) -> Result<Vec<AbstractRecoveryBatch>> {
    let mut stmt = conn.prepare("SELECT id,status,created_at,started_at,finished_at,total,completed,recovered,not_found,failed,remaining,error_summary FROM abstract_recovery_batches ORDER BY id DESC LIMIT ?1")?;
    let rows = stmt.query_map(params![limit], row_to_abstract_recovery_batch)?;
    rows.collect()
}
pub fn list_abstract_recovery_items(conn: &Connection, batch_id: i64) -> Result<Vec<AbstractRecoveryItem>> {
    let mut stmt = conn.prepare("SELECT i.id,i.batch_id,i.paper_id,p.title,i.status,i.current_source,i.outcome,i.started_at,i.completed_at,i.next_retry_at,i.error_summary FROM abstract_recovery_items i LEFT JOIN papers p ON p.id=i.paper_id WHERE i.batch_id=?1 ORDER BY i.id")?;
    let rows = stmt.query_map(params![batch_id], |r| Ok(AbstractRecoveryItem { id:r.get(0)?, batch_id:r.get(1)?, paper_id:r.get(2)?, title:r.get(3)?, status:r.get(4)?, current_source:r.get(5)?, outcome:r.get(6)?, started_at:r.get(7)?, completed_at:r.get(8)?, next_retry_at:r.get(9)?, error_summary:r.get(10)? }))?;
    rows.collect()
}
pub fn start_abstract_recovery_item(conn: &Connection, item_id: i64, source: &str) -> Result<()> {
    let now = now_utc();
    conn.execute("UPDATE abstract_recovery_items SET status='running', current_source=?1, started_at=COALESCE(started_at,?2) WHERE id=?3", params![source,now,item_id])?;
    conn.execute("INSERT INTO abstract_recovery_attempts (item_id,source,started_at) VALUES (?1,?2,?3)", params![item_id,source,now])?;
    Ok(())
}
pub fn finish_abstract_recovery_attempt(conn: &Connection, item_id: i64, source: &str, outcome: &str, error: Option<&str>) -> Result<()> {
    let now = now_utc();
    conn.execute("UPDATE abstract_recovery_attempts SET outcome=?1,completed_at=?2,error_summary=?3 WHERE id=(SELECT id FROM abstract_recovery_attempts WHERE item_id=?4 AND source=?5 AND completed_at IS NULL ORDER BY id DESC LIMIT 1)", params![outcome,now,error,item_id,source])?;
    Ok(())
}
pub fn finish_abstract_recovery_item(conn: &Connection, item_id: i64, outcome: &str, error: Option<&str>, next_retry_at: Option<&str>) -> Result<()> {
    let now = now_utc();
    conn.execute("UPDATE abstract_recovery_items SET status='completed',outcome=?1,error_summary=?2,next_retry_at=?3,completed_at=?4 WHERE id=?5", params![outcome,error,next_retry_at,now,item_id])?;
    Ok(())
}
pub fn update_abstract_recovery_batch_counts(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("UPDATE abstract_recovery_batches SET completed=(SELECT COUNT(*) FROM abstract_recovery_items WHERE batch_id=?1 AND status='completed'), recovered=(SELECT COUNT(*) FROM abstract_recovery_items WHERE batch_id=?1 AND outcome='recovered'), not_found=(SELECT COUNT(*) FROM abstract_recovery_items WHERE batch_id=?1 AND outcome='notFound'), failed=(SELECT COUNT(*) FROM abstract_recovery_items WHERE batch_id=?1 AND outcome='networkFailure'), remaining=(SELECT COUNT(*) FROM abstract_recovery_items WHERE batch_id=?1 AND status!='completed') WHERE id=?1", params![id])?;
    Ok(())
}
pub fn finalize_abstract_recovery_batch(conn: &Connection, id: i64, status: &str, error: Option<&str>) -> Result<()> {
    conn.execute("UPDATE abstract_recovery_batches SET status=?1,finished_at=?2,error_summary=?3 WHERE id=?4", params![status,now_utc(),error,id])?;
    Ok(())
}
pub fn recover_interrupted_abstract_recovery_batches(conn: &Connection) -> Result<usize> {
    conn.execute("UPDATE abstract_recovery_batches SET status='interrupted',finished_at=?1,error_summary=COALESCE(error_summary,'App restarted; remaining papers can be retried.') WHERE status='running'", params![now_utc()])
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

// ---------- Round 6：Recommendation Runs / Items ----------

pub fn create_recommendation_run(conn: &Connection, cycle_key: &str, status: &str) -> Result<i64> {
    let now = now_utc();
    conn.execute(
        "INSERT OR IGNORE INTO recommendation_runs (cycle_key, cycle_start, status, created_at)
         VALUES (?1, ?2, ?3, ?2)",
        params![cycle_key, now, status],
    )?;
    let id = conn.query_row(
        "SELECT id FROM recommendation_runs WHERE cycle_key = ?1",
        params![cycle_key],
        |r| r.get::<_, i64>(0),
    )?;
    Ok(id)
}

pub fn find_recommendation_run_by_cycle_key(conn: &Connection, cycle_key: &str) -> Result<Option<i64>> {
    let id = conn
        .query_row(
            "SELECT id FROM recommendation_runs WHERE cycle_key = ?1",
            params![cycle_key],
            |r| r.get::<_, i64>(0),
        )
        .optional()?;
    Ok(id)
}

/// finalize 所有非当前 cycle_key 的 open run。
pub fn finalize_open_runs_except(conn: &Connection, cycle_key: &str, finalized_at: &str) -> Result<()> {
    conn.execute(
        "UPDATE recommendation_runs SET status = 'finalized', cycle_end = ?1, finalized_at = ?1
         WHERE status = 'open' AND cycle_key != ?2",
        params![finalized_at, cycle_key],
    )?;
    Ok(())
}

fn row_to_recommendation_run(row: &rusqlite::Row) -> Result<RecommendationRun> {
    Ok(RecommendationRun {
        id: row.get("id")?,
        cycle_key: row.get("cycle_key")?,
        cycle_start: row.get("cycle_start")?,
        cycle_end: row.get("cycle_end")?,
        status: row.get("status")?,
        created_at: row.get("created_at")?,
        finalized_at: row.get("finalized_at")?,
        item_count: row.get("item_count")?,
        max_score: row.get("max_score")?,
        journal_count: row.get("journal_count")?,
    })
}

pub fn get_recommendation_run(conn: &Connection, id: i64) -> Result<Option<RecommendationRun>> {
    conn.query_row(
        "SELECT r.*,
            (SELECT COUNT(*) FROM recommendation_items i WHERE i.run_id = r.id) AS item_count,
            (SELECT MAX(score_snapshot) FROM recommendation_items i WHERE i.run_id = r.id) AS max_score,
            (SELECT COUNT(DISTINCT p.journal_id) FROM recommendation_items i JOIN papers p ON p.id = i.paper_id WHERE i.run_id = r.id) AS journal_count
         FROM recommendation_runs r WHERE r.id = ?1",
        params![id],
        row_to_recommendation_run,
    )
    .optional()
}

pub fn list_recommendation_runs(conn: &Connection) -> Result<Vec<RecommendationRun>> {
    let mut stmt = conn.prepare(
        "SELECT r.*,
            (SELECT COUNT(*) FROM recommendation_items i WHERE i.run_id = r.id) AS item_count,
            (SELECT MAX(score_snapshot) FROM recommendation_items i WHERE i.run_id = r.id) AS max_score,
            (SELECT COUNT(DISTINCT p.journal_id) FROM recommendation_items i JOIN papers p ON p.id = i.paper_id WHERE i.run_id = r.id) AS journal_count
         FROM recommendation_runs r ORDER BY r.cycle_key DESC, r.id DESC",
    )?;
    let rows = stmt.query_map([], row_to_recommendation_run)?;
    rows.collect()
}

pub fn list_recommendation_items(conn: &Connection, run_id: i64) -> Result<Vec<RecommendationItem>> {
    let mut stmt = conn.prepare(
        "SELECT id, run_id, paper_id, rank, score_snapshot, added_at
         FROM recommendation_items WHERE run_id = ?1 ORDER BY rank ASC",
    )?;
    let rows = stmt.query_map(params![run_id], |r| {
        Ok(RecommendationItem {
            id: r.get(0)?,
            run_id: r.get(1)?,
            paper_id: r.get(2)?,
            rank: r.get(3)?,
            score_snapshot: r.get(4)?,
            added_at: r.get(5)?,
        })
    })?;
    rows.collect()
}

// ---------- Round 6.4：User Collections（built-in 保护） ----------

/// built-in 集合（UTD24 / FT50）：membership 由 catalog 管理，用户不可改名/删除/改成员。
pub fn is_builtin_collection_code(code: &str) -> bool {
    matches!(code, "UTD24" | "FT50")
}

pub fn collection_code_by_id(conn: &Connection, id: i64) -> Result<Option<String>> {
    let v: Option<String> = conn
        .query_row("SELECT code FROM journal_collections WHERE id = ?1", params![id], |r| r.get(0))
        .optional()?;
    Ok(v)
}

pub fn rename_collection(conn: &Connection, id: i64, name: &str) -> Result<()> {
    conn.execute(
        "UPDATE journal_collections SET name = ?1, updated_at = ?2 WHERE id = ?3",
        params![name, now_utc(), id],
    )?;
    Ok(())
}

pub fn delete_collection(conn: &Connection, id: i64) -> Result<()> {
    // members 由 FK ON DELETE CASCADE 一并删除；journal/paper/subscription 不受影响
    conn.execute("DELETE FROM journal_collections WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn remove_collection_member(conn: &Connection, collection_id: i64, journal_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM journal_collection_members WHERE collection_id = ?1 AND journal_id = ?2",
        params![collection_id, journal_id],
    )?;
    Ok(())
}

/// 某集合的 journals（DB 视角，含手动添加期刊；与 catalog 静态列表不同）。
pub fn list_collection_journals(conn: &Connection, code: &str) -> Result<Vec<Journal>> {
    let mut stmt = conn.prepare(
        "SELECT j.*, (SELECT COUNT(*) FROM papers p WHERE p.journal_id = j.id) AS paper_count
         FROM journal_collection_members m
         JOIN journal_collections c ON c.id = m.collection_id
         JOIN journals j ON j.id = m.journal_id
         WHERE c.code = ?1
         ORDER BY j.name ASC",
    )?;
    let mut journals: Vec<Journal> = stmt.query_map(params![code], row_to_journal)?.collect::<Result<Vec<_>>>()?;
    enrich_journals(conn, &mut journals)?;
    Ok(journals)
}

// ---------- Round 6.5：Tag Config Versions ----------

pub fn list_tag_config_items(conn: &Connection, version_id: i64) -> Result<Vec<crate::models::TagConfigItem>> {
    let mut stmt = conn.prepare(
        "SELECT version_id, tag_id, name, description, enabled, deleted
         FROM tag_config_version_items WHERE version_id = ?1 ORDER BY tag_id",
    )?;
    let rows = stmt.query_map(params![version_id], |r| {
        Ok(crate::models::TagConfigItem {
            version_id: r.get(0)?,
            tag_id: r.get(1)?,
            name: r.get(2)?,
            description: r.get(3)?,
            enabled: r.get::<_, i64>(4)? != 0,
            deleted: r.get::<_, i64>(5)? != 0,
        })
    })?;
    rows.collect()
}

/// scheduled 配置（至多一个 pending）。
pub fn scheduled_tag_config(conn: &Connection) -> Result<Option<crate::models::TagConfigVersion>> {
    let v: Option<crate::models::TagConfigVersion> = conn
        .query_row(
            "SELECT id, status, effective_cycle_key, created_at, activated_at
             FROM tag_config_versions WHERE status = 'scheduled' ORDER BY id DESC LIMIT 1",
            [],
            |r| {
                Ok(crate::models::TagConfigVersion {
                    id: r.get(0)?,
                    status: r.get(1)?,
                    effective_cycle_key: r.get(2)?,
                    created_at: r.get(3)?,
                    activated_at: r.get(4)?,
                })
            },
        )
        .optional()?;
    Ok(v)
}

/// 替换 scheduled 配置（一个 upcoming cycle 至多一个）。
pub fn replace_scheduled_tag_config(
    conn: &Connection,
    draft: &[crate::models::TagDraftItem],
    effective_cycle_key: &str,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM tag_config_versions WHERE status = 'scheduled'", [])?;
    let now = now_utc();
    tx.execute(
        "INSERT INTO tag_config_versions (status, effective_cycle_key, created_at) VALUES ('scheduled', ?1, ?2)",
        params![effective_cycle_key, now],
    )?;
    let vid = tx.last_insert_rowid();
    for item in draft {
        // scheduled 中新增 tag 以 0 占位（激活时按 name 创建/匹配）
        let tag_id = if item.id > 0 { item.id } else { 0 };
        let _ = tx.execute(
            "INSERT OR REPLACE INTO tag_config_version_items (version_id, tag_id, name, description, enabled, deleted)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![vid, tag_id, item.name, item.description, item.enabled as i64, item.deleted as i64],
        );
    }
    tx.commit()?;
    Ok(())
}

/// 创建新 active version（当前 tags 表快照），并把旧 active 置 retired。
pub fn create_active_tag_version(conn: &Connection) -> Result<i64> {
    let tx = conn.unchecked_transaction()?;
    let now = now_utc();
    tx.execute(
        "UPDATE tag_config_versions SET status = 'retired' WHERE status = 'active'",
        [],
    )?;
    tx.execute(
        "INSERT INTO tag_config_versions (status, created_at, activated_at) VALUES ('active', ?1, ?1)",
        params![now],
    )?;
    let vid = tx.last_insert_rowid();
    tx.execute(
        "INSERT OR IGNORE INTO tag_config_version_items (version_id, tag_id, name, description, enabled, deleted)
         SELECT ?1, id, name, description, enabled, 0 FROM tags",
        params![vid],
    )?;
    tx.commit()?;
    Ok(vid)
}

/// Repair：为无 tag_id 的历史 tag 记录补 identity + 当前 semantic hash，并按 tag_id 去重。
/// 幂等；不删除 Paper、不调用 AI。迁移与启动时执行。
pub fn repair_paper_tag_matches(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("SELECT id, tag_matches_json FROM papers WHERE tag_matches_json IS NOT NULL")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut by_id: Vec<(i64, String)> = Vec::new();
    for row in rows {
        by_id.push(row?);
    }
    for (pid, json) in by_id {
        let mut matches: Vec<crate::models::TagMatch> = serde_json::from_str(&json).unwrap_or_default();
        let mut changed = false;
        // 1) 补 tag_id + hash（按 name 匹配 tags 表）
        for m in matches.iter_mut() {
            if m.tag_id.is_none() {
                if let Some(id) = find_tag_by_name(conn, &m.tag)? {
                    if let Some(t) = get_tag_by_id(conn, id)? {
                        m.tag_id = Some(id);
                        m.semantic_hash = Some(crate::tag_config::tag_semantic_hash(
                            id,
                            &t.name,
                            t.description.as_deref().unwrap_or_default(),
                        ));
                        changed = true;
                    }
                }
            }
        }
        // 2) 同 tag_id 多条 → 保留 hash 匹配当前 active 语义的一条（active_tags 优先），否则保留第一条
        let active = crate::tag_config::active_tags(conn).unwrap_or_default();
        let mut seen: Vec<i64> = Vec::new();
        let mut deduped: Vec<crate::models::TagMatch> = Vec::new();
        let matches_len = matches.len();
        for m in matches {
            if let Some(tid) = m.tag_id {
                if seen.contains(&tid) {
                    // 重复：保留 hash 匹配 active 的
                    let active_hit = active.iter().any(|(id, name, desc)| {
                        *id == tid && {
                            let expect = crate::tag_config::tag_semantic_hash(*id, name, desc);
                            m.semantic_hash.as_deref() == Some(expect.as_str())
                        }
                    });
                    if active_hit {
                        if let Some(existing) = deduped.iter_mut().find(|d| d.tag_id == Some(tid)) {
                            *existing = m;
                        } else {
                            deduped.push(m);
                        }
                        changed = true;
                    }
                    // 非 active 匹配的重复 → 丢弃
                    continue;
                }
                seen.push(tid);
                deduped.push(m);
            } else {
                deduped.push(m);
            }
        }
        if changed || deduped.len() != matches_len {
            let new_json = serde_json::to_string(&deduped).unwrap_or_else(|_| "[]".to_string());
            conn.execute(
                "UPDATE papers SET tag_matches_json = ?1, updated_at = ?2 WHERE id = ?3",
                params![new_json, now_utc(), pid],
            )?;
            if let Ok(active) = &crate::tag_config::active_tags(conn) {
                let _ = crate::tag_config::recompute_paper_total_score(conn, pid, active);
            }
        }
    }
    Ok(())
}

fn get_tag_by_id(conn: &Connection, id: i64) -> Result<Option<crate::models::Tag>> {
    conn.query_row(
        "SELECT id, name, description, enabled, created_at, updated_at FROM tags WHERE id = ?1",
        params![id],
        |r| {
            Ok(crate::models::Tag {
                id: r.get(0)?,
                name: r.get(1)?,
                description: r.get(2)?,
                enabled: r.get::<_, i64>(3)? != 0,
                created_at: r.get(4)?,
                updated_at: r.get(5)?,
            })
        },
    )
    .optional()
}

pub fn find_tag_by_name(conn: &Connection, name: &str) -> Result<Option<i64>> {
    let id = conn
        .query_row("SELECT id FROM tags WHERE name = ?1", params![name], |r| r.get::<_, i64>(0))
        .optional()?;
    Ok(id)
}

/// 合并 tag-only 评分结果到 paper（保留其他 tag 分数；写 semantic hash；本地重算 total）。
pub fn set_paper_tag_scores(
    conn: &Connection,
    paper_id: i64,
    scores: &[(i64, f64)],
    semantic: &[(i64, String, String)],
) -> Result<()> {
    let json: Option<String> = conn
        .query_row("SELECT tag_matches_json FROM papers WHERE id = ?1", params![paper_id], |r| r.get(0))
        .optional()?
        .flatten();
    let mut matches: Vec<crate::models::TagMatch> = json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    for (tid, score) in scores {
        if let Some((_, name, desc)) = semantic.iter().find(|(id, _, _)| id == tid) {
            let hash = crate::tag_config::tag_semantic_hash(*tid, name, desc);
            // 先按 tag_id 精确匹配；旧数据无 tag_id → 按 name fallback 替换（避免同一逻辑 Tag 两条）
            let hit = matches.iter_mut().find(|m| m.tag_id == Some(*tid));
            match hit {
                Some(m) => {
                    m.score = *score;
                    m.semantic_hash = Some(hash.clone());
                    m.tag = name.clone();
                }
                None => {
                    // name fallback：只替换无 tag_id 的同名记录（旧 Full AI 数据）；带其他 tag_id 的同名记录不误改
                    if let Some(m) = matches.iter_mut().find(|m| m.tag_id.is_none() && m.tag == *name) {
                        m.score = *score;
                        m.semantic_hash = Some(hash.clone());
                        m.tag_id = Some(*tid);
                    } else {
                        matches.push(crate::models::TagMatch {
                            tag: name.clone(),
                            score: *score,
                            tag_id: Some(*tid),
                            semantic_hash: Some(hash),
                        });
                    }
                }
            }
        }
    }
    let new_json = serde_json::to_string(&matches).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "UPDATE papers SET tag_matches_json = ?1, updated_at = ?2 WHERE id = ?3",
        params![new_json, now_utc(), paper_id],
    )?;
    // 本地重算 total_score（active enabled + hash 匹配）；失败不阻塞评分写回
    if let Ok(active) = crate::tag_config::active_tags(conn) {
        let _ = crate::tag_config::recompute_paper_total_score(conn, paper_id, &active);
    }
    Ok(())
}

/// 需要 tag-only 评分的论文（有摘要；缺 requested tag 的 score 或 semantic hash stale）。
pub fn papers_needing_tag_scores(
    conn: &Connection,
    tags: &[(i64, String, String)],
) -> Result<Vec<i64>> {
    if tags.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT id, tag_matches_json FROM papers
         WHERE abstract IS NOT NULL AND abstract != '' AND analysis_status != 'waitingForAbstract'",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (id, json) = row?;
        let matches: Vec<crate::models::TagMatch> = json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        let needs = tags.iter().any(|(tid, name, desc)| {
            let expect = crate::tag_config::tag_semantic_hash(*tid, name, desc);
            let hit = matches.iter().find(|m| m.tag_id == Some(*tid));
            match hit {
                Some(m) => m.semantic_hash.as_deref() != Some(expect.as_str()),
                None => true,
            }
        });
        if needs {
            out.push(id);
        }
    }
    Ok(out)
}

/// 含指定 tag 名（removed/disabled）的 paper id 列表（本地重算用）。
pub fn paper_ids_with_tag_names(conn: &Connection, removed: &[String], disabled: &[String]) -> Result<Vec<i64>> {
    let names: Vec<&str> = removed.iter().chain(disabled.iter()).map(|s| s.as_str()).collect();
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare("SELECT id, tag_matches_json FROM papers WHERE tag_matches_json IS NOT NULL")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (id, json) = row?;
        let matches: Vec<crate::models::TagMatch> = serde_json::from_str(&json).unwrap_or_default();
        if matches.iter().any(|m| names.contains(&m.tag.as_str())) {
            out.push(id);
        }
    }
    Ok(out)
}

/// Tag-only 入队：允许从 succeeded/failed/pendingAnalysis 进入 queued（不限于 pendingAnalysis）。
pub fn enqueue_for_tag_update(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE papers SET analysis_status = 'queued', queued_at = ?1, retry_count = 0, updated_at = ?1
         WHERE id = ?2 AND analysis_status IN ('analysisSucceeded','analysisFailed','pendingAnalysis')",
        params![now_utc(), id],
    )?;
    Ok(())
}

/// 删除 scheduled 配置（激活后消费）。
pub fn delete_scheduled_tag_config(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM tag_config_versions WHERE status = 'scheduled'", [])?;
    Ok(())
}

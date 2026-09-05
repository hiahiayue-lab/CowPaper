use rusqlite::{params, Connection, OptionalExtension, Result};
use sha2::{Digest, Sha256};
use lopdf::{Document, LoadOptions, Object};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Runtime};

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
/// Literature Workspace 为 v14；v15 为 Library Attachments + User Metadata；
/// v16 为 canonical bibliographic keywords；v17 为出版字段；v18 为 RC5
/// Library overrides、collection-scoped tags 与 PDF enrichment jobs。
/// 生产构建中仅由迁移系统隐式使用；测试中直接断言。
#[allow(dead_code)]
pub const SCHEMA_VERSION: i64 = 18;

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
pub fn find_paper_id(conn: &Connection, _journal_id: i64, c: &PaperCandidate) -> Result<Option<i64>> {
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
                "SELECT id FROM papers WHERE publisher_article_id = ?1 AND (normalized_doi IS NULL OR ?2 IS NULL OR normalized_doi=?2)",
                params![paid,c.normalized_doi],
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
                "SELECT id FROM papers WHERE openalex_work_id = ?1 AND (normalized_doi IS NULL OR ?2 IS NULL OR normalized_doi=?2)",
                params![wid,c.normalized_doi],
                |r| r.get::<_, i64>(0),
            )
            .optional()?;
        if id.is_some() {
            return Ok(id);
        }
    }
    // Provider sync retains the historical exact normalized title/year
    // fallback for records with no scholarly identifier. External PDF import
    // never calls this path: it keeps title/author/year as a manual candidate.
    if c.publisher_article_id.is_none() && c.openalex_work_id.is_none() {
        if let (Some(title), Some(year)) = (&c.title, c.year) {
        let norm = normalize_title(title);
        let id = conn
            .query_row(
                "SELECT id FROM papers WHERE journal_id = ?1 AND year = ?2 AND title_norm = ?3",
                params![_journal_id, year, norm],
                |r| r.get::<_, i64>(0),
            )
            .optional()?;
        if id.is_some() {
            return Ok(id);
        }
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
    let protect_provider = cur_source.as_deref().is_some_and(is_provider_abstract_source) && !is_provider_abstract_source(&cand_source);
    if let Some((t, q, r)) = new_cand.as_ref().filter(|_| !protect_provider) {
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
    update_abstract_provenance(conn, paper_id)?;

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
    fill_publication_metadata(conn, id, c)?;
    let authors_json = serde_json::to_string(&c.authors).unwrap_or_else(|_| "[]".to_string());
    conn.execute(
        "UPDATE papers SET
            normalized_doi = COALESCE(normalized_doi, ?1),
            original_doi = COALESCE(original_doi, ?2),
            url = COALESCE(url, ?3),
            title = COALESCE(title, ?4),
            authors_json = CASE WHEN authors_json IS NULL OR authors_json = '[]' THEN ?5 ELSE authors_json END,
            published_date = COALESCE(published_date, ?6),
            year = COALESCE(year, ?7),
            publisher_article_id = COALESCE(publisher_article_id, ?8),
            openalex_work_id = COALESCE(openalex_work_id, ?9),
            discovery_source = COALESCE(discovery_source, ?10),
            updated_at = ?11
         WHERE id = ?12",
        params![
            c.normalized_doi,
            c.original_doi,
            c.url,
            c.title,
            authors_json,
            c.published_date,
            c.year,
            c.publisher_article_id,
            c.openalex_work_id,
            c.discovery_source,
            now_utc(),
            id
        ],
    )?;
    Ok(())
}

pub fn upsert_paper(conn: &Connection, journal_id: i64, c: &PaperCandidate) -> Result<UpsertOutcome> {
    if let Some(existing_id) = find_paper_id(conn, journal_id, c)? {
        let (abstract_filled, abstract_upgraded) = merge_abstract(conn, existing_id, c)?;
        update_abstract_provenance(conn, existing_id)?;
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
    fill_publication_metadata(conn, id, c)?;
    update_abstract_provenance(conn, id)?;
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
) -> Result<i64> {
    conn.execute(
        "INSERT INTO source_records (paper_id, source, source_id, raw_json, retrieved_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![paper_id, source, source_id, raw_json, now_utc()],
    )?;
    let source_record_id = conn.last_insert_rowid();
    if let Some(raw_json) = raw_json {
        let _ = insert_keyword_inputs(
            conn,
            paper_id,
            &keyword_inputs_from_provider_json(source, raw_json),
            Some(source_record_id),
        )?;
    }
    Ok(source_record_id)
}

/// Normalize only for deterministic duplicate suppression. This does not
/// rewrite the displayed keyword and does not perform semantic/fuzzy merging.
pub fn normalize_keyword(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn accepted_keyword_kind(kind: &str) -> bool {
    matches!(kind, "author_keyword" | "publisher_keyword" | "subject" | "concept")
}

fn keyword_input_is_allowed(input: &crate::models::PaperKeywordInput) -> bool {
    if !accepted_keyword_kind(&input.kind) || input.source.trim().is_empty() {
        return false;
    }
    // OpenAlex concepts/topics and Crossref subjects are not author keywords.
    // AI output is not a bibliographic source at all.
    if input.kind == "author_keyword"
        && matches!(input.source.as_str(), "openalex" | "crossref" | "ai" | "ai_suggestion")
    {
        return false;
    }
    !matches!(input.source.as_str(), "ai" | "ai_suggestion")
}

/// Persist provider/PDF keyword evidence without touching any recommendation
/// column on `papers`. `INSERT OR IGNORE` makes repeated sync/import passes
/// idempotent under the v16 uniqueness key.
pub fn insert_keyword_inputs(
    conn: &Connection,
    paper_id: i64,
    inputs: &[crate::models::PaperKeywordInput],
    source_record_id: Option<i64>,
) -> Result<usize> {
    let mut inserted = 0;
    for input in inputs {
        let keyword = input.keyword.trim();
        let normalized_keyword = normalize_keyword(keyword);
        if keyword.is_empty() || normalized_keyword.is_empty() || !keyword_input_is_allowed(input) {
            continue;
        }
        inserted += conn.execute(
            "INSERT OR IGNORE INTO paper_keywords (
                paper_id, keyword, normalized_keyword, kind, source, confidence,
                source_locator, source_record_id, language, position, retrieved_at, created_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11)",
            params![
                paper_id,
                keyword,
                normalized_keyword,
                input.kind,
                input.source,
                input.confidence,
                input.source_locator,
                source_record_id,
                input.language,
                input.position,
                now_utc(),
            ],
        )?;
    }
    Ok(inserted)
}

pub fn list_paper_keywords(conn: &Connection, paper_id: i64) -> Result<Vec<crate::models::PaperKeyword>> {
    let mut stmt = conn.prepare(
        "SELECT id, paper_id, keyword, normalized_keyword, kind, source, confidence,
                source_locator, source_record_id, language, position, retrieved_at, created_at
         FROM paper_keywords WHERE paper_id=?1 ORDER BY kind, position IS NULL, position, id",
    )?;
    let rows = stmt.query_map(params![paper_id], |r| {
        Ok(crate::models::PaperKeyword {
            id: r.get(0)?,
            paper_id: r.get(1)?,
            keyword: r.get(2)?,
            normalized_keyword: r.get(3)?,
            kind: r.get(4)?,
            source: r.get(5)?,
            confidence: r.get(6)?,
            source_locator: r.get(7)?,
            source_record_id: r.get(8)?,
            language: r.get(9)?,
            position: r.get(10)?,
            retrieved_at: r.get(11)?,
            created_at: r.get(12)?,
        })
    })?;
    rows.collect()
}

fn value_text(value: &serde_json::Value) -> Option<String> {
    value.as_str().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

/// Extract only explicit source fields. In particular, OpenAlex `concepts`,
/// `keywords`, and `topics` all become `concept`, never `author_keyword`.
pub(crate) fn keyword_inputs_from_provider_json(source: &str, raw_json: &str) -> Vec<crate::models::PaperKeywordInput> {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(raw_json) else { return Vec::new(); };
    if let Some(message) = value.get("message") {
        value = message.clone();
    }
    let source = source.trim().to_lowercase();
    let mut out = Vec::new();
    if source == "crossref" {
        if let Some(values) = value.get("subject").and_then(|v| v.as_array()) {
            for (position, value) in values.iter().enumerate() {
                if let Some(keyword) = value_text(value) {
                    out.push(crate::models::PaperKeywordInput {
                        keyword,
                        kind: "subject".to_string(),
                        source: "crossref".to_string(),
                        confidence: "HIGH".to_string(),
                        source_locator: Some(format!("message.subject[{}]", position)),
                        language: None,
                        position: Some(position as i64),
                    });
                }
            }
        }
    } else if source == "openalex" {
        for field in ["keywords", "concepts", "topics"] {
            if let Some(values) = value.get(field).and_then(|v| v.as_array()) {
                for (position, value) in values.iter().enumerate() {
                    let keyword = value_text(value)
                        .or_else(|| value.get("keyword").and_then(value_text))
                        .or_else(|| value.get("display_name").and_then(value_text));
                    if let Some(keyword) = keyword {
                        out.push(crate::models::PaperKeywordInput {
                            keyword,
                            kind: "concept".to_string(),
                            source: "openalex".to_string(),
                            confidence: value.get("score").and_then(|v| v.as_f64())
                                .map(|score| if score >= 0.75 { "HIGH" } else { "MEDIUM" })
                                .unwrap_or("MEDIUM")
                                .to_string(),
                            source_locator: Some(format!("{}.{}", field, position)),
                            language: None,
                            position: Some(position as i64),
                        });
                    }
                }
            }
        }
    }
    out
}

fn legacy_bibliographic_source(value: Option<&str>) -> Option<String> {
    let value = value.map(str::trim).filter(|value| !value.is_empty())?;
    let lower = value.to_ascii_lowercase();
    let is_provenance = matches!(
        lower.as_str(),
        "external pdf import" | "crossref" | "openalex" | "publisher" | "provider" | "manual" | "sync" | "discovery"
    ) || ["external ", "external_", "crossref:", "openalex:", "publisher:", "provider:", "source:"]
        .iter()
        .any(|prefix| lower.starts_with(prefix));
    (!is_provenance).then(|| value.to_string())
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
    let legacy_source: Option<String> = row.get("discovery_source")?;
    let journal_name = row
        .get::<_, Option<String>>("journal_name")?
        .and_then(|value| {
            let value = value.trim().to_string();
            legacy_bibliographic_source(Some(&value))
        })
        .or_else(|| legacy_bibliographic_source(legacy_source.as_deref()));
    Ok(Paper {
        id: row.get("id")?,
        journal_id: row.get("journal_id")?,
        journal_name,
        publisher: row.get("publisher")?,
        volume: row.get("volume")?,
        issue: row.get("issue")?,
        pages: row.get("pages")?,
        abstract_provenance: row.get("abstract_provenance")?,

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
        keywords: Vec::new(),
    })
}

pub fn list_papers(conn: &Connection, journal_id: Option<i64>, limit: i64) -> Result<Vec<Paper>> {
    let sql = format!(
        "SELECT p.*, COALESCE(NULLIF(trim(p.container_title), ''), j.name) AS journal_name FROM papers p
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
    enrich_papers_keywords(conn, &mut papers)?;
    filter_current_tag_matches(conn, &mut papers)?;
    Ok(papers)
}

pub fn list_papers_for_first_seen_cycle(conn: &Connection, cycle_key: &str, missing_only: bool) -> Result<Vec<Paper>> {
    let sql = if missing_only {
        "SELECT p.*,COALESCE(NULLIF(trim(p.container_title), ''), j.name) AS journal_name FROM papers p JOIN journals j ON j.id=p.journal_id WHERE p.first_seen_cycle=?1 AND p.first_seen_abstract_missing=1 ORDER BY p.id DESC"
    } else {
        "SELECT p.*,COALESCE(NULLIF(trim(p.container_title), ''), j.name) AS journal_name FROM papers p JOIN journals j ON j.id=p.journal_id WHERE p.first_seen_cycle=?1 ORDER BY p.id DESC"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![cycle_key], row_to_paper)?;
    let mut papers: Vec<Paper> = rows.collect::<Result<Vec<_>>>()?;
    enrich_papers_collections(conn, &mut papers)?;
    enrich_papers_keywords(conn, &mut papers)?;
    filter_current_tag_matches(conn, &mut papers)?;
    Ok(papers)
}

pub fn list_current_missing_papers_for_cycle(conn: &Connection, cycle_key: &str) -> Result<Vec<Paper>> {
    let mut stmt = conn.prepare("SELECT p.*,COALESCE(NULLIF(trim(p.container_title), ''), j.name) AS journal_name FROM papers p JOIN journals j ON j.id=p.journal_id WHERE p.first_seen_cycle=?1 AND p.abstract_quality='missing' ORDER BY p.id DESC")?;
    let rows = stmt.query_map(params![cycle_key], row_to_paper)?;
    let mut papers: Vec<Paper> = rows.collect::<Result<Vec<_>>>()?;
    enrich_papers_collections(conn, &mut papers)?;
    enrich_papers_keywords(conn, &mut papers)?;
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

/// Populate bibliographic keywords from the canonical relation. This keeps
/// Discovery and Library on the same Paper DTO; Library-only tags and metadata
/// overrides remain separate layers.
fn enrich_papers_keywords(conn: &Connection, papers: &mut [Paper]) -> Result<()> {
    for paper in papers.iter_mut() {
        paper.keywords = list_paper_keywords(conn, paper.id)?;
    }
    Ok(())
}

/// 单篇论文（含 collections 派生）。
pub fn get_paper(conn: &Connection, id: i64) -> Result<Option<Paper>> {
    let p = conn
        .query_row(
            "SELECT p.*, COALESCE(NULLIF(trim(p.container_title), ''), j.name) AS journal_name FROM papers p LEFT JOIN journals j ON j.id = p.journal_id WHERE p.id = ?1",
            params![id],
            row_to_paper,
        )
        .optional()?;
    let mut v = Vec::new();
    if let Some(p) = p {
        v.push(p);
        enrich_papers_collections(conn, &mut v)?;
        enrich_papers_keywords(conn, &mut v)?;
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
    // `source_override` is a source label, not a journal. In particular,
    // abstract/discovery provenance values must never become effective_journal.
    let effective_source = metadata
        .as_ref()
        .and_then(|m| m.source_override.clone())
        .or_else(|| paper.discovery_source.clone());
    let effective_year = metadata.as_ref().and_then(|m| m.year_override).or(paper.year);
    let effective_authors = metadata
        .as_ref()
        .and_then(|m| m.authors_override.clone())
        .unwrap_or_else(|| paper.authors.clone());
    let effective_abstract = metadata
        .as_ref()
        .and_then(|m| m.abstract_override.clone())
        .or_else(|| (paper.abstract_provenance != "legacy_unverified").then(|| paper.abstract_text.clone()).flatten());
    let (legacy, translation_hash): (bool, Option<String>) = conn.query_row("SELECT p.legacy_abstract_unverified,m.chinese_abstract_source_hash FROM papers p LEFT JOIN library_item_metadata m ON m.paper_id=p.id WHERE p.id=?1", params![paper.id], |r| Ok((r.get(0)?,r.get(1)?)))?;
    let translation_current = match translation_hash.as_deref() {
        Some(hash) => effective_abstract.as_deref().is_some_and(|text| hash == abstract_text_hash(text)),
        None => !legacy,
    };
    let effective_chinese_abstract = metadata
        .as_ref()
        .and_then(|m| translation_current.then(|| m.chinese_abstract_override.clone()).flatten())
        .or_else(|| (paper.abstract_provenance != "legacy_unverified" && !legacy).then(|| paper.chinese_abstract.clone()).flatten());
    let note = metadata.as_ref().and_then(|m| m.note.clone());
    let attachments = list_paper_attachments(conn, paper.id)?;
    let effective_journal = metadata.as_ref().and_then(|m| m.journal_override.clone()).or_else(|| paper.journal_name.clone());
    let effective_publisher = metadata.as_ref().and_then(|m| m.publisher_override.clone()).or_else(|| paper.publisher.clone());
    let effective_publication_date = metadata.as_ref().and_then(|m| m.publication_date_override.clone()).or_else(|| paper.published_date.clone());
    let effective_volume = metadata.as_ref().and_then(|m| m.volume_override.clone()).or_else(|| paper.volume.clone());
    let effective_issue = metadata.as_ref().and_then(|m| m.issue_override.clone()).or_else(|| paper.issue.clone());
    let effective_pages = metadata.as_ref().and_then(|m| m.pages_override.clone()).or_else(|| paper.pages.clone());
    let effective_doi = metadata.as_ref().and_then(|m| m.doi_override.clone()).or_else(|| paper.normalized_doi.clone());
    let effective_url = metadata.as_ref().and_then(|m| m.url_override.clone()).or_else(|| paper.url.clone());
    Ok(crate::models::LibraryPaper {
        effective_journal,
        effective_publisher,
        effective_publication_date,
        effective_volume,
        effective_issue,
        effective_pages,
        effective_doi,
        effective_url,
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
        journal_override: row.get("journal_override")?,
        publisher_override: row.get("publisher_override")?,
        publication_date_override: row.get("publication_date_override")?,
        volume_override: row.get("volume_override")?,
        issue_override: row.get("issue_override")?,
        pages_override: row.get("pages_override")?,
        doi_override: row.get("doi_override").unwrap_or(None),
        url_override: row.get("url_override").unwrap_or(None),

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
    let old_chinese = get_library_item_metadata(&tx,paper_id)?.and_then(|m| m.chinese_abstract_override);
    let authors_json = input
        .authors_override
        .as_ref()
        .map(|authors| serde_json::to_string(authors).unwrap_or_else(|_| "[]".to_string()));
    let doi_override = match clean_optional_text(input.doi_override.as_deref()) {
        Some(value) => Some(crate::util::normalize_doi(&value).ok_or_else(|| rusqlite::Error::InvalidParameterName("doi_override".into()))?),
        None => None,
    };
    let url_override = clean_optional_text(input.url_override.as_deref());
    if let Some(url) = url_override.as_deref() {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(rusqlite::Error::InvalidParameterName("url_override".into()));
        }
    }
    let now = now_utc();
    tx.execute(
        "INSERT INTO library_item_metadata (
            paper_id, title_override, chinese_title_override, source_override,
            doi_override, url_override,
            year_override, authors_override, abstract_override,
            chinese_abstract_override, note, updated_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
         ON CONFLICT(paper_id) DO UPDATE SET
            title_override=excluded.title_override,
            chinese_title_override=excluded.chinese_title_override,
            source_override=excluded.source_override,
            doi_override=excluded.doi_override,
            url_override=excluded.url_override,
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
            doi_override,
            url_override,
            input.year_override,
            authors_json,
            clean_optional_text(input.abstract_override.as_deref()),
            clean_optional_text(input.chinese_abstract_override.as_deref()),
            clean_optional_text(input.note.as_deref()),
            now,
        ],
    )?;
    tx.execute("UPDATE library_item_metadata SET journal_override=?1, publisher_override=?2, publication_date_override=?3, volume_override=?4, issue_override=?5, pages_override=?6 WHERE paper_id=?7",
        params![clean_optional_text(input.journal_override.as_deref()), clean_optional_text(input.publisher_override.as_deref()), clean_optional_text(input.publication_date_override.as_deref()), clean_optional_text(input.volume_override.as_deref()), clean_optional_text(input.issue_override.as_deref()), clean_optional_text(input.pages_override.as_deref()), paper_id])?;
    if clean_optional_text(input.chinese_abstract_override.as_deref()) != old_chinese {
        let source_hash = get_library_paper(&tx,paper_id)?.and_then(|p| p.effective_abstract).map(|s| abstract_text_hash(&s));
        tx.execute("UPDATE library_item_metadata SET chinese_abstract_source_hash=?1 WHERE paper_id=?2",params![source_hash,paper_id])?;
    }
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
            journal_override=NULL, publisher_override=NULL, publication_date_override=NULL, volume_override=NULL, issue_override=NULL, pages_override=NULL,
            title_override=NULL, chinese_title_override=NULL, source_override=NULL,
            doi_override=NULL, url_override=NULL,
            year_override=NULL, authors_override=NULL, abstract_override=NULL,
            chinese_abstract_override=NULL, updated_at=?1 WHERE paper_id=?2",
        params![now_utc(), paper_id],
    )?;
    get_library_item_metadata(conn, paper_id)
}

const PDF_FILE_HANDLING_MODE_KEY: &str = "settings.pdf_file_handling_mode";
const PDF_LIBRARY_ROOT_KEY: &str = "settings.pdf_library_root";
const PDF_NAMING_TEMPLATE_KEY: &str = "settings.pdf_naming_template";
const PDF_SUBFOLDER_RULE_KEY: &str = "settings.pdf_subfolder_rule";
pub const PREFERRED_PDF_READER_KEY: &str = "settings.preferred_pdf_reader";
const MAX_MANAGED_FILENAME_BYTES: usize = 180;

#[derive(Debug, Clone)]
struct PdfStorageConfig {
    mode: String,
    library_root: String,
    naming_template: String,
    subfolder_rule: String,
}

#[derive(Debug, Clone)]
struct PreparedPdfStorage {
    storage_mode: String,
    absolute_path: PathBuf,
    relative_path: String,
    source_path: PathBuf,
    source_sha256: String,
    delete_source: bool,
    created_destination: bool,
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

pub fn validate_pdf_storage_settings(
    mode: &str,
    library_root: &str,
    naming_template: &str,
    subfolder_rule: &str,
) -> std::result::Result<(), String> {
    if !matches!(mode, "none" | "copy" | "move") {
        return Err("PDF file handling mode 必须为 none、copy 或 move".to_string());
    }
    if !matches!(subfolder_rule, "none" | "year" | "journal/source") {
        return Err("PDF 子文件夹规则必须为 none、year 或 journal/source".to_string());
    }
    if naming_template.trim().is_empty() {
        return Err("PDF 命名模板不能为空".to_string());
    }
    if mode != "none" {
        let root = Path::new(library_root.trim());
        if library_root.trim().is_empty() {
            return Err("copy/move 模式必须配置 Library root directory".to_string());
        }
        if !root.is_absolute() {
            return Err("Library root directory 必须是绝对路径".to_string());
        }
    }
    Ok(())
}

/// Validate a reader preference without probing or executing it. `system` is
/// portable; custom readers must be absolute paths so a setting cannot select
/// an unexpected executable through PATH lookup.
pub fn validate_preferred_pdf_reader(reader: &str) -> std::result::Result<(), String> {
    let reader = reader.trim();
    if reader == "system" { return Ok(()); }
    if reader.is_empty() || !Path::new(reader).is_absolute() {
        return Err("preferred PDF reader 必须为 system 或绝对路径".to_string());
    }
    Ok(())
}

fn pdf_storage_config(conn: &Connection) -> Result<PdfStorageConfig> {
    let mode = get_setting(conn, PDF_FILE_HANDLING_MODE_KEY)
        .unwrap_or_else(crate::models::default_pdf_file_handling_mode);
    let library_root = get_setting(conn, PDF_LIBRARY_ROOT_KEY).unwrap_or_default();
    let naming_template = get_setting(conn, PDF_NAMING_TEMPLATE_KEY)
        .unwrap_or_else(crate::models::default_pdf_naming_template);
    let subfolder_rule = get_setting(conn, PDF_SUBFOLDER_RULE_KEY)
        .unwrap_or_else(crate::models::default_pdf_subfolder_rule);
    validate_pdf_storage_settings(&mode, &library_root, &naming_template, &subfolder_rule)
        .map_err(rusqlite::Error::InvalidParameterName)?;
    Ok(PdfStorageConfig { mode, library_root, naming_template, subfolder_rule })
}

fn author_display_name(author: &Author) -> String {
    if let Some(name) = author.name.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        return name.to_string();
    }
    match (
        author.given.as_deref().map(str::trim).filter(|value| !value.is_empty()),
        author.family.as_deref().map(str::trim).filter(|value| !value.is_empty()),
    ) {
        (Some(given), Some(family)) => format!("{given} {family}"),
        (Some(given), None) => given.to_string(),
        (None, Some(family)) => family.to_string(),
        (None, None) => String::new(),
    }
}

#[derive(Debug, Clone, Default)]
struct PdfNamingContext {
    title: String,
    journal: String,
    source: String,
    year: String,
    authors: String,
    first_author: String,
    doi: String,
}

fn paper_naming_context(conn: &Connection, paper_id: i64) -> Result<PdfNamingContext> {
    conn.query_row(
        "SELECT p.title, p.authors_json, p.year, p.normalized_doi, p.original_doi,
                p.discovery_source,
                COALESCE(NULLIF(trim(p.container_title), ''), NULLIF(j.name, 'External PDF Import')),
                m.journal_override, m.source_override, m.year_override, m.doi_override
         FROM papers p LEFT JOIN journals j ON j.id = p.journal_id
         LEFT JOIN library_item_metadata m ON m.paper_id=p.id
         WHERE p.id=?1",
        params![paper_id],
        |row| {
            let authors_json: Option<String> = row.get(1)?;
            let authors: Vec<Author> = authors_json
                .as_deref()
                .and_then(|value| serde_json::from_str(value).ok())
                .unwrap_or_default();
            let author_names: Vec<String> = authors
                .iter()
                .map(author_display_name)
                .filter(|value| !value.is_empty())
                .collect();
            let canonical_journal = legacy_bibliographic_source(row.get::<_, Option<String>>(6)?.as_deref()).unwrap_or_default();
            let journal = row.get::<_, Option<String>>(7)?.unwrap_or(canonical_journal);
            let source = row.get::<_, Option<String>>(8)?.or_else(|| row.get(5).ok()).unwrap_or_default();
            Ok(PdfNamingContext {
                title: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                journal: journal.clone(),
                source: if source.trim().is_empty() { journal } else { source },
                year: row.get::<_, Option<i32>>(9)?.or(row.get(2)?).map(|value| value.to_string()).unwrap_or_default(),
                authors: author_names.join(", "),
                first_author: author_names.first().cloned().unwrap_or_default(),
                doi: row.get::<_, Option<String>>(10)?.or(row.get(3)?).or(row.get(4)?).unwrap_or_default(),
            })
        },
    )
}

fn template_token_value<'a>(token: &str, context: &'a PdfNamingContext) -> Option<&'a str> {
    match token {
        "title" => Some(&context.title),
        "journal" => Some(&context.journal),
        "source" => Some(&context.source),
        "year" => Some(&context.year),
        "authors" => Some(&context.authors),
        "first_author" => Some(&context.first_author),
        "doi" => Some(&context.doi),
        _ => None,
    }
}

fn is_template_separator(ch: char) -> bool {
    matches!(ch, '-' | '–' | '—' | '_')
}

fn has_trailing_template_separator(value: &str) -> bool {
    value.trim_end().chars().last().is_some_and(is_template_separator)
}

fn strip_leading_template_separator(value: &str) -> String {
    let mut chars = value.char_indices();
    while let Some((index, ch)) = chars.next() {
        if ch.is_whitespace() {
            continue;
        } else if is_template_separator(ch) {
            let mut end = index + ch.len_utf8();
            while let Some((next_index, next)) = chars.next() {
                if !next.is_whitespace() {
                    return value[next_index..].to_string();
                }
                end = next_index + next.len_utf8();
            }
            return value[end..].to_string();
        } else {
            break;
        }
    }
    value.to_string()
}

fn trim_trailing_template_separator_before_extension(value: &mut String) {
    let Some(extension_start) = value.to_ascii_lowercase().rfind(".pdf") else { return; };
    let mut stem = value[..extension_start].trim_end().to_string();
    while stem.chars().last().is_some_and(is_template_separator) {
        stem.pop();
        while stem.chars().last().is_some_and(|ch| ch.is_whitespace()) {
            stem.pop();
        }
    }
    *value = format!("{}{}", stem.trim_end(), &value[extension_start..]);
}

fn render_pdf_filename(template: &str, context: &PdfNamingContext) -> String {
    let mut output = String::new();
    let mut cursor = 0;
    let mut skip_leading_separator = false;
    while cursor < template.len() {
        let Some(open_offset) = template[cursor..].find('{') else {
            let literal = &template[cursor..];
            if skip_leading_separator {
                let stripped = strip_leading_template_separator(literal);
                output.push_str(&stripped);
            } else {
                output.push_str(literal);
            }
            break;
        };
        let open = cursor + open_offset;
        let literal = &template[cursor..open];
        if skip_leading_separator {
            output.push_str(&strip_leading_template_separator(literal));
        } else {
            output.push_str(literal);
        }
        let Some(close_offset) = template[open + 1..].find('}') else {
            output.push_str(&template[open..]);
            break;
        };
        let close = open + 1 + close_offset;
        let token = &template[open + 1..close];
        if let Some(value) = template_token_value(token, context) {
            if value.trim().is_empty() {
                skip_leading_separator = !has_trailing_template_separator(&output);
            } else {
                output.push_str(value.trim());
                skip_leading_separator = false;
            }
        } else {
            // Unknown tokens are treated as empty fields so a future template
            // token can never leak braces or an unsafe path component to disk.
            skip_leading_separator = !has_trailing_template_separator(&output);
        }
        cursor = close + 1;
    }
    trim_trailing_template_separator_before_extension(&mut output);
    let output = output.trim().to_string();
    if output.is_empty() { "document.pdf".to_string() } else { output }
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes { return value.to_string(); }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) { end -= 1; }
    value[..end].to_string()
}

fn is_windows_reserved_basename(value: &str) -> bool {
    let upper = value.trim().to_ascii_uppercase();
    let basename = upper.split('.').next().unwrap_or(upper.as_str());
    matches!(basename, "CON" | "PRN" | "AUX" | "NUL")
        || ((basename.starts_with("COM") || basename.starts_with("LPT"))
            && basename[3..].parse::<u8>().is_ok_and(|number| (1..=9).contains(&number)))
}

fn sanitize_filename(value: &str) -> String {
    let mut sanitized: String = value
        .chars()
        .map(|ch| if ch.is_control() || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { ch })
        .collect();
    sanitized = sanitized.trim().trim_matches('.').trim().to_string();
    if sanitized.is_empty() { sanitized = "document".to_string(); }
    let has_pdf_extension = sanitized.to_ascii_lowercase().ends_with(".pdf");
    if !has_pdf_extension { sanitized.push_str(".pdf"); }
    let extension = ".pdf";
    let stem_end = sanitized.len().saturating_sub(extension.len());
    let mut stem = truncate_utf8(&sanitized[..stem_end], MAX_MANAGED_FILENAME_BYTES.saturating_sub(extension.len()));
    if stem.trim().is_empty() { stem = "document".to_string(); }
    if is_windows_reserved_basename(&stem) {
        stem.insert(0, '_');
        stem = truncate_utf8(&stem, MAX_MANAGED_FILENAME_BYTES.saturating_sub(extension.len()));
    }
    truncate_utf8(&format!("{stem}{extension}"), MAX_MANAGED_FILENAME_BYTES)
}

fn sanitize_folder_component(value: &str) -> String {
    let mut component: String = value
        .chars()
        .map(|ch| if ch.is_control() || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { ch })
        .collect();
    component = component.trim().trim_matches('.').trim().to_string();
    if component.is_empty() { component = "Unknown".to_string(); }
    if is_windows_reserved_basename(&component) { component.insert(0, '_'); }
    truncate_utf8(&component, 120)
}

fn managed_destination_directory(root: &Path, rule: &str, context: &PdfNamingContext) -> PathBuf {
    match rule {
        "year" => root.join(sanitize_folder_component(if context.year.is_empty() { "Unknown" } else { &context.year })),
        "journal/source" => root
            .join(sanitize_folder_component(if context.journal.is_empty() { "Unknown" } else { &context.journal }))
            .join(sanitize_folder_component(if context.source.is_empty() { "Unknown" } else { &context.source })),
        _ => root.to_path_buf(),
    }
}

fn collision_filename(filename: &str, number: usize) -> String {
    let suffix = format!(" ({number})");
    let extension = ".pdf";
    let stem = filename.strip_suffix(extension).unwrap_or(filename);
    let max_stem_bytes = MAX_MANAGED_FILENAME_BYTES.saturating_sub(extension.len() + suffix.len());
    format!("{}{}{}", truncate_utf8(stem, max_stem_bytes), suffix, extension)
}

fn copy_file_verified(source: &Path, source_sha256: &str, directory: &Path, filename: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(directory).map_err(|_| rusqlite::Error::InvalidQuery)?;
    for number in 1..=10_000_usize {
        let candidate_name = if number == 1 { filename.to_string() } else { collision_filename(filename, number) };
        let candidate = directory.join(candidate_name);
        let mut destination = match OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(rusqlite::Error::InvalidQuery),
        };
        let copy_result = (|| {
            let mut input = File::open(source).map_err(|_| rusqlite::Error::InvalidQuery)?;
            std::io::copy(&mut input, &mut destination).map_err(|_| rusqlite::Error::InvalidQuery)?;
            destination.sync_all().map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok::<(), rusqlite::Error>(())
        })();
        drop(destination);
        if let Err(error) = copy_result {
            let _ = std::fs::remove_file(&candidate);
            return Err(error);
        }
        let destination_hash = sha256_file(&candidate);
        if !destination_hash.is_ok_and(|hash| hash == source_sha256) {
            let _ = std::fs::remove_file(&candidate);
            return Err(rusqlite::Error::InvalidQuery);
        }
        return std::fs::canonicalize(&candidate).map_err(|_| rusqlite::Error::InvalidQuery);
    }
    Err(rusqlite::Error::InvalidQuery)
}

fn managed_relative_path(root: &Path, destination: &Path) -> Result<String> {
    destination
        .strip_prefix(root)
        .map(|relative| relative.to_string_lossy().into_owned())
        .map_err(|_| rusqlite::Error::InvalidParameterName("managed_path".into()))
}

fn prepare_managed_pdf(
    conn: &Connection,
    paper_id: i64,
    source_path: &Path,
    source_sha256: &str,
    mode: &str,
) -> Result<PreparedPdfStorage> {
    if !matches!(mode, "copy" | "move") {
        return Err(rusqlite::Error::InvalidParameterName("storage_mode".into()));
    }
    let config = pdf_storage_config(conn)?;
    let root = PathBuf::from(config.library_root.trim());
    std::fs::create_dir_all(&root).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let root = std::fs::canonicalize(&root).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let context = paper_naming_context(conn, paper_id)?;
    let directory = managed_destination_directory(&root, &config.subfolder_rule, &context);
    std::fs::create_dir_all(&directory).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let directory = std::fs::canonicalize(&directory).map_err(|_| rusqlite::Error::InvalidQuery)?;
    let filename = sanitize_filename(&render_pdf_filename(&config.naming_template, &context));
    let preferred = directory.join(&filename);
    let source_is_preferred = source_path == preferred.as_path()
        || std::fs::canonicalize(&preferred).ok().is_some_and(|path| path.as_path() == source_path);
    let destination = if source_is_preferred {
        source_path.to_path_buf()
    } else {
        copy_file_verified(source_path, source_sha256, &directory, &filename)?
    };
    let relative_path = managed_relative_path(&root, &destination)?;
    Ok(PreparedPdfStorage {
        storage_mode: "managed".to_string(),
        absolute_path: destination,
        relative_path,
        source_path: source_path.to_path_buf(),
        source_sha256: source_sha256.to_string(),
        // `move` is a verified copy followed by source deletion only after
        // the database update commits. Preparation/copy failures therefore
        // always leave the source untouched.
        delete_source: mode == "move" && !source_is_preferred,
        created_destination: !source_is_preferred,
    })
}

fn insert_attachment_row(
    conn: &Connection,
    paper_id: i64,
    file: &LinkedFile,
    prepared: Option<&PreparedPdfStorage>,
) -> Result<i64> {
    let now = now_utc();
    if let Some(prepared) = prepared {
        conn.execute(
            "INSERT INTO paper_attachments (
                paper_id, kind, storage_mode, absolute_path, relative_path, url,
                filename, mime_type, sha256, created_at, updated_at
             ) VALUES (?1,'pdf',?2,?3,?4,NULL,?5,'application/pdf',?6,?7,?7)",
            params![
                paper_id,
                prepared.storage_mode.as_str(),
                prepared.absolute_path.to_string_lossy().as_ref(),
                prepared.relative_path.as_str(),
                prepared.absolute_path.file_name().and_then(|value| value.to_str()).unwrap_or(&file.filename),
                prepared.source_sha256.as_str(),
                now,
            ],
        )?;
    } else {
        conn.execute(
            "INSERT INTO paper_attachments (
                paper_id, kind, storage_mode, absolute_path, relative_path, url,
                filename, mime_type, sha256, created_at, updated_at
             ) VALUES (?1,'pdf','linked',?2,NULL,NULL,?3,'application/pdf',?4,?5,?5)",
            params![paper_id, file.absolute_path.to_string_lossy().as_ref(), file.filename, file.sha256, now],
        )?;
    }
    Ok(conn.last_insert_rowid())
}

fn finalize_prepared_storage(conn: &Connection, prepared: &PreparedPdfStorage) -> Result<()> {
    if !prepared.absolute_path.is_file() || !sha256_file(&prepared.absolute_path).is_ok_and(|hash| hash == prepared.source_sha256) {
        if prepared.created_destination { let _ = std::fs::remove_file(&prepared.absolute_path); }
        return Err(rusqlite::Error::InvalidQuery);
    }
    if !prepared.delete_source || prepared.source_path == prepared.absolute_path {
        return Ok(());
    }
    // A concurrent editor must not have its newer source content deleted after
    // the copy. If the source changed, keep it and let the user retry.
    if !prepared.source_path.is_file()
        || !sha256_file(&prepared.source_path).is_ok_and(|hash| hash == prepared.source_sha256)
    {
        return Err(rusqlite::Error::InvalidParameterName("source_pdf_changed_during_move".into()));
    }
    let referenced_elsewhere: bool = conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM paper_attachments
            WHERE absolute_path=?1 AND absolute_path != ?2
        )",
        params![prepared.source_path.to_string_lossy().as_ref(), prepared.absolute_path.to_string_lossy().as_ref()],
        |row| row.get(0),
    )?;
    if referenced_elsewhere {
        return Err(rusqlite::Error::InvalidParameterName("source_pdf_is_still_referenced".into()));
    }
    std::fs::remove_file(&prepared.source_path).map_err(|_| rusqlite::Error::InvalidQuery)
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

fn prepare_current_pdf_storage(
    conn: &Connection,
    paper_id: i64,
    file: &LinkedFile,
) -> Result<Option<PreparedPdfStorage>> {
    let config = pdf_storage_config(conn)?;
    if config.mode == "none" {
        Ok(None)
    } else {
        prepare_managed_pdf(conn, paper_id, &file.absolute_path, &file.sha256, &config.mode).map(Some)
    }
}

fn cleanup_prepared_destination(prepared: &PreparedPdfStorage) {
    if prepared.created_destination {
        let _ = std::fs::remove_file(&prepared.absolute_path);
    }
}

fn insert_file_attachment(
    conn: &Connection,
    paper_id: i64,
    file: &LinkedFile,
) -> Result<crate::models::PaperAttachment> {
    if !paper_exists(conn, paper_id)? {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    // The v0.2.0 one-PDF rule is a UI policy. Keep the attachment schema and
    // low-level attach API multi-attachment capable so explicit relink/manage
    // operations and future versions do not silently collapse relations.
    let prepared = prepare_current_pdf_storage(conn, paper_id, file)?;
    let tx = conn.unchecked_transaction()?;
    let id = match insert_attachment_row(&tx, paper_id, file, prepared.as_ref()) {
        Ok(id) => id,
        Err(error) => {
            if let Some(prepared) = prepared.as_ref() { cleanup_prepared_destination(prepared); }
            return Err(error);
        }
    };
    if let Err(error) = tx.commit() {
        if let Some(prepared) = prepared.as_ref() { cleanup_prepared_destination(prepared); }
        return Err(error);
    }
    if let Some(prepared) = prepared.as_ref() {
        finalize_prepared_storage(conn, prepared)?;
    }
    get_paper_attachment(conn, id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

/// Attach an existing local PDF to a canonical Paper. The configured file
/// handling mode decides whether it remains linked or becomes managed.
pub fn attach_pdf_to_paper(
    conn: &Connection,
    paper_id: i64,
    path: &str,
) -> Result<crate::models::PaperAttachment> {
    let file = linked_file(path)?;
    insert_file_attachment(conn, paper_id, &file)
}

/// Discovery's Attach PDF action is a durable Library action. It atomically
/// creates membership (when absent), clears Read Later, and inserts the PDF
/// using the configured file handling mode.
pub fn attach_discovery_pdf(
    conn: &Connection,
    paper_id: i64,
    path: &str,
) -> Result<crate::models::PaperAttachment> {
    let file = linked_file(path)?;
    if !paper_exists(conn, paper_id)? {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    if let Some(existing) = list_paper_attachments(conn, paper_id)?.into_iter().find(|a| a.sha256.as_deref() == Some(&file.sha256)) {
        let now = now_utc();
        conn.execute(
            "INSERT INTO library_items(paper_id,added_at,added_source) VALUES(?1,?2,'discovery_attach_pdf') ON CONFLICT(paper_id) DO NOTHING",
            params![paper_id, now],
        )?;
        return Ok(existing);
    }
    let prepared = prepare_current_pdf_storage(conn, paper_id, &file)?;
    let tx = conn.unchecked_transaction()?;
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
    let id = match insert_attachment_row(&tx, paper_id, &file, prepared.as_ref()) {
        Ok(id) => id,
        Err(error) => {
            if let Some(prepared) = prepared.as_ref() { cleanup_prepared_destination(prepared); }
            return Err(error);
        }
    };
    if let Err(error) = tx.commit() {
        if let Some(prepared) = prepared.as_ref() { cleanup_prepared_destination(prepared); }
        return Err(error);
    }
    if let Some(prepared) = prepared.as_ref() {
        finalize_prepared_storage(conn, prepared)?;
    }
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

fn update_attachment_as_managed(
    conn: &Connection,
    attachment_id: i64,
    prepared: &PreparedPdfStorage,
) -> Result<()> {
    conn.execute(
        "UPDATE paper_attachments SET storage_mode='managed', absolute_path=?1,
            relative_path=?2, url=NULL, filename=?3, mime_type='application/pdf',
            sha256=?4, updated_at=?5 WHERE id=?6",
        params![
            prepared.absolute_path.to_string_lossy().as_ref(),
            prepared.relative_path.as_str(),
            prepared.absolute_path.file_name().and_then(|value| value.to_str()).unwrap_or("document.pdf"),
            prepared.source_sha256.as_str(),
            now_utc(),
            attachment_id,
        ],
    )?;
    Ok(())
}

fn manage_existing_attachment(
    conn: &Connection,
    attachment_id: i64,
    mode: &str,
) -> Result<crate::models::PaperAttachment> {
    if !matches!(mode, "copy" | "move") {
        return Err(rusqlite::Error::InvalidParameterName("storage_mode".into()));
    }
    let current = get_paper_attachment(conn, attachment_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    let source_path = resolve_linked_pdf_path(&current.absolute_path)?;
    if !is_pdf_file(&source_path)? {
        return Err(rusqlite::Error::InvalidParameterName("pdf_path".into()));
    }
    let source_sha256 = sha256_file(&source_path)?;
    let prepared = prepare_managed_pdf(conn, current.paper_id, &source_path, &source_sha256, mode)?;
    let tx = conn.unchecked_transaction()?;
    if let Err(error) = update_attachment_as_managed(&tx, attachment_id, &prepared) {
        cleanup_prepared_destination(&prepared);
        return Err(error);
    }
    if let Err(error) = tx.commit() {
        cleanup_prepared_destination(&prepared);
        return Err(error);
    }
    finalize_prepared_storage(conn, &prepared)?;
    get_paper_attachment(conn, attachment_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

/// Explicitly reorganize one existing linked/managed attachment into the
/// configured library. Settings changes never call this implicitly.
pub fn reorganize_pdf(
    conn: &Connection,
    attachment_id: i64,
    mode: &str,
) -> Result<crate::models::PaperAttachment> {
    manage_existing_attachment(conn, attachment_id, mode)
}

/// Explicitly rename/reorganize a managed PDF using the current naming
/// template and subfolder rule. Metadata edits do not invoke this command.
pub fn rename_managed_pdf(
    conn: &Connection,
    attachment_id: i64,
) -> Result<crate::models::PaperAttachment> {
    let current = get_paper_attachment(conn, attachment_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    if current.storage_mode != "managed" {
        return Err(rusqlite::Error::InvalidParameterName("storage_mode".into()));
    }
    manage_existing_attachment(conn, attachment_id, "move")
}

fn launch_file_action(path: &Path, reveal: bool, preferred_reader: Option<&str>) -> Result<()> {
    if !path.is_file() {
        return Err(rusqlite::Error::InvalidParameterName("missing_attachment".into()));
    }
    #[cfg(target_os = "macos")]
    let status = if reveal {
        Command::new("open").arg("-R").arg(path).status()
    } else if let Some(reader) = preferred_reader.filter(|v| *v != "system") {
        Command::new("open").args(["-a", reader]).arg(path).status()
    } else {
        Command::new("open").arg(path).status()
    };
    #[cfg(target_os = "windows")]
    let status = if reveal {
        Command::new("explorer").arg(format!("/select,{}", path.display())).status()
    } else if let Some(reader) = preferred_reader.filter(|v| *v != "system") {
        Command::new(reader).arg(path).status()
    } else {
        let path_string = path.to_string_lossy().into_owned();
        Command::new("cmd").args(["/C", "start", ""]).arg(path_string).status()
    };
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let status = if reveal {
        Command::new("xdg-open").arg(path.parent().unwrap_or(path)).status()
    } else if let Some(reader) = preferred_reader.filter(|v| *v != "system") {
        Command::new(reader).arg(path).status()
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
    let reader = get_setting(conn, PREFERRED_PDF_READER_KEY).unwrap_or_else(crate::models::default_preferred_pdf_reader);
    validate_preferred_pdf_reader(&reader).map_err(rusqlite::Error::InvalidParameterName)?;
    launch_file_action(Path::new(&attachment.absolute_path), false, Some(&reader))
}

pub fn reveal_pdf(conn: &Connection, attachment_id: i64) -> Result<()> {
    let attachment = get_paper_attachment(conn, attachment_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    launch_file_action(Path::new(&attachment.absolute_path), true, None)
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
        // PDF content streams may glue the following Copyright label to DOI.
        let lower = candidate.to_ascii_lowercase();
        let boundary = lower.find("copyright").filter(|i| *i > 0 && candidate.as_bytes()[i-1].is_ascii_digit()
            && value[start + i + "copyright".len()..].trim_start().starts_with([':', '©']));
        let candidate = &candidate[..boundary.unwrap_or(candidate.len())];
        let candidate = candidate.trim_end_matches(|ch: char| matches!(ch, '.' | ',' | ';' | ':' | ')' | ']'));
        let valid_prefix = candidate.split_once('/').is_some_and(|(prefix,suffix)| {
            let digits = prefix.strip_prefix("10.").unwrap_or("");
            (4..=9).contains(&digits.len()) && digits.chars().all(|c| c.is_ascii_digit()) && !suffix.is_empty()
        });
        if valid_prefix {
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
    if bytes.len() < 4 {
        return None;
    }
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

fn parse_keyword_metadata(
    value: Option<&str>,
    kind: &str,
    source: &str,
    source_locator: &str,
) -> Vec<crate::models::PaperKeywordInput> {
    let Some(value) = value else { return Vec::new(); };
    value
        .split(|ch: char| matches!(ch, ';' | ',' | '\n' | '\r' | '|'))
        .map(str::trim)
        .filter(|keyword| !keyword.is_empty())
        .enumerate()
        .map(|(position, keyword)| crate::models::PaperKeywordInput {
            keyword: keyword.to_string(),
            kind: kind.to_string(),
            source: source.to_string(),
            confidence: "MEDIUM".to_string(),
            source_locator: Some(source_locator.to_string()),
            language: None,
            position: Some(position as i64),
        })
        .collect()
}

fn xmp_element_values(xml: &str, names: &[&str]) -> Vec<String> {
    let wanted: std::collections::HashSet<String> = names.iter().map(|name| name.to_ascii_lowercase()).collect();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut values = Vec::new();
    let mut capture: Option<(usize, String)> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                let name = String::from_utf8_lossy(event.local_name().as_ref()).to_ascii_lowercase();
                if let Some((depth, value)) = capture.as_mut() {
                    *depth += 1;
                    if !value.is_empty() { value.push(' '); }
                } else if wanted.contains(&name) {
                    capture = Some((1, String::new()));
                }
            }
            Ok(Event::Text(event)) => {
                if let Some((_, value)) = capture.as_mut() {
                    if !value.is_empty() { value.push(' '); }
                    value.push_str(String::from_utf8_lossy(event.as_ref()).trim());
                }
            }
            Ok(Event::CData(event)) => {
                if let Some((_, value)) = capture.as_mut() {
                    if let Ok(text) = std::str::from_utf8(event.as_ref()) {
                        if !value.is_empty() { value.push(' '); }
                        value.push_str(text.trim());
                    }
                }
            }
            Ok(Event::End(_)) => {
                if let Some((depth, _)) = capture.as_mut() {
                    if *depth == 1 {
                        if let Some((_, value)) = capture.take() {
                            if let Some(value) = clean_optional_text(Some(&value)) { values.push(value); }
                        }
                    } else {
                        *depth -= 1;
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    values
}

fn xmp_container_list_values(xml: &str, container: &str) -> Vec<String> {
    let lower = xml.to_ascii_lowercase();
    let container_lower = container.to_ascii_lowercase();
    let Some(start) = lower.find(&format!("<{}", container_lower)) else { return Vec::new(); };
    let Some(end_offset) = lower[start..].find(&format!("</{}>", container_lower)) else { return Vec::new(); };
    let end = start + end_offset + container.len() + 3;
    xmp_element_values(&xml[start..end], &["li"])
}

fn pdf_xmp_text(path: &Path) -> Option<String> {
    let doc = Document::load_with_options(path, LoadOptions::with_max_decompressed_size(8 * 1024 * 1024)).ok()?;
    let metadata = doc.catalog().ok()?.get(b"Metadata").ok()?;
    let (_, object) = doc.dereference(metadata).ok()?;
    let stream = object.as_stream().ok()?;
    let bytes = stream.decompressed_content_with_limit(512 * 1024).ok()?;
    String::from_utf8(bytes).ok()
}

fn pdf_first_page_text(path: &Path) -> Option<String> {
    let doc = Document::load_with_options(path, LoadOptions::with_max_decompressed_size(8 * 1024 * 1024)).ok()?;
    if doc.get_pages().is_empty() { return None; }
    doc.extract_text_with_limit(&[1], 512 * 1024).ok().filter(|text| !text.trim().is_empty())
}

fn first_page_title(text: &str) -> Option<String> {
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()).take(16) {
        let lower = line.to_ascii_lowercase();
        if line.len() < 12 || line.len() > 240 || lower.contains("doi") || lower.contains("abstract")
            || lower.contains("keywords") || lower.contains("received") || lower.contains("published")
            || lower.contains('@') || parse_year_metadata(Some(line)).is_some() { continue; }
        if line.split_whitespace().count() >= 2 { return clean_optional_text(Some(line)); }
    }
    None
}

fn first_page_authors(text: &str) -> Vec<crate::models::Author> {
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()).take(24) {
        let lower = line.to_ascii_lowercase();
        for prefix in ["authors:", "author:", "by:"] {
            if lower.starts_with(prefix) {
                return parse_author_metadata(Some(line[prefix.len()..].trim()));
            }
        }
    }
    Vec::new()
}

fn first_page_year(text: &str) -> Option<i32> {
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()).take(32) {
        let lower = line.to_ascii_lowercase();
        if ["published", "publication", "year", "copyright", "accepted"].iter().any(|word| lower.contains(word)) {
            if let Some(year) = parse_year_metadata(Some(line)) { return Some(year); }
        }
    }
    None
}

fn pdf_object_text(value: &lopdf::Object) -> Option<String> {
    match value {
        Object::String(bytes, _) => String::from_utf8(bytes.clone()).ok().and_then(|v| clean_optional_text(Some(&v))),
        Object::Name(bytes) => String::from_utf8(bytes.clone()).ok().and_then(|v| clean_optional_text(Some(&v))),
        _ => None,
    }
}

/// Parse PDF Info/XMP first, then bounded first-page text. File creation and
/// modification dates are intentionally excluded: they describe the PDF file,
/// not the publication year. Invalid/fixture PDFs retain the raw metadata
/// fallback used by the existing import path.
pub fn parse_external_pdf_metadata(path: &Path, filename: &str) -> Result<crate::models::ExternalPdfMetadata> {
    let bytes = std::fs::read(path).map_err(|_| rusqlite::Error::InvalidQuery)?;
    const PDF_TEXT_SCAN_LIMIT: usize = 1024 * 1024;
    let raw_text = String::from_utf8_lossy(&bytes);
    let bounded_text = String::from_utf8_lossy(&bytes[..bytes.len().min(PDF_TEXT_SCAN_LIMIT)]);
    let xmp = pdf_xmp_text(path).unwrap_or_default();
    let first_page = pdf_first_page_text(path).unwrap_or_default();
    let info = Document::load_metadata(path).ok();
    let xmp_title = xmp_element_values(&xmp, &["title"]).into_iter().next()
        .or_else(|| xml_metadata_value(&xmp, &["dc:title", "title"]));
    let xmp_author = xmp_element_values(&xmp, &["creator"]).into_iter().next()
        .or_else(|| xml_metadata_value(&xmp, &["dc:creator", "creator", "Author"]));
    let xmp_doi = xmp_element_values(&xmp, &["doi", "identifier"]).into_iter().find_map(|value| first_doi(Some(&value)))
        .or_else(|| xml_metadata_value(&xmp, &["prism:doi", "bibo:doi", "doi"]).and_then(|v| first_doi(Some(&v))));
    let title = info.as_ref().and_then(|value| value.title.clone())
        .or_else(|| pdf_info_value(&raw_text, "Title"))
        .or(xmp_title)
        .or_else(|| first_page_title(&first_page))
        .or_else(|| Path::new(filename).file_stem().and_then(|value| value.to_str()).map(str::to_string));
    let author_value = info.as_ref().and_then(|value| value.author.clone())
        .or_else(|| pdf_info_value(&raw_text, "Author"))
        .or(xmp_author);
    let doi = first_doi(info.as_ref().and_then(|value| value.custom.get(b"DOI".as_slice())).and_then(pdf_object_text).as_deref())
        .or_else(|| first_doi(pdf_info_value(&raw_text, "DOI").as_deref()))
        .or(xmp_doi)
        .or_else(|| first_doi(Some(&first_page)))
        .or_else(|| first_doi(Some(&bounded_text)));
    let scholarly_id = pdf_info_value(&raw_text, "OpenAlex")
        .or_else(|| pdf_info_value(&raw_text, "PMID"))
        .or_else(|| pdf_info_value(&raw_text, "PMCID"))
        .or_else(|| pdf_info_value(&raw_text, "arXiv"))
        .or_else(|| xml_metadata_value(&xmp, &["openalex", "pmid", "pmcid", "arXiv"]));
    let abstract_text = extract_structured_pdf_abstract(&first_page);
    let abstract_provenance = if abstract_text.is_some() { "pdf_structured" } else { "missing" }.to_string();
    let year = first_page_year(&first_page).or_else(|| parse_year_metadata(
        pdf_info_value(&raw_text, "Year")
            .or_else(|| pdf_info_value(&raw_text, "PublicationDate"))
            .or_else(|| xml_metadata_value(&xmp, &["prism:publicationDate", "dc:date"]))
            .as_deref(),
    ));
    let info_keywords = info
        .as_ref()
        .and_then(|value| value.keywords.clone())
        .or_else(|| pdf_info_value(&raw_text, "Keywords"));
    let mut keywords = parse_keyword_metadata(
        info_keywords.as_deref(),
        "publisher_keyword",
        "pdf_info",
        "Info.Keywords",
    );
    keywords.extend(parse_keyword_metadata(
        xml_metadata_value(&xmp, &["dc:subject"]).as_deref(),
        "subject",
        "pdf_xmp",
        "XMP.dc:subject",
    ));
    for (position, keyword) in xmp_container_list_values(&xmp, "dc:subject").into_iter().enumerate() {
        keywords.push(crate::models::PaperKeywordInput { keyword, kind: "subject".to_string(), source: "pdf_xmp".to_string(), confidence: "MEDIUM".to_string(), source_locator: Some("XMP.dc:subject".to_string()), language: None, position: Some(position as i64) });
    }
    let authors = parse_author_metadata(author_value.as_deref());
    Ok(crate::models::ExternalPdfMetadata {
        filename: filename.to_string(),
        abstract_provenance,
        title,
        authors: if authors.is_empty() { first_page_authors(&first_page) } else { authors },
        year,
        doi,
        scholarly_id: clean_optional_text(scholarly_id.as_deref()),
        abstract_text,
        keywords,
        ..Default::default()
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
    if !paper_exists(conn, paper_id)? {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    if let Some(existing) = list_paper_attachments(conn, paper_id)?.into_iter().find(|a| a.sha256.as_deref() == Some(&file.sha256)) {
        conn.execute(
            "INSERT INTO library_items(paper_id,added_at,added_source) VALUES(?1,?2,?3) ON CONFLICT(paper_id) DO NOTHING",
            params![paper_id, now_utc(), added_source],
        )?;
        return Ok(existing);
    }
    let prepared = prepare_current_pdf_storage(conn, paper_id, file)?;
    let tx = conn.unchecked_transaction()?;
    let now = now_utc();
    tx.execute(
        "INSERT INTO library_items (paper_id, added_at, added_source)
         VALUES (?1,?2,?3) ON CONFLICT(paper_id) DO NOTHING",
        params![paper_id, now, added_source],
    )?;
    tx.execute("UPDATE papers SET is_favorite=0, updated_at=?1 WHERE id=?2", params![now, paper_id])?;
    let id = match insert_attachment_row(&tx, paper_id, file, prepared.as_ref()) {
        Ok(id) => id,
        Err(error) => {
            if let Some(prepared) = prepared.as_ref() { cleanup_prepared_destination(prepared); }
            return Err(error);
        }
    };
    if let Err(error) = tx.commit() {
        if let Some(prepared) = prepared.as_ref() { cleanup_prepared_destination(prepared); }
        return Err(error);
    }
    if let Some(prepared) = prepared.as_ref() {
        finalize_prepared_storage(conn, prepared)?;
    }
    get_paper_attachment(conn, id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

/// Apply one exact-identity provider result to the local PDF metadata. Every
/// field is fill-only: an existing value shown to the user is never silently
/// replaced by a conflicting provider value.
pub(crate) fn merge_external_pdf_metadata_from_candidate(
    metadata: &mut crate::models::ExternalPdfMetadata,
    candidate: &PaperCandidate,
    source: &str,
) {
    if metadata.doi.as_deref().and_then(crate::util::normalize_doi) != candidate.normalized_doi
        || candidate.normalized_doi.is_none() { return; }
    let publication = publication_metadata(candidate);
    metadata.journal = metadata.journal.take().or(publication.journal);
    metadata.publisher = metadata.publisher.take().or(publication.publisher);
    metadata.publication_date = metadata.publication_date.take().or(publication.publication_date);
    metadata.volume = metadata.volume.take().or(publication.volume);
    metadata.issue = metadata.issue.take().or(publication.issue);
    metadata.pages = metadata.pages.take().or(publication.pages);
    if metadata.title.as_deref().map(|v| v.trim().is_empty()).unwrap_or(true) {
        metadata.title = candidate.title.clone();
    }
    if metadata.authors.is_empty() && !candidate.authors.is_empty() {
        metadata.authors = candidate.authors.clone();
    }
    if metadata.year.is_none() {
        metadata.year = candidate.year;
    }
    if metadata.doi.is_none() {
        metadata.doi = candidate.normalized_doi.clone();
    }
    if metadata.abstract_provenance != "provider" {
        if let Some(text) = candidate.abstract_text.as_deref().filter(|s| !s.trim().is_empty()) {
            metadata.abstract_text = Some(text.to_string());
            metadata.abstract_provenance = "provider".into();
        }
    }
    if metadata.scholarly_id.is_none() {
        metadata.scholarly_id = candidate
            .openalex_work_id
            .clone()
            .or_else(|| candidate.publisher_article_id.clone());
    }
    if let Some(raw_json) = candidate.raw_json.as_deref() {
        metadata.keywords.extend(keyword_inputs_from_provider_json(source, raw_json));
    }
}

/// Resolve metadata only after an exact DOI was extracted. Network failures
/// are non-fatal: the PDF's explicit local metadata remains importable.
fn external_provider_candidates(doi: &str) -> Vec<(String, PaperCandidate)> {
    #[cfg(test)]
    {
        let _ = doi;
        return Vec::new();
    }

    #[cfg(not(test))]
    {
        const MAILTO: &str = "dev@cowpaper.local";
        let crossref = crate::api::crossref::Crossref::new(MAILTO);
        let openalex = crate::api::openalex::OpenAlex::new(MAILTO);
        let mut out = Vec::new();
        if let Ok(Some(candidate)) = crossref.work_by_doi(doi) {
            out.push(("crossref".to_string(), candidate));
        }
        if let Ok(Some(candidate)) = openalex.work_by_doi(doi) {
            out.push(("openalex".to_string(), candidate));
        }
        out
    }
}

pub(crate) fn fill_missing_canonical_metadata_from_candidate(
    conn: &Connection,
    paper_id: i64,
    candidate: &PaperCandidate,
) -> Result<()> {
    let existing_doi: Option<String> = conn.query_row("SELECT normalized_doi FROM papers WHERE id=?1", params![paper_id], |r| r.get(0))?;
    if existing_doi.is_some() && candidate.normalized_doi != existing_doi { return Ok(()); }
    // This helper intentionally has no UPDATE path for an already populated
    // title/authors/year/source/DOI. It is a conservative exact-identity fill.
    fill_other_fields(conn, paper_id, candidate)?;
    let current_abstract: Option<String> = conn.query_row(
        "SELECT abstract FROM papers WHERE id=?1",
        params![paper_id],
        |row| row.get(0),
    )?;
    let provenance: String = conn.query_row("SELECT abstract_provenance FROM papers WHERE id=?1", params![paper_id], |r| r.get(0))?;
    let abstract_missing = provenance == "legacy_unverified" || current_abstract
        .as_deref()
        .map(|value| value.trim().is_empty())
        .unwrap_or(true);
    if abstract_missing {
        if let Some(text) = candidate.abstract_text.as_deref() {
            let normalized = crate::abstract_quality::normalize_abstract_text(text);
            if !normalized.trim().is_empty() && candidate.abstract_source.as_deref().is_some_and(is_provider_abstract_source) {
                if let Some(old) = current_abstract.as_deref() {
                    record_abstract_source(conn, paper_id, "legacy_unverified", old, "partial", "retained_before_exact_provider_refresh")?;
                }
                let (quality, _) = crate::abstract_quality::assess_abstract_quality(&normalized);
                let now = now_utc();
                conn.execute(
                    "UPDATE papers SET abstract=?1, abstract_source=?2,
                        abstract_quality=?3, abstract_retrieved_at=?4,
                        abstract_last_checked_at=?4,
                        updated_at=?4 WHERE id=?5",
                    params![normalized, candidate.abstract_source, quality, now, paper_id],
                )?;
                record_abstract_source(
                    conn,
                    paper_id,
                    candidate.abstract_source.as_deref().unwrap_or("provider"),
                    &normalized,
                    quality,
                    "exact_identity_provider_fill",
                )?;
            }
        }
    }
    update_abstract_provenance(conn, paper_id)?;
    refresh_abstract_status(conn, paper_id)?;
    Ok(())
}

fn persist_external_metadata(
    conn: &Connection,
    paper_id: i64,
    metadata: &crate::models::ExternalPdfMetadata,
    providers: &[(String, PaperCandidate)],
) -> Result<()> {
    persist_structured_pdf_abstract(conn, paper_id, metadata)?;
    let doi: Option<String> = conn.query_row("SELECT normalized_doi FROM papers WHERE id=?1",params![paper_id],|r| r.get(0))?;
    if doi == metadata.doi {
        if let Some(date) = metadata.publication_date.as_deref() {
            conn.execute("UPDATE papers SET published_date=?1,year=?2 WHERE id=?3 AND discovery_source='external_pdf_import'", params![date,crate::util::extract_year(date),paper_id])?;
        }
    }
    let pdf_record_id = insert_source_record(
        conn,
        paper_id,
        "pdf_metadata",
        Some(&metadata.filename),
        None,
    )?;
    insert_keyword_inputs(conn, paper_id, &metadata.keywords, Some(pdf_record_id))?;
    for (source, candidate) in providers {
        let doi: Option<String> = conn.query_row("SELECT normalized_doi FROM papers WHERE id=?1",params![paper_id],|r| r.get(0))?;
        if doi != candidate.normalized_doi { continue; }
        fill_publication_metadata(conn, paper_id, candidate)?;
        insert_source_record(
            conn,
            paper_id,
            source,
            candidate.source_id.as_deref(),
            candidate.raw_json.as_deref(),
        )?;
    }
    Ok(())
}

/// Import a local PDF into the canonical Paper graph using the fast-first
/// contract. Exact DOI and exact scholarly IDs merge immediately;
/// title+authors+year is a candidate only and remains pending until the caller
/// supplies explicit confirmation. Network enrichment is queued after the
/// local transaction and is never performed on this call's critical path.
pub fn import_external_pdf(
    conn: &Connection,
    path: &str,
    confirmed_paper_id: Option<i64>,
) -> Result<crate::models::ExternalPdfImportResult> {
    import_external_pdf_fast(conn, path, confirmed_paper_id)
}

/// Fast-first import path used by the Tauri command. Local PDF parsing and
/// the Paper/Library/Attachment transaction are completed before this returns;
/// provider calls are intentionally not performed here.
pub fn import_external_pdf_fast(
    conn: &Connection,
    path: &str,
    confirmed_paper_id: Option<i64>,
) -> Result<crate::models::ExternalPdfImportResult> {
    let file = linked_file(path)?;
    let mut result = import_prepared_external_pdf(conn, file, confirmed_paper_id, Vec::new())?;
    if let (Some(paper_id), Some(attachment), Some(doi)) = (
        result.paper_id,
        result.attachment.as_ref(),
        result.metadata.doi.as_deref(),
    ) {
        result.enrichment_status = enqueue_pdf_enrichment(conn, paper_id, attachment.id, doi)?;
    }
    Ok(result)
}

fn enqueue_pdf_enrichment(conn: &Connection, paper_id: i64, attachment_id: i64, doi: &str) -> Result<String> {
    let now = now_utc();
    conn.execute(
        "INSERT INTO pdf_enrichment_jobs(paper_id,attachment_id,doi,status,created_at,updated_at)
         VALUES(?1,?2,?3,'queued',?4,?4)
         ON CONFLICT(attachment_id) DO UPDATE SET
           paper_id=excluded.paper_id, doi=excluded.doi,
           status=CASE WHEN pdf_enrichment_jobs.status='completed' THEN 'completed' ELSE 'queued' END,
           error=CASE WHEN pdf_enrichment_jobs.status='completed' THEN pdf_enrichment_jobs.error ELSE NULL END,
           updated_at=excluded.updated_at",
        params![paper_id, attachment_id, doi, now],
    )?;
    conn.query_row(
        "SELECT status FROM pdf_enrichment_jobs WHERE attachment_id=?1",
        params![attachment_id],
        |r| r.get(0),
    )
}

/// Background exact-DOI enrichment. Network I/O happens without the SQLite
/// mutex held; all writes remain fill-only and are discarded for mismatched
/// provider identities.
pub fn run_pdf_enrichment<R: Runtime>(
    db: &Arc<Mutex<Connection>>,
    app: &AppHandle<R>,
    paper_id: i64,
    attachment_id: i64,
    doi: &str,
) {
    let emit = |event: &str, payload: serde_json::Value| {
        if let Err(error) = app.emit(event, payload) {
            eprintln!("pdf enrichment emit failed: event={event}; error={error}");
        }
    };
    {
        let Ok(conn) = db.lock() else { return; };
        let claimed = conn.execute(
            "UPDATE pdf_enrichment_jobs SET status='running', updated_at=?1
             WHERE attachment_id=?2 AND status='queued'",
            params![now_utc(), attachment_id],
        ).unwrap_or(0);
        if claimed != 1 { return; }
    }
    emit("pdf://enrichment-started", serde_json::json!({"paperId": paper_id, "attachmentId": attachment_id}));
    let providers = external_provider_candidates(doi);
    emit("pdf://enrichment-progress", serde_json::json!({"paperId": paper_id, "attachmentId": attachment_id, "stage": "providersFetched", "providerCount": providers.len()}));
    let write_result = (|| -> Result<usize> {
        let conn = db.lock().map_err(|_| rusqlite::Error::InvalidQuery)?;
        let current: Option<String> = conn.query_row(
            "SELECT normalized_doi FROM papers WHERE id=?1 AND EXISTS(SELECT 1 FROM paper_attachments WHERE id=?2 AND paper_id=?1)",
            params![paper_id, attachment_id],
            |r| r.get(0),
        ).optional()?;
        if current.as_deref() != Some(doi) {
            return Err(rusqlite::Error::InvalidParameterName("pdf_enrichment_doi_mismatch".into()));
        }
        let mut enriched = 0;
        for (source, candidate) in &providers {
            if candidate.normalized_doi.as_deref() != Some(doi) { continue; }
            fill_missing_canonical_metadata_from_candidate(&conn, paper_id, candidate)?;
            insert_source_record(&conn, paper_id, source, candidate.source_id.as_deref(), candidate.raw_json.as_deref())?;
            enriched += 1;
        }
        conn.execute(
            "UPDATE pdf_enrichment_jobs SET status='completed', error=NULL, updated_at=?1 WHERE attachment_id=?2",
            params![now_utc(), attachment_id],
        )?;
        Ok(enriched)
    })();
    match write_result {
        Ok(enriched) => emit("pdf://enrichment-completed", serde_json::json!({"paperId": paper_id, "attachmentId": attachment_id, "providerCount": enriched})),
        Err(error) => {
            let message = error.to_string();
            if let Ok(conn) = db.lock() {
                let _ = conn.execute(
                    "UPDATE pdf_enrichment_jobs SET status='failed', error=?1, updated_at=?2 WHERE attachment_id=?3",
                    params![message, now_utc(), attachment_id],
                );
            }
            emit("pdf://enrichment-failed", serde_json::json!({"paperId": paper_id, "attachmentId": attachment_id, "error": message}));
        }
    }
}

#[cfg(test)]
pub(crate) fn import_external_pdf_with_candidates(conn: &Connection, path: &str, confirmed_paper_id: Option<i64>, providers: Vec<(String, PaperCandidate)>) -> Result<crate::models::ExternalPdfImportResult> {
    import_prepared_external_pdf(conn, linked_file(path)?, confirmed_paper_id, providers)
}

fn import_prepared_external_pdf(conn: &Connection, file: LinkedFile, confirmed_paper_id: Option<i64>, providers: Vec<(String, PaperCandidate)>) -> Result<crate::models::ExternalPdfImportResult> {
    let mut metadata = file.metadata.clone();
    let mut providers: Vec<_> = providers.into_iter().filter(|(_, c)| c.normalized_doi.is_some() && c.normalized_doi == metadata.doi).collect();
    providers.sort_by_key(|(source,_)| if source == "crossref" { 0 } else { 1 });
    for (source, candidate) in &providers {
        merge_external_pdf_metadata_from_candidate(&mut metadata, candidate, source);
    }

    let same_file: Option<(i64, Option<String>)> = conn.query_row(
        "SELECT p.id,p.normalized_doi FROM paper_attachments a JOIN papers p ON p.id=a.paper_id WHERE a.sha256=?1 ORDER BY a.id LIMIT 1",
        params![file.sha256], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
    if let Some((paper_id, old_doi)) = same_file {
        if let Some(doi) = metadata.doi.as_deref() {
            let owner: Option<i64> = conn.query_row("SELECT id FROM papers WHERE normalized_doi=?1", params![doi], |r| r.get(0)).optional()?;
            if owner.is_some_and(|id| id != paper_id) { return Err(rusqlite::Error::InvalidParameterName("doi_conflicts_with_existing_paper_manual_review_required".into())); }
            // A same-file reimport is not permission to repair or replace a
            // canonical DOI. Any disagreement is conservative manual review;
            // only the already-established exact DOI may be enriched.
            if old_doi.as_deref() != Some(doi) {
                return Err(rusqlite::Error::InvalidParameterName("pdf_identity_conflict_manual_review_required".into()));
            }
        }
        for (_, candidate) in &providers { fill_missing_canonical_metadata_from_candidate(conn, paper_id, candidate)?; }
        persist_external_metadata(conn, paper_id, &metadata, &providers)?;
        conn.execute("INSERT INTO library_items(paper_id,added_at,added_source) VALUES(?1,?2,'external_pdf_import') ON CONFLICT(paper_id) DO NOTHING",params![paper_id,now_utc()])?;
        let attachment = list_paper_attachments(conn, paper_id)?.into_iter().find(|a| a.sha256.as_deref() == Some(&file.sha256));
        return Ok(crate::models::ExternalPdfImportResult { outcome: "existingAttachmentRefreshed".into(), paper_id: Some(paper_id), attachment, metadata, candidate: None, candidates: vec![], requires_confirmation: false, enrichment_status: "ready".into(), enrichment_error: None });
    }

    if let Some(doi) = metadata.doi.as_deref() {
        if let Some(paper_id) = conn
            .query_row("SELECT id FROM papers WHERE normalized_doi=?1", params![doi], |row| row.get(0))
            .optional()?
        {
            for (_, candidate) in &providers {
                fill_missing_canonical_metadata_from_candidate(conn, paper_id, candidate)?;
            }
            let attachment = add_library_and_attach(conn, paper_id, &file, "external_pdf_import")?;
            persist_external_metadata(conn, paper_id, &metadata, &providers)?;
            return Ok(crate::models::ExternalPdfImportResult {
                outcome: "existingDoi".to_string(),
                paper_id: Some(paper_id),
                attachment: Some(attachment),
                metadata,
                candidate: None,
                candidates: Vec::new(),
                requires_confirmation: false,
                enrichment_status: "ready".into(),
                enrichment_error: None,
            });
        }
    }

    if let Some(scholarly_id) = metadata.scholarly_id.as_deref() {
        if let Some(paper_id) = find_paper_by_exact_scholarly_id(conn, scholarly_id)? {
            for (_, candidate) in &providers {
                fill_missing_canonical_metadata_from_candidate(conn, paper_id, candidate)?;
            }
            let attachment = add_library_and_attach(conn, paper_id, &file, "external_pdf_import")?;
            persist_external_metadata(conn, paper_id, &metadata, &providers)?;
            return Ok(crate::models::ExternalPdfImportResult {
                outcome: "existingScholarlyId".to_string(),
                paper_id: Some(paper_id),
                attachment: Some(attachment),
                metadata,
                candidate: None,
                candidates: Vec::new(),
                requires_confirmation: false,
                enrichment_status: "ready".into(),
                enrichment_error: None,
            });
        }
    }

    let candidates = title_author_year_candidates(conn, &metadata)?;
    if let Some(paper_id) = confirmed_paper_id {
        if !paper_exists(conn, paper_id)? {
            return Err(rusqlite::Error::QueryReturnedNoRows);
        }
        for (_, candidate) in &providers {
            fill_missing_canonical_metadata_from_candidate(conn, paper_id, candidate)?;
        }
        let attachment = add_library_and_attach(conn, paper_id, &file, "external_pdf_manual_confirmation")?;
        persist_external_metadata(conn, paper_id, &metadata, &providers)?;
        return Ok(crate::models::ExternalPdfImportResult {
            outcome: "manualConfirmation".to_string(),
            paper_id: Some(paper_id),
            attachment: Some(attachment),
            metadata,
            candidate: candidates.iter().find(|candidate| candidate.paper_id == paper_id).cloned(),
            candidates,
            requires_confirmation: false,
            enrichment_status: "ready".into(),
            enrichment_error: None,
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
            enrichment_status: "ready".into(),
            enrichment_error: None,
        });
    }

    let journal_id = ensure_external_pdf_journal(conn)?;
    let doi = metadata.doi.clone();
    let abstract_source = metadata.abstract_text.as_ref().map(|_| {
        if metadata.abstract_provenance == "provider" {
            providers.iter().find_map(|(_, c)| c.abstract_text.as_ref().filter(|t| Some(*t) == metadata.abstract_text.as_ref()).and(c.abstract_source.clone())).unwrap_or_else(|| "provider".into())
        } else { "pdf_structured".into() }
    });
    let candidate = crate::models::PaperCandidate {
        normalized_doi: doi.clone(),
        original_doi: doi.clone(),
        title: metadata.title.clone(),
        authors: metadata.authors.clone(),
        published_date: metadata.publication_date.clone(),
        year: metadata.year,
        abstract_text: metadata.abstract_text.clone(),
        abstract_source,
        abstract_source_url: None,
        url: doi.as_deref().map(|doi| format!("https://doi.org/{}", doi)),
        publisher_article_id: metadata.scholarly_id.clone(),
        openalex_work_id: None,
        discovery_source: "external_pdf_import".to_string(),
        source_id: doi,
        raw_json: providers.first().and_then(|(_, candidate)| candidate.raw_json.clone()),
    };
    let paper_id = insert_paper_without_identity_merge(conn, journal_id, &candidate)?;
    let attachment = add_library_and_attach(conn, paper_id, &file, "external_pdf_import")?;
    persist_external_metadata(conn, paper_id, &metadata, &providers)?;
    Ok(crate::models::ExternalPdfImportResult {
        outcome: "createdExternalPaper".to_string(),
        paper_id: Some(paper_id),
        attachment: Some(attachment),
        metadata,
        candidate: None,
        candidates: Vec::new(),
        requires_confirmation: false,
        enrichment_status: "ready".into(),
        enrichment_error: None,
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
    list_library_papers_scoped(conn, view, None, &[], limit)
}

/// List Library papers with backend-owned scope semantics. A collection is
/// resolved through `library_collection_items`; every requested tag gets its
/// own EXISTS predicate, therefore multiple tags are an AND filter rather
/// than a client-side OR.
pub fn list_library_papers_scoped(
    conn: &Connection,
    view: &str,
    collection_id: Option<i64>,
    tag_ids: &[i64],
    limit: i64,
) -> Result<Vec<crate::models::LibraryPaper>> {
    let order = match view {
        "recent" => "li.added_at DESC, p.id DESC",
        "all" | "unfiled" => "COALESCE(p.published_date, p.created_at) DESC, p.id DESC",
        _ => return Err(rusqlite::Error::InvalidParameterName("view".into())),
    };
    validate_library_tag_ids(conn, tag_ids)?;
    if let Some(collection_id) = collection_id {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM library_collections WHERE id=?1)",
            params![collection_id],
            |r| r.get(0),
        )?;
        if !exists { return Err(rusqlite::Error::QueryReturnedNoRows); }
    }
    let mut sql = format!(
        "SELECT p.*, COALESCE(NULLIF(trim(p.container_title), ''), j.name) AS journal_name FROM papers p
         JOIN journals j ON j.id = p.journal_id
         JOIN library_items li ON li.paper_id = p.id
         WHERE 1=1"
    );
    let mut args: Vec<rusqlite::types::Value> = Vec::new();
    if view == "unfiled" {
        sql.push_str(" AND NOT EXISTS (SELECT 1 FROM library_collection_items ci WHERE ci.paper_id=p.id)");
    }
    if let Some(collection_id) = collection_id {
        args.push(rusqlite::types::Value::Integer(collection_id));
        let n = args.len();
        sql.push_str(&format!(" AND EXISTS (SELECT 1 FROM library_collection_items ci WHERE ci.paper_id=p.id AND ci.collection_id=?{n})"));
    }
    for tag_id in tag_ids {
        args.push(rusqlite::types::Value::Integer(*tag_id));
        let n = args.len();
        sql.push_str(&format!(" AND EXISTS (SELECT 1 FROM library_item_tags lit WHERE lit.paper_id=p.id AND lit.tag_id=?{n})"));
    }
    let limit_placeholder = args.len() + 1;
    args.push(rusqlite::types::Value::Integer(limit));
    sql.push_str(&format!(" ORDER BY {order} LIMIT ?{limit_placeholder}"));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args.iter()), row_to_paper)?;
    let mut papers = rows.collect::<Result<Vec<_>>>()?;
    enrich_papers_collections(conn, &mut papers)?;
    enrich_papers_keywords(conn, &mut papers)?;
    filter_current_tag_matches(conn, &mut papers)?;
    papers.into_iter().map(|p| library_paper(conn, p)).collect()
}

pub fn get_library_paper(conn: &Connection, paper_id: i64) -> Result<Option<crate::models::LibraryPaper>> {
    let paper = conn
        .query_row(
            "SELECT p.*, COALESCE(NULLIF(trim(p.container_title), ''), j.name) AS journal_name FROM papers p
             JOIN journals j ON j.id = p.journal_id
             JOIN library_items li ON li.paper_id = p.id WHERE p.id = ?1",
            params![paper_id],
            row_to_paper,
        )
        .optional()?;
    let Some(mut paper) = paper else { return Ok(None); };
    enrich_papers_collections(conn, std::slice::from_mut(&mut paper))?;
    enrich_papers_keywords(conn, std::slice::from_mut(&mut paper))?;
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
    let has_children: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM library_collections WHERE parent_id=?1)", params![id], |r| r.get(0))?;
    if has_children { return Err(rusqlite::Error::InvalidParameterName("collection_has_children".into())); }
    let changed = tx.execute("DELETE FROM library_collections WHERE id = ?1", params![id])?;
    tx.commit()?;
    Ok(changed == 1)
}

pub fn list_library_tags(conn: &Connection) -> Result<Vec<crate::models::LibraryTag>> {
    let mut stmt = conn.prepare("SELECT * FROM library_tags ORDER BY name, id")?;
    let rows = stmt.query_map([], library_tag_from_row)?;
    rows.collect()
}

pub fn list_library_tag_facets(conn: &Connection, collection_id: i64) -> Result<Vec<crate::models::LibraryTagFacet>> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM library_collections WHERE id=?1)",
        params![collection_id],
        |r| r.get(0),
    )?;
    if !exists { return Err(rusqlite::Error::QueryReturnedNoRows); }
    let mut stmt = conn.prepare(
        "SELECT t.*, COUNT(DISTINCT lci.paper_id) AS paper_count
         FROM library_tags t
         LEFT JOIN library_item_tags lit ON lit.tag_id=t.id
         LEFT JOIN library_items li ON li.paper_id=lit.paper_id
         JOIN library_collection_items lci
           ON lci.paper_id=li.paper_id AND lci.collection_id=?1
         GROUP BY t.id
         ORDER BY t.name, t.id",
    )?;
    let rows = stmt.query_map(params![collection_id], |row| {
        Ok(crate::models::LibraryTagFacet {
            tag: library_tag_from_row(row)?,
            paper_count: row.get("paper_count")?,
        })
    })?;
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
        (16, "bibliographic-keywords", migrate_to_v16),
        (17, "Bibliographic Publication Metadata", migrate_to_v17),
        (18, "library-rc5-overrides-scoped-tags-pdf-enrichment", migrate_to_v18),
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

/// v16: canonical bibliographic keywords. This relation is intentionally
/// independent from Library Tags and Research Tags and never participates in
/// recommendation scoring. The source/provenance columns make it possible for
/// the UI to distinguish explicit publisher keywords from provider subjects or
/// OpenAlex concepts.
fn migrate_to_v16(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS paper_keywords (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            paper_id INTEGER NOT NULL REFERENCES papers(id) ON DELETE CASCADE,
            keyword TEXT NOT NULL,
            normalized_keyword TEXT NOT NULL,
            kind TEXT NOT NULL CHECK (kind IN ('author_keyword', 'publisher_keyword', 'subject', 'concept')),
            source TEXT NOT NULL,
            confidence TEXT NOT NULL,
            source_locator TEXT,
            source_record_id INTEGER REFERENCES source_records(id) ON DELETE SET NULL,
            language TEXT,
            position INTEGER,
            retrieved_at TEXT NOT NULL,
            created_at TEXT NOT NULL,
            UNIQUE (paper_id, normalized_keyword, kind, source)
        );
        CREATE INDEX IF NOT EXISTS idx_paper_keywords_paper
            ON paper_keywords(paper_id, kind, position, id);
        CREATE INDEX IF NOT EXISTS idx_paper_keywords_normalized
            ON paper_keywords(normalized_keyword, kind);
        CREATE INDEX IF NOT EXISTS idx_paper_keywords_source_record
            ON paper_keywords(source_record_id);
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
        "SELECT p.*, COALESCE(NULLIF(trim(p.container_title), ''), j.name) AS journal_name FROM papers p JOIN journals j ON j.id = p.journal_id \
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

/// v17 adds publication fields without rewriting any earlier migration or
/// inferring publisher from the journal name. Historical PDF abstracts remain
/// stored for audit; absence of structured evidence is never treated as trust.
fn migrate_to_v17(conn: &Connection) -> Result<()> {
    for field in ["container_title", "publisher", "volume", "issue", "pages"] {
        if !column_exists(conn,"papers",field) { conn.execute_batch(&format!("ALTER TABLE papers ADD COLUMN {field} TEXT;"))?; }
    }
    for (table,field,definition) in [("papers","abstract_provenance","TEXT NOT NULL DEFAULT 'missing'"), ("papers","legacy_abstract_unverified","INTEGER NOT NULL DEFAULT 0"), ("library_item_metadata","chinese_abstract_source_hash","TEXT")] {
        if !column_exists(conn,table,field) { conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {field} {definition};"))?; }
    }
    for field in ["journal", "publisher", "publication_date", "volume", "issue", "pages"] {
        if !column_exists(conn,"library_item_metadata",&format!("{field}_override")) { conn.execute_batch(&format!("ALTER TABLE library_item_metadata ADD COLUMN {field}_override TEXT;"))?; }
    }
    conn.execute_batch("UPDATE papers SET abstract_provenance=CASE
        WHEN abstract IS NULL OR trim(abstract)='' THEN 'missing'
        WHEN abstract_source IN ('crossref','openalex','provider') OR abstract_source LIKE 'publisher%' THEN 'provider'
        WHEN abstract_source='pdf_structured' THEN 'pdf_structured'
        ELSE 'legacy_unverified' END;
        UPDATE papers SET legacy_abstract_unverified=1 WHERE abstract_provenance='legacy_unverified';")?;
    Ok(())
}

/// v18: RC5 Library-only identity overrides, scoped tags, and durable PDF
/// enrichment state. Every field is additive and backfills to the old
/// semantics; canonical DOI/URL values are never rewritten by this migration.
fn migrate_to_v18(conn: &Connection) -> Result<()> {
    for (field, definition) in [
        ("doi_override", "TEXT"),
        ("url_override", "TEXT"),
    ] {
        if !column_exists(conn, "library_item_metadata", field) {
            conn.execute_batch(&format!("ALTER TABLE library_item_metadata ADD COLUMN {field} {definition};"))?;
        }
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS pdf_enrichment_jobs (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             paper_id INTEGER NOT NULL REFERENCES papers(id) ON DELETE CASCADE,
             attachment_id INTEGER NOT NULL REFERENCES paper_attachments(id) ON DELETE CASCADE,
             doi TEXT NOT NULL,
             status TEXT NOT NULL CHECK (status IN ('queued','running','completed','failed')),
             error TEXT,
             created_at TEXT NOT NULL,
             updated_at TEXT NOT NULL,
             UNIQUE(attachment_id)
         );
         CREATE INDEX IF NOT EXISTS idx_pdf_enrichment_jobs_status ON pdf_enrichment_jobs(status, id);",
    )?;
    Ok(())
}

fn update_abstract_provenance(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("UPDATE papers SET abstract_provenance=CASE
        WHEN abstract IS NULL OR trim(abstract)='' THEN 'missing'
        WHEN abstract_source IN ('crossref','openalex','provider') OR abstract_source LIKE 'publisher%' THEN 'provider'
        WHEN abstract_source='pdf_structured' THEN 'pdf_structured'
        ELSE 'legacy_unverified' END WHERE id=?1", params![id])?;
    Ok(())
}

/// Shared mapping from retained structured provider evidence; journal and
/// publisher are separate concepts. `published_date` is the existing canonical
/// publication-date column. Pages deliberately remains a string (e.g. e1234).
fn publication_metadata(c: &PaperCandidate) -> crate::models::ExternalPdfMetadata {
    let mut m = crate::models::ExternalPdfMetadata::default();
    m.publication_date = c.published_date.clone();
    let Some(v) = c.raw_json.as_deref().and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok()) else { return m; };
    let string = |v: Option<&serde_json::Value>| v.and_then(|v| v.as_str().map(str::to_string).or_else(|| v.as_i64().map(|n| n.to_string()))).and_then(|v| clean_optional_text(Some(&v)));
    if v.get("DOI").is_some() {
        m.journal = string(v.get("container-title").and_then(|v| v.as_array()).and_then(|v| v.first()));
        m.publisher = string(v.get("publisher"));
        m.volume = string(v.get("volume"));
        m.issue = string(v.get("issue"));
        m.pages = string(v.get("page")).or_else(|| string(v.get("article-number")));
    } else if v.get("primary_location").is_some() || v.get("biblio").is_some() {
        m.journal = string(v.pointer("/primary_location/source/display_name"));
        m.publication_date = string(v.get("publication_date")).or(m.publication_date);
        m.volume = string(v.pointer("/biblio/volume"));
        m.issue = string(v.pointer("/biblio/issue"));
        let first = string(v.pointer("/biblio/first_page"));
        let last = string(v.pointer("/biblio/last_page"));
        m.pages = match (first, last) {
            (Some(a), Some(b)) if a != b => Some(format!("{a}-{b}")),
            (Some(a), _) => Some(a),
            _ => None,
        };
    }
    m
}

fn fill_publication_metadata(conn: &Connection, id: i64, c: &PaperCandidate) -> Result<()> {
    let m = publication_metadata(c);
    conn.execute("UPDATE papers SET container_title=COALESCE(container_title,?1),
        publisher=COALESCE(publisher,?2), published_date=COALESCE(published_date,?3),
        volume=COALESCE(volume,?4), issue=COALESCE(issue,?5), pages=COALESCE(pages,?6)
        WHERE id=?7", params![m.journal,m.publisher,m.publication_date,m.volume,m.issue,m.pages,id])?;
    Ok(())
}

pub fn add_paper_to_collection(conn: &Connection, paper_id: i64, collection_id: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    if !library_item_exists(&tx, paper_id)? { return Err(rusqlite::Error::QueryReturnedNoRows); }
    validate_collection_ids(&tx, &[collection_id])?;
    tx.execute("INSERT OR IGNORE INTO library_collection_items(collection_id,paper_id,added_at) VALUES(?1,?2,?3)", params![collection_id,paper_id,now_utc()])?;
    tx.commit()
}

pub fn add_paper_library_tag(conn: &Connection, paper_id: i64, tag_id: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    if !library_item_exists(&tx, paper_id)? { return Err(rusqlite::Error::QueryReturnedNoRows); }
    validate_library_tag_ids(&tx, &[tag_id])?;
    tx.execute("INSERT OR IGNORE INTO library_item_tags(paper_id,tag_id,added_at) VALUES(?1,?2,?3)", params![paper_id,tag_id,now_utc()])?;
    tx.commit()
}

/// Patch only the translated personal field, avoiding lost concurrent edits to
/// other overrides while an API call is in flight. No canonical/analysis writes.
pub fn set_library_translation(conn: &Connection, paper_id: i64, translated: &str, title: bool) -> Result<crate::models::LibraryItemMetadata> {
    let tx = conn.unchecked_transaction()?;
    if !library_item_exists(&tx, paper_id)? { return Err(rusqlite::Error::QueryReturnedNoRows); }
    let source_hash = if title { None } else { get_library_paper(&tx,paper_id)?.and_then(|p| p.effective_abstract).map(|s| abstract_text_hash(&s)) };
    let field = if title { "chinese_title_override" } else { "chinese_abstract_override" };
    tx.execute(&format!("INSERT INTO library_item_metadata(paper_id,{field},updated_at) VALUES(?1,?2,?3)
        ON CONFLICT(paper_id) DO UPDATE SET {field}=excluded.{field},updated_at=excluded.updated_at"),
        params![paper_id,clean_optional_text(Some(translated)),now_utc()])?;
    if !title { tx.execute("UPDATE library_item_metadata SET chinese_abstract_source_hash=?1 WHERE paper_id=?2",params![source_hash,paper_id])?; }
    tx.commit()?;
    get_library_item_metadata(conn, paper_id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

fn abstract_text_hash(text: &str) -> String { format!("{:x}", Sha256::digest(text.as_bytes())) }
fn is_provider_abstract_source(source: &str) -> bool { matches!(source, "crossref" | "openalex" | "provider") || source.starts_with("publisher") }

/// Translation is allowed only for a real English abstract. This is a
/// conservative language gate; it is never used to create or overwrite a
/// canonical abstract.
pub(crate) fn is_english_abstract(text: &str) -> bool {
    let letters = text.chars().filter(|c| c.is_alphabetic()).count();
    let ascii = text.chars().filter(|c| c.is_ascii_alphabetic()).count();
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphabetic()).to_ascii_lowercase())
        .collect();
    let signals = ["the", "we", "this", "that", "with", "from", "and", "of", "in", "to"]
        .iter()
        .filter(|word| words.iter().any(|w| w == **word))
        .count();
    letters >= 40 && ascii * 10 >= letters * 9 && signals >= 3
}

/// Accept a labeled, terminated section only. Fail closed for mixed columns,
/// metadata, first-page paragraphs, incomplete sections, and citation snippets.
pub(crate) fn extract_structured_pdf_abstract(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let mut start = None;
    for (i, _) in lower.match_indices("abstract") {
        let prefix = &text[..i];
        if !prefix.rsplit('\n').next().unwrap_or("").trim().is_empty() { continue; }
        let tail = &text[i + 8..];
        if tail.starts_with(['.', ':', '\n', '\r']) || (prefix.rsplit('\n').next().unwrap_or("").trim().is_empty() && tail.starts_with(' ')) {
            start = Some(i + 8); break;
        }
    }
    let start = start?;
    let body = text[start..].trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '.' | ':' | '—'));
    let lower = body.to_ascii_lowercase();
    let end = ["keywords", "key words", "1. introduction", "1 introduction", "\nintroduction", "history:", "funding:", "supplemental material:"]
        .iter().filter_map(|label| lower.find(label)).min()?;
    let body = body[..end].trim();
    let words = body.split_whitespace().count();
    if !(40..=800).contains(&words) || body.len() > 8000 { return None; }
    let lower = body.to_ascii_lowercase();
    if ["copyright", "https://", "received:", "revised:", "accepted:", "issn", "management science 20", "references"].iter().any(|label| lower.contains(label)) { return None; }
    Some(body.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn persist_structured_pdf_abstract(conn: &Connection, id: i64, metadata: &crate::models::ExternalPdfMetadata) -> Result<()> {
    if metadata.abstract_provenance != "pdf_structured" { return Ok(()); }
    let Some(text) = metadata.abstract_text.as_deref() else { return Ok(()); };
    let (old, provenance): (Option<String>, String) = conn.query_row("SELECT abstract,abstract_provenance FROM papers WHERE id=?1",params![id],|r|Ok((r.get(0)?,r.get(1)?)))?;
    if !matches!(provenance.as_str(), "missing" | "legacy_unverified") { return Ok(()); }
    if let Some(old) = old { record_abstract_source(conn,id,"legacy_unverified",&old,"partial","retained_before_structured_pdf_refresh")?; }
    let (quality, reason) = crate::abstract_quality::assess_abstract_quality(text);
    conn.execute("UPDATE papers SET abstract=?1,abstract_source='pdf_structured',abstract_provenance='pdf_structured',abstract_quality=?2 WHERE id=?3",params![text,quality,id])?;
    record_abstract_source(conn,id,"pdf_structured",text,quality,reason)?;
    refresh_abstract_status(conn,id)
}

#[cfg(test)]
pub(crate) fn init_test_schema_at_version(conn: &Connection, version: i64) -> Result<()> {
    conn.execute_batch(SCHEMA)?;
    for (v, _, up) in migrations().into_iter().filter(|(v,_,_)| *v <= version) {
        up(conn)?;
        conn.pragma_update(None,"user_version",v)?;
    }
    Ok(())
}

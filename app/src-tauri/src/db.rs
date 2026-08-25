use rusqlite::{params, Connection, OptionalExtension, Result};
use std::path::Path;

use crate::models::{
    Author, Journal, Paper, PaperCandidate, Tag, TagMatch, UpsertOutcome, ST_PENDING,
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

/// 当前 schema 版本（下一轮 Batch 表将从 v2 开始递增）。
/// 生产构建中仅由迁移系统隐式使用；测试中直接断言。
#[allow(dead_code)]
pub const SCHEMA_VERSION: i64 = 1;

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
    })
}

pub fn list_journals(conn: &Connection) -> Result<Vec<Journal>> {
    let mut stmt = conn.prepare(
        "SELECT j.*, (SELECT COUNT(*) FROM papers p WHERE p.journal_id = j.id) AS paper_count
         FROM journals j ORDER BY j.enabled DESC, j.priority DESC, j.name ASC",
    )?;
    let rows = stmt.query_map([], row_to_journal)?;
    rows.collect()
}

pub fn get_journal(conn: &Connection, id: i64) -> Result<Option<Journal>> {
    conn.query_row(
        "SELECT j.*, (SELECT COUNT(*) FROM papers p WHERE p.journal_id = j.id) AS paper_count
         FROM journals j WHERE j.id = ?1",
        params![id],
        row_to_journal,
    )
    .optional()
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

/// 补齐已有论文的缺失字段（只填空，不覆盖非空，满足 §8.3 来源优先级原则）。
fn fill_missing_fields(conn: &Connection, id: i64, c: &PaperCandidate) -> Result<bool> {
    let mut abstract_filled = false;

    let current_abstract: Option<String> = conn
        .query_row("SELECT abstract FROM papers WHERE id = ?1", params![id], |r| r.get(0))
        .optional()?
        .flatten();
    if current_abstract.is_none() && c.abstract_text.is_some() {
        conn.execute(
            "UPDATE papers SET abstract = ?1, abstract_source = ?2, abstract_retrieved_at = ?3, updated_at = ?4 WHERE id = ?5",
            params![c.abstract_text, c.abstract_source, now_utc(), now_utc(), id],
        )?;
        conn.execute(
            "UPDATE papers SET analysis_status = 'pendingAnalysis' WHERE id = ?1 AND analysis_status = 'waitingForAbstract'",
            params![id],
        )?;
        abstract_filled = true;
    }

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

    Ok(abstract_filled)
}

pub fn upsert_paper(conn: &Connection, journal_id: i64, c: &PaperCandidate) -> Result<UpsertOutcome> {
    if let Some(existing_id) = find_paper_id(conn, journal_id, c)? {
        let abstract_filled = fill_missing_fields(conn, existing_id, c)?;
        return Ok(UpsertOutcome::Existing {
            id: existing_id,
            abstract_filled,
        });
    }

    let authors_json = serde_json::to_string(&c.authors).unwrap_or_else(|_| "[]".to_string());
    let title_norm = c.title.as_deref().map(normalize_title);
    let analysis_status = if c.abstract_text.is_some() {
        ST_PENDING
    } else {
        ST_WAITING_ABSTRACT
    };
    let now = now_utc();

    conn.execute(
        "INSERT INTO papers (
            journal_id, normalized_doi, original_doi, title, title_norm, authors_json,
            published_date, year, abstract, abstract_source, abstract_retrieved_at,
            url, publisher_article_id, openalex_work_id, discovery_source,
            analysis_status, created_at, updated_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?17)",
        params![
            journal_id,
            c.normalized_doi,
            c.original_doi,
            c.title,
            title_norm,
            authors_json,
            c.published_date,
            c.year,
            c.abstract_text,
            c.abstract_source,
            c.abstract_text.as_ref().map(|_| now.clone()),
            c.url,
            c.publisher_article_id,
            c.openalex_work_id,
            c.discovery_source,
            analysis_status,
            now
        ],
    )?;
    let id = conn.last_insert_rowid();
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
    rows.collect()
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
    vec![(1, "round3-baseline", migrate_to_v1)]
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
pub fn get_paper_title_abstract(conn: &Connection, id: i64) -> Result<Option<(String, String)>> {
    conn.query_row(
        "SELECT COALESCE(title,''), COALESCE(abstract,'') FROM papers WHERE id=?1",
        params![id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
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

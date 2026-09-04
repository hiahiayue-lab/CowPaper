//! Temporary, standalone Library Search spike.
//!
//! This example uses only an in-memory SQLite database and synthetic rows.
//! It must not be used by the application runtime or migration system.

use rusqlite::{params, Connection, Result};
use std::time::Instant;

fn han(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

fn cjk_bigrams(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut run = Vec::new();
    let flush = |run: &mut Vec<char>, out: &mut Vec<String>| {
        if run.len() >= 2 {
            for pair in run.windows(2) {
                out.push(pair.iter().collect());
            }
        }
        run.clear();
    };
    for c in chars {
        if han(c) {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
        }
    }
    flush(&mut run, &mut out);
    out.join(" ")
}

fn search_query(input: &str) -> String {
    let mut terms = Vec::new();
    for raw in input.split_whitespace() {
        let cjk: Vec<char> = raw.chars().filter(|c| han(*c)).collect();
        let latin: String = raw.chars().filter(|c| !han(*c)).collect();
        if cjk.len() >= 2 {
            terms.extend(cjk.windows(2).map(|pair| pair.iter().collect::<String>()));
        }
        if !latin.is_empty() {
            terms.push(latin);
        }
    }
    terms
        .into_iter()
        .map(|term| format!("\"{}\"", term.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE papers (
            id INTEGER PRIMARY KEY,
            title TEXT,
            chinese_title TEXT,
            abstract TEXT,
            year INTEGER,
            discovery_source TEXT
        );
        CREATE TABLE library_items (paper_id INTEGER PRIMARY KEY REFERENCES papers(id));
        CREATE TABLE library_collections (
            id INTEGER PRIMARY KEY,
            parent_id INTEGER REFERENCES library_collections(id),
            name TEXT NOT NULL
        );
        CREATE TABLE library_collection_items (
            collection_id INTEGER NOT NULL,
            paper_id INTEGER NOT NULL,
            PRIMARY KEY(collection_id, paper_id)
        );
        CREATE TABLE library_tags (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
        CREATE TABLE library_item_tags (
            paper_id INTEGER NOT NULL,
            tag_id INTEGER NOT NULL,
            PRIMARY KEY(paper_id, tag_id)
        );
        CREATE TABLE library_item_overrides (
            paper_id INTEGER PRIMARY KEY,
            title_override TEXT,
            note TEXT
        );
        CREATE TABLE library_item_annotations (
            id INTEGER PRIMARY KEY,
            paper_id INTEGER NOT NULL,
            text TEXT NOT NULL
        );
        CREATE TABLE library_search_documents (
            paper_id INTEGER PRIMARY KEY,
            title TEXT,
            chinese_title TEXT,
            abstract TEXT,
            note TEXT,
            override_text TEXT,
            annotation_text TEXT,
            cjk_ngrams TEXT
        );
        CREATE VIRTUAL TABLE library_search_fts USING fts5(
            title, chinese_title, abstract, note, override_text, annotation_text, cjk_ngrams,
            content='library_search_documents', content_rowid='paper_id',
            tokenize='unicode61 remove_diacritics 1'
        );
        CREATE VIRTUAL TABLE library_search_trigram USING fts5(
            title, chinese_title, abstract, note, override_text, annotation_text,
            content='library_search_documents', content_rowid='paper_id', tokenize='trigram'
        );
        CREATE INDEX idx_lci_paper ON library_collection_items(paper_id);
        CREATE INDEX idx_lit_paper ON library_item_tags(paper_id);
        CREATE INDEX idx_papers_year_source ON papers(year, discovery_source);
        "#,
    )
}

fn refresh_document(conn: &Connection, paper_id: i64) -> Result<()> {
    let row: (Option<String>, Option<String>, Option<String>, Option<String>, Option<String>) = conn.query_row(
        "SELECT p.title, p.chinese_title, p.abstract, o.note, o.title_override
         FROM papers p LEFT JOIN library_item_overrides o ON o.paper_id=p.id WHERE p.id=?1",
        params![paper_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    let annotation: String = conn.query_row(
        "SELECT COALESCE(group_concat(text, ' '), '') FROM library_item_annotations WHERE paper_id=?1",
        params![paper_id], |r| r.get(0),
    )?;
    let title = row.4.clone().or(row.0.clone());
    let cjk = cjk_bigrams(&format!("{} {}", row.1.as_deref().unwrap_or(""), title.as_deref().unwrap_or("")));
    conn.execute("DELETE FROM library_search_fts WHERE rowid=?1", params![paper_id])?;
    conn.execute("DELETE FROM library_search_trigram WHERE rowid=?1", params![paper_id])?;
    conn.execute("DELETE FROM library_search_documents WHERE paper_id=?1", params![paper_id])?;
    conn.execute(
        "INSERT INTO library_search_documents(paper_id,title,chinese_title,abstract,note,override_text,annotation_text,cjk_ngrams)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![paper_id, title, row.1, row.2, row.3, row.4, annotation, cjk],
    )?;
    conn.execute(
        "INSERT INTO library_search_fts(rowid,title,chinese_title,abstract,note,override_text,annotation_text,cjk_ngrams)
         SELECT paper_id,title,chinese_title,abstract,note,override_text,annotation_text,cjk_ngrams FROM library_search_documents WHERE paper_id=?1",
        params![paper_id],
    )?;
    conn.execute(
        "INSERT INTO library_search_trigram(rowid,title,chinese_title,abstract,note,override_text,annotation_text)
         SELECT paper_id,title,chinese_title,abstract,note,override_text,annotation_text FROM library_search_documents WHERE paper_id=?1",
        params![paper_id],
    )?;
    Ok(())
}

fn candidate_ids(conn: &Connection, q: &str, collection: Option<i64>, tags: &[i64], year: Option<i64>, source: Option<&str>) -> Result<Vec<i64>> {
    let mut sql = if q.is_empty() {
        String::from("SELECT p.id FROM papers p JOIN library_items li ON li.paper_id=p.id WHERE 1=1")
    } else {
        String::from("SELECT p.id FROM library_search_fts JOIN papers p ON p.id=library_search_fts.rowid JOIN library_items li ON li.paper_id=p.id WHERE library_search_fts MATCH ?1")
    };
    let mut next = if q.is_empty() { 1 } else { 2 };
    if collection.is_some() {
        sql.push_str(&format!(" AND EXISTS (WITH RECURSIVE descendants(id) AS (SELECT ?{next} UNION ALL SELECT c.id FROM library_collections c JOIN descendants d ON c.parent_id=d.id) SELECT 1 FROM library_collection_items ci JOIN descendants d ON d.id=ci.collection_id WHERE ci.paper_id=p.id)"));
        next += 1;
    }
    if year.is_some() { sql.push_str(&format!(" AND p.year=?{next}")); next += 1; }
    if source.is_some() { sql.push_str(&format!(" AND p.discovery_source=?{next}")); next += 1; }
    for (i, _) in tags.iter().enumerate() {
        sql.push_str(&format!(" AND EXISTS (SELECT 1 FROM library_item_tags it WHERE it.paper_id=p.id AND it.tag_id=?{next})"));
        next += 1;
    }
    if q.is_empty() { sql.push_str(" ORDER BY p.id LIMIT 100"); } else { sql.push_str(" ORDER BY rank LIMIT 100"); }
    let mut stmt = conn.prepare(&sql)?;
    let mut values: Vec<&dyn rusqlite::ToSql> = Vec::new();
    if !q.is_empty() { values.push(&q); }
    let collection_value = collection.unwrap_or(0);
    if collection.is_some() { values.push(&collection_value); }
    let year_value = year.unwrap_or(0);
    if year.is_some() { values.push(&year_value); }
    let source_value = source.unwrap_or("");
    if source.is_some() { values.push(&source_value); }
    for tag in tags { values.push(tag); }
    stmt.query_map(rusqlite::params_from_iter(values), |r| r.get(0))?.collect()
}

fn assert_ids(conn: &Connection, q: &str, expected: &[i64], collection: Option<i64>, tags: &[i64], year: Option<i64>, source: Option<&str>) -> Result<()> {
    let got = candidate_ids(conn, &search_query(q), collection, tags, year, source)?;
    assert_eq!(got, expected, "query={q:?} fts={:?}", search_query(q));
    Ok(())
}

fn fixture(conn: &Connection) -> Result<()> {
    conn.execute("INSERT INTO papers VALUES (1,'Platform Governance and Network Effects','平台治理与网络效应','This abstract studies platform pricing and network effects.',2024,'crossref')", [])?;
    conn.execute("INSERT INTO papers VALUES (2,'A Quiet Paper','平台经济中的网络效应','关于平台经济和网络效应的中文摘要。',2023,'openalex')", [])?;
    conn.execute("INSERT INTO papers VALUES (3,'Metadata Only','元数据与来源','An abstract with no special note.',2024,'crossref')", [])?;
    conn.execute("INSERT INTO library_items VALUES (1),(2),(3)", [])?;
    conn.execute("INSERT INTO library_collections VALUES (10,NULL,'Research'),(11,10,'Platforms'),(12,NULL,'Methods')", [])?;
    conn.execute("INSERT INTO library_collection_items VALUES (10,1),(11,2),(12,3)", [])?;
    conn.execute("INSERT INTO library_tags VALUES (20,'important'),(21,'to-read'),(22,'methods')", [])?;
    conn.execute("INSERT INTO library_item_tags VALUES (1,20),(1,21),(2,20),(3,22)", [])?;
    conn.execute("INSERT INTO library_item_overrides VALUES (3,NULL,'follow up on network effects')", [])?;
    conn.execute("INSERT INTO library_item_annotations VALUES (1,3,'annotation: compare platform pricing')", [])?;
    for id in 1..=3 { refresh_document(conn, id)?; }
    Ok(())
}

fn functional_checks() -> Result<()> {
    let conn = Connection::open_in_memory()?;
    schema(&conn)?;
    fixture(&conn)?;
    // Default unicode61 misses a Chinese substring inside a longer run.
    let default_hit: i64 = conn.query_row("SELECT count(*) FROM library_search_fts WHERE library_search_fts MATCH 'chinese_title:平台'", [], |r| r.get(0))?;
    assert_eq!(default_hit, 0, "unicode61 does not split a longer Chinese run into individual words");
    assert_ids(&conn, "Governance", &[1], None, &[], None, None)?;
    assert_ids(&conn, "平台", &[1, 2], None, &[], None, None)?;
    assert_ids(&conn, "studies pricing", &[1], None, &[], None, None)?;
    assert_ids(&conn, "follow up", &[3], None, &[], None, None)?;
    assert_ids(&conn, "annotation compare", &[3], None, &[], None, None)?;
    assert_ids(&conn, "Governance", &[1], Some(10), &[], None, None)?;
    assert_ids(&conn, "", &[1, 2], None, &[20], None, None)?;
    assert_ids(&conn, "", &[1], None, &[20, 21], None, None)?;
    assert_ids(&conn, "", &[2], Some(11), &[20], None, None)?;
    assert_ids(&conn, "", &[3], None, &[22], Some(2024), Some("crossref"))?;
    // Descendant collection semantics: Platforms is included by Research.
    assert_ids(&conn, "", &[1, 2], Some(10), &[], None, None)?;
    // Reindexing after annotation/override changes is deterministic.
    conn.execute("INSERT INTO library_item_overrides(paper_id,note) VALUES (1,'rare robustness note') ON CONFLICT(paper_id) DO UPDATE SET note=excluded.note", [])?;
    refresh_document(&conn, 1)?;
    assert_ids(&conn, "rare robustness", &[1], None, &[], None, None)?;
    conn.execute("UPDATE library_item_overrides SET note='changed note' WHERE paper_id=3", [])?;
    assert_ids(&conn, "follow up", &[3], None, &[], None, None)?;
    refresh_document(&conn, 3)?;
    assert_ids(&conn, "changed note", &[3], None, &[], None, None)?;
    println!("functional checks: PASS (English, Chinese, abstract, note, override, annotation, collection, tag AND, source/year, nested collection)");
    Ok(())
}

fn benchmark(n: i64) -> Result<()> {
    let conn = Connection::open_in_memory()?;
    schema(&conn)?;
    let start = Instant::now();
    let tx = conn.unchecked_transaction()?;
    for id in 1..=n {
        let title = if id % 100 == 0 { "Platform Governance and Network Effects" } else { "Synthetic Research Paper" };
        let chinese = if id % 125 == 0 { "平台治理与网络效应" } else { "合成研究论文" };
        let abstract_text = if id % 200 == 0 { "This abstract discusses platform pricing and network effects." } else { "Synthetic abstract with reproducible benchmark content." };
        tx.execute("INSERT INTO papers VALUES (?1,?2,?3,?4,?5,?6)", params![id, title, chinese, abstract_text, 2000 + (id % 25), if id % 2 == 0 { "crossref" } else { "openalex" }])?;
        tx.execute("INSERT INTO library_items VALUES (?1)", params![id])?;
        tx.execute("INSERT INTO library_search_documents VALUES (?1,?2,?3,?4,'','', '',?5)", params![id, title, chinese, abstract_text, cjk_bigrams(&format!("{title} {chinese}"))])?;
    }
    tx.commit()?;
    let insert_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    conn.execute("INSERT INTO library_search_fts(library_search_fts) VALUES('rebuild')", [])?;
    let rebuild_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    conn.execute("INSERT INTO library_search_trigram(library_search_trigram) VALUES('rebuild')", [])?;
    let trigram_rebuild_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let count: i64 = conn.query_row("SELECT count(*) FROM library_search_fts WHERE library_search_fts MATCH 'platform'", [], |r| r.get(0))?;
    let search_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let cjk_count: i64 = conn.query_row("SELECT count(*) FROM library_search_fts WHERE library_search_fts MATCH '平台'", [], |r| r.get(0))?;
    let cjk_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert!(count > 0 && cjk_count > 0);
    println!("benchmark rows={n} insert_ms={insert_ms:.1} unicode_rebuild_ms={rebuild_ms:.1} trigram_rebuild_ms={trigram_rebuild_ms:.1} english_search_ms={search_ms:.3} cjk_search_ms={cjk_ms:.3} english_hits={count} cjk_hits={cjk_count}");
    Ok(())
}

fn main() -> Result<()> {
    functional_checks()?;
    for n in [10_000, 50_000, 100_000] { benchmark(n)?; }
    Ok(())
}

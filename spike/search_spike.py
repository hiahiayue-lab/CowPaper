#!/usr/bin/env python3
"""Temporary Library Search spike; only uses an in-memory synthetic database."""
import sqlite3
import time


def han(c):
    return any((lo <= ord(c) <= hi) for lo, hi in ((0x3400, 0x4DBF), (0x4E00, 0x9FFF), (0xF900, 0xFAFF)))


def cjk_bigrams(text):
    out, run = [], []
    def flush():
        if len(run) >= 2:
            out.extend("".join(run[i:i + 2]) for i in range(len(run) - 1))
        run.clear()
    for c in text:
        if han(c):
            run.append(c)
        else:
            flush()
    flush()
    return " ".join(out)


def fts_query(text):
    terms = []
    for raw in text.split():
        cjk = [c for c in raw if han(c)]
        latin = "".join(c for c in raw if not han(c))
        if len(cjk) >= 2:
            terms.extend("".join(pair) for pair in zip(cjk, cjk[1:]))
        if latin:
            terms.append(latin.replace('"', ''))
    return " AND ".join(f'"{t}"' for t in terms)


SCHEMA = """
CREATE TABLE papers (id INTEGER PRIMARY KEY, title, chinese_title, abstract, year, discovery_source);
CREATE TABLE library_items (paper_id INTEGER PRIMARY KEY REFERENCES papers(id));
CREATE TABLE library_collections (id INTEGER PRIMARY KEY, parent_id, name);
CREATE TABLE library_collection_items (collection_id, paper_id, PRIMARY KEY(collection_id, paper_id));
CREATE TABLE library_tags (id INTEGER PRIMARY KEY, name);
CREATE TABLE library_item_tags (paper_id, tag_id, PRIMARY KEY(paper_id, tag_id));
CREATE TABLE library_item_overrides (paper_id INTEGER PRIMARY KEY, title_override, note);
CREATE TABLE library_item_annotations (id INTEGER PRIMARY KEY, paper_id, text);
CREATE TABLE library_search_documents (paper_id INTEGER PRIMARY KEY, title, chinese_title, abstract, note, override_text, annotation_text, cjk_ngrams);
CREATE VIRTUAL TABLE library_search_fts USING fts5(title, chinese_title, abstract, note, override_text, annotation_text, cjk_ngrams, content='library_search_documents', content_rowid='paper_id', tokenize='unicode61 remove_diacritics 1');
CREATE VIRTUAL TABLE library_search_trigram USING fts5(title, chinese_title, abstract, note, override_text, annotation_text, content='library_search_documents', content_rowid='paper_id', tokenize='trigram');
CREATE INDEX idx_lci_paper ON library_collection_items(paper_id);
CREATE INDEX idx_lit_paper ON library_item_tags(paper_id);
CREATE INDEX idx_papers_year_source ON papers(year, discovery_source);
"""


def refresh(conn, paper_id):
    title, chinese, abstract, note, override = conn.execute(
        "SELECT p.title,p.chinese_title,p.abstract,o.note,o.title_override FROM papers p LEFT JOIN library_item_overrides o ON o.paper_id=p.id WHERE p.id=?",
        (paper_id,),
    ).fetchone()
    annotations = conn.execute(
        "SELECT coalesce(group_concat(text, ' '),'') FROM library_item_annotations WHERE paper_id=?", (paper_id,)
    ).fetchone()[0]
    effective_title = override or title or ""
    cjk = cjk_bigrams(f"{chinese or ''} {effective_title}")
    conn.execute("DELETE FROM library_search_fts WHERE rowid=?", (paper_id,))
    conn.execute("DELETE FROM library_search_trigram WHERE rowid=?", (paper_id,))
    conn.execute("DELETE FROM library_search_documents WHERE paper_id=?", (paper_id,))
    conn.execute("INSERT INTO library_search_documents VALUES (?,?,?,?,?,?,?,?)", (paper_id, effective_title, chinese, abstract, note, override, annotations, cjk))
    conn.execute("INSERT INTO library_search_fts(rowid,title,chinese_title,abstract,note,override_text,annotation_text,cjk_ngrams) SELECT paper_id,title,chinese_title,abstract,note,override_text,annotation_text,cjk_ngrams FROM library_search_documents WHERE paper_id=?", (paper_id,))
    conn.execute("INSERT INTO library_search_trigram(rowid,title,chinese_title,abstract,note,override_text,annotation_text) SELECT paper_id,title,chinese_title,abstract,note,override_text,annotation_text FROM library_search_documents WHERE paper_id=?", (paper_id,))


def candidates(conn, query, collection=None, tags=(), year=None, source=None):
    args, where = [], []
    if query:
        from_sql = "library_search_fts JOIN papers p ON p.id=library_search_fts.rowid JOIN library_items li ON li.paper_id=p.id"
        where.append("library_search_fts MATCH ?")
        args.append(fts_query(query))
    else:
        from_sql = "papers p JOIN library_items li ON li.paper_id=p.id"
    if collection is not None:
        where.append("EXISTS (WITH RECURSIVE descendants(id) AS (SELECT ? UNION ALL SELECT c.id FROM library_collections c JOIN descendants d ON c.parent_id=d.id) SELECT 1 FROM library_collection_items ci JOIN descendants d ON d.id=ci.collection_id WHERE ci.paper_id=p.id)")
        args.append(collection)
    if year is not None:
        where.append("p.year=?")
        args.append(year)
    if source is not None:
        where.append("p.discovery_source=?")
        args.append(source)
    for tag in tags:
        where.append("EXISTS (SELECT 1 FROM library_item_tags it WHERE it.paper_id=p.id AND it.tag_id=?)")
        args.append(tag)
    order = "rank" if query else "p.id"
    sql = f"SELECT p.id FROM {from_sql} WHERE {' AND '.join(where) or '1=1'} ORDER BY {order} LIMIT 100"
    return [r[0] for r in conn.execute(sql, args)]


def functional_checks():
    conn = sqlite3.connect(":memory:")
    conn.executescript(SCHEMA)
    conn.executemany("INSERT INTO papers VALUES (?,?,?,?,?,?)", [
        (1, "Platform Governance and Network Effects", "平台治理与网络效应", "This abstract studies platform pricing and network effects.", 2024, "crossref"),
        (2, "A Quiet Paper", "平台经济中的网络效应", "关于平台经济和网络效应的中文摘要。", 2023, "openalex"),
        (3, "Metadata Only", "元数据与来源", "An abstract with no special note.", 2024, "crossref"),
    ])
    conn.executemany("INSERT INTO library_items VALUES (?)", [(1,), (2,), (3,)])
    conn.executemany("INSERT INTO library_collections VALUES (?,?,?)", [(10, None, "Research"), (11, 10, "Platforms"), (12, None, "Methods")])
    conn.executemany("INSERT INTO library_collection_items VALUES (?,?)", [(10, 1), (11, 2), (12, 3)])
    conn.executemany("INSERT INTO library_tags VALUES (?,?)", [(20, "important"), (21, "to-read"), (22, "methods")])
    conn.executemany("INSERT INTO library_item_tags VALUES (?,?)", [(1, 20), (1, 21), (2, 20), (3, 22)])
    conn.execute("INSERT INTO library_item_overrides VALUES (?,?,?)", (3, None, "follow up on network effects"))
    conn.execute("INSERT INTO library_item_annotations VALUES (?,?,?)", (1, 3, "annotation: compare platform pricing"))
    for paper_id in (1, 2, 3):
        refresh(conn, paper_id)
    default_hit = conn.execute("SELECT count(*) FROM library_search_fts WHERE library_search_fts MATCH 'chinese_title:平台'").fetchone()[0]
    assert default_hit == 0, default_hit
    checks = [
        ("Governance", [1], None, (), None, None),
        ("平台", [1, 2], None, (), None, None),
        ("studies pricing", [1], None, (), None, None),
        ("follow up", [3], None, (), None, None),
        ("annotation compare", [3], None, (), None, None),
        ("Governance", [1], 10, (), None, None),
        ("", [1, 2], None, (20,), None, None),
        ("", [1], None, (20, 21), None, None),
        ("", [2], 11, (20,), None, None),
        ("", [3], None, (22,), 2024, "crossref"),
        ("", [1, 2], 10, (), None, None),
    ]
    for q, expected, collection, tags, year, source in checks:
        got = candidates(conn, q, collection, tags, year, source)
        assert set(got) == set(expected), (q, got, expected)
    conn.execute("INSERT INTO library_item_overrides(paper_id,note) VALUES (1,'rare robustness note') ON CONFLICT(paper_id) DO UPDATE SET note=excluded.note")
    refresh(conn, 1)
    assert candidates(conn, "rare robustness") == [1]
    conn.execute("UPDATE library_item_overrides SET note='changed note' WHERE paper_id=3")
    assert candidates(conn, "follow up") == [3], "external-content FTS is stale until explicitly synchronized"
    refresh(conn, 3)
    assert candidates(conn, "follow up") == []
    assert candidates(conn, "changed note") == [3]
    trigram = [r[0] for r in conn.execute("SELECT rowid FROM library_search_trigram WHERE library_search_trigram MATCH '经济中的' ORDER BY rowid")]
    assert trigram == [2], trigram
    print("functional checks: PASS (English, Chinese, abstract, note, override, annotation, collection, tag AND, source/year, nested collection, trigram probe)")


def benchmark(n):
    conn = sqlite3.connect(":memory:")
    conn.executescript(SCHEMA)
    start = time.perf_counter()
    with conn:
        for paper_id in range(1, n + 1):
            title = "Platform Governance and Network Effects" if paper_id % 100 == 0 else "Synthetic Research Paper"
            chinese = "平台治理与网络效应" if paper_id % 125 == 0 else "合成研究论文"
            abstract = "This abstract discusses platform pricing and network effects." if paper_id % 200 == 0 else "Synthetic abstract with reproducible benchmark content."
            conn.execute("INSERT INTO papers VALUES (?,?,?,?,?,?)", (paper_id, title, chinese, abstract, 2000 + paper_id % 25, "crossref" if paper_id % 2 == 0 else "openalex"))
            conn.execute("INSERT INTO library_items VALUES (?)", (paper_id,))
            conn.execute("INSERT INTO library_search_documents VALUES (?,?,?,?,?,?,?,?)", (paper_id, title, chinese, abstract, "", "", "", cjk_bigrams(f"{title} {chinese}")))
    insert_ms = (time.perf_counter() - start) * 1000
    start = time.perf_counter(); conn.execute("INSERT INTO library_search_fts(library_search_fts) VALUES('rebuild')"); rebuild_ms = (time.perf_counter() - start) * 1000
    start = time.perf_counter(); conn.execute("INSERT INTO library_search_trigram(library_search_trigram) VALUES('rebuild')"); trigram_rebuild_ms = (time.perf_counter() - start) * 1000
    start = time.perf_counter(); hits = conn.execute("SELECT count(*) FROM library_search_fts WHERE library_search_fts MATCH 'platform'").fetchone()[0]; search_ms = (time.perf_counter() - start) * 1000
    start = time.perf_counter(); cjk_hits = conn.execute("SELECT count(*) FROM library_search_fts WHERE library_search_fts MATCH '平台'").fetchone()[0]; cjk_ms = (time.perf_counter() - start) * 1000
    assert hits and cjk_hits
    print(f"benchmark rows={n} insert_ms={insert_ms:.1f} unicode_rebuild_ms={rebuild_ms:.1f} trigram_rebuild_ms={trigram_rebuild_ms:.1f} english_search_ms={search_ms:.3f} cjk_search_ms={cjk_ms:.3f} english_hits={hits} cjk_hits={cjk_hits}")


if __name__ == "__main__":
    print(f"python_sqlite={sqlite3.sqlite_version}")
    functional_checks()
    for size in (10_000, 50_000, 100_000):
        benchmark(size)

# CowPaper Library Search 技术 Spike

日期：2026-09-03  
分支：spike/library-search，基于 origin/main 本地引用  
范围：只验证 Library Search 设计和临时 prototype；没有修改 v0.2.0 Library 行为。

## 结论摘要

| 项目 | 结论 |
|---|---|
| RECOMMENDED SEARCH ENGINE | FTS5 YES |
| RECOMMENDED TOKENIZER | unicode61 remove_diacritics 1；中文由应用侧生成二元词并放入独立 FTS 列 |
| CHINESE SEARCH STRATEGY | 连续中文保留原文列；额外为 Han 字符串生成重叠二元词，查询时把中文短语转换成二元词 AND。trigram 只作为可选 substring/fallback，不作为默认索引 |
| SEARCHABLE FIELDS | canonical title、Chinese title、abstract、Library note、override metadata、future annotation text；使用分列以支持 BM25 权重 |
| STRUCTURED FILTERS | Library membership、collection、nested collection、tag、year、source；使用普通 JOIN/EXISTS，不复制进 FTS |
| COLLECTION FILTER SEMANTICS | 选择 collection 时包含自身和所有 descendants；空 collection 不返回结果 |
| TAG FILTER SEMANTICS | 一个 tag 是一个 EXISTS；多个 tag 是 AND（同一 paper 必须拥有全部 tag） |
| FUTURE ANNOTATION SUPPORT | YES；预留 annotation_text projection 列，并在 annotation 写入/删除事务中刷新同一 paper 的索引 |
| PROPOSED SCHEMA | library_search_documents external-content source table + library_search_fts FTS5 virtual table |
| PROPOSED COMMAND/API | search_library(query, filters, limit, offset)；Rust 负责参数化 SQL、FTS query escaping 和返回 LibraryPaper/search hit |
| INDEX STRATEGY | 一篇 Library paper 一行 projection/FTS row；增量 refresh；后台/显式 rebuild；FTS candidate set 后再做 structured filters |
| PERFORMANCE FINDINGS | 10k/50k/100k 的 unicode61 rebuild 为 15.6/86.7/198.3 ms，English query 为 0.055/0.080/0.101 ms；trigram rebuild 更慢，为 31.4/254.1/592.8 ms |
| MAC SUPPORT | YES，前提是发布产物继续使用 rusqlite 的 bundled SQLite，并在构建 smoke test 断言 FTS5/tokenizer |
| WINDOWS SUPPORT | YES，同上；不能依赖系统 SQLite 或 DLL 中恰好存在的 tokenizer |
| PROTOTYPE CREATED | YES |
| FORMAL MIGRATION CREATED | NO |
| USER DB TOUCHED | NO |
| READY FOR IMPLEMENTATION | YES，但应先完成 bundled SQLite 的 macOS/Windows 构建 smoke test |

## 当前架构检查

app/src-tauri/Cargo.toml 使用 rusqlite 0.32 且启用 bundled；Cargo.lock 中底层 crate 为 libsqlite3-sys 0.30.1。数据库当前 SCHEMA_VERSION 为 14，v14 已建立：

- canonical papers，由 library_items(paper_id) 表示是否收录；
- library_collections(parent_id, ...) 和 library_collection_items，支持树形文献夹；
- library_tags 和 library_item_tags；
- 没有 Library note、override metadata 或 annotation 表。

因此搜索不能创建第二个 paper entity，也不能把 collection/tag 的派生属性复制到 canonical papers。

## FTS5 适配性

FTS5 很适合这一层：它提供虚拟表、MATCH 查询、列过滤、prefix 查询、bm25()/rank 和 external-content table。SQLite 官方文档说明，FTS5 自 3.9.0 起随 amalgamation 提供，但实际构建仍需 SQLITE_ENABLE_FTS5；官方也明确 external-content index 必须由应用保持同步，现有数据在创建 trigger 时不会自动进入索引，需要一次 rebuild。

建议使用 external-content 设计，而不是把长摘要复制一份进 FTS：

    CREATE TABLE library_search_documents (
      paper_id INTEGER PRIMARY KEY REFERENCES papers(id) ON DELETE CASCADE,
      title TEXT, chinese_title TEXT, abstract TEXT, note TEXT,
      override_text TEXT, annotation_text TEXT, cjk_ngrams TEXT
    );
    CREATE VIRTUAL TABLE library_search_fts USING fts5(
      title, chinese_title, abstract, note, override_text,
      annotation_text, cjk_ngrams,
      content='library_search_documents', content_rowid='paper_id',
      tokenize='unicode61 remove_diacritics 1'
    );

library_search_documents 是可重建的 search projection，不是产品事实来源。FTS rowid 直接等于 canonical paper_id，查询后 JOIN 回 papers 和 Library membership。

## English + Chinese 验证

unicode61 默认按 Unicode 的 letter/number run 形成 token，并做 Unicode case-folding；它不是中文分词器。prototype 对 平台治理与网络效应 做了检查：只在 chinese_title 列搜索 平台 时，默认 unicode61 不命中，因为原文是一个连续 token；对完整连续短语可以命中。

所以：

- English title/abstract：足够作为基础词搜索；
- Chinese 完整连续 token：可用；
- 中文常见的子串/短语搜索：不够；
- 自定义 ICU/第三方 CJK tokenizer：暂不推荐作为 v0.2.1 基线，因其需要跨平台静态集成和额外升级/分发风险。

推荐应用侧 CJK 二元词。例如 平台经济中的网络效应 额外生成 平台 台经 经济 济中 中的 的网 网络 络效 效应，查询 平台 转成一个 quoted term，查询 网络效应 转成 网络 AND 络效 AND 效应。英文词仍按 unicode61 查询。单个中文字符应走明确的 fallback（例如受限 LIKE 或提示至少两个字符），不要把所有查询默认降级为全表 LIKE。

### trigram 判断

SQLite 内置 trigram tokenizer 支持一般 substring matching，且可以加速部分 LIKE/GLOB；但少于 3 个 Unicode 字符的 full-text query 不会命中，索引通常也明显更大。它适合作为可选的模糊/任意子串索引，不适合作为中英文 Library 默认索引。

FTS5/trigram 支持不是所有 SQLite 编译都必然打开：SQLite 官方要求用 SQLITE_ENABLE_FTS5 编译 FTS5；运行时可以用 PRAGMA compile_options 或 sqlite_compileoption_used() 检查。当前项目使用 bundled 是正确方向，但仍应在两个 release binary 中做启动 smoke test。

参考：

- https://www.sqlite.org/fts5.html
- https://www.sqlite.org/fts5.html#the_trigram_tokenizer
- https://www.sqlite.org/compile.html
- https://www.sqlite.org/c3ref/compileoption_get.html

## Effective text 与未来字段

建议的 effective text 规则：

1. title 列使用 title_override（有值时）否则使用 canonical papers.title；
2. chinese_title、canonical abstract 保留为独立列；
3. note、所有 override metadata 的可搜索文本放入 note/override_text；
4. 多条 annotation 聚合到 annotation_text，但显示结果始终从 annotation 表和 canonical 表读取；
5. 不把 tag 名称或 collection 名称放入 FTS。它们是结构化 filter，避免重命名后出现过期索引。

未来可以增加逻辑表：

    CREATE TABLE library_item_overrides (
      paper_id INTEGER PRIMARY KEY REFERENCES papers(id) ON DELETE CASCADE,
      title_override TEXT, abstract_override TEXT, note TEXT,
      updated_at TEXT NOT NULL
    );
    CREATE TABLE library_annotations (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      paper_id INTEGER NOT NULL REFERENCES papers(id) ON DELETE CASCADE,
      text TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
    );

如果产品最终允许 override abstract，effective abstract 应另设一列或明确 COALESCE(abstract_override, papers.abstract)，不要静默覆盖 canonical abstract。prototype 已验证 note、override 和 annotation 写入 projection 后可搜索。

## 同步与 rebuild

external-content FTS5 的正确性由同步策略决定。建议所有下列动作都走同一个 Rust transaction helper refresh_library_search_document(tx, paper_id)：

- add/remove paper from Library：插入/删除 projection 和 FTS row；
- canonical title/abstract/Chinese fields 更新：若 paper 在 Library 中则 refresh；
- override/note/annotation 写入、更新、删除：refresh；
- collection/tag membership 变化：不需要 reindex，只影响 structured JOIN；
- collection rename：不需要 reindex，因为按 collection id 过滤。

prototype 明确验证了：只更新 external-content source row 而不刷新 FTS 时，旧命中仍然存在；只有先删除 FTS row，再更新 projection，再插入新 row，旧命中才消失。这不是可省略的清理步骤。

启动时不做每行 rebuild。增加 rebuild_library_search_index 内部操作或版本检查：projection 缺失/索引损坏时在后台执行 INSERT INTO library_search_fts(library_search_fts) VALUES ('rebuild')。同步逻辑必须提供一次性 backfill，且创建 FTS table 后显式 rebuild。

## 查询形状与语义

候选集查询应保持 FTS 先缩小、结构化条件再过滤的形状：

    WITH RECURSIVE descendants(id) AS (
      SELECT :collection_id
      UNION ALL
      SELECT c.id FROM library_collections c
      JOIN descendants d ON c.parent_id = d.id
    )
    SELECT p.*, bm25(library_search_fts, 10.0, 10.0, 2.0, 4.0, 6.0, 2.0, 1.0) AS relevance
    FROM library_search_fts
    JOIN papers p ON p.id = library_search_fts.rowid
    JOIN library_items li ON li.paper_id = p.id
    WHERE library_search_fts MATCH :fts_query
      AND EXISTS (
        SELECT 1 FROM library_collection_items ci
        JOIN descendants d ON d.id = ci.collection_id
        WHERE ci.paper_id = p.id
      )
      AND (:year IS NULL OR p.year = :year)
      AND (:source IS NULL OR p.discovery_source = :source)
    ORDER BY relevance ASC, p.id DESC
    LIMIT :limit OFFSET :offset;

每一个 selected tag 都追加一个 EXISTS；不要用一个 IN 实现 AND。空文本（tag-only/filter-only）不应执行 MATCH ''，而应从 library_items 开始查询并保留同一组 structured predicates。

source 在 v0.2.1 建议明确映射到 papers.discovery_source（如 crossref/openalex）；不要把 abstract_source 和 discovery source 隐式混为一个 facet。BM25 是相关性排序，不是产品 score，不能改变已有 recommendation score 或历史排序语义。

## Prototype 与 benchmark

创建的临时文件：

- spike/search_spike.py：可直接运行的 in-memory prototype/benchmark；
- app/src-tauri/examples/search_spike.rs：目标运行时的 rusqlite bundled 版本 prototype，使用同样的临时 schema；
- 本报告。

运行方式：

    python3 spike/search_spike.py
    cargo run --manifest-path app/src-tauri/Cargo.toml --example search_spike --release

脚本验证了 English title、Chinese title、abstract、note、override、annotation、title + collection、tag-only、collection + tag、multiple-tag AND、source/year、nested collection 和 trigram probe。所有数据均为 in-memory synthetic rows；没有打开或写入应用用户 DB。

本机可执行的 Python 结果（单次运行）：

| rows | projection insert ms | unicode61 rebuild ms | trigram rebuild ms | English search ms | CJK search ms |
|---:|---:|---:|---:|---:|---:|
| 10,000 | 209.9 | 15.6 | 31.4 | 0.055 | 0.013 |
| 50,000 | 1,071.2 | 86.7 | 254.1 | 0.080 | 0.031 |
| 100,000 | 2,124.0 | 198.3 | 592.8 | 0.101 | 0.047 |

命中数分别为 English 100/500/1000、CJK 80/400/800。结果说明 100k 行的 rebuild 在 prototype 中是可接受的数量级，而查询耗时远小于 rebuild；正式实现仍应在 macOS 和 Windows release binary 上复测。

### 环境与限制

- python3 绑定 SQLite 3.51.0，ENABLE_FTS5 已启用；
- 当前执行环境没有 cargo/rustc，所以 rusqlite bundled example 未能在本机编译运行；
- 因此 bundled SQLite 的实际版本、FTS5 compile option 和 trigram 在 macOS/Windows 安装包中的结果仍需 CI/release smoke test 明确记录；
- Python 脚本验证的是 SQL/FTS 行为和规模趋势，不替代 bundled target validation；
- 未实现正式 command/API、UI、migration、PDF backend 或 Annotation 产品行为。

## 风险与 v0.2.1 建议

RISKS：

1. external-content FTS 如果遗漏任意一个写路径，会出现表里有数据但搜索不到或旧 row 残留；必须集中 refresh、启动 integrity check 和可重复 rebuild。
2. 应用侧中文二元词会增加 projection/index 写入量；需要限制超长 annotation/abstract 的最大索引文本或设置 detail/columnsize 前先测 snippet 需求。
3. FTS query parser 不能直接拼接用户输入；必须把用户文本转成安全的 quoted terms，并处理空词、引号、AND/OR 等语法字符。
4. trigram 对两字符中文搜索仍无 full-text 命中，不能单独解决中文搜索；自定义 CJK tokenizer 的跨平台分发和升级风险更高。
5. 多 tag 的 AND filter 和 nested collection 的 recursive CTE 需要保持索引，尤其是 library_collection_items(paper_id, collection_id) 和 library_item_tags(paper_id, tag_id)。
6. BM25 是相关性排序，不是产品 score；不能改变已有 recommendation score 或历史排序语义。

RECOMMENDATION FOR v0.2.1：实现 FTS5 + unicode61 + 应用侧中文二元词 + external-content projection；把 collection/tag/year/source 保持为 structured filters；首版预留 annotation 列但不做 annotation UI；提供 backfill/rebuild、断言 FTS5 能力的启动检查，以及 macOS/Windows release smoke test。

READY FOR IMPLEMENTATION：YES。实现前提是完成 bundled SQLite 双平台构建验证并把上述同步 helper 接入所有 canonical/Library metadata 写路径。

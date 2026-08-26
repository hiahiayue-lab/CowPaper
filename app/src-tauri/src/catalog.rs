//! Round 5C：Verified Journal Catalog 导入。
//!
//! 单一 canonical catalog（`catalog.json`，编译期内嵌）：
//! - canonical journals（唯一定义）+ collections（UTD24 / FT50-2026）分离
//! - membership 来源：UT Dallas 官方 / FT 官方；identifiers 来源：Crossref 结构化数据
//! - 导入幂等：重复执行不产生重复 collection / journal / membership / identifier
//! - 复用 Round 5A resolver（resolve_journal_by_identifier / issn_l），不建立第二套 identity
//! - enrichment 不覆盖用户订阅（enabled）与任何用户设置
//! - identifier 未可靠解决 → metadata_needs_review 标记，不阻塞导入

use rusqlite::Connection;
use serde::Deserialize;

use crate::db;
use crate::models::{IDT_ONLINE, IDT_PRINT};

pub const CATALOG_JSON: &str = include_str!("catalog.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogFile {
    #[allow(dead_code)]
    pub catalog_version: i64,
    #[allow(dead_code)]
    pub generated_at: String,
    pub collections: Vec<CatalogCollectionDef>,
    pub journals: Vec<CatalogJournalDef>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogCollectionDef {
    pub code: String,
    pub name: String,
    pub version: String,
    pub effective_from: Option<String>,
    pub source_name: String,
    pub source_url: String,
    #[allow(dead_code)]
    pub last_verified_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogJournalDef {
    #[allow(dead_code)]
    pub catalog_id: String,
    pub canonical_title: String,
    pub publisher: Option<String>,
    pub print_issn: Option<String>,
    pub online_issn: Option<String>,
    pub issn_l: Option<String>,
    #[allow(dead_code)]
    pub aliases: Vec<String>,
    #[allow(dead_code)]
    pub metadata_sources: Vec<String>,
    #[allow(dead_code)]
    pub last_verified_at: String,
    pub collections: Vec<String>,
    pub metadata_needs_review: bool,
}

#[derive(Debug, Default)]
pub struct CatalogImportReport {
    pub journals_created: i64,
    pub journals_merged: i64,
    pub memberships_added: i64,
    pub identifiers_added: i64,
    pub needs_review: Vec<String>,
}

/// 幂等导入 catalog（app 启动时调用；重复执行安全）。
pub fn import_catalog(conn: &Connection) -> Result<CatalogImportReport, String> {
    let data: CatalogFile =
        serde_json::from_str(CATALOG_JSON).map_err(|e| format!("catalog 解析失败: {}", e))?;
    let mut report = CatalogImportReport::default();

    // 1) Collections（code 唯一；已存在则复用）
    let mut cids: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for c in &data.collections {
        let id = match db::find_collection_by_code(conn, &c.code).map_err(|e| e.to_string())? {
            Some(id) => id,
            None => db::create_collection(
                conn,
                &c.code,
                &c.name,
                Some(&c.version),
                c.effective_from.as_deref(),
                Some(&c.source_name),
                Some(&c.source_url),
            )
            .map_err(|e| e.to_string())?,
        };
        cids.insert(c.code.clone(), id);
    }

    // 2) Journals（复用 Round 5A resolver；enrichment 不覆盖用户设置）
    for j in &data.journals {
        let print_norm = j.print_issn.as_deref().and_then(crate::util::normalize_issn);
        let online_norm = j.online_issn.as_deref().and_then(crate::util::normalize_issn);
        let issn_l_norm = j.issn_l.as_deref().and_then(crate::util::normalize_issn);

        // resolve 优先级（Round 5C.1）：
        // 1) exact normalized ISSN（print/online 任一）
        // 2) ISSN-L
        // 3) 显式 alias / canonical_title（catalog.json 已验证的别名；带 identifier 冲突检测）
        let mut jid = None;
        for n in [&print_norm, &online_norm].into_iter().flatten() {
            if let Some(id) = db::resolve_journal_by_identifier(conn, n).map_err(|e| e.to_string())? {
                jid = Some(id);
                break;
            }
        }
        if jid.is_none() {
            if let Some(il) = &issn_l_norm {
                jid = db::find_journal_by_issn_l(conn, il).map_err(|e| e.to_string())?;
            }
        }
        // alias 命中后必须检查 identifier 冲突：冲突 → 不 merge（保留两条并标记 review）
        let mut alias_conflict = false;
        if jid.is_none() {
            let mut alias_list = j.aliases.clone();
            alias_list.push(j.canonical_title.clone());
            if let Some(id) = db::find_journal_by_aliases(conn, &alias_list).map_err(|e| e.to_string())? {
                if db::journal_has_conflicting_identifiers(conn, id, print_norm.as_deref(), online_norm.as_deref())
                    .map_err(|e| e.to_string())?
                {
                    // identifiers 冲突：不自动 merge；创建 catalog Journal 并标记 review，
                    // 已有 Journal 的 possible_duplicate 由 list_journals 标题规范化检测。
                    alias_conflict = true;
                } else {
                    jid = Some(id);
                }
            }
        }

        let id = match jid {
            Some(id) => {
                report.journals_merged += 1;
                id
            }
            None => {
                let id = db::insert_journal(
                    conn,
                    &j.canonical_title,
                    print_norm.as_deref(),
                    online_norm.as_deref(),
                    j.publisher.as_deref(),
                    None,
                )
                .map_err(|e| e.to_string())?;
                // Catalog ≠ Subscription：新建期刊默认不订阅（enabled=0），用户自行选择
                db::set_journal_enabled(conn, id, false).map_err(|e| e.to_string())?;
                report.journals_created += 1;
                if alias_conflict {
                    db::set_journal_review_flag(conn, id, true).map_err(|e| e.to_string())?;
                    if !report.needs_review.contains(&j.canonical_title) {
                        report.needs_review.push(j.canonical_title.clone());
                    }
                }
                id
            }
        };

        // identifiers（幂等：INSERT OR IGNORE）
        for (n, ty) in [
            (print_norm.as_deref(), IDT_PRINT),
            (online_norm.as_deref(), IDT_ONLINE),
        ]
        .into_iter()
        {
            if let Some(n) = n {
                let before = db::list_journal_identifiers(conn, id).map_err(|e| e.to_string())?;
                let has = before.iter().any(|i| i.value == n);
                db::insert_identifier(conn, id, ty, n, Some("catalog"))
                    .map_err(|e| e.to_string())?;
                if !has {
                    report.identifiers_added += 1;
                }
            }
        }
        // issn_l：仅当目前为空时补（不覆盖）
        if let Some(il) = &issn_l_norm {
            let cur = db::get_journal_issn_l(conn, id).map_err(|e| e.to_string())?;
            if cur.as_deref().is_none() || cur.as_deref() == Some("") {
                db::set_journal_issn_l(conn, id, Some(il)).map_err(|e| e.to_string())?;
            }
        }
        // metadata_needs_review：仅标记为 true 时写入（不覆盖 false）
        if j.metadata_needs_review {
            db::set_journal_review_flag(conn, id, true).map_err(|e| e.to_string())?;
            if !report.needs_review.contains(&j.canonical_title) {
                report.needs_review.push(j.canonical_title.clone());
            }
        }
        // collections membership（幂等：PRIMARY KEY 拒绝重复）
        for code in &j.collections {
            if let Some(cid) = cids.get(code) {
                let added = db::add_collection_member(conn, *cid, id).map_err(|e| e.to_string())?;
                if added {
                    report.memberships_added += 1;
                }
            }
        }
    }

    Ok(report)
}

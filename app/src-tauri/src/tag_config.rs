//! Round 6.5：Versioned Tag Configuration & Incremental Reranking。
//!
//! - tags 表 = 当前 active 配置；tag_config_versions 保存 active/scheduled/retired 历史。
//! - scheduled：下个推荐周期生效（不调 AI、不重排当前周期）；同一 upcoming cycle 至多一个。
//! - immediate：立即成为 active → 计算 diff → 本地处理 + tag-only 增量 AI。
//! - tag_semantic_hash = hash(tag_id + normalized name + normalized description + score prompt version)，
//!   Paper tag score 关联该 hash；hash 不变 → cache 复用，变化 → 只该 tag stale。
//! - 增量更新绝不重新生成 chineseTitle / chineseAbstract / oneSentenceSummary。

use rusqlite::{params, Connection};

use crate::db;
use crate::models::{
    TagConfigDiff, TagConfigItem, TagDraftItem, TagMatch, SaveTagConfigResult,
};
use crate::util::hash64;

/// tag 评分 prompt 版本（影响 semantic hash；修改 prompt 语义时递增）。
pub const TAG_SCORE_PROMPT_VERSION: &str = "v1";

#[allow(dead_code)] // 语义常量（db 层以字面量使用；保留文档价值）
pub const TCV_ACTIVE: &str = "active";
#[allow(dead_code)]
pub const TCV_SCHEDULED: &str = "scheduled";
#[allow(dead_code)]
pub const TCV_RETIRED: &str = "retired";

fn normalize_semantic(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// tag 语义 hash：tag_id + 规范化 name + 规范化 description + prompt 版本。
pub fn tag_semantic_hash(tag_id: i64, name: &str, description: &str) -> String {
    hash64(&format!(
        "{}|{}|{}|{}",
        tag_id,
        normalize_semantic(name),
        normalize_semantic(description),
        TAG_SCORE_PROMPT_VERSION
    ))
}

/// 当前 active tag 配置（tags 表 enabled 视角；返回 (id, name, description)）。
pub fn active_tags(conn: &Connection) -> Result<Vec<(i64, String, String)>, String> {
    let tags = db::list_tags(conn).map_err(|e| e.to_string())?;
    Ok(tags
        .into_iter()
        .filter(|t| t.enabled)
        .map(|t| (t.id, t.name, t.description.unwrap_or_default()))
        .collect())
}

/// 计算 old active 配置 → new 配置 的 diff（按 tag_id 对齐；新 tag id=0 占位按 name 对齐）。
pub fn compute_diff(old_items: &[TagConfigItem], new_items: &[TagDraftItem]) -> TagConfigDiff {
    let mut diff = TagConfigDiff::default();
    let mut old_by_id: std::collections::HashMap<i64, &TagConfigItem> = std::collections::HashMap::new();
    for o in old_items {
        old_by_id.insert(o.tag_id, o);
    }
    for n in new_items {
        // 新增（id=0）或 old 中不存在 → added
        let old = if n.id > 0 {
            old_by_id.get(&n.id).copied()
        } else {
            // 按 name 对齐旧配置（重命名场景无法对齐，按 added 处理）
            old_items.iter().find(|o| o.name == n.name)
        };
        let Some(o) = old else {
            if !n.deleted {
                diff.added.push(n.name.clone());
            }
            continue;
        };

        if n.deleted {
            diff.removed.push(n.name.clone());
            continue;
        }
        if !o.enabled && n.enabled {
            diff.enabled.push(n.name.clone());
            continue;
        }
        if o.enabled && !n.enabled {
            diff.disabled.push(n.name.clone());
            continue;
        }
        if o.enabled && n.enabled {
            let o_desc = o.description.clone().unwrap_or_default();
            let n_desc = n.description.clone().unwrap_or_default();
            let o_hash = tag_semantic_hash(o.tag_id, &o.name, &o_desc);
            // 新 tag 尚未分配 id：用 0 语义（name+desc）比较
            let n_id = if n.id > 0 { n.id } else { o.tag_id };
            let n_hash = tag_semantic_hash(n_id, &n.name, &n_desc);
            if o_hash != n_hash {
                diff.semantic_changed.push(n.name.clone());
            } else {
                diff.unchanged.push(n.name.clone());
            }
        } else {
            diff.unchanged.push(n.name.clone());
        }
    }
    // removed：old 中存在（非 deleted）但 draft 中已移除（splice 删除）→ removed
    for o in old_items {
        if o.deleted {
            continue;
        }
        let in_new = new_items.iter().any(|n| n.id > 0 && n.id == o.tag_id);
        if !in_new {
            diff.removed.push(o.name.clone());
        }
    }
    diff
}

/// 本地重算某 Paper 的 total_score：只加 active（非 deleted、enabled）且 semantic hash 匹配的 tag score。
/// 保留 tag_matches_json 全部记录（disabled 作为缓存保留），只重算 total。
pub fn recompute_paper_total_score(conn: &Connection, paper_id: i64, active_tags: &[(i64, String, String)]) -> Result<(), String> {
    let matches_json: Option<String> = conn
        .query_row(
            "SELECT tag_matches_json FROM papers WHERE id = ?1",
            params![paper_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    let Some(json) = matches_json else {
        return Ok(());
    };
    let matches: Vec<TagMatch> = serde_json::from_str(&json).unwrap_or_default();
    let mut total: f64 = 0.0;
    for m in &matches {
        // 判定该 match 是否对应某个 active tag 且 hash 匹配
        let active_hit = active_tags.iter().find(|(id, name, desc)| {
            let id_match = m.tag_id == Some(*id) || (m.tag_id.is_none() && &m.tag == name);
            if !id_match {
                return false;
            }
            let expect = tag_semantic_hash(*id, name, desc);
            m.semantic_hash.as_deref() == Some(expect.as_str())
        });
        if active_hit.is_some() {
            total += m.score;
        }
    }
    conn.execute(
        "UPDATE papers SET total_score = ?1, updated_at = ?2 WHERE id = ?3",
        params![total, db::now_utc(), paper_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 保存 draft 为 scheduled 配置（替换已有 scheduled；不调 AI、不改 tags、不重排）。
pub fn save_scheduled_config(conn: &Connection, draft: &[TagDraftItem], effective_cycle_key: &str) -> Result<SaveTagConfigResult, String> {
    db::replace_scheduled_tag_config(conn, draft, effective_cycle_key).map_err(|e| e.to_string())?;
    Ok(SaveTagConfigResult {
        mode: "scheduled".to_string(),
        effective_cycle_key: Some(effective_cycle_key.to_string()),
        diff: TagConfigDiff::default(),
        ai_needed_papers: 0,
    })
}

/// 立即保存：写 tags 表（active）→ 新 active version → diff → 本地重算 → 返回 diff。
/// AI-needed（added/semanticChanged）由调用方（命令层）启动 tag-only batch。
pub fn save_immediate_config(conn: &Connection, draft: &[TagDraftItem]) -> Result<SaveTagConfigResult, String> {
    // old = 保存前 tags 表当前状态（active 语义以 tags 表为准；version items 仅为历史快照）
    let old_tags = db::list_tags(conn).map_err(|e| e.to_string())?;
    let old_items: Vec<TagConfigItem> = old_tags
        .iter()
        .map(|t| TagConfigItem {
            version_id: 0,
            tag_id: t.id,
            name: t.name.clone(),
            description: t.description.clone(),
            enabled: t.enabled,
            deleted: false,
        })
        .collect();
    // 1) 写 tags 表（active）
    for item in draft {
        if item.id > 0 {
            if item.deleted {
                db::delete_tag(conn, item.id).map_err(|e| e.to_string())?;
            } else {
                db::update_tag(conn, item.id, &item.name, item.description.as_deref(), item.enabled)
                    .map_err(|e| e.to_string())?;
            }
        } else if !item.deleted {
            // 新增：先按 name 查重（UNIQUE 约束由 DB 保证）
            let exists = db::find_tag_by_name(conn, &item.name).map_err(|e| e.to_string())?;
            match exists {
                Some(id) => {
                    db::update_tag(conn, id, &item.name, item.description.as_deref(), item.enabled)
                        .map_err(|e| e.to_string())?;
                }
                None => {
                    db::add_tag(conn, &item.name, item.description.as_deref()).map_err(|e| e.to_string())?;
                }
            }
        }
    }
    // 1.5) 删除 old 中存在但 draft 中已移除的 tag（前端 splice 删除语义）
    for o in &old_items {
        let in_draft = draft.iter().any(|d| d.id > 0 && d.id == o.tag_id);
        if !in_draft {
            db::delete_tag(conn, o.tag_id).map_err(|e| e.to_string())?;
        }
    }
    // 2) 新 active version（当前 tags 表快照）
    db::create_active_tag_version(conn).map_err(|e| e.to_string())?;
    // 3) diff：old active items vs draft（draft 即 new）
    let diff = compute_diff(&old_items, draft);
    // 4) 本地重算：removed/disabled 的 paper 总分（AI-needed 部分等 tag-only 完成后统一重算）
    let active = active_tags(conn)?;
    let mut local_paper_ids: Vec<i64> = Vec::new();
    if !diff.removed.is_empty() || !diff.disabled.is_empty() {
        local_paper_ids = db::paper_ids_with_tag_names(conn, &diff.removed, &diff.disabled)
            .map_err(|e| e.to_string())?;
    }
    for pid in &local_paper_ids {
        recompute_paper_total_score(conn, *pid, &active).map_err(|e| e.to_string())?;
    }
    // immediate 激活即消费 scheduled：不得在下一 cutoff 重复激活/重复 tag-only
    db::delete_scheduled_tag_config(conn).map_err(|e| e.to_string())?;
    Ok(SaveTagConfigResult {
        mode: "immediate".to_string(),
        effective_cycle_key: None,
        diff,
        ai_needed_papers: 0,
    })
}

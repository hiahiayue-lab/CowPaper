//! Round 6：每日推荐时间线与历史。
//!
//! - 推荐周期 cutoff = Settings 的每日检查时间（daily_check_time，默认 09:00）。
//! - cycle_key = 本地时区日期：当本地时间未到当日 cutoff 时，当前周期仍是"昨天"的日期。
//! - 同一 Paper 一生只进入一个推荐周期（recommendation_items.UNIQUE(paper_id) 硬约束 +
//!   查询 NOT EXISTS 双重保证）。
//! - open run 随 AI 完成/手动同步自动刷新（rank/score_snapshot 可更新）；
//!   finalized run 冻结（不再修改 membership/rank/score）。
//! - 全部由 Rust/DB 负责，前端不得自行推导历史推荐。

use chrono::{DateTime, Local};
use rusqlite::{params, Connection};

use crate::db;

pub const RC_OPEN: &str = "open";
#[allow(dead_code)] // 语义说明与测试使用
pub const RC_FINALIZED: &str = "finalized";

/// 解析 HH:MM 为当日 cutoff 时刻（非法/缺失回退 09:00）。
pub fn cutoff_time(daily_check_time: &str) -> (u32, u32) {
    let parts: Vec<&str> = daily_check_time.split(':').collect();
    let h: u32 = parts.first().and_then(|p| p.parse().ok()).unwrap_or(9).min(23);
    let m: u32 = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(0).min(59);
    (h, m)
}

/// 当前推荐周期 key（本地时区）：当天 cutoff 未到 → 使用昨天日期。
pub fn cycle_key_for(now: &DateTime<Local>, daily_check_time: &str) -> String {
    let (h, m) = cutoff_time(daily_check_time);
    let cutoff = now.date_naive().and_hms_opt(h, m, 0).expect("valid cutoff");
    let now_dt = now.naive_local();
    let day = if now_dt < cutoff {
        now.date_naive() - chrono::Days::new(1)
    } else {
        now.date_naive()
    };
    day.format("%Y-%m-%d").to_string()
}

/// 幂等：确保存在"当前 open run"。
/// 1) 按 cycle_key 查找 → 存在（无论 open）即返回；
/// 2) 不存在 → finalize 所有更早的 open run（跨 cutoff / restart catch-up）→ 创建新 open run。
pub fn ensure_current_recommendation_cycle(
    conn: &Connection,
    now: &DateTime<Local>,
    daily_check_time: &str,
) -> Result<i64, String> {
    let key = cycle_key_for(now, daily_check_time);
    if let Some(id) = db::find_recommendation_run_by_cycle_key(conn, &key).map_err(|e| e.to_string())? {
        return Ok(id);
    }
    // finalize 所有非当前 key 的 open run（App 关闭跨过 cutoff 后启动时 catch-up）
    db::finalize_open_runs_except(conn, &key, &db::now_utc()).map_err(|e| e.to_string())?;
    let id = db::create_recommendation_run(conn, &key, RC_OPEN).map_err(|e| e.to_string())?;
    Ok(id)
}

/// 重算当前 open run 的推荐项：
/// - 删除本 run 现有 items（open 允许重建）后重算；
/// - 候选 = 现有推荐规则（totalScore 非空且未忽略）+ NOT EXISTS(recommendation_items)
///   （从未在任何周期推荐过；paper_id 全局唯一约束兜底）；
/// - 按 totalScore DESC、published_date DESC、id DESC 排序，rank 重排。
pub fn refresh_current_recommendations(
    conn: &Connection,
    now: &DateTime<Local>,
    daily_check_time: &str,
) -> Result<i64, String> {
    let run_id = ensure_current_recommendation_cycle(conn, now, daily_check_time)?;
    // 已 finalize 的 run 不得重建
    let run = db::get_recommendation_run(conn, run_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "推荐周期不存在".to_string())?;
    if run.status != RC_OPEN {
        return Ok(run_id); // finalized：冻结
    }
    let now_iso = db::now_utc();
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM recommendation_items WHERE run_id = ?1", params![run_id])
        .map_err(|e| e.to_string())?;
    let mut stmt = tx
        .prepare(
            "SELECT p.id, COALESCE(p.total_score, 0) FROM papers p
             WHERE p.total_score IS NOT NULL AND p.is_ignored = 0
               AND NOT EXISTS (SELECT 1 FROM recommendation_items ri WHERE ri.paper_id = p.id)
             ORDER BY p.total_score DESC, p.published_date DESC, p.id DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?)))
        .map_err(|e| e.to_string())?;
    let mut rank: i64 = 1;
    for row in rows {
        let (pid, score) = row.map_err(|e| e.to_string())?;
        let _ = tx.execute(
            "INSERT OR IGNORE INTO recommendation_items (run_id, paper_id, rank, score_snapshot, added_at)
             VALUES (?1,?2,?3,?4,?5)",
            params![run_id, pid, rank, score, now_iso],
        );
        rank += 1;
    }
    drop(stmt);
    tx.commit().map_err(|e| e.to_string())?;
    Ok(run_id)
}

/// 读取 run 的 items 视图（join Paper 内容）。
pub fn run_items_with_papers(
    conn: &Connection,
    run_id: i64,
) -> Result<Vec<crate::models::RecommendationItemView>, String> {
    let _run = db::get_recommendation_run(conn, run_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "推荐周期不存在".to_string())?;
    let items = db::list_recommendation_items(conn, run_id).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for it in items {
        let paper = db::get_paper(conn, it.paper_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("论文 {} 不存在", it.paper_id))?;
        out.push(crate::models::RecommendationItemView {
            run_id: it.run_id,
            paper_id: it.paper_id,
            rank: it.rank,
            score_snapshot: it.score_snapshot,
            paper,
        });
    }
    // 保持 rank 顺序
    out.sort_by_key(|v| v.rank);
    Ok(out)
}

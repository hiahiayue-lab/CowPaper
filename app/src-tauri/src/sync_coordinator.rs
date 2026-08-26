//! 全局同步协调器：同一时间只允许一个同步任务运行。
//! 所有入口（manual / startup / daily / tray / journalTest）都必须经过 try_acquire。
//! `SyncGuard` 提供 RAII 释放：无论同步任务正常返回、提前 return 还是 panic/unwind，
//! running 状态最终都会被释放（panic-safe）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::models::{SyncStartResult, SyncTrigger};

pub struct SyncCoordinator {
    running: AtomicBool,
    current_trigger: Mutex<Option<SyncTrigger>>,
    started_at: Mutex<Option<String>>,
}

impl SyncCoordinator {
    pub fn new() -> Self {
        SyncCoordinator {
            running: AtomicBool::new(false),
            current_trigger: Mutex::new(None),
            started_at: Mutex::new(None),
        }
    }

    /// 尝试获取同步权。成功返回 Some(started_at)，已运行返回 None。
    pub fn try_acquire(&self, trigger: SyncTrigger) -> Option<String> {
        match self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => {
                let now = chrono::Utc::now().to_rfc3339();
                *self.current_trigger.lock().unwrap() = Some(trigger);
                *self.started_at.lock().unwrap() = Some(now.clone());
                Some(now)
            }
            Err(_) => None,
        }
    }

    /// 同步结束后释放锁（由 SyncGuard::drop 调用，保证 panic-safe）。
    pub fn release(&self) {
        self.running.store(false, Ordering::SeqCst);
        *self.current_trigger.lock().unwrap() = None;
        *self.started_at.lock().unwrap() = None;
    }

    #[allow(dead_code)] // 测试与调试使用
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// 当前状态（供 UI 判断“正在检查新论文”）。
    #[allow(dead_code)] // 测试与调试使用
    pub fn status(&self) -> SyncStartResult {
        if self.running.load(Ordering::SeqCst) {
            let trigger = self.current_trigger.lock().unwrap().map(|t| t.as_str().to_string());
            let started_at = self.started_at.lock().unwrap().clone();
            SyncStartResult {
                started: true,
                reason: "running".to_string(),
                trigger,
                started_at,
            }
        } else {
            SyncStartResult {
                started: false,
                reason: "idle".to_string(),
                trigger: None,
                started_at: None,
            }
        }
    }
}

/// RAII 释放守卫：Drop 时无条件调用 coordinator.release()。
/// 持有者被 unwind（panic）或正常 drop 时都会释放同步锁。
pub struct SyncGuard {
    coord: Arc<SyncCoordinator>,
}

impl SyncGuard {
    pub fn new(coord: Arc<SyncCoordinator>) -> Self {
        SyncGuard { coord }
    }
}

impl Drop for SyncGuard {
    fn drop(&mut self) {
        self.coord.release();
    }
}

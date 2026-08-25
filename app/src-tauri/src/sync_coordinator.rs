//! 全局同步协调器：同一时间只允许一个同步任务运行。
//! 所有入口（manual / startup / daily / tray / journalTest）都必须经过 try_acquire。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

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

    /// 同步结束后释放锁。
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

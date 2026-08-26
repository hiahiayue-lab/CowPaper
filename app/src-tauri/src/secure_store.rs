//! API Key 安全存储抽象。
//! 生产实现使用 macOS Keychain（keyring / Security framework）；
//! 测试使用内存 Mock。Key 绝不写入 SQLite / app_state / 日志。
//!
//! 命名空间隔离：生产使用 `production()` 的正式 service/account；
//! 测试必须使用完全独立的 test namespace（唯一后缀），
//! 绝不触碰 production credential。

#[cfg(test)]
use std::sync::Mutex;

/// 安全存储抽象：生产用 macOS Keychain，测试用 Mock。
pub trait SecureStore: Send + Sync {
    fn save(&self, key: &str) -> Result<(), String>;
    fn get(&self) -> Result<Option<String>, String>;
    fn delete(&self) -> Result<(), String>;
    fn has(&self) -> bool;
}

/// 生产命名空间（正式 CowPaper DeepSeek Key 所在）。
pub const PRODUCTION_SERVICE: &str = "com.cowpaper.app";
pub const PRODUCTION_ACCOUNT: &str = "deepseek_api_key";

/// macOS Keychain 实现（Security framework，经 keyring crate）。
/// service/account 可注入：生产用 production()，测试用唯一 test namespace。
pub struct KeychainStore {
    pub(crate) service: String,
    pub(crate) account: String,
}

impl KeychainStore {
    /// 正式命名空间：生产 App 保存/读取真实 DeepSeek Key。
    pub fn production() -> Self {
        KeychainStore {
            service: PRODUCTION_SERVICE.to_string(),
            account: PRODUCTION_ACCOUNT.to_string(),
        }
    }

    /// 测试命名空间：必须与 production service/account 完全不同。
    /// 仅测试构建使用（live/keychain 冒烟测试）。
    #[allow(dead_code)]
    pub fn with_namespace(service: &str, account: &str) -> Self {
        KeychainStore {
            service: service.to_string(),
            account: account.to_string(),
        }
    }

    fn entry(&self) -> Result<keyring::Entry, String> {
        keyring::Entry::new(&self.service, &self.account)
            .map_err(|e| format!("Keychain 打开失败: {}", e))
    }
}

impl SecureStore for KeychainStore {
    fn save(&self, key: &str) -> Result<(), String> {
        self.entry()?
            .set_password(key)
            .map_err(|e| format!("Keychain 写入失败: {}", e))
    }
    fn get(&self) -> Result<Option<String>, String> {
        match self.entry()?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("Keychain 读取失败: {}", e)),
        }
    }
    fn delete(&self) -> Result<(), String> {
        match self.entry()?.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("Keychain 删除失败: {}", e)),
        }
    }
    fn has(&self) -> bool {
        self.get().map(|o| o.is_some()).unwrap_or(false)
    }
}

/// 测试用内存实现（仅测试构建；不得用于生产）。
#[cfg(test)]
pub struct MockStore {
    inner: Mutex<Option<String>>,
}

#[cfg(test)]
impl MockStore {
    pub fn new() -> Self {
        MockStore {
            inner: Mutex::new(None),
        }
    }
    pub fn with_key(k: &str) -> Self {
        MockStore {
            inner: Mutex::new(Some(k.to_string())),
        }
    }
}

#[cfg(test)]
impl SecureStore for MockStore {
    fn save(&self, key: &str) -> Result<(), String> {
        *self.inner.lock().unwrap() = Some(key.to_string());
        Ok(())
    }
    fn get(&self) -> Result<Option<String>, String> {
        Ok(self.inner.lock().unwrap().clone())
    }
    fn delete(&self) -> Result<(), String> {
        *self.inner.lock().unwrap() = None;
        Ok(())
    }
    fn has(&self) -> bool {
        self.inner.lock().unwrap().is_some()
    }
}

/// 清理守卫：即使测试 panic 也会删除自己创建的 test credential。
#[cfg(test)]
struct TestCleanup {
    service: String,
    account: String,
}

#[cfg(test)]
impl Drop for TestCleanup {
    fn drop(&mut self) {
        let store = KeychainStore::with_namespace(&self.service, &self.account);
        let _ = store.delete();
    }
}

/// 真实 Keychain 冒烟测试（ignored，需钥匙串访问）：
/// 使用唯一 test namespace（com.cowpaper.test.<unique>），
/// 保存/读取/删除测试值，绝不触碰 production namespace。
#[cfg(test)]
pub fn keychain_smoke() -> Result<String, String> {
    let unique = format!(
        "{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    );
    let service = format!("com.cowpaper.test.{}", unique);
    let account = "cowpaper-test-credential";
    debug_assert_ne!(service, PRODUCTION_SERVICE);
    debug_assert_ne!(account, PRODUCTION_ACCOUNT);

    let store = KeychainStore::with_namespace(&service, &account);
    let _cleanup = TestCleanup {
        service: service.clone(),
        account: account.to_string(),
    };

    let test_value = "cowpaper-keychain-smoke";
    store.save(test_value)?;
    let got = store.get()?.unwrap_or_default();
    store.delete()?;
    if got == test_value {
        Ok(format!(
            "真实 macOS Keychain save/get/delete 验证通过（独立 test namespace: {}）",
            service
        ))
    } else {
        Err("Keychain 读写值不一致".to_string())
    }
}

//! API Key 安全存储抽象。
//! 生产实现使用 macOS Keychain（keyring / Security framework）；
//! 测试使用内存 Mock。Key 绝不写入 SQLite / app_state / 日志。

#[cfg(test)]
use std::sync::Mutex;

/// 安全存储抽象：生产用 macOS Keychain，测试用 Mock。
pub trait SecureStore: Send + Sync {
    fn save(&self, key: &str) -> Result<(), String>;
    fn get(&self) -> Result<Option<String>, String>;
    fn delete(&self) -> Result<(), String>;
    fn has(&self) -> bool;
}

const KEYCHAIN_SERVICE: &str = "com.cowpaper.app";
const KEYCHAIN_ACCOUNT: &str = "deepseek_api_key";

/// macOS Keychain 实现（Security framework，经 keyring crate）。
pub struct KeychainStore;

impl KeychainStore {
    pub fn new() -> Self {
        KeychainStore
    }
    fn entry(&self) -> Result<keyring::Entry, String> {
        keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
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

/// 真实 Keychain 冒烟测试（ignored）：save/get/has/delete 一个测试值，验证真实钥匙串可用。
#[cfg(test)]
pub fn keychain_smoke() -> Result<String, String> {
    let store = KeychainStore::new();
    let test_value = "cowpaper-keychain-smoke";
    store.save(test_value)?;
    let got = store.get()?.unwrap_or_default();
    store.delete()?;
    if got == test_value {
        Ok("真实 macOS Keychain save/get/delete 验证通过".to_string())
    } else {
        Err("Keychain 读写值不一致".to_string())
    }
}

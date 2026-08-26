//! API Key 本地文件存储（Round 5A.1）。
//!
//! 已停止使用 macOS Keychain：避免系统不定期弹出钥匙串授权弹窗。
//! 生产实现 `LocalFileSecretStore` 把 Key 保存在本地 JSON 文件
//! （Application Support/CowPaper/secrets.json），目录 0700、文件 0600，
//! 通过 temp 文件 + fsync + atomic rename 写入，避免崩溃产生半截 JSON。
//!
//! Key 绝不写入 SQLite / app_state / localStorage / 日志。
//! 前端只能调用 save/has/delete/test 命令，无法读取完整 Key（无 get 命令）。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// 安全存储抽象：生产用本地文件，测试用临时目录/内存。
pub trait SecureStore: Send + Sync {
    fn save(&self, key: &str) -> Result<(), String>;
    fn get(&self) -> Result<Option<String>, String>;
    fn delete(&self) -> Result<(), String>;
    fn has(&self) -> bool;
}

const SECRETS_FILENAME: &str = "secrets.json";
const KEY_FIELD: &str = "deepseek_api_key";

/// 本地 secret 文件存储（production）。
pub struct LocalFileSecretStore {
    file: PathBuf,
}

impl LocalFileSecretStore {
    pub fn new(dir: &Path) -> Self {
        LocalFileSecretStore {
            file: dir.join(SECRETS_FILENAME),
        }
    }

    fn load_map(&self) -> Result<serde_json::Map<String, serde_json::Value>, String> {
        match fs::read(&self.file) {
            Ok(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|v| v.as_object().cloned())
                .ok_or_else(|| "secret 文件损坏或格式无效".to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::Map::new()),
            Err(e) => Err(format!("读取 secret 文件失败: {}", e)),
        }
    }

    fn write_map(&self, map: &serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
        let dir = self
            .file
            .parent()
            .ok_or_else(|| "secret 文件无父目录".to_string())?;
        fs::create_dir_all(dir).map_err(|e| format!("创建目录失败: {}", e))?;
        // 目录权限 0700（仅当前用户）
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
        }
        // 安全写入：temp 文件 → 权限 0600 → flush + sync → atomic rename
        let tmp = self.file.with_extension("tmp");
        {
            let mut f = fs::File::create(&tmp).map_err(|e| format!("创建临时文件失败: {}", e))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = f.set_permissions(fs::Permissions::from_mode(0o600));
            }
            serde_json::to_writer(&mut f, map).map_err(|e| format!("写入 secret 失败: {}", e))?;
            f.flush().map_err(|e| format!("flush 失败: {}", e))?;
            f.sync_all().map_err(|e| format!("sync 失败: {}", e))?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
        }
        fs::rename(&tmp, &self.file).map_err(|e| format!("原子替换 secret 失败: {}", e))?;
        Ok(())
    }
}

impl SecureStore for LocalFileSecretStore {
    fn save(&self, key: &str) -> Result<(), String> {
        // 文件损坏时从空 map 重建（允许用户重新保存恢复），不静默丢 Key
        let mut map = self.load_map().unwrap_or_default();
        map.insert(KEY_FIELD.to_string(), serde_json::Value::String(key.to_string()));
        self.write_map(&map)
    }
    fn get(&self) -> Result<Option<String>, String> {
        let map = self.load_map()?;
        Ok(map
            .get(KEY_FIELD)
            .and_then(|v| v.as_str())
            .map(str::to_string))
    }
    fn delete(&self) -> Result<(), String> {
        let mut map = self.load_map().unwrap_or_default();
        map.remove(KEY_FIELD);
        self.write_map(&map)
    }
    fn has(&self) -> bool {
        self.get().map(|o| o.is_some()).unwrap_or(false)
    }
}

/// 测试用临时目录实现（绝不触碰用户真实目录）。
/// 保存目录以便 restart 测试用同一路径重新打开。
#[cfg(test)]
pub struct TempDirSecretStore {
    inner: LocalFileSecretStore,
    dir: PathBuf,
}

#[cfg(test)]
impl TempDirSecretStore {
    pub fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "cowpaper-secrets-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        Self::new_in(&dir)
    }

    pub fn new_in(dir: &Path) -> Self {
        let _ = fs::create_dir_all(dir);
        TempDirSecretStore {
            inner: LocalFileSecretStore::new(dir),
            dir: dir.to_path_buf(),
        }
    }

}

#[cfg(test)]
impl Default for TempDirSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl SecureStore for TempDirSecretStore {
    fn save(&self, key: &str) -> Result<(), String> {
        self.inner.save(key)
    }
    fn get(&self) -> Result<Option<String>, String> {
        self.inner.get()
    }
    fn delete(&self) -> Result<(), String> {
        self.inner.delete()
    }
    fn has(&self) -> bool {
        self.inner.has()
    }
}

/// 测试用内存实现（仅测试构建；ai_queue 测试使用，不落盘）。
#[cfg(test)]
pub struct MockStore {
    inner: std::sync::Mutex<Option<String>>,
}

#[cfg(test)]
impl MockStore {
    pub fn new() -> Self {
        MockStore {
            inner: std::sync::Mutex::new(None),
        }
    }
    pub fn with_key(k: &str) -> Self {
        MockStore {
            inner: std::sync::Mutex::new(Some(k.to_string())),
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

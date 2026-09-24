//! 应用运行态：路径、配置、索引、图标缓存、使用统计。
//!
//! 所有可变状态都放在 `AppState` 里通过 Tauri 的 `State` 注入命令，
//! 避免使用全局可变变量。锁的持有范围严格限制在单个表达式内，
//! 绝不在持锁期间调用 `window.hide()` 之类的 UI 操作，防止与主线程互等。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::{self, Config};
use crate::model::LauncherItem;
use crate::search::IndexedItem;

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 应用数据目录下的各文件位置。
pub struct Paths {
    pub dir: PathBuf,
    pub config: PathBuf,
    pub index_cache: PathBuf,
    pub usage: PathBuf,
}

impl Paths {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            config: dir.join("config.json"),
            index_cache: dir.join("index-cache.json"),
            usage: dir.join("usage.json"),
            dir,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageEntry {
    pub count: u32,
    pub last_used_at: u64,
}

/// 使用记录。
///
/// PRD 8.2 建议用 SQLite。这里改为单文件 JSON：条目量级在 10^3 以内，
/// 全量读入内存仅几十 KB，读写都是 O(1) 内存操作 + 一次落盘，
/// 且省掉 `rusqlite`（bundled 需要本地 C 工具链）带来的构建风险。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub entries: HashMap<String, UsageEntry>,
}

impl Usage {
    pub fn load(path: &PathBuf) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &PathBuf) {
        if let Ok(bytes) = serde_json::to_vec(self) {
            let _ = config::write_atomic(path, &bytes);
        }
    }

    pub fn record(&mut self, id: &str, at: u64) {
        let entry = self.entries.entry(id.to_string()).or_default();
        entry.count = entry.count.saturating_add(1);
        entry.last_used_at = at;
    }

    pub fn get(&self, id: &str) -> Option<&UsageEntry> {
        self.entries.get(id)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// 最近使用过的条目 ID，按时间倒序。
    pub fn recent_ids(&self, limit: usize) -> Vec<String> {
        let mut all: Vec<(&String, &UsageEntry)> = self.entries.iter().collect();
        all.sort_by(|a, b| b.1.last_used_at.cmp(&a.1.last_used_at));
        all.into_iter().take(limit).map(|(id, _)| id.clone()).collect()
    }
}

pub struct AppState {
    pub paths: Paths,
    pub config: Mutex<Config>,
    /// 最近一次扫描得到的应用列表（尚未与自定义条目合并）
    pub scan_cache: Mutex<Vec<LauncherItem>>,
    /// 扫描结果 + 预计算的搜索键（小写、拼音、首字母）
    pub index: RwLock<Vec<IndexedItem>>,
    /// ICNS 解析结果缓存：id -> data URL（None 表示确认无可用图标）
    pub icons: Mutex<HashMap<String, Option<String>>>,
    pub usage: Mutex<Usage>,
    pub scanning: AtomicBool,
    pub last_scan_at: Mutex<Option<u64>>,
}

impl AppState {
    pub fn new(paths: Paths) -> Self {
        let config = Config::load(&paths.config);
        let usage = Usage::load(&paths.usage);
        Self {
            paths,
            config: Mutex::new(config),
            scan_cache: Mutex::new(Vec::new()),
            index: RwLock::new(Vec::new()),
            icons: Mutex::new(HashMap::new()),
            usage: Mutex::new(usage),
            scanning: AtomicBool::new(false),
            last_scan_at: Mutex::new(None),
        }
    }

    pub fn config_snapshot(&self) -> Config {
        self.config.lock().map(|c| c.clone()).unwrap_or_default()
    }

    pub fn is_scanning(&self) -> bool {
        self.scanning.load(Ordering::SeqCst)
    }

    /// 保存配置到磁盘（调用方需先更新 `self.config`）。
    pub fn persist_config(&self) {
        if let Ok(cfg) = self.config.lock() {
            if let Err(err) = cfg.save(&self.paths.config) {
                eprintln!("[quicklaunch] 配置写入失败: {err}");
            }
        }
    }

    pub fn persist_usage(&self) {
        if let Ok(usage) = self.usage.lock() {
            usage.save(&self.paths.usage);
        }
    }

    pub fn drop_icon_cache(&self) {
        if let Ok(mut cache) = self.icons.lock() {
            cache.clear();
        }
    }
}

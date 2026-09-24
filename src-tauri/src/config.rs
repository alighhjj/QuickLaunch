//! 配置的加载、校验与原子落盘。
//!
//! 落盘策略参考 PRD 5.3：先写同目录 `.tmp` 再 `rename` 覆盖，
//! `rename` 在同一文件系统内是原子操作，避免进程中断写出半个 JSON。
//! 若磁盘上的配置已损坏，则把原文件改名备份并回落到默认值，保证应用仍可用。

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::ItemKind;

pub const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralConfig {
    pub launch_at_login: bool,
    pub hide_on_blur: bool,
    pub max_results: usize,
    /// 是否在 Dock 中显示图标。关闭后应用仅驻留菜单栏，
    /// 但 macOS 在 Accessory 激活策略下的取焦行为不稳定，故默认开启。
    pub show_in_dock: bool,
    /// 执行自定义命令前是否强制二次确认
    pub command_confirm: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            launch_at_login: false,
            hide_on_blur: true,
            max_results: 20,
            show_in_dock: true,
            command_confirm: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceConfig {
    /// system | light | dark
    pub theme: String,
    /// 面板背景不透明度 0.4 ~ 1.0
    pub opacity: f32,
    /// small | medium | large
    pub icon_size: String,
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            opacity: 0.82,
            icon_size: "medium".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutConfig {
    pub toggle: String,
}

impl Default for ShortcutConfig {
    fn default() -> Self {
        Self {
            toggle: "Option+Space".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomItem {
    pub id: String,
    pub kind: ItemKind,
    pub name: String,
    /// 文件夹路径 / URL / shell 命令
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default)]
    pub confirm: bool,
    #[serde(default)]
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub version: u32,
    pub general: GeneralConfig,
    pub appearance: AppearanceConfig,
    pub shortcut: ShortcutConfig,
    pub scan_paths: Vec<String>,
    pub exclude_patterns: Vec<String>,
    pub custom_items: Vec<CustomItem>,
    /// 收藏：排序加 1000 分
    pub favorites: Vec<String>,
    /// 置顶：排序加 1500 分，优先于收藏
    pub pinned: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            general: GeneralConfig::default(),
            appearance: AppearanceConfig::default(),
            shortcut: ShortcutConfig::default(),
            scan_paths: vec![
                "/Applications".into(),
                "/System/Applications".into(),
                "~/Applications".into(),
            ],
            exclude_patterns: vec!["*.prefPane".into()],
            custom_items: Vec::new(),
            favorites: Vec::new(),
            pinned: Vec::new(),
        }
    }
}

impl Config {
    /// 从磁盘读取；文件不存在时返回默认配置，损坏时备份并回落默认配置。
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Config>(&text) {
                Ok(mut cfg) => {
                    cfg.normalize();
                    cfg
                }
                Err(err) => {
                    eprintln!("[quicklaunch] 配置解析失败（{err}），已回落到默认配置");
                    let backup = path.with_extension("json.broken");
                    let _ = std::fs::rename(path, &backup);
                    Config::default()
                }
            },
            Err(_) => Config::default(),
        }
    }

    /// 修正越界值、去重、剔除空项。
    pub fn normalize(&mut self) {
        self.general.max_results = self.general.max_results.clamp(5, 60);
        self.appearance.opacity = self.appearance.opacity.clamp(0.4, 1.0);
        if !matches!(self.appearance.theme.as_str(), "system" | "light" | "dark") {
            self.appearance.theme = "system".into();
        }
        if !matches!(self.appearance.icon_size.as_str(), "small" | "medium" | "large") {
            self.appearance.icon_size = "medium".into();
        }
        if self.shortcut.toggle.trim().is_empty() {
            self.shortcut.toggle = ShortcutConfig::default().toggle;
        }
        dedupe(&mut self.scan_paths);
        dedupe(&mut self.exclude_patterns);
        dedupe(&mut self.favorites);
        dedupe(&mut self.pinned);
        // 同一个条目不应同时出现在置顶和收藏里，置顶优先。
        let pinned = self.pinned.clone();
        self.favorites.retain(|id| !pinned.contains(id));
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(to_io)?;
        write_atomic(path, &bytes)
    }
}

fn dedupe(list: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    list.retain(|s| {
        let t = s.trim();
        !t.is_empty() && seen.insert(t.to_string())
    });
    for s in list.iter_mut() {
        *s = s.trim().to_string();
    }
}

fn to_io(err: serde_json::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, err)
}

/// 原子写入：临时文件 → fsync → rename。
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// 展开 `~` 与 `~/xxx`，并规范化路径分隔。
pub fn expand_path(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if trimmed == "~" {
        if let Some(home) = home_dir() {
            return home;
        }
    }
    if let Some(rest) = trimmed.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(trimmed)
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// 把绝对路径压回 `~/...` 形式展示，列表里更短更易读。
pub fn prettify_path(path: &Path) -> String {
    if let Some(home) = home_dir() {
        if let Ok(rest) = path.strip_prefix(&home) {
            return format!("~/{}", rest.display());
        }
    }
    path.display().to_string()
}

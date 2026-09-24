//! 领域模型：条目、搜索结果、启动回报、运行状态。
//!
//! 这些结构体同时是 IPC 契约，字段命名统一为 camelCase 以便前端直接消费。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemKind {
    App,
    Folder,
    Url,
    Command,
}

/// 可被启动的条目。
///
/// 刻意不携带 base64 图标：图标体量大，若随搜索结果下发，每次按键都要序列化
/// 上百 KB，既拖慢 IPC 也推高内存（PRD 5.2）。改为前端按需调用 `get_icon`。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherItem {
    pub id: String,
    pub name: String,
    pub kind: ItemKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundle_id: Option<String>,
    /// 应用包路径 / 文件夹路径 / URL / shell 命令，取决于 `kind`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// 应用图标 .icns 的绝对路径（内部字段，前端不用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_file: Option<String>,
    /// 执行自定义命令时的工作目录
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// 内置扫描条目为 true，用户自定义条目为 false
    pub builtin: bool,
    /// 自定义命令是否需要执行前二次确认
    #[serde(default)]
    pub confirm: bool,
}

impl LauncherItem {
    /// 稳定 ID：内置应用用 bundle id，自定义条目用 `custom-<n>`。
    pub fn app_id(bundle_id: &str) -> String {
        format!("app-{bundle_id}")
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub item: LauncherItem,
    pub score: i64,
    /// 命中区间 `[起始字符序号, 长度]`，相对于 `item.name`，用于前端高亮。
    /// 拼音命中与路径命中无法映射回原文字符位置，此时为空数组。
    pub highlights: Vec<[u32; 2]>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchReport {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub version: String,
    pub item_count: usize,
    pub app_count: usize,
    pub custom_count: usize,
    pub scanning: bool,
    pub last_scan_at: Option<u64>,
    pub autostart: bool,
}

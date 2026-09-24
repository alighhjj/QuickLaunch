//! 文件系统扫描：遍历扫描根目录，识别 `.app` 包并解析 `Info.plist`。
//!
//! 两点设计取舍：
//! 1. 命中 `.app` 后立刻停止下探，因此不会把 Xcode、Chrome 内部那几百个
//!    辅助进程包（Helper.app）当成可启动应用收集进来。
//! 2. 单个目录读取失败只跳过该目录，不中断整体扫描 —— PRD 5.3 要求
//!    「扫描失败不影响核心启动功能」（例如未授予完全磁盘访问权限的系统目录）。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::config::{expand_path, prettify_path};
use crate::model::{ItemKind, LauncherItem};

/// 相对扫描根目录的最大下探层数，用于覆盖 `/Applications/Utilities/` 这类子目录。
const MAX_DEPTH: usize = 3;

#[derive(Debug)]
pub struct ScanOutcome {
    pub items: Vec<LauncherItem>,
    pub scanned_roots: Vec<String>,
    pub errors: Vec<String>,
}

pub fn scan_all(roots: &[String], excludes: &[String]) -> ScanOutcome {
    let mut items: Vec<LauncherItem> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut scanned_roots: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for raw in roots {
        let root = expand_path(raw);
        if !root.is_dir() {
            errors.push(format!("扫描路径不存在或不可读：{}", root.display()));
            continue;
        }
        scanned_roots.push(prettify_path(&root));

        let mut stack: Vec<(PathBuf, usize)> = vec![(root, 0)];
        while let Some((dir, depth)) = stack.pop() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(entries) => entries,
                // 权限不足等情况：跳过该目录，继续处理其他分支
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if name.starts_with('.') {
                    continue;
                }
                if is_excluded(name, &path, excludes) {
                    continue;
                }
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if !is_dir {
                    continue;
                }
                if name.ends_with(".app") {
                    if let Some(item) = read_app_bundle(&path) {
                        if seen.insert(item.id.clone()) {
                            items.push(item);
                        }
                    }
                    continue;
                }
                if depth + 1 < MAX_DEPTH {
                    stack.push((path, depth + 1));
                }
            }
        }
    }

    items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    ScanOutcome {
        items,
        scanned_roots,
        errors,
    }
}

fn is_excluded(name: &str, path: &Path, excludes: &[String]) -> bool {
    let full = path.to_string_lossy();
    excludes
        .iter()
        .any(|pattern| glob_match(pattern, name) || glob_match(pattern, &full))
}

/// 解析 `.app` 包。缺少 `CFBundleIdentifier` 的目录直接丢弃 ——
/// 那种目录通常是残留物，启动价值低于它在列表中造成的噪声。
fn read_app_bundle(bundle: &Path) -> Option<LauncherItem> {
    let contents = bundle.join("Contents");
    let plist_value = plist::Value::from_file(contents.join("Info.plist")).ok()?;
    let dict = plist_value.as_dictionary()?;

    let get = |key: &str| -> Option<String> {
        match dict.get(key) {
            Some(plist::Value::String(s)) if !s.trim().is_empty() => Some(s.clone()),
            _ => None,
        }
    };

    let bundle_id = get("CFBundleIdentifier")?;
    let fallback_name = bundle
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| bundle_id.clone());
    let name = get("CFBundleDisplayName")
        .or_else(|| get("CFBundleName"))
        .unwrap_or(fallback_name);

    Some(LauncherItem {
        id: LauncherItem::app_id(&bundle_id),
        name,
        kind: ItemKind::App,
        subtitle: Some(prettify_path(bundle)),
        bundle_id: Some(bundle_id),
        target: Some(bundle.to_string_lossy().into_owned()),
        icon_file: resolve_icon(&contents, get("CFBundleIconFile").as_deref()),
        cwd: None,
        keywords: Vec::new(),
        builtin: true,
        confirm: false,
    })
}

/// 依次尝试：`Info.plist` 声明的图标名 → 常见约定名 → `Resources` 下任意 `.icns`。
fn resolve_icon(contents: &Path, hint: Option<&str>) -> Option<String> {
    let resources = contents.join("Resources");

    if let Some(hint) = hint.map(str::trim).filter(|h| !h.is_empty()) {
        let direct = resources.join(hint);
        if direct.is_file() {
            return Some(direct.to_string_lossy().into_owned());
        }
        if !hint.to_ascii_lowercase().ends_with(".icns") {
            let with_ext = resources.join(format!("{hint}.icns"));
            if with_ext.is_file() {
                return Some(with_ext.to_string_lossy().into_owned());
            }
        }
    }

    for candidate in ["AppIcon.icns", "app.icns"] {
        let path = resources.join(candidate);
        if path.is_file() {
            return Some(path.to_string_lossy().into_owned());
        }
    }

    let entries = std::fs::read_dir(&resources).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if file_name.to_ascii_lowercase().ends_with(".icns") && !file_name.starts_with('_') {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    None
}

/// 极简 glob：仅支持 `*` 与 `?`，大小写不敏感。
/// 配置里的排除项（如 `*.prefPane`）都是这种简单形态，无需引入完整 glob 依赖。
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    glob_here(&p, &t)
}

fn glob_here(pattern: &[char], text: &[char]) -> bool {
    match pattern.first() {
        None => text.is_empty(),
        Some('*') => (0..=text.len()).any(|skip| glob_here(&pattern[1..], &text[skip..])),
        Some('?') => !text.is_empty() && glob_here(&pattern[1..], &text[1..]),
        Some(c) => !text.is_empty() && text[0] == *c && glob_here(&pattern[1..], &text[1..]),
    }
}

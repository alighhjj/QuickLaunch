//! IPC 命令层：前端能触达的全部后端能力都在这里。
//!
//! 两条硬约束：
//! 1. 副作用（隐藏窗口、改激活策略）一律放在锁的作用域之外调用，避免主线程互等。
//! 2. 自定义命令必须带 `confirmed = true` 才会真正执行（PRD F-27 / 5.4），
//!    确认逻辑在后端强制执行，前端弹窗只是它的入口，绕过前端也执行不了。

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};
use tauri_plugin_notification::NotificationExt;

use crate::config::{self, Config, CustomItem};
use crate::icons;
use crate::model::{ItemKind, LaunchReport, LauncherItem, SearchHit, Status};
use crate::scanner;
use crate::search::{self, IndexedItem};
use crate::state::{now_secs, AppState};

// ─────────────────────────── 查询类命令 ───────────────────────────

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> Config {
    state.config_snapshot()
}

#[tauri::command]
pub fn search_items(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Vec<SearchHit> {
    let config = state.config_snapshot();
    let max = limit
        .unwrap_or(config.general.max_results)
        .clamp(1, 60);

    let Ok(index) = state.index.read() else {
        return Vec::new();
    };
    let Ok(usage) = state.usage.lock() else {
        return Vec::new();
    };
    search::search(&index, &config, &usage, &query, max)
}

/// 按需拉取单个条目的图标。图标体积大，绝不随搜索结果批量下发。
#[tauri::command]
pub fn get_icon(state: State<'_, AppState>, id: String) -> Option<String> {
    if let Ok(cache) = state.icons.lock() {
        if let Some(hit) = cache.get(&id) {
            return hit.clone();
        }
    }

    // 故意的两次加锁：中间的文件读取不持锁
    let icon_file = {
        let Ok(index) = state.index.read() else {
            return None;
        };
        index
            .iter()
            .find(|entry| entry.item.id == id)
            .and_then(|entry| entry.item.icon_file.clone())
    };
    let resolved = icon_file
        .as_deref()
        .and_then(|path| icons::icns_to_data_url(Path::new(path)));

    if let Ok(mut cache) = state.icons.lock() {
        cache.insert(id, resolved.clone());
    }
    resolved
}

#[tauri::command]
pub fn status(app: AppHandle, state: State<'_, AppState>) -> Status {
    let (item_count, app_count, custom_count) = match state.index.read() {
        Ok(index) => {
            let apps = index.iter().filter(|e| e.item.builtin).count();
            (index.len(), apps, index.len() - apps)
        }
        Err(_) => (0, 0, 0),
    };
    let last_scan_at = state.last_scan_at.lock().ok().and_then(|v| *v);

    Status {
        version: app.package_info().version.to_string(),
        item_count,
        app_count,
        custom_count,
        scanning: state.is_scanning(),
        last_scan_at,
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
    }
}

#[tauri::command]
pub fn validate_shortcut(accelerator: String) -> Result<String, String> {
    parse_shortcut(&accelerator).map(|sc| format!("{sc:?}"))
}

#[tauri::command]
pub fn validate_target(kind: ItemKind, target: String) -> Result<(), String> {
    let target = target.trim();
    if target.is_empty() {
        return Err("目标不能为空".into());
    }
    match kind {
        ItemKind::Folder => {
            let path = config::expand_path(target);
            if !path.is_dir() {
                return Err(format!("目录不存在：{}", path.display()));
            }
        }
        ItemKind::Url => {
            let lower = target.to_ascii_lowercase();
            if !lower.starts_with("http://") && !lower.starts_with("https://") {
                return Err("网址需以 http:// 或 https:// 开头".into());
            }
        }
        ItemKind::Command => {}
        ItemKind::App => {
            let path = config::expand_path(target);
            if !path.is_dir() {
                return Err(format!("应用包不存在：{}", path.display()));
            }
        }
    }
    Ok(())
}

// ─────────────────────────── 配置写入 ───────────────────────────

#[tauri::command]
pub fn save_config(
    app: AppHandle,
    state: State<'_, AppState>,
    config: Config,
) -> Result<Config, String> {
    apply_config(&app, &state, config)
}

/// 校验 → 应用外部副作用 → 落盘 → 重算索引。
///
/// 顺序是刻意的：快捷键注册与开机自启都属于「可能失败且失败后必须不留痕」
/// 的操作，所以先做它们；任何一步失败就直接返回错误，配置不落盘，
/// 用户会被明确告知这次修改没生效，而不会在下次启动时才发现快捷键已经失效。
fn apply_config(
    app: &AppHandle,
    state: &AppState,
    mut next: Config,
) -> Result<Config, String> {
    next.version = config::CONFIG_VERSION;
    next.normalize();

    let previous = state.config_snapshot();

    if previous.shortcut.toggle != next.shortcut.toggle {
        apply_shortcut(app, &next.shortcut.toggle, Some(&previous.shortcut.toggle))?;
    }

    if previous.general.launch_at_login != next.general.launch_at_login {
        let manager = app.autolaunch();
        let outcome = if next.general.launch_at_login {
            manager.enable()
        } else {
            manager.disable()
        };
        if let Err(err) = outcome {
            let _ = manager.disable();
            return Err(format!("开机自启设置失败：{err}"));
        }
    }

    {
        let mut guard = state
            .config
            .lock()
            .map_err(|_| "配置状态不可用".to_string())?;
        *guard = next.clone();
    }
    state.persist_config();

    if previous.general.show_in_dock != next.general.show_in_dock {
        apply_activation_policy(app, next.general.show_in_dock);
    }

    rebuild_index(state, &next);
    Ok(next)
}

#[tauri::command]
pub fn export_config(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let target = PathBuf::from(path);
    let config = state.config_snapshot();
    config
        .save(&target)
        .map_err(|err| format!("导出失败：{err}"))?;
    Ok(target.display().to_string())
}

#[tauri::command]
pub fn import_config(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<Config, String> {
    let text = std::fs::read_to_string(&path).map_err(|err| format!("读取配置失败：{err}"))?;
    let imported: Config =
        serde_json::from_str(&text).map_err(|err| format!("配置格式不合法：{err}"))?;
    apply_config(&app, &state, imported)
}

#[tauri::command]
pub fn clear_usage(state: State<'_, AppState>) -> Result<(), String> {
    {
        let mut usage = state
            .usage
            .lock()
            .map_err(|_| "使用记录不可用".to_string())?;
        usage.clear();
    }
    state.persist_usage();
    Ok(())
}

#[tauri::command]
pub fn toggle_favorite(state: State<'_, AppState>, id: String) -> Result<Config, String> {
    let mut config = state.config_snapshot();
    if let Some(position) = config.favorites.iter().position(|x| x == &id) {
        config.favorites.remove(position);
    } else {
        config.favorites.push(id);
    }
    config.normalize();
    {
        let mut guard = state
            .config
            .lock()
            .map_err(|_| "配置状态不可用".to_string())?;
        *guard = config.clone();
    }
    state.persist_config();
    Ok(config)
}

// ─────────────────────────── 启动条目 ───────────────────────────

#[tauri::command]
pub fn launch(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    confirmed: bool,
) -> Result<LaunchReport, String> {
    let item = {
        let Ok(index) = state.index.read() else {
            return Err("索引不可用".into());
        };
        index
            .iter()
            .find(|entry| entry.item.id == id)
            .map(|entry| entry.item.clone())
    };
    let Some(item) = item else {
        return Err("条目已不存在，请重新扫描".into());
    };

    let config = state.config_snapshot();
    let needs_confirm = item.kind == ItemKind::Command
        && (item.confirm || config.general.command_confirm);
    if needs_confirm && !confirmed {
        return Ok(LaunchReport {
            ok: false,
            message: "该命令需要确认后才能执行".into(),
        });
    }

    let target = item.target.clone().unwrap_or_default();
    let result = match item.kind {
        ItemKind::App => open_with_system(&target, true),
        ItemKind::Folder => open_with_system(&target, false),
        ItemKind::Url => open_with_system(&normalize_url(&target), false),
        ItemKind::Command => run_shell_command(&app, &item),
    };

    match result {
        Ok(message) => {
            if let Ok(mut usage) = state.usage.lock() {
                usage.record(&id, now_secs());
            }
            state.persist_usage();
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.hide();
            }
            Ok(LaunchReport { ok: true, message })
        }
        Err(err) => {
            notify(&app, "启动失败", &format!("「{}」{err}", item.name));
            Err(err)
        }
    }
}

fn normalize_url(raw: &str) -> String {
    let trimmed = raw.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    }
}

/// 交给系统的 `open` 处理。
///
/// 这里刻意不用 shell：参数以 argv 直接传给 `/usr/bin/open`，
/// 因此路径里的空格、引号、`;` 都不可能被解释成命令 —— 天然免疫注入。
fn open_with_system(target: &str, is_app_bundle: bool) -> Result<String, String> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return Err("目标为空".into());
    }
    if trimmed.starts_with('-') {
        // 防止被当成 `open` 的选项
        return Err("目标以 '-' 开头，已拒绝执行".into());
    }

    let mut command = std::process::Command::new("/usr/bin/open");
    if is_app_bundle {
        command.arg("-a");
    }
    command
        .arg(trimmed)
        .spawn()
        .map(|_| format!("已打开 {trimmed}"))
        .map_err(|err| format!("调用 open 失败：{err}"))
}

/// 执行用户自定义命令。
///
/// 命令原文按设计就是交给 `/bin/sh -lc` 的，这是功能本身而非漏洞；
/// 安全边界由「执行前确认」（默认开启）与用户的完全授权共同构成，
/// 详见 docs/DECISIONS.md。
fn run_shell_command(app: &AppHandle, item: &LauncherItem) -> Result<String, String> {
    let script = item.target.as_deref().unwrap_or("").trim();
    if script.is_empty() {
        return Err("命令为空".into());
    }

    let mut command = std::process::Command::new("/bin/sh");
    command.arg("-lc").arg(script);
    if let Some(raw_cwd) = item.cwd.as_deref().filter(|c| !c.trim().is_empty()) {
        let cwd = config::expand_path(raw_cwd);
        if !cwd.is_dir() {
            return Err(format!("工作目录不存在：{}", cwd.display()));
        }
        command.current_dir(cwd);
    }

    let mut child = command
        .spawn()
        .map_err(|err| format!("命令启动失败：{err}"))?;

    // 不阻塞等待：命令可能是长驻进程（如 dev server）。
    // 另起线程回收退出码，仅在失败时发系统通知（PRD F-30）。
    let handle = app.clone();
    let name = item.name.clone();
    std::thread::spawn(move || match child.wait() {
        Ok(status) if status.success() => {}
        Ok(status) => notify(
            &handle,
            "命令执行失败",
            &format!(
                "「{name}」退出码 {}",
                status.code().map(|c| c.to_string()).unwrap_or_else(|| "未知".into())
            ),
        ),
        Err(err) => notify(&handle, "命令执行异常", &format!("「{name}」{err}")),
    });

    Ok("命令已执行".into())
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    let _ = app
        .notification()
        .builder()
        .title(title)
        .body(body)
        .show();
}

// ─────────────────────────── 窗口与扫描 ───────────────────────────

#[tauri::command]
pub fn hide_main(app: AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

#[tauri::command]
pub fn toggle_main(app: AppHandle) {
    toggle_main_window(&app);
}

#[tauri::command]
pub fn open_settings(app: AppHandle) -> Result<(), String> {
    open_settings_window(&app)
}

#[tauri::command]
pub fn rescan(app: AppHandle) {
    spawn_scan(app);
}

#[tauri::command]
pub fn open_data_dir(app: AppHandle) {
    let state = app.state::<AppState>();
    let dir = state.paths.dir.clone();
    drop(state);
    let _ = open_with_system(&dir.to_string_lossy(), false);
}

pub fn toggle_main_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    } else {
        reveal_main(app);
    }
}

pub fn reveal_main(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    position_on_cursor_screen(app, &window);
    let _ = window.show();
    let _ = window.set_focus();
    // 让前端把输入框清空并回到空状态
    let _ = app.emit("launcher-shown", ());
}

/// 在鼠标所在的显示器上居中（PRD F-05）。
fn position_on_cursor_screen(app: &AppHandle, window: &tauri::WebviewWindow) {
    let Ok(cursor) = app.cursor_position() else {
        return;
    };
    let Ok(Some(monitor)) = app.monitor_from_point(cursor.x, cursor.y) else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };
    let screen = monitor.size();
    let origin = monitor.position();
    let x = origin.x as f64 + (screen.width as f64 - size.width as f64) / 2.0;
    let y = origin.y as f64 + (screen.height as f64 - size.height as f64) / 2.0;
    let _ = window.set_position(tauri::PhysicalPosition::new(
        x.round() as i32,
        y.round() as i32,
    ));
}

pub fn open_settings_window(app: &AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window("settings") {
        let _ = existing.show();
        let _ = existing.set_focus();
        return Ok(());
    }
    tauri::WebviewWindowBuilder::new(
        app,
        "settings",
        tauri::WebviewUrl::App("index.html?view=settings".into()),
    )
    .title("QuickLaunch 设置")
    .inner_size(780.0, 620.0)
    .min_inner_size(720.0, 520.0)
    .resizable(true)
    .center()
    .build()
    .map(|_| ())
    .map_err(|err| format!("无法打开设置窗口：{err}"))
}

/// 后台线程扫描，扫描期间不阻塞 IPC。
pub fn spawn_scan(app: AppHandle) {
    let state = app.state::<AppState>();
    if state.is_scanning() {
        return;
    }
    state.scanning.store(true, Ordering::SeqCst);
    let config = state.config_snapshot();
    let cache_path = state.paths.index_cache.clone();
    drop(state);

    let _ = app.emit("scan-state", true);

    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        let outcome = scanner::scan_all(&config.scan_paths, &config.exclude_patterns);
        if !outcome.errors.is_empty() {
            eprintln!("[quicklaunch] 扫描告警：{}", outcome.errors.join("；"));
        }
        eprintln!(
            "[quicklaunch] 扫描完成：{} 个应用，用时 {} ms（根目录：{}）",
            outcome.items.len(),
            started.elapsed().as_millis(),
            outcome.scanned_roots.join("、")
        );

        {
            let state = app.state::<AppState>();
            if let Ok(mut apps) = state.scan_cache.lock() {
                *apps = outcome.items;
            }
            if let Ok(mut last) = state.last_scan_at.lock() {
                *last = Some(now_secs());
            }
            rebuild_index(&state, &config);
            state.drop_icon_cache();
            state.scanning.store(false, Ordering::SeqCst);

            if let Ok(apps) = state.scan_cache.lock() {
                if let Ok(bytes) = serde_json::to_vec(&*apps) {
                    let _ = config::write_atomic(&cache_path, &bytes);
                }
            }
        }

        let state = app.state::<AppState>();
        let total = state
            .index
            .read()
            .map(|index| index.len())
            .unwrap_or(0);
        drop(state);

        let _ = app.emit("scan-state", false);
        let _ = app.emit("index-updated", total);
    });
}

/// 用缓存的扫描结果先填满索引，让冷启动后的第一次唤起就能出结果。
pub fn preload_index_cache(app: &AppHandle) -> usize {
    let state = app.state::<AppState>();
    let apps: Vec<LauncherItem> = std::fs::read_to_string(&state.paths.index_cache)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    if apps.is_empty() {
        return 0;
    }
    if let Ok(mut cache) = state.scan_cache.lock() {
        *cache = apps;
    }
    let config = state.config_snapshot();
    rebuild_index(&state, &config);
    state.index.read().map(|index| index.len()).unwrap_or(0)
}

/// 索引 = 扫描到的应用 + 配置里的自定义条目。
pub fn rebuild_index(state: &AppState, config: &Config) {
    let mut items: Vec<LauncherItem> = state
        .scan_cache
        .lock()
        .map(|apps| apps.clone())
        .unwrap_or_default();
    items.extend(config.custom_items.iter().map(custom_to_item));

    let index: Vec<IndexedItem> = search::build_index(items);
    if let Ok(mut guard) = state.index.write() {
        *guard = index;
    }
}

/// 自定义条目 → 统一条目模型。
pub fn custom_to_item(custom: &CustomItem) -> LauncherItem {
    let target = match custom.kind {
        ItemKind::Folder => config::expand_path(&custom.target)
            .to_string_lossy()
            .into_owned(),
        _ => custom.target.trim().to_string(),
    };
    let subtitle = match custom.kind {
        ItemKind::Folder => config::prettify_path(Path::new(&target)),
        ItemKind::Command => custom
            .cwd
            .clone()
            .filter(|c| !c.trim().is_empty())
            .unwrap_or_else(|| target.clone()),
        _ => target.clone(),
    };

    LauncherItem {
        id: custom.id.clone(),
        name: custom.name.clone(),
        kind: custom.kind,
        subtitle: Some(subtitle),
        bundle_id: None,
        target: Some(target),
        icon_file: None,
        cwd: custom
            .cwd
            .clone()
            .filter(|c| !c.trim().is_empty()),
        keywords: custom.keywords.clone(),
        builtin: false,
        confirm: custom.kind == ItemKind::Command && custom.confirm,
    }
}

// ─────────────────────────── 系统集成 ───────────────────────────

/// 注册全局快捷键。
///
/// 失败时把 `fallback`（上一次可用的组合）重新注册回去，保证用户永远
/// 至少有一个能唤起启动器的热键 —— 否则一次误配置就会把应用锁在门外。
pub fn apply_shortcut(
    app: &AppHandle,
    accelerator: &str,
    fallback: Option<&str>,
) -> Result<(), String> {
    let parsed = parse_shortcut(accelerator)?;
    let manager = app.global_shortcut();
    let _ = manager.unregister_all();

    match manager.register(parsed) {
        Ok(()) => Ok(()),
        Err(err) => {
            if let Some(fallback) = fallback.and_then(|accel| parse_shortcut(accel).ok()) {
                let _ = manager.register(fallback);
            }
            Err(format!(
                "快捷键「{accelerator}」注册失败（可能已被系统或其他应用占用）：{err}"
            ))
        }
    }
}

/// Dock 图标开关。Accessory 策略下应用不占 Dock 也不进 Cmd+Tab。
#[cfg(target_os = "macos")]
pub fn apply_activation_policy(app: &AppHandle, show_in_dock: bool) {
    let policy = if show_in_dock {
        tauri::ActivationPolicy::Regular
    } else {
        tauri::ActivationPolicy::Accessory
    };
    if let Err(err) = app.set_activation_policy(policy) {
        eprintln!("[quicklaunch] 设置激活策略失败：{err}");
    }
}

#[cfg(not(target_os = "macos"))]
pub fn apply_activation_policy(_app: &AppHandle, _show_in_dock: bool) {}

/// 解析快捷键字符串。
///
/// 只认 `+` 分隔的组合，修饰键至少一个、主键恰好一个 —— 这样既覆盖了
/// 常见写法（`Option+Space`、`Cmd+Shift+K`），也让错误信息足够明确，
/// 便于设置页在用户输入时实时提示。
pub fn parse_shortcut(accelerator: &str) -> Result<Shortcut, String> {
    let mut modifiers = Modifiers::empty();
    let mut code: Option<Code> = None;

    for raw in accelerator.split('+') {
        let part = raw.trim();
        if part.is_empty() {
            continue;
        }
        match part.to_ascii_lowercase().as_str() {
            "option" | "alt" => modifiers |= Modifiers::ALT,
            "cmd" | "command" | "super" | "meta" | "⌘" => modifiers |= Modifiers::SUPER,
            "ctrl" | "control" | "⌃" => modifiers |= Modifiers::CONTROL,
            "shift" | "⇧" => modifiers |= Modifiers::SHIFT,
            _ => {
                if code.is_some() {
                    return Err(format!("「{accelerator}」包含了多个主键"));
                }
                code = Some(parse_code(part)?);
            }
        }
    }

    let Some(code) = code else {
        return Err("快捷键缺少主键，例如 Option+Space".into());
    };
    if modifiers.is_empty() {
        return Err("快捷键至少需要一个修饰键（Cmd / Option / Ctrl / Shift）".into());
    }
    Ok(Shortcut::new(Some(modifiers), code))
}

fn parse_code(part: &str) -> Result<Code, String> {
    let lower = part.to_ascii_lowercase();
    let code = match lower.as_str() {
        "space" | "空格" => Code::Space,
        "enter" | "return" | "回车" => Code::Enter,
        "tab" => Code::Tab,
        "escape" | "esc" => Code::Escape,
        "backspace" => Code::Backspace,
        "delete" | "del" => Code::Delete,
        "up" | "arrowup" => Code::ArrowUp,
        "down" | "arrowdown" => Code::ArrowDown,
        "left" | "arrowleft" => Code::ArrowLeft,
        "right" | "arrowright" => Code::ArrowRight,
        "home" => Code::Home,
        "end" => Code::End,
        "pageup" => Code::PageUp,
        "pagedown" => Code::PageDown,
        "f1" => Code::F1,
        "f2" => Code::F2,
        "f3" => Code::F3,
        "f4" => Code::F4,
        "f5" => Code::F5,
        "f6" => Code::F6,
        "f7" => Code::F7,
        "f8" => Code::F8,
        "f9" => Code::F9,
        "f10" => Code::F10,
        "f11" => Code::F11,
        "f12" => Code::F12,
        "`" | "backquote" => Code::Backquote,
        "-" | "minus" => Code::Minus,
        "=" | "equal" => Code::Equal,
        "[" => Code::BracketLeft,
        "]" => Code::BracketRight,
        "\\" => Code::Backslash,
        ";" => Code::Semicolon,
        "'" => Code::Quote,
        "," => Code::Comma,
        "." | "period" => Code::Period,
        "/" => Code::Slash,
        single if single.chars().count() == 1 => {
            let ch = single.chars().next().unwrap_or(' ');
            match ch {
                'a'..='z' => letter_code(ch)?,
                '0'..='9' => digit_code(ch)?,
                _ => return Err(format!("不支持的按键：{part}")),
            }
        }
        _ => return Err(format!("不支持的按键：{part}")),
    };
    Ok(code)
}

fn letter_code(ch: char) -> Result<Code, String> {
    Ok(match ch {
        'a' => Code::KeyA,
        'b' => Code::KeyB,
        'c' => Code::KeyC,
        'd' => Code::KeyD,
        'e' => Code::KeyE,
        'f' => Code::KeyF,
        'g' => Code::KeyG,
        'h' => Code::KeyH,
        'i' => Code::KeyI,
        'j' => Code::KeyJ,
        'k' => Code::KeyK,
        'l' => Code::KeyL,
        'm' => Code::KeyM,
        'n' => Code::KeyN,
        'o' => Code::KeyO,
        'p' => Code::KeyP,
        'q' => Code::KeyQ,
        'r' => Code::KeyR,
        's' => Code::KeyS,
        't' => Code::KeyT,
        'u' => Code::KeyU,
        'v' => Code::KeyV,
        'w' => Code::KeyW,
        'x' => Code::KeyX,
        'y' => Code::KeyY,
        'z' => Code::KeyZ,
        other => return Err(format!("不支持的按键：{other}")),
    })
}

fn digit_code(ch: char) -> Result<Code, String> {
    Ok(match ch {
        '0' => Code::Digit0,
        '1' => Code::Digit1,
        '2' => Code::Digit2,
        '3' => Code::Digit3,
        '4' => Code::Digit4,
        '5' => Code::Digit5,
        '6' => Code::Digit6,
        '7' => Code::Digit7,
        '8' => Code::Digit8,
        '9' => Code::Digit9,
        other => return Err(format!("不支持的按键：{other}")),
    })
}

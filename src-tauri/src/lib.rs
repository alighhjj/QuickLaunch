//! QuickLaunch 应用装配。
//!
//! 启动顺序经过刻意安排，每一段的理由都写在注释里 —— 顺序错了会直接
//! 影响「按快捷键到看到结果」的体感延迟。

mod commands;
mod config;
mod icons;
mod model;
mod scanner;
mod search;
mod state;
mod tray;

use tauri::{Emitter, Manager, WindowEvent};

use crate::state::{AppState, Paths};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    // 只响应按下：否则松开时会立刻把窗口又关掉
                    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        commands::toggle_main_window(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let handle = app.handle().clone();

            let paths = Paths::new(handle.path().app_config_dir()?);
            // 配置文件的「首次存在性」就是首次运行的判据
            let first_run = !paths.config.exists();

            let state = AppState::new(paths);
            let config = state.config_snapshot();
            app.manage(state);

            // 1) 先吃掉上次的扫描缓存：冷启动后第一次按下快捷键就应立刻有结果，
            //    不必等后台扫描跑完（PRD 5.2 热唤起 < 100ms）
            let preloaded = commands::preload_index_cache(&handle);

            // 2) 系统集成：激活策略 → 全局快捷键 → 菜单栏图标
            commands::apply_activation_policy(&handle, config.general.show_in_dock);
            if let Err(err) = commands::apply_shortcut(&handle, &config.shortcut.toggle, None) {
                // 不阻断启动：窗口仍可通过菜单栏打开
                eprintln!("[quicklaunch] {err}");
            }
            tray::build(&handle)?;

            // 3) 首次运行主动露个面，否则用户根本不知道程序装到哪去了
            if first_run {
                commands::reveal_main(&handle);
            }

            // 4) 后台全量扫描，完成后通过事件通知前端刷新
            commands::spawn_scan(handle.clone());

            eprintln!("[quicklaunch] 已就绪：缓存条目 {preloaded}，快捷键 {}", config.shortcut.toggle);
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // 失去焦点即隐藏（可通过设置关闭）
            WindowEvent::Focused(false) => {
                if window.label() != "main" {
                    return;
                }
                // 用 try_state 而不是 state：窗口事件可能早于 setup 里的
                // app.manage() 触发，直接 state() 会 panic。
                let should_hide = window
                    .app_handle()
                    .try_state::<AppState>()
                    .and_then(|state| {
                        state
                            .config
                            .lock()
                            .map(|config| config.general.hide_on_blur)
                            .ok()
                    })
                    .unwrap_or(false);
                if should_hide {
                    let _ = window.hide();
                }
            }
            // 重新获得焦点：通知前端清空输入并回到空状态
            WindowEvent::Focused(true) => {
                if window.label() == "main" {
                    let _ = window.app_handle().emit("launcher-shown", ());
                }
            }
            // 主窗口永远不真正关闭，只隐藏 —— 否则常驻菜单栏就无从谈起
            WindowEvent::CloseRequested { api, .. } => {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::search_items,
            commands::get_icon,
            commands::status,
            commands::validate_shortcut,
            commands::validate_target,
            commands::export_config,
            commands::import_config,
            commands::clear_usage,
            commands::toggle_favorite,
            commands::launch,
            commands::hide_main,
            commands::toggle_main,
            commands::open_settings,
            commands::rescan,
            commands::open_data_dir,
        ])
        .run(tauri::generate_context!())
        .expect("QuickLaunch 启动失败");
}

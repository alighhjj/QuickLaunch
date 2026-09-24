//! 菜单栏常驻图标（PRD F-29）。

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

use crate::commands;

pub const TRAY_ID: &str = "quicklaunch-tray";

/// 托盘图标直接内嵌进二进制：菜单栏图标必须在窗口未创建时就能用，
/// 且尺寸固定 32px，没必要走运行时文件读取。
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray.png");

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, "toggle", "显示 / 隐藏启动器", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "设置…", true, None::<&str>)?;
    let rescan = MenuItem::with_id(app, "rescan", "重新扫描应用", true, None::<&str>)?;
    let data_dir = MenuItem::with_id(app, "data-dir", "打开配置目录", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 QuickLaunch", true, None::<&str>)?;

    // 逐个 append 而不是 Menu::with_items(&[...])：
    // 数组元素的类型必须一致，而 MenuItem 与 PredefinedMenuItem 是不同类型，
    // 混用时 with_items 推断不出 `&dyn IsMenuItem` 会直接编译失败。
    let menu = Menu::new(app)?;
    menu.append(&toggle)?;
    menu.append(&settings)?;
    menu.append(&rescan)?;
    menu.append(&data_dir)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&quit)?;

    let icon = Image::from_bytes(TRAY_ICON)?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        // 模板图标：macOS 会按菜单栏的明暗自动反色
        .icon_as_template(true)
        .menu(&menu)
        // 左键单击直接切换窗口，右键才弹菜单 —— 启动器的高频操作不该埋在菜单里
        .show_menu_on_left_click(false)
        .tooltip("QuickLaunch")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => commands::toggle_main_window(app),
            "settings" => {
                if let Err(err) = commands::open_settings_window(app) {
                    eprintln!("[quicklaunch] {err}");
                }
            }
            "rescan" => commands::spawn_scan(app.clone()),
            "data-dir" => commands::open_data_dir(app.clone()),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                commands::toggle_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

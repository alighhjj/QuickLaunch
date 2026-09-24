// Windows 下发布版不弹控制台窗口；macOS 上该属性无副作用。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    quicklaunch_lib::run()
}

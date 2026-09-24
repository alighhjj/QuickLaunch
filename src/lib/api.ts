import { invoke } from "@tauri-apps/api/core";

import type {
  Config,
  ItemKind,
  LaunchReport,
  SearchHit,
  Status,
} from "../types";

/**
 * 全部后端调用的唯一入口。
 *
 * 刻意不引入任何 Tauri 插件的前端包（除文件对话框）：窗口、开机自启、
 * 通知、命令执行都由自定义 Rust 命令封装，这样前端依赖面最小，
 * 也避免 JS 包与 Rust crate 版本漂移导致的能力不匹配。
 */
export const api = {
  getConfig: () => invoke<Config>("get_config"),
  saveConfig: (config: Config) => invoke<Config>("save_config", { config }),

  search: (query: string, limit?: number) =>
    invoke<SearchHit[]>("search_items", { query, limit }),
  getIcon: (id: string) => invoke<string | null>("get_icon", { id }),
  status: () => invoke<Status>("status"),

  validateShortcut: (accelerator: string) =>
    invoke<string>("validate_shortcut", { accelerator }),
  validateTarget: (kind: ItemKind, target: string) =>
    invoke<void>("validate_target", { kind, target }),

  launch: (id: string, confirmed = false) =>
    invoke<LaunchReport>("launch", { id, confirmed }),

  toggleFavorite: (id: string) => invoke<Config>("toggle_favorite", { id }),
  clearUsage: () => invoke<void>("clear_usage"),

  exportConfig: (path: string) => invoke<string>("export_config", { path }),
  importConfig: (path: string) => invoke<Config>("import_config", { path }),

  hideMain: () => invoke<void>("hide_main"),
  toggleMain: () => invoke<void>("toggle_main"),
  openSettings: () => invoke<void>("open_settings"),
  rescan: () => invoke<void>("rescan"),
  openDataDir: () => invoke<void>("open_data_dir"),
};

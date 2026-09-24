/** 与 Rust 侧 `model.rs` / `config.rs` 一一对应的类型定义。 */

export type ItemKind = "app" | "folder" | "url" | "command";

export interface LauncherItem {
  id: string;
  name: string;
  kind: ItemKind;
  subtitle?: string;
  bundleId?: string;
  target?: string;
  cwd?: string;
  keywords?: string[];
  builtin: boolean;
  confirm: boolean;
}

export interface SearchHit {
  item: LauncherItem;
  score: number;
  /** [起始字符序号, 长度]，相对 item.name */
  highlights: [number, number][];
}

export interface GeneralConfig {
  launchAtLogin: boolean;
  hideOnBlur: boolean;
  maxResults: number;
  showInDock: boolean;
  commandConfirm: boolean;
}

export type ThemeMode = "system" | "light" | "dark";
export type IconSize = "small" | "medium" | "large";

export interface AppearanceConfig {
  theme: ThemeMode;
  opacity: number;
  iconSize: IconSize;
}

export interface ShortcutConfig {
  toggle: string;
}

export interface CustomItem {
  id: string;
  kind: ItemKind;
  name: string;
  target: string;
  cwd?: string;
  confirm: boolean;
  keywords: string[];
}

export interface Config {
  version: number;
  general: GeneralConfig;
  appearance: AppearanceConfig;
  shortcut: ShortcutConfig;
  scanPaths: string[];
  excludePatterns: string[];
  customItems: CustomItem[];
  favorites: string[];
  pinned: string[];
}

export interface Status {
  version: string;
  itemCount: number;
  appCount: number;
  customCount: number;
  scanning: boolean;
  lastScanAt: number | null;
  autostart: boolean;
}

export interface LaunchReport {
  ok: boolean;
  message: string;
}

export const KIND_LABEL: Record<ItemKind, string> = {
  app: "应用",
  folder: "文件夹",
  url: "网址",
  command: "命令",
};

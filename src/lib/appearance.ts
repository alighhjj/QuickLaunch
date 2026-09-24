import type { IconSize, ThemeMode } from "../types";

/** 把配置里的主题模式解析成实际生效的明暗。 */
export function resolveTheme(mode: ThemeMode): "light" | "dark" {
  if (mode === "light" || mode === "dark") {
    return mode;
  }
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

export function applyTheme(mode: ThemeMode) {
  document.documentElement.dataset.theme = resolveTheme(mode);
}

const ICON_PX: Record<IconSize, number> = {
  small: 24,
  medium: 32,
  large: 40,
};

export function iconPx(size: IconSize): number {
  return ICON_PX[size] ?? ICON_PX.medium;
}

/** 外观设置直接落到 CSS 变量上，React 无需为它重渲染。 */
export function applyAppearance(opacity: number, iconSize: IconSize) {
  const root = document.documentElement;
  root.style.setProperty("--panel-alpha", String(opacity));
  root.style.setProperty("--icon-size", `${iconPx(iconSize)}px`);
}

/** 跟随系统时，系统外观变化要实时反映到界面上。 */
export function watchSystemTheme(mode: ThemeMode, onChange: () => void) {
  if (mode !== "system" || !window.matchMedia) {
    return () => {};
  }
  const query = window.matchMedia("(prefers-color-scheme: dark)");
  query.addEventListener("change", onChange);
  return () => query.removeEventListener("change", onChange);
}

const MOD_SYMBOL: Record<string, string> = {
  cmd: "⌘",
  command: "⌘",
  super: "⌘",
  meta: "⌘",
  option: "⌥",
  alt: "⌥",
  ctrl: "⌃",
  control: "⌃",
  shift: "⇧",
};

const KEY_LABEL: Record<string, string> = {
  space: "Space",
  enter: "↩",
  return: "↩",
  tab: "⇥",
  escape: "⎋",
  esc: "⎋",
  backspace: "⌫",
  delete: "⌦",
  up: "↑",
  down: "↓",
  left: "←",
  right: "→",
  arrowup: "↑",
  arrowdown: "↓",
  arrowleft: "←",
  arrowright: "→",
  pageup: "⇞",
  pagedown: "⇟",
};

/** `Option+Space` → `⌥ Space`，用于界面展示。解析仍以原始字符串为准。 */
export function prettyAccel(accelerator: string): string {
  return accelerator
    .split("+")
    .map((part) => {
      const key = part.trim().toLowerCase();
      if (!key) {
        return "";
      }
      if (MOD_SYMBOL[key]) {
        return MOD_SYMBOL[key];
      }
      return KEY_LABEL[key] ?? part.trim().toUpperCase();
    })
    .filter(Boolean)
    .join(" ");
}

export function formatTime(seconds: number | null): string {
  if (!seconds) {
    return "尚未扫描";
  }
  const date = new Date(seconds * 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(
    date.getDate(),
  )} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

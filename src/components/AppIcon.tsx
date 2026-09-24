import type { ItemKind, LauncherItem } from "../types";

/** 非应用类条目用矢量字形，避免引入表情符号破坏原生观感。 */
const GLYPH: Record<Exclude<ItemKind, "app">, JSX.Element> = {
  folder: (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path
        d="M3 7.2c0-1 .8-1.7 1.7-1.7h4l1.9 2h7.7c.9 0 1.7.8 1.7 1.7v7.6c0 .9-.8 1.7-1.7 1.7H4.7c-.9 0-1.7-.8-1.7-1.7V7.2Z"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinejoin="round"
      />
    </svg>
  ),
  url: (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="12" cy="12" r="8.2" fill="none" stroke="currentColor" strokeWidth="1.6" />
      <path
        d="M3.8 12h16.4M12 3.8c2.2 2.3 3.3 5.1 3.3 8.2S14.2 17.9 12 20.2c-2.2-2.3-3.3-5.1-3.3-8.2S9.8 6.1 12 3.8Z"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.6"
      />
    </svg>
  ),
  command: (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <rect
        x="3.2"
        y="4.2"
        width="17.6"
        height="15.6"
        rx="2.4"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.6"
      />
      <path
        d="m7.6 9.6 2.6 2.6-2.6 2.6M12.6 15h4"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  ),
};

function initial(name: string): string {
  const chars = Array.from(name.trim());
  return chars.length > 0 ? chars[0].toUpperCase() : "?";
}

interface Props {
  item: LauncherItem;
  /** 已解析出的图标 data URL；undefined 表示尚未拉取，null 表示确认无图标 */
  dataUrl?: string | null;
  size: number;
  selected?: boolean;
}

export function AppIcon({ item, dataUrl, size, selected }: Props) {
  const style = { width: size, height: size };

  if (dataUrl) {
    return (
      <img className="icon" style={style} src={dataUrl} alt="" draggable={false} />
    );
  }
  if (item.kind === "app") {
    return (
      <span
        className="icon icon-letter"
        style={{ ...style, fontSize: Math.round(size * 0.42) }}
        data-selected={selected ? "true" : "false"}
      >
        {initial(item.name)}
      </span>
    );
  }
  return (
    <span className="icon icon-glyph" style={style} data-kind={item.kind}>
      {GLYPH[item.kind as Exclude<ItemKind, "app">]}
    </span>
  );
}

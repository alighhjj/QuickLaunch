import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import { AppIcon } from "../components/AppIcon";
import { Button } from "../components/Controls";
import { Highlight } from "../components/Highlight";
import { api } from "../lib/api";
import {
  applyAppearance,
  applyTheme,
  iconPx,
  prettyAccel,
  watchSystemTheme,
} from "../lib/appearance";
import { KIND_LABEL, type Config, type SearchHit, type Status } from "../types";

/** 首屏预取图标的数量：足以覆盖一屏可见行，又不会让 IPC 负载失控。 */
const ICON_PREFETCH = 24;

export default function Launcher() {
  const [config, setConfig] = useState<Config | null>(null);
  const [status, setStatus] = useState<Status | null>(null);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [selected, setSelected] = useState(0);
  const [icons, setIcons] = useState<Record<string, string | null>>({});
  const [pending, setPending] = useState<SearchHit | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [refreshToken, setRefreshToken] = useState(0);

  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const requestedIcons = useRef<Set<string>>(new Set());
  const searchSeq = useRef(0);

  const maxResults = config?.general.maxResults ?? 20;

  // ── 配置与外观 ────────────────────────────────────────────────
  const loadConfig = useCallback(async () => {
    const next = await api.getConfig();
    setConfig(next);
    applyTheme(next.appearance.theme);
    applyAppearance(next.appearance.opacity, next.appearance.iconSize);
  }, []);

  const loadStatus = useCallback(async () => {
    try {
      setStatus(await api.status());
    } catch {
      // 扫描进行中时 status 可能瞬时失败，忽略即可
    }
  }, []);

  const refresh = useCallback(() => setRefreshToken((token) => token + 1), []);

  useEffect(() => {
    void loadConfig();
    void loadStatus();
  }, [loadConfig, loadStatus]);

  useEffect(() => {
    if (!config) {
      return;
    }
    return watchSystemTheme(config.appearance.theme, () => {
      applyTheme(config.appearance.theme);
    });
  }, [config]);

  // ── 搜索 ─────────────────────────────────────────────────────
  useEffect(() => {
    const seq = (searchSeq.current += 1);
    api
      .search(query, maxResults)
      .then((result) => {
        if (seq !== searchSeq.current) {
          return;
        }
        setHits(result);
        setSelected(0);
      })
      .catch(() => {
        if (seq === searchSeq.current) {
          setHits([]);
        }
      });
  }, [query, maxResults, refreshToken]);

  // ── 图标按需拉取 ──────────────────────────────────────────────
  useEffect(() => {
    const missing = hits
      .slice(0, ICON_PREFETCH)
      .filter(
        (hit) =>
          hit.item.kind === "app" && !requestedIcons.current.has(hit.item.id),
      );
    if (missing.length === 0) {
      return;
    }
    let alive = true;
    void (async () => {
      for (const hit of missing) {
        requestedIcons.current.add(hit.item.id);
        try {
          const url = await api.getIcon(hit.item.id);
          if (!alive) {
            return;
          }
          setIcons((prev) => ({ ...prev, [hit.item.id]: url ?? null }));
        } catch {
          if (alive) {
            setIcons((prev) => ({ ...prev, [hit.item.id]: null }));
          }
        }
      }
    })();
    return () => {
      alive = false;
    };
  }, [hits]);

  // ── 主进程事件 ───────────────────────────────────────────────
  useEffect(() => {
    let active = true;
    const unlisten: Array<() => void> = [];

    void (async () => {
      const handlers: Array<Promise<() => void>> = [
        listen("launcher-shown", () => {
          setQuery("");
          setSelected(0);
          setPending(null);
          setToast(null);
          // 设置窗口可能刚改过配置，重新读一次
          void loadConfig();
          void loadStatus();
          refresh();
        }),
        listen("scan-state", () => void loadStatus()),
        listen("index-updated", () => {
          void loadStatus();
          refresh();
        }),
      ];
      const resolved = await Promise.all(handlers);
      if (active) {
        unlisten.push(...resolved);
      } else {
        resolved.forEach((fn) => fn());
      }
    })();

    return () => {
      active = false;
      unlisten.forEach((fn) => fn());
    };
  }, [loadConfig, loadStatus, refresh]);

  useEffect(() => {
    inputRef.current?.focus();
    const focus = window.setInterval(() => {
      if (document.activeElement !== inputRef.current) {
        inputRef.current?.focus();
      }
    }, 600);
    return () => window.clearInterval(focus);
  }, []);

  useEffect(() => {
    if (!toast) {
      return;
    }
    const timer = window.setTimeout(() => setToast(null), 3200);
    return () => window.clearTimeout(timer);
  }, [toast]);

  useEffect(() => {
    listRef.current
      ?.querySelector<HTMLElement>(`[data-index="${selected}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [selected, hits]);

  // ── 行为 ─────────────────────────────────────────────────────
  const execute = useCallback(
    async (hit: SearchHit, confirmed: boolean) => {
      try {
        const report = await api.launch(hit.item.id, confirmed);
        if (!report.ok) {
          setToast(report.message);
          return;
        }
        setPending(null);
        setQuery("");
        refresh();
      } catch (err) {
        setPending(null);
        setToast(String(err));
      }
    },
    [refresh],
  );

  const activate = useCallback(
    (hit: SearchHit | undefined) => {
      if (!hit) {
        return;
      }
      const needsConfirm =
        hit.item.kind === "command" &&
        (hit.item.confirm || (config?.general.commandConfirm ?? true));
      if (needsConfirm) {
        setPending(hit);
        return;
      }
      void execute(hit, false);
    },
    [config, execute],
  );

  const toggleFavorite = useCallback(
    async (hit: SearchHit | undefined) => {
      if (!hit) {
        return;
      }
      try {
        const next = await api.toggleFavorite(hit.item.id);
        setConfig(next);
        setToast(
          next.favorites.includes(hit.item.id) ? "已收藏" : "已取消收藏",
        );
        refresh();
      } catch (err) {
        setToast(String(err));
      }
    },
    [refresh],
  );

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (pending) {
      if (event.key === "Enter") {
        event.preventDefault();
        void execute(pending, true);
      } else if (event.key === "Escape") {
        event.preventDefault();
        setPending(null);
      }
      return;
    }

    const meta = event.metaKey || event.ctrlKey;

    if (event.key === "ArrowDown" || (meta && event.key.toLowerCase() === "n")) {
      event.preventDefault();
      setSelected((prev) => (hits.length === 0 ? 0 : (prev + 1) % hits.length));
      return;
    }
    if (event.key === "ArrowUp" || (meta && event.key.toLowerCase() === "p")) {
      event.preventDefault();
      setSelected((prev) =>
        hits.length === 0 ? 0 : (prev - 1 + hits.length) % hits.length,
      );
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      activate(hits[selected]);
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      void api.hideMain();
      return;
    }
    if (meta && event.key === ",") {
      event.preventDefault();
      void api.openSettings();
      return;
    }
    if (meta && event.key.toLowerCase() === "k") {
      event.preventDefault();
      setQuery("");
      return;
    }
    if (meta && event.key.toLowerCase() === "d") {
      event.preventDefault();
      void toggleFavorite(hits[selected]);
      return;
    }
    if (meta && event.key.toLowerCase() === "r") {
      event.preventDefault();
      void api.rescan();
      setToast("已开始重新扫描");
    }
  };

  const shortcut = config ? prettyAccel(config.shortcut.toggle) : "⌥ Space";
  const empty = hits.length === 0;

  return (
    <div className="launcher" onKeyDown={onKeyDown}>
      <div className="search">
        <svg className="search-icon" viewBox="0 0 24 24" aria-hidden="true">
          <circle cx="10.8" cy="10.8" r="6.6" fill="none" stroke="currentColor" strokeWidth="1.8" />
          <path d="m15.8 15.8 4 4" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
        </svg>
        <input
          ref={inputRef}
          value={query}
          spellCheck={false}
          autoComplete="off"
          placeholder="搜索应用、文件夹、网址或命令…"
          onChange={(event) => setQuery(event.target.value)}
        />
        {query ? (
          <button
            type="button"
            className="search-clear"
            title="清空 (⌘K)"
            onClick={() => {
              setQuery("");
              inputRef.current?.focus();
            }}
          >
            ✕
          </button>
        ) : null}
      </div>

      <div className="results" ref={listRef}>
        {empty && query === "" ? (
          <div className="placeholder">
            <p className="placeholder-title">
              {status?.scanning ? "正在扫描应用…" : "输入关键词开始搜索"}
            </p>
            <p className="placeholder-sub">
              支持拼音首字母，例如 <kbd>wx</kbd> 搜「微信」、<kbd>vsc</kbd> 搜
              「Visual Studio Code」
            </p>
          </div>
        ) : null}

        {empty && query !== "" ? (
          <div className="placeholder">
            <p className="placeholder-title">未找到匹配项</p>
            <p className="placeholder-sub">
              可以试试添加自定义文件夹、网址或命令（<kbd>⌘</kbd>
              <kbd>,</kbd> 打开设置）
            </p>
          </div>
        ) : null}

        {hits.map((hit, index) => (
          <div
            key={hit.item.id}
            data-index={index}
            data-selected={index === selected ? "true" : "false"}
            className="row"
            onMouseMove={() => setSelected(index)}
            onClick={() => activate(hit)}
          >
            <AppIcon
              item={hit.item}
              dataUrl={icons[hit.item.id]}
              size={iconPx(config?.appearance.iconSize ?? "medium")}
              selected={index === selected}
            />
            <div className="row-text">
              <div className="row-name">
                <Highlight text={hit.item.name} ranges={hit.highlights} />
              </div>
              {hit.item.subtitle ? (
                <div className="row-sub">{hit.item.subtitle}</div>
              ) : null}
            </div>
            <div className="row-meta">
              {config?.favorites.includes(hit.item.id) ? (
                <span className="star" title="已收藏">★</span>
              ) : null}
              <span className={`badge ${hit.item.kind}`}>
                {KIND_LABEL[hit.item.kind]}
              </span>
            </div>
          </div>
        ))}
      </div>

      <div className="hints">
        <span><kbd>↑</kbd><kbd>↓</kbd> 选择</span>
        <span><kbd>↩</kbd> 启动</span>
        <span><kbd>⌘</kbd><kbd>D</kbd> 收藏</span>
        <span><kbd>⌘</kbd><kbd>K</kbd> 清空</span>
        <span><kbd>⌘</kbd><kbd>,</kbd> 设置</span>
        <span className="hints-right">
          {status ? `共 ${status.itemCount} 个条目` : ""}
          <span className="hints-shortcut">{shortcut} 唤起</span>
        </span>
      </div>

      {toast ? <div className="toast">{toast}</div> : null}

      {pending ? (
        <div className="confirm-backdrop">
          <div className="confirm">
            <h3>确认执行命令</h3>
            <p className="confirm-name">{pending.item.name}</p>
            <pre className="confirm-cmd">{pending.item.target}</pre>
            {pending.item.cwd ? (
              <p className="confirm-cwd">工作目录：{pending.item.cwd}</p>
            ) : null}
            <div className="confirm-actions">
              <Button onClick={() => setPending(null)}>取消</Button>
              <Button variant="primary" onClick={() => void execute(pending, true)}>
                执行
              </Button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}

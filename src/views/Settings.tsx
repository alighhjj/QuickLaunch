import { useCallback, useEffect, useRef, useState } from "react";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";

import { Button, Field, Segmented, Section, Switch } from "../components/Controls";
import { api } from "../lib/api";
import {
  applyAppearance,
  applyTheme,
  formatTime,
  prettyAccel,
} from "../lib/appearance";
import type {
  Config,
  CustomItem,
  GeneralConfig,
  ItemKind,
  Status,
} from "../types";

type TabId = "general" | "appearance" | "sources" | "shortcut" | "about";

const TABS: { id: TabId; label: string }[] = [
  { id: "general", label: "通用" },
  { id: "appearance", label: "外观" },
  { id: "sources", label: "数据源" },
  { id: "shortcut", label: "快捷键" },
  { id: "about", label: "关于" },
];

const CUSTOM_KINDS: { value: ItemKind; label: string }[] = [
  { value: "folder", label: "文件夹" },
  { value: "url", label: "网址" },
  { value: "command", label: "命令" },
];

interface Notice {
  kind: "ok" | "error";
  text: string;
}

/** 把 KeyboardEvent 映射成与后端解析器一致的主键名。 */
function normalizeKey(event: React.KeyboardEvent): string | null {
  const code = event.code;
  if (/^Key[A-Z]$/.test(code)) {
    return code.slice(3);
  }
  if (/^Digit[0-9]$/.test(code)) {
    return code.slice(5);
  }
  if (/^F([1-9]|1[0-2])$/.test(code)) {
    return code;
  }
  const table: Record<string, string> = {
    Space: "Space",
    Enter: "Enter",
    Tab: "Tab",
    Backquote: "`",
    Minus: "-",
    Equal: "=",
    BracketLeft: "[",
    BracketRight: "]",
    Backslash: "\\",
    Semicolon: ";",
    Quote: "'",
    Comma: ",",
    Period: ".",
    Slash: "/",
    ArrowUp: "Up",
    ArrowDown: "Down",
    ArrowLeft: "Left",
    ArrowRight: "Right",
    Home: "Home",
    End: "End",
    PageUp: "PageUp",
    PageDown: "PageDown",
    Backspace: "Backspace",
    Delete: "Delete",
  };
  return table[code] ?? null;
}

function ShortcutRecorder({
  value,
  onChange,
}: {
  value: string;
  onChange: (accelerator: string) => void;
}) {
  const [recording, setRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const onKeyDown = (event: React.KeyboardEvent<HTMLButtonElement>) => {
    if (!recording) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();

    if (event.key === "Escape") {
      setRecording(false);
      setError(null);
      return;
    }

    const modifiers: string[] = [];
    if (event.metaKey) modifiers.push("Cmd");
    if (event.ctrlKey) modifiers.push("Ctrl");
    if (event.altKey) modifiers.push("Option");
    if (event.shiftKey) modifiers.push("Shift");

    const key = normalizeKey(event);
    // 只按修饰键时还没有主键，继续等待
    if (!key) {
      return;
    }
    if (modifiers.length === 0) {
      setError("至少需要一个修饰键（Cmd / Option / Ctrl / Shift）");
      return;
    }

    const accelerator = [...modifiers, key].join("+");
    setError(null);
    setRecording(false);
    onChange(accelerator);
  };

  return (
    <div className="recorder-wrap">
      <button
        type="button"
        className="recorder"
        data-recording={recording ? "true" : "false"}
        onClick={() => {
          setRecording(true);
          setError(null);
        }}
        onKeyDown={onKeyDown}
        onBlur={() => setRecording(false)}
      >
        {recording ? "请按下新的组合键…" : prettyAccel(value)}
      </button>
      {error ? <small className="danger">{error}</small> : null}
    </div>
  );
}

export default function Settings() {
  const [config, setConfig] = useState<Config | null>(null);
  const [status, setStatus] = useState<Status | null>(null);
  const [tab, setTab] = useState<TabId>("general");
  const [notice, setNotice] = useState<Notice | null>(null);
  const saveTimer = useRef<number | null>(null);
  const noticeTimer = useRef<number | null>(null);

  const loadStatus = useCallback(async () => {
    try {
      setStatus(await api.status());
    } catch {
      setStatus(null);
    }
  }, []);

  useEffect(() => {
    void (async () => {
      try {
        const loaded = await api.getConfig();
        setConfig(loaded);
        applyTheme(loaded.appearance.theme);
        applyAppearance(loaded.appearance.opacity, loaded.appearance.iconSize);
      } catch (err) {
        setNotice({ kind: "error", text: String(err) });
      }
      await loadStatus();
    })();
  }, [loadStatus]);

  useEffect(
    () => () => {
      if (saveTimer.current !== null) {
        window.clearTimeout(saveTimer.current);
      }
      if (noticeTimer.current !== null) {
        window.clearTimeout(noticeTimer.current);
      }
    },
    [],
  );

  const showNotice = useCallback((next: Notice) => {
    setNotice(next);
    if (noticeTimer.current !== null) {
      window.clearTimeout(noticeTimer.current);
    }
    noticeTimer.current = window.setTimeout(
      () => setNotice(null),
      next.kind === "ok" ? 1800 : 6000,
    );
  }, []);

  /**
   * 所有配置变更的统一出口。
   *
   * 外观类改动立刻反映到界面上（即使后端保存失败，用户也先看到效果），
   * 保存本身按 350ms 防抖，避免拖动滑块时把磁盘写爆。
   */
  const commit = useCallback(
    (next: Config, immediate = false) => {
      setConfig(next);
      applyTheme(next.appearance.theme);
      applyAppearance(next.appearance.opacity, next.appearance.iconSize);

      if (saveTimer.current !== null) {
        window.clearTimeout(saveTimer.current);
      }
      const persist = async () => {
        try {
          const saved = await api.saveConfig(next);
          setConfig(saved);
          showNotice({ kind: "ok", text: "已保存" });
        } catch (err) {
          showNotice({ kind: "error", text: String(err) });
        }
      };
      if (immediate) {
        void persist();
      } else {
        saveTimer.current = window.setTimeout(() => void persist(), 350);
      }
    },
    [showNotice],
  );

  if (!config) {
    return <div className="settings settings-loading">正在读取配置…</div>;
  }

  const patchGeneral = (patch: Partial<GeneralConfig>) =>
    commit({ ...config, general: { ...config.general, ...patch } });

  const setList = (
    key: "scanPaths" | "excludePatterns",
    list: string[],
  ) => commit({ ...config, [key]: list }, true);

  const addCustomItem = (item: CustomItem) =>
    commit({ ...config, customItems: [...config.customItems, item] }, true);

  const removeCustomItem = (id: string) =>
    commit(
      { ...config, customItems: config.customItems.filter((x) => x.id !== id) },
      true,
    );

  const onExport = async () => {
    const path = await saveDialog({
      defaultPath: "quicklaunch-config.json",
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!path) {
      return;
    }
    try {
      const written = await api.exportConfig(path);
      showNotice({ kind: "ok", text: `已导出到 ${written}` });
    } catch (err) {
      showNotice({ kind: "error", text: String(err) });
    }
  };

  const onImport = async () => {
    const picked = await openDialog({
      multiple: false,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (typeof picked !== "string") {
      return;
    }
    try {
      const imported = await api.importConfig(picked);
      setConfig(imported);
      applyTheme(imported.appearance.theme);
      applyAppearance(imported.appearance.opacity, imported.appearance.iconSize);
      showNotice({ kind: "ok", text: "配置已导入并生效" });
      await loadStatus();
    } catch (err) {
      showNotice({ kind: "error", text: String(err) });
    }
  };

  return (
    <div className="settings">
      <nav className="settings-nav">
        {TABS.map((item) => (
          <button
            key={item.id}
            type="button"
            data-active={tab === item.id ? "true" : "false"}
            onClick={() => setTab(item.id)}
          >
            {item.label}
          </button>
        ))}
        <div className="settings-nav-foot">
          {status ? `v${status.version}` : ""}
        </div>
      </nav>

      <main className="settings-main">
        {tab === "general" ? (
          <Section title="通用" description="启动器的基本行为。">
            <Field label="开机自启" hint="登录时自动启动并常驻菜单栏">
              <Switch
                label="开机自启"
                checked={config.general.launchAtLogin}
                onChange={(value) => patchGeneral({ launchAtLogin: value })}
              />
            </Field>
            <Field label="失去焦点时隐藏" hint="切到其他应用后自动收起面板">
              <Switch
                label="失去焦点时隐藏"
                checked={config.general.hideOnBlur}
                onChange={(value) => patchGeneral({ hideOnBlur: value })}
              />
            </Field>
            <Field
              label="在 Dock 中显示图标"
              hint="关闭后仅驻留菜单栏，但 macOS 下取焦行为不稳定"
            >
              <Switch
                label="在 Dock 中显示图标"
                checked={config.general.showInDock}
                onChange={(value) => patchGeneral({ showInDock: value })}
              />
            </Field>
            <Field label="执行命令前确认" hint="对自定义 Shell 命令强制二次确认">
              <Switch
                label="执行命令前确认"
                checked={config.general.commandConfirm}
                onChange={(value) => patchGeneral({ commandConfirm: value })}
              />
            </Field>
            <Field label="显示条目数" hint={`${config.general.maxResults} 条`}>
              <input
                type="range"
                min={5}
                max={60}
                step={1}
                value={config.general.maxResults}
                onChange={(event) =>
                  patchGeneral({ maxResults: Number(event.target.value) })
                }
              />
            </Field>
          </Section>
        ) : null}

        {tab === "appearance" ? (
          <Section title="外观" description="主题、透明度与图标尺寸。">
            <Field label="主题">
              <Segmented
                value={config.appearance.theme}
                options={[
                  { value: "system", label: "跟随系统" },
                  { value: "light", label: "浅色" },
                  { value: "dark", label: "深色" },
                ]}
                onChange={(value) =>
                  commit({
                    ...config,
                    appearance: { ...config.appearance, theme: value },
                  })
                }
              />
            </Field>
            <Field
              label="面板不透明度"
              hint={`${Math.round(config.appearance.opacity * 100)}%`}
            >
              <input
                type="range"
                min={40}
                max={100}
                step={2}
                value={Math.round(config.appearance.opacity * 100)}
                onChange={(event) =>
                  commit({
                    ...config,
                    appearance: {
                      ...config.appearance,
                      opacity: Number(event.target.value) / 100,
                    },
                  })
                }
              />
            </Field>
            <Field label="图标大小">
              <Segmented
                value={config.appearance.iconSize}
                options={[
                  { value: "small", label: "小" },
                  { value: "medium", label: "中" },
                  { value: "large", label: "大" },
                ]}
                onChange={(value) =>
                  commit({
                    ...config,
                    appearance: { ...config.appearance, iconSize: value },
                  })
                }
              />
            </Field>
          </Section>
        ) : null}

        {tab === "sources" ? (
          <>
            <Section
              title="扫描路径"
              description="递归查找目录下的 .app 包，命中后不再深入应用内部。"
            >
              <ListEditor
                values={config.scanPaths}
                placeholder="/Applications 或 ~/Applications"
                addLabel="添加路径"
                onChange={(list) => setList("scanPaths", list)}
              />
              <Field label="排除规则" hint="支持 * 与 ? 通配，匹配文件名或完整路径">
                <ListEditor
                  values={config.excludePatterns}
                  placeholder="例如 *.prefPane"
                  addLabel="添加规则"
                  onChange={(list) => setList("excludePatterns", list)}
                />
              </Field>
              <div className="row-actions">
                <Button
                  onClick={() => {
                    void api.rescan();
                    showNotice({ kind: "ok", text: "已开始重新扫描" });
                  }}
                >
                  立即重新扫描
                </Button>
                <Button onClick={() => void api.openDataDir()}>打开配置目录</Button>
                {status ? (
                  <span className="muted">
                    应用 {status.appCount} · 自定义 {status.customCount} · 上次扫描{" "}
                    {formatTime(status.lastScanAt)}
                  </span>
                ) : null}
              </div>
            </Section>

            <Section
              title="自定义条目"
              description="文件夹、网址与 Shell 命令，同样支持拼音搜索。"
            >
              {config.customItems.length === 0 ? (
                <p className="muted">还没有自定义条目。</p>
              ) : (
                <ul className="item-list">
                  {config.customItems.map((item) => (
                    <li key={item.id}>
                      <div className="item-text">
                        <span className="item-name">
                          {item.name}
                          <span className={`badge ${item.kind}`}>
                            {CUSTOM_KINDS.find((k) => k.value === item.kind)?.label}
                          </span>
                        </span>
                        <span className="item-target">{item.target}</span>
                      </div>
                      <Button variant="danger" onClick={() => removeCustomItem(item.id)}>
                        删除
                      </Button>
                    </li>
                  ))}
                </ul>
              )}
              <CustomItemForm onSubmit={addCustomItem} onError={showNotice} />
            </Section>
          </>
        ) : null}

        {tab === "shortcut" ? (
          <Section title="快捷键" description="修完即时生效，无需重启。">
            <Field label="唤起 / 隐藏" hint="至少包含一个修饰键">
              <ShortcutRecorder
                value={config.shortcut.toggle}
                onChange={(accelerator) =>
                  commit({
                    ...config,
                    shortcut: { toggle: accelerator },
                  })
                }
              />
            </Field>
            <Field label="命令执行前确认">
              <Switch
                label="命令执行前确认"
                checked={config.general.commandConfirm}
                onChange={(value) => patchGeneral({ commandConfirm: value })}
              />
            </Field>
            <div className="keymap">
              <h3>默认按键</h3>
              <dl>
                <div><dt><kbd>↑</kbd> <kbd>↓</kbd></dt><dd>选择结果</dd></div>
                <div><dt><kbd>↩</kbd></dt><dd>启动选中项</dd></div>
                <div><dt><kbd>⎋</kbd></dt><dd>隐藏面板</dd></div>
                <div><dt><kbd>⌘</kbd> <kbd>K</kbd></dt><dd>清空输入</dd></div>
                <div><dt><kbd>⌘</kbd> <kbd>D</kbd></dt><dd>收藏 / 取消收藏</dd></div>
                <div><dt><kbd>⌘</kbd> <kbd>R</kbd></dt><dd>重新扫描应用</dd></div>
                <div><dt><kbd>⌘</kbd> <kbd>,</kbd></dt><dd>打开设置</dd></div>
              </dl>
            </div>
          </Section>
        ) : null}

        {tab === "about" ? (
          <Section title="关于" description="QuickLaunch 为个人自用工具，全程离线运行。">
            <Field label="版本" hint={status ? `v${status.version}` : "—"}>
              <span className="muted">
                {status
                  ? `共 ${status.itemCount} 个条目，其中应用 ${status.appCount} 个`
                  : "—"}
              </span>
            </Field>
            <Field label="配置备份" hint="导出为 JSON，换机时可直接导入恢复">
              <div className="row-actions">
                <Button onClick={() => void onExport()}>导出配置</Button>
                <Button onClick={() => void onImport()}>导入配置</Button>
              </div>
            </Field>
            <Field label="使用记录" hint="收藏不变，仅清空频率与最近使用">
              <Button
                variant="danger"
                onClick={() => {
                  void api
                    .clearUsage()
                    .then(() => {
                      showNotice({ kind: "ok", text: "使用记录已清空" });
                      return loadStatus();
                    })
                    .catch((err: unknown) =>
                      showNotice({ kind: "error", text: String(err) }),
                    );
                }}
              >
                清空使用记录
              </Button>
            </Field>
            <p className="muted note">
              首次打开若被 Gatekeeper 拦截，可在「系统设置 → 隐私与安全性」中选择仍要打开，
              或执行 <code>xattr -rd com.apple.quarantine /Applications/QuickLaunch.app</code>。
            </p>
          </Section>
        ) : null}

        {notice ? (
          <div className="notice" data-kind={notice.kind}>
            {notice.text}
          </div>
        ) : null}
      </main>
    </div>
  );
}

function ListEditor({
  values,
  placeholder,
  addLabel,
  onChange,
}: {
  values: string[];
  placeholder: string;
  addLabel: string;
  onChange: (values: string[]) => void;
}) {
  const [draft, setDraft] = useState("");

  const add = () => {
    const value = draft.trim();
    if (!value || values.includes(value)) {
      setDraft("");
      return;
    }
    onChange([...values, value]);
    setDraft("");
  };

  return (
    <div className="list-editor">
      {values.map((value) => (
        <div key={value} className="list-row">
          <code>{value}</code>
          <button
            type="button"
            title="移除"
            onClick={() => onChange(values.filter((x) => x !== value))}
          >
            ✕
          </button>
        </div>
      ))}
      <div className="list-add">
        <input
          value={draft}
          placeholder={placeholder}
          spellCheck={false}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              add();
            }
          }}
        />
        <Button onClick={add}>{addLabel}</Button>
      </div>
    </div>
  );
}

function CustomItemForm({
  onSubmit,
  onError,
}: {
  onSubmit: (item: CustomItem) => void;
  onError: (notice: Notice) => void;
}) {
  const [kind, setKind] = useState<ItemKind>("folder");
  const [name, setName] = useState("");
  const [target, setTarget] = useState("");
  const [cwd, setCwd] = useState("");
  const [confirm, setConfirm] = useState(true);
  const [keywords, setKeywords] = useState("");

  const reset = () => {
    setName("");
    setTarget("");
    setCwd("");
    setKeywords("");
    setConfirm(true);
  };

  const submit = async () => {
    const trimmedName = name.trim();
    const trimmedTarget = target.trim();
    if (!trimmedName) {
      onError({ kind: "error", text: "名称不能为空" });
      return;
    }
    try {
      // 后端做最终校验：目录是否存在、URL 协议是否合法
      await api.validateTarget(kind, trimmedTarget);
    } catch (err) {
      onError({ kind: "error", text: String(err) });
      return;
    }
    onSubmit({
      id: `custom-${Date.now().toString(36)}`,
      kind,
      name: trimmedName,
      target: trimmedTarget,
      cwd: cwd.trim() ? cwd.trim() : undefined,
      confirm: kind === "command" ? confirm : false,
      keywords: keywords
        .split(/[,，\s]+/)
        .map((k) => k.trim())
        .filter(Boolean),
    });
    reset();
  };

  return (
    <div className="custom-form">
      <h3>新增条目</h3>
      <div className="custom-grid">
        <label>
          <span>类型</span>
          <Segmented value={kind} options={CUSTOM_KINDS} onChange={setKind} />
        </label>
        <label>
          <span>名称</span>
          <input
            value={name}
            placeholder="显示在结果列表里的名字"
            onChange={(event) => setName(event.target.value)}
          />
        </label>
        <label className="span-2">
          <span>
            {kind === "folder" ? "目录路径" : kind === "url" ? "网址" : "Shell 命令"}
          </span>
          <input
            value={target}
            spellCheck={false}
            placeholder={
              kind === "folder"
                ? "~/Projects"
                : kind === "url"
                  ? "https://github.com"
                  : "npm run dev"
            }
            onChange={(event) => setTarget(event.target.value)}
          />
        </label>
        {kind === "command" ? (
          <label>
            <span>工作目录（可选）</span>
            <input
              value={cwd}
              spellCheck={false}
              placeholder="~/Projects/my-app"
              onChange={(event) => setCwd(event.target.value)}
            />
          </label>
        ) : null}
        <label>
          <span>关键词（可选，便于搜索）</span>
          <input
            value={keywords}
            placeholder="用空格或逗号分隔"
            onChange={(event) => setKeywords(event.target.value)}
          />
        </label>
        {kind === "command" ? (
          <label className="inline">
            <Switch
              label="执行前确认"
              checked={confirm}
              onChange={setConfirm}
            />
            <span>执行前需要确认</span>
          </label>
        ) : null}
      </div>
      <div className="row-actions">
        <Button variant="primary" onClick={() => void submit()}>
          添加条目
        </Button>
      </div>
    </div>
  );
}

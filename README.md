# QuickLaunch

轻量级 macOS 应用启动器。按 `Option + Space` 唤起，输入关键词即可启动应用、打开文件夹与网址、执行自定义命令。

面向个人自用：**不联网、不申请额外系统权限、无 Apple 签名**，构建与打包全部交给 GitHub Actions，本机无需安装 Xcode。

---

## 快速开始

```bash
npm install          # 安装前端依赖
npm run tauri:dev    # 需要本机有 Rust 工具链，仅开发用
```

本机没有 Rust 工具链时，直接走 CI：

```bash
git push             # 推送后 GitHub Actions 自动产出 universal .dmg / .app
```

产物在 Actions 页面的 **Artifacts → QuickLaunch-macOS-universal**。

## 本地可做的验证

没有 Rust 工具链也能挡掉大部分问题：

```bash
npm run typecheck              # tsc --noEmit，strict + noUnusedLocals
npm run build                  # 前端生产构建（target: safari15）
python3 scripts/verify.py      # 静态契约校验，见下
python3 scripts/make_icons.py  # 重新生成 PNG / ICNS / ICO / 托盘图标
```

`scripts/verify.py` 专门检查**只在运行时才暴露**的那类错误，并已挂进 CI 的前端环节：

| 检查项 | 挡掉什么 |
|---|---|
| `tauri.conf.json` 必填字段与 identifier | identifier 仍是 `com.tauri.dev` 导致打包失败 |
| `bundle.icon` 文件存在性 | 图标缺失导致 `generate_context!` 编译失败 |
| `frontendDist` 目录存在性 | 没先 build 就编译 |
| 前端 `invoke("x")` ↔ `generate_handler!` | 命令名拼错，运行时才报 command not found |
| 前端 `listen("x")` ↔ Rust `emit("x")` | 事件永不触发，界面静默不刷新 |
| 运行时窗口 label ↔ capabilities | 新窗口漏配权限，其 IPC 全被拒绝 |

Rust 侧无法本地编译，因此 CI 是唯一的编译验证环节。


## 首次运行（无签名版本）

1. 从 Artifacts 下载并解压，把 `QuickLaunch.app` 拖入 `/Applications`。
2. 右键点击应用 → **打开** → 在弹窗中再点 **打开**。
3. 若提示「已损坏」，执行：

   ```bash
   sudo xattr -rd com.apple.quarantine /Applications/QuickLaunch.app
   ```

   之后重新右键打开即可。

## 快捷键

| 操作 | 按键 |
|---|---|
| 唤起 / 隐藏 | `Option + Space`（可在设置中修改） |
| 选择 | `↑` / `↓`（或 `⌘N` / `⌘P`） |
| 启动 | `↩` |
| 隐藏面板 | `⎋` |
| 清空输入 | `⌘K` |
| 收藏 / 取消收藏 | `⌘D` |
| 重新扫描应用 | `⌘R` |
| 打开设置 | `⌘,` |

## 搜索能做什么

- **名称模糊匹配**：`chrme` 也能命中 Chrome（子序列匹配）。
- **拼音全拼与首字母**：`wx` → 微信，`weixin` 同样命中。
- **跨分隔符与驼峰**：`vsc` / `vscode` → Visual Studio Code。
- **多关键词**：空格分隔，全部命中才算匹配。
- **排序权重**：置顶 1500 > 收藏 1000 > 使用频率（每次 +10，上限 500）> 最近使用（半衰期 7 天，上限 300）> 匹配质量（精确 1000 → 模糊 220）。

## 项目结构

```text
QuickLaunch/
├── index.html                 # 两个窗口共用的入口，靠 ?view=settings 分流
├── src/                       # 前端（React + TypeScript）
│   ├── main.tsx
│   ├── types.ts               # 与 Rust 侧一一对应的类型
│   ├── lib/api.ts             # 全部 IPC 调用的唯一出口
│   ├── lib/appearance.ts      # 主题 / 外观 / 快捷键格式化
│   ├── components/            # AppIcon / Highlight / Controls
│   └── views/                 # Launcher（主面板）、Settings（设置窗口）
├── src-tauri/                 # Rust 后端
│   └── src/
│       ├── lib.rs             # 装配与启动顺序
│       ├── commands.rs        # 全部 IPC 命令
│       ├── scanner.rs         # 扫描 .app 与 Info.plist 解析
│       ├── icons.rs           # ICNS → PNG data URL
│       ├── search.rs          # 索引构建与排序
│       ├── config.rs          # 配置原子读写
│       ├── state.rs           # 运行态与使用统计
│       ├── tray.rs            # 菜单栏图标
│       └── model.rs           # 领域模型
├── scripts/make_icons.py      # 生成 PNG / ICNS / ICO / 托盘图标
└── .github/workflows/build.yml
```

## 配置文件

位于 `~/Library/Application Support/com.zhaoyajun.quicklaunch/`：

| 文件 | 说明 |
|---|---|
| `config.json` | 全部设置、自定义条目、收藏与置顶 |
| `usage.json` | 使用频率与最近使用时间 |
| `index-cache.json` | 上次扫描结果，用于冷启动时立刻出结果 |

配置采用「临时文件 + `rename`」原子写入；若文件损坏，会被改名为 `config.json.broken` 备份并回落到默认配置。

## 与 PRD 的差异

实现过程中对若干技术选型做了替换，逐条理由、影响面与回退方式见 **[docs/DECISIONS.md](docs/DECISIONS.md)**。要点：

- 使用统计由 SQLite 改为单文件 JSON（省掉 `rusqlite` 的本地 C 工具链依赖）。
- 命令执行与系统集成改用 Rust 原生实现，不引入 `tauri-plugin-shell` 等插件。
- 菜单栏图标使用 Tauri v2 内置能力，无需 `tauri-plugin-tray`。
- Dock 图标默认开启（Accessory 激活策略下 macOS 取焦不稳定）。

## 已知限制

- 系统自带应用（`/System/Applications` 下的部分条目）图标存放在系统资产目录中，磁盘上没有独立 `.icns`，此时列表会退化为字母徽标。第三方应用不受影响。
- 应用未做公证，首次打开必须走上面的放行步骤。
- 未实现自动更新，升级需手动下载新版本（PRD 已将其列为非目标）。

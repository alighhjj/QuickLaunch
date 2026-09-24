# 实现决策与技术偏差记录

本文逐条记录实现过程中**偏离 PRD 原始描述**的地方：改了什么、为什么改、
影响面有多大、想回退该怎么做。目的是让后续迭代不必重新推导这些取舍。

原始 PRD 见 [../PRD.md](../PRD.md)。

---

## 1. PRD 开放问题的落地选择

PRD 第 12 节列了 7 个开放问题，实现时做了如下决定：

| 问题 | 决定 | 理由 |
|---|---|---|
| 产品名称 | `QuickLaunch` | PRD 已给出暂定名，未收到更改要求 |
| 默认全局快捷键 | `Option + Space` | 与 PRD 13.1 一致；注册失败时会自动回滚到上一次可用的组合并提示 |
| 前端框架 | **React 18 + TypeScript + Vite** | 生态最成熟，`tsc --noEmit` 可独立做类型校验 |
| 剪贴板历史 / 窗口切换 | 不实现 | PRD 2.3 已列为非目标 |
| 多语言 | 不实现 | PRD 2.3 已列为非目标，界面文案直接写中文 |
| 自动更新 | 不实现 | PRD 2.3 已列为非目标 |
| 命令变量替换 | 不实现 | 属于 P2，且 `{clipboard}` 一类插值会显著扩大命令注入面 |

---

## 2. 使用统计：SQLite → 单文件 JSON

**PRD 原文**：6.2 技术选型表「本地存储：`tauri-plugin-store` 或 SQLite」、8.2「使用记录（SQLite）」。

**实现**：配置与使用记录都存 JSON（`config.json` / `usage.json`）。

**理由**：

- 条目量级在 10³ 以内，全量读入内存不过几十 KB，查询就是一次哈希表查找，
  SQLite 的索引与事务能力在此完全用不上。
- `rusqlite` 的 `bundled` 特性需要本地 C 工具链。本项目的构建**全部发生在
  GitHub Actions 上**，而 macOS runner 的 Xcode Command Line Tools 版本不受控；
  引入一个可有可无的原生依赖，等于把「首次 CI 能否跑绿」押在环境上。
- JSON 可读可 diff，配置出问题时用户自己就能看明白。

**影响**：无功能缺失。PRD 8.2 定义的 `usage(id, item_id, count, last_used_at)`
四个字段在 `UsageEntry { count, last_used_at }` 中完整保留（`id` 即 map 的键）。

**回退方式**：`state.rs` 中 `Usage` 的读写被收拢在 `load` / `save` / `record` /
`get` / `recent_ids` 五个方法内。要换回 SQLite，只需替换这五个方法的实现，
`search.rs` 与 `commands.rs` 无需改动。

---

## 3. 命令执行：`tauri-plugin-shell` → `std::process::Command`

**PRD 原文**：6.3「命令执行：使用 `tauri-plugin-shell`，执行前弹窗确认」。

**实现**：`commands::run_shell_command` 直接用 `std::process::Command` 调 `/bin/sh -lc`。

**理由**：

- 插件在 v2 中要求为每条被允许的命令在 `capabilities` 里声明 scope，
  而本项目的命令内容由用户在设置界面自由填写，无法预先枚举。
- 不走 shell 的三种目标（应用 / 文件夹 / URL）改用 `/usr/bin/open` 并以
  **argv 方式**传参，路径里的空格、引号、`;` 都无法被解释成命令，天然免疫注入。

**安全边界**（对应 PRD 5.4）：自定义命令按定义就是交给 shell 执行的，这是功能
而非漏洞。真正的控制点是**执行前确认**，且该逻辑在后端强制执行
（`launch` 命令中 `needs_confirm && !confirmed` 直接返回失败），
绕过前端直接调 IPC 也执行不了。

**影响**：无需在 `capabilities/default.json` 中开放 `shell:*` 权限。

---

## 4. 菜单栏图标：`tauri-plugin-tray` → Tauri v2 内置能力

**PRD 原文**：6.2 列出 `tauri-plugin-tray`。

**实现**：使用 `tauri::tray::TrayIconBuilder`，Cargo 中开启 `tray-icon` feature。

**理由**：Tauri v2 已把托盘能力并入核心，不存在独立的 `tauri-plugin-tray` 包。
PRD 该行按 v1 的插件划分编写，属于版本差异而非设计分歧。

---

## 5. 配置目录：改用 Tauri 约定的路径

**PRD 原文**：6.3「配置存储：`~/Library/Application Support/QuickLaunch/config.json`」。

**实现**：`app.path().app_config_dir()`，即
`~/Library/Application Support/com.zhaoyajun.quicklaunch/`。

**理由**：走 Tauri 官方目录 API，避免手写平台路径；目录名与 bundle identifier
绑定，重命名产品时不会残留孤儿目录。设置页「关于」中提供了「打开配置目录」按钮，
用户无需记忆路径。

---

## 6. Dock 图标默认开启

**PRD 原文**：6.3 窗口配置含 `skipTaskbar: true`，暗示不占 Dock。

**实现**：提供「在 Dock 中显示图标」开关，**默认开启**。

**理由**：macOS 的 `Accessory` 激活策略下，应用不会进入 Dock 与 Cmd+Tab，
但随之而来的是**窗口取焦不稳定** —— 全局快捷键唤起的面板可能抢不到键盘焦点，
直接表现为「按了快捷键但输入不进去」。启动器的核心体验就是「按一下就能打字」，
不能为了省一个 Dock 图标牺牲它。

**影响**：想彻底隐藏 Dock 的用户可以在「设置 → 通用」关闭，代价是可能需要
手动点一下面板才能输入。

---

## 7. 图标提取：只透传，不解码

**实现**：`icons.rs` 按 ICNS 容器格式切出分块，挑一个尺寸合适的 **PNG 分块原样
输出**为 data URL，不做任何解码或格式转换。

**理由**：

- 零图像库依赖（不需要 `image` 的完整解码能力，也避开了 `icns` crate 的版本风险）。
- 优先取 128/256px 而非最大的 1024px：显示尺寸是 24~40pt，再大的分块只是成倍
  放大 base64 体积。

**已知缺口**：macOS 11 之后，系统自带应用的图标被移入系统资产目录（`Assets.car`），
磁盘上没有独立 `.icns`，此时提取失败，前端退化为**字母徽标**（名称首字母 + 主题色底）。
`/Applications` 下的第三方应用不受影响。

**若要补全**：引入 `objc2` 系 crate 调 `NSWorkspace.icon(forFile:)` 取 NSImage 再
转 PNG，代价是新增一组原生依赖，与「CI 首次构建必须一次通过」的目标冲突，故暂缓。

---

## 8. 前端依赖面收窄

**实现**：前端只依赖 `@tauri-apps/api` 与 `@tauri-apps/plugin-dialog`。

开机自启、系统通知、窗口显示隐藏、命令执行全部封装成自定义 Rust 命令，
不由前端直接调用插件 JS 包。

**理由**：JS 包与 Rust crate 分别发版，一旦 minor 版本错位就会出现
「命令存在但调不通」这类难排查的问题。把插件调用收敛到 Rust 一侧后，
前端与后端的契约只剩 `src/types.ts` 里那几个结构体。

**唯一例外**是文件对话框：导入/导出配置需要系统原生文件选择器，
同步阻塞式调用在命令里不好用，因此保留 `@tauri-apps/plugin-dialog`，
并在 `capabilities/default.json` 中只开放 `dialog:*`。

---

## 9. CSP 设为 `null`

**实现**：`tauri.conf.json` 中 `app.security.csp = null`。

**理由**：应用完全离线，不加载任何远程资源，没有内联脚本注入面。
若启用 CSP 却漏配 `connect-src ipc:` 一类的协议白名单，会出现「界面正常、
所有 IPC 全部失败」的静默故障 —— 对一个自用工具来说，这个风险高于收益。

**若要收紧**：建议的起点是
`default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; connect-src 'self' ipc: http://ipc.localhost`
配置后**必须**在真机上验证搜索与启动功能仍然可用。

---

## 10. 启动顺序上的两处刻意安排

这两点不属于偏差，但改动时极易踩坑，因此记录：

1. **`apply_config` 先做副作用再落盘。**
   快捷键注册与开机自启都属于「可能失败」的操作。若先落盘再注册，
   注册失败后配置里留着一个不可用的快捷键，下次启动直接失去唤起能力。
   现在的顺序是：注册失败 → 不落盘 → 返回错误 → 前端提示。
   同时 `apply_shortcut` 在失败时会把上一次可用的组合重新注册回去，
   保证任何情况下都至少有一个热键可用。

2. **索引分两步加载。**
   启动时先用 `index-cache.json` 填索引（同步、微秒级），让冷启动后第一次
   按下快捷键立刻有结果；随后后台线程全量扫描并通过 `index-updated` 事件
   通知前端刷新。若反过来只等扫描完成，第一次唤起会看到空列表。

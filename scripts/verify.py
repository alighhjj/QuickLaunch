#!/usr/bin/env python3
"""QuickLaunch 静态契约校验。

本机（以及任何没有 Rust 工具链的环境）无法编译 Rust，而下面这几类错误
**只在运行时才暴露**，且往往第一次按下快捷键才发现：

* 前端 `invoke("x")` 的名字和 `generate_handler!` 里的对不上 → 报 command not found
* 前端 `listen("x")` 的事件后端从来没 `emit` 过 → 界面永远不刷新，静默无反应
* `tauri.conf.json` 里 `bundle.icon` 指向不存在的文件 → `generate_context!` 编译失败
* 新建的窗口 label 没写进 capabilities → 该窗口所有 IPC 被 ACL 拒绝

所以把这几项固化成脚本，挂进 CI 的前端环节，几秒钟就能挡掉，
不必等几十秒的 macOS 编译跑完才知道。

用法：python3 scripts/verify.py
退出码 0 = 全部通过，1 = 有致命问题。
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TAURI_DIR = ROOT / "src-tauri"
SRC_DIR = ROOT / "src"

errors: list[str] = []
warnings: list[str] = []


def fail(message: str) -> None:
    errors.append(message)


def warn(message: str) -> None:
    warnings.append(message)


# ── 1. tauri.conf.json ──────────────────────────────────────────────
config = {}
config_path = TAURI_DIR / "tauri.conf.json"
try:
    config = json.loads(config_path.read_text(encoding="utf-8"))
except FileNotFoundError:
    fail(f"缺少 {config_path.relative_to(ROOT)}")
except json.JSONDecodeError as err:
    fail(f"tauri.conf.json 不是合法 JSON：{err}")

if config:
    identifier = config.get("identifier", "")
    if not identifier:
        fail("tauri.conf.json 缺少 identifier")
    elif identifier in {"com.tauri.dev", "com.tauri.app"}:
        fail(f"identifier 仍是默认值 {identifier}，打包会失败")
    for key in ("productName", "version"):
        if not config.get(key):
            fail(f"tauri.conf.json 缺少 {key}")

    # bundle.icon 里列的文件必须真实存在，否则 generate_context! 直接失败
    for icon in config.get("bundle", {}).get("icon", []):
        if not (TAURI_DIR / icon).is_file():
            fail(f"bundle.icon 指向的文件不存在：{icon}")

    # frontendDist 目录必须存在（CI 里要先 npm run build）
    dist = config.get("build", {}).get("frontendDist")
    if dist and not (TAURI_DIR / dist).resolve().is_dir():
        fail(
            f"frontendDist 目录不存在：{dist}"
            "（本地跑之前先执行 npm run build）"
        )

    windows = config.get("app", {}).get("windows", [])
    if not windows:
        fail("tauri.conf.json 未定义任何窗口")


# ── 2. invoke ↔ generate_handler! ───────────────────────────────────
INVOKE_RE = re.compile(r"""invoke\s*(?:<[^>()]*>)?\s*\(\s*["']([A-Za-z0-9_]+)["']""")
HANDLER_RE = re.compile(r"generate_handler!\s*\[(.*?)\]", re.DOTALL)
COMMAND_REF_RE = re.compile(r"(\w+)\s*::\s*(\w+)")

frontend_calls: dict[str, set[str]] = {}
for path in sorted(SRC_DIR.rglob("*.ts*")):
    text = path.read_text(encoding="utf-8")
    for name in INVOKE_RE.findall(text):
        frontend_calls.setdefault(name, set()).add(str(path.relative_to(ROOT)))

lib_rs = (TAURI_DIR / "src" / "lib.rs").read_text(encoding="utf-8")
handler_block = HANDLER_RE.search(lib_rs)
registered: set[str] = set()
if not handler_block:
    fail("lib.rs 中找不到 generate_handler![...]")
else:
    for _module, name in COMMAND_REF_RE.findall(handler_block.group(1)):
        registered.add(name)

for name in sorted(frontend_calls):
    if name not in registered:
        where = "、".join(sorted(frontend_calls[name]))
        fail(f"前端调用了未注册的命令 `{name}`（{where}）")

for name in sorted(registered):
    if name not in frontend_calls:
        warn(f"命令 `{name}` 已注册但前端未使用")


# ── 3. listen ↔ emit ────────────────────────────────────────────────
LISTEN_RE = re.compile(r"""listen\s*(?:<[^>()]*>)?\s*\(\s*["']([A-Za-z0-9_:-]+)["']""")
EMIT_RE = re.compile(r"""emit(?:_to)?\s*\(\s*["']([A-Za-z0-9_:-]+)["']""")

listened: set[str] = set()
for path in sorted(SRC_DIR.rglob("*.ts*")):
    listened.update(LISTEN_RE.findall(path.read_text(encoding="utf-8")))

rust_text = "\n".join(
    path.read_text(encoding="utf-8") for path in sorted((TAURI_DIR / "src").rglob("*.rs"))
)
emitted = set(EMIT_RE.findall(rust_text))

for name in sorted(listened):
    if name not in emitted:
        fail(f"前端监听了事件 `{name}`，但 Rust 侧从未 emit —— 该监听永远不会触发")

for name in sorted(emitted - listened):
    warn(f"Rust 侧 emit 了事件 `{name}`，前端未监听")


# ── 4. 运行时窗口 label ↔ capabilities ───────────────────────────────
CAP_WINDOW_RE = re.compile(r"""WebviewWindowBuilder::new\s*\(\s*[^,]+,\s*["']([^"']+)["']""")
runtime_labels = set(CAP_WINDOW_RE.findall(rust_text))

try:
    caps = json.loads((TAURI_DIR / "capabilities" / "default.json").read_text(encoding="utf-8"))
    allowed = set(caps.get("windows", []))
    for label in sorted(runtime_labels):
        if label not in allowed:
            fail(
                f"代码创建了 label 为 `{label}` 的窗口，"
                f"但 capabilities 的 windows 未包含它，该窗口的 IPC 会被拒绝"
            )
    for label in sorted(allowed - runtime_labels - {"main"}):
        warn(f"capabilities 里的窗口 `{label}` 在代码中未出现（若是配置里声明的窗口可忽略）")
except FileNotFoundError:
    fail("缺少 src-tauri/capabilities/default.json")
except json.JSONDecodeError as err:
    fail(f"capabilities/default.json 不是合法 JSON：{err}")


# ── 5. 图标齐备性 ───────────────────────────────────────────────────
required_icons = ["icon.icns", "icon.ico", "32x32.png", "128x128.png", "128x128@2x.png"]
missing = [name for name in required_icons if not (TAURI_DIR / "icons" / name).is_file()]
if missing:
    fail(f"缺少图标文件：{'、'.join(missing)}（跑 python3 scripts/make_icons.py 生成）")


# ── 输出 ───────────────────────────────────────────────────────────
print(f"校验根目录：{ROOT}")
print(f"  命令 {len(registered)} 个 · 前端调用点 {len(frontend_calls)} 个")
print(f"  前端监听事件 {len(listened)} 个 · Rust emit 事件 {len(emitted)} 个")
print(f"  运行时窗口 label：{sorted(runtime_labels) or '无'}")

if warnings:
    print("\n警告：")
    for message in warnings:
        print(f"  ! {message}")

if errors:
    print("\n错误：")
    for message in errors:
        print(f"  ✗ {message}")
    print(f"\n未通过：{len(errors)} 项错误")
    sys.exit(1)

print("\n全部检查通过")

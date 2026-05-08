# Windows 开发与调试指南

适用：你已在 Windows 机器上 `git pull` 了仓库，想直接跑 `pnpm tauri dev`。

## 一次性环境

### 1. 必装

| 工具 | 说明 |
|---|---|
| **Rust 1.77+** | https://www.rust-lang.org/tools/install — `rustup-init.exe`，默认装稳定版即可 |
| **Visual Studio Build Tools** | https://visualstudio.microsoft.com/visual-cpp-build-tools/ — 勾选 "Desktop development with C++"（MSVC 链接器是 Tauri 必需） |
| **Node.js 20+ (LTS) 或 24** | https://nodejs.org/ — 装好后 `npm install -g pnpm` |
| **WebView2 Runtime** | Win11 已内置；Win10 若没有去 https://developer.microsoft.com/microsoft-edge/webview2/ 装 Evergreen Standalone |
| **Git for Windows** | https://git-scm.com/download/win |

可选：

| 工具 | 说明 |
|---|---|
| Windows Terminal | 体验更好的终端 |
| VS Code + rust-analyzer | IDE |

### 2. 拉取 sing-box + wintun 二进制

仓库里 `src-tauri/binaries/` 目录是 gitignored 的（二进制不入库）。
首次需要在 Windows 终端里跑：

```powershell
node scripts/fetch-singbox.mjs
```

脚本会下载 sing-box 1.13.11 的 Windows + Linux 二进制和 wintun 0.14.1
到 `src-tauri/binaries/`，并把 `sing-box-x86_64-pc-windows-msvc.exe`
按 Tauri sidecar 命名规则准备好。

> 脚本会依次尝试 `tar.exe`（Win10/11 自带）→ PowerShell `Expand-Archive`
> 解压，无需额外装 `unzip`。Linux 端仍走 `unzip`（`apt install unzip` 即可）。

### 3. 装前端依赖

```powershell
pnpm install
```

## 跑起来

```powershell
pnpm tauri dev
```

**首次构建需要 5–15 分钟**（下载并编译 ~600 个 Rust crate；含 wry/webkit2gtk
的 Windows 等价物）。后续增量编译几秒。

成功后会自动弹出 Tauri 窗口（在 Windows 上是真原生窗口，**不需要 noVNC**）。
Dashboard 上的 "version" 字段应该看到 sing-box 版本输出。

## TUN 模式权限

Windows 上的 sing-box TUN 需要：

1. **wintun.dll** 与 `sing-box.exe` 同目录 — `fetch-singbox.mjs` 已经放好。
2. **Administrator 权限** — sidecar 没有自动 elevate 的 manifest，启动 TUN 时需要：
   - 以管理员身份打开终端，再 `pnpm tauri dev`，**或**
   - 暂时只用非 TUN inbound（`mixed` / `socks` / `http`）调 UI 流程
3. **Defender / 安全软件** 可能拦截 wintun 加载第一次，按提示放行。

## 常见 Windows 坑

| 现象 | 处理 |
|---|---|
| `error: linker 'link.exe' not found` | 没装 VS Build Tools 的 C++ workload，重装时勾上 |
| `WebView2Loader.dll missing` | 装 WebView2 Runtime |
| `pnpm: cannot run scripts` | PowerShell 执行策略限制：`Set-ExecutionPolicy -Scope CurrentUser RemoteSigned` |
| `node-gyp` 报错 | `npm config set msvs_version 2022` |
| 防火墙弹窗 | sing-box 创建 TUN 接口和 vite dev server 监听 1420 都会弹，全部 Allow |
| 行尾 CRLF 警告 | `.gitattributes` 已设 `* text=auto`，shell 脚本/Linux 二进制保留 LF |
| `cargo` 慢 | 用 sparse 索引：`setx CARGO_REGISTRIES_CRATES_IO_PROTOCOL sparse` |

## 烟雾测试已实现的功能

进 Tauri 窗口后：

1. **Dashboard** — 自动调 `core_version`，应显示 sing-box 1.13.11 + 编译 tags
2. **Config** 页 → 点 "Add config" → "Add local file" → 选 `src-tauri/resources/example-tun-config.json`
   - 摘要应显示：1 inbound (tun) / 3 outbounds / 2 rules / final=proxy
3. 点卡片设为 active → sing-box 自动启动；可在 Dashboard 看到 running 状态
4. **Route** / **DNS** 页可对路由 / DNS 规则做本地 override 增删改

## 不在 Windows 上做的事

- **不要** 跑 `bash scripts/grant-tun-cap.sh`（Linux 专属）
- **不要** 跑 `bash scripts/dev-container.sh ...`（也是 Linux 用的 docker dev 路径）
- 这两个脚本对 Windows 调试无意义；忽略即可。

## 把 Windows 上的改动 push 回去

```powershell
git add -A
git commit -m "..."
git push
```

Linux 端再 `git pull` 即可。`Cargo.lock` 与 `pnpm-lock.yaml` 都跨平台
确定性的，提交它们没问题（实际上必须提交）。

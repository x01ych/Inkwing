# macOS 开发与调试指南

本项目对 macOS 的支持范围（截至当前）：

- ✅ **本地 dev** (`pnpm tauri dev`) — Apple Silicon 与 Intel 都可
- ✅ sing-box sidecar 二进制（`x86_64-apple-darwin` + `aarch64-apple-darwin`）由 `scripts/fetch-singbox.mjs` 拉取
- ❌ **CI 出 .dmg/.app 包** — `.github/workflows/release.yml` 还没加 macos runner
- ❌ **代码签名 + 公证** — 无签名包用户下载会被 Gatekeeper 拒绝；待加 Apple Developer 凭据后再做

也就是说：**你能在自己 Mac 上 dev 调试，但要打包给别人用还需要后续工作**。

## 一次性环境

| 工具 | 装法 |
|---|---|
| **Xcode CLI tools** | `xcode-select --install`（弹窗确认） |
| **Homebrew** | https://brew.sh — 一行 curl |
| **Rust 1.77+** | `brew install rustup-init && rustup-init -y`（或直接 https://www.rust-lang.org/tools/install） |
| **Node.js 20+ / 24** | `brew install node` |
| **pnpm** | `npm install -g pnpm` |

WebView2 不需要装——macOS 上 Tauri 用系统自带的 WKWebView。

## clone + 启动

```bash
git clone https://github.com/x01ych/Inkwing.git
cd Inkwing

# 下载 sing-box + wintun。--skip-foreign 只拿你 Mac host 对应的那一个
# (~18 MiB)，不带 flag 会把 linux/windows/intel-mac/arm-mac 全下 (~80 MiB)
node scripts/fetch-singbox.mjs --skip-foreign

pnpm install
pnpm tauri dev   # 首次 cargo build ~10 min
```

成功后会弹出 Tauri 窗口，与 Linux/Windows 上一样。

## TUN 模式在 macOS 上

macOS 的 TUN 通过 `utun` 设备：

- **必须 root**：sing-box 需要 `setuid root` 或 `sudo` 才能 open `/dev/utun*`。我们没有签名 + NetworkExtension entitlement，所以 dev 模式下唯一办法就是用管理员/sudo 启动。
- **dev 时绕过**：开 Config 页关闭 TUN switch，只用 mixed/socks/http 端口走代理。这种模式不需要任何特权。
- **想要 TUN**：在终端 `sudo pnpm tauri dev`（不推荐，会让整个 Vite/Cargo 都跑在 root）。或者 build release 后 `sudo /Applications/Inkwing.app/Contents/MacOS/Inkwing`（同样不优雅）。
- **正确解决方案**：申请 Apple Developer 账号（$99/年）→ 给 sing-box.exe 加 `com.apple.developer.networking.networkextension` entitlement → 用户授权一次后即可不需 sudo。这是 v1.1 的事。

## 常见 macOS 坑

| 现象 | 处理 |
|---|---|
| `pnpm tauri dev` 报 `failed to find tool: cc` | `xcode-select --install` 没装好，手动重装 |
| WKWebView 渲染白屏 | macOS 11+ 应该没事；老系统升级 |
| dev 启动后 sing-box "operation not permitted" | TUN 配置且非 root；改 settings 关闭 TUN，或 sudo 启动 |
| `binary "sing-box" not found at known path` | 没跑 `node scripts/fetch-singbox.mjs`，或下载只跑了 `--skip-foreign` 但 host 检测错了。检查 `src-tauri/binaries/sing-box-{aarch64,x86_64}-apple-darwin` 是否存在 |
| `resource path 'binaries/wintun-amd64.dll' doesn't exist` | 旧版 fetch 脚本不会在 macOS 上创建占位。`git pull` 拉新版后再 `node scripts/fetch-singbox.mjs --skip-foreign`，会自动 touch 一个 0 字节的 `wintun-amd64.dll`。Mac 永远不会加载它，纯粹让 Tauri 的 bundle.resources 检查通过 |
| 防火墙弹窗 | 第一次给 sing-box / vite 端口都点 "Allow" |
| 升级到新版 sing-box | 改 `scripts/fetch-singbox.mjs` 顶部 `SING_BOX_VERSION`，重跑脚本 |

## 调本项目当前实现的功能

跑通后参考 [`docs/WINDOWS-DEV.md`](./WINDOWS-DEV.md) 第 "烟雾测试已实现的功能" 段——UI 行为完全一致，只是 macOS 上用 Cmd 替代 Ctrl 快捷键。

## 不在 macOS 上做的事

- 不要 `bash scripts/grant-tun-cap.sh` —— Linux 专属，macOS 上 setcap 不存在
- 不要 `bash scripts/dev-container.sh ...` —— Linux 容器开发路径
- 这些脚本 macOS 上无意义

## 把改动 push 回去

```bash
git add -A
git commit -m "..."
git push
```

`Cargo.lock` 与 `pnpm-lock.yaml` 跨平台确定性，提交即可。

## 后续要做的（项目侧，不是你需要做的）

1. `.github/workflows/release.yml` 加 `macos-14` (Apple Silicon) + 可选 `macos-13` (Intel) matrix entry
2. `tauri.conf.json` `bundle.targets` 加 `dmg` / `app`
3. 加 `bundle.macOS.minimumSystemVersion` 之类的 metadata
4. 接 Apple Developer 凭据做签名 + 公证（Gatekeeper-friendly）
5. NetworkExtension entitlement 让 TUN 不需要 sudo

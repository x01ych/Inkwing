<div align="center">
  <img src="src-tauri/icons/icon.png" alt="Inkwing" width="120" height="120">

  <h1>Inkwing</h1>

  <p>
    <strong>一款直接运行你既有 sing-box 配置、零失真不重写源文件的桌面客户端</strong>
  </p>

  <p>
    <a href="#项目介绍">📖 项目介绍</a> •
    <a href="#功能特性">✨ 功能特性</a> •
    <a href="#工作原理">🏗️ 工作原理</a> •
    <a href="#快速开始">🚀 快速开始</a> •
    <a href="#开发">🛠️ 开发</a> •
    <a href="#文档">📚 文档</a>
  </p>

  <p>
    <a href="README.md">🇺🇸 English</a> •
    <a href="README.zh.md">🇨🇳 中文</a>
  </p>

  <p>
    <img src="https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-blue?style=for-the-badge&logo=tauri" alt="Platform">
    <img src="https://img.shields.io/badge/stack-Tauri%202%20%2B%20React%20%2B%20shadcn-purple?style=for-the-badge" alt="Stack">
    <img src="https://img.shields.io/badge/license-MIT-green?style=for-the-badge" alt="License">
  </p>
</div>

---

## 项目介绍

Inkwing 是围绕 [sing-box](https://github.com/SagerNet/sing-box) 内核构建的桌面客户端。它加载你已经写好的 sing-box JSON 配置，把 sing-box 跑起来，并把路由、DNS、代理、连接、日志全部可视化。

**Inkwing 不生成配置**，没有 step-by-step 的配置向导。你需要先有一份完整的 sing-box JSON —— 自己手写、用 [sub-store](https://github.com/sub-store-org/Sub-Store) 生成、由订阅商提供，或者其他兼容 sing-box 的工具产出 —— Inkwing 从这里接手。

你导入的源 `.json` **永远不会被修改**。在 GUI 里做的所有编辑都存在独立的 overrides 层；启动时 Inkwing 把 *源 ⊕ overrides* 合并成 runtime config 喂给 sing-box。这意味着你精心维护的注释、字段顺序、Inkwing 不认识的字段，全部按字节保留。

## 功能特性

- **多 config 库**：支持本地文件 / 粘贴 JSON / 从订阅 URL 拉取。任意切换，sing-box 自动重启。
- **Route 编辑器**：查看 / 添加 / 屏蔽 / 修改合并后的 `route.rules` 与 `route.rule_set`。对源规则的修改会降级为本地 override，可一键还原。
- **DNS 编辑器**：`dns.servers` 与 `dns.rules` 同样模型。常见 server type 字段化展示，匹配器结构化编辑，所有 modifications / masks 都是 per-config。
- **代理**：分组卡片、延迟测试、⚡ 节点测速（保留 group 原本选中的节点，避免点测速结果反过来切换 active 节点）。
- **实时日志**：虚拟化列表、级别过滤、搜索 payload、CSV 导出。
- **连接**：Active + Closed 双 tab，host / process / rule 列，右键关闭连接。自动过滤掉 Inkwing 自己访问 clash_api 的流量。
- **订阅管理**：保存订阅 URL，支持手动 / 按源定时刷新；每次 fetch 写入新的 library entry，不会覆盖原条目。
- **托盘 + 开机启动**：关窗最小化到托盘（可关），托盘 Quit 干净地杀 sing-box，登录时自启动。
- **无边框窗口**：Win/Linux 自绘窗口控制按钮；macOS 保留原生 traffic-lights。
- **国际化**：English + 简体中文（sidebar + 设置已覆盖，其它页面持续迁移）。

### Inkwing **不**做这些事

- **不做配置生成器。** 请准备一份完整的 sing-box JSON。Sub-Store 或者订阅商提供的链接都可以是上游来源。
- **不展示 fakeip 配置 UI。** sing-box 的 fakeip 模式（DNS 返回池里的虚拟 IP，路由根据虚拟 IP 决策）**支持**——只要你的源 config 里直接配好了 `dns.servers[].type: "fakeip"` + `dns.fakeip` 块，Inkwing 会原样加载并跑起来。但 Inkwing 的 DNS 编辑器**不会**展示 fakeip 特有字段。想用 fakeip，请在导入前手动写到源文件里。
- **不做格式转换。** Inkwing 只接受 sing-box 原生 JSON。Clash YAML / URI 列表请在上游处理好。

## 工作原理

Inkwing 围绕 **realip 模式** 设计（sing-box 默认的路由模型 —— 路由决策基于真实目的 IP，sing-box 正常解析 DNS）。所有路由相关的 UI、Mode 切换、本地覆盖层都按这个假设展开。

### 架构

- **前端**：React 19 + TypeScript + Tailwind v3 + shadcn/ui + Zustand + react-router-dom + dnd-kit + recharts。
- **桥接层**：Tauri 2 `invoke` 走命令式调用；`listen` 推送事件流（日志、连接、流量、override 变更）。
- **后端**：Rust + Tauri 2，模块在 `src-tauri/src/core/`：
  - `process` —— sing-box sidecar 生命周期（spawn / kill / 退出 watcher / Windows Job Object kill-on-job-close）
  - `clash_api` 与三泵（`log_pump` / `traffic_pump` / `conn_pump`）
  - `clash_inject` —— 运行时 overlay（clash_api 注入、mode、TUN、本地端口、cache_file 路径）
  - `overrides` —— GUI 编辑产生的本地覆盖层（[文档](docs/overrides.md)）
  - `rules` / `dns` / `library` / `config` / `subscriptions`
- **持久化**：tauri-plugin-store 存 library / settings / subscriptions；per-config + global JSON 文件存在 `<data_dir>/overrides/`；runtime config 每次启动重写；`cache.db` 重定向到 data_dir 防止 orphan 锁冲突。

### 一段话讲清 override 模型

源 config 文件对 Inkwing 编辑器是只读的。新增 route 规则会写到 `<data_dir>/overrides/<entry_id>.json`（per-config）或者 `overrides/global.json`。修改源规则会写一个 `modifications` 条目，key 是该原规则的 SHA-256 签名（可还原）。屏蔽源规则会把签名加到 `masked` 集合里。启动时 Inkwing 把 `source ⊕ overrides` 合并到 `<data_dir>/runtime/config.json`，sing-box 跑这个文件。你的源 `.json` 一字节都没动。详细 schema 见 [`docs/overrides.md`](docs/overrides.md)。

## 支持的配置导入方式

| 输入 | 操作路径 |
| --- | --- |
| 本地 sing-box JSON 文件 | **Add config → Add local file…** |
| 粘贴的 JSON 文本 | **Add config → Add from text…** |
| 订阅 URL（返回 sing-box JSON 配置） | **Add config → Add from subscription URL…** |

其它格式（Clash YAML、URI 列表、分享链接）请先在上游转好。Inkwing 会原样跑那份转好的 JSON。

## 快速开始

1. 从 [GitHub Releases](https://github.com/x01ych/Inkwing/releases) 下载安装包，或者从源码构建（见 [开发](#开发)）。
2. 打开 Inkwing → **Config** → **Add config** → 选择导入方式（本地文件 / 粘贴 / 订阅 URL）。
3. 点击对应 config 卡片设为 active。Inkwing 会自动用它启动 sing-box。
4. sidebar 的 **Mode** 段控件用来在 `rule / global / direct` 之间切换。**TUN** 开关用来运行时切换 TUN inbound（Linux 需要 `CAP_NET_ADMIN`；Windows 需要管理员；macOS 需要内置二进制签了网络扩展）。
5. 在 **Route** 自定义路由规则，在 **DNS** 自定义 DNS。所有改动都在 overrides 层，点 **Save & Restart** 后生效。

## 开发

前置：pnpm、Rust ≥ 1.77、Tauri 2 平台依赖（Linux 装 `webkit2gtk-4.1`，macOS 装 Xcode 命令行工具，Windows 装 MSVC 构建工具）。

```bash
# clone
git clone https://github.com/x01ych/Inkwing.git
cd Inkwing

# 装前端依赖
pnpm install

# 拉 sing-box sidecar 到 src-tauri/binaries/
node scripts/fetch-singbox.mjs

# 仅 Linux：把 CAP_NET_ADMIN 授给 sing-box，这样 TUN 不需要 root
sudo bash scripts/grant-tun-cap.sh

# 启动桌面 dev
pnpm tauri dev

# 前端检查
pnpm exec tsc -b
pnpm build

# 后端检查
cd src-tauri && cargo check && cargo test --lib

# 打 release 包
pnpm tauri build           # 当前平台
pnpm tauri build --debug   # 调试包，快
```

产物在 `src-tauri/target/release/bundle/`：

- macOS：`bundle/macos/Inkwing.app`（签名后会有 `.dmg`）
- Windows：`bundle/nsis/Inkwing-*.exe`
- Linux：`bundle/deb/inkwing_*.deb`、`bundle/appimage/inkwing_*.AppImage`

Docker 开发容器（Ubuntu 22.04 + Xvfb + noVNC）见 [`scripts/dev-container.sh`](scripts/dev-container.sh)。

## 本地数据目录

所有数据都在本机，不上传任何地方。

- **Windows**：`%APPDATA%\inkwing\Inkwing\data\`
- **Linux**：`~/.local/share/inkwing/`
- **macOS**：`~/Library/Application Support/dev.inkwing.Inkwing/`

目录结构：

```
<data_dir>/
├── configs/<id>.json     library 里管理的 config 文件（编辑器只读）
├── overrides/
│   ├── <id>.json         per-config 覆盖
│   └── global.json       全局追加覆盖
├── runtime/config.json   合并后的 runtime config（每次启动重写）
└── cache.db              sing-box 的 rule-set 缓存 + URL-test 历史
```

## 文档

- [English README](README.md)
- [Overrides 架构](docs/overrides.md) —— GUI 本地编辑如何叠加在源 config 上
- [UI 栈说明](docs/ui-stack.md) —— Tailwind / shadcn 开发约定
- [macOS 开发笔记](docs/MACOS-DEV.md)

## 项目结构

```
src/                       React + Tailwind + shadcn 前端
  pages/                   Dashboard / Config / Proxies / Route / DNS / Logs / Connections / Settings
  components/              ui（shadcn）+ Layout + 各功能子目录
  api/                     Tauri command 的薄封装（invoke / listen）
  store/                   Zustand store
src-tauri/
  src/commands/            按功能分组的 Tauri command
  src/core/                sing-box 内部
  binaries/                sing-box sidecar（gitignored — `scripts/fetch-singbox.mjs` 拉）
  icons/                   应用图标（全平台）
docs/
```

## 贡献

欢迎在 [GitHub repo](https://github.com/x01ych/Inkwing) 提 Issue / PR。改动请尽量保持与现有架构和「零失真 / 只通过 overrides 改」契约一致。

## 致谢

- [sing-box](https://github.com/SagerNet/sing-box) —— 内核
- [Tauri](https://tauri.app/) —— 桌面桥接
- [shadcn/ui](https://ui.shadcn.com/) —— UI 原子组件
- [Sub-Store](https://github.com/sub-store-org/Sub-Store) —— 推荐的上游配置生成器
- [sing-box-windows](https://github.com/xinggaoya/sing-box-windows) —— README 排版、功能矩阵约定，以及基于 Tauri 的 sing-box 客户端先行实现
- [clash-verge-rev](https://github.com/clash-verge-rev/clash-verge-rev) —— 桌面代理客户端的 UI / 交互参考

## License

MIT. 见 [LICENSE](LICENSE)。

---

<div align="center">
  <p>
    <strong>声明：</strong>Inkwing 仅供个人学习与合规使用，请遵守所在司法管辖区的法律法规。
  </p>
</div>

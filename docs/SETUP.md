# 开发环境前置（一次性）

Tauri 2 在 Linux 下编译/运行需要 `webkit2gtk-4.1`、`dbus-1`、`gtk-3` 等系统库。
**Ubuntu 20.04 (focal) 仓库不带 webkit2gtk-4.1**，所以本项目默认用 **Docker
开发容器**（jammy + 全套依赖 + Xvfb + noVNC），通过浏览器访问 GUI。

## 默认路径：方案 B — Docker dev 容器（已验证可用）

适用于：远程 SSH 开发机（无 DISPLAY）、Ubuntu 20.04、不想污染宿主 apt。

### 一次性

```bash
# 拉 sing-box + wintun 二进制（host 上跑，文件 bind-mount 给容器）
node scripts/fetch-singbox.mjs

# 构建并启动开发容器（首次约 5-10min：apt + rustup + Node）
bash scripts/dev-container.sh up
```

容器内启动了：
- **Xvfb** :1（虚拟 X server）
- **fluxbox**（窗口管理器）
- **x11vnc** :5900（VNC server，仅监听 127.0.0.1）
- **noVNC websockify** :6080（HTTP-to-VNC 桥，监听 0.0.0.0）

### 日常开发

```bash
# 进入容器
bash scripts/dev-container.sh shell

# 容器内执行：
pnpm install                                       # 仅首次
sudo bash scripts/grant-tun-cap.sh                 # 给 sing-box CAP_NET_ADMIN
pnpm tauri dev                                     # 启动 dev server + 编译 + 打开窗口
```

### 看 GUI

容器把 noVNC 暴露在宿主机 `127.0.0.1:6080`。本地浏览器：

- **本机即开发机**：`http://localhost:6080/vnc.html`
- **远程开发机（SSH）**：先在本地终端 `ssh -L 6080:localhost:6080 user@dev-host`
  ，再访问 `http://localhost:6080/vnc.html`

无密码，连进去就能看到 fluxbox 桌面 + Tauri 窗口。

### 常用命令

```bash
bash scripts/dev-container.sh shell          # 进容器
bash scripts/dev-container.sh logs           # 看容器日志
bash scripts/dev-container.sh cargo-check    # 仅类型检查 Rust
bash scripts/dev-container.sh tauri-dev      # 跑 pnpm tauri dev
bash scripts/dev-container.sh down           # 停容器（卷保留）
bash scripts/dev-container.sh reset          # 停 + 删卷（重新开始）
```

### TUN 在容器内

`docker-compose.dev.yml` 已配 `cap_add: NET_ADMIN` + `devices: /dev/net/tun`，
所以容器内 sing-box 能 setcap + 拉起 TUN 接口（与宿主的网络命名空间隔离，
不会影响主机路由）。

---

## 备选路径：宿主机直装（仅当宿主是 Ubuntu 22.04+）

```bash
sudo apt update
sudo apt install -y \
  build-essential pkg-config curl wget file \
  libssl-dev libdbus-1-dev libgtk-3-dev \
  libwebkit2gtk-4.1-dev libsoup-3.0-dev \
  libjavascriptcoregtk-4.1-dev librsvg2-dev libxdo-dev \
  libayatana-appindicator3-dev   # 托盘图标依赖

# Node 24 + pnpm 已通过 nvm + npm 装好（host）

node scripts/fetch-singbox.mjs
bash scripts/grant-tun-cap.sh
pnpm install
pnpm tauri dev
```

---

## 验证环境

容器或宿主机内：

```bash
pkg-config --modversion webkit2gtk-4.1   # 应 ≥ 2.36（容器：2.50.4 ✓）
pkg-config --modversion dbus-1            # 应 ≥ 1.6（容器：1.12.20 ✓）
node -v                                   # 容器：v24.x ✓
pnpm -v                                   # 容器：10.x ✓
rustc --version                           # 容器：1.88.0 ✓
ls -la /dev/net/tun                       # 容器：crw-rw-rw- ✓
```

每次重跑 `node scripts/fetch-singbox.mjs` 后都要重做 setcap
（`setcap` 在文件被覆盖时丢失）。

[English](README.md) · [Português](README.pt-BR.md) · [Español](README.es.md) · **简体中文**

# Rojo Manager

一个小巧的桌面应用，用一个窗口运行并管理多个本地 [Rojo](https://rojo.space) `serve`
进程——每个项目一个，带有各自的实时日志，并常驻系统托盘，所以关闭窗口不会中断你的 serve。

使用 **Tauri 2 + React + TypeScript** 构建。暖色调的 Claude 风格界面，支持浅色/深色/跟随
系统三种主题。

## 功能

- 添加 / 编辑 / 删除项目（名称、目录、Rojo 项目文件、端口、可选参数）。以 JSON 保存在系统的
  应用数据目录（Windows 上为 `%APPDATA%/com.rojomanager.app/projects.json`）。
- 每个项目独立启动/停止——多个 `rojo serve` 实例并行运行。
- 每个项目的输出都会被捕获并流式送入日志控制台，不会卡住界面。
- 拒绝启动已在运行的项目，或端口已被另一个 serve 占用的项目。
- 关闭窗口会最小化到**系统托盘**（serve 继续运行）。托盘菜单：显示 / 隐藏 / 停止所有 serve /
  退出。退出会干净地结束所有子进程。

`rojo` 必须已安装并位于 `PATH` 中（例如通过
[Rokit](https://github.com/rojo-rbx/rokit) 安装）。

## 下载

到[最新 release](https://github.com/ocauapaz/rojo-manager/releases/latest) 下载对应系统的
安装包：

| 系统 | 文件 |
| --- | --- |
| Windows | `.msi` 或 `.exe`（NSIS） |
| macOS | `.dmg` |
| Linux | `.AppImage` 或 `.deb` |

这些构建产物未经签名，首次启动时 Windows SmartScreen 和 macOS Gatekeeper 会发出警告——
选择"更多信息 → 仍要运行"，或在 macOS 上右键 → 打开。如果不想信任二进制文件，可以按下面
的步骤自行编译。

## 从源码编译

需要 [Node 20+](https://nodejs.org) 和 [Rust 工具链](https://rustup.rs)，以及对应系统的
[Tauri 系统依赖](https://tauri.app/start/prerequisites/)。

```bash
git clone https://github.com/ocauapaz/rojo-manager.git
cd rojo-manager
npm install
npm run tauri build
```

会在 `src-tauri/target/release/` 生成无控制台窗口的可执行文件和安装包（安装包位于
`src-tauri/target/release/bundle/`）。安装它（或直接运行 `.exe`），然后像普通应用一样打开。

## 开发

```bash
npm install
npm run tauri dev      # 带热重载的开发窗口
```

## 目录结构

| 路径 | 用途 |
| --- | --- |
| `src-tauri/src/lib.rs` | 进程管理、持久化、托盘、生命周期 |
| `src/App.tsx` | 主面板：侧边列表 + 项目详情 + 日志控制台 |
| `src/components/` | `ProjectForm`、`LogPanel`、`ThemeToggle` |
| `src/api.ts` / `src/types.ts` | Tauri 命令封装 + 共享类型 |
| `src/styles.css` | 主题（CSS 变量；`:root[data-theme="dark"]`） |

## 发布

推送一个 `v*` 标签，GitHub Actions 会构建 Windows、macOS 和 Linux 的安装包并生成
**草稿** release；再手动编辑并发布。

```bash
npm version 0.1.1 --no-git-tag-version   # 更新 package.json 中的版本号
# 同步修改 src-tauri/tauri.conf.json 中的 `version`
git commit -am "chore: v0.1.1" && git tag v0.1.1 && git push --follow-tags
```

## 许可证

MIT —— 见 [LICENSE](LICENSE)。

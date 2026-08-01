# Rojo Manager

A small desktop app to run and supervise multiple local [Rojo](https://rojo.space) `serve`
processes — one per project — from a single window, with live per-project logs and a
system-tray presence so closing the window never stops your serves.

Built with **Tauri 2 + React + TypeScript**. Warm "Claude-like" UI with Light/Dark/System themes.

## What it does

- Add / edit / remove projects (name, folder, Rojo project file, port, optional args). Stored
  as JSON in the OS app-data dir (`%APPDATA%/com.rojomanager.app/projects.json` on Windows).
- Start/stop each project independently — multiple `rojo serve` instances run in parallel.
- Per-project output is captured and streamed to a log console without freezing the UI.
- Refuses to start a project that's already running, or one whose port is held by another serve.
- Closing the window hides to the **system tray** (serves keep running). Tray menu: Show / Hide /
  Stop All Serves / Quit. Quit stops every child process cleanly.

`rojo` must be installed and on your `PATH` (e.g. via [Rokit](https://github.com/rojo-rbx/rokit)).

## Download

Grab the installer for your OS from the
[latest release](https://github.com/ocauapaz/rojo-manager/releases/latest):

| OS | File |
| --- | --- |
| Windows | `.msi` or `.exe` (NSIS) |
| macOS | `.dmg` |
| Linux | `.AppImage` or `.deb` |

The builds are unsigned, so Windows SmartScreen and macOS Gatekeeper will warn on first
launch — "More info → Run anyway", or right-click → Open on macOS. Build from source
(below) if you'd rather not trust a binary.

## Build from source

Needs [Node 20+](https://nodejs.org) and the
[Rust toolchain](https://rustup.rs), plus the
[Tauri system dependencies](https://tauri.app/start/prerequisites/) for your OS.

```bash
git clone https://github.com/ocauapaz/rojo-manager.git
cd rojo-manager
npm install
npm run tauri build
```

Outputs a console-free executable and installer under
`src-tauri/target/release/` (the bundle/installer is in `src-tauri/target/release/bundle/`).
Install it (or run the `.exe` directly) and launch it like any other app.

## Develop

```bash
npm install
npm run tauri dev      # hot-reloading dev window
```

## Layout

| Path | Purpose |
| --- | --- |
| `src-tauri/src/lib.rs` | Process management, persistence, tray, lifecycle |
| `src/App.tsx` | Dashboard: sidebar list + project detail + log console |
| `src/components/` | `ProjectForm`, `LogPanel`, `ThemeToggle` |
| `src/api.ts` / `src/types.ts` | Tauri command wrappers + shared types |
| `src/styles.css` | Theme (CSS variables; `:root[data-theme="dark"]`) |

## Releases

Push a `v*` tag and GitHub Actions builds Windows, macOS and Linux bundles into a **draft**
release; edit and publish it by hand.

```bash
npm version 0.1.1 --no-git-tag-version   # bump package.json
# bump `version` in src-tauri/tauri.conf.json to match
git commit -am "chore: v0.1.1" && git tag v0.1.1 && git push --follow-tags
```

## License

MIT — see [LICENSE](LICENSE).

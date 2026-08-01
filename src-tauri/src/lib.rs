// Rojo Manager — backend.
// Manages independent `rojo serve` child processes per project, captures their
// output, persists project definitions, and lives in the system tray.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WindowEvent,
};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

const LOG_CAP: usize = 500; // ponytail: per-project ring buffer; bump if devs want longer history
const DEFAULT_PORT: u16 = 34872; // matches the form default; first port handed out by auto-scan
const MAX_SCAN_DIRS: usize = 20_000; // ponytail: cap walk so a huge/looping tree can't hang the scan
const MAX_RECONNECTS: u32 = 5; // consecutive auto-restarts before giving up (avoids crash-looping a broken config)
const RECONNECT_STABLE_MS: i64 = 20_000; // served this long before dropping => treat the next drop as fresh, reset the counter

// Directories the scan never descends into — vendored deps, build output, VCS, caches.
// Compared lower-cased. ponytail: add more here if a noisy folder shows up in results.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    "caches",
    ".cache",
    "logs",
    "packages",
    "devpackages",
    "serverpackages",
];

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Project {
    id: String,
    name: String,
    folder: String,
    project_file: String,
    port: u16,
    #[serde(default)]
    args: Vec<String>,
}

/// A Rojo project found on disk by the auto-scan, ready to be added with one click.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DiscoveredProject {
    name: String,
    folder: String,
    project_file: String,
    port: u16,
    reason: String, // which file matched, shown in the UI
}

#[derive(Serialize, Clone)]
struct LogLine {
    ts: i64,
    stream: String, // "stdout" | "stderr" | "system"
    line: String,
}

#[derive(Serialize, Clone)]
struct StatusPayload {
    id: String,
    status: String, // "running" | "stopped" | "error"
    code: Option<i32>,
    message: Option<String>,
}

struct RunningProc {
    child: CommandChild,
    port: u16,
}

#[derive(Default)]
struct AppState {
    procs: Mutex<HashMap<String, RunningProc>>,
    logs: Mutex<HashMap<String, VecDeque<LogLine>>>,
    stopping: Mutex<HashSet<String>>, // ids the user asked to stop, so exit != crash
    wanted: Mutex<HashSet<String>>,   // ids that should stay served; drop => auto-reconnect. cleared on user stop.
}

/// Backoff before an auto-reconnect: 2s, 4s, … capped at 10s.
fn reconnect_delay_ms(attempt: u32) -> u64 {
    (attempt as u64 * 2000).min(10_000)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---- persistence -----------------------------------------------------------

fn projects_file(app: &AppHandle) -> PathBuf {
    let dir = app.path().app_config_dir().expect("no app config dir");
    std::fs::create_dir_all(&dir).ok();
    dir.join("projects.json")
}

fn read_projects(app: &AppHandle) -> Vec<Project> {
    std::fs::read_to_string(projects_file(app))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_projects(app: &AppHandle, list: &[Project]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    std::fs::write(projects_file(app), json).map_err(|e| e.to_string())
}

// ---- auto-scan -------------------------------------------------------------

/// Canonical path of `p`, falling back to the path as-given if it can't be resolved.
fn canonical_or(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Resolved on-disk path of a project's `.project.json`, used to compare against
/// what's already saved (handles relative vs absolute project files uniformly).
fn resolve_project_path(folder: &str, project_file: &str) -> PathBuf {
    canonical_or(&PathBuf::from(folder).join(project_file))
}

/// Pull the `"name"` field out of a Rojo project file; fall back to None on any error.
fn project_name(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("name")?.as_str().map(str::to_string)
}

/// Next port not already taken, starting at `DEFAULT_PORT`. Reserves the chosen
/// port in `used` so repeated calls hand out distinct ports.
fn next_free_port(used: &mut HashSet<u16>) -> u16 {
    let mut port = DEFAULT_PORT;
    while port < u16::MAX && used.contains(&port) {
        port += 1; // ponytail: linear probe is plenty for a handful of projects
    }
    used.insert(port);
    port
}

/// Recursively scan `root` for Rojo project files and return add-ready snippets.
/// Skips noisy directories, dedupes against saved projects, and assigns free ports.
/// Inaccessible folders are skipped rather than failing the whole scan.
#[tauri::command]
fn scan_projects(app: AppHandle, root: String) -> Result<Vec<DiscoveredProject>, String> {
    let root_path = PathBuf::from(&root);
    if !root_path.is_dir() {
        return Err(format!("Not a folder: {root}"));
    }
    Ok(discover(&root_path, &read_projects(&app)))
}

/// Pure core of the scan, split out so it's testable without an `AppHandle`.
fn discover(root_path: &Path, existing: &[Project]) -> Vec<DiscoveredProject> {
    let mut used_ports: HashSet<u16> = existing.iter().map(|p| p.port).collect();
    let existing_paths: HashSet<PathBuf> = existing
        .iter()
        .map(|p| resolve_project_path(&p.folder, &p.project_file))
        .collect();

    let mut found: Vec<DiscoveredProject> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut stack = vec![root_path.to_path_buf()];
    let mut visited = 0usize;

    while let Some(dir) = stack.pop() {
        visited += 1;
        if visited > MAX_SCAN_DIRS {
            break;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue, // unreadable folder (permissions, gone) — skip, don't crash
        };

        // Collect this directory's project files and child dirs separately so we can
        // treat a directory that *is* a project root as a leaf (don't descend into it).
        let mut project_files: Vec<String> = Vec::new();
        let mut subdirs: Vec<PathBuf> = Vec::new();
        for entry in entries.flatten() {
            let file_type = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if file_type.is_dir() {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if !SKIP_DIRS.contains(&name.as_str()) {
                    subdirs.push(entry.path());
                }
            } else if file_type.is_file() {
                let file_name = entry.file_name().to_string_lossy().to_string();
                if file_name.ends_with(".project.json") {
                    project_files.push(file_name);
                }
            }
        }

        if project_files.is_empty() {
            // Not a project root — keep looking deeper.
            stack.extend(subdirs);
            continue;
        }

        // This directory is a Rojo project root. Record its config(s) and stop here so
        // vendored deps/forks with their own project files aren't reported as nested projects.
        project_files.sort();
        for file_name in project_files {
            let path = dir.join(&file_name);
            let resolved = canonical_or(&path);
            if existing_paths.contains(&resolved) || !seen.insert(resolved) {
                continue; // already saved, or already found in this scan
            }
            let name = project_name(&path).unwrap_or_else(|| {
                dir.file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Rojo project".into())
            });
            found.push(DiscoveredProject {
                port: next_free_port(&mut used_ports),
                reason: format!("Found {file_name}"),
                name,
                folder: dir.to_string_lossy().to_string(),
                project_file: file_name,
            });
        }
    }

    // default.project.json first, then alphabetical by name — predictable review order.
    found.sort_by(|a, b| {
        let a_default = a.project_file == "default.project.json";
        let b_default = b.project_file == "default.project.json";
        b_default
            .cmp(&a_default)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proj(folder: &str, file: &str, port: u16) -> Project {
        Project {
            id: "x".into(),
            name: "x".into(),
            folder: folder.into(),
            project_file: file.into(),
            port,
            args: vec![],
        }
    }

    #[test]
    fn discover_stops_at_project_roots_dedupes_and_assigns_ports() {
        // root/ has no project file, so the scan descends into its children.
        //   GameA/default.project.json        -> a project root
        //   GameA/vendor/default.project.json -> vendored dep, must NOT be reported
        //   GameB/default.project.json        -> a project root (already saved -> skipped)
        let dir = std::env::temp_dir().join(format!("rojo_scan_test_{}", now_ms()));
        let game_a = dir.join("GameA");
        let vendor = game_a.join("vendor");
        let game_b = dir.join("GameB");
        std::fs::create_dir_all(&vendor).unwrap();
        std::fs::create_dir_all(&game_b).unwrap();
        std::fs::write(game_a.join("default.project.json"), r#"{"name":"GameA"}"#).unwrap();
        std::fs::write(vendor.join("default.project.json"), r#"{"name":"Vendor"}"#).unwrap();
        std::fs::write(game_b.join("default.project.json"), r#"{"name":"GameB"}"#).unwrap();

        // GameB is already saved on the default port.
        let existing = vec![proj(
            game_b.to_str().unwrap(),
            "default.project.json",
            DEFAULT_PORT,
        )];
        let found = discover(&dir, &existing);

        // Only GameA: the nested vendor project is skipped (parent is a project root),
        // and GameB is skipped (already saved).
        let names: Vec<_> = found.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(found.len(), 1, "got: {names:?}");
        assert_eq!(found[0].name, "GameA");
        // DEFAULT_PORT is taken by the saved GameB, so the next free port is handed out.
        assert_eq!(found[0].port, DEFAULT_PORT + 1);

        std::fs::remove_dir_all(&dir).ok();
    }
}

// ---- logging ---------------------------------------------------------------

fn push_log(app: &AppHandle, id: &str, stream: &str, line: String) {
    let entry = LogLine {
        ts: now_ms(),
        stream: stream.to_string(),
        line,
    };
    let state = app.state::<AppState>();
    {
        let mut logs = state.logs.lock().unwrap();
        let buf = logs.entry(id.to_string()).or_default();
        buf.push_back(entry.clone());
        while buf.len() > LOG_CAP {
            buf.pop_front();
        }
    }
    let _ = app.emit("log-line", (id, entry));
}

fn emit_status(app: &AppHandle, payload: StatusPayload) {
    let _ = app.emit("status-changed", payload);
}

// ---- process control -------------------------------------------------------

/// Kill the live child for `id` (if any) and mark the stop as intentional.
/// Also clears the "wanted" flag so any pending auto-reconnect is cancelled.
fn stop_running(app: &AppHandle, state: &AppState, id: &str) {
    state.wanted.lock().unwrap().remove(id);
    let proc = state.procs.lock().unwrap().remove(id);
    if let Some(proc) = proc {
        state.stopping.lock().unwrap().insert(id.to_string());
        let _ = proc.child.kill();
        push_log(app, id, "system", "stop requested".into());
    } else {
        // Nothing live — likely mid-reconnect; settle the UI to stopped.
        emit_status(
            app,
            StatusPayload {
                id: id.to_string(),
                status: "stopped".into(),
                code: None,
                message: None,
            },
        );
    }
}

fn stop_all_internal(app: &AppHandle, state: &AppState) {
    let ids: Vec<String> = state.procs.lock().unwrap().keys().cloned().collect();
    for id in ids {
        stop_running(app, state, &id);
    }
}

// ---- commands --------------------------------------------------------------

#[tauri::command]
fn list_projects(app: AppHandle) -> Vec<Project> {
    read_projects(&app)
}

#[tauri::command]
fn save_project(app: AppHandle, project: Project) -> Result<Vec<Project>, String> {
    let mut list = read_projects(&app);
    match list.iter_mut().find(|p| p.id == project.id) {
        Some(existing) => *existing = project,
        None => list.push(project),
    }
    write_projects(&app, &list)?;
    Ok(list)
}

#[tauri::command]
fn delete_project(app: AppHandle, state: State<AppState>, id: String) -> Result<Vec<Project>, String> {
    stop_running(&app, &state, &id);
    let mut list = read_projects(&app);
    list.retain(|p| p.id != id);
    write_projects(&app, &list)?;
    state.logs.lock().unwrap().remove(&id);
    Ok(list)
}

#[tauri::command]
fn get_logs(state: State<AppState>, id: String) -> Vec<LogLine> {
    state
        .logs
        .lock()
        .unwrap()
        .get(&id)
        .map(|b| b.iter().cloned().collect())
        .unwrap_or_default()
}

/// Ids of every project we currently have a live child for.
#[tauri::command]
fn get_running(state: State<AppState>) -> Vec<String> {
    state.procs.lock().unwrap().keys().cloned().collect()
}

#[tauri::command]
fn start_project(app: AppHandle, state: State<AppState>, project: Project) -> Result<(), String> {
    {
        let procs = state.procs.lock().unwrap();
        if procs.contains_key(&project.id) {
            return Err(format!("{} is already running.", project.name));
        }
        if let Some(p) = procs.values().find(|p| p.port == project.port) {
            return Err(format!(
                "Port {} is already in use by another running serve.",
                p.port
            ));
        }
    }

    state
        .logs
        .lock()
        .unwrap()
        .entry(project.id.clone())
        .or_default()
        .clear();

    state.wanted.lock().unwrap().insert(project.id.clone());
    spawn_serve(&app, &project, 0)
}

/// Launch `rojo serve` for a project and wire up its output/lifecycle.
/// `attempt` is 0 for a user start; the auto-reconnect path calls back in with
/// an incrementing count so it can back off and eventually give up.
fn spawn_serve(app: &AppHandle, project: &Project, attempt: u32) -> Result<(), String> {
    let state = app.state::<AppState>();
    // Guard against a double-spawn (a manual start racing a pending reconnect).
    if state.procs.lock().unwrap().contains_key(&project.id) {
        return Ok(());
    }

    push_log(
        app,
        &project.id,
        "system",
        format!("rojo serve {} --port {}", project.project_file, project.port),
    );

    let mut args = vec![
        "serve".to_string(),
        project.project_file.clone(),
        "--port".to_string(),
        project.port.to_string(),
    ];
    args.extend(project.args.clone());

    let cmd = app
        .shell()
        .command("rojo")
        .args(args)
        .current_dir(PathBuf::from(&project.folder));

    let (mut rx, child) = cmd.spawn().map_err(|e| {
        let msg = format!("Failed to launch rojo: {e}. Is rojo installed and on PATH?");
        state.wanted.lock().unwrap().remove(&project.id);
        push_log(app, &project.id, "system", msg.clone());
        emit_status(
            app,
            StatusPayload {
                id: project.id.clone(),
                status: "error".into(),
                code: None,
                message: Some(msg.clone()),
            },
        );
        msg
    })?;

    state.procs.lock().unwrap().insert(
        project.id.clone(),
        RunningProc {
            child,
            port: project.port,
        },
    );
    emit_status(
        app,
        StatusPayload {
            id: project.id.clone(),
            status: "running".into(),
            code: None,
            message: None,
        },
    );

    let app2 = app.clone();
    let project2 = project.clone();
    let id = project.id.clone();
    let started_at = now_ms();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(bytes) => push_log(
                    &app2,
                    &id,
                    "stdout",
                    String::from_utf8_lossy(&bytes).trim_end().to_string(),
                ),
                CommandEvent::Stderr(bytes) => push_log(
                    &app2,
                    &id,
                    "stderr",
                    String::from_utf8_lossy(&bytes).trim_end().to_string(),
                ),
                CommandEvent::Error(err) => push_log(&app2, &id, "system", err),
                CommandEvent::Terminated(payload) => {
                    let st = app2.state::<AppState>();
                    st.procs.lock().unwrap().remove(&id);
                    let intentional = st.stopping.lock().unwrap().remove(&id);
                    let code = payload.code;
                    push_log(&app2, &id, "system", format!("process terminated (code {:?})", code));

                    // Clean exit or a stop the user asked for — settle and stop here.
                    if intentional || code == Some(0) {
                        st.wanted.lock().unwrap().remove(&id);
                        emit_status(
                            &app2,
                            StatusPayload {
                                id: id.clone(),
                                status: "stopped".into(),
                                code,
                                message: None,
                            },
                        );
                        break;
                    }

                    // Unexpected drop. Reset the attempt counter if it had been
                    // serving a while, so an occasional hiccup never exhausts retries.
                    let ran = now_ms() - started_at;
                    let next = if ran >= RECONNECT_STABLE_MS { 1 } else { attempt + 1 };
                    let still_wanted = st.wanted.lock().unwrap().contains(&id);

                    if still_wanted && next <= MAX_RECONNECTS {
                        let delay = reconnect_delay_ms(next);
                        push_log(
                            &app2,
                            &id,
                            "system",
                            format!(
                                "serve dropped (code {:?}) — reconnecting (attempt {}/{}) in {}s",
                                code,
                                next,
                                MAX_RECONNECTS,
                                delay / 1000
                            ),
                        );
                        // Status stays "running" through the brief backoff so a
                        // self-healing drop is transparent; the log line records it.
                        let app3 = app2.clone();
                        let project3 = project2.clone();
                        let id3 = id.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_millis(delay));
                            let st = app3.state::<AppState>();
                            // User may have hit Stop during the wait.
                            if !st.wanted.lock().unwrap().contains(&id3) {
                                return;
                            }
                            let _ = spawn_serve(&app3, &project3, next);
                        });
                    } else {
                        st.wanted.lock().unwrap().remove(&id);
                        let msg = if next > MAX_RECONNECTS {
                            format!(
                                "rojo keeps exiting (code {:?}); gave up after {} reconnect attempts",
                                code, MAX_RECONNECTS
                            )
                        } else {
                            format!("rojo exited with code {:?}", code)
                        };
                        emit_status(
                            &app2,
                            StatusPayload {
                                id: id.clone(),
                                status: "error".into(),
                                code,
                                message: Some(msg),
                            },
                        );
                    }
                    break;
                }
                _ => {}
            }
        }
    });

    Ok(())
}

#[tauri::command]
fn stop_project(app: AppHandle, state: State<AppState>, id: String) {
    stop_running(&app, &state, &id);
}

#[tauri::command]
fn stop_all(app: AppHandle, state: State<AppState>) {
    stop_all_internal(&app, &state);
}

// ---- tray + lifecycle ------------------------------------------------------

fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.center();
        let _ = w.set_focus();
    }
}

fn kill_all_children(app: &AppHandle) {
    let state = app.state::<AppState>();
    let ids: Vec<String> = state.procs.lock().unwrap().keys().cloned().collect();
    for id in ids {
        if let Some(proc) = state.procs.lock().unwrap().remove(&id) {
            let _ = proc.child.kill();
        }
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Rojo Manager", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "Hide Window", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop_all", "Stop All Serves", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &hide, &stop, &quit])?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("Rojo Manager")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_window(app),
            "hide" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            "stop_all" => {
                let state = app.state::<AppState>();
                stop_all_internal(app, &state);
            }
            "quit" => {
                kill_all_children(app);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick { .. } = event {
                show_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be the first plugin: a second launch hands its args to the running
        // instance and exits, so we keep exactly one tray process. Just resurface.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_window(app);
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            build_tray(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps the app alive in the tray; serves keep running.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_projects,
            scan_projects,
            save_project,
            delete_project,
            get_logs,
            get_running,
            start_project,
            stop_project,
            stop_all,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                kill_all_children(app);
            }
        });
}

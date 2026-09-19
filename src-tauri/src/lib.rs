// Rojo Manager — backend.
// Manages independent `rojo serve` child processes per project, captures their
// output, persists project definitions, and lives in the system tray.

mod core;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Mutex;

use core::{discover, now_ms, DiscoveredProject, LogLine, Project, LOG_CAP};
use serde::Serialize;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WindowEvent,
};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

const MAX_RECONNECTS: u32 = 5; // consecutive auto-restarts before giving up (avoids crash-looping a broken config)
const RECONNECT_STABLE_MS: i64 = 20_000; // served this long before dropping => treat the next drop as fresh, reset the counter

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

// ---- persistence -----------------------------------------------------------

fn projects_file(app: &AppHandle) -> PathBuf {
    let dir = app.path().app_config_dir().expect("no app config dir");
    std::fs::create_dir_all(&dir).ok();
    dir.join("projects.json")
}

fn read_projects(app: &AppHandle) -> Vec<Project> {
    core::read_projects_from(&projects_file(app))
}

fn write_projects(app: &AppHandle, list: &[Project]) -> Result<(), String> {
    core::write_projects_to(&projects_file(app), list)
}

// ---- auto-scan -------------------------------------------------------------

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

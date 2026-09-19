// Rojo Manager CLI (`rojom`) — a terminal dashboard for the same projects the
// desktop app manages. Reads/writes the same `projects.json`, spawns one
// `rojo serve` child per project, streams their output into per-project logs.

#[path = "../../src-tauri/src/core.rs"]
mod core;
mod ui;

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use core::{now_ms, DiscoveredProject, LogLine, Project, APP_IDENTIFIER, LOG_CAP};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

const TICK: Duration = Duration::from_millis(100); // UI redraw / channel drain cadence
const EXIT_POLL: Duration = Duration::from_millis(150); // how often a watcher thread checks its child
const TOAST_TTL: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Running,
    Stopped,
    Error(String),
}

pub enum Mode {
    Normal,
    ScanInput {
        input: String,
    },
    ScanPick {
        items: Vec<DiscoveredProject>,
        checked: Vec<bool>,
        cursor: usize,
    },
    ConfirmDelete,
    ConfirmQuit,
}

enum Msg {
    Log {
        id: String,
        stream: &'static str,
        line: String,
    },
    Exited {
        id: String,
        code: Option<i32>,
    },
}

struct Proc {
    child: Arc<Mutex<Child>>,
    port: u16,
}

pub struct Toast {
    pub text: String,
    pub is_error: bool,
    at: Instant,
}

pub struct App {
    pub projects: Vec<Project>,
    pub projects_path: PathBuf,
    pub selected: usize,
    pub status: HashMap<String, Status>,
    pub logs: HashMap<String, VecDeque<LogLine>>,
    pub mode: Mode,
    pub log_scroll: usize, // lines scrolled up from the bottom; 0 = follow tail
    pub toast: Option<Toast>,
    procs: HashMap<String, Proc>,
    stopping: HashSet<String>, // ids the user asked to stop, so exit != crash
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    quit: bool,
}

fn projects_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_IDENTIFIER)
        .join("projects.json")
}

/// Same shape as the desktop fallback id (`p_<ms>_<random>`); stdlib-only randomness.
fn new_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let random = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    format!("p_{}_{:x}", now_ms(), random)
}

/// Forward every line a child writes on `reader` into the app channel.
fn pump(reader: impl Read + Send + 'static, stream: &'static str, id: String, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        for chunk in BufReader::new(reader).split(b'\n').map_while(Result::ok) {
            let line = String::from_utf8_lossy(&chunk).trim_end().to_string();
            if tx
                .send(Msg::Log {
                    id: id.clone(),
                    stream,
                    line,
                })
                .is_err()
            {
                break;
            }
        }
    });
}

impl App {
    pub fn new() -> Self {
        let projects_path = projects_path();
        let projects = core::read_projects_from(&projects_path);
        let (tx, rx) = mpsc::channel();
        Self {
            projects,
            projects_path,
            selected: 0,
            status: HashMap::new(),
            logs: HashMap::new(),
            mode: Mode::Normal,
            log_scroll: 0,
            toast: None,
            procs: HashMap::new(),
            stopping: HashSet::new(),
            tx,
            rx,
            quit: false,
        }
    }

    pub fn selected_project(&self) -> Option<&Project> {
        self.projects.get(self.selected)
    }

    pub fn status_of(&self, id: &str) -> Status {
        self.status.get(id).cloned().unwrap_or(Status::Stopped)
    }

    pub fn running_count(&self) -> usize {
        self.procs.len()
    }

    fn toast(&mut self, text: impl Into<String>, is_error: bool) {
        self.toast = Some(Toast {
            text: text.into(),
            is_error,
            at: Instant::now(),
        });
    }

    fn push_log(&mut self, id: &str, stream: &str, line: String) {
        let buf = self.logs.entry(id.to_string()).or_default();
        buf.push_back(LogLine {
            ts: now_ms(),
            stream: stream.to_string(),
            line,
        });
        while buf.len() > LOG_CAP {
            buf.pop_front();
        }
    }

    fn save(&mut self) {
        if let Err(e) = core::write_projects_to(&self.projects_path, &self.projects) {
            self.toast(format!("Could not save projects: {e}"), true);
        }
    }

    // ---- process control ---------------------------------------------------

    fn start(&mut self, idx: usize) {
        let Some(project) = self.projects.get(idx).cloned() else {
            return;
        };
        if self.procs.contains_key(&project.id) {
            self.toast(format!("{} is already running.", project.name), true);
            return;
        }
        if let Some(other) = self.procs.values().find(|p| p.port == project.port) {
            self.toast(
                format!(
                    "Port {} is already in use by another running serve.",
                    other.port
                ),
                true,
            );
            return;
        }
        self.logs.entry(project.id.clone()).or_default().clear();
        self.log_scroll = 0;
        if let Err(msg) = self.spawn_serve(&project) {
            self.push_log(&project.id, "system", msg.clone());
            self.status
                .insert(project.id.clone(), Status::Error(msg.clone()));
            self.toast(msg, true);
        }
    }

    fn spawn_serve(&mut self, project: &Project) -> Result<(), String> {
        if !Path::new(&project.folder).is_dir() {
            return Err(format!("Folder not found: {}", project.folder));
        }
        self.push_log(
            &project.id,
            "system",
            format!(
                "rojo serve {} --port {}",
                project.project_file, project.port
            ),
        );
        let mut child = Command::new("rojo")
            .arg("serve")
            .arg(&project.project_file)
            .arg("--port")
            .arg(project.port.to_string())
            .args(&project.args)
            .current_dir(&project.folder)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to launch rojo: {e}. Is rojo installed and on PATH?"))?;

        if let Some(out) = child.stdout.take() {
            pump(out, "stdout", project.id.clone(), self.tx.clone());
        }
        if let Some(err) = child.stderr.take() {
            pump(err, "stderr", project.id.clone(), self.tx.clone());
        }

        let child = Arc::new(Mutex::new(child));
        let watched = Arc::clone(&child);
        let id = project.id.clone();
        let tx = self.tx.clone();
        // Poll instead of blocking on wait() so the main thread can still lock the
        // child to kill it. ponytail: 150ms poll is invisible for a dev server.
        std::thread::spawn(move || loop {
            let exited = watched.lock().unwrap().try_wait().ok().flatten();
            if let Some(status) = exited {
                let _ = tx.send(Msg::Exited {
                    id,
                    code: status.code(),
                });
                break;
            }
            std::thread::sleep(EXIT_POLL);
        });

        self.procs.insert(
            project.id.clone(),
            Proc {
                child,
                port: project.port,
            },
        );
        self.status.insert(project.id.clone(), Status::Running);
        Ok(())
    }

    fn stop(&mut self, id: &str) {
        let Some(proc) = self.procs.get(id) else {
            return;
        };
        self.stopping.insert(id.to_string());
        let _ = proc.child.lock().unwrap().kill();
        self.push_log(id, "system", "stop requested".into());
    }

    fn stop_all(&mut self) {
        for id in self.procs.keys().cloned().collect::<Vec<_>>() {
            self.stop(&id);
        }
    }

    fn start_all(&mut self) {
        for idx in 0..self.projects.len() {
            if !self.procs.contains_key(&self.projects[idx].id) {
                self.start(idx);
            }
        }
    }

    /// Kill every live child and reap it. Returns how many were running.
    pub fn kill_all(&mut self) -> usize {
        let procs: Vec<Proc> = self.procs.drain().map(|(_, p)| p).collect();
        for proc in &procs {
            let mut child = proc.child.lock().unwrap();
            let _ = child.kill();
            let _ = child.wait();
        }
        procs.len()
    }

    fn handle_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Log { id, stream, line } => self.push_log(&id, stream, line),
            Msg::Exited { id, code } => {
                self.procs.remove(&id);
                let intentional = self.stopping.remove(&id);
                self.push_log(&id, "system", format!("process terminated (code {code:?})"));
                // ponytail: no auto-reconnect here (the desktop app has one); add if serves drop in practice
                if intentional || code == Some(0) {
                    self.status.insert(id, Status::Stopped);
                } else {
                    let code = code.map_or("?".to_string(), |c| c.to_string());
                    let msg = format!("rojo exited with code {code}");
                    self.status.insert(id.clone(), Status::Error(msg.clone()));
                    let name = self.name_of(&id);
                    self.toast(format!("{name}: {msg}"), true);
                }
            }
        }
    }

    fn name_of(&self, id: &str) -> String {
        self.projects
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    // ---- project list ------------------------------------------------------

    fn select(&mut self, delta: isize) {
        if self.projects.is_empty() {
            return;
        }
        let last = self.projects.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, last) as usize;
        self.log_scroll = 0;
    }

    fn reload(&mut self) {
        self.projects = core::read_projects_from(&self.projects_path);
        self.selected = self.selected.min(self.projects.len().saturating_sub(1));
        self.toast(
            format!("Reloaded {} project(s) from disk", self.projects.len()),
            false,
        );
    }

    fn delete_selected(&mut self) {
        let Some(project) = self.selected_project().cloned() else {
            return;
        };
        self.stop(&project.id);
        self.projects.retain(|p| p.id != project.id);
        self.logs.remove(&project.id);
        self.status.remove(&project.id);
        self.selected = self.selected.min(self.projects.len().saturating_sub(1));
        self.save();
        self.toast(format!("Deleted {}", project.name), false);
    }

    fn run_scan(&mut self, input: &str) {
        let root = PathBuf::from(input.trim());
        if !root.is_dir() {
            self.toast(format!("Not a folder: {}", root.display()), true);
            return;
        }
        let items = core::discover(&root, &self.projects);
        if items.is_empty() {
            self.toast(
                format!("No new Rojo projects under {}", root.display()),
                false,
            );
            self.mode = Mode::Normal;
            return;
        }
        self.mode = Mode::ScanPick {
            checked: vec![true; items.len()],
            items,
            cursor: 0,
        };
    }

    fn add_discovered(&mut self, items: Vec<DiscoveredProject>, checked: Vec<bool>) {
        let first_new = self.projects.len();
        let picked = items.into_iter().zip(checked).filter(|(_, on)| *on);
        let mut added = 0;
        for (d, _) in picked {
            self.projects.push(Project {
                id: new_id(),
                name: d.name,
                folder: d.folder,
                project_file: d.project_file,
                port: d.port,
                args: vec![],
            });
            added += 1;
        }
        if added > 0 {
            self.selected = first_new;
            self.save();
        }
        self.toast(format!("Added {added} project(s)"), false);
        self.mode = Mode::Normal;
    }

    // ---- input -------------------------------------------------------------

    fn handle_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return;
        }
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => self.handle_normal_key(key),
            Mode::ScanInput { input } => self.handle_scan_input_key(key, input),
            Mode::ScanPick {
                items,
                checked,
                cursor,
            } => self.handle_scan_pick_key(key, items, checked, cursor),
            Mode::ConfirmDelete => {
                if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                    self.delete_selected();
                }
            }
            Mode::ConfirmQuit => {
                if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                    self.quit = true;
                }
            }
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') if self.procs.is_empty() => self.quit = true,
            KeyCode::Char('q') => self.mode = Mode::ConfirmQuit,
            KeyCode::Up | KeyCode::Char('k') => self.select(-1),
            KeyCode::Down | KeyCode::Char('j') => self.select(1),
            KeyCode::Enter => match self.selected_project().map(|p| p.id.clone()) {
                Some(id) if self.procs.contains_key(&id) => self.stop(&id),
                Some(_) => self.start(self.selected),
                None => {}
            },
            KeyCode::Char('s') => self.start(self.selected),
            KeyCode::Char('x') => {
                if let Some(id) = self.selected_project().map(|p| p.id.clone()) {
                    self.stop(&id);
                }
            }
            KeyCode::Char('S') => self.start_all(),
            KeyCode::Char('X') => self.stop_all(),
            KeyCode::Char('a') => {
                let cwd = std::env::current_dir().unwrap_or_default();
                self.mode = Mode::ScanInput {
                    input: cwd.to_string_lossy().to_string(),
                };
            }
            KeyCode::Char('d') if !self.projects.is_empty() => self.mode = Mode::ConfirmDelete,
            KeyCode::Char('r') => self.reload(),
            KeyCode::Char('c') => {
                if let Some(id) = self.selected_project().map(|p| p.id.clone()) {
                    self.logs.remove(&id);
                    self.log_scroll = 0;
                }
            }
            KeyCode::PageUp => self.log_scroll = self.log_scroll.saturating_add(10),
            KeyCode::PageDown => self.log_scroll = self.log_scroll.saturating_sub(10),
            KeyCode::Home | KeyCode::Char('g') => self.log_scroll = usize::MAX, // clamped at draw time
            KeyCode::End | KeyCode::Char('G') => self.log_scroll = 0,
            _ => {}
        }
    }

    fn handle_scan_input_key(&mut self, key: KeyEvent, mut input: String) {
        match key.code {
            KeyCode::Esc => {}
            KeyCode::Enter => self.run_scan(&input),
            KeyCode::Backspace => {
                input.pop();
                self.mode = Mode::ScanInput { input };
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.mode = Mode::ScanInput {
                    input: String::new(),
                };
            }
            KeyCode::Char(ch) => {
                input.push(ch);
                self.mode = Mode::ScanInput { input };
            }
            _ => self.mode = Mode::ScanInput { input },
        }
    }

    fn handle_scan_pick_key(
        &mut self,
        key: KeyEvent,
        items: Vec<DiscoveredProject>,
        mut checked: Vec<bool>,
        mut cursor: usize,
    ) {
        match key.code {
            KeyCode::Esc => return,
            KeyCode::Enter => return self.add_discovered(items, checked),
            KeyCode::Up | KeyCode::Char('k') => cursor = cursor.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => cursor = (cursor + 1).min(items.len() - 1),
            KeyCode::Char(' ') => checked[cursor] = !checked[cursor],
            KeyCode::Char('a') => {
                let all_on = checked.iter().all(|c| *c);
                checked.iter_mut().for_each(|c| *c = !all_on);
            }
            _ => {}
        }
        self.mode = Mode::ScanPick {
            items,
            checked,
            cursor,
        };
    }

    fn tick(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.handle_msg(msg);
        }
        if self
            .toast
            .as_ref()
            .is_some_and(|t| t.at.elapsed() > TOAST_TTL)
        {
            self.toast = None;
        }
    }
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> std::io::Result<()> {
    while !app.quit {
        app.tick();
        terminal.draw(|frame| ui::draw(frame, app))?;
        if event::poll(TICK)? {
            // Windows reports key releases too; act on presses only.
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.handle_key(key);
                }
            }
        }
    }
    Ok(())
}

fn print_help() {
    let version = env!("CARGO_PKG_VERSION");
    let path = projects_path();
    println!(
        "rojom {version} — terminal dashboard for Rojo Manager

Usage: rojom [--help | --version]

Runs an interactive TUI: pick a project, start/stop its `rojo serve`, watch its logs.
Projects are shared with the desktop app via
  {}

Keys: ↑↓ select · Enter start/stop · s start · x stop · S start all · X stop all
      a add (scan a folder) · d delete · r reload · c clear log · PgUp/PgDn scroll · q quit",
        path.display()
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("rojom {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }

    let mut app = App::new();
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();

    let stopped = app.kill_all();
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
    if stopped > 0 {
        println!("Stopped {stopped} serve(s).");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_without_stop_request_is_an_error_and_clean_exit_is_stopped() {
        let mut app = App::new();
        app.projects = vec![Project {
            id: "p1".into(),
            name: "Game".into(),
            folder: ".".into(),
            project_file: "default.project.json".into(),
            port: 1,
            args: vec![],
        }];

        app.handle_msg(Msg::Exited {
            id: "p1".into(),
            code: Some(1),
        });
        assert!(matches!(app.status_of("p1"), Status::Error(_)));
        assert!(app.toast.as_ref().is_some_and(|t| t.is_error));

        app.stopping.insert("p1".into());
        app.handle_msg(Msg::Exited {
            id: "p1".into(),
            code: Some(1),
        });
        assert_eq!(app.status_of("p1"), Status::Stopped);
        assert!(app.logs["p1"].iter().all(|l| l.stream == "system"));
    }

    /// Real end-to-end check against an installed `rojo`; skipped when it isn't on PATH.
    #[test]
    fn starts_streams_and_kills_a_real_rojo_serve() {
        if Command::new("rojo").arg("--version").output().is_err() {
            eprintln!("rojo not on PATH — skipping");
            return;
        }
        let dir = std::env::temp_dir().join(format!("rojom_serve_test_{}", now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("default.project.json"),
            r#"{"name":"T","tree":{"$className":"DataModel"}}"#,
        )
        .unwrap();

        let mut app = App::new();
        app.projects = vec![Project {
            id: "t".into(),
            name: "T".into(),
            folder: dir.to_string_lossy().to_string(),
            project_file: "default.project.json".into(),
            port: 34999,
            args: vec![],
        }];
        app.start(0);
        let toast = app.toast.as_ref().map(|t| t.text.clone());
        assert_eq!(app.status_of("t"), Status::Running, "{toast:?}");

        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            app.tick();
            if app.logs["t"].iter().any(|l| l.stream != "system") {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let lines: Vec<_> = app.logs["t"].iter().map(|l| l.line.clone()).collect();
        assert!(
            lines
                .iter()
                .any(|l| l.contains("34999") || l.to_lowercase().contains("listening")),
            "{lines:?}"
        );

        assert_eq!(app.kill_all(), 1);
        assert_eq!(app.running_count(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }
}

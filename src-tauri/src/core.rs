// Rojo Manager — shared core.
// Everything here is plain Rust with no Tauri dependency: project definitions,
// the `projects.json` persistence format, and the on-disk auto-scan. The desktop
// app (`lib.rs`) and the terminal UI (`cli/`) both include this file, so the two
// front-ends always agree on the data they share.
// ponytail: shared via `#[path]` from the CLI crate; promote to a workspace crate
// if a third consumer ever shows up.

#![allow(dead_code)]

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const LOG_CAP: usize = 500; // ponytail: per-project ring buffer; bump if devs want longer history
pub const DEFAULT_PORT: u16 = 34872; // matches the form default; first port handed out by auto-scan
pub const MAX_SCAN_DIRS: usize = 20_000; // ponytail: cap walk so a huge/looping tree can't hang the scan

// Directories the scan never descends into — vendored deps, build output, VCS, caches.
// Compared lower-cased. ponytail: add more here if a noisy folder shows up in results.
pub const SKIP_DIRS: &[&str] = &[
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
pub struct Project {
    pub id: String,
    pub name: String,
    pub folder: String,
    pub project_file: String,
    pub port: u16,
    #[serde(default)]
    pub args: Vec<String>,
}

/// A Rojo project found on disk by the auto-scan, ready to be added with one click.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredProject {
    pub name: String,
    pub folder: String,
    pub project_file: String,
    pub port: u16,
    pub reason: String, // which file matched, shown in the UI
}

#[derive(Serialize, Clone)]
pub struct LogLine {
    pub ts: i64,
    pub stream: String, // "stdout" | "stderr" | "system"
    pub line: String,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---- persistence -----------------------------------------------------------

/// `projects.json` lives in the OS config dir under the app identifier, e.g.
/// `%APPDATA%\\com.rojomanager.app` on Windows — the same place Tauri resolves
/// `app_config_dir()` to, so the desktop app and the CLI edit one list.
pub const APP_IDENTIFIER: &str = "com.rojomanager.app";

pub fn read_projects_from(path: &Path) -> Vec<Project> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn write_projects_to(path: &Path, list: &[Project]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

// ---- auto-scan -------------------------------------------------------------

/// Canonical path of `p`, falling back to the path as-given if it can't be resolved.
pub fn canonical_or(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Resolved on-disk path of a project's `.project.json`, used to compare against
/// what's already saved (handles relative vs absolute project files uniformly).
pub fn resolve_project_path(folder: &str, project_file: &str) -> PathBuf {
    canonical_or(&PathBuf::from(folder).join(project_file))
}

/// Pull the `"name"` field out of a Rojo project file; fall back to None on any error.
pub fn project_name(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("name")?.as_str().map(str::to_string)
}

/// Next port not already taken, starting at `DEFAULT_PORT`. Reserves the chosen
/// port in `used` so repeated calls hand out distinct ports.
pub fn next_free_port(used: &mut HashSet<u16>) -> u16 {
    let mut port = DEFAULT_PORT;
    while port < u16::MAX && used.contains(&port) {
        port += 1; // ponytail: linear probe is plenty for a handful of projects
    }
    used.insert(port);
    port
}

/// Pure core of the scan, split out so it's testable without an `AppHandle`.
pub fn discover(root_path: &Path, existing: &[Project]) -> Vec<DiscoveredProject> {
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

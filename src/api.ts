import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { DiscoveredProject, LogLine, Project, StatusPayload } from "./types";

export const listProjects = () => invoke<Project[]>("list_projects");
export const scanProjects = (root: string) => invoke<DiscoveredProject[]>("scan_projects", { root });
export const saveProject = (project: Project) => invoke<Project[]>("save_project", { project });
export const deleteProject = (id: string) => invoke<Project[]>("delete_project", { id });
export const startProject = (project: Project) => invoke<void>("start_project", { project });
export const stopProject = (id: string) => invoke<void>("stop_project", { id });
export const stopAll = () => invoke<void>("stop_all");
export const getLogs = (id: string) => invoke<LogLine[]>("get_logs", { id });
export const getRunning = () => invoke<string[]>("get_running");

export const onLogLine = (cb: (id: string, line: LogLine) => void): Promise<UnlistenFn> =>
  listen<[string, LogLine]>("log-line", (e) => cb(e.payload[0], e.payload[1]));

export const onStatus = (cb: (p: StatusPayload) => void): Promise<UnlistenFn> =>
  listen<StatusPayload>("status-changed", (e) => cb(e.payload));

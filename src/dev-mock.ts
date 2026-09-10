// ponytail: browser-only stand-in for the Tauri bridge so `vite` alone renders the UI. Never loaded in the app.
import type { LogLine, Project } from "./types";

type Handler = (e: { event: string; id: number; payload: unknown }) => void;
const callbacks = new Map<number, Handler>();
const listeners = new Map<string, Set<number>>();
let nextId = 1;

let projects: Project[] = [
  { id: "a", name: "Tower Defense", folder: "D:/Games/tower-defense", projectFile: "default.project.json", port: 34872, args: [] },
  { id: "b", name: "Pet Sim", folder: "D:/Games/pet-sim", projectFile: "place.project.json", port: 34873, args: ["--watch"] },
  { id: "c", name: "Lobby", folder: "D:/Games/lobby", projectFile: "default.project.json", port: 34874, args: [] },
];
const running = new Set<string>();
const logs = new Map<string, LogLine[]>();
const timers = new Map<string, number>();

function emit(event: string, payload: unknown) {
  for (const id of listeners.get(event) ?? []) callbacks.get(id)?.({ event, id, payload });
}
function log(id: string, stream: LogLine["stream"], line: string) {
  const entry = { ts: Date.now(), stream, line };
  logs.set(id, (logs.get(id) ?? []).concat(entry));
  emit("log-line", [id, entry]);
}
function start(p: Project) {
  running.add(p.id);
  emit("status-changed", { id: p.id, status: "running" });
  log(p.id, "system", `rojo serve ${p.projectFile} --port ${p.port}`);
  log(p.id, "stdout", `Rojo server listening:\n  Address: localhost\n  Port:    ${p.port}`);
  log(p.id, "stdout", "Visit localhost:" + p.port + " in your browser for more information");
  let n = 0;
  timers.set(p.id, window.setInterval(() => {
    n++;
    log(p.id, n % 7 === 0 ? "stderr" : "stdout", n % 7 === 0 ? `[WARN] instance ReplicatedStorage.Shared.Util${n} has no parent` : `[INFO] change detected: src/server/Handlers/Round${n}.luau`);
  }, 1400));
}
function stop(id: string) {
  window.clearInterval(timers.get(id));
  timers.delete(id);
  if (running.delete(id)) {
    log(id, "system", "process exited with code 0");
    emit("status-changed", { id, status: "stopped" });
  }
}

const commands: Record<string, (args: any) => unknown> = {
  list_projects: () => projects,
  get_running: () => [...running],
  get_logs: ({ id }) => logs.get(id) ?? [],
  save_project: ({ project }) => {
    const i = projects.findIndex((p) => p.id === project.id);
    projects = i >= 0 ? projects.map((p) => (p.id === project.id ? project : p)) : projects.concat(project);
    return projects;
  },
  delete_project: ({ id }) => (stop(id), (projects = projects.filter((p) => p.id !== id))),
  start_project: ({ project }) => start(project),
  stop_project: ({ id }) => stop(id),
  stop_all: () => [...running].forEach(stop),
  scan_projects: () => [
    { name: "Racing", folder: "D:/Games/racing", projectFile: "default.project.json", port: 34875, reason: "default.project.json" },
    { name: "Obby", folder: "D:/Games/obby", projectFile: "obby.project.json", port: 34876, reason: "obby.project.json" },
  ],
  "plugin:event|listen": ({ event, handler }) => {
    if (!listeners.has(event)) listeners.set(event, new Set());
    listeners.get(event)!.add(handler);
    return handler;
  },
  "plugin:event|unlisten": ({ event, eventId }) => listeners.get(event)?.delete(eventId),
  "plugin:dialog|open": () => "D:/Games/new-project",
};

(window as any).__TAURI_INTERNALS__ = {
  transformCallback: (cb: Handler) => (callbacks.set(nextId, cb), nextId++),
  unregisterCallback: (id: number) => callbacks.delete(id),
  invoke: async (cmd: string, args: any) => {
    const fn = commands[cmd];
    if (!fn) throw new Error(`mock: unknown command ${cmd}`);
    return fn(args ?? {});
  },
};

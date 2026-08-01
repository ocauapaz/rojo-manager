import { useEffect, useMemo, useState } from "react";
import * as api from "./api";
import type { LogLine, Project, Status } from "./types";
import ProjectForm from "./components/ProjectForm";
import ScanModal from "./components/ScanModal";
import LogPanel from "./components/LogPanel";
import ThemeToggle from "./components/ThemeToggle";

const CLIENT_LOG_CAP = 2000;

const STATUS_LABEL: Record<Status, string> = {
  running: "Running",
  stopped: "Stopped",
  error: "Error",
};

export default function App() {
  const [projects, setProjects] = useState<Project[]>([]);
  const [statuses, setStatuses] = useState<Record<string, Status>>({});
  const [messages, setMessages] = useState<Record<string, string>>({});
  const [logs, setLogs] = useState<Record<string, LogLine[]>>({});
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [editing, setEditing] = useState<Project | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [scanOpen, setScanOpen] = useState(false);
  const [toast, setToast] = useState<string>("");

  // --- bootstrap + live subscriptions ---------------------------------------
  useEffect(() => {
    let unlistenLog: (() => void) | undefined;
    let unlistenStatus: (() => void) | undefined;

    (async () => {
      const [list, running] = await Promise.all([api.listProjects(), api.getRunning()]);
      setProjects(list);
      setStatuses(Object.fromEntries(running.map((id) => [id, "running" as Status])));
      setSelectedId((cur) => cur ?? list[0]?.id ?? null);
      // Hydrate logs for anything already serving (e.g. window reopened from tray).
      const hist = await Promise.all(running.map(async (id) => [id, await api.getLogs(id)] as const));
      setLogs(Object.fromEntries(hist));
      setLoading(false);
    })();

    api
      .onLogLine((id, line) => {
        setLogs((prev) => {
          const next = (prev[id] ?? []).concat(line);
          if (next.length > CLIENT_LOG_CAP) next.splice(0, next.length - CLIENT_LOG_CAP);
          return { ...prev, [id]: next };
        });
      })
      .then((fn) => (unlistenLog = fn));

    api
      .onStatus((p) => {
        setStatuses((prev) => ({ ...prev, [p.id]: p.status }));
        setMessages((prev) => ({ ...prev, [p.id]: p.message ?? "" }));
      })
      .then((fn) => (unlistenStatus = fn));

    return () => {
      unlistenLog?.();
      unlistenStatus?.();
    };
  }, []);

  const selected = useMemo(() => projects.find((p) => p.id === selectedId) ?? null, [projects, selectedId]);
  const statusOf = (id: string): Status => statuses[id] ?? "stopped";
  const runningCount = projects.filter((p) => statusOf(p.id) === "running").length;

  function flash(msg: string) {
    setToast(msg);
    window.setTimeout(() => setToast(""), 4000);
  }

  // --- actions --------------------------------------------------------------
  async function handleStart(p: Project) {
    try {
      if (!logs[p.id]) setLogs((prev) => ({ ...prev, [p.id]: [] }));
      await api.startProject(p);
    } catch (e) {
      flash(String(e));
    }
  }

  async function handleStop(id: string) {
    try {
      await api.stopProject(id);
    } catch (e) {
      flash(String(e));
    }
  }

  async function handleSave(p: Project) {
    const next = await api.saveProject(p);
    setProjects(next);
    setSelectedId(p.id);
    setFormOpen(false);
    setEditing(null);
  }

  async function handleDelete(p: Project) {
    if (!confirm(`Remove "${p.name}"? This stops its serve and forgets the project.`)) return;
    const next = await api.deleteProject(p.id);
    setProjects(next);
    setLogs(({ [p.id]: _drop, ...rest }) => rest);
    if (selectedId === p.id) setSelectedId(next[0]?.id ?? null);
  }

  // Save a discovered snippet without disturbing the scan modal (it stays open for more).
  async function handleAddDiscovered(p: Project) {
    const next = await api.saveProject(p);
    setProjects(next);
    setSelectedId(p.id);
  }

  function openNew() {
    setEditing(null);
    setFormOpen(true);
  }

  function openEdit(p: Project) {
    setEditing(p);
    setFormOpen(true);
  }

  // --- render ---------------------------------------------------------------
  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <img className="brand-logo" src="/rojo-logo.png" alt="Rojo" draggable={false} />
          <span className="brand-caption">
            Manager · {runningCount > 0 ? <em>{runningCount} serving</em> : "idle"}
          </span>
        </div>

        <div className="side-actions">
          <button className="btn primary block" onClick={openNew}>+ Add project</button>
          <button className="btn ghost block" onClick={() => setScanOpen(true)}>Scan a folder…</button>
        </div>

        <nav className="proj-list" aria-label="Projects">
          {loading ? (
            <div className="side-skeleton">
              {[0, 1, 2].map((i) => (
                <div key={i} className="skel-row" />
              ))}
            </div>
          ) : projects.length === 0 ? (
            <p className="side-empty">No projects yet.</p>
          ) : (
            projects.map((p) => {
              const st = statusOf(p.id);
              return (
                <button
                  key={p.id}
                  className={`proj-item ${selectedId === p.id ? "active" : ""}`}
                  onClick={() => setSelectedId(p.id)}
                >
                  <span className={`dot ${st}`} title={STATUS_LABEL[st]} />
                  <span className="proj-name">{p.name}</span>
                  <span className="proj-port">:{p.port}</span>
                </button>
              );
            })
          )}
        </nav>

        <div className="side-foot">
          <button className="btn ghost block" onClick={() => api.stopAll()} disabled={runningCount === 0}>
            Stop all serves
          </button>
          <ThemeToggle />
        </div>
      </aside>

      <main className="detail">
        {loading ? (
          <CenterNote title="Loading projects…" />
        ) : !selected ? (
          <EmptyState onAdd={openNew} onScan={() => setScanOpen(true)} hasProjects={projects.length > 0} />
        ) : (
          <div className="detail-body" key={selected.id}>
            <ProjectDetail
              project={selected}
              status={statusOf(selected.id)}
              message={messages[selected.id]}
              logs={logs[selected.id] ?? []}
              onStart={() => handleStart(selected)}
              onStop={() => handleStop(selected.id)}
              onEdit={() => openEdit(selected)}
              onDelete={() => handleDelete(selected)}
            />
          </div>
        )}
      </main>

      {formOpen && (
        <ProjectForm
          initial={editing}
          existingPorts={projects.map((p) => ({ id: p.id, port: p.port }))}
          onCancel={() => {
            setFormOpen(false);
            setEditing(null);
          }}
          onSave={handleSave}
        />
      )}

      {scanOpen && <ScanModal onClose={() => setScanOpen(false)} onAdd={handleAddDiscovered} />}

      {toast && (
        <div className="toast" role="alert">
          {toast}
        </div>
      )}
    </div>
  );
}

function ProjectDetail({
  project,
  status,
  message,
  logs,
  onStart,
  onStop,
  onEdit,
  onDelete,
}: {
  project: Project;
  status: Status;
  message?: string;
  logs: LogLine[];
  onStart: () => void;
  onStop: () => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const running = status === "running";
  return (
    <>
      <header className="detail-head">
        <div className="detail-title">
          <span className={`dot ${status} lg`} />
          <h1>{project.name}</h1>
          <span className={`badge ${status}`}>{STATUS_LABEL[status]}</span>
        </div>
        <div className="detail-actions">
          {running ? (
            <button className="btn stop" onClick={onStop}>
              ■ Stop
            </button>
          ) : (
            <button className="btn primary" onClick={onStart}>
              ▶ Start serve
            </button>
          )}
          <button className="btn ghost" onClick={onEdit} disabled={running}>
            Edit
          </button>
          <button className="btn ghost danger" onClick={onDelete}>
            Remove
          </button>
        </div>
      </header>

      {status === "error" && message && (
        <div className="banner error">
          <strong>Serve error</strong> {message}
        </div>
      )}

      <section className="meta-grid">
        <Meta label="Port" value={`:${project.port}`} mono />
        <Meta label="Project file" value={project.projectFile} mono />
        <Meta label="Folder" value={project.folder} mono wide />
        {project.args.length > 0 && <Meta label="Extra args" value={project.args.join(" ")} mono wide />}
      </section>

      <section className="log-section">
        <div className="log-head">
          <span className="log-title">Output</span>
          <span className="log-count">{logs.length} lines</span>
        </div>
        <LogPanel lines={logs} />
      </section>
    </>
  );
}

function Meta({ label, value, mono, wide }: { label: string; value: string; mono?: boolean; wide?: boolean }) {
  return (
    <div className={`meta ${wide ? "wide" : ""}`}>
      <span className="meta-label">{label}</span>
      <span className={`meta-value ${mono ? "mono" : ""}`}>{value}</span>
    </div>
  );
}

function EmptyState({ onAdd, onScan, hasProjects }: { onAdd: () => void; onScan: () => void; hasProjects: boolean }) {
  return (
    <div className="empty">
      <div className="empty-logo-wrap">
        <img className="empty-logo" src="/rojo-logo.png" alt="Rojo Manager" draggable={false} />
      </div>
      <h1>{hasProjects ? "Select a project" : "No projects yet"}</h1>
      <p>
        {hasProjects
          ? "Pick a project from the sidebar to see its status and stream its logs."
          : "Add a Rojo project to start managing local serve processes — each runs independently on its own port."}
      </p>
      {!hasProjects && (
        <div className="empty-actions">
          <button className="btn primary" onClick={onAdd}>
            + Add your first project
          </button>
          <button className="btn ghost" onClick={onScan}>
            Scan a folder for projects
          </button>
        </div>
      )}
    </div>
  );
}

function CenterNote({ title }: { title: string }) {
  return (
    <div className="empty">
      <div className="spinner" aria-hidden />
      <h1>{title}</h1>
    </div>
  );
}

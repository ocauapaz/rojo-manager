import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import * as api from "../api";
import type { DiscoveredProject, Project } from "../types";

interface Props {
  onClose: () => void;
  onAdd: (p: Project) => Promise<void> | void;
}

const newId = () =>
  (crypto.randomUUID && crypto.randomUUID()) || `p_${Date.now()}_${Math.random().toString(36).slice(2)}`;

const keyOf = (d: DiscoveredProject) => `${d.folder}|${d.projectFile}`;

export default function ScanModal({ onClose, onAdd }: Props) {
  const [root, setRoot] = useState("");
  const [scanning, setScanning] = useState(false);
  const [scanned, setScanned] = useState(false);
  const [error, setError] = useState("");
  const [results, setResults] = useState<DiscoveredProject[]>([]);
  const [added, setAdded] = useState<Set<string>>(new Set());

  async function chooseAndScan() {
    const picked = await open({ directory: true, title: "Select a folder to scan for Rojo projects" });
    if (typeof picked !== "string") return;
    setRoot(picked);
    setError("");
    setScanned(false);
    setResults([]);
    setAdded(new Set());
    setScanning(true);
    try {
      setResults(await api.scanProjects(picked));
      setScanned(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setScanning(false);
    }
  }

  async function add(d: DiscoveredProject) {
    const project: Project = {
      id: newId(),
      name: d.name,
      folder: d.folder,
      projectFile: d.projectFile,
      port: d.port,
      args: [],
    };
    try {
      await onAdd(project);
      setAdded((prev) => new Set(prev).add(keyOf(d)));
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="modal-scrim" onMouseDown={onClose}>
      <div className="modal" role="dialog" aria-modal="true" aria-label="Scan for projects" onMouseDown={(e) => e.stopPropagation()}>
        <header className="modal-head">
          <h2>Find Rojo projects</h2>
          <button className="icon-btn" onClick={onClose} aria-label="Close">✕</button>
        </header>

        <div className="form">
          <label className="field">
            <span>Folder to scan</span>
            <div className="path-row">
              <input value={root} readOnly placeholder="No folder chosen yet" spellCheck={false} />
              <button type="button" className="btn ghost" onClick={chooseAndScan} disabled={scanning}>
                {root ? "Rescan…" : "Choose folder…"}
              </button>
            </div>
            <small className="hint">Scans recursively, skipping node_modules, build output, and caches.</small>
          </label>

          {error && <p className="form-error">{error}</p>}

          {scanning ? (
            <div className="scan-state">
              <div className="spinner" aria-hidden />
              <p>Scanning {root}…</p>
            </div>
          ) : scanned && results.length === 0 ? (
            <div className="scan-state">
              <p>No new Rojo projects found here. Anything already added is skipped.</p>
            </div>
          ) : results.length > 0 ? (
            <div className="scan-list">
              {results.map((d) => {
                const done = added.has(keyOf(d));
                return (
                  <div className={`scan-item ${done ? "done" : ""}`} key={keyOf(d)}>
                    <div className="scan-info">
                      <span className="scan-name">{d.name}</span>
                      <span className="scan-path mono">{d.folder}</span>
                      <span className="scan-meta mono">
                        {d.reason} · port :{d.port}
                      </span>
                    </div>
                    <button
                      className={`btn ${done ? "ghost" : "primary"}`}
                      onClick={() => add(d)}
                      disabled={done}
                    >
                      {done ? "Added ✓" : "Add"}
                    </button>
                  </div>
                );
              })}
            </div>
          ) : null}

          <footer className="modal-foot">
            <button type="button" className="btn ghost" onClick={onClose}>Done</button>
          </footer>
        </div>
      </div>
    </div>
  );
}

import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { Project } from "../types";

interface Props {
  initial: Project | null;
  existingPorts: { id: string; port: number }[];
  onCancel: () => void;
  onSave: (p: Project) => void;
}

const newId = () =>
  (crypto.randomUUID && crypto.randomUUID()) || `p_${Date.now()}_${Math.random().toString(36).slice(2)}`;

export default function ProjectForm({ initial, existingPorts, onCancel, onSave }: Props) {
  const [name, setName] = useState(initial?.name ?? "");
  const [folder, setFolder] = useState(initial?.folder ?? "");
  const [projectFile, setProjectFile] = useState(initial?.projectFile ?? "default.project.json");
  const [port, setPort] = useState(String(initial?.port ?? 34872));
  const [args, setArgs] = useState((initial?.args ?? []).join(" "));
  const [error, setError] = useState("");

  const id = initial?.id ?? newId();

  async function pickFolder() {
    const picked = await open({ directory: true, title: "Select project folder" });
    if (typeof picked === "string") {
      setFolder(picked);
      if (!name) setName(picked.split(/[\\/]/).filter(Boolean).pop() ?? "");
    }
  }

  async function pickFile() {
    const picked = await open({
      title: "Select Rojo project file",
      defaultPath: folder || undefined,
      filters: [{ name: "Rojo project", extensions: ["json"] }],
    });
    if (typeof picked === "string") {
      // Prefer a path relative to the project folder for portability.
      const rel = folder && picked.startsWith(folder) ? picked.slice(folder.length).replace(/^[\\/]/, "") : picked;
      setProjectFile(rel);
    }
  }

  function submit(e: React.FormEvent) {
    e.preventDefault();
    const portNum = Number(port);
    if (!name.trim()) return setError("Give the project a display name.");
    if (!folder.trim()) return setError("Choose the project folder.");
    if (!projectFile.trim()) return setError("Set the Rojo project file (e.g. default.project.json).");
    if (!Number.isInteger(portNum) || portNum < 1 || portNum > 65535)
      return setError("Port must be a whole number between 1 and 65535.");
    if (existingPorts.some((p) => p.port === portNum && p.id !== id))
      return setError(`Port ${portNum} is already assigned to another project.`);

    onSave({
      id,
      name: name.trim(),
      folder: folder.trim(),
      projectFile: projectFile.trim(),
      port: portNum,
      args: args.trim() ? args.trim().split(/\s+/) : [],
    });
  }

  return (
    <div className="modal-scrim" onMouseDown={onCancel}>
      <div className="modal" role="dialog" aria-modal="true" aria-label="Project settings" onMouseDown={(e) => e.stopPropagation()}>
        <header className="modal-head">
          <h2>{initial ? "Edit project" : "New project"}</h2>
          <button className="icon-btn" onClick={onCancel} aria-label="Close">✕</button>
        </header>

        <form onSubmit={submit} className="form">
          <label className="field">
            <span>Display name</span>
            <input value={name} onChange={(e) => setName(e.target.value)} placeholder="My Game — Main place" autoFocus />
          </label>

          <label className="field">
            <span>Project folder</span>
            <div className="path-row">
              <input value={folder} onChange={(e) => setFolder(e.target.value)} placeholder="C:\path\to\project" spellCheck={false} />
              <button type="button" className="btn ghost" onClick={pickFolder}>Browse…</button>
            </div>
          </label>

          <label className="field">
            <span>Rojo project file</span>
            <div className="path-row">
              <input value={projectFile} onChange={(e) => setProjectFile(e.target.value)} placeholder="default.project.json" spellCheck={false} />
              <button type="button" className="btn ghost" onClick={pickFile}>Browse…</button>
            </div>
            <small className="hint">Relative to the project folder, or an absolute path.</small>
          </label>

          <div className="field-row">
            <label className="field">
              <span>Serve port</span>
              <input value={port} onChange={(e) => setPort(e.target.value)} inputMode="numeric" placeholder="34872" />
            </label>
            <label className="field grow">
              <span>Extra args <em>(optional)</em></span>
              <input value={args} onChange={(e) => setArgs(e.target.value)} placeholder="--verbose" spellCheck={false} />
            </label>
          </div>

          {error && <p className="form-error">{error}</p>}

          <footer className="modal-foot">
            <button type="button" className="btn ghost" onClick={onCancel}>Cancel</button>
            <button type="submit" className="btn primary">{initial ? "Save changes" : "Add project"}</button>
          </footer>
        </form>
      </div>
    </div>
  );
}

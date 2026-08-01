import { useEffect, useState } from "react";

type Mode = "light" | "dark" | "system";
const KEY = "rojo-theme";
const mql = () => window.matchMedia("(prefers-color-scheme: dark)");

function resolve(mode: Mode): "light" | "dark" {
  return mode === "system" ? (mql().matches ? "dark" : "light") : mode;
}

export function applyStoredTheme() {
  const mode = (localStorage.getItem(KEY) as Mode) || "system";
  document.documentElement.dataset.theme = resolve(mode);
}

const OPTS: { mode: Mode; label: string; icon: string }[] = [
  { mode: "light", label: "Light", icon: "☀" },
  { mode: "dark", label: "Dark", icon: "☾" },
  { mode: "system", label: "System", icon: "◑" },
];

export default function ThemeToggle() {
  const [mode, setMode] = useState<Mode>(() => (localStorage.getItem(KEY) as Mode) || "system");

  useEffect(() => {
    localStorage.setItem(KEY, mode);
    document.documentElement.dataset.theme = resolve(mode);
    if (mode !== "system") return;
    const m = mql();
    const onChange = () => (document.documentElement.dataset.theme = resolve("system"));
    m.addEventListener("change", onChange);
    return () => m.removeEventListener("change", onChange);
  }, [mode]);

  return (
    <div className="theme-toggle" role="group" aria-label="Theme">
      {OPTS.map((o) => (
        <button
          key={o.mode}
          className={`theme-opt ${mode === o.mode ? "active" : ""}`}
          onClick={() => setMode(o.mode)}
          aria-pressed={mode === o.mode}
          title={o.label}
        >
          <span aria-hidden>{o.icon}</span>
        </button>
      ))}
    </div>
  );
}

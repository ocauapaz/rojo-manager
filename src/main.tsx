import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { applyStoredTheme } from "./components/ThemeToggle";
import "./styles.css";

if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) await import("./dev-mock");

applyStoredTheme();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

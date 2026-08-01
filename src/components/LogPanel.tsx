import { useEffect, useRef } from "react";
import type { LogLine } from "../types";

const MAX_RENDER = 500; // ponytail: only paint the tail so a chatty serve never janks the UI

function fmt(ts: number) {
  const d = new Date(ts);
  return d.toLocaleTimeString(undefined, { hour12: false }) + "." + String(d.getMilliseconds()).padStart(3, "0");
}

export default function LogPanel({ lines }: { lines: LogLine[] }) {
  const ref = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  // Stay pinned to the bottom unless the user has scrolled up to read history.
  useEffect(() => {
    const el = ref.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [lines]);

  function onScroll() {
    const el = ref.current;
    if (!el) return;
    stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
  }

  if (lines.length === 0) {
    return (
      <div className="log-empty">
        <span className="log-empty-mark">/_</span>
        <p>No output yet. Start the serve to stream rojo's logs here.</p>
      </div>
    );
  }

  const shown = lines.length > MAX_RENDER ? lines.slice(-MAX_RENDER) : lines;

  return (
    <div className="log-view" ref={ref} onScroll={onScroll}>
      {lines.length > MAX_RENDER && (
        <div className="log-trim">… {lines.length - MAX_RENDER} earlier lines hidden</div>
      )}
      {shown.map((l, i) => (
        <div key={i} className={`log-line stream-${l.stream}`}>
          <span className="log-ts">{fmt(l.ts)}</span>
          <span className="log-text">{l.line || " "}</span>
        </div>
      ))}
    </div>
  );
}

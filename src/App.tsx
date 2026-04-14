import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { WorkItem, PushbackEntry } from "./types";
import { WorkItemCard } from "./components/WorkItemCard";

const POLL_INTERVAL_MS = 5_000;

function relativeTime(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const m = Math.floor(diff / 60_000);
  const h = Math.floor(m / 60);
  const d = Math.floor(h / 24);
  if (d > 0) return `${d}d ago`;
  if (h > 0) return `${h}h ago`;
  if (m > 0) return `${m}m ago`;
  return "just now";
}

function shortPath(p: string): string {
  return p.replace(/^\/Users\/[^/]+\//, "~/");
}

export default function App() {
  const [items, setItems] = useState<WorkItem[]>([]);
  const [log, setLog] = useState<PushbackEntry[]>([]);
  const [updatedAt, setUpdatedAt] = useState<Date | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function refresh() {
    try {
      const [newItems, newLog] = await Promise.all([
        invoke<WorkItem[]>("list_work_items"),
        invoke<PushbackEntry[]>("list_pushback_log"),
      ]);
      setItems(newItems);
      setLog(newLog);
      setUpdatedAt(new Date());
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    void refresh();
    const id = setInterval(() => void refresh(), POLL_INTERVAL_MS);
    return () => clearInterval(id);
  }, []);

  const active    = items.filter((i) => i.status === "active");
  const blocked   = items.filter((i) => i.status === "blocked");
  const completed = items.filter((i) => i.status === "completed");

  return (
    <>
      <header className="header">
        <span className="header-title">Angel</span>
        <div className="header-meta">
          <span className="badge badge-active">{active.length} active</span>
          <span className="badge badge-blocked">{blocked.length} blocked</span>
          <span className="badge badge-completed">{completed.length} done</span>
          {updatedAt && <span>updated {relativeTime(updatedAt.toISOString())}</span>}
        </div>
      </header>

      <main className="main">
        {error && <div className="error-msg">Error: {error}</div>}

        <div className="columns">
          <Column title="Active"    dot="active"    items={active}    />
          <Column title="Blocked"   dot="blocked"   items={blocked}   />
          <Column title="Completed" dot="completed" items={completed} />
        </div>

        <div>
          <div className="section-label">Recent Alerts</div>
          {log.length === 0 ? (
            <div className="log-empty">No pushback events yet.</div>
          ) : (
            <table className="log-table">
              <thead>
                <tr>
                  <th>When</th>
                  <th>Escalation</th>
                  <th>Session</th>
                  <th>Deep Link</th>
                </tr>
              </thead>
              <tbody>
                {log.map((entry) => (
                  <tr key={entry.id}>
                    <td style={{ whiteSpace: "nowrap" }}>
                      {relativeTime(entry.triggered_at)}
                    </td>
                    <td>
                      <span className={`escalation-badge escalation-${entry.escalation_level}`}>
                        {entry.escalation_level}
                      </span>
                    </td>
                    <td>
                      <span className="log-path" title={entry.new_session_path}>
                        {shortPath(entry.new_session_path)}
                      </span>
                    </td>
                    <td>
                      <span className="card-link" title={entry.deep_link}>
                        {entry.deep_link}
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </main>
    </>
  );
}

interface ColumnProps {
  title: string;
  dot: "active" | "blocked" | "completed";
  items: WorkItem[];
}

function Column({ title, dot, items }: ColumnProps) {
  return (
    <div>
      <div className="column-header">
        <span className={`column-dot column-dot-${dot}`} />
        {title}
        <span className="column-count">{items.length}</span>
      </div>
      {items.length === 0 ? (
        <div className="column-empty">Nothing here.</div>
      ) : (
        items.map((item) => <WorkItemCard key={item.id} item={item} />)
      )}
    </div>
  );
}

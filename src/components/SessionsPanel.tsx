import { useEffect, useState } from "react";
import { sessionAction, sessionLaunch, sessions } from "../lib/ipc";
import { fmtDate } from "../lib/format";
import type { SessionInfo } from "../types";

export function SessionsPanel({ active }: { active: boolean }) {
  const [list, setList] = useState<SessionInfo[] | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = () => {
    sessions().then(setList).catch((e) => setError(String(e)));
  };

  useEffect(() => {
    if (active && list === null) load();
  }, [active, list]);

  if (!active) return null;

  const act = async (id: string, kind: "resume" | "fork" | "archive" | "delete") => {
    setError(null);
    try {
      if (kind === "resume" || kind === "fork") {
        await sessionLaunch(id, kind);
      } else {
        if (kind === "delete" && confirmDelete !== id) {
          setConfirmDelete(id);
          return;
        }
        setConfirmDelete(null);
        await sessionAction(id, kind);
        load();
      }
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div>
      {error && <div className="warning-strip">⚠ {error}</div>}
      {list === null ? (
        <div className="empty">
          <span className="spin" style={{ display: "inline-block" }} />
        </div>
      ) : list.length === 0 ? (
        <div className="empty">
          <div className="big">💬</div>
          No sessions found in ~/.codex/sessions.
        </div>
      ) : (
        list.map((s) => (
          <div className="session-row" key={s.id}>
            <div style={{ minWidth: 0, flex: 1 }}>
              <div className="title">{s.title}</div>
              <div className="meta">
                {fmtDate(s.createdAt)} · {s.messageCount} msgs · {s.cwd}
              </div>
            </div>
            <button className="btn sm" onClick={() => act(s.id, "resume")}>Resume</button>
            <button className="btn sm" onClick={() => act(s.id, "fork")}>Fork</button>
            <button className="btn sm" onClick={() => act(s.id, "archive")}>Archive</button>
            <button
              className={`btn sm ${confirmDelete === s.id ? "danger" : ""}`}
              onClick={() => act(s.id, "delete")}
            >
              {confirmDelete === s.id ? "Really delete?" : "Delete"}
            </button>
          </div>
        ))
      )}
    </div>
  );
}

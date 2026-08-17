import { useEffect, useState } from "react";
import { activityLog, auditLog } from "../lib/ipc";
import type { ActivityEntry, AuditEntry } from "../types";

export function ActivityPanel({ active }: { active: boolean }) {
  const [seg, setSeg] = useState<"activity" | "audit">("activity");
  const [activity, setActivity] = useState<ActivityEntry[]>([]);
  const [audit, setAudit] = useState<AuditEntry[]>([]);

  useEffect(() => {
    if (!active) return;
    activityLog(60).then(setActivity).catch(() => {});
    auditLog(60).then(setAudit).catch(() => {});
  }, [active]);

  if (!active) return null;

  const fmt = (ts: number) =>
    new Date(ts * 1000).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });

  return (
    <div className="card">
      <div className="card-head">
        <h3>LOGS</h3>
        <div className="spacer" />
        <div className="seg" style={{ width: 260 }}>
          <button className={seg === "activity" ? "on" : ""} onClick={() => setSeg("activity")}>
            Warm-up activity
          </button>
          <button className={seg === "audit" ? "on" : ""} onClick={() => setSeg("audit")}>
            Audit
          </button>
        </div>
      </div>

      {seg === "activity" ? (
        activity.length === 0 ? (
          <div className="empty">No warm-ups yet. Every request the app makes will show up here.</div>
        ) : (
          <div className="log-list">
            {activity.map((a, i) => (
              <div className="log-row" key={i}>
                <span className="ts">{fmt(a.ts)}</span>
                <span className={`log-dot ${a.ok ? "ok" : "err"}`} />
                <span className="who">{a.accountName}</span>
                <span className="what">{a.detail || a.action}</span>
              </div>
            ))}
          </div>
        )
      ) : audit.length === 0 ? (
        <div className="empty">No switches recorded yet.</div>
      ) : (
        <div className="log-list">
          {audit.map((a, i) => (
            <div className="log-row" key={i}>
              <span className="ts">{fmt(a.ts)}</span>
              <span className="what">{a.kind}</span>
              <span className="who">{a.detail}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

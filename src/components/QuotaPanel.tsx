import { useState } from "react";
import { fmtDate, fmtTokens, initials, timeAgo } from "../lib/format";
import type { AccountPublic, UsageSnapshot, WarmupConfig } from "../types";
import { setWarmup, warmup } from "../lib/ipc";
import { Gauge } from "./Gauge";
import { Sparkline } from "./Sparkline";

export function QuotaPanel({
  account,
  snapshot,
  now,
  refreshing,
  onRefresh,
  onOpenCodex,
  onRename,
  onExport,
  onWarmupSaved,
}: {
  account: AccountPublic;
  snapshot: UsageSnapshot | undefined;
  now: number;
  refreshing: boolean;
  onRefresh: () => void;
  onOpenCodex: () => void;
  onRename: () => void;
  onExport: () => void;
  onWarmupSaved: (w: WarmupConfig) => void;
}) {
  const [timeInput, setTimeInput] = useState("");

  const sess = snapshot?.session ?? null;
  const weekly = snapshot?.weekly ?? null;
  const stats = snapshot?.stats ?? null;
  const credits = snapshot?.resetCredits ?? null;
  const isKey = account.kind === "apikey";

  const updateWarmup = async (patch: Partial<WarmupConfig>) => {
    const next = { ...account.warmup, ...patch };
    await setWarmup(account.id, next);
    onWarmupSaved(next);
  };

  const addTime = () => {
    if (!/^\d{2}:\d{2}$/.test(timeInput)) return;
    if (account.warmup.timedAt.includes(timeInput)) return;
    updateWarmup({ timedAt: [...account.warmup.timedAt, timeInput].sort() });
    setTimeInput("");
  };

  return (
    <div className="detail-grid">
      {/* Hero */}
      <div className="card full">
        <div className="acct-hero">
          <div className={`avatar ${account.kind}`}>{initials(account.name)}</div>
          <div>
            <h2>{account.name}</h2>
            <div className="email">{account.email ?? "no email on record"}</div>
          </div>
          <div className="spacer" />
          {account.active && <span className="badge ok">● ACTIVE</span>}
          {account.plan && <span className="badge plan">{account.plan.toUpperCase()}</span>}
          <span className="badge kind">{account.kind === "chatgpt" ? "ChatGPT" : "API key"}</span>
          <button className="btn primary" onClick={() => void onOpenCodex()}>
            {account.active ? "Open Codex" : "Switch & Open Codex"}
          </button>
          <button className="btn" onClick={onRename}>Rename</button>
          <button className="btn" onClick={onExport}>Export auth.json</button>
        </div>
      </div>

      {isKey ? (
        <div className="card full">
          <div className="empty">
            <div className="big">🔑</div>
            API-key accounts don't expose ChatGPT usage stats.
            <br />
            Switch to this account and launch Codex to use it.
          </div>
        </div>
      ) : (
        <>
          {/* Gauges */}
          <div className="card full">
            <div className="card-head">
              <h3>RATE LIMITS</h3>
              <div className="spacer" />
              {snapshot && (
                <span className="muted" style={{ fontSize: 11 }}>
                  fetched {timeAgo(snapshot.fetchedAt, now)}
                </span>
              )}
              <button className="btn sm" onClick={onRefresh} disabled={refreshing}>
                {refreshing ? <span className="spin" /> : null}
                {refreshing ? "Refreshing…" : "Refresh"}
              </button>
            </div>
            {!snapshot ? (
              <div className="empty">
                <div className="big">📡</div>
                No usage data yet — hit Refresh.
              </div>
            ) : !sess && !weekly ? (
              <div className="empty">
                <div className="big">📡</div>
                This account reports no limit windows right now.
              </div>
            ) : (
              <div className="gauges" style={{ justifyContent: "space-around" }}>
                {sess && <Gauge bucket={sess} label="Session window" now={now} />}
                {weekly && <Gauge bucket={weekly} label="Weekly limit" now={now} />}
              </div>
            )}
          </div>

          {/* Limits + credits */}
          {snapshot && (
            <>
              <div className="card">
                <div className="card-head"><h3>LIMITS</h3></div>
                <div className="kv"><span className="k">System hard limit</span><span className="v">{snapshot.systemHardLimitUsd != null ? `$${snapshot.systemHardLimitUsd}` : "—"}</span></div>
                {snapshot.sessionSeconds && (
                  <div className="kv">
                    <span className="k">Session seconds</span>
                    <span className="v">
                      {snapshot.sessionSeconds.soft != null ? `${fmtTokens(snapshot.sessionSeconds.soft)} soft` : ""}
                      {snapshot.sessionSeconds.hard != null ? ` / ${fmtTokens(snapshot.sessionSeconds.hard)} hard` : ""}
                    </span>
                  </div>
                )}
                <div className="kv">
                  <span className="k">Subscription expires</span>
                  <span className="v">
                    {snapshot.subscriptionExpiresAt ? fmtDate(snapshot.subscriptionExpiresAt) : "—"}
                  </span>
                </div>
              </div>

              <div className="card">
                <div className="card-head"><h3>MANUAL RESET CREDITS</h3></div>
                {!credits ? (
                  <div className="muted" style={{ fontSize: 13 }}>No reset credits for this plan.</div>
                ) : credits.resets.length === 0 ? (
                  <div className="muted" style={{ fontSize: 13 }}>
                    <span className="badge ok">{credits.availableCount} available</span>{" "}
                    none expiring soon
                  </div>
                ) : (
                  <>
                    <div style={{ marginBottom: 8 }}>
                      <span className="badge ok">{credits.availableCount} available</span>
                    </div>
                    {credits.resets.map((r) => {
                      const days = (r.expiresAt - now) / 86400;
                      const cls = days < 3 ? "crit" : days < 10 ? "warn" : "ok";
                      return (
                        <div className="reset-chip" key={r.id}>
                          <span>expires {fmtDate(r.expiresAt)}</span>
                          <span className={`badge ${cls}`}>
                            {days < 0 ? "expired" : `${Math.ceil(days)}d left`}
                          </span>
                        </div>
                      );
                    })}
                  </>
                )}
              </div>
            </>
          )}

          {/* Usage stats */}
          {stats && (
            <div className="card full">
              <div className="card-head"><h3>USAGE STATS</h3></div>
              <div className="stat-grid">
                <Stat v={fmtTokens(stats.lifetimeTokens)} k="Lifetime tokens" />
                <Stat v={fmtTokens(stats.todayTokens)} k="Today" />
                <Stat v={fmtTokens(stats.last7Tokens)} k="Last 7 days" />
                <Stat v={fmtTokens(stats.last30Tokens)} k="Last 30 days" />
                <Stat v={`${stats.currentStreakDays}d`} k="Current streak" />
                <Stat v={`${stats.longestStreakDays}d`} k="Longest streak" />
                {stats.busiestDay && (
                  <Stat v={fmtTokens(stats.busiestDay.tokens)} k={`Busiest · ${stats.busiestDay.date.slice(5)}`} />
                )}
              </div>
              <div style={{ marginTop: 14 }}>
                <Sparkline daily={stats.daily} />
              </div>
              {stats.integrations.length > 0 && (
                <div style={{ marginTop: 12 }}>
                  <div className="card-head"><h3>TOP INTEGRATIONS</h3></div>
                  {stats.integrations.map((i) => (
                    <div className="integration-row" key={i.name}>
                      <span className="name">{i.name}</span>
                      <div className="bar">
                        <div
                          className="fill"
                          style={{
                            width: `${(i.tokens / Math.max(stats.integrations[0].tokens, 1)) * 100}%`,
                          }}
                        />
                      </div>
                      <span className="tokens">{fmtTokens(i.tokens)}</span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          {/* Warm-up */}
          <div className="card full">
            <div className="card-head">
              <h3>WARM-UP</h3>
              <div className="spacer" />
              <button className="btn sm" onClick={() => warmup([account.id])}>
                Warm up now
              </button>
            </div>
            <div className="sched-row">
              <span className="label">
                <b>Enabled</b>
                <div className="hint">allow scheduled warm-ups for this account</div>
              </span>
              <Switch checked={account.warmup.enabled} onChange={(v) => updateWarmup({ enabled: v })} />
            </div>
            <div className="sched-row">
              <span className="label">
                <b>After each reset window</b>
                <div className="hint">warm once whenever a limit window resets (skips empty weekly limits)</div>
              </span>
              <Switch checked={account.warmup.autoAfterReset} onChange={(v) => updateWarmup({ autoAfterReset: v })} />
            </div>
            <div className="sched-row">
              <span className="label">
                <b>Timed</b>
                <div className="hint">warm at fixed times of day</div>
              </span>
              <div className="time-chips">
                {account.warmup.timedAt.map((t) => (
                  <span
                    key={t}
                    className="time-chip"
                    title="click to remove"
                    onClick={() =>
                      updateWarmup({ timedAt: account.warmup.timedAt.filter((x) => x !== t) })
                    }
                  >
                    {t} ✕
                  </span>
                ))}
                <span className="time-add">
                  <input
                    type="time"
                    value={timeInput}
                    onChange={(e) => setTimeInput(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && addTime()}
                  />
                  <button className="btn sm" onClick={addTime}>Add</button>
                </span>
              </div>
            </div>
            <div className="hint" style={{ marginTop: 8 }}>
              Every warm-up request is recorded in the Activity tab — nothing happens silently.
            </div>
          </div>
        </>
      )}
    </div>
  );
}

function Stat({ v, k }: { v: string; k: string }) {
  return (
    <div className="stat">
      <div className="v">{v}</div>
      <div className="k">{k}</div>
    </div>
  );
}

export function Switch({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="switch">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <span className="track"><span className="thumb" /></span>
    </label>
  );
}

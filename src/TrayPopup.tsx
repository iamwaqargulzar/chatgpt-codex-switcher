import { useCallback, useEffect, useState } from "react";
import { EV, hidePopup, onEvent, openCodex, quit, refreshUsage, status, switchAccount, warmup, showMain } from "./lib/ipc";
import { fmtTokens, initials, pctOf } from "./lib/format";
import type { ProcessInfo, StatusBundle } from "./types";

/** Compact always-on-top panel next to the cursor. */
export default function TrayPopup() {
  const [bundle, setBundle] = useState<StatusBundle | null>(null);
  const [now, setNow] = useState(() => Date.now() / 1000);
  const [note, setNote] = useState<string | null>(null);

  const load = useCallback(() => {
    status().then(setBundle).catch(() => {});
  }, []);

  useEffect(() => {
    load();
    const t = setInterval(() => setNow(Date.now() / 1000), 1000);
    const unsubs = [
      onEvent(EV.popupOpened, () => load()),
      onEvent(EV.vaultChanged, () => load()),
      onEvent(EV.usageUpdated, () => load()),
      onEvent<ProcessInfo[]>(EV.switchBlocked, (procs) =>
        setNote(`${procs.length} codex process${procs.length > 1 ? "es" : ""} running`),
      ),
    ];
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") void hidePopup();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      clearInterval(t);
      unsubs.forEach((u) => void u.then((fn) => fn()));
      window.removeEventListener("keydown", onKey);
    };
  }, [load]);

  // Theme.
  useEffect(() => {
    const theme = bundle?.settings.theme ?? "dark";
    document.documentElement.dataset.theme =
      theme === "auto"
        ? window.matchMedia("(prefers-color-scheme: light)").matches
          ? "light"
          : "dark"
        : theme;
  }, [bundle?.settings.theme]);

  if (!bundle) {
    return (
      <div className="tray">
        <div className="empty" style={{ padding: 40 }}>
          <span className="spin" style={{ display: "inline-block" }} />
        </div>
      </div>
    );
  }

  const active = bundle.accounts.find((a) => a.id === bundle.activeAccountId) ?? null;
  const snap = active ? bundle.snapshots[active.id] : undefined;
  const sess = snap?.session ?? null;
  const weekly = snap?.weekly ?? null;
  const stats = snap?.stats ?? null;

  const doSwitch = async (id: string) => {
    try {
      const res = await switchAccount(id, false);
      if (res.switched) {
        setNote(null);
      } else {
        setNote(`${res.blocked.length} codex process${res.blocked.length > 1 ? "es" : ""} running — switch from the dashboard`);
      }
    } catch (e) {
      setNote(String(e));
    }
  };

  return (
    <div className="tray">
      <div className="tray-head">
        <div className={`avatar ${active?.kind ?? "chatgpt"}`} style={{ width: 34, height: 34 }}>
          {active ? initials(active.name) : "?"}
        </div>
        <div>
          <div className="name">{active ? active.name : "No active account"}</div>
          <div className="sub">
            {active?.plan ? `${active.plan} · ` : ""}
            {active?.kind === "apikey" ? "API key" : "ChatGPT"}
          </div>
        </div>
        <div className="spacer" />
        <button className="btn sm" onClick={() => void quit()} title="Quit CodexDesk">⏻</button>
      </div>

      {active && snap && (
        <>
          <div className="tray-bars">
            {sess && (
              <div className="row">
                <span className="lbl">5h</span>
                <div className="meter" style={{ flex: 1 }}>
                  <div
                    className={`fill ${pctOf(sess) < 10 ? "crit" : pctOf(sess) < 25 ? "warn" : ""}`}
                    style={{ width: `${pctOf(sess)}%` }}
                  />
                </div>
                <span className="val">
                  {Math.round(pctOf(sess))}% · {fmtTokens(sess.remainingTokens)}
                </span>
              </div>
            )}
            {weekly && (
              <div className="row">
                <span className="lbl">WK</span>
                <div className="meter" style={{ flex: 1 }}>
                  <div
                    className={`fill ${pctOf(weekly) < 10 ? "crit" : pctOf(weekly) < 25 ? "warn" : ""}`}
                    style={{ width: `${pctOf(weekly)}%` }}
                  />
                </div>
                <span className="val">
                  {Math.round(pctOf(weekly))}% · {fmtTokens(weekly.remainingTokens)}
                </span>
              </div>
            )}
            {(sess ?? weekly) && (
              <div className="acct-sub mono" style={{ fontSize: 11 }}>
                resets {fmtReset((sess ?? weekly)!.resetAt, now)}
                {snap.resetCredits ? ` · ${snap.resetCredits.availableCount} reset credits` : ""}
              </div>
            )}
          </div>

          {stats && (
            <div className="tray-stats">
              <div className="mini">
                <div className="v">{fmtTokens(stats.todayTokens)}</div>
                <div className="k">Today</div>
              </div>
              <div className="mini">
                <div className="v">{fmtTokens(stats.last7Tokens)}</div>
                <div className="k">7 days</div>
              </div>
              <div className="mini">
                <div className="v">{stats.currentStreakDays}d</div>
                <div className="k">Streak</div>
              </div>
            </div>
          )}
        </>
      )}

      {note && <div className="warning-strip" style={{ margin: "0 12px 8px" }}>⚠ {note}</div>}

      <div className="tray-accts">
        {bundle.accounts.map((a) => {
          const snap = bundle.snapshots[a.id];
          const s = snap?.session ?? snap?.weekly ?? null;
          return (
            <button
              key={a.id}
              className={`tray-acct ${a.active ? "on" : ""}`}
              onClick={() => void doSwitch(a.id)}
            >
              <div className={`avatar ${a.kind}`}>{initials(a.name)}</div>
              <span className="nm">{a.name}</span>
              {s && <span className="pct">{Math.round(pctOf(s))}%</span>}
              {a.active && <span className="badge ok" style={{ fontSize: 9 }}>ACTIVE</span>}
            </button>
          );
        })}
        {bundle.accounts.length === 0 && (
          <div className="empty" style={{ padding: 20 }}>
            No accounts — open the dashboard to add one.
          </div>
        )}
      </div>

      <div className="tray-foot">
        <button
          className="btn"
          title="Refresh active account usage"
          onClick={() => {
            if (active) void refreshUsage(active.id).then(load).catch(() => {});
          }}
        >
          ↻
        </button>
        <button className="btn" title="Warm up all" onClick={() => void warmup()}>🔥</button>
        <button
          className="btn"
          title="Open Codex in a terminal"
          onClick={() => void openCodex(active?.id).catch(() => {})}
        >
          ⌨
        </button>
        <button className="btn" title="Open dashboard" onClick={() => void showMain()}>⧉</button>
      </div>
    </div>
  );
}

function fmtReset(ts: number, now: number): string {
  const s = ts - now;
  if (s <= 0) return "now";
  const m = Math.floor(s / 60);
  if (m < 60) return `in ${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `in ${h}h ${m % 60}m`;
  return `in ${Math.floor(h / 24)}d`;
}

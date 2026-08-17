import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  EV,
  exportAccount,
  killCodex,
  onEvent,
  openCodex,
  oauthStart,
  refreshAll,
  refreshUsage,
  renameAccount,
  status,
  switchAccount,
  warmup,
} from "./lib/ipc";
import type { AuthDonePayload, ProcessInfo, StatusBundle, WarmupConfig } from "./types";
import { AccountRow } from "./components/AccountRow";
import { QuotaPanel } from "./components/QuotaPanel";
import { AddAccountModal } from "./components/AddAccountModal";
import { SettingsSheet } from "./components/SettingsSheet";
import { SessionsPanel } from "./components/SessionsPanel";
import { ProfilesPanel } from "./components/ProfilesPanel";
import { ActivityPanel } from "./components/ActivityPanel";

type Tab = "quota" | "sessions" | "profiles" | "activity";
type Filter = "all" | "chatgpt" | "apikey" | "active";

interface Toast {
  id: number;
  text: string;
  err: boolean;
}

export default function App() {
  const [bundle, setBundle] = useState<StatusBundle | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("quota");
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [addOpen, setAddOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [blocked, setBlocked] = useState<{ id: string; procs: ProcessInfo[] } | null>(null);
  const [renaming, setRenaming] = useState(false);
  const [renameValue, setRenameValue] = useState("");
  const [refreshing, setRefreshing] = useState(false);
  const [oauthStep, setOauthStep] = useState("");
  const [oauthError, setOauthError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now() / 1000);

  const searchRef = useRef<HTMLInputElement>(null);
  const toastId = useRef(0);
  const selectedIdRef = useRef<string | null>(null);
  selectedIdRef.current = selectedId;

  const toast = useCallback((text: string, err = false) => {
    const id = ++toastId.current;
    setToasts((t) => [...t, { id, text, err }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 4200);
  }, []);

  const load = useCallback(() => {
    status()
      .then((b) => {
        setBundle(b);
        setSelectedId((prev) =>
          prev && b.accounts.some((a) => a.id === prev)
            ? prev
            : b.activeAccountId ?? b.accounts[0]?.id ?? null,
        );
      })
      .catch((e) => toast(String(e), true));
  }, [toast]);

  // ---- lifecycle ----

  useEffect(() => {
    load();
    const t = setInterval(() => setNow(Date.now() / 1000), 1000);
    return () => clearInterval(t);
  }, [load]);

  useEffect(() => {
    const unsubs = [
      onEvent(EV.vaultChanged, () => load()),
      onEvent(EV.usageUpdated, () => load()),
      onEvent(EV.settingsChanged, () => load()),
      onEvent<string>(EV.toast, (msg) => toast(msg)),
      onEvent<ProcessInfo[]>(EV.switchBlocked, (procs) =>
        setBlocked({ id: selectedIdRef.current ?? "", procs }),
      ),
      onEvent<string>(EV.authProgress, (step) => {
        setOauthStep(step);
        setOauthError(null);
      }),
      onEvent<AuthDonePayload>(EV.authDone, (d) => {
        if (d.ok) {
          setOauthStep("");
          setOauthError(null);
          setAddOpen(false);
          toast(`added ${d.account?.name ?? "account"}`);
        } else {
          setOauthError(d.error ?? "login failed");
        }
      }),
    ];
    return () => unsubs.forEach((u) => void u.then((fn) => fn()));
  }, [load, toast]);

  // Theme + compact mode.
  useEffect(() => {
    const theme = bundle?.settings.theme ?? "dark";
    const apply = () => {
      const t =
        theme === "auto"
          ? window.matchMedia("(prefers-color-scheme: light)").matches
            ? "light"
            : "dark"
          : theme;
      document.documentElement.dataset.theme = t;
    };
    apply();
    if (theme === "auto") {
      const mq = window.matchMedia("(prefers-color-scheme: light)");
      mq.addEventListener("change", apply);
      return () => mq.removeEventListener("change", apply);
    }
  }, [bundle?.settings.theme]);

  // ---- derived ----

  const accounts = useMemo(() => {
    if (!bundle) return [];
    const q = query.trim().toLowerCase();
    return bundle.accounts.filter((a) => {
      if (filter === "chatgpt" && a.kind !== "chatgpt") return false;
      if (filter === "apikey" && a.kind !== "apikey") return false;
      if (filter === "active" && !a.active) return false;
      if (!q) return true;
      return [a.name, a.email, a.plan, a.profile].some((s) => s?.toLowerCase().includes(q));
    });
  }, [bundle, query, filter]);

  const selected = bundle?.accounts.find((a) => a.id === selectedId) ?? null;
  const selectedSnap = selected ? bundle!.snapshots[selected.id] : undefined;
  const runningProcs = bundle?.processes ?? [];

  // ---- actions ----

  const doSwitch = async (id: string, force = false) => {
    try {
      const res = await switchAccount(id, force);
      if (!res.switched) {
        setBlocked({ id, procs: res.blocked });
      } else {
        toast("account switched");
        load();
      }
    } catch (e) {
      toast(String(e), true);
    }
  };

  const handleOpenCodex = async () => {
    if (!selected) return;
    try {
      if (!selected.active) {
        const res = await switchAccount(selected.id, false);
        if (!res.switched) {
          setBlocked({ id: selected.id, procs: res.blocked });
          return;
        }
        toast("account switched");
      }
      await openCodex(selected.id);
    } catch (e) {
      toast(String(e), true);
    }
  };

  const handleRefresh = async () => {
    if (!selected) return;
    setRefreshing(true);
    try {
      await refreshUsage(selected.id);
      load();
    } catch (e) {
      toast(String(e), true);
    } finally {
      setRefreshing(false);
    }
  };

  const handleExport = async () => {
    if (!selected) return;
    try {
      const path = await exportAccount(selected.id);
      toast(`exported to ${path}`);
    } catch (e) {
      toast(String(e), true);
    }
  };

  const handleRename = async () => {
    if (!selected) return;
    const name = renameValue.trim();
    if (!name) return;
    try {
      await renameAccount(selected.id, name);
      setRenaming(false);
      toast("renamed");
      load();
    } catch (e) {
      toast(String(e), true);
    }
  };

  const warmAll = async () => {
    try {
      await warmup();
      toast("warm-up started — watch the Activity tab");
    } catch (e) {
      toast(String(e), true);
    }
  };

  // ---- keyboard ----

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const inField =
        e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      if (paletteOpen) return;
      if (e.key === "Escape") {
        setAddOpen(false);
        setSettingsOpen(false);
        setBlocked(null);
        setRenaming(false);
        return;
      }
      if (inField) return;
      if (e.key === "/") {
        e.preventDefault();
        searchRef.current?.focus();
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPaletteOpen(true);
        return;
      }
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        if (accounts.length === 0) return;
        const idx = accounts.findIndex((a) => a.id === selectedId);
        const next =
          e.key === "ArrowDown"
            ? accounts[Math.min(idx + 1, accounts.length - 1)]
            : accounts[Math.max(idx - 1, 0)];
        setSelectedId(next.id);
      }
      if (e.key === "Enter" && selected) {
        e.preventDefault();
        void doSwitch(selected.id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  if (!bundle) {
    return (
      <div className="app" style={{ placeItems: "center" }}>
        <div className="empty">
          <span className="spin" style={{ display: "inline-block" }} />
          <div style={{ marginTop: 10 }}>loading CodexDesk…</div>
        </div>
      </div>
    );
  }

  return (
    <div className={`app ${bundle.settings.compact ? "compact" : ""}`}>
      {/* ---------- sidebar ---------- */}
      <aside className="sidebar">
        <div className="brand">
          <div className="logo">⧉</div>
          <div>
            <div className="title">CodexDesk</div>
            <div className="sub">
              {bundle.codexVersion ? `codex ${bundle.codexVersion}` : "codex CLI manager"}
            </div>
          </div>
        </div>

        <div className="searchbox">
          <span className="kbd">/</span>
          <input
            ref={searchRef}
            placeholder="Search accounts…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>

        <div className="chips">
          {(
            [
              ["all", "All"],
              ["chatgpt", "ChatGPT"],
              ["apikey", "API keys"],
              ["active", "Active"],
            ] as [Filter, string][]
          ).map(([f, label]) => (
            <button
              key={f}
              className={`chip ${filter === f ? "on" : ""}`}
              onClick={() => setFilter(f)}
            >
              {label}
            </button>
          ))}
        </div>

        <div className="acct-list">
          {accounts.map((a) => (
            <AccountRow
              key={a.id}
              account={a}
              snapshot={bundle.snapshots[a.id]}
              selected={a.id === selectedId}
              onClick={() => setSelectedId(a.id)}
            />
          ))}
          {accounts.length === 0 && (
            <div className="empty">
              <div className="big">🗂</div>
              No accounts yet — add one below.
            </div>
          )}
        </div>

        <div className="sidebar-foot">
          <button className="btn primary" style={{ flex: 1 }} onClick={() => setAddOpen(true)}>
            + Add account
          </button>
          <button
            className="btn"
            title="Refresh usage for every account"
            onClick={() => void refreshAll().then(load).catch((e) => toast(String(e), true))}
          >
            ↻
          </button>
        </div>
      </aside>

      {/* ---------- main ---------- */}
      <main className="main">
        <div className="topbar">
          <div className="row">
            <span className="muted" style={{ fontSize: 12 }}>ACTIVE:</span>
            <b>
              {bundle.accounts.find((a) => a.id === bundle.activeAccountId)?.name ??
                "none — switch to an account"}
            </b>
          </div>
          {runningProcs.length > 0 && (
            <span className="badge warn" title={runningProcs.map((p) => p.cmdline).join("\n")}>
              {runningProcs.length} codex process{runningProcs.length > 1 ? "es" : ""} running
            </span>
          )}
          <div className="spacer" />
          <button className="btn" onClick={warmAll}>🔥 Warm up all</button>
          <button className="btn" onClick={() => setSettingsOpen(true)}>⚙ Settings</button>
        </div>

        <div className="tabs">
          {(
            [
              ["quota", "Quota"],
              ["sessions", "Sessions"],
              ["profiles", "Profiles"],
              ["activity", "Activity"],
            ] as [Tab, string][]
          ).map(([t, label]) => (
            <button key={t} className={`tab ${tab === t ? "on" : ""}`} onClick={() => setTab(t)}>
              {label}
            </button>
          ))}
        </div>

        <div className="scroll-area">
          {tab === "quota" &&
            (selected ? (
              <QuotaPanel
                account={selected}
                snapshot={selectedSnap}
                now={now}
                refreshing={refreshing}
                onRefresh={handleRefresh}
                onOpenCodex={handleOpenCodex}
                onRename={() => {
                  setRenameValue(selected.name);
                  setRenaming(true);
                }}
                onExport={handleExport}
                onWarmupSaved={(_w: WarmupConfig) => load()}
              />
            ) : (
              <div className="empty">
                <div className="big">👈</div>
                Select or add an account.
              </div>
            ))}

          {tab === "sessions" && <SessionsPanel active={tab === "sessions"} />}
          {tab === "profiles" && (
            <ProfilesPanel
              active={tab === "profiles"}
              accountId={selected?.id ?? null}
              currentProfile={selected?.profile ?? null}
              onAssigned={() => load()}
            />
          )}
          {tab === "activity" && <ActivityPanel active={tab === "activity"} />}
        </div>
      </main>

      {/* ---------- overlays ---------- */}
      <AddAccountModal
        open={addOpen}
        oauthStep={oauthStep}
        oauthError={oauthError}
        onClose={() => setAddOpen(false)}
        onOAuthStart={() => {
          setOauthError(null);
          setOauthStep("opening browser");
          oauthStart().catch((e) => setOauthError(String(e)));
        }}
      />

      <SettingsSheet
        open={settingsOpen}
        status={bundle}
        onClose={() => setSettingsOpen(false)}
        onSaved={() => load()}
      />

      {renaming && selected && (
        <div className="modal-back" onClick={() => setRenaming(false)}>
          <div className="modal" style={{ width: 380 }} onClick={(e) => e.stopPropagation()}>
            <h3>Rename account</h3>
            <input
              className="input"
              autoFocus
              value={renameValue}
              onChange={(e) => setRenameValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void handleRename();
                if (e.key === "Escape") setRenaming(false);
              }}
            />
            <div className="modal-foot">
              <button className="btn" onClick={() => setRenaming(false)}>Cancel</button>
              <button className="btn primary" onClick={handleRename}>Save</button>
            </div>
          </div>
        </div>
      )}

      {blocked && (
        <BlockedModal
          procs={blocked.procs}
          onCancel={() => setBlocked(null)}
          onSwitchAnyway={() => {
            setBlocked(null);
            void doSwitch(blocked.id, true);
          }}
          onKillAndSwitch={async () => {
            const procs = blocked.procs;
            setBlocked(null);
            for (const p of procs) {
              try {
                await killCodex(p.pid);
              } catch {
                /* keep going */
              }
            }
            void doSwitch(blocked.id, true);
          }}
        />
      )}

      {paletteOpen && (
        <Palette
          bundle={bundle}
          selectedId={selectedId}
          onClose={() => setPaletteOpen(false)}
          onSwitch={(id) => {
            setPaletteOpen(false);
            void doSwitch(id);
          }}
          onAdd={() => {
            setPaletteOpen(false);
            setAddOpen(true);
          }}
          onCommand={(fn) => {
            setPaletteOpen(false);
            fn();
          }}
        />
      )}

      <div className="toasts">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.err ? "err" : ""}`}>
            {t.text}
          </div>
        ))}
      </div>
    </div>
  );
}

// ---------- blocked-switch modal ----------

function BlockedModal({
  procs,
  onCancel,
  onSwitchAnyway,
  onKillAndSwitch,
}: {
  procs: ProcessInfo[];
  onCancel: () => void;
  onSwitchAnyway: () => void;
  onKillAndSwitch: () => void;
}) {
  return (
    <div className="modal-back">
      <div className="modal" style={{ width: 460 }}>
        <h3>Codex is still running</h3>
        <div className="hint" style={{ marginBottom: 12 }}>
          A running Codex session keeps using the old account's credentials. Close the
          session(s) below — or let CodexDesk terminate them before switching.
        </div>
        {procs.map((p) => (
          <div className="reset-chip" key={p.pid}>
            <span className="mono" style={{ fontSize: 12 }}>
              pid {p.pid}
            </span>
            <span className="muted" style={{ fontSize: 12, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", flex: 1, marginLeft: 10 }}>
              {p.cmdline}
            </span>
          </div>
        ))}
        <div className="modal-foot">
          <button className="btn" onClick={onCancel}>Cancel</button>
          <button className="btn" onClick={onSwitchAnyway}>Switch anyway</button>
          <button className="btn danger" onClick={onKillAndSwitch}>
            Close all & switch
          </button>
        </div>
      </div>
    </div>
  );
}

// ---------- command palette ----------

function Palette({
  bundle,
  selectedId,
  onClose,
  onSwitch,
  onAdd,
  onCommand,
}: {
  bundle: StatusBundle;
  selectedId: string | null;
  onClose: () => void;
  onSwitch: (id: string) => void;
  onAdd: () => void;
  onCommand: (fn: () => void) => void;
}) {
  const [q, setQ] = useState("");
  const [idx, setIdx] = useState(0);

  interface Item {
    label: string;
    hint: string;
    run: () => void;
  }

  const items: Item[] = [
    ...bundle.accounts.map((a) => ({
      label: `Switch to ${a.name}`,
      hint: a.active ? "active" : "",
      run: () => onSwitch(a.id),
    })),
    { label: "Warm up all accounts", hint: "", run: () => void warmup() },
    {
      label: "Refresh usage",
      hint: "",
      run: () => {
        if (selectedId) void refreshUsage(selectedId);
      },
    },
    { label: "Add account", hint: "", run: onAdd },
  ].filter((i) => i.label.toLowerCase().includes(q.toLowerCase()));

  useEffect(() => {
    setIdx(0);
  }, [q]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setIdx((i) => Math.min(i + 1, items.length - 1));
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setIdx((i) => Math.max(i - 1, 0));
      }
      if (e.key === "Enter" && items[idx]) {
        e.preventDefault();
        onCommand(items[idx].run);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  return (
    <div className="modal-back" style={{ background: "rgba(0,0,0,0.35)" }} onClick={onClose}>
      <div className="palette" onClick={(e) => e.stopPropagation()}>
        <input
          autoFocus
          placeholder="Type a command…"
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
        {items.map((i, n) => (
          <div
            key={i.label}
            className={`palette-row ${n === idx ? "on" : ""}`}
            onMouseEnter={() => setIdx(n)}
            onClick={() => onCommand(i.run)}
          >
            <span>{i.label}</span>
            {i.hint && <span className="cmd">{i.hint}</span>}
          </div>
        ))}
        {items.length === 0 && <div className="empty">no matches</div>}
      </div>
    </div>
  );
}

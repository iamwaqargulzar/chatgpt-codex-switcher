import { useState } from "react";
import type { Settings, StatusBundle } from "../types";
import { quit, restoreBackup, revealVault, saveSettings } from "../lib/ipc";
import { Switch } from "./QuotaPanel";

export function SettingsSheet({
  open,
  status,
  onClose,
  onSaved,
}: {
  open: boolean;
  status: StatusBundle;
  onClose: () => void;
  onSaved: (s: Settings) => void;
}) {
  const [s, setS] = useState<Settings>(status.settings);
  const [busy, setBusy] = useState(false);

  if (!open) return null;

  const patch = (p: Partial<Settings>) => setS((prev) => ({ ...prev, ...p }));

  const save = async () => {
    setBusy(true);
    try {
      const saved = await saveSettings(s);
      onSaved(saved);
      onClose();
    } catch (e) {
      alert(String(e));
    } finally {
      setBusy(false);
    }
  };

  const row = (title: string, desc: string, control: React.ReactNode) => (
    <div className="setting-row">
      <div className="label">
        <div className="t">{title}</div>
        <div className="d">{desc}</div>
      </div>
      {control}
    </div>
  );

  return (
    <div className="modal-back" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal">
        <h3>Settings</h3>

        {row("Theme", "dark, light, or follow the system", (
          <select className="select" style={{ width: 150 }} value={s.theme} onChange={(e) => patch({ theme: e.target.value })}>
            <option value="dark">Dark</option>
            <option value="light">Light</option>
            <option value="auto">Auto</option>
          </select>
        ))}

        {row("Tray style", "how the tray indicator appears", (
          <select className="select" style={{ width: 150 }} value={s.trayMode} onChange={(e) => patch({ trayMode: e.target.value })}>
            <option value="icon">Icon + menu</option>
            <option value="text">Text</option>
            <option value="hidden">Hidden</option>
          </select>
        ))}

        {row("Global hotkey", "pops the quick panel (X11 only)", (
          <input
            className="input"
            style={{ width: 150 }}
            value={s.hotkey ?? ""}
            placeholder="ctrl+alt+d"
            onChange={(e) => patch({ hotkey: e.target.value || null })}
          />
        ))}

        {row("Desktop notifications", "low-limit and window-reset alerts", (
          <Switch checked={s.notifications} onChange={(v) => patch({ notifications: v })} />
        ))}

        {row("Launch at login", "start CodexDesk with your session", (
          <Switch checked={s.launchAtLogin} onChange={(v) => patch({ launchAtLogin: v })} />
        ))}

        {row("Compact mode", "denser layout for small screens", (
          <Switch checked={s.compact} onChange={(v) => patch({ compact: v })} />
        ))}

        {row("Usage refresh interval", "seconds between background quota refreshes", (
          <input
            className="input mono"
            style={{ width: 90 }}
            type="number"
            min={30}
            value={s.refreshIntervalSecs}
            onChange={(e) => patch({ refreshIntervalSecs: Number(e.target.value) || 300 })}
          />
        ))}

        {row("Terminal", "used for Open Codex and session resume", (
          <select className="select" style={{ width: 190 }} value={s.terminalPreset} onChange={(e) => patch({ terminalPreset: e.target.value })}>
            <option value="auto">Auto-detect</option>
            {status.terminalPresets.map(([id, name, found]) => (
              <option key={id} value={id}>
                {name}
                {found ? "" : " (not installed)"}
              </option>
            ))}
          </select>
        ))}

        {row("Custom terminal command", "overrides the preset; gets $CODEXDESK_TITLE and $CODEXDESK_COMMAND", (
          <input
            className="input mono"
            style={{ width: 260 }}
            placeholder="e.g. /path/to/term -e $CODEXDESK_COMMAND"
            value={s.customTerminal ?? ""}
            onChange={(e) => patch({ customTerminal: e.target.value || null })}
          />
        ))}

        <div style={{ marginTop: 16 }} className="danger-zone">
          <div style={{ fontWeight: 700, marginBottom: 8 }}>Vault & recovery</div>
          <div className="hint" style={{ marginBottom: 10 }}>
            {status.vaultEncrypted
              ? "🔒 Vault is encrypted with your OS keyring (AES-256-GCM)."
              : "⚠ No OS keyring found — the vault is stored unencrypted with 0600 permissions."}
          </div>
          <div className="row" style={{ flexWrap: "wrap", gap: 8 }}>
            <button className="btn sm" onClick={() => void revealVault()}>Reveal vault folder</button>
            <button
              className="btn sm"
              onClick={async () => {
                const ok = await restoreBackup();
                alert(ok ? "Previous auth.json restored." : "No backup to restore.");
              }}
            >
              Restore previous auth.json
            </button>
            <button className="btn sm danger" onClick={() => void quit()}>Quit CodexDesk</button>
          </div>
        </div>

        <div className="modal-foot">
          <button className="btn" onClick={onClose}>Cancel</button>
          <button className="btn primary" onClick={save} disabled={busy}>
            {busy ? <span className="spin" /> : null}
            Save
          </button>
        </div>
      </div>
    </div>
  );
}

import { useState } from "react";
import { importApiKey, importAuth } from "../lib/ipc";

export type AddTab = "oauth" | "import" | "apikey";

export function AddAccountModal({
  open,
  oauthStep,
  oauthError,
  onClose,
  onOAuthStart,
}: {
  open: boolean;
  oauthStep: string;
  oauthError: string | null;
  onClose: () => void;
  onOAuthStart: () => void;
}) {
  const [tab, setTab] = useState<AddTab>("oauth");
  const [key, setKey] = useState("");
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!open) return null;

  const doImport = async () => {
    setBusy(true);
    setError(null);
    try {
      await importAuth();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const doApiKey = async () => {
    setBusy(true);
    setError(null);
    try {
      await importApiKey(key, name || undefined);
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modal-back" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal">
        <h3>Add an account</h3>
        <div className="seg">
          <button className={tab === "oauth" ? "on" : ""} onClick={() => setTab("oauth")}>
            ChatGPT login
          </button>
          <button className={tab === "import" ? "on" : ""} onClick={() => setTab("import")}>
            Import auth.json
          </button>
          <button className={tab === "apikey" ? "on" : ""} onClick={() => setTab("apikey")}>
            API key
          </button>
        </div>

        {tab === "oauth" && (
          <div className="oauth-zone">
            <div className="big">🔐</div>
            <div className="hint">
              Opens your browser to the official ChatGPT sign-in page. Approve it, and the
              account lands in your vault automatically.
            </div>
            {oauthStep ? (
              <div className="oauth-steps">
                <div>✓ browser opened</div>
                {oauthStep !== "opening browser" && <div>✓ {oauthStep}</div>}
                <div className="muted">
                  {oauthError ? (
                    <span style={{ color: "var(--red)" }}>{oauthError}</span>
                  ) : (
                    "waiting for the callback…"
                  )}
                </div>
              </div>
            ) : (
              <div style={{ marginTop: 16 }}>
                <button className="btn primary" onClick={onOAuthStart}>
                  Sign in with ChatGPT
                </button>
              </div>
            )}
            {oauthStep && !oauthError && (
              <div style={{ marginTop: 10 }}>
                <span className="spin" style={{ display: "inline-block" }} />
              </div>
            )}
          </div>
        )}

        {tab === "import" && (
          <>
            <div className="hint" style={{ marginBottom: 14 }}>
              Pick an <code>auth.json</code> — the file Codex writes in{" "}
              <code>~/.codex/auth.json</code>. CodexDesk copies its credentials into the vault;
              the original file is left untouched.
            </div>
            <button className="btn" onClick={doImport} disabled={busy}>
              {busy ? <span className="spin" /> : null}
              Choose auth.json…
            </button>
          </>
        )}

        {tab === "apikey" && (
          <>
            <div className="field">
              <label>OpenAI API key</label>
              <input
                className="input"
                type="password"
                placeholder="sk-…"
                value={key}
                onChange={(e) => setKey(e.target.value)}
              />
            </div>
            <div className="field">
              <label>Name (optional)</label>
              <input
                className="input"
                placeholder="e.g. work key"
                value={name}
                onChange={(e) => setName(e.target.value)}
              />
            </div>
            <button className="btn primary" onClick={doApiKey} disabled={busy || !key.trim()}>
              {busy ? <span className="spin" /> : null}
              Add API key
            </button>
          </>
        )}

        {error && (
          <div className="warning-strip" style={{ marginTop: 14 }}>⚠ {error}</div>
        )}

        <div className="modal-foot">
          <button className="btn" onClick={onClose}>Close</button>
        </div>
      </div>
    </div>
  );
}

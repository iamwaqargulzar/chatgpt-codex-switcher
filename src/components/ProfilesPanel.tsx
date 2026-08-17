import { useEffect, useState } from "react";
import { profileCreate, profileRead, profileWrite, profiles, setProfile } from "../lib/ipc";
import type { ProfileInfo } from "../types";

export function ProfilesPanel({
  active,
  accountId,
  currentProfile,
  onAssigned,
}: {
  active: boolean;
  accountId: string | null;
  currentProfile: string | null;
  onAssigned: (name: string | null) => void;
}) {
  const [list, setList] = useState<ProfileInfo[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [content, setContent] = useState<string>("");
  const [newName, setNewName] = useState("");
  const [dirty, setDirty] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const load = () => {
    profiles().then(setList).catch((e) => setError(String(e)));
  };

  useEffect(() => {
    if (active && list === null) load();
  }, [active, list]);

  useEffect(() => {
    if (!selected) return;
    profileRead(selected)
      .then((p) => setContent(p.content ?? ""))
      .catch((e) => setError(String(e)));
    setDirty(false);
    setSaved(false);
  }, [selected]);

  if (!active) return null;

  const create = async () => {
    const name = newName.trim();
    if (!name) return;
    try {
      await profileCreate(name);
      setNewName("");
      load();
      setSelected(name);
    } catch (e) {
      setError(String(e));
    }
  };

  const save = async () => {
    if (!selected) return;
    try {
      await profileWrite(selected, content);
      setDirty(false);
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setError(String(e));
    }
  };

  const assign = async (name: string | null) => {
    if (!accountId) return;
    try {
      await setProfile(accountId, name);
      onAssigned(name);
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="detail-grid">
      {error && (
        <div className="warning-strip" style={{ gridColumn: "1 / -1" }}>⚠ {error}</div>
      )}
      <div className="card">
        <div className="card-head">
          <h3>PROFILES</h3>
        </div>
        <div className="hint" style={{ marginBottom: 10 }}>
          A profile is <code>~/.codex/&lt;name&gt;.config.toml</code>, layered over your base
          config when Codex runs with <code>--profile &lt;name&gt;</code>.
        </div>
        {(list ?? []).map((p) => (
          <div
            key={p.name}
            className={`profile-row ${selected === p.name ? "on" : ""}`}
            onClick={() => setSelected(p.name)}
          >
            <span className="nm">{p.name}</span>
            {p.isBase && <span className="badge kind">base</span>}
            <div className="spacer" />
            {accountId && currentProfile === p.name && <span className="badge ok">this account</span>}
            {accountId && currentProfile !== p.name && (
              <button
                className="btn sm"
                onClick={(e) => {
                  e.stopPropagation();
                  void assign(p.name);
                }}
              >
                Use for this account
              </button>
            )}
          </div>
        ))}
        <div className="row" style={{ marginTop: 10 }}>
          <input
            className="input mono"
            placeholder="new-profile-name"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && create()}
          />
          <button className="btn sm" onClick={create}>Create</button>
        </div>
        {currentProfile && accountId && (
          <div className="row" style={{ marginTop: 8 }}>
            <button className="btn sm" onClick={() => void assign(null)}>
              Detach from this account
            </button>
          </div>
        )}
      </div>

      <div className="card">
        <div className="card-head">
          <h3>{selected ? `${selected}.config.toml` : "EDITOR"}</h3>
          <div className="spacer" />
          {saved && <span className="badge ok">saved</span>}
          {dirty && <span className="badge warn">unsaved</span>}
          <button className="btn sm primary" onClick={save} disabled={!selected || !dirty}>
            Save
          </button>
        </div>
        {!selected ? (
          <div className="empty">
            <div className="big">⚙️</div>
            Pick a profile to edit its TOML.
          </div>
        ) : (
          <>
            <textarea
              className="textarea"
              spellCheck={false}
              value={content}
              onChange={(e) => {
                setContent(e.target.value);
                setDirty(true);
              }}
            />
            <div className="hint" style={{ marginTop: 8 }}>
              Valid TOML only — invalid content is rejected before it touches your config.
            </div>
          </>
        )}
      </div>
    </div>
  );
}

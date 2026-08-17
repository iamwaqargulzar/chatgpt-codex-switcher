import { fmtTokens, initials } from "../lib/format";
import type { AccountPublic, UsageSnapshot } from "../types";
import { MiniBar } from "./Gauge";

export function AccountRow({
  account,
  snapshot,
  selected,
  onClick,
}: {
  account: AccountPublic;
  snapshot: UsageSnapshot | undefined;
  selected: boolean;
  onClick: () => void;
}) {
  const sess = snapshot?.session ?? null;
  return (
    <div
      className={`acct-row ${selected ? "selected" : ""} ${account.active ? "active-row" : ""}`}
      onClick={onClick}
    >
      <div className={`avatar ${account.kind}`}>{initials(account.name)}</div>
      <div className="acct-main">
        <div className="acct-name">{account.name}</div>
        <div className="acct-sub">
          {account.plan ?? account.kind}
          {account.profile ? ` · ${account.profile}` : ""}
        </div>
      </div>
      <div className="acct-side">
        {sess && <MiniBar bucket={sess} />}
        {sess && (
          <div className="acct-sub mono" style={{ marginTop: 3 }}>
            {sess.unit === "pct"
              ? `${Math.round(sess.remainingTokens)}%`
              : fmtTokens(sess.remainingTokens)}
          </div>
        )}
      </div>
    </div>
  );
}

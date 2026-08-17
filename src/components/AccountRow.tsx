import { initials, pctOf } from "../lib/format";
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
  // Session window when present, otherwise the weekly limit — the sidebar
  // always shows the account's most pressing window.
  const bucket = snapshot?.session ?? snapshot?.weekly ?? null;
  const isWeekly = !snapshot?.session && !!snapshot?.weekly;
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
        {bucket && <MiniBar bucket={bucket} />}
        {bucket && (
          <div className="acct-sub mono" style={{ marginTop: 3 }}>
            {Math.round(pctOf(bucket))}% {isWeekly ? "wk" : "5h"}
          </div>
        )}
      </div>
    </div>
  );
}

import { countdownTo, fmtTokens, pctOf } from "../lib/format";
import type { RateBucket } from "../types";

/** Ring gauge for a rate-limit window, with live reset countdown. */
export function Gauge({
  bucket,
  label,
  now,
}: {
  bucket: RateBucket | null;
  label: string;
  now: number;
}) {
  const R = 56;
  const C = 2 * Math.PI * R;
  const pct = bucket ? pctOf(bucket) : 0;
  const tone = pct < 10 ? "var(--red)" : pct < 25 ? "var(--amber)" : "var(--accent)";
  const late = bucket && now >= bucket.resetAt;

  return (
    <div className="gauge">
      <div className="ring-wrap">
        <svg width="138" height="138" viewBox="0 0 138 138">
          <circle cx="69" cy="69" r={R} fill="none" stroke="var(--panel2)" strokeWidth="11" />
          <circle
            cx="69"
            cy="69"
            r={R}
            fill="none"
            stroke={tone}
            strokeWidth="11"
            strokeLinecap="round"
            strokeDasharray={`${(pct / 100) * C} ${C}`}
            transform="rotate(-90 69 69)"
            style={{ transition: "stroke-dasharray 0.5s ease" }}
          />
        </svg>
        <div className="ring-pct">
          {bucket ? `${Math.round(pct)}%` : "—"}
        </div>
        <div className="ring-sub">
          {bucket
            ? bucket.unit === "pct"
              ? `${Math.round(bucket.remainingTokens)}% left`
              : `${fmtTokens(bucket.remainingTokens)} left`
            : "no data"}
        </div>
      </div>
      <div className="gauge-label">{label}</div>
      {bucket && (
        <div className={`countdown ${late ? "late" : ""}`}>
          {countdownTo(bucket.resetAt, now)}
        </div>
      )}
      {bucket && (
        <div className="used">
          {bucket.unit === "pct"
            ? `${Math.round(bucket.usedTokens)}% used`
            : `${fmtTokens(bucket.usedTokens)} / ${fmtTokens(bucket.maximumTokens)}`}
        </div>
      )}
    </div>
  );
}

/** Small horizontal usage bar used in lists. */
export function MiniBar({ bucket }: { bucket: RateBucket | null }) {
  const pct = bucket ? pctOf(bucket) : 0;
  const cls = pct < 10 ? "crit" : pct < 25 ? "warn" : "";
  return (
    <div className="mini-bar">
      <div className={`fill ${cls}`} style={{ width: `${pct}%` }} />
    </div>
  );
}

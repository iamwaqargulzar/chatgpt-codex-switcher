import { useState } from "react";
import type { DayPoint } from "../types";
import { fmtDay, fmtTokens } from "../lib/format";

/** Minimal SVG area chart for daily token activity (last 30 days). */
export function Sparkline({ daily }: { daily: DayPoint[] }) {
  const [hover, setHover] = useState<number | null>(null);

  if (daily.length < 2) {
    return <div className="empty" style={{ padding: 12 }}>not enough data yet</div>;
  }
  const W = 600;
  const H = 64;
  const P = 6;
  const max = Math.max(...daily.map((d) => d.tokens), 1);
  const pts = daily.map((d, i) => {
    const x = P + (i / (daily.length - 1)) * (W - 2 * P);
    const y = H - P - (d.tokens / max) * (H - 2 * P);
    return [x, y] as const;
  });
  const line = pts.map(([x, y], i) => `${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`).join(" ");
  const area = `${line} L${pts[pts.length - 1][0].toFixed(1)},${H - P} L${pts[0][0].toFixed(1)},${H - P} Z`;

  return (
    <div style={{ position: "relative" }}>
      <svg
        className="sparkline"
        viewBox={`0 0 ${W} ${H}`}
        preserveAspectRatio="none"
        onMouseLeave={() => setHover(null)}
        onMouseMove={(e) => {
          const rect = e.currentTarget.getBoundingClientRect();
          const frac = (e.clientX - rect.left) / rect.width;
          setHover(Math.round(frac * (daily.length - 1)));
        }}
      >
        <path className="area" d={area} />
        <path d={line} />
        {hover !== null && hover < daily.length && (
          <circle
            cx={pts[hover][0]}
            cy={pts[hover][1]}
            r="4"
            fill="var(--accent)"
            vectorEffect="non-scaling-stroke"
          />
        )}
      </svg>
      {hover !== null && hover < daily.length && (
        <div className="acct-sub mono" style={{ position: "absolute", top: 0, right: 0 }}>
          {fmtDay(daily[hover].date)} · {fmtTokens(daily[hover].tokens)} tokens
        </div>
      )}
    </div>
  );
}

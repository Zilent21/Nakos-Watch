import { useEffect, useState } from "react";
import { cn } from "@/lib/utils";

/**
 * Score ring. The one place (besides CTAs and focus) that uses Bitcoin
 * orange. On first paint the arc sweeps from empty to the score while the
 * ring spins into place; with reduced motion it just renders.
 */
export function ScoreRing({
  score,
  label,
  size = 96,
  className,
}: {
  /** 0-100, or null when there is nothing real to show. */
  score: number | null;
  label: string;
  size?: number;
  className?: string;
}) {
  const stroke = 8;
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const target = score === null ? 0 : Math.max(0, Math.min(100, score));
  const [shown, setShown] = useState(0);

  useEffect(() => {
    const id = requestAnimationFrame(() => setShown(target));
    return () => cancelAnimationFrame(id);
  }, [target]);

  return (
    <div className={cn("relative inline-flex items-center justify-center", className)} style={{ width: size, height: size }} role="img" aria-label={`${label}: ${score === null ? "no data" : score + " out of 100"}`}>
      <svg width={size} height={size} className="ring-spin-in">
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="hsl(var(--secondary))" strokeWidth={stroke} />
        <circle
          cx={size / 2}
          cy={size / 2}
          r={r}
          fill="none"
          stroke="hsl(var(--bitcoin))"
          strokeWidth={stroke}
          strokeLinecap="round"
          strokeDasharray={c}
          strokeDashoffset={c - (c * shown) / 100}
          style={{ transition: "stroke-dashoffset 900ms cubic-bezier(0.22, 1, 0.36, 1)" }}
        />
      </svg>
      <span className="mono-nums absolute text-xl font-semibold">{score === null ? "—" : Math.round(score)}</span>
    </div>
  );
}

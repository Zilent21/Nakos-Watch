import { useQuery } from "@tanstack/react-query";
import * as api from "@/bindings/commands";
import { cn } from "@/lib/utils";

/**
 * A QR code for a bitcoin: link. Rust makes the modules (`payment_qr`); this only draws them, so
 * no JavaScript QR library and no outside service ever sees the address. Always black on white,
 * with a quiet zone, whatever the theme: scanners need the contrast.
 */
export function QrCode({ text, size = 224, className }: { text: string; size?: number; className?: string }) {
  const { data, error } = useQuery({ queryKey: ["payment-qr", text], queryFn: () => api.paymentQr(text), staleTime: Infinity });
  if (error) return <p className="text-sm text-destructive">Couldn't draw the QR code: {String(error)}</p>;
  if (!data) return <div className={cn("animate-pulse rounded-md bg-secondary", className)} style={{ width: size, height: size }} aria-hidden />;
  const quiet = 4;
  const n = data.size + quiet * 2;
  let d = "";
  for (let y = 0; y < data.size; y++) {
    for (let x = 0; x < data.size; x++) {
      if (data.modules[y * data.size + x] === "1") d += `M${x + quiet} ${y + quiet}h1v1h-1z`;
    }
  }
  return (
    <svg
      viewBox={`0 0 ${n} ${n}`}
      width={size}
      height={size}
      shapeRendering="crispEdges"
      role="img"
      aria-label="QR code of the payment link"
      className={cn("rounded-md", className)}
    >
      <rect width={n} height={n} fill="#ffffff" />
      <path d={d} fill="#000000" />
    </svg>
  );
}

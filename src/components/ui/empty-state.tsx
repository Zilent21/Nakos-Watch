import type { ComponentType, ReactNode } from "react";
import { cn } from "@/lib/utils";

/** One sentence, one primary action, an optional one-line helper. */
export function EmptyState({
  icon: Icon,
  title,
  helper,
  action,
  className,
}: {
  icon?: ComponentType<{ className?: string }>;
  title: string;
  helper?: string;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex flex-col items-center justify-center gap-3 px-6 py-16 text-center", className)}>
      {Icon && (
        <div className="flex h-12 w-12 items-center justify-center rounded-lg border border-border bg-secondary/50">
          <Icon className="h-5 w-5 text-muted-foreground" />
        </div>
      )}
      <p className="text-base font-medium">{title}</p>
      {helper && <p className="max-w-sm text-sm text-muted-foreground">{helper}</p>}
      {action && <div className="pt-2">{action}</div>}
    </div>
  );
}

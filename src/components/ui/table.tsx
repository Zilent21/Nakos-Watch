import * as React from "react";
import { cn } from "@/lib/utils";

/**
 * Quiet data table: hairline rows, 12px muted headers, 16px cell padding,
 * 24px at the outer edges to line up with card content. Cells don't wrap;
 * the wrapper scrolls horizontally if the window is very narrow.
 */
const Table = ({ className, ...props }: React.TableHTMLAttributes<HTMLTableElement>) => (
  <div className="overflow-x-auto">
    <table className={cn("w-full text-sm", className)} {...props} />
  </div>
);
const THead = (props: React.HTMLAttributes<HTMLTableSectionElement>) => <thead {...props} />;
const TBody = (props: React.HTMLAttributes<HTMLTableSectionElement>) => <tbody {...props} />;
const TR = ({ className, ...props }: React.HTMLAttributes<HTMLTableRowElement>) => (
  <tr className={cn("border-b border-border/60 last:border-0 transition-colors hover:bg-accent/40", className)} {...props} />
);
const TH = ({ className, numeric, ...props }: React.ThHTMLAttributes<HTMLTableCellElement> & { numeric?: boolean }) => (
  <th
    className={cn("whitespace-nowrap border-b border-border px-4 py-3 text-xs font-medium text-muted-foreground first:pl-6 last:pr-6", numeric ? "text-right" : "text-left", className)}
    {...props}
  />
);
const TD = ({ className, numeric, ...props }: React.TdHTMLAttributes<HTMLTableCellElement> & { numeric?: boolean }) => (
  <td className={cn("whitespace-nowrap px-4 py-4 align-middle first:pl-6 last:pr-6", numeric && "mono-nums text-right", className)} {...props} />
);

export { Table, THead, TBody, TR, TH, TD };

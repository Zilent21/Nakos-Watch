import { useState } from "react";
import { NavLink, useLocation, useNavigate } from "react-router-dom";
import { MoreHorizontal } from "lucide-react";
import { cn } from "@/lib/utils";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { usePreferencesStore } from "@/stores/preferences-store";
import { moreScreens } from "@/lib/phone-tabs";
import { screenFor } from "./nav-items";

const TAB = "flex min-w-0 flex-1 flex-col items-center justify-center gap-1 py-2 text-[11px] focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-[hsl(var(--ring))]";

/** Below md the sidebar is hidden and this bar takes its place. The WebView reports no bottom inset for
 * Android's gesture handle, so the bar keeps at least 1.25rem below its labels. */
export function BottomNav() {
  const [moreOpen, setMoreOpen] = useState(false);
  const { pathname } = useLocation();
  const navigate = useNavigate();
  // The four tabs are the person's choice (Settings, Bottom bar); More lists everything else.
  const phoneTabs = usePreferencesStore((s) => s.phoneTabs);
  const tabs = phoneTabs.map(screenFor);
  const more = moreScreens(phoneTabs).map(screenFor);
  const onMore = more.some((s) => pathname === s.to || pathname.startsWith(s.to + "/"));

  return (
    <nav data-testid="bottom-nav" className="flex shrink-0 border-t border-border bg-card/80 pb-[max(env(safe-area-inset-bottom),1.25rem)] md:hidden">
      {tabs.map((item) => (
        <NavLink key={item.to} to={item.to} end={item.end} className={({ isActive }) => cn(TAB, isActive ? "text-foreground" : "text-muted-foreground")}>
          <item.icon className="h-5 w-5" />
          <span className="truncate">{item.label}</span>
        </NavLink>
      ))}
      <button type="button" onClick={() => setMoreOpen(true)} className={cn(TAB, onMore ? "text-foreground" : "text-muted-foreground")}>
        <MoreHorizontal className="h-5 w-5" />
        <span>More</span>
      </button>

      <Dialog open={moreOpen} onOpenChange={setMoreOpen}>
        <DialogContent className="w-[calc(100%-2rem)] p-4">
          <DialogHeader>
            <DialogTitle>More</DialogTitle>
          </DialogHeader>
          <div className="grid gap-1">
            {more.map((item) => (
              <button
                key={item.to}
                type="button"
                onClick={() => {
                  setMoreOpen(false);
                  navigate(item.to);
                }}
                className={cn(
                  "flex h-11 items-center gap-3 rounded-md px-3 text-sm",
                  pathname === item.to ? "bg-accent font-medium text-foreground" : "text-muted-foreground hover:bg-accent/60",
                )}
              >
                <item.icon className="h-4 w-4 shrink-0" />
                {item.label}
              </button>
            ))}
          </div>
        </DialogContent>
      </Dialog>
    </nav>
  );
}

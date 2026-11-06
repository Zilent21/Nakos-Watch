import { NavLink } from "react-router-dom";
import { cn } from "@/lib/utils";
import { NakosMark } from "@/components/brand/nakos-mark";
import { NAV_GROUPS, SETTINGS_ITEM, type NavItem } from "./nav-items";

function NavRow({ item }: { item: NavItem }) {
  return (
    <NavLink
      to={item.to}
      end={item.end}
      className={({ isActive }) =>
        cn(
          "flex h-9 items-center gap-3 rounded-md px-3 text-sm transition-colors focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-[hsl(var(--ring))]",
          isActive ? "bg-accent font-medium text-foreground" : "text-muted-foreground hover:bg-accent/60 hover:text-foreground",
        )
      }
    >
      <item.icon className="h-4 w-4 shrink-0" />
      <span className="truncate">{item.label}</span>
    </NavLink>
  );
}

export function Sidebar() {
  return (
    <aside className="hidden h-full w-60 shrink-0 flex-col border-r border-border bg-card/40 md:flex">
      <div className="flex h-14 items-center gap-3 px-5" data-tauri-drag-region>
        <NakosMark className="h-7" />
        <span className="text-sm font-semibold tracking-tight">Nakos</span>
      </div>

      <nav className="flex-1 space-y-6 overflow-y-auto px-3 py-4">
        {NAV_GROUPS.map((group) => (
          <div key={group.heading} className="space-y-1">
            <div className="px-3 pb-1 text-xs font-medium text-muted-foreground/70">{group.heading}</div>
            {group.items.map((item) => (
              <NavRow key={item.to} item={item} />
            ))}
          </div>
        ))}
      </nav>

      <div className="border-t border-border p-3">
        <NavRow item={SETTINGS_ITEM} />
      </div>
    </aside>
  );
}

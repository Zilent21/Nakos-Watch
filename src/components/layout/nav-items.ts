import {
  Sun,
  LayoutDashboard,
  ShieldCheck,
  EyeOff,
  Coins,
  SendHorizonal,
  Network,
  Server,
  Users,
  Settings,
  HeartPulse,
  QrCode,
} from "lucide-react";
import type { ComponentType } from "react";

export interface NavItem {
  to: string;
  label: string;
  icon: ComponentType<{ className?: string }>;
  end?: boolean;
}

/** Sidebar screens, in the order and with the labels of the IA. */
export const NAV_GROUPS: { heading: string; items: NavItem[] }[] = [
  {
    heading: "Overview",
    items: [
      { to: "/", label: "Today", icon: Sun, end: true },
      { to: "/portfolio", label: "Portfolio", icon: LayoutDashboard },
      { to: "/health", label: "Health", icon: HeartPulse },
    ],
  },
  {
    heading: "Protect",
    items: [
      { to: "/security", label: "Security", icon: ShieldCheck },
      { to: "/privacy", label: "Privacy", icon: EyeOff },
      { to: "/inheritance", label: "Inheritance", icon: Users },
    ],
  },
  {
    heading: "Operate",
    items: [
      { to: "/receive", label: "Receive", icon: QrCode },
      { to: "/utxos", label: "Coins", icon: Coins },
      { to: "/spend", label: "Spend", icon: SendHorizonal },
      { to: "/asset-map", label: "Map", icon: Network },
      { to: "/node", label: "Node", icon: Server },
    ],
  },
];

export const SETTINGS_ITEM: NavItem = { to: "/settings", label: "Settings", icon: Settings };

export const ALL_SCREENS: NavItem[] = [...NAV_GROUPS.flatMap((g) => g.items), SETTINGS_ITEM];

/** The nav item for a route path (the phone bar stores paths; see src/lib/phone-tabs.ts). */
export function screenFor(path: string): NavItem {
  const item = ALL_SCREENS.find((s) => s.to === path);
  if (!item) throw new Error(`unknown screen ${path}`);
  return item;
}

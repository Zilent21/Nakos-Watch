import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ChevronRight, Bitcoin, Layers, Wallet as WalletIcon, Coins, Search } from "lucide-react";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import { formatAddress, formatTxid } from "@/lib/format";
import { useFormatAmount } from "@/lib/use-amount";
import { PageHeader } from "@/components/ui/page-header";
import * as api from "@/bindings/commands";
import { ArchiveWalletButton } from "@/components/archive-wallet-button";
import type { Utxo, Wallet } from "@/bindings/types";

type TreeNode = {
  id: string;
  label: string;
  type: "root" | "custody" | "wallet" | "utxo";
  icon: React.ElementType;
  sats?: number;
  children?: TreeNode[];
  detail?: Record<string, string | number>;
  /// Only set on "wallet" nodes — lets the detail panel offer an archive
  /// control without threading the full Wallet type through every node kind.
  walletRef?: Wallet;
};

function buildTree(wallets: Wallet[], utxosByWallet: Record<string, Utxo[]>, balances: Map<string, number>, amount: (sats: number, btcDecimals?: number) => string): TreeNode {
  // Same per-wallet figure as the Portfolio table; the local sum is only a
  // fallback while balances load.
  const walletSats = (w: Wallet) =>
    balances.get(w.id) ?? (utxosByWallet[w.id] ?? []).reduce((s, u) => s + u.amount_sats, 0) + (w.manual_balance_sats ?? 0);
  const byCustody = new Map<string, Wallet[]>();
  for (const w of wallets) {
    const list = byCustody.get(w.custody_type) ?? [];
    list.push(w);
    byCustody.set(w.custody_type, list);
  }

  const custodyNodes: TreeNode[] = Array.from(byCustody.entries()).map(([custody, ws]) => ({
    id: `custody-${custody}`,
    label: custody.replace("_", " "),
    type: "custody",
    icon: Layers,
    sats: ws.reduce((sum, w) => sum + walletSats(w), 0),
    children: ws.map((w) => {
      const utxos = utxosByWallet[w.id] ?? [];
      return {
        id: `wallet-${w.id}`,
        label: w.name,
        type: "wallet" as const,
        icon: WalletIcon,
        sats: walletSats(w),
        detail: {
          network: w.network,
          source: w.source_type,
          risk: w.risk_level,
          master_fingerprint: w.master_fingerprint ?? "not on file",
        },
        walletRef: w,
        children: utxos.map((u) => ({
          id: `utxo-${u.id}`,
          label: `${formatTxid(u.txid)}:${u.vout}`,
          type: "utxo" as const,
          icon: Coins,
          sats: u.amount_sats,
          detail: {
            amount: amount(u.amount_sats),
            confirmations: u.confirmations,
            frozen: u.is_frozen ? "yes" : "no",
            label: u.label ?? "—",
          },
        })),
      };
    }),
  }));

  return { id: "root", label: "Bitcoin", type: "root", icon: Bitcoin, sats: custodyNodes.reduce((s, c) => s + (c.sats ?? 0), 0), children: custodyNodes };
}

function TreeItem({
  node,
  depth,
  search,
  selected,
  onSelect,
}: {
  node: TreeNode;
  depth: number;
  search: string;
  selected: string | null;
  onSelect: (n: TreeNode) => void;
}) {
  const amount = useFormatAmount();
  const [open, setOpen] = useState(depth < 2);
  const matches = search ? node.label.toLowerCase().includes(search.toLowerCase()) : true;
  const childMatches = node.children?.some((c) => nodeOrDescendantMatches(c, search)) ?? false;

  if (search && !matches && !childMatches) return null;

  const Icon = node.icon;
  const hasChildren = (node.children?.length ?? 0) > 0;

  return (
    <div>
      <button
        onClick={() => {
          onSelect(node);
          if (hasChildren) setOpen((v) => !v);
        }}
        className={cn(
          "flex w-full items-center gap-1.5 rounded-md px-2 py-1.5 text-left text-sm hover:bg-accent/60",
          selected === node.id && "bg-accent text-foreground",
        )}
        style={{ paddingLeft: depth * 16 + 8 }}
      >
        {hasChildren ? (
          <ChevronRight className={cn("h-3.5 w-3.5 shrink-0 transition-transform", open && "rotate-90")} />
        ) : (
          <span className="w-3.5" />
        )}
        <Icon className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
        <span className="truncate">{node.label}</span>
        {node.sats !== undefined && <span className="ml-auto mono-nums text-xs text-muted-foreground">{amount(node.sats, 4)}</span>}
      </button>
      {open && hasChildren && (
        <div>
          {node.children!.map((c) => (
            <TreeItem key={c.id} node={c} depth={depth + 1} search={search} selected={selected} onSelect={onSelect} />
          ))}
        </div>
      )}
    </div>
  );
}

function nodeOrDescendantMatches(node: TreeNode, search: string): boolean {
  if (!search) return true;
  if (node.label.toLowerCase().includes(search.toLowerCase())) return true;
  return node.children?.some((c) => nodeOrDescendantMatches(c, search)) ?? false;
}

export function AssetMapPage() {
  const amount = useFormatAmount();
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<TreeNode | null>(null);

  const { data: wallets = [] } = useQuery({ queryKey: ["wallets"], queryFn: () => api.listWallets() });
  const { data: utxos = [] } = useQuery({ queryKey: ["utxos", "all"], queryFn: () => api.listUtxos() });

  const utxosByWallet = useMemo(() => {
    const map: Record<string, Utxo[]> = {};
    for (const u of utxos) {
      (map[u.wallet_id] ??= []).push(u);
    }
    return map;
  }, [utxos]);

  const { data: balances } = useQuery({
    queryKey: ["portfolio-summary", "wallet-balances", false],
    queryFn: () => api.listWalletBalances(false),
  });
  const balanceMap = useMemo(() => new Map((balances ?? []).map((b) => [b.wallet_id, b.balance_sats])), [balances]);

  const tree = useMemo(() => buildTree(wallets, utxosByWallet, balanceMap, amount), [wallets, utxosByWallet, balanceMap, amount]);

  return (
    <div className="space-y-6">
      <PageHeader title="Map" description="Bitcoin, custody type, wallet, then coin." />

      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <Card className="md:col-span-2">
          <CardContent className="p-3">
            <div className="mb-2 flex items-center gap-2 rounded-md border border-border bg-secondary/40 px-2.5 py-1.5">
              <Search className="h-3.5 w-3.5 text-muted-foreground" />
              <input
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder="Search wallets, UTXOs, addresses…"
                className="w-full bg-transparent text-sm outline-hidden placeholder:text-muted-foreground"
              />
            </div>
            <div className="max-h-[560px] overflow-y-auto">
              <TreeItem node={tree} depth={0} search={search} selected={selected?.id ?? null} onSelect={setSelected} />
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardContent className="p-5">
            {selected ? (
              <div className="space-y-3">
                <div className="flex items-center gap-2">
                  <selected.icon className="h-4 w-4 text-muted-foreground" />
                  <h3 className="font-medium">{selected.label}</h3>
                </div>
                <Badge variant="outline" className="capitalize">
                  {selected.type}
                </Badge>
                {selected.sats !== undefined && (
                  <div className="mono-nums text-lg font-semibold">{amount(selected.sats)}</div>
                )}
                {selected.detail && (
                  <div className="space-y-1.5 border-t border-border pt-3">
                    {Object.entries(selected.detail).map(([k, v]) => (
                      <div key={k} className="flex justify-between text-xs">
                        <span className="capitalize text-muted-foreground">{k}</span>
                        <span className="mono-nums">{typeof v === "string" && v.length > 20 ? formatAddress(v) : v}</span>
                      </div>
                    ))}
                  </div>
                )}
                {selected.walletRef && (
                  <div className="border-t border-border pt-3">
                    <ArchiveWalletButton wallet={selected.walletRef} />
                    <p className="mt-1 text-[11px] text-muted-foreground">
                      Unarchiving is only available from Portfolio's "Show archived" list.
                    </p>
                  </div>
                )}
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">Select a node to inspect it.</p>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

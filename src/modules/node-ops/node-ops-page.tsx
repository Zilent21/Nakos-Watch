import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { Server, Plus, CheckCircle2, XCircle, Wifi, FolderOpen, ShieldAlert } from "lucide-react";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Progress } from "@/components/ui/progress";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import * as api from "@/bindings/commands";
import { formatRelativeTime } from "@/lib/format";
import { toast } from "@/hooks/use-toast";
import { cn } from "@/lib/utils";
import { ArchiveNodeButton } from "@/components/archive-node-button";
import type { NodeConfig } from "@/bindings/types";
import { PublicServerFacts } from "@/components/public-server-gate";
import { useOs } from "@/lib/platform";
import { coreCookieExample, coreDataFolderHint } from "@/lib/os-text";

/** The public Electrum server Nakos offers to people without a node (TLS, checked to sync). */
const PUBLIC_ELECTRUM = { host: "electrum.blockstream.info", port: 50002 };

export function NodeOpsPage() {
  const queryClient = useQueryClient();
  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<NodeConfig | null>(null);

  const { data: configs = [] } = useQuery({ queryKey: ["node-configs"], queryFn: api.listNodeConfigs });
  const [showArchived, setShowArchived] = useState(false);
  // Archived connections are stored as disabled (enabled = false) and are never used.
  const activeConfigs = configs.filter((c) => c.enabled);
  const archivedCount = configs.length - activeConfigs.length;
  const visibleConfigs = showArchived ? configs : activeConfigs;
  const { data: status } = useQuery({ queryKey: ["node-status"], queryFn: api.getNodeStatus, refetchInterval: 15_000 });
  const { data: fees } = useQuery({ queryKey: ["fee-estimates"], queryFn: api.getFeeEstimates });
  // Makes no request unless the public explorer is switched on in Settings.
  const { data: explorerHeight } = useQuery({ queryKey: ["explorer-height"], queryFn: api.getExplorerHeight });

  const [kind, setKind] = useState<"bitcoin_core_rpc" | "electrum">("electrum");
  const [authMode, setAuthMode] = useState<"cookie" | "userpass">("cookie");
  const [label, setLabel] = useState("");
  const [host, setHost] = useState("127.0.0.1");
  const [port, setPort] = useState(50002);
  const [rpcUser, setRpcUser] = useState("");
  const [rpcPassword, setRpcPassword] = useState("");
  const [cookiePath, setCookiePath] = useState("");
  const os = useOs();
  const [useTls, setUseTls] = useState(true);
  const [usingPublicPreset, setUsingPublicPreset] = useState(false);

  /** Fill the form with a public Electrum server for people who do not run a node. */
  function fillPublicServer() {
    setKind("electrum");
    setLabel("Blockstream public Electrum");
    setHost(PUBLIC_ELECTRUM.host);
    setPort(PUBLIC_ELECTRUM.port);
    setUseTls(true);
    setUsingPublicPreset(true);
  }

  function onKindChange(next: "bitcoin_core_rpc" | "electrum") {
    setKind(next);
    setPort(next === "bitcoin_core_rpc" ? 8332 : 50002);
    if (next === "bitcoin_core_rpc") setUseTls(false);
  }

  function openAddDialog() {
    setEditing(null);
    setKind("electrum");
    setAuthMode("cookie");
    setLabel("");
    setHost("127.0.0.1");
    setPort(50002);
    setRpcUser("");
    setRpcPassword("");
    setCookiePath("");
    setUseTls(true);
    setUsingPublicPreset(false);
    setFormOpen(true);
  }

  function openEditDialog(c: NodeConfig) {
    setEditing(c);
    setKind(c.kind);
    setAuthMode(c.cookie_path ? "cookie" : "userpass");
    setLabel(c.label);
    setHost(c.host);
    setPort(c.port);
    setRpcUser(c.rpc_user ?? "");
    // Never prefilled — list_node_configs/NodeConfig doesn't return the
    // decrypted password at all, so there is nothing to show here anyway.
    setRpcPassword("");
    setCookiePath(c.cookie_path ?? "");
    setUseTls(c.use_tls);
    setFormOpen(true);
  }

  async function browseCookieFile() {
    const path = await open({
      multiple: false,
      title: "Select bitcoin.conf's .cookie file",
      filters: [{ name: "Cookie file", extensions: ["cookie", "*"] }],
    });
    if (path && !Array.isArray(path)) setCookiePath(path);
  }

  const saveMutation = useMutation({
    mutationFn: () => {
      const input = {
        kind,
        label,
        host,
        port,
        rpc_user: kind === "bitcoin_core_rpc" && authMode === "userpass" ? rpcUser : undefined,
        rpc_password: kind === "bitcoin_core_rpc" && authMode === "userpass" ? rpcPassword : undefined,
        cookie_path: kind === "bitcoin_core_rpc" && authMode === "cookie" ? cookiePath : undefined,
        // Bitcoin Core RPC is plain HTTP only; the backend refuses a Core connection with TLS on.
        use_tls: kind === "electrum" ? useTls : false,
        use_tor: false,
      };
      return editing ? api.updateNodeConfig(editing.id, input) : api.addNodeConfig(input);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["node-configs"] });
      queryClient.invalidateQueries({ queryKey: ["node-status"] });
      setFormOpen(false);
      toast({ title: "Saved", description: `${label} connection saved`, variant: "success" });
    },
    onError: (e) => toast({ title: "Could not save connection", description: String(e), variant: "destructive" }),
  });

  const primaryMutation = useMutation({
    mutationFn: (c: NodeConfig) => api.setPrimaryNode(c.id),
    onSuccess: (_d, c) => {
      queryClient.invalidateQueries({ queryKey: ["node-configs"] });
      queryClient.invalidateQueries({ queryKey: ["node-status"] });
      queryClient.invalidateQueries({ queryKey: ["fee-estimates"] });
      toast({ title: "Primary updated", description: `${c.label} is now the primary ${c.kind === "bitcoin_core_rpc" ? "Bitcoin Core node" : "Electrum server"}`, variant: "success" });
    },
    onError: (e) => toast({ title: "Could not set primary", description: String(e), variant: "destructive" }),
  });

  /** Same rule as the backend: the explicit primary of a type, else the oldest enabled one (the list is oldest first). */
  function primaryIdFor(k: NodeConfig["kind"]): string | undefined {
    const ofKind = configs.filter((o) => o.kind === k && o.enabled);
    return (ofKind.find((o) => o.is_default) ?? ofKind[0])?.id;
  }

  const testMutation = useMutation({
    mutationFn: (id: string) => api.testNodeConnection(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["node-configs"] });
      queryClient.invalidateQueries({ queryKey: ["node-status"] });
    },
  });

  return (
    <div className="space-y-6">
      <div className="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Node</h1>
          <p className="text-sm text-muted-foreground">Connect Bitcoin Core for node health and an Electrum server to sync wallets.</p>
        </div>
        <Button onClick={openAddDialog}>
          <Plus className="h-4 w-4" />
          Add connection
        </Button>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Server className="h-4 w-4" /> Bitcoin Core
            </CardTitle>
          </CardHeader>
          <CardContent>
            {status?.core ? (
              <div className="space-y-3 text-sm">
                <div className="flex justify-between">
                  <span className="text-muted-foreground">Chain</span>
                  <span>{status.core.chain}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted-foreground">Sync</span>
                  <span>
                    {status.core.blocks.toLocaleString()} / {status.core.headers.toLocaleString()}
                  </span>
                </div>
                <Progress value={status.core.verification_progress * 100} />
                <div className="flex justify-between">
                  <span className="text-muted-foreground">Peers</span>
                  <span>
                    {status.core.connections_in} in / {status.core.connections_out} out
                  </span>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted-foreground">Pruned</span>
                  <span>{status.core.is_pruned ? `yes (from ${status.core.prune_height})` : "no"}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted-foreground">Tor reachable</span>
                  <span>{status.core.network_reachable_via_tor ? "yes" : "no"}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted-foreground">Mempool</span>
                  <span>{status.core.mempool_tx_count.toLocaleString()} txs</span>
                </div>
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">No Bitcoin Core node connected.</p>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Wifi className="h-4 w-4" /> Electrum
            </CardTitle>
          </CardHeader>
          <CardContent>
            {status?.electrum ? (
              <div className="space-y-3 text-sm">
                <div className="flex justify-between">
                  <span className="text-muted-foreground">Server</span>
                  <span>{status.electrum.server_version}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted-foreground">Protocol</span>
                  <span>{status.electrum.protocol_version}</span>
                </div>
                <div className="flex justify-between">
                  <span className="text-muted-foreground">Tip height</span>
                  <span className="mono-nums">{status.electrum.tip_height.toLocaleString()}</span>
                </div>
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">No Electrum server connected.</p>
            )}
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Chain tip cross-check</CardTitle>
          <CardDescription>Compares your Bitcoin Core height with the optional public explorer.</CardDescription>
        </CardHeader>
        <CardContent className="text-sm">
          {!explorerHeight ? null : !explorerHeight.enabled ? (
            <span className="text-muted-foreground">Disabled. The public explorer is off, so nothing is requested. Turn it on in Settings to compare.</span>
          ) : explorerHeight.error ? (
            <span className="text-destructive">Explorer unreachable: {explorerHeight.error}</span>
          ) : (
            <span className="mono-nums">
              Explorer height {explorerHeight.height?.toLocaleString()}
              {status?.core ? (
                <>
                  {" "}
                  vs Core height {status.core.blocks.toLocaleString()}
                  {explorerHeight.height !== null && explorerHeight.height !== undefined && explorerHeight.height !== status.core.blocks
                    ? ` (${Math.abs(explorerHeight.height - status.core.blocks)} block${Math.abs(explorerHeight.height - status.core.blocks) === 1 ? "" : "s"} apart)`
                    : " (in step)"}
                </>
              ) : (
                " · Bitcoin Core isn't reachable to compare"
              )}
            </span>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Fee estimates</CardTitle>
          <CardDescription>
            Source: {fees?.source ?? "—"}
            {fees?.source === "none" && fees.explorer_status === "disabled" && " · no local estimate, and the public explorer is off (Settings)"}
            {fees?.explorer_status?.startsWith("error") && ` · explorer ${fees.explorer_status}`}
          </CardDescription>
        </CardHeader>
        <CardContent>
          {fees?.public_estimate ? (
            <div className="grid grid-cols-3 md:grid-cols-5 gap-3 text-center">
              {[
                ["Fastest", fees.public_estimate.fastest_fee],
                ["30 min", fees.public_estimate.half_hour_fee],
                ["1 hour", fees.public_estimate.hour_fee],
                ["Economy", fees.public_estimate.economy_fee],
                ["Minimum", fees.public_estimate.minimum_fee],
              ].map(([label, val]) => (
                <div key={label as string} className="rounded-md border border-border p-3">
                  <div className="mono-nums text-lg font-semibold">{val as number}</div>
                  <div className="text-xs text-muted-foreground">{label} sat/vB</div>
                </div>
              ))}
            </div>
          ) : (
            <div className="grid grid-cols-3 md:grid-cols-6 gap-3 text-center">
              {(fees?.core_estimates ?? []).map((e) => (
                <div key={e.target_blocks} className="rounded-md border border-border p-3">
                  <div className="mono-nums text-lg font-semibold">{e.sat_per_vb?.toFixed(1) ?? "—"}</div>
                  <div className="text-xs text-muted-foreground">{e.target_blocks} blocks</div>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <div className="flex items-center justify-between gap-4">
            <CardTitle>Connections</CardTitle>
            <label className="flex items-center gap-3 text-sm text-muted-foreground">
              <Switch checked={showArchived} onCheckedChange={setShowArchived} />
              Show archived{archivedCount > 0 ? ` (${archivedCount})` : ""}
            </label>
          </div>
          <CardDescription>
            Nakos uses one primary connection of each type for status, fee estimates, the health score and wallet sync. Test a connection, then choose Use as primary. Without a choice, the oldest saved connection of a type is used.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-2">
          {configs.length === 0 && <p className="text-sm text-muted-foreground">No connections configured yet.</p>}
          {configs.length > 0 && visibleConfigs.length === 0 && (
            <p className="text-sm text-muted-foreground">No active connections. Turn on Show archived to see the {archivedCount} archived.</p>
          )}
          {visibleConfigs.map((c) => (
            <div key={c.id} className={cn("flex flex-col gap-2 rounded-md border border-border p-3 md:flex-row md:items-center md:justify-between", !c.enabled && "opacity-60")}>
              <button
                type="button"
                onClick={() => openEditDialog(c)}
                className="min-w-0 flex-1 cursor-pointer rounded-sm text-left focus-visible:outline-hidden focus-visible:ring-1 focus-visible:ring-[hsl(var(--ring))]"
                title="Click to edit this connection"
              >
                <div className="flex flex-wrap items-center gap-2 text-sm font-medium">
                  {c.label}
                  <Badge variant="outline">{c.kind === "bitcoin_core_rpc" ? "Core RPC" : "Electrum"}</Badge>
                  {primaryIdFor(c.kind) === c.id && <Badge variant="success">Primary</Badge>}
                  {c.third_party && (
                    <Badge variant="warning" title="Someone else runs this server. It sees the addresses Nakos syncs and your IP address.">
                      third-party
                    </Badge>
                  )}
                  {!c.enabled && <Badge variant="muted">archived</Badge>}
                  {c.enabled && activeConfigs.findIndex((o) => o.kind === c.kind && o.host.toLowerCase() === c.host.toLowerCase() && o.port === c.port) < activeConfigs.indexOf(c) && (
                    <Badge variant="muted" title="Another saved connection uses the same host and port">
                      duplicate
                    </Badge>
                  )}
                </div>
                <div className="text-xs text-muted-foreground">
                  {c.host}:{c.port} · last connected {formatRelativeTime(c.last_connected_at)}
                </div>
              </button>
              <div className="flex flex-wrap items-center gap-2 self-end md:self-auto">
                <ArchiveNodeButton node={c} />
                {c.enabled && primaryIdFor(c.kind) !== c.id && c.last_connected_at && (
                  <Button
                    size="sm"
                    onClick={(e) => {
                      e.stopPropagation();
                      primaryMutation.mutate(c);
                    }}
                    disabled={primaryMutation.isPending}
                    title="This connection passed its test. Use it for status, fees, the health score and sync."
                  >
                    Use as primary
                  </Button>
                )}
                {c.enabled && (
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={(e) => {
                      e.stopPropagation();
                      testMutation.mutate(c.id);
                    }}
                    disabled={testMutation.isPending}
                  >
                    {testMutation.isSuccess && testMutation.variables === c.id ? (
                      <CheckCircle2 className="h-4 w-4 text-success" />
                    ) : testMutation.isError && testMutation.variables === c.id ? (
                      <XCircle className="h-4 w-4 text-destructive" />
                    ) : null}
                    Test connection
                  </Button>
                )}
              </div>
            </div>
          ))}
        </CardContent>
      </Card>

      <Dialog open={formOpen} onOpenChange={setFormOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{editing ? "Edit connection" : "Add node connection"}</DialogTitle>
          </DialogHeader>
          <div className="space-y-4">
            {!editing && (
              <div className="rounded-md border border-border p-3 text-sm">
                <p className="font-semibold">No node of your own?</p>
                <p className="mt-1 text-muted-foreground">Nakos can sync through a public Electrum server instead. Everything works, but you give up privacy: read what the server sees before you choose it.</p>
                <Button variant="outline" size="sm" className="mt-2" onClick={fillPublicServer}>
                  Use a public server
                </Button>
              </div>
            )}
            {kind === "electrum" && usingPublicPreset && host === PUBLIC_ELECTRUM.host && (
              <div role="note" className="space-y-2 rounded-md border border-warning/50 bg-warning/10 p-3">
                <p className="flex items-center gap-2 text-sm font-semibold">
                  <ShieldAlert className="h-4 w-4 text-warning" /> What a public server sees
                </p>
                <PublicServerFacts host={host} />
                <p className="text-xs text-muted-foreground">Nakos asks you to confirm this again before the first sync.</p>
              </div>
            )}
            <div className="space-y-1.5">
              <Label>Type</Label>
              <Select value={kind} onValueChange={(v) => onKindChange(v as typeof kind)}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="electrum">Electrum server</SelectItem>
                  <SelectItem value="bitcoin_core_rpc">Bitcoin Core RPC</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <Label>Label</Label>
              <Input value={label} onChange={(e) => setLabel(e.target.value)} placeholder="My node" />
            </div>
            <div className="grid grid-cols-3 gap-3">
              <div className="col-span-2 space-y-1.5">
                <Label>Host</Label>
                <Input value={host} onChange={(e) => setHost(e.target.value)} placeholder="127.0.0.1" />
              </div>
              <div className="space-y-1.5">
                <Label>Port</Label>
                <Input type="number" value={port} onChange={(e) => setPort(Number(e.target.value))} />
              </div>
            </div>
            {kind === "bitcoin_core_rpc" && (
              <>
                <div className="space-y-1.5">
                  <Label>Authentication</Label>
                  <Select value={authMode} onValueChange={(v) => setAuthMode(v as typeof authMode)}>
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="cookie">Cookie file (default for Bitcoin Core)</SelectItem>
                      <SelectItem value="userpass">rpcuser / rpcpassword</SelectItem>
                    </SelectContent>
                  </Select>
                </div>

                {authMode === "cookie" ? (
                  <div className="space-y-1.5">
                    <Label>.cookie file path</Label>
                    <div className="flex gap-2">
                      <Input
                        value={cookiePath}
                        onChange={(e) => setCookiePath(e.target.value)}
                        placeholder={coreCookieExample(os)}
                        className="font-mono text-xs"
                      />
                      <Button type="button" variant="outline" size="icon" onClick={browseCookieFile}>
                        <FolderOpen className="h-4 w-4" />
                      </Button>
                    </div>
                    <p className="text-[11px] text-muted-foreground">
                      Bitcoin Core writes it in its data folder, usually <code>{coreDataFolderHint(os)}</code>, or
                      in the network subfolder for testnet/signet.
                    </p>
                  </div>
                ) : (
                  <div className="grid grid-cols-2 gap-3">
                    <div className="space-y-1.5">
                      <Label>RPC user</Label>
                      <Input value={rpcUser} onChange={(e) => setRpcUser(e.target.value)} />
                    </div>
                    <div className="space-y-1.5">
                      <Label>RPC password</Label>
                      <Input
                        type="password"
                        value={rpcPassword}
                        onChange={(e) => setRpcPassword(e.target.value)}
                        placeholder={editing ? "Leave blank to keep current password" : undefined}
                      />
                    </div>
                  </div>
                )}
              </>
            )}
            {kind === "electrum" ? (
              <label className="flex items-center gap-2 text-sm">
                <Switch checked={useTls} onCheckedChange={setUseTls} />
                Use TLS/SSL
              </label>
            ) : (
              <p className="text-xs text-muted-foreground">
                Nakos connects to Bitcoin Core over plain HTTP. TLS isn&apos;t supported here, so the RPC password or cookie is not encrypted on the network. Use a node on a network you trust.
              </p>
            )}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setFormOpen(false)}>
              Cancel
            </Button>
            <Button onClick={() => saveMutation.mutate()} disabled={!label || !host || saveMutation.isPending}>
              Save
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

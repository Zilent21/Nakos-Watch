import { useSearchParams } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { PageHeader } from "@/components/ui/page-header";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import * as api from "@/bindings/commands";
import { WalletReceive } from "./wallet-receive";
import { SavedAddresses } from "./saved-addresses";
import { AlertsPanel } from "./alerts-panel";
import { AddressSourcesNote } from "./shared";

const TABS = ["wallet", "saved", "alerts"] as const;
type Tab = (typeof TABS)[number];

export function ReceivePage() {
  const [params, setParams] = useSearchParams();
  const raw = params.get("tab");
  const tab: Tab = TABS.includes(raw as Tab) ? (raw as Tab) : "wallet";
  const { data: unread = [] } = useQuery({ queryKey: ["alerts", false], queryFn: () => api.listAlerts(false) });

  return (
    <>
      <PageHeader
        title="Receive"
        description="Fresh addresses for your own wallets, without plugging in the hardware wallet. Watch-only: Nakos can't send or move coins."
      />
      <Tabs value={tab} onValueChange={(v) => setParams(v === "wallet" ? {} : { tab: v }, { replace: true })}>
        <TabsList>
          <TabsTrigger value="wallet">From a wallet</TabsTrigger>
          <TabsTrigger value="saved">Saved addresses</TabsTrigger>
          <TabsTrigger value="alerts">Alerts{unread.length ? ` (${unread.length})` : ""}</TabsTrigger>
        </TabsList>
        <TabsContent value="wallet" className="mt-6 space-y-6">
          <AddressSourcesNote />
          <WalletReceive />
        </TabsContent>
        <TabsContent value="saved" className="mt-6 space-y-6">
          <AddressSourcesNote />
          <SavedAddresses />
        </TabsContent>
        <TabsContent value="alerts" className="mt-6">
          <AlertsPanel />
        </TabsContent>
      </Tabs>
    </>
  );
}

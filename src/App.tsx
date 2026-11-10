import { useEffect, useState } from "react";
import { Routes, Route, Navigate } from "react-router-dom";
import { useVaultStore } from "@/stores/vault-store";
import { usePreferencesStore } from "@/stores/preferences-store";
import { runDailyHealthSnapshot } from "@/bindings/commands";
import { useQueryClient } from "@tanstack/react-query";
import { useAutoLock } from "@/hooks/use-auto-lock";
import { AppShell } from "@/components/layout/app-shell";
import { WelcomeScreen } from "@/modules/onboarding/welcome-screen";
import { UnlockScreen } from "@/modules/onboarding/unlock-screen";
import { PortfolioPage } from "@/modules/portfolio/portfolio-page";
import { SecurityPage } from "@/modules/security/security-page";
import { AssetMapPage } from "@/modules/asset-mapping/asset-map-page";
import { UtxoManagerPage } from "@/modules/utxo-manager/utxo-manager-page";
import { SpendingPlannerPage } from "@/modules/spending-planner/spending-planner-page";
import { ReceivePage } from "@/modules/receive/receive-page";
import { TodayPage } from "@/modules/today/today-page";
import { HealthScorePage } from "@/modules/health-score/health-score-page";
import { PrivacyPage } from "@/modules/privacy/privacy-page";
import { NodeOpsPage } from "@/modules/node-ops/node-ops-page";
import { InheritancePage } from "@/modules/inheritance/inheritance-page";
import { SettingsPage } from "@/modules/settings-page";

export default function App() {
  const { initialized, unlocked, loading, keychainRemembered, refresh, tryKeychainUnlock } = useVaultStore();
  const loadPrefs = usePreferencesStore((s) => s.load);
  const [attemptedAutoUnlock, setAttemptedAutoUnlock] = useState(false);
  const queryClient = useQueryClient();
  useAutoLock(unlocked);

  useEffect(() => {
    // However the vault locked (button, timer, background), nothing it showed stays cached.
    if (!unlocked) queryClient.clear();
  }, [unlocked, queryClient]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    // Only ever touch the OS keychain if the user opted in; never on a plain start.
    if (!loading && initialized && !unlocked && keychainRemembered && !attemptedAutoUnlock) {
      setAttemptedAutoUnlock(true);
      tryKeychainUnlock().catch(() => {});
    }
  }, [loading, initialized, unlocked, keychainRemembered, attemptedAutoUnlock, tryKeychainUnlock]);

  useEffect(() => {
    if (unlocked) loadPrefs();
  }, [unlocked, loadPrefs]);

  useEffect(() => {
    // Optional daily health snapshot; the backend does nothing unless the user turned it on.
    if (unlocked) void runDailyHealthSnapshot().catch(() => {});
  }, [unlocked]);

  if (loading) {
    return (
      <div className="flex h-screen w-screen items-center justify-center bg-background">
        <div className="h-8 w-8 animate-spin rounded-full border-2 border-muted-foreground border-t-transparent" />
      </div>
    );
  }

  if (!initialized) {
    return <WelcomeScreen />;
  }

  if (!unlocked) {
    return <UnlockScreen />;
  }

  return (
    <AppShell>
      <Routes>
        <Route path="/" element={<TodayPage />} />
        <Route path="/portfolio" element={<PortfolioPage />} />
        <Route path="/security" element={<SecurityPage />} />
        <Route path="/asset-map" element={<AssetMapPage />} />
        <Route path="/utxos" element={<UtxoManagerPage />} />
        <Route path="/spend" element={<SpendingPlannerPage />} />
        <Route path="/receive" element={<ReceivePage />} />
        <Route path="/health" element={<HealthScorePage />} />
        <Route path="/privacy" element={<PrivacyPage />} />
        <Route path="/node" element={<NodeOpsPage />} />
        <Route path="/inheritance" element={<InheritancePage />} />
        <Route path="/settings" element={<SettingsPage />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </AppShell>
  );
}

import { useEffect } from "react";
import { appSettings } from "../lib/tauri";
import type { MainView } from "../lib/appModel";
import type { SettingsInfo } from "../lib/tauri";

type UseSettingsViewRefreshOptions = {
  activeView: MainView;
  settingsInfo: SettingsInfo | null;
  updateSettingsInfo: (settingsInfo: SettingsInfo) => void;
  loadSettingsMaintenance: () => Promise<void>;
  setError: (message: string) => void;
};

export function useSettingsViewRefresh({
  activeView,
  settingsInfo,
  updateSettingsInfo,
  loadSettingsMaintenance,
  setError,
}: UseSettingsViewRefreshOptions) {
  useEffect(() => {
    if (activeView !== "settings") return;
    if (settingsInfo?.notificationDatabaseChecked) return;
    const timer = window.setTimeout(() => {
      appSettings(true).then(updateSettingsInfo).catch((err) => setError(err instanceof Error ? err.message : String(err)));
    }, 120);
    return () => window.clearTimeout(timer);
  }, [activeView, settingsInfo, updateSettingsInfo, setError]);

  useEffect(() => {
    if (activeView !== "settings") return;
    loadSettingsMaintenance().catch((err) => setError(err instanceof Error ? err.message : String(err)));
  }, [activeView, loadSettingsMaintenance, setError]);
}

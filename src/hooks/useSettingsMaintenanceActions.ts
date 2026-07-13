import { useCallback } from "react";
import type { Dispatch, SetStateAction } from "react";
import {
  clearActionHistory,
  compactArchive,
  pruneArchive,
} from "../lib/tauri";
import type { ActionHistoryEntry, ArchiveStats } from "../lib/tauri";

type UseSettingsMaintenanceActionsOptions = {
  setActionHistoryItems: Dispatch<SetStateAction<ActionHistoryEntry[]>>;
  setArchiveStatsInfo: Dispatch<SetStateAction<ArchiveStats | null>>;
  refresh: () => Promise<void>;
  loadSettingsMaintenance: () => Promise<void>;
  setNotice: (message: string) => void;
  setError: (message: string) => void;
};

export function useSettingsMaintenanceActions({
  setActionHistoryItems,
  setArchiveStatsInfo,
  refresh,
  loadSettingsMaintenance,
  setNotice,
  setError,
}: UseSettingsMaintenanceActionsOptions) {
  const clearHistory = useCallback(async () => {
    await clearActionHistory();
    setActionHistoryItems([]);
    await loadSettingsMaintenance();
    setError("");
    setNotice("执行历史已清空");
  }, [loadSettingsMaintenance, setActionHistoryItems, setError, setNotice]);

  const compactArchiveData = useCallback(async () => {
    const stats = await compactArchive();
    setArchiveStatsInfo(stats);
    setError("");
    setNotice("本地归档已压缩");
  }, [setArchiveStatsInfo, setError, setNotice]);

  const pruneArchiveData = useCallback(async (retentionDays: number) => {
    const stats = await pruneArchive(retentionDays);
    setArchiveStatsInfo(stats);
    await refresh();
    setError("");
    setNotice(`已清理 ${retentionDays} 天以前的归档`);
  }, [refresh, setArchiveStatsInfo, setError, setNotice]);

  return {
    clearHistory,
    compactArchiveData,
    pruneArchiveData,
  };
}

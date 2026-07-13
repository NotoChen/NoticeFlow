import { useCallback, useState } from "react";
import type { Dispatch, SetStateAction } from "react";
import {
  clearHiddenNotifications,
  deleteSystemNotification,
  hideNotification,
  matchRuleNamesForNotification,
} from "../lib/tauri";
import type { NotificationRecord } from "../lib/tauri";

type NotificationMenuState = {
  record: NotificationRecord;
  x: number;
  y: number;
} | null;

type UseNotificationActionsOptions = {
  setNotifications: Dispatch<SetStateAction<NotificationRecord[]>>;
  setNotificationRecordId: Dispatch<SetStateAction<number | null>>;
  setPreviewRecordId: Dispatch<SetStateAction<number | null>>;
  refresh: () => Promise<void>;
  loadSettingsMaintenance: () => Promise<void>;
  setNotice: (message: string) => void;
  setError: (message: string) => void;
};

function shouldRemoveAfterSystemDeleteError(message: string) {
  return (
    message.includes("系统通知已删除") ||
    message.includes("系统通知库中未找到") ||
    message.includes("都未找到这条通知") ||
    message.includes("已隐藏列表中的旧记录")
  );
}

export function useNotificationActions({
  setNotifications,
  setNotificationRecordId,
  setPreviewRecordId,
  refresh,
  loadSettingsMaintenance,
  setNotice,
  setError,
}: UseNotificationActionsOptions) {
  const [notificationMenu, setNotificationMenu] = useState<NotificationMenuState>(null);
  const [pendingDeleteNotification, setPendingDeleteNotification] = useState<NotificationRecord | null>(null);

  const openNotificationMenu = useCallback((record: NotificationRecord, x: number, y: number) => {
    setNotificationMenu({ record, x, y });
  }, []);

  const closeNotificationMenu = useCallback(() => {
    setNotificationMenu(null);
  }, []);

  const removeNotificationLocally = useCallback((recordId: number) => {
    setNotifications((current) => current.filter((item) => item.id !== recordId));
    setNotificationRecordId((current) => (current === recordId ? null : current));
    setPreviewRecordId((current) => (current === recordId ? null : current));
  }, [setNotificationRecordId, setNotifications, setPreviewRecordId]);

  const testNotification = useCallback(async (record: NotificationRecord) => {
    try {
      const matchedRuleNames = await matchRuleNamesForNotification(record.id);
      if (!matchedRuleNames.length) {
        setError("");
        setNotice("没有规则匹配该通知");
        return;
      }
      setError("");
      setNotice(`匹配 ${matchedRuleNames.length} 条规则：${matchedRuleNames.join("、")}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setNotificationMenu(null);
    }
  }, [setError, setNotice]);

  const hideNotificationRecord = useCallback(async (record: NotificationRecord) => {
    try {
      await hideNotification(record);
      removeNotificationLocally(record.id);
      setNotificationMenu(null);
      setError("");
      setNotice("通知已从列表隐藏");
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      if (message.includes("都未找到这条通知")) {
        removeNotificationLocally(record.id);
        setNotificationMenu(null);
        setError("");
        setNotice(message);
        return;
      }
      setError(message);
    }
  }, [removeNotificationLocally, setError, setNotice]);

  const requestDeleteNotification = useCallback((record: NotificationRecord) => {
    setPendingDeleteNotification(record);
    setNotificationMenu(null);
  }, []);

  const cancelDeleteNotification = useCallback(() => {
    setPendingDeleteNotification(null);
  }, []);

  const deleteNotificationRecord = useCallback(async (record: NotificationRecord) => {
    try {
      await deleteSystemNotification(record);
      removeNotificationLocally(record.id);
      setPendingDeleteNotification(null);
      setNotificationMenu(null);
      loadSettingsMaintenance().catch(() => undefined);
      setError("");
      setNotice("通知已从系统历史删除");
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      if (shouldRemoveAfterSystemDeleteError(message)) {
        removeNotificationLocally(record.id);
      }
      setPendingDeleteNotification(null);
      setNotificationMenu(null);
      loadSettingsMaintenance().catch(() => undefined);
      if (message.includes("系统通知库中未找到")) {
        setError("");
        setNotice(message);
      } else {
        setError(message);
      }
    }
  }, [loadSettingsMaintenance, removeNotificationLocally, setError, setNotice]);

  const restoreHiddenNotifications = useCallback(async () => {
    await clearHiddenNotifications();
    await refresh();
    await loadSettingsMaintenance();
    setError("");
    setNotice("已恢复隐藏通知");
  }, [loadSettingsMaintenance, refresh, setError, setNotice]);

  return {
    notificationMenu,
    pendingDeleteNotification,
    openNotificationMenu,
    closeNotificationMenu,
    testNotification,
    hideNotificationRecord,
    requestDeleteNotification,
    cancelDeleteNotification,
    deleteNotificationRecord,
    restoreHiddenNotifications,
  };
}

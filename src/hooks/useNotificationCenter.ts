import { useEffect, useMemo, useState } from "react";
import {
  filterNotifications,
  newestNotifications,
  notificationSearchIndex,
} from "../lib/appModel";
import type { MainView } from "../lib/appModel";
import type { ApplicationInfo, NotificationRecord } from "../lib/tauri";

type UseNotificationCenterOptions = {
  activeView: MainView;
  notifications: NotificationRecord[];
  appById: Map<string, ApplicationInfo>;
  selectedRecordId: number | null;
  setSelectedRecordId: (recordId: number | null) => void;
};

export function useNotificationCenter({
  activeView,
  notifications,
  appById,
  selectedRecordId,
  setSelectedRecordId,
}: UseNotificationCenterOptions) {
  const [notificationQuery, setNotificationQuery] = useState("");
  const [notificationAppFilter, setNotificationAppFilter] = useState("");

  const notificationsByNewest = useMemo(
    () => newestNotifications(notifications),
    [notifications],
  );
  const notificationSearchItems = useMemo(
    () => notificationSearchIndex(notifications, appById),
    [notifications, appById],
  );
  const notificationCenterItems = useMemo(
    () => filterNotifications(notificationsByNewest, notificationSearchItems, notificationQuery, notificationAppFilter),
    [notificationAppFilter, notificationQuery, notificationSearchItems, notificationsByNewest],
  );

  useEffect(() => {
    if (activeView !== "home" && activeView !== "notifications") return;
    if (!notificationCenterItems.length) {
      if (selectedRecordId !== null) setSelectedRecordId(null);
      return;
    }
    if (!selectedRecordId || !notificationCenterItems.some((item) => item.id === selectedRecordId)) {
      setSelectedRecordId(notificationCenterItems[0].id);
    }
  }, [activeView, notificationCenterItems, selectedRecordId, setSelectedRecordId]);

  return {
    notificationsByNewest,
    notificationSearchItems,
    notificationCenterItems,
    notificationQuery,
    setNotificationQuery,
    notificationAppFilter,
    setNotificationAppFilter,
  };
}

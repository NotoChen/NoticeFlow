import { startTransition, useCallback, useEffect, useRef, useState } from "react";
import {
  emptyPreview,
  filterNotifications,
  sameNotificationList,
} from "../lib/appModel";
import type { MainView } from "../lib/appModel";
import { previewVariables } from "../lib/tauri";
import type { AutomationRule, NotificationRecord, VariablePreview } from "../lib/tauri";

type UseRulePreviewOptions = {
  activeView: MainView;
  notificationsByNewest: NotificationRecord[];
  notificationSearchItems: Map<number, string>;
  editingRule: AutomationRule | null;
  editingAppIdentifier: string;
  previewRecordId: number | null;
  setPreviewRecordId: (recordId: number | null) => void;
  setError: (message: string) => void;
};

export function useRulePreview({
  activeView,
  notificationsByNewest,
  notificationSearchItems,
  editingRule,
  editingAppIdentifier,
  previewRecordId,
  setPreviewRecordId,
  setError,
}: UseRulePreviewOptions) {
  const [preview, setPreview] = useState<VariablePreview>(emptyPreview);
  const [previewQuery, setPreviewQuery] = useState("");
  const [previewAppFilter, setPreviewAppFilter] = useState("");
  const [filteredNotifications, setFilteredNotifications] = useState<NotificationRecord[]>([]);
  const [previewFilterReady, setPreviewFilterReady] = useState(false);
  const previewFilterRunRef = useRef(0);

  const resetPreviewFilters = useCallback(() => {
    setPreviewQuery("");
    setPreviewAppFilter("");
  }, []);

  useEffect(() => {
    if (activeView !== "editor") return;
    const runId = ++previewFilterRunRef.current;
    setPreviewFilterReady(false);
    const timer = window.setTimeout(() => {
      const linkedApp = editingAppIdentifier || previewAppFilter;
      const next = filterNotifications(notificationsByNewest, notificationSearchItems, previewQuery, linkedApp);
      if (previewFilterRunRef.current !== runId) return;
      startTransition(() => {
        setFilteredNotifications((current) => (sameNotificationList(current, next) ? current : next));
        setPreviewFilterReady(true);
      });
    }, 0);
    return () => window.clearTimeout(timer);
  }, [activeView, notificationsByNewest, notificationSearchItems, previewQuery, previewAppFilter, editingAppIdentifier]);

  useEffect(() => {
    if (activeView !== "editor") return;
    if (!previewRecordId) {
      setPreview(emptyPreview());
      return;
    }
    let cancelled = false;
    setPreview(emptyPreview());
    previewVariables(previewRecordId, editingRule?.variableExtractions ?? [])
      .then((nextPreview) => {
        if (!cancelled) setPreview(nextPreview);
      })
      .catch((err) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [activeView, previewRecordId, editingRule?.variableExtractions, setError]);

  useEffect(() => {
    if (activeView !== "editor") return;
    if (!previewFilterReady) return;
    if (!filteredNotifications.length) {
      if (previewRecordId !== null) setPreviewRecordId(null);
      return;
    }
    if (!previewRecordId || !filteredNotifications.some((item) => item.id === previewRecordId)) {
      setPreviewRecordId(filteredNotifications[0].id);
    }
  }, [activeView, filteredNotifications, previewFilterReady, previewRecordId, setPreviewRecordId]);

  return {
    preview,
    previewQuery,
    setPreviewQuery,
    previewAppFilter,
    setPreviewAppFilter,
    filteredNotifications,
    previewFilterReady,
    resetPreviewFilters,
  };
}

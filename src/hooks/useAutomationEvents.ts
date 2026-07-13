import { useCallback, useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  mergeNotificationRefreshOptions,
  notificationFetchLimit,
  shouldRefreshNotificationsForView,
} from "../lib/appModel";
import type { MainView, NotificationRefreshOptions } from "../lib/appModel";
import {
  automationStatus,
  listNotifications,
  listRules,
} from "../lib/tauri";
import type {
  AutomationEvent,
  AutomationRule,
  AutomationStatus,
  NotificationRecord,
} from "../lib/tauri";
import type { MutableRefObject } from "react";

type UseAutomationEventsOptions = {
  activeView: MainView;
  activeViewRef: MutableRefObject<MainView>;
  refresh: (options?: NotificationRefreshOptions) => Promise<void>;
  loadApplications: (forceRefresh?: boolean) => Promise<void>;
  loadActionQueue: () => Promise<void>;
  loadActionHistory: () => Promise<void>;
  applyNotificationsResult: (notifications: NotificationRecord[], options?: NotificationRefreshOptions) => void;
  applyRulesResult: (rules: AutomationRule[]) => void;
  updateStatus: (status: AutomationStatus) => void;
  setNotice: (message: string) => void;
  setError: (message: string) => void;
};

export function useAutomationEvents({
  activeView,
  activeViewRef,
  refresh,
  loadApplications,
  loadActionQueue,
  loadActionHistory,
  applyNotificationsResult,
  applyRulesResult,
  updateStatus,
  setNotice,
  setError,
}: UseAutomationEventsOptions) {
  const notificationRefreshTimerRef = useRef<number | null>(null);
  const notificationRefreshInFlightRef = useRef(false);
  const ruleRefreshTimerRef = useRef<number | null>(null);
  const ruleRefreshInFlightRef = useRef(false);
  const pendingNotificationRefreshRef = useRef(false);
  const pendingNotificationRefreshOptionsRef = useRef<NotificationRefreshOptions>({});
  const pendingRuleRefreshRef = useRef(false);
  const scheduleNotificationRefreshRef = useRef<(options?: NotificationRefreshOptions) => void>(() => undefined);
  const scheduleRuleRefreshRef = useRef<() => void>(() => undefined);

  const scheduleNotificationRefresh = useCallback((options: NotificationRefreshOptions = {}) => {
    if (!shouldRefreshNotificationsForView(activeViewRef.current)) {
      pendingNotificationRefreshRef.current = true;
      pendingNotificationRefreshOptionsRef.current = mergeNotificationRefreshOptions(
        pendingNotificationRefreshOptionsRef.current,
        options,
      );
      return;
    }
    pendingNotificationRefreshRef.current = false;
    if (notificationRefreshTimerRef.current !== null) {
      window.clearTimeout(notificationRefreshTimerRef.current);
    }
    notificationRefreshTimerRef.current = window.setTimeout(async () => {
      notificationRefreshTimerRef.current = null;
      if (notificationRefreshInFlightRef.current) {
        scheduleNotificationRefreshRef.current(options);
        return;
      }
      notificationRefreshInFlightRef.current = true;
      try {
        const nextNotifications = await listNotifications(notificationFetchLimit);
        applyNotificationsResult(nextNotifications, options);
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        notificationRefreshInFlightRef.current = false;
      }
    }, 60);
  }, [activeViewRef, applyNotificationsResult, setError]);

  const scheduleRuleRefresh = useCallback(() => {
    if (activeViewRef.current !== "home") {
      pendingRuleRefreshRef.current = true;
      return;
    }
    pendingRuleRefreshRef.current = false;
    if (ruleRefreshTimerRef.current !== null) {
      window.clearTimeout(ruleRefreshTimerRef.current);
    }
    ruleRefreshTimerRef.current = window.setTimeout(async () => {
      ruleRefreshTimerRef.current = null;
      if (ruleRefreshInFlightRef.current) {
        scheduleRuleRefreshRef.current();
        return;
      }
      ruleRefreshInFlightRef.current = true;
      try {
        const nextRules = await listRules();
        applyRulesResult(nextRules);
      } catch {
        // Full refresh and status polling surface persistent failures; keep match feedback quiet.
      } finally {
        ruleRefreshInFlightRef.current = false;
      }
    }, 300);
  }, [activeViewRef, applyRulesResult]);

  useEffect(() => {
    scheduleNotificationRefreshRef.current = scheduleNotificationRefresh;
  }, [scheduleNotificationRefresh]);

  useEffect(() => {
    scheduleRuleRefreshRef.current = scheduleRuleRefresh;
  }, [scheduleRuleRefresh]);

  useEffect(() => {
    refresh();
    const applicationTimer = window.setTimeout(() => {
      loadApplications(false);
    }, 250);
    const timer = window.setInterval(() => {
      automationStatus().then(updateStatus).catch(() => undefined);
    }, 5000);
    const unlisten = listen<AutomationEvent>("noticeflow://automation", (event) => {
      if (event.payload.kind === "records") {
        scheduleNotificationRefresh({
          selectLatestNotification: true,
        });
      }
      if (event.payload.kind === "manual_refresh") {
        setError("");
        setNotice(event.payload.message);
        refresh({
          selectLatestPreview: true,
          selectLatestNotification: true,
        });
      }
      if (event.payload.kind === "status") {
        setError("");
        setNotice(event.payload.message);
      }
      if (event.payload.kind === "error") {
        setNotice("");
        setError(event.payload.message);
      }
      if (event.payload.kind === "match") {
        scheduleRuleRefresh();
      }
      if (event.payload.kind === "queue") {
        loadActionQueue().catch(() => undefined);
      }
      if (event.payload.kind === "action") {
        loadActionHistory().catch(() => undefined);
        loadActionQueue().catch(() => undefined);
      }
      if (event.payload.kind === "action_success") {
        setError("");
        setNotice(event.payload.message);
        loadActionHistory().catch(() => undefined);
        loadActionQueue().catch(() => undefined);
      }
      if (event.payload.kind === "action_error") {
        setNotice("");
        setError(event.payload.message);
        loadActionHistory().catch(() => undefined);
        loadActionQueue().catch(() => undefined);
      }
      if (event.payload.kind !== "records") {
        automationStatus().then(updateStatus).catch(() => undefined);
      }
    });
    return () => {
      window.clearTimeout(applicationTimer);
      window.clearInterval(timer);
      if (notificationRefreshTimerRef.current !== null) {
        window.clearTimeout(notificationRefreshTimerRef.current);
      }
      if (ruleRefreshTimerRef.current !== null) {
        window.clearTimeout(ruleRefreshTimerRef.current);
      }
      unlisten.then((dispose) => dispose()).catch(() => undefined);
    };
  }, [
    loadActionHistory,
    loadActionQueue,
    loadApplications,
    refresh,
    scheduleNotificationRefresh,
    scheduleRuleRefresh,
    setError,
    setNotice,
    updateStatus,
  ]);

  useEffect(() => {
    if (!shouldRefreshNotificationsForView(activeView)) return;
    if (!pendingNotificationRefreshRef.current) return;
    const options = pendingNotificationRefreshOptionsRef.current;
    pendingNotificationRefreshOptionsRef.current = {};
    scheduleNotificationRefresh(options);
  }, [activeView, scheduleNotificationRefresh]);

  useEffect(() => {
    if (activeView !== "home") return;
    if (!pendingRuleRefreshRef.current) return;
    scheduleRuleRefresh();
  }, [activeView, scheduleRuleRefresh]);
}

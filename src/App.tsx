import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Copy,
  EyeOff,
  RefreshCw,
  ShieldCheck,
  Trash2,
} from "lucide-react";
import { ConfirmDialog, ContextMenu } from "./components/AppShell";
import { RuleBoard, RuleCard } from "./features/rules/RuleBoard";
import { RuleEditor, PreviewPanel } from "./features/rules/RuleEditor";
import { NotificationsPage } from "./features/notifications/NotificationsPage";
import { ActionHistoryPage } from "./features/history/ActionHistoryPage";
import { SettingsPanel } from "./features/settings/SettingsPanel";
import { useAutoDismissNotice } from "./hooks/useAutoDismissNotice";
import { useAutomationEvents } from "./hooks/useAutomationEvents";
import { useGlobalMenuDismiss } from "./hooks/useGlobalMenuDismiss";
import { useNotificationActions } from "./hooks/useNotificationActions";
import { useNotificationCenter } from "./hooks/useNotificationCenter";
import { useRuleActions } from "./hooks/useRuleActions";
import { useRuleDrag } from "./hooks/useRuleDrag";
import { useRuleEditorDraft } from "./hooks/useRuleEditorDraft";
import { useRuleNavigation } from "./hooks/useRuleNavigation";
import { useRulePreview } from "./hooks/useRulePreview";
import { useRuleTestActions } from "./hooks/useRuleTestActions";
import { useSettingsMaintenanceActions } from "./hooks/useSettingsMaintenanceActions";
import { useSettingsViewRefresh } from "./hooks/useSettingsViewRefresh";
import {
  ActionHistoryEntry,
  ActionQueueStatus,
  ApplicationInfo,
  ArchiveStats,
  AutomationRule,
  AutomationStatus,
  NotificationRecord,
  SettingsInfo,
  SystemDeleteAuditEntry,
  actionHistory,
  actionQueueStatus,
  appSettings,
  archiveStats,
  automationStatus,
  listApplications,
  listNotifications,
  listRules,
  openFullDiskAccessSettings,
  revealPath,
  saveRules,
  systemDeleteAudit,
} from "./lib/tauri";
import {
  MainView,
  NotificationRefreshOptions,
  RuleEditorTab,
  RuleTestReport,
  applicationMap,
  cloneRule,
  emptyApplicationList,
  emptyNotificationList,
  notificationFetchLimit,
  permissionBannerMessage,
  readCachedApplications,
  sameApplicationList,
  sameNotificationList,
  sameRules,
  sameSettings,
  sameStatus,
  mergeNotificationRefreshOptions,
  validationIssues,
  variableNamesForRule,
  writeCachedApplications,
} from "./lib/appModel";

type ContextMenuState = {
  rule: AutomationRule;
  x: number;
  y: number;
} | null;

export default function App() {
  const [activeView, setActiveView] = useState<MainView>("home");
  const [rules, setRules] = useState<AutomationRule[]>([]);
  const [editingRule, setEditingRule] = useState<AutomationRule | null>(null);
  const [notifications, setNotifications] = useState<NotificationRecord[]>([]);
  const [actionHistoryItems, setActionHistoryItems] = useState<ActionHistoryEntry[]>([]);
  const [actionQueueInfo, setActionQueueInfo] = useState<ActionQueueStatus | null>(null);
  const [archiveStatsInfo, setArchiveStatsInfo] = useState<ArchiveStats | null>(null);
  const [systemDeleteAuditItems, setSystemDeleteAuditItems] = useState<SystemDeleteAuditEntry[]>([]);
  const [apps, setApps] = useState<ApplicationInfo[]>(() => readCachedApplications());
  const [settingsInfo, setSettingsInfo] = useState<SettingsInfo | null>(null);
  const [previewRecordId, setPreviewRecordId] = useState<number | null>(null);
  const [notificationRecordId, setNotificationRecordId] = useState<number | null>(null);
  const [status, setStatus] = useState<AutomationStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [editorTab, setEditorTab] = useState<RuleEditorTab>("basic");
  const [testReport, setTestReport] = useState<RuleTestReport | null>(null);
  const [contextMenu, setContextMenu] = useState<ContextMenuState>(null);
  const [editingIsDirty, setEditingIsDirty] = useState(false);
  const rulesRef = useRef<AutomationRule[]>([]);
  const editingRuleRef = useRef<AutomationRule | null>(null);
  const editingIsDirtyRef = useRef(false);
  const refreshInFlightRef = useRef(false);
  const refreshQueuedRef = useRef(false);
  const refreshQueuedOptionsRef = useRef<NotificationRefreshOptions>({});
  const applicationRefreshInFlightRef = useRef(false);
  const applicationRefreshQueuedRef = useRef(false);
  const applicationRefreshQueuedForceRef = useRef(false);
  const activeViewRef = useRef<MainView>("home");

  const updateStatus = useCallback((nextStatus: AutomationStatus) => {
    setStatus((current) => (sameStatus(current, nextStatus) ? current : nextStatus));
  }, []);

  const updateSettingsInfo = useCallback((nextSettingsInfo: SettingsInfo) => {
    setSettingsInfo((current) => (sameSettings(current, nextSettingsInfo) ? current : nextSettingsInfo));
  }, []);

  useEffect(() => {
    rulesRef.current = rules;
  }, [rules]);

  useEffect(() => {
    editingRuleRef.current = editingRule;
  }, [editingRule]);

  useEffect(() => {
    editingIsDirtyRef.current = editingIsDirty;
  }, [editingIsDirty]);

  useEffect(() => {
    activeViewRef.current = activeView;
  }, [activeView]);

  const appById = useMemo(() => applicationMap(apps), [apps]);

  const editingIssues = useMemo(() => validationIssues(editingRule, { allowDisabledDraft: true }), [editingRule]);
  const editingAppIdentifier = editingRule?.appIdentifiers?.[0] ?? "";

  const permissionMessage = useMemo(() => permissionBannerMessage(error, status), [error, status]);
  const persistenceLoadMessage = settingsInfo?.persistenceLoadError ?? "";
  const notificationDatabaseMessage = settingsInfo?.notificationDatabaseError ?? "";
  const systemMessage = persistenceLoadMessage || notificationDatabaseMessage || permissionMessage;
  const {
    notificationsByNewest,
    notificationSearchItems,
    notificationCenterItems,
    notificationQuery,
    setNotificationQuery,
    notificationAppFilter,
    setNotificationAppFilter,
  } = useNotificationCenter({
    activeView,
    notifications,
    selectedRecordId: notificationRecordId,
    setSelectedRecordId: setNotificationRecordId,
  });
  const {
    preview,
    previewQuery,
    setPreviewQuery,
    previewAppFilter,
    setPreviewAppFilter,
    filteredNotifications,
    previewFilterReady,
    resetPreviewFilters,
  } = useRulePreview({
    activeView,
    notificationsByNewest,
    notificationSearchItems,
    editingRule,
    editingAppIdentifier,
    previewRecordId,
    setPreviewRecordId,
    setError,
  });
  const variableNames = useMemo(() => variableNamesForRule(editingRule, preview), [editingRule, preview]);

  const applyNotificationsResult = useCallback((nextNotifications: NotificationRecord[], options: NotificationRefreshOptions = {}) => {
    setNotifications((current) => (sameNotificationList(current, nextNotifications) ? current : nextNotifications));
    const latestRecordId = nextNotifications[nextNotifications.length - 1]?.id ?? null;
    const hasRecord = (recordId: number | null) => recordId !== null && nextNotifications.some((item) => item.id === recordId);
    setPreviewRecordId((current) => {
      if (options.selectLatestPreview) return latestRecordId;
      return hasRecord(current) ? current : latestRecordId;
    });
    setNotificationRecordId((current) => {
      if (options.selectLatestNotification) return latestRecordId;
      return hasRecord(current) ? current : latestRecordId;
    });
  }, []);

  const runRefreshOnce = useCallback(async (options: NotificationRefreshOptions = {}) => {
    setError("");
    const [
      rulesResult,
      notificationsResult,
      statusResult,
      settingsResult,
      actionHistoryResult,
      actionQueueResult,
    ] = await Promise.allSettled([
      listRules(),
      listNotifications(notificationFetchLimit),
      automationStatus(),
      appSettings(true),
      actionHistory(),
      actionQueueStatus(),
    ]);

    if (rulesResult.status === "fulfilled") {
      rulesRef.current = rulesResult.value;
      setRules((current) => (sameRules(current, rulesResult.value) ? current : rulesResult.value));
      const currentEditingRule = editingRuleRef.current;
      if (currentEditingRule && !editingIsDirtyRef.current) {
        const fresh = rulesResult.value.find((rule) => rule.id === currentEditingRule.id);
        if (fresh) {
          const nextEditingRule = cloneRule(fresh);
          editingRuleRef.current = nextEditingRule;
          setEditingRule(nextEditingRule);
          setEditingIsDirty(false);
        }
      }
    }
    if (notificationsResult.status === "fulfilled") {
      applyNotificationsResult(notificationsResult.value, options);
    } else {
      setNotifications([]);
      setPreviewRecordId(null);
      setNotificationRecordId(null);
      setError(String(notificationsResult.reason));
    }
    if (statusResult.status === "fulfilled") updateStatus(statusResult.value);
    if (settingsResult.status === "fulfilled") updateSettingsInfo(settingsResult.value);
    if (actionHistoryResult.status === "fulfilled") setActionHistoryItems(actionHistoryResult.value);
    if (actionQueueResult.status === "fulfilled") setActionQueueInfo(actionQueueResult.value);

    const firstError = [rulesResult, statusResult, settingsResult, actionHistoryResult, actionQueueResult].find((result) => result.status === "rejected");
    if (firstError?.status === "rejected") {
      setError(String(firstError.reason));
    }
  }, [applyNotificationsResult, updateSettingsInfo, updateStatus]);

  const applyRulesResult = useCallback((nextRules: AutomationRule[]) => {
    rulesRef.current = nextRules;
    setRules((current) => (sameRules(current, nextRules) ? current : nextRules));
    const currentEditingRule = editingRuleRef.current;
    if (currentEditingRule && !editingIsDirtyRef.current) {
      const fresh = nextRules.find((rule) => rule.id === currentEditingRule.id);
      if (fresh) {
        const nextEditingRule = cloneRule(fresh);
        editingRuleRef.current = nextEditingRule;
        setEditingRule(nextEditingRule);
        setEditingIsDirty(false);
      }
    }
  }, []);

  const loadApplications = useCallback(async (forceRefresh = false) => {
    if (applicationRefreshInFlightRef.current) {
      applicationRefreshQueuedRef.current = true;
      applicationRefreshQueuedForceRef.current = applicationRefreshQueuedForceRef.current || forceRefresh;
      return;
    }
    applicationRefreshInFlightRef.current = true;
    try {
      let nextForceRefresh = forceRefresh;
      do {
        applicationRefreshQueuedRef.current = false;
        applicationRefreshQueuedForceRef.current = false;
        const nextApps = await listApplications(nextForceRefresh);
        writeCachedApplications(nextApps);
        setApps((current) => (sameApplicationList(current, nextApps) ? current : nextApps));
        nextForceRefresh = applicationRefreshQueuedForceRef.current;
      } while (applicationRefreshQueuedRef.current);
      if (forceRefresh) setError("");
    } catch (err) {
      if (forceRefresh) {
        setError(err instanceof Error ? err.message : String(err));
      }
    } finally {
      applicationRefreshInFlightRef.current = false;
    }
  }, []);

  const refresh = useCallback(async (options: NotificationRefreshOptions = {}) => {
    if (refreshInFlightRef.current) {
      refreshQueuedRef.current = true;
      refreshQueuedOptionsRef.current = mergeNotificationRefreshOptions(refreshQueuedOptionsRef.current, options);
      return;
    }
    refreshInFlightRef.current = true;
    setLoading(true);
    try {
      let nextOptions = options;
      do {
        refreshQueuedRef.current = false;
        refreshQueuedOptionsRef.current = {};
        await runRefreshOnce(nextOptions);
        nextOptions = refreshQueuedOptionsRef.current;
      } while (refreshQueuedRef.current);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      refreshInFlightRef.current = false;
      setLoading(false);
    }
  }, [runRefreshOnce]);

  const loadActionQueue = useCallback(async () => {
    const item = await actionQueueStatus();
    setActionQueueInfo(item);
  }, []);

  const loadSettingsMaintenance = useCallback(async () => {
    const [queueResult, statsResult, auditResult] = await Promise.allSettled([
      actionQueueStatus(),
      archiveStats(),
      systemDeleteAudit(),
    ]);
    if (queueResult.status === "fulfilled") setActionQueueInfo(queueResult.value);
    if (statsResult.status === "fulfilled") setArchiveStatsInfo(statsResult.value);
    if (auditResult.status === "fulfilled") setSystemDeleteAuditItems(auditResult.value);
    const firstError = [queueResult, statsResult, auditResult].find((result) => result.status === "rejected");
    if (firstError?.status === "rejected") throw firstError.reason;
  }, []);

  const refreshAll = useCallback(async () => {
    try {
      await Promise.all([refresh(), loadApplications(true), loadSettingsMaintenance()]);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [loadApplications, loadSettingsMaintenance, refresh, setError]);

  const loadActionHistory = useCallback(async () => {
    const items = await actionHistory();
    setActionHistoryItems(items);
  }, []);

  const {
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
  } = useNotificationActions({
    setNotifications,
    setNotificationRecordId,
    setPreviewRecordId,
    refresh,
    loadSettingsMaintenance,
    setNotice,
    setError,
  });

  const closeRuleContextMenu = useCallback(() => {
    setContextMenu(null);
  }, []);

  async function openPermissionSettingsFromBanner() {
    try {
      await openFullDiskAccessSettings();
      setError("");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }

  useAutomationEvents({
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
  });

  useSettingsViewRefresh({
    activeView,
    settingsInfo,
    updateSettingsInfo,
    loadSettingsMaintenance,
    setError,
  });

  useGlobalMenuDismiss({
    closeRuleMenu: closeRuleContextMenu,
    closeNotificationMenu,
  });
  useAutoDismissNotice(notice, setNotice);

  const handleDeletedEditingRule = useCallback(() => {
    activeViewRef.current = "home";
    editingRuleRef.current = null;
    editingIsDirtyRef.current = false;
    setEditingRule(null);
    setEditingIsDirty(false);
    setActiveView("home");
  }, []);

  const {
    pendingDeleteRuleId,
    saveEditingRule,
    toggleRule,
    requestDeleteRule,
    cancelDeleteRule,
    deleteRule,
    duplicateRule,
  } = useRuleActions({
    rules,
    rulesRef,
    setRules,
    editingRule,
    editingIsDirtyRef,
    setEditingRule,
    setEditingIsDirty,
    setEditorTab,
    closeRuleMenu: closeRuleContextMenu,
    onDeletedEditingRule: handleDeletedEditingRule,
    setNotice,
    setError,
  });

  const {
    clearHistory,
    compactArchiveData,
    pruneArchiveData,
  } = useSettingsMaintenanceActions({
    setActionHistoryItems,
    setArchiveStatsInfo,
    refresh,
    loadSettingsMaintenance,
    setNotice,
    setError,
  });

  const {
    dragging,
    startDrag,
    moveDrag,
    endDrag,
    isRuleClickSuppressed,
  } = useRuleDrag({
    rulesRef,
    setRules,
    saveRuleOrder: saveRules,
    onSaved: () => {
      setError("");
      setNotice("规则顺序已保存");
    },
    onError: (err) => {
      setError(err instanceof Error ? err.message : String(err));
      refresh();
    },
  });

  const {
    pendingEditorExitAction,
    cancelPendingEditorExit,
    confirmPendingEditorExit,
    selectRule,
    createRule,
    createRuleFromNotification,
    navigateTo,
    leaveEditor,
  } = useRuleNavigation({
    activeViewRef,
    editingRuleRef,
    editingIsDirtyRef,
    setActiveView,
    setEditingRule,
    setEditingIsDirty,
    setEditorTab,
    setPreviewRecordId,
    closeRuleMenu: closeRuleContextMenu,
    closeNotificationMenu,
    resetPreviewFilters,
    isRuleClickSuppressed,
    setNotice,
    setError,
  });

  const openRuleContextMenu = useCallback((rule: AutomationRule, x: number, y: number) => {
    setContextMenu({ rule, x, y });
  }, []);

  const handleSettingsSaved = useCallback((info: SettingsInfo) => {
    setSettingsInfo(info);
    setError("");
    setNotice("设置已保存");
    refreshAll();
  }, [refreshAll]);

  const {
    updateEditing,
    updateCondition,
    updateVariable,
    updateAction,
    updateActionParam,
  } = useRuleEditorDraft({
    editingRuleRef,
    editingIsDirtyRef,
    setEditingRule,
    setEditingIsDirty,
  });

  const { runTest, runExecute } = useRuleTestActions({
    editingRule,
    previewRecordId,
    editingIsDirty,
    setEditorTab,
    setTestReport,
    setNotice,
    setError,
    loadActionHistory,
    loadActionQueue,
  });

  const draggingRule = dragging ? rules.find((rule) => rule.id === dragging.id) : null;

  // 测试报告与当前编辑上下文绑定：切换规则、样本或离开编辑器后即失效。
  useEffect(() => {
    setTestReport(null);
  }, [editingRule?.id, previewRecordId]);
  useEffect(() => {
    if (activeView !== "editor") setTestReport(null);
  }, [activeView]);

  useEffect(() => {
    if (activeView !== "history") return;
    loadActionHistory().catch((err) => setError(err instanceof Error ? err.message : String(err)));
  }, [activeView, loadActionHistory]);

  return (
    <main className="flex h-full min-h-0 flex-col bg-[#f5f7f8] text-ink">
      {systemMessage ? (
        <div className="flex shrink-0 flex-wrap items-center justify-between gap-3 border-b border-amber-200 bg-amber-50 px-5 py-2 text-sm text-amber-800">
          <div className="flex min-w-0 items-center gap-2">
            <ShieldCheck size={16} className="shrink-0" />
            <span className="truncate">{systemMessage}</span>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            {permissionMessage ? (
              <button className="button-secondary h-8 border-amber-200 bg-white text-amber-900" onClick={openPermissionSettingsFromBanner}>
                打开权限设置
              </button>
            ) : null}
            <button className="button-secondary h-8 border-amber-200 bg-white text-amber-900" onClick={refreshAll} disabled={loading}>
              <RefreshCw size={14} className={loading ? "animate-spin" : ""} />
              刷新
            </button>
          </div>
        </div>
      ) : null}
      {error && !systemMessage ? <div className="border-b border-red-200 bg-red-50 px-5 py-2 text-sm text-red-700">{error}</div> : null}
      {notice && !error ? <div className="border-b border-emerald-200 bg-emerald-50 px-5 py-2 text-sm text-emerald-700">{notice}</div> : null}

      <section className="min-h-0 flex-1 p-4">
        <div className={activeView === "home" ? "h-full min-h-0" : "hidden h-full min-h-0"}>
          <MemoRuleBoard
            apps={apps}
            rules={rules}
            appById={appById}
            notifications={notificationCenterItems}
            notificationSelectedRecordId={notificationRecordId}
            setNotificationSelectedRecordId={setNotificationRecordId}
            notificationQuery={notificationQuery}
            setNotificationQuery={setNotificationQuery}
            notificationAppFilter={notificationAppFilter}
            setNotificationAppFilter={setNotificationAppFilter}
            draggingId={dragging?.id ?? ""}
            loading={loading}
            createRule={createRule}
            refreshAll={refreshAll}
            openSettings={() => navigateTo("settings")}
            openHistory={() => navigateTo("history")}
            selectRule={selectRule}
            toggleRule={toggleRule}
            startDrag={startDrag}
            moveDrag={moveDrag}
            endDrag={endDrag}
            openContextMenu={openRuleContextMenu}
            openNotificationMenu={openNotificationMenu}
          />
        </div>

        {activeView === "editor" && editingRule ? (
          <div className="grid h-full min-h-0 grid-cols-[minmax(440px,1.15fr)_minmax(300px,.85fr)] gap-4">
            <MemoRuleEditor
              rule={editingRule}
              apps={apps}
              variableNames={variableNames}
              preview={preview}
              selectedRecordId={previewRecordId}
              issues={editingIssues}
              tab={editorTab}
              setTab={setEditorTab}
              updateEditing={updateEditing}
              updateCondition={updateCondition}
              updateVariable={updateVariable}
              updateAction={updateAction}
              updateActionParam={updateActionParam}
              isDirty={editingIsDirty}
              canRunPreviewActions={!!previewRecordId}
              testReport={testReport}
              onSave={() => saveEditingRule()}
              onRunTest={runTest}
              onRunExecute={runExecute}
              onCloseTestReport={() => setTestReport(null)}
              onBack={leaveEditor}
            />
            <PreviewPanel
              apps={apps}
              notifications={previewFilterReady ? filteredNotifications : emptyNotificationList}
              loading={loading}
              refresh={() => refresh({ selectLatestPreview: true })}
              selectedRecordId={previewRecordId}
              setSelectedRecordId={setPreviewRecordId}
              preview={preview}
              query={previewQuery}
              setQuery={setPreviewQuery}
              appFilter={previewAppFilter}
              setAppFilter={setPreviewAppFilter}
              linkedApp={editingAppIdentifier}
              appById={appById}
            />
          </div>
        ) : null}

        <div className={activeView === "notifications" ? "h-full min-h-0" : "hidden h-full min-h-0"}>
          <MemoNotificationsPage
            apps={apps}
            appById={appById}
            notifications={notificationCenterItems}
            loading={loading}
            refresh={() => refresh({ selectLatestNotification: true })}
            selectedRecordId={notificationRecordId}
            setSelectedRecordId={setNotificationRecordId}
            query={notificationQuery}
            setQuery={setNotificationQuery}
            appFilter={notificationAppFilter}
            setAppFilter={setNotificationAppFilter}
            openMenu={openNotificationMenu}
            onBack={() => navigateTo("home")}
          />
        </div>

        <div className={activeView === "history" ? "h-full min-h-0" : "hidden h-full min-h-0"}>
          <ActionHistoryPage
            items={activeView === "history" ? actionHistoryItems : []}
            rules={rules}
            loading={loading}
            refresh={() => {
              loadActionHistory().catch((err) => setError(err instanceof Error ? err.message : String(err)));
            }}
            clearHistory={clearHistory}
            onBack={() => navigateTo("home")}
          />
        </div>

        <div className={activeView === "settings" ? "h-full min-h-0" : "hidden h-full min-h-0"}>
          <MemoSettingsPanel
            info={activeView === "settings" ? settingsInfo : null}
            status={activeView === "settings" ? status : null}
            apps={activeView === "settings" ? apps : emptyApplicationList}
            appCount={activeView === "settings" ? apps.length : 0}
            ruleCount={activeView === "settings" ? rules.length : 0}
            notificationCount={activeView === "settings" ? notifications.length : 0}
            actionQueue={activeView === "settings" ? actionQueueInfo : null}
            archiveStats={activeView === "settings" ? archiveStatsInfo : null}
            systemDeleteAudit={activeView === "settings" ? systemDeleteAuditItems : []}
            refresh={refreshAll}
            refreshMaintenance={loadSettingsMaintenance}
            revealPath={revealPath}
            openFullDiskAccessSettings={openFullDiskAccessSettings}
            restoreHiddenNotifications={restoreHiddenNotifications}
            compactArchive={compactArchiveData}
            pruneArchive={pruneArchiveData}
            onSaved={handleSettingsSaved}
            onBack={() => navigateTo("home")}
          />
        </div>
      </section>

      {contextMenu ? (
        <ContextMenu x={contextMenu.x} y={contextMenu.y}>
          <button className="block w-full px-3 py-2 text-left hover:bg-muted" onClick={() => {
            selectRule(contextMenu.rule);
            setContextMenu(null);
          }}>
            编辑
          </button>
          <button className="flex w-full items-center gap-2 px-3 py-2 text-left hover:bg-muted" onClick={() => duplicateRule(contextMenu.rule)}>
            <Copy size={14} />
            复制
          </button>
          <button className="block w-full px-3 py-2 text-left text-red-600 hover:bg-red-50" onClick={() => {
            requestDeleteRule(contextMenu.rule.id);
          }}>
            删除
          </button>
        </ContextMenu>
      ) : null}

      {notificationMenu ? (
        <ContextMenu x={notificationMenu.x} y={notificationMenu.y}>
          <button className="block w-full px-3 py-2 text-left hover:bg-muted" onClick={() => {
            setNotificationRecordId(notificationMenu.record.id);
            closeNotificationMenu();
            navigateTo("notifications");
          }}>
            查看详情
          </button>
          <button className="block w-full px-3 py-2 text-left hover:bg-muted" onClick={() => testNotification(notificationMenu.record)}>
            测试匹配
          </button>
          <button className="block w-full px-3 py-2 text-left hover:bg-muted" onClick={() => {
            closeNotificationMenu();
            createRuleFromNotification(notificationMenu.record);
          }}>
            基于此通知创建规则
          </button>
          <button className="flex w-full items-center gap-2 px-3 py-2 text-left hover:bg-muted" onClick={() => hideNotificationRecord(notificationMenu.record)}>
            <EyeOff size={14} />
            隐藏
          </button>
          <button className="flex w-full items-center gap-2 px-3 py-2 text-left text-red-600 hover:bg-red-50" onClick={() => requestDeleteNotification(notificationMenu.record)}>
            <Trash2 size={14} />
            系统删除
          </button>
        </ContextMenu>
      ) : null}

      {pendingEditorExitAction ? (
        <ConfirmDialog
          title="放弃未保存的规则？"
          message="当前规则还没有保存，返回或切换页面会丢弃这些修改。"
          cancelLabel="继续编辑"
          confirmLabel="放弃并离开"
          onCancel={cancelPendingEditorExit}
          onConfirm={confirmPendingEditorExit}
        />
      ) : null}

      {pendingDeleteRuleId ? (
        <ConfirmDialog
          title="删除这条规则？"
          message={`规则「${rules.find((rule) => rule.id === pendingDeleteRuleId)?.name || "未命名规则"}」删除后无法恢复。`}
          cancelLabel="取消"
          confirmLabel="删除"
          destructive
          onCancel={cancelDeleteRule}
          onConfirm={() => deleteRule(pendingDeleteRuleId)}
        />
      ) : null}

      {pendingDeleteNotification ? (
        <ConfirmDialog
          title="从系统通知历史删除？"
          message={`通知「${pendingDeleteNotification.title || "无标题"}」会从 macOS 通知记录库删除，并从 NoticeFlow 本地列表隐藏。`}
          cancelLabel="取消"
          confirmLabel="系统删除"
          destructive
          onCancel={cancelDeleteNotification}
          onConfirm={() => deleteNotificationRecord(pendingDeleteNotification)}
        />
      ) : null}

      {dragging && draggingRule ? (
        <div
          className="pointer-events-none fixed z-50 w-[280px]"
          style={{ left: dragging.x - dragging.offsetX, top: dragging.y - dragging.offsetY }}
        >
          <RuleCard rule={draggingRule} app={appById.get((draggingRule.appIdentifiers?.[0] ?? "").toLowerCase())} dragging />
        </div>
      ) : null}
    </main>
  );
}

const MemoRuleBoard = memo(RuleBoard);
const MemoNotificationsPage = memo(NotificationsPage);
const MemoSettingsPanel = memo(SettingsPanel);
const MemoRuleEditor = memo(RuleEditor);

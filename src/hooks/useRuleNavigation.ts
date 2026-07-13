import { startTransition, useCallback, useState } from "react";
import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import { newRule, cloneRule } from "../lib/appModel";
import type { MainView, RuleEditorTab } from "../lib/appModel";
import type { AutomationRule, NotificationRecord } from "../lib/tauri";

type PendingEditorExitAction =
  | { kind: "navigate"; view: Exclude<MainView, "editor"> }
  | { kind: "selectRule"; rule: AutomationRule }
  | { kind: "createRule" }
  | { kind: "createRuleFromNotification"; record: NotificationRecord };

type UseRuleNavigationOptions = {
  activeViewRef: MutableRefObject<MainView>;
  editingRuleRef: MutableRefObject<AutomationRule | null>;
  editingIsDirtyRef: MutableRefObject<boolean>;
  setActiveView: Dispatch<SetStateAction<MainView>>;
  setEditingRule: Dispatch<SetStateAction<AutomationRule | null>>;
  setEditingIsDirty: Dispatch<SetStateAction<boolean>>;
  setEditorTab: Dispatch<SetStateAction<RuleEditorTab>>;
  setPreviewRecordId: Dispatch<SetStateAction<number | null>>;
  closeRuleMenu: () => void;
  closeNotificationMenu: () => void;
  resetPreviewFilters: () => void;
  isRuleClickSuppressed: () => boolean;
  setNotice: (message: string) => void;
  setError: (message: string) => void;
};

export function useRuleNavigation({
  activeViewRef,
  editingRuleRef,
  editingIsDirtyRef,
  setActiveView,
  setEditingRule,
  setEditingIsDirty,
  setEditorTab,
  setPreviewRecordId,
  closeRuleMenu,
  closeNotificationMenu,
  resetPreviewFilters,
  isRuleClickSuppressed,
  setNotice,
  setError,
}: UseRuleNavigationOptions) {
  const [pendingEditorExitAction, setPendingEditorExitAction] = useState<PendingEditorExitAction | null>(null);

  const openRuleEditor = useCallback((rule: AutomationRule) => {
    const nextEditingRule = cloneRule(rule);
    activeViewRef.current = "editor";
    editingRuleRef.current = nextEditingRule;
    editingIsDirtyRef.current = false;
    setActiveView("editor");
    setEditorTab("basic");
    setEditingRule(nextEditingRule);
    setEditingIsDirty(false);
    setError("");
  }, [activeViewRef, editingIsDirtyRef, editingRuleRef, setActiveView, setEditingIsDirty, setEditingRule, setEditorTab, setError]);

  const openNewRuleEditor = useCallback((appIdentifier = "") => {
    const nextEditingRule = newRule(appIdentifier);
    activeViewRef.current = "editor";
    editingRuleRef.current = nextEditingRule;
    editingIsDirtyRef.current = false;
    setActiveView("editor");
    setEditorTab("basic");
    setEditingRule(nextEditingRule);
    setEditingIsDirty(false);
    setError("");
  }, [activeViewRef, editingIsDirtyRef, editingRuleRef, setActiveView, setEditingIsDirty, setEditingRule, setEditorTab, setError]);

  const openRuleFromNotification = useCallback((record: NotificationRecord) => {
    setPreviewRecordId(record.id);
    resetPreviewFilters();
    openNewRuleEditor(record.appIdentifier);
  }, [openNewRuleEditor, resetPreviewFilters, setPreviewRecordId]);

  const needsEditorExitConfirmation = useCallback((nextRuleId?: string) => {
    const currentRule = editingRuleRef.current;
    if (activeViewRef.current !== "editor" || !currentRule || !editingIsDirtyRef.current) {
      return false;
    }
    return !nextRuleId || currentRule.id !== nextRuleId;
  }, [activeViewRef, editingIsDirtyRef, editingRuleRef]);

  const selectRule = useCallback((rule: AutomationRule) => {
    if (isRuleClickSuppressed()) return;
    if (needsEditorExitConfirmation(rule.id)) {
      setPendingEditorExitAction({ kind: "selectRule", rule });
      return;
    }
    openRuleEditor(rule);
  }, [isRuleClickSuppressed, needsEditorExitConfirmation, openRuleEditor]);

  const createRule = useCallback(() => {
    if (needsEditorExitConfirmation()) {
      setPendingEditorExitAction({ kind: "createRule" });
      return;
    }
    openNewRuleEditor("");
  }, [needsEditorExitConfirmation, openNewRuleEditor]);

  const createRuleFromNotification = useCallback((record: NotificationRecord) => {
    if (needsEditorExitConfirmation()) {
      setPendingEditorExitAction({ kind: "createRuleFromNotification", record });
      return;
    }
    openRuleFromNotification(record);
  }, [needsEditorExitConfirmation, openRuleFromNotification]);

  const commitNavigation = useCallback((view: Exclude<MainView, "editor">) => {
    activeViewRef.current = view;
    editingRuleRef.current = null;
    editingIsDirtyRef.current = false;
    setActiveView(view);
    startTransition(() => {
      setEditingRule(null);
      setEditingIsDirty(false);
      setEditorTab("basic");
      closeRuleMenu();
      closeNotificationMenu();
      setError("");
      if (view === "home") setNotice("");
    });
  }, [
    activeViewRef,
    closeNotificationMenu,
    closeRuleMenu,
    editingIsDirtyRef,
    editingRuleRef,
    setActiveView,
    setEditingIsDirty,
    setEditingRule,
    setEditorTab,
    setError,
    setNotice,
  ]);

  const navigateTo = useCallback((view: Exclude<MainView, "editor">) => {
    const currentView = activeViewRef.current;
    const currentEditingRule = editingRuleRef.current;
    if (currentView === view && !currentEditingRule) return;
    if (currentView === "editor" && currentEditingRule && editingIsDirtyRef.current) {
      setPendingEditorExitAction({ kind: "navigate", view });
      return;
    }
    commitNavigation(view);
  }, [activeViewRef, commitNavigation, editingIsDirtyRef, editingRuleRef]);

  const leaveEditor = useCallback(() => {
    navigateTo("home");
  }, [navigateTo]);

  const continueAfterDiscardingEditor = useCallback((action: PendingEditorExitAction) => {
    switch (action.kind) {
      case "navigate":
        commitNavigation(action.view);
        break;
      case "selectRule":
        openRuleEditor(action.rule);
        break;
      case "createRule":
        openNewRuleEditor("");
        break;
      case "createRuleFromNotification":
        openRuleFromNotification(action.record);
        break;
    }
  }, [commitNavigation, openNewRuleEditor, openRuleEditor, openRuleFromNotification]);

  const cancelPendingEditorExit = useCallback(() => {
    setPendingEditorExitAction(null);
  }, []);

  const confirmPendingEditorExit = useCallback(() => {
    const action = pendingEditorExitAction;
    if (!action) return;
    setPendingEditorExitAction(null);
    continueAfterDiscardingEditor(action);
  }, [continueAfterDiscardingEditor, pendingEditorExitAction]);

  return {
    pendingEditorExitAction,
    cancelPendingEditorExit,
    confirmPendingEditorExit,
    selectRule,
    createRule,
    createRuleFromNotification,
    navigateTo,
    leaveEditor,
  };
}

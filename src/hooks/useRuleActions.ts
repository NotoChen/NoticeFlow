import { useCallback, useState } from "react";
import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import type { RuleEditorTab } from "../lib/appModel";
import { cloneRule, createRuleId, validationIssuesWithBackend } from "../lib/appModel";
import { saveRules, setRuleEnabled } from "../lib/tauri";
import type { AutomationRule } from "../lib/tauri";

type SaveEditingRuleOptions = {
  silent?: boolean;
};

type UseRuleActionsOptions = {
  rules: AutomationRule[];
  rulesRef: MutableRefObject<AutomationRule[]>;
  setRules: Dispatch<SetStateAction<AutomationRule[]>>;
  editingRule: AutomationRule | null;
  editingIsDirtyRef: MutableRefObject<boolean>;
  setEditingRule: Dispatch<SetStateAction<AutomationRule | null>>;
  setEditingIsDirty: Dispatch<SetStateAction<boolean>>;
  setEditorTab: Dispatch<SetStateAction<RuleEditorTab>>;
  closeRuleMenu: () => void;
  onDeletedEditingRule: () => void;
  setNotice: (message: string) => void;
  setError: (message: string) => void;
};

export function useRuleActions({
  rules,
  rulesRef,
  setRules,
  editingRule,
  editingIsDirtyRef,
  setEditingRule,
  setEditingIsDirty,
  setEditorTab,
  closeRuleMenu,
  onDeletedEditingRule,
  setNotice,
  setError,
}: UseRuleActionsOptions) {
  const [pendingDeleteRuleId, setPendingDeleteRuleId] = useState<string | null>(null);

  const persistRules = useCallback(async (nextRules: AutomationRule[]) => {
    await saveRules(nextRules);
    rulesRef.current = nextRules;
    setRules(nextRules);
  }, [rulesRef, setRules]);

  const saveEditingRule = useCallback(async (options: SaveEditingRuleOptions = {}): Promise<boolean> => {
    if (!editingRule) return false;
    const issues = await validationIssuesWithBackend(editingRule, { allowDisabledDraft: true });
    if (issues.length) {
      setEditorTab(issues[0].tab);
      setNotice("");
      setError(`请补齐：${issues.map((issue) => issue.label).join("、")}`);
      return false;
    }
    const nextRules = rules.some((rule) => rule.id === editingRule.id)
      ? rules.map((rule) => (rule.id === editingRule.id ? editingRule : rule))
      : [editingRule, ...rules];
    try {
      await persistRules(nextRules);
      editingIsDirtyRef.current = false;
      setEditingIsDirty(false);
      setError("");
      if (!options.silent) setNotice("规则已保存");
      return true;
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      return false;
    }
  }, [editingIsDirtyRef, editingRule, persistRules, rules, setEditingIsDirty, setEditorTab, setError, setNotice]);

  const toggleRule = useCallback(async (rule: AutomationRule, enabled: boolean) => {
    setRules((current) => {
      const next = current.map((item) => (item.id === rule.id ? { ...item, enabled } : item));
      rulesRef.current = next;
      return next;
    });
    setEditingRule((current) => (current?.id === rule.id ? { ...current, enabled } : current));
    try {
      await setRuleEnabled(rule.id, enabled);
      setError("");
      setNotice(enabled ? "规则已启用" : "规则已停用");
    } catch (err) {
      setRules((current) => {
        const next = current.map((item) => (item.id === rule.id ? { ...item, enabled: rule.enabled } : item));
        rulesRef.current = next;
        return next;
      });
      setEditingRule((current) => (current?.id === rule.id ? { ...current, enabled: rule.enabled } : current));
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [rulesRef, setEditingRule, setError, setNotice, setRules]);

  const requestDeleteRule = useCallback((ruleId: string) => {
    setPendingDeleteRuleId(ruleId);
    closeRuleMenu();
  }, [closeRuleMenu]);

  const cancelDeleteRule = useCallback(() => {
    setPendingDeleteRuleId(null);
  }, []);

  const deleteRule = useCallback(async (ruleId: string) => {
    const rule = rules.find((item) => item.id === ruleId);
    if (!rule) {
      setPendingDeleteRuleId(null);
      closeRuleMenu();
      return;
    }
    const nextRules = rules.filter((item) => item.id !== ruleId);
    try {
      await persistRules(nextRules);
      if (editingRule?.id === ruleId) onDeletedEditingRule();
      setPendingDeleteRuleId(null);
      closeRuleMenu();
      setError("");
      setNotice("规则已删除");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [closeRuleMenu, editingRule?.id, onDeletedEditingRule, persistRules, rules, setError, setNotice]);

  const duplicateRule = useCallback(async (rule: AutomationRule) => {
    const nextRule: AutomationRule = {
      ...cloneRule(rule),
      id: createRuleId(),
      name: `${rule.name || "未命名规则"} 副本`,
      enabled: rule.enabled ?? true,
      hitCount: 0,
    };
    try {
      await persistRules([nextRule, ...rules]);
      closeRuleMenu();
      setError("");
      setNotice("规则已复制");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [closeRuleMenu, persistRules, rules, setError, setNotice]);

  return {
    pendingDeleteRuleId,
    saveEditingRule,
    toggleRule,
    requestDeleteRule,
    cancelDeleteRule,
    deleteRule,
    duplicateRule,
  };
}

import { useCallback } from "react";
import type { Dispatch, MutableRefObject, SetStateAction } from "react";
import type {
  ActionConfig,
  AutomationRule,
  MatchCondition,
  VariableExtractionRule,
} from "../lib/tauri";

type UseRuleEditorDraftOptions = {
  editingRuleRef: MutableRefObject<AutomationRule | null>;
  editingIsDirtyRef: MutableRefObject<boolean>;
  setEditingRule: Dispatch<SetStateAction<AutomationRule | null>>;
  setEditingIsDirty: Dispatch<SetStateAction<boolean>>;
};

export function useRuleEditorDraft({
  editingRuleRef,
  editingIsDirtyRef,
  setEditingRule,
  setEditingIsDirty,
}: UseRuleEditorDraftOptions) {
  const markEditingDirty = useCallback(() => {
    editingIsDirtyRef.current = true;
    setEditingIsDirty(true);
  }, [editingIsDirtyRef, setEditingIsDirty]);

  const updateEditing = useCallback((patch: Partial<AutomationRule>) => {
    setEditingRule((current) => {
      if (!current) return current;
      const next = { ...current, ...patch };
      editingRuleRef.current = next;
      return next;
    });
    markEditingDirty();
  }, [editingRuleRef, markEditingDirty, setEditingRule]);

  const updateCondition = useCallback((index: number, patch: Partial<MatchCondition>) => {
    setEditingRule((current) => {
      if (!current) return current;
      const items = [...(current.matchConditions ?? [])];
      items[index] = { ...items[index], ...patch };
      const next = { ...current, matchConditions: items };
      editingRuleRef.current = next;
      return next;
    });
    markEditingDirty();
  }, [editingRuleRef, markEditingDirty, setEditingRule]);

  const updateVariable = useCallback((index: number, patch: Partial<VariableExtractionRule>) => {
    setEditingRule((current) => {
      if (!current) return current;
      const items = [...(current.variableExtractions ?? [])];
      items[index] = { ...items[index], ...patch };
      const next = { ...current, variableExtractions: items };
      editingRuleRef.current = next;
      return next;
    });
    markEditingDirty();
  }, [editingRuleRef, markEditingDirty, setEditingRule]);

  const updateAction = useCallback((index: number, patch: Partial<ActionConfig>) => {
    setEditingRule((current) => {
      if (!current) return current;
      const items = [...(current.actions ?? [])];
      items[index] = { ...items[index], ...patch };
      const next = { ...current, actions: items };
      editingRuleRef.current = next;
      return next;
    });
    markEditingDirty();
  }, [editingRuleRef, markEditingDirty, setEditingRule]);

  const updateActionParam = useCallback((index: number, key: string, value: string) => {
    setEditingRule((current) => {
      if (!current) return current;
      const action = (current.actions ?? [])[index];
      if (!action) return current;
      const items = [...(current.actions ?? [])];
      items[index] = { ...action, parameters: { ...action.parameters, [key]: value } };
      const next = { ...current, actions: items };
      editingRuleRef.current = next;
      return next;
    });
    markEditingDirty();
  }, [editingRuleRef, markEditingDirty, setEditingRule]);

  return {
    markEditingDirty,
    updateEditing,
    updateCondition,
    updateVariable,
    updateAction,
    updateActionParam,
  };
}

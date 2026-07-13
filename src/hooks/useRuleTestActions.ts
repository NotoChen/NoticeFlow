import { useCallback } from "react";
import type { Dispatch, SetStateAction } from "react";
import { validationIssuesWithBackend } from "../lib/appModel";
import {
  explainRuleDraftOnNotification,
  testRuleDraftOnNotification,
} from "../lib/tauri";
import type { AutomationRule, AutomationStatus, MatchExplanation } from "../lib/tauri";
import type { RuleEditorTab } from "../lib/appModel";

type UseRuleTestActionsOptions = {
  editingRule: AutomationRule | null;
  previewRecordId: number | null;
  editingIsDirty: boolean;
  setStatus: Dispatch<SetStateAction<AutomationStatus | null>>;
  setEditorTab: Dispatch<SetStateAction<RuleEditorTab>>;
  setNotice: (message: string) => void;
  setError: (message: string) => void;
  loadActionHistory: () => Promise<void>;
  loadActionQueue: () => Promise<void>;
};

export function useRuleTestActions({
  editingRule,
  previewRecordId,
  editingIsDirty,
  setStatus,
  setEditorTab,
  setNotice,
  setError,
  loadActionHistory,
  loadActionQueue,
}: UseRuleTestActionsOptions) {
  const appendStatusLogs = useCallback((logs: string[]) => {
    setStatus((current) => ({
      watcherRunning: current?.watcherRunning ?? true,
      lastRecordId: current?.lastRecordId ?? 0,
      logs: [...(current?.logs ?? []), ...logs].slice(-200),
    }));
  }, [setStatus]);

  const runMatchOnly = useCallback(async () => {
    if (!editingRule || !previewRecordId) return;
    const issues = (await validationIssuesWithBackend(editingRule)).filter((issue) => issue.tab !== "actions");
    if (issues.length) {
      setEditorTab(issues[0].tab);
      setNotice("");
      setError(`请补齐：${issues.map((issue) => issue.label).join("、")}`);
      return;
    }
    try {
      const explanation = await explainRuleDraftOnNotification(editingRule, previewRecordId);
      appendStatusLogs(matchExplanationLines(explanation));
      if (explanation.matched) {
        setError("");
        setNotice("匹配检查完成，未执行动作");
      } else {
        setNotice("");
        setError(explanation.message);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [appendStatusLogs, editingRule, previewRecordId, setEditorTab, setError, setNotice]);

  const runTest = useCallback(async () => {
    if (!editingRule || !previewRecordId) return;
    const issues = await validationIssuesWithBackend(editingRule);
    if (issues.length) {
      setEditorTab(issues[0].tab);
      setNotice("");
      setError(`请补齐：${issues.map((issue) => issue.label).join("、")}`);
      return;
    }
    try {
      const logs = await testRuleDraftOnNotification(editingRule, previewRecordId);
      appendStatusLogs(logs);
      setError("");
      setNotice(editingIsDirty ? "草稿测试完成，未保存修改仍保留" : "测试执行完成");
      loadActionHistory().catch(() => undefined);
      loadActionQueue().catch(() => undefined);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [
    appendStatusLogs,
    editingIsDirty,
    editingRule,
    loadActionHistory,
    loadActionQueue,
    previewRecordId,
    setEditorTab,
    setError,
    setNotice,
  ]);

  return {
    runMatchOnly,
    runTest,
  };
}

function matchExplanationLines(explanation: MatchExplanation) {
  const lines = [
    explanation.message,
    `应用：${passFail(explanation.appMatched)}；时间：${passFail(explanation.timeMatched)}；变量：${explanation.variableCount} 个`,
  ];
  for (const [index, condition] of explanation.conditions.entries()) {
    lines.push(
      `条件 ${index + 1}：${condition.variableName} ${operatorLabel(condition.operatorType)} ${condition.expectedValue || ""}，实际值：${shortValue(condition.actualValue)}，${passFail(condition.matched)}`,
    );
  }
  return lines;
}

function operatorLabel(value: string) {
  const labels: Record<string, string> = {
    equals: "等于",
    not_equals: "不等于",
    contains: "包含",
    not_contains: "不包含",
    starts_with: "开头是",
    ends_with: "结尾是",
    regex: "正则匹配",
    not_regex: "正则不匹配",
    is_empty: "为空",
    is_not_empty: "不为空",
  };
  return labels[value] ?? value;
}

function passFail(value: boolean) {
  return value ? "通过" : "未通过";
}

function shortValue(value: string) {
  const trimmed = value.trim();
  return trimmed.length > 120 ? `${trimmed.slice(0, 120)}...` : trimmed;
}

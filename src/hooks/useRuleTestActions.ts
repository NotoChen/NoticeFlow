import { useCallback } from "react";
import type { Dispatch, SetStateAction } from "react";
import { validationIssuesWithBackend } from "../lib/appModel";
import type { RuleEditorTab, RuleTestReport } from "../lib/appModel";
import {
  dryRunRuleDraftOnNotification,
  testRuleDraftOnNotification,
} from "../lib/tauri";
import type { AutomationRule } from "../lib/tauri";

type UseRuleTestActionsOptions = {
  editingRule: AutomationRule | null;
  previewRecordId: number | null;
  editingIsDirty: boolean;
  setEditorTab: Dispatch<SetStateAction<RuleEditorTab>>;
  setTestReport: Dispatch<SetStateAction<RuleTestReport | null>>;
  setNotice: (message: string) => void;
  setError: (message: string) => void;
  loadActionHistory: () => Promise<void>;
  loadActionQueue: () => Promise<void>;
};

export function useRuleTestActions({
  editingRule,
  previewRecordId,
  editingIsDirty,
  setEditorTab,
  setTestReport,
  setNotice,
  setError,
  loadActionHistory,
  loadActionQueue,
}: UseRuleTestActionsOptions) {
  const ensureReady = useCallback(async () => {
    if (!editingRule || !previewRecordId) return false;
    const issues = await validationIssuesWithBackend(editingRule);
    if (issues.length) {
      setEditorTab(issues[0].tab);
      setNotice("");
      setError(`请补齐：${issues.map((issue) => issue.label).join("、")}`);
      return false;
    }
    return true;
  }, [editingRule, previewRecordId, setEditorTab, setError, setNotice]);

  // 测试 = 干跑：解释匹配结果并预览变量替换后的动作参数，不执行动作。
  const runTest = useCallback(async () => {
    if (!editingRule || !previewRecordId) return;
    if (!(await ensureReady())) return;
    try {
      const report = await dryRunRuleDraftOnNotification(editingRule, previewRecordId);
      setTestReport({ kind: "dry", report });
      setError("");
      setNotice(
        report.explanation.matched
          ? "测试完成：动作未真正执行，请在结果面板中确认后再执行"
          : "测试完成：当前通知未命中该规则",
      );
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [editingRule, ensureReady, previewRecordId, setError, setNotice, setTestReport]);

  // 真实执行：动作会实际运行（脚本、HTTP 等有副作用），结果写入执行历史。
  const runExecute = useCallback(async () => {
    if (!editingRule || !previewRecordId) return;
    if (!(await ensureReady())) return;
    try {
      const executions = await testRuleDraftOnNotification(editingRule, previewRecordId);
      setTestReport({ kind: "executed", executions });
      setError("");
      const failed = executions.filter((item) => !item.success).length;
      const summary = failed
        ? `已执行 ${executions.length} 个动作，${failed} 个失败`
        : `已执行 ${executions.length} 个动作`;
      setNotice(editingIsDirty ? `${summary}（草稿修改仍未保存）` : summary);
      loadActionHistory().catch(() => undefined);
      loadActionQueue().catch(() => undefined);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [
    editingIsDirty,
    editingRule,
    ensureReady,
    loadActionHistory,
    loadActionQueue,
    previewRecordId,
    setError,
    setNotice,
    setTestReport,
  ]);

  return {
    runTest,
    runExecute,
  };
}

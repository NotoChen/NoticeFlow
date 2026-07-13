import type { ActionHistoryEntry } from "./tauri";

export const ACTION_TYPE_LABELS: Record<string, string> = {
  open_url: "打开链接",
  open_app: "打开应用",
  activate_app: "激活应用",
  send_notification: "本地通知",
  http_request: "HTTP 请求",
  run_shell: "Shell 脚本",
  run_applescript: "AppleScript",
  run_javascript: "JavaScript",
  run_python: "Python",
};

export function actionTypeLabel(type: string) {
  return ACTION_TYPE_LABELS[type] ?? type;
}

export function originLabel(origin: string) {
  if (origin === "test") return "测试";
  if (origin === "auto") return "自动";
  return origin;
}

export function formatHistoryTime(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

export function shortId(value: string) {
  return value.length <= 8 ? value : value.slice(0, 8);
}

export function parseVariableSnapshot(value: string | undefined) {
  if (!value) return [];
  try {
    const parsed = JSON.parse(value) as unknown;
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return [];
    return Object.entries(parsed)
      .filter((entry): entry is [string, string] => typeof entry[1] === "string")
      .sort(([left], [right]) => left.localeCompare(right));
  } catch {
    return [];
  }
}

export type HistoryStatusFilter = "all" | "success" | "failure";
export type HistoryOriginFilter = "all" | "auto" | "test";

export function filterActionHistory(
  items: ActionHistoryEntry[],
  ruleId: string,
  status: HistoryStatusFilter,
  origin: HistoryOriginFilter,
) {
  return items.filter((item) => {
    if (ruleId && item.ruleId !== ruleId) return false;
    if (status === "success" && !item.success) return false;
    if (status === "failure" && item.success) return false;
    if (origin !== "all" && (item.origin ?? "auto") !== origin) return false;
    return true;
  });
}

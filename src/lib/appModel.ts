import type {
  ActionConfig,
  ApplicationInfo,
  AutomationRule,
  AutomationStatus,
  MatchCondition,
  NotificationRecord,
  SettingsInfo,
  VariableExtractionRule,
  VariablePreview,
} from "./tauri";
import { validateRegex } from "./tauri";

export const baseVariables = ["app_id", "app_name", "title", "subtitle", "body", "url", "url_count", "urls", "urls_count", "urls_json", "timestamp"];

export type MainView = "home" | "editor" | "notifications" | "settings";
export type RuleEditorTab = "basic" | "match" | "variables" | "actions";
export type NotificationRefreshOptions = {
  selectLatestNotification?: boolean;
  selectLatestPreview?: boolean;
};
export type RuleIssue = {
  tab: RuleEditorTab;
  label: string;
};
type ValidationOptions = {
  allowDisabledDraft?: boolean;
};

export const notificationFetchLimit = 400;
export const emptyNotificationList: NotificationRecord[] = [];
export const emptyApplicationList: ApplicationInfo[] = [];

const applicationCacheStorageKey = "noticeflow.application-cache.v1";
const applicationCacheMaxAgeMs = 1000 * 60 * 60 * 24 * 14;
const applicationCacheMaxItems = 500;
let lastApplicationCacheSignature = "";

export function notificationSearchText(item: NotificationRecord) {
  return [item.title, item.subtitle, item.body, item.appName, item.appIdentifier].join("\n").toLowerCase();
}

const permissionErrorPattern = /authorization denied|operation not permitted|permission|unable to open database file/i;

export function applicationMap(apps: ApplicationInfo[]) {
  const map = new Map<string, ApplicationInfo>();
  for (const app of apps) {
    const key = app.bundleId.toLowerCase();
    const existing = map.get(key);
    if (!existing || (!existing.iconDataUrl && !existing.iconPath && (app.iconDataUrl || app.iconPath))) {
      map.set(key, app);
    }
  }
  return map;
}

export function permissionBannerMessage(error: string, status: AutomationStatus | null) {
  let latestError = "";
  const logs = status?.logs ?? [];
  for (let index = logs.length - 1; index >= 0; index -= 1) {
    const log = logs[index];
    if (permissionErrorPattern.test(log)) {
      latestError = log;
      break;
    }
  }
  if (!latestError && !permissionErrorPattern.test(error)) return "";
  return "需要为 NoticeFlow 开启完全磁盘访问，开启后刷新即可读取通知记录。";
}

export function variableNamesForRule(rule: AutomationRule | null | undefined, preview: VariablePreview) {
  const names = new Set(baseVariables);
  for (const item of rule?.variableExtractions ?? []) {
    if (item.name.trim()) names.add(item.name.trim());
  }
  for (const name of preview.displayNames) names.add(name);
  return Array.from(names);
}

export function newestNotifications(notifications: NotificationRecord[]) {
  return notifications.slice().sort((left, right) => {
    const leftTime = Date.parse(left.deliveredAt);
    const rightTime = Date.parse(right.deliveredAt);
    if (Number.isFinite(leftTime) && Number.isFinite(rightTime) && leftTime !== rightTime) {
      return rightTime - leftTime;
    }
    if (Number.isFinite(leftTime) !== Number.isFinite(rightTime)) {
      return Number.isFinite(rightTime) ? 1 : -1;
    }
    return right.id - left.id;
  });
}

export function notificationSearchIndex(notifications: NotificationRecord[]) {
  const index = new Map<number, string>();
  for (const item of notifications) {
    index.set(item.id, notificationSearchText(item));
  }
  return index;
}

export function filterNotifications(
  notifications: NotificationRecord[],
  searchIndex: Map<number, string>,
  query: string,
  appIdentifier: string,
) {
  const text = query.trim().toLowerCase();
  const app = appIdentifier.trim().toLowerCase();
  if (!app && !text) return notifications;
  return notifications.filter((item) => {
    if (app && item.appIdentifier.toLowerCase() !== app) return false;
    if (!text) return true;
    return (searchIndex.get(item.id) ?? "").includes(text);
  });
}

export function shouldRefreshNotificationsForView(view: MainView) {
  return view === "home" || view === "editor" || view === "notifications";
}

export function mergeNotificationRefreshOptions(
  left: NotificationRefreshOptions,
  right: NotificationRefreshOptions,
): NotificationRefreshOptions {
  return {
    selectLatestNotification: !!left.selectLatestNotification || !!right.selectLatestNotification,
    selectLatestPreview: !!left.selectLatestPreview || !!right.selectLatestPreview,
  };
}

export function sameNotificationList(left: NotificationRecord[], right: NotificationRecord[]) {
  if (left.length !== right.length) return false;
  return left.every((item, index) => {
    const other = right[index];
    if (!other) return false;
    return (
      item.id === other.id &&
      item.appIdentifier === other.appIdentifier &&
      item.appName === other.appName &&
      item.deliveredAt === other.deliveredAt &&
      item.title === other.title &&
      item.subtitle === other.subtitle &&
      item.body === other.body
    );
  });
}

export function sameApplicationList(left: ApplicationInfo[], right: ApplicationInfo[]) {
  if (left.length !== right.length) return false;
  return left.every((item, index) => {
    const other = right[index];
    if (!other) return false;
    return (
      item.bundleId === other.bundleId &&
      item.name === other.name &&
      item.path === other.path &&
      item.iconPath === other.iconPath &&
      item.iconCacheKey === other.iconCacheKey &&
      item.iconDataUrl === other.iconDataUrl
    );
  });
}

function applicationCacheSignature(apps: ApplicationInfo[]) {
  return apps
    .map((app) => [app.bundleId, app.name, app.path, app.iconPath ?? "", app.iconCacheKey ?? ""].join("\u001f"))
    .join("\u001e");
}

function normalizeApplicationForCache(app: ApplicationInfo): ApplicationInfo | null {
  const name = String(app.name ?? "").trim();
  const bundleId = String(app.bundleId ?? "").trim();
  const path = String(app.path ?? "").trim();
  if (!name || !bundleId || !path) return null;
  const iconPath = String(app.iconPath ?? "").trim();
  const iconCacheKey = String(app.iconCacheKey ?? "").trim();
  return {
    name,
    bundleId,
    path,
    iconPath: iconPath || undefined,
    iconCacheKey: iconCacheKey || undefined,
  };
}

export function readCachedApplications(): ApplicationInfo[] {
  try {
    const raw = window.localStorage.getItem(applicationCacheStorageKey);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as { savedAt?: number; apps?: ApplicationInfo[] };
    if (!parsed.savedAt || Date.now() - parsed.savedAt > applicationCacheMaxAgeMs) return [];
    if (!Array.isArray(parsed.apps)) return [];
    const apps = parsed.apps
      .map(normalizeApplicationForCache)
      .filter((app): app is ApplicationInfo => !!app)
      .slice(0, applicationCacheMaxItems);
    lastApplicationCacheSignature = applicationCacheSignature(apps);
    return apps;
  } catch {
    return [];
  }
}

export function writeCachedApplications(apps: ApplicationInfo[]) {
  try {
    const cachedApps = apps
      .map(normalizeApplicationForCache)
      .filter((app): app is ApplicationInfo => !!app)
      .slice(0, applicationCacheMaxItems);
    if (!cachedApps.length) return;
    const signature = applicationCacheSignature(cachedApps);
    if (signature === lastApplicationCacheSignature) return;
    window.localStorage.setItem(
      applicationCacheStorageKey,
      JSON.stringify({
        savedAt: Date.now(),
        apps: cachedApps,
      }),
    );
    lastApplicationCacheSignature = signature;
  } catch {
    // Cache is an optimization only; quota or privacy failures should not affect the app.
  }
}

export function sameStatus(left: AutomationStatus | null, right: AutomationStatus) {
  if (!left) return false;
  const leftLogs = left.logs ?? [];
  const rightLogs = right.logs ?? [];
  return (
    left.watcherRunning === right.watcherRunning &&
    left.lastRecordId === right.lastRecordId &&
    leftLogs.length === rightLogs.length &&
    leftLogs[leftLogs.length - 1] === rightLogs[rightLogs.length - 1]
  );
}

export function sameSettings(left: SettingsInfo | null, right: SettingsInfo) {
  if (!left) return false;
  return (
    left.notificationDatabasePath === right.notificationDatabasePath &&
    left.notificationDatabaseChecked === right.notificationDatabaseChecked &&
    left.notificationDatabaseAccessible === right.notificationDatabaseAccessible &&
    (left.notificationDatabaseError ?? "") === (right.notificationDatabaseError ?? "") &&
    left.dataDirectory === right.dataDirectory &&
    left.notificationArchivePath === right.notificationArchivePath &&
    left.rulesPath === right.rulesPath &&
    left.settingsPath === right.settingsPath &&
    left.launchAtLogin === right.launchAtLogin &&
    left.appFilterMode === right.appFilterMode &&
    left.watcherRunning === right.watcherRunning &&
    left.lastRecordId === right.lastRecordId &&
    (left.persistenceLoadError ?? "") === (right.persistenceLoadError ?? "") &&
    sameStringArray(left.ignoredAppIdentifiers, right.ignoredAppIdentifiers)
  );
}

export function sameRules(left: AutomationRule[], right: AutomationRule[]) {
  if (left.length !== right.length) return false;
  return left.every((rule, index) => sameRule(rule, right[index]));
}

function sameRule(left: AutomationRule, right: AutomationRule | undefined) {
  if (!right) return false;
  return (
    left.id === right.id &&
    left.name === right.name &&
    (left.enabled ?? true) === (right.enabled ?? true) &&
    (left.triggerTime ?? "") === (right.triggerTime ?? "") &&
    (left.cooldownSeconds ?? 0) === (right.cooldownSeconds ?? 0) &&
    (left.hitCount ?? 0) === (right.hitCount ?? 0) &&
    sameStringArray(left.appIdentifiers ?? [], right.appIdentifiers ?? []) &&
    sameMatchConditions(left.matchConditions ?? [], right.matchConditions ?? []) &&
    sameVariableExtractions(left.variableExtractions ?? [], right.variableExtractions ?? []) &&
    sameActions(left.actions ?? [], right.actions ?? [])
  );
}

function sameMatchConditions(left: MatchCondition[], right: MatchCondition[]) {
  if (left.length !== right.length) return false;
  return left.every((item, index) => {
    const other = right[index];
    return (
      item.variableName === other.variableName &&
      item.operatorType === other.operatorType &&
      (item.expectedValue ?? "") === (other.expectedValue ?? "") &&
      item.caseSensitive === other.caseSensitive
    );
  });
}

function sameVariableExtractions(left: VariableExtractionRule[], right: VariableExtractionRule[]) {
  if (left.length !== right.length) return false;
  return left.every((item, index) => {
    const other = right[index];
    return (
      item.name === other.name &&
      item.source === other.source &&
      item.method === other.method &&
      (item.pattern ?? "") === (other.pattern ?? "") &&
      (item.endPattern ?? "") === (other.endPattern ?? "") &&
      (item.groupIndex ?? 1) === (other.groupIndex ?? 1)
    );
  });
}

function sameActions(left: ActionConfig[], right: ActionConfig[]) {
  if (left.length !== right.length) return false;
  return left.every((item, index) => {
    const other = right[index];
    return item.type === other.type && sameStringRecord(item.parameters, other.parameters);
  });
}

function sameStringRecord(left: Record<string, string>, right: Record<string, string>) {
  const leftKeys = Object.keys(left);
  const rightKeys = Object.keys(right);
  if (leftKeys.length !== rightKeys.length) return false;
  return leftKeys.every((key) => left[key] === right[key]);
}

export function sameStringArray(left: string[], right: string[]) {
  if (left.length !== right.length) return false;
  return left.every((item, index) => item === right[index]);
}

export function sameStringSetIgnoreOrder(left: string[], right: string[]) {
  const leftValues = normalizedStringSet(left);
  const rightValues = normalizedStringSet(right);
  if (leftValues.size !== rightValues.size) return false;
  return Array.from(leftValues).every((item) => rightValues.has(item));
}

function normalizedStringSet(values: string[]) {
  return new Set(values.map((item) => item.trim().toLowerCase()).filter(Boolean));
}

export function emptyPreview(): VariablePreview {
  return { variables: {}, displayNames: baseVariables };
}

export function newRule(appId = ""): AutomationRule {
  return {
    id: createRuleId(),
    name: "",
    enabled: true,
    triggerTime: "",
    cooldownSeconds: 0,
    hitCount: 0,
    appIdentifiers: appId ? [appId] : [],
    matchConditions: [],
    variableExtractions: [],
    actions: [],
  };
}

export function cloneRule(rule: AutomationRule): AutomationRule {
  if (typeof structuredClone === "function") {
    return structuredClone(rule);
  }
  return JSON.parse(JSON.stringify(rule)) as AutomationRule;
}

export function createRuleId() {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  const bytes = new Uint8Array(16);
  if (typeof crypto !== "undefined" && typeof crypto.getRandomValues === "function") {
    crypto.getRandomValues(bytes);
  } else {
    for (let index = 0; index < bytes.length; index += 1) {
      bytes[index] = Math.floor(Math.random() * 256);
    }
  }
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0"));
  return `${hex.slice(0, 4).join("")}-${hex.slice(4, 6).join("")}-${hex.slice(6, 8).join("")}-${hex.slice(8, 10).join("")}-${hex.slice(10).join("")}`;
}

export function validationIssues(rule: AutomationRule | null | undefined, options: ValidationOptions = {}): RuleIssue[] {
  if (!rule) return [];
  if (options.allowDisabledDraft && rule.enabled === false) return [];
  const issues: RuleIssue[] = [];
  if (!rule.name.trim()) issues.push({ tab: "basic", label: "规则名称" });
  if (!(rule.appIdentifiers?.[0] ?? "").trim()) issues.push({ tab: "basic", label: "触发应用" });
  if (!(rule.matchConditions?.length ?? 0)) issues.push({ tab: "match", label: "匹配条件" });
  if (!(rule.actions?.length ?? 0)) issues.push({ tab: "actions", label: "触发动作" });
  for (const [index, condition] of (rule.matchConditions ?? []).entries()) {
    const label = `条件 ${index + 1}`;
    if (!condition.variableName.trim()) issues.push({ tab: "match", label: `${label}变量` });
    if (!validMatchOperators.has(condition.operatorType)) issues.push({ tab: "match", label: `${label}匹配方式` });
    if (matchOperatorNeedsValue(condition.operatorType) && !(condition.expectedValue ?? "").trim()) {
      issues.push({ tab: "match", label: `${label}匹配值` });
    }
  }
  const customVariableNames = new Set<string>();
  for (const [index, variable] of (rule.variableExtractions ?? []).entries()) {
    const label = `变量 ${index + 1}`;
    const name = variable.name.trim();
    if (!validVariableNamePattern.test(name)) {
      issues.push({ tab: "variables", label: `${label}名称` });
    } else {
      const key = name.toLowerCase();
      if (customVariableNames.has(key)) issues.push({ tab: "variables", label: `${label}重复名称` });
      customVariableNames.add(key);
    }
    if (variable.method === "regex") {
      if (!(variable.pattern ?? "").trim()) issues.push({ tab: "variables", label: `${label}正则` });
      if (!Number.isInteger(variable.groupIndex ?? 1) || (variable.groupIndex ?? 1) < 0) {
        issues.push({ tab: "variables", label: `${label}分组序号` });
      }
    }
  }
  for (const [index, action] of (rule.actions ?? []).entries()) {
    issues.push(...actionValidationIssues(action, index));
  }
  return issues;
}

export async function validationIssuesWithBackend(rule: AutomationRule | null | undefined, options: ValidationOptions = {}): Promise<RuleIssue[]> {
  const issues = validationIssues(rule, options);
  if (!rule || (options.allowDisabledDraft && rule.enabled === false)) return issues;
  for (const [index, condition] of (rule.matchConditions ?? []).entries()) {
    if (!["regex", "not_regex"].includes(condition.operatorType)) continue;
    const pattern = condition.expectedValue ?? "";
    if (!pattern.trim()) continue;
    try {
      await validateRegex(pattern, condition.caseSensitive);
    } catch {
      issues.push({ tab: "match", label: `条件 ${index + 1}正则格式` });
    }
  }
  for (const [index, variable] of (rule.variableExtractions ?? []).entries()) {
    if (variable.method !== "regex") continue;
    const pattern = variable.pattern ?? "";
    if (!pattern.trim()) continue;
    try {
      await validateRegex(pattern, true);
    } catch {
      issues.push({ tab: "variables", label: `变量 ${index + 1}正则格式` });
    }
  }
  return issues;
}

const validVariableNamePattern = /^[A-Za-z_][A-Za-z0-9_]*$/;
const validMatchOperators = new Set([
  "equals",
  "not_equals",
  "contains",
  "not_contains",
  "starts_with",
  "ends_with",
  "regex",
  "not_regex",
  "is_empty",
  "is_not_empty",
]);
const validActionTypes = new Set([
  "open_url",
  "open_app",
  "activate_app",
  "send_notification",
  "run_shell",
  "run_applescript",
  "run_javascript",
  "run_python",
  "http_request",
]);

function matchOperatorNeedsValue(operatorType: string) {
  return !["is_empty", "is_not_empty"].includes(operatorType);
}

function actionValidationIssues(action: ActionConfig, index: number): RuleIssue[] {
  const issues: RuleIssue[] = [];
  const label = `动作 ${index + 1}`;
  const parameters = action.parameters ?? {};
  if (!validActionTypes.has(action.type)) {
    issues.push({ tab: "actions", label: `${label}类型` });
    return issues;
  }
  if (timeoutIsInvalid(parameters.timeout_seconds ?? parameters.timeout)) {
    issues.push({ tab: "actions", label: `${label}超时秒数` });
  }
  if (jsonStringRecordIsInvalid(parameters.env_json ?? parameters.envJson)) {
    issues.push({ tab: "actions", label: `${label}环境变量 JSON` });
  }
  switch (action.type) {
    case "open_url":
      if (!stringParam(parameters.url).trim()) issues.push({ tab: "actions", label: `${label}URL` });
      break;
    case "open_app":
    case "activate_app":
      if (!stringParam(parameters.bundle_id ?? parameters.bundleId).trim()) issues.push({ tab: "actions", label: `${label}应用` });
      break;
    case "run_shell":
      if (!["", "bash", "zsh"].includes(stringParam(parameters.shell).trim().toLowerCase())) {
        issues.push({ tab: "actions", label: `${label}Shell` });
      }
      if (!["", "standard", "login", "interactive", "login_interactive", "login-interactive"].includes(stringParam(parameters.shell_mode ?? parameters.shellMode).trim().toLowerCase())) {
        issues.push({ tab: "actions", label: `${label}Shell 模式` });
      }
      if (!stringParam(parameters.script).trim()) issues.push({ tab: "actions", label: `${label}命令` });
      break;
    case "run_applescript":
    case "run_python":
      if (!stringParam(parameters.script).trim()) issues.push({ tab: "actions", label: `${label}脚本` });
      break;
    case "run_javascript":
      if (!stringParam(parameters.code).trim()) issues.push({ tab: "actions", label: `${label}代码` });
      break;
    case "http_request":
      if (!stringParam(parameters.url).trim()) issues.push({ tab: "actions", label: `${label}HTTP URL` });
      if (!/^[A-Za-z]+$/.test(stringParam(parameters.method || "GET").trim())) {
        issues.push({ tab: "actions", label: `${label}HTTP 方法` });
      }
      if (jsonStringRecordIsInvalid(parameters.headers)) {
        issues.push({ tab: "actions", label: `${label}Headers JSON` });
      }
      if (integerParamIsInvalid(parameters.retry_count ?? parameters.retryCount, 0, 5)) {
        issues.push({ tab: "actions", label: `${label}重试次数` });
      }
      if (integerParamIsInvalid(parameters.retry_interval_seconds ?? parameters.retryIntervalSeconds, 0, 60)) {
        issues.push({ tab: "actions", label: `${label}重试间隔` });
      }
      break;
  }
  return issues;
}

function stringParam(value: unknown) {
  return typeof value === "string" ? value : "";
}

function timeoutIsInvalid(value: unknown) {
  return integerParamIsInvalid(value, 1, 300);
}

function integerParamIsInvalid(value: unknown, min: number, max: number) {
  const text = stringParam(value).trim();
  if (!text) return false;
  if (!/^\d+$/.test(text)) return true;
  const parsed = Number(text);
  return !Number.isInteger(parsed) || parsed < min || parsed > max;
}

function jsonStringRecordIsInvalid(value: unknown) {
  const text = stringParam(value).trim();
  if (!text) return false;
  try {
    const parsed = JSON.parse(text) as unknown;
    return !isPlainStringRecord(parsed);
  } catch {
    return true;
  }
}

function isPlainStringRecord(value: unknown): value is Record<string, string> {
  return (
    typeof value === "object" &&
    value !== null &&
    !Array.isArray(value) &&
    Object.values(value).every((item) => typeof item === "string")
  );
}

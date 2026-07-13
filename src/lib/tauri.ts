import { invoke } from "@tauri-apps/api/core";

export type NotificationRecord = {
  id: number;
  appIdentifier: string;
  appName: string;
  deliveredAt: string;
  title: string;
  subtitle: string;
  body: string;
};

export type ApplicationInfo = {
  name: string;
  bundleId: string;
  path: string;
  iconPath?: string;
  iconCacheKey?: string;
  iconDataUrl?: string;
};

export type MatchCondition = {
  variableName: string;
  operatorType: string;
  expectedValue?: string;
  caseSensitive: boolean;
};

export type VariableExtractionRule = {
  name: string;
  source: "title" | "subtitle" | "body";
  method: "regex" | "between";
  pattern?: string;
  endPattern?: string;
  groupIndex?: number;
};

export type ActionConfig = {
  type: string;
  parameters: Record<string, string>;
};

export type AutomationRule = {
  id: string;
  name: string;
  enabled?: boolean;
  triggerTime?: string;
  cooldownSeconds?: number;
  hitCount?: number;
  appIdentifiers?: string[];
  matchConditions?: MatchCondition[];
  variableExtractions?: VariableExtractionRule[];
  actions?: ActionConfig[];
};

export type VariablePreview = {
  variables: Record<string, string>;
  displayNames: string[];
};

export type AutomationStatus = {
  watcherRunning: boolean;
  lastRecordId: number;
  logs: string[];
};

export type AutomationEvent = {
  kind: string;
  message: string;
};

export type ActionQueueItem = {
  id: string;
  queuedAt: string;
  ruleId: string;
  ruleName: string;
  notificationId: number;
  notificationTitle: string;
  appIdentifier: string;
  actionCount: number;
};

export type ActionQueueStatus = {
  pendingCount: number;
  maxPendingCount: number;
  running?: ActionQueueItem | null;
  pending: ActionQueueItem[];
};

export type SettingsInfo = {
  notificationDatabasePath: string;
  notificationDatabaseChecked: boolean;
  notificationDatabaseAccessible: boolean;
  notificationDatabaseError?: string;
  dataDirectory: string;
  notificationArchivePath: string;
  rulesPath: string;
  settingsPath: string;
  launchAtLogin: boolean;
  appFilterMode: "exclude" | "include";
  ignoredAppIdentifiers: string[];
  watcherRunning: boolean;
  lastRecordId: number;
  persistenceLoadError?: string;
};

export type ActionHistoryEntry = {
  id: string;
  timestamp: string;
  queueId?: string;
  ruleId: string;
  ruleName: string;
  notificationId: number;
  notificationTitle: string;
  appIdentifier: string;
  actionIndex: number;
  actionType: string;
  success: boolean;
  message: string;
  output?: string;
  origin: "auto" | "test" | string;
  durationMs: number;
  attemptCount: number;
  variablesJson?: string;
};

export type ArchiveStats = {
  path: string;
  sizeBytes: number;
  notificationCount: number;
  hiddenCount: number;
  systemDeletedCount: number;
  actionHistoryCount: number;
  systemDeleteAuditCount: number;
};

export type SystemDeleteAuditEntry = {
  id: string;
  timestamp: string;
  recordId: number;
  appIdentifier: string;
  appName: string;
  title: string;
  subtitle: string;
  body: string;
  systemRowsDeleted: number;
};

export type ConditionExplanation = {
  variableName: string;
  operatorType: string;
  expectedValue: string;
  actualValue: string;
  caseSensitive: boolean;
  matched: boolean;
};

export type MatchExplanation = {
  matched: boolean;
  ruleName: string;
  appMatched: boolean;
  timeMatched: boolean;
  variableCount: number;
  conditions: ConditionExplanation[];
  message: string;
};

export type ActionExecution = {
  actionType: string;
  success: boolean;
  message: string;
  output?: string;
  durationMs: number;
  attemptCount: number;
};

export type DryRunParameter = {
  name: string;
  value: string;
};

export type ActionDryRun = {
  actionType: string;
  parameters: DryRunParameter[];
};

export type RuleDryRunReport = {
  explanation: MatchExplanation;
  actions: ActionDryRun[];
};

export type SettingsUpdate = {
  launchAtLogin: boolean;
  dataDirectory?: string;
  appFilterMode?: "exclude" | "include";
  ignoredAppIdentifiers: string[];
};

export function listNotifications(limit = 200) {
  return invoke<NotificationRecord[]>("list_notifications", { limit });
}

export function listApplications(forceRefresh = false) {
  return invoke<ApplicationInfo[]>("list_applications", { forceRefresh });
}

export function applicationIcon(iconPath: string) {
  return invoke<string | null>("application_icon", { iconPath });
}

export function applicationIconForBundle(bundleId: string) {
  return invoke<string | null>("application_icon_for_bundle", { bundleId });
}

export function listRules() {
  return invoke<AutomationRule[]>("list_rules");
}

export function saveRules(rules: AutomationRule[]) {
  return invoke<void>("save_rules", { rules });
}

export function setRuleEnabled(ruleId: string, enabled: boolean) {
  return invoke<void>("set_rule_enabled", { ruleId, enabled });
}

export function validateRegex(pattern: string, caseSensitive = true) {
  return invoke<void>("validate_regex", { pattern, caseSensitive });
}

export function previewVariables(recordId: number, customRules: VariableExtractionRule[]) {
  return invoke<VariablePreview>("preview_variables", { recordId, customRules });
}

export function automationStatus() {
  return invoke<AutomationStatus>("automation_status");
}

export function actionQueueStatus() {
  return invoke<ActionQueueStatus>("action_queue_status");
}

export function actionHistory() {
  return invoke<ActionHistoryEntry[]>("action_history");
}

export function clearActionHistory() {
  return invoke<void>("clear_action_history");
}

export function archiveStats() {
  return invoke<ArchiveStats>("archive_stats");
}

export function compactArchive() {
  return invoke<ArchiveStats>("compact_archive");
}

export function pruneArchive(notificationRetentionDays = 90) {
  return invoke<ArchiveStats>("prune_archive", { notificationRetentionDays });
}

export function systemDeleteAudit() {
  return invoke<SystemDeleteAuditEntry[]>("system_delete_audit");
}

export function appSettings(deepCheck = false) {
  return invoke<SettingsInfo>("app_settings", { deepCheck });
}

export function saveAppSettings(update: SettingsUpdate) {
  return invoke<SettingsInfo>("save_app_settings", { update });
}

export function revealPath(path: string) {
  return invoke<void>("reveal_path", { path });
}

export function openFullDiskAccessSettings() {
  return invoke<void>("open_full_disk_access_settings");
}

export function chooseDataDirectory() {
  return invoke<string | null>("choose_data_directory");
}

export function chooseScriptFile() {
  return invoke<string | null>("choose_script_file");
}

export function hideNotification(record: NotificationRecord) {
  return invoke<void>("hide_notification", { record });
}

export function clearHiddenNotifications() {
  return invoke<void>("clear_hidden_notifications");
}

export function deleteSystemNotification(record: NotificationRecord) {
  return invoke<void>("delete_system_notification", { record });
}

export function matchRuleNamesForNotification(recordId: number) {
  return invoke<string[]>("match_rule_names_for_notification", { recordId });
}

export function testRuleOnNotification(ruleId: string, recordId: number) {
  return invoke<ActionExecution[]>("test_rule_on_notification", { ruleId, recordId });
}

export function testRuleDraftOnNotification(rule: AutomationRule, recordId: number) {
  return invoke<ActionExecution[]>("test_rule_draft_on_notification", { rule, recordId });
}

export function matchRuleDraftOnNotification(rule: AutomationRule, recordId: number) {
  return invoke<string[]>("match_rule_draft_on_notification", { rule, recordId });
}

export function explainRuleDraftOnNotification(rule: AutomationRule, recordId: number) {
  return invoke<MatchExplanation>("explain_rule_draft_on_notification", { rule, recordId });
}

export function dryRunRuleDraftOnNotification(rule: AutomationRule, recordId: number) {
  return invoke<RuleDryRunReport>("dry_run_rule_draft_on_notification", { rule, recordId });
}

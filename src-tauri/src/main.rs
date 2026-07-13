#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod action_runner;
mod app_catalog;
mod app_settings;
mod notification_archive;
mod notification_db;
mod rules;
mod variables;

use app_catalog::{
    is_known_application_icon_path, render_application_icon, render_application_icon_for_bundle,
    rescan_applications, scan_applications, ApplicationInfo,
};
use app_settings::{
    data_dir_for_settings, launch_at_login_enabled, load_settings, normalize_settings,
    save_settings, set_launch_at_login, settings_path, validate_data_directory, AppSettings,
};
use notification_archive::ActionHistoryEntry;
use notification_db::{
    delete_record as delete_system_notification_record, max_record_id,
    notification_database_corrupt_backup_path, notification_database_path,
    recent_records_excluding, recent_records_including, record_by_id, record_count, records_after,
    NotificationRecord,
};
use rules::{
    load_rules_from_dir, matching_rules, save_rule_file_in_dir, validate_regex_pattern,
    ActionConfig, AutomationRule, RuleFile,
};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, AtomicI64, Ordering},
    Mutex,
};
use std::thread;
use std::time::{Duration, Instant};
use tauri::image::Image;
use tauri::menu::MenuBuilder;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, RunEvent, State, WindowEvent};
use variables::{display_variable_names, extract_variables, VariableExtractionRule};

const HIT_COUNT_FLUSH_INTERVAL: Duration = Duration::from_secs(5);
const HIT_COUNT_FLUSH_THRESHOLD: u64 = 20;
const MAX_ACTION_HISTORY_ENTRIES: usize = 500;
const MAX_ACTION_QUEUE_ENTRIES: usize = 200;
const ACTION_WORKER_IDLE_INTERVAL: Duration = Duration::from_millis(50);
const WATCHER_IDLE_INTERVAL: Duration = Duration::from_millis(300);
const WATCHER_RECORD_BATCH_LIMIT: usize = 100;
const MAX_WATCHER_BATCHES_PER_TICK: usize = 10;
const DEFAULT_NOTIFICATION_LIST_LIMIT: usize = 200;
const MAX_NOTIFICATION_LIST_LIMIT: usize = 1_000;
const MAX_ACTION_TIMEOUT_SECONDS: u64 = 300;
const MAX_HTTP_RETRY_COUNT: u64 = 5;
const MAX_HTTP_RETRY_INTERVAL_SECONDS: u64 = 60;
const AUTOMATION_RESULT_SUCCESS_TITLE: &str = "NoticeFlow 动作完成";
const AUTOMATION_RESULT_FAILURE_TITLE: &str = "NoticeFlow 动作失败";
const AUTOMATION_RESULT_SUBTITLE: &str = "NoticeFlow Automation";
const TRAY_OPEN_MENU_ID: &str = "noticeflow_tray_open";
const TRAY_REFRESH_MENU_ID: &str = "noticeflow_tray_refresh";
const TRAY_QUIT_MENU_ID: &str = "noticeflow_tray_quit";

struct AppState {
    rules: Mutex<Vec<AutomationRule>>,
    settings: Mutex<AppSettings>,
    persistence_load_errors: Mutex<PersistenceLoadErrors>,
    logs: Mutex<Vec<String>>,
    action_history: Mutex<Vec<ActionHistoryEntry>>,
    action_queue: Mutex<VecDeque<ActionJob>>,
    current_action_job: Mutex<Option<ActionQueueItem>>,
    rule_cooldowns: Mutex<HashMap<String, Instant>>,
    pending_hit_counts: Mutex<HashMap<String, u64>>,
    last_hit_count_save: Mutex<Instant>,
    action_worker_running: AtomicBool,
    watcher_running: AtomicBool,
    last_record_id: AtomicI64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct PersistenceLoadErrors {
    rules: Option<String>,
    settings: Option<String>,
}

#[derive(Clone, Copy)]
enum PersistenceDomain {
    Rules,
    Any,
}

#[derive(Clone)]
struct ActionJob {
    id: String,
    queued_at: String,
    rule: AutomationRule,
    record: NotificationRecord,
    variables: BTreeMap<String, String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ActionQueueItem {
    id: String,
    queued_at: String,
    rule_id: String,
    rule_name: String,
    notification_id: i64,
    notification_title: String,
    app_identifier: String,
    action_count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ActionQueueStatus {
    pending_count: usize,
    max_pending_count: usize,
    running: Option<ActionQueueItem>,
    pending: Vec<ActionQueueItem>,
}

struct RuleMatchAnalysis {
    explanation: MatchExplanation,
    variables: BTreeMap<String, String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MatchExplanation {
    matched: bool,
    rule_name: String,
    app_matched: bool,
    time_matched: bool,
    variable_count: usize,
    conditions: Vec<ConditionExplanation>,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConditionExplanation {
    variable_name: String,
    operator_type: String,
    expected_value: String,
    actual_value: String,
    case_sensitive: bool,
    matched: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VariablePreview {
    variables: std::collections::BTreeMap<String, String>,
    display_names: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AutomationEvent {
    kind: String,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AutomationStatus {
    watcher_running: bool,
    last_record_id: i64,
    logs: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsInfo {
    notification_database_path: String,
    notification_database_checked: bool,
    notification_database_accessible: bool,
    notification_database_error: Option<String>,
    data_directory: String,
    notification_archive_path: String,
    rules_path: String,
    settings_path: String,
    launch_at_login: bool,
    app_filter_mode: String,
    ignored_app_identifiers: Vec<String>,
    watcher_running: bool,
    last_record_id: i64,
    persistence_load_error: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsUpdate {
    launch_at_login: bool,
    data_directory: Option<String>,
    app_filter_mode: Option<String>,
    ignored_app_identifiers: Vec<String>,
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct NotificationRecordIdentity {
    id: i64,
    app_identifier: String,
    app_name: String,
    delivered_at: String,
    title: String,
    subtitle: String,
    body: String,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum AppFilterMode {
    Exclude,
    Include,
}

#[tauri::command]
fn list_notifications(
    limit: Option<usize>,
    state: State<AppState>,
) -> Result<Vec<NotificationRecord>, String> {
    let filter_apps = filter_app_identifiers(&state);
    let filter_mode = app_filter_mode(&state);
    let limit = normalized_notification_list_limit(limit);
    let system_result = match filter_mode {
        AppFilterMode::Exclude => recent_records_excluding(limit, &filter_apps),
        AppFilterMode::Include => recent_records_including(limit, &filter_apps),
    };

    match system_result {
        Ok(system_records) => {
            if let Err(error) = notification_archive::upsert_records(&system_records) {
                push_log(
                    &state,
                    format!("同步本地归档失败，临时使用系统通知结果：{error}"),
                );
                return Ok(system_records);
            }

            match archived_notifications(limit, filter_mode, &filter_apps) {
                Ok(records) => Ok(records),
                Err(error) => {
                    push_log(
                        &state,
                        format!("读取本地归档失败，临时使用系统通知结果：{error}"),
                    );
                    Ok(system_records)
                }
            }
        }
        Err(system_error) => match archived_notifications(limit, filter_mode, &filter_apps) {
            Ok(records) if !records.is_empty() => Ok(records),
            Ok(_) => Err(system_error.to_string()),
            Err(archive_error) => Err(format!(
                "读取系统通知失败：{system_error}；读取本地归档失败：{archive_error}"
            )),
        },
    }
}

fn normalized_notification_list_limit(limit: Option<usize>) -> usize {
    limit
        .unwrap_or(DEFAULT_NOTIFICATION_LIST_LIMIT)
        .min(MAX_NOTIFICATION_LIST_LIMIT)
}

fn archived_notifications(
    limit: usize,
    filter_mode: AppFilterMode,
    filter_apps: &[String],
) -> Result<Vec<NotificationRecord>, Box<dyn std::error::Error>> {
    match filter_mode {
        AppFilterMode::Exclude => {
            notification_archive::recent_records_excluding(limit, filter_apps)
        }
        AppFilterMode::Include => {
            notification_archive::recent_records_including(limit, filter_apps)
        }
    }
}

#[tauri::command]
fn list_applications(force_refresh: Option<bool>) -> Result<Vec<ApplicationInfo>, String> {
    if force_refresh.unwrap_or(false) {
        rescan_applications().map_err(|error| error.to_string())
    } else {
        scan_applications().map_err(|error| error.to_string())
    }
}

#[tauri::command]
fn application_icon(icon_path: String) -> Result<Option<String>, String> {
    if !is_known_application_icon_path(&icon_path).map_err(|error| error.to_string())? {
        return Err("不允许读取未扫描应用的图标路径".to_string());
    }
    render_application_icon(&icon_path).map_err(|error| error.to_string())
}

#[tauri::command]
fn application_icon_for_bundle(bundle_id: String) -> Result<Option<String>, String> {
    render_application_icon_for_bundle(&bundle_id).map_err(|error| error.to_string())
}

fn ensure_persistence_loaded(
    state: &State<AppState>,
    domain: PersistenceDomain,
) -> Result<(), String> {
    let errors = state
        .persistence_load_errors
        .lock()
        .map_err(|_| "持久化加载错误状态已损坏".to_string())?;
    let message = match domain {
        PersistenceDomain::Rules => errors.rules.as_deref().map(|error| {
            format!("规则文件加载失败，为避免覆盖原文件，已阻止保存。请修复 rules.json 后重启应用：{error}")
        }),
        PersistenceDomain::Any => persistence_load_error_message(&errors),
    };
    match message {
        Some(message) => Err(message),
        None => Ok(()),
    }
}

fn persistence_load_error_message(errors: &PersistenceLoadErrors) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(error) = errors.rules.as_deref() {
        parts.push(format!("rules.json：{error}"));
    }
    if let Some(error) = errors.settings.as_deref() {
        parts.push(format!("settings.json：{error}"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(format!(
            "配置加载失败，为避免覆盖原文件，保存已被阻止。请修复文件后重启应用。{}",
            parts.join("；")
        ))
    }
}

fn data_directory_for_state(state: &AppState) -> Result<PathBuf, String> {
    let settings = state
        .settings
        .lock()
        .map_err(|_| "设置状态已损坏".to_string())?
        .clone();
    data_dir_for_settings(&settings).map_err(|error| error.to_string())
}

#[tauri::command]
fn list_rules(state: State<AppState>) -> Result<Vec<AutomationRule>, String> {
    state
        .rules
        .lock()
        .map(|rules| rules.clone())
        .map_err(|_| "规则状态已损坏".to_string())
}

#[tauri::command]
fn save_rules(mut rules: Vec<AutomationRule>, state: State<AppState>) -> Result<(), String> {
    ensure_persistence_loaded(&state, PersistenceDomain::Rules)?;
    validate_rules_for_save(&rules)?;
    let data_directory = data_directory_for_state(&state)?;
    let mut state_rules = state
        .rules
        .lock()
        .map_err(|_| "规则状态已损坏".to_string())?;
    for rule in &mut rules {
        if let Some(current_rule) = state_rules.iter().find(|current| current.id == rule.id) {
            let current_hit_count = current_rule.hit_count.unwrap_or(0);
            if current_hit_count > rule.hit_count.unwrap_or(0) {
                rule.hit_count = Some(current_hit_count);
            }
        }
    }
    save_rule_file_in_dir(&data_directory, &rules).map_err(|error| error.to_string())?;
    *state_rules = rules;
    if let Ok(mut pending_hit_counts) = state.pending_hit_counts.lock() {
        pending_hit_counts.clear();
    }
    if let Ok(mut cooldowns) = state.rule_cooldowns.lock() {
        cooldowns.clear();
    }
    if let Ok(mut last_save) = state.last_hit_count_save.lock() {
        *last_save = Instant::now();
    }
    Ok(())
}

#[tauri::command]
fn set_rule_enabled(rule_id: String, enabled: bool, state: State<AppState>) -> Result<(), String> {
    ensure_persistence_loaded(&state, PersistenceDomain::Rules)?;
    let data_directory = data_directory_for_state(&state)?;
    let mut rules = state
        .rules
        .lock()
        .map_err(|_| "规则状态已损坏".to_string())?;
    let mut next_rules = rules.clone();
    let Some(rule) = next_rules.iter_mut().find(|rule| rule.id == rule_id) else {
        return Err("未找到规则".to_string());
    };
    if enabled {
        validate_rule_for_save(rule).map_err(|error| format!("规则「{}」{error}", rule.name))?;
    }
    rule.enabled = Some(enabled);
    save_rule_file_in_dir(&data_directory, &next_rules).map_err(|error| error.to_string())?;
    *rules = next_rules;
    Ok(())
}

#[tauri::command]
fn validate_regex(pattern: String, case_sensitive: Option<bool>) -> Result<(), String> {
    validate_regex_pattern(&pattern, case_sensitive.unwrap_or(true))
}

fn validate_rule_regexes(rule: &AutomationRule) -> Result<(), String> {
    for (index, condition) in rule
        .match_conditions
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        if !matches!(condition.operator_type.as_str(), "regex" | "not_regex") {
            continue;
        }
        let pattern = condition.expected_value.as_deref().unwrap_or_default();
        validate_regex_pattern(pattern, condition.case_sensitive)
            .map_err(|error| format!("条件 {} 正则无效：{error}", index + 1))?;
    }

    for (index, variable) in rule
        .variable_extractions
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        if !matches!(&variable.method, variables::VariableExtractionMethod::Regex) {
            continue;
        }
        let pattern = variable.pattern.as_deref().unwrap_or_default();
        validate_regex_pattern(pattern, true)
            .map_err(|error| format!("变量 {} 正则无效：{error}", index + 1))?;
    }

    Ok(())
}

fn validate_rule_for_save(rule: &AutomationRule) -> Result<(), String> {
    if rule.name.trim().is_empty() {
        return Err("规则名称为空".to_string());
    }
    if !rule_has_trigger_app(rule) {
        return Err("触发应用为空".to_string());
    }
    if !rule_has_match_conditions(rule) {
        return Err("匹配条件为空".to_string());
    }
    if !rule_has_actions(rule) {
        return Err("触发动作为空".to_string());
    }
    validate_match_conditions_for_save(rule)?;
    validate_variable_extractions_for_save(rule)?;
    validate_actions_for_save(rule)?;
    validate_rule_regexes(rule)
}

fn validate_rules_for_save(rules: &[AutomationRule]) -> Result<(), String> {
    for rule in rules {
        if !rule.is_enabled() {
            continue;
        }
        validate_rule_for_save(rule).map_err(|error| format!("规则「{}」{}", rule.name, error))?;
    }
    Ok(())
}

fn validate_match_conditions_for_save(rule: &AutomationRule) -> Result<(), String> {
    for (index, condition) in rule
        .match_conditions
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        let label = format!("条件 {}", index + 1);
        if condition.variable_name.trim().is_empty() {
            return Err(format!("{label}变量为空"));
        }
        if !is_valid_match_operator(&condition.operator_type) {
            return Err(format!("{label}匹配方式无效"));
        }
        if match_operator_needs_value(&condition.operator_type)
            && condition
                .expected_value
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
        {
            return Err(format!("{label}匹配值为空"));
        }
    }
    Ok(())
}

fn validate_variable_extractions_for_save(rule: &AutomationRule) -> Result<(), String> {
    let mut names = HashSet::new();
    for (index, variable) in rule
        .variable_extractions
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        let label = format!("变量 {}", index + 1);
        let name = variable.name.trim();
        if !variables::is_valid_variable_name(name) {
            return Err(format!("{label}名称无效"));
        }
        if !names.insert(name.to_lowercase()) {
            return Err(format!("{label}名称重复"));
        }
        if matches!(&variable.method, variables::VariableExtractionMethod::Regex)
            && variable
                .pattern
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
        {
            return Err(format!("{label}正则为空"));
        }
    }
    Ok(())
}

fn validate_actions_for_save(rule: &AutomationRule) -> Result<(), String> {
    for (index, action) in rule.actions.as_deref().unwrap_or(&[]).iter().enumerate() {
        validate_action_for_save(action, index)?;
    }
    Ok(())
}

fn validate_action_for_save(action: &ActionConfig, index: usize) -> Result<(), String> {
    let label = format!("动作 {}", index + 1);
    if !is_valid_action_type(&action.action_type) {
        return Err(format!("{label}类型无效"));
    }
    validate_action_common_parameters(action, &label)?;
    match action.action_type.as_str() {
        "open_url" => {
            require_action_parameter(action, &["url", "url_pattern"], &format!("{label}URL"))?;
        }
        "open_app" | "activate_app" => {
            require_action_parameter(action, &["bundle_id", "bundleId"], &format!("{label}应用"))?;
        }
        "send_notification" => {}
        "run_shell" => {
            validate_shell_action(action, &label)?;
            require_action_parameter(action, &["script"], &format!("{label}命令"))?;
        }
        "run_applescript" | "run_python" => {
            require_action_parameter(action, &["script"], &format!("{label}脚本"))?;
        }
        "run_javascript" => {
            require_action_parameter(action, &["code"], &format!("{label}代码"))?;
        }
        "http_request" => {
            validate_http_action(action, &label)?;
        }
        _ => unreachable!("validated action type should be exhaustive"),
    }
    Ok(())
}

fn validate_action_common_parameters(action: &ActionConfig, label: &str) -> Result<(), String> {
    validate_integer_parameter(
        action,
        &["timeout_seconds", "timeout"],
        1,
        MAX_ACTION_TIMEOUT_SECONDS,
        &format!("{label}超时秒数"),
    )?;
    validate_json_string_record_parameter(
        action,
        &["env_json", "envJson"],
        &format!("{label}环境变量 JSON"),
    )
}

fn validate_shell_action(action: &ActionConfig, label: &str) -> Result<(), String> {
    let shell = action_parameter(action, &["shell"])
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    if !matches!(shell.as_str(), "" | "bash" | "zsh") {
        return Err(format!("{label}Shell 无效"));
    }

    let shell_mode = action_parameter(action, &["shell_mode", "shellMode"])
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    if !matches!(
        shell_mode.as_str(),
        "" | "standard" | "login" | "interactive" | "login_interactive" | "login-interactive"
    ) {
        return Err(format!("{label}Shell 模式无效"));
    }
    Ok(())
}

fn validate_http_action(action: &ActionConfig, label: &str) -> Result<(), String> {
    require_action_parameter(action, &["url"], &format!("{label}HTTP URL"))?;
    let method = action_parameter(action, &["method"])
        .unwrap_or_default()
        .trim();
    if !method.is_empty()
        && !method
            .chars()
            .all(|character| character.is_ascii_alphabetic())
    {
        return Err(format!("{label}HTTP 方法无效"));
    }
    validate_json_string_record_parameter(action, &["headers"], &format!("{label}Headers JSON"))?;
    validate_integer_parameter(
        action,
        &["retry_count", "retryCount"],
        0,
        MAX_HTTP_RETRY_COUNT,
        &format!("{label}重试次数"),
    )?;
    validate_integer_parameter(
        action,
        &["retry_interval_seconds", "retryIntervalSeconds"],
        0,
        MAX_HTTP_RETRY_INTERVAL_SECONDS,
        &format!("{label}重试间隔"),
    )
}

fn require_action_parameter(
    action: &ActionConfig,
    keys: &[&str],
    label: &str,
) -> Result<(), String> {
    if action_parameter(action, keys).is_some_and(|value| !value.trim().is_empty()) {
        Ok(())
    } else {
        Err(format!("{label}为空"))
    }
}

fn validate_integer_parameter(
    action: &ActionConfig,
    keys: &[&str],
    min: u64,
    max: u64,
    label: &str,
) -> Result<(), String> {
    let Some(value) = action_parameter(action, keys).map(str::trim) else {
        return Ok(());
    };
    if value.is_empty() {
        return Ok(());
    }
    let parsed = value.parse::<u64>().map_err(|_| format!("{label}无效"))?;
    if parsed < min || parsed > max {
        return Err(format!("{label}超出范围"));
    }
    Ok(())
}

fn validate_json_string_record_parameter(
    action: &ActionConfig,
    keys: &[&str],
    label: &str,
) -> Result<(), String> {
    let Some(value) = action_parameter(action, keys).map(str::trim) else {
        return Ok(());
    };
    if value.is_empty() {
        return Ok(());
    }
    serde_json::from_str::<BTreeMap<String, String>>(value)
        .map(|_| ())
        .map_err(|error| format!("{label}格式错误：{error}"))
}

fn action_parameter<'a>(action: &'a ActionConfig, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| action.parameters.get(*key).map(String::as_str))
}

fn is_valid_match_operator(operator_type: &str) -> bool {
    matches!(
        operator_type,
        "equals"
            | "not_equals"
            | "contains"
            | "not_contains"
            | "starts_with"
            | "ends_with"
            | "regex"
            | "not_regex"
            | "is_empty"
            | "is_not_empty"
    )
}

fn match_operator_needs_value(operator_type: &str) -> bool {
    !matches!(operator_type, "is_empty" | "is_not_empty")
}

fn is_valid_action_type(action_type: &str) -> bool {
    matches!(
        action_type,
        "open_url"
            | "open_app"
            | "activate_app"
            | "send_notification"
            | "run_shell"
            | "run_applescript"
            | "run_javascript"
            | "run_python"
            | "http_request"
    )
}

#[tauri::command]
fn automation_status(state: State<AppState>) -> Result<AutomationStatus, String> {
    let logs = state
        .logs
        .lock()
        .map_err(|_| "日志状态已损坏".to_string())?
        .clone();
    Ok(AutomationStatus {
        watcher_running: state.watcher_running.load(Ordering::Relaxed),
        last_record_id: state.last_record_id.load(Ordering::Relaxed),
        logs,
    })
}

#[tauri::command]
fn action_queue_status(state: State<AppState>) -> Result<ActionQueueStatus, String> {
    let pending = state
        .action_queue
        .lock()
        .map_err(|_| "动作队列状态已损坏".to_string())?
        .iter()
        .map(action_job_summary)
        .collect::<Vec<_>>();
    let running = state
        .current_action_job
        .lock()
        .map_err(|_| "动作执行状态已损坏".to_string())?
        .clone();
    Ok(ActionQueueStatus {
        pending_count: pending.len(),
        max_pending_count: MAX_ACTION_QUEUE_ENTRIES,
        running,
        pending,
    })
}

#[tauri::command]
fn action_history(state: State<AppState>) -> Result<Vec<ActionHistoryEntry>, String> {
    if let Ok(items) = notification_archive::recent_action_history(MAX_ACTION_HISTORY_ENTRIES) {
        return Ok(items);
    }
    state
        .action_history
        .lock()
        .map(|items| items.iter().rev().cloned().collect())
        .map_err(|_| "执行历史状态已损坏".to_string())
}

#[tauri::command]
fn clear_action_history(state: State<AppState>) -> Result<(), String> {
    notification_archive::clear_action_history().map_err(|error| error.to_string())?;
    state
        .action_history
        .lock()
        .map(|mut items| items.clear())
        .map_err(|_| "执行历史状态已损坏".to_string())
}

#[tauri::command]
fn archive_stats() -> Result<notification_archive::ArchiveStats, String> {
    notification_archive::archive_stats().map_err(|error| error.to_string())
}

#[tauri::command]
fn compact_archive() -> Result<notification_archive::ArchiveStats, String> {
    notification_archive::compact_archive().map_err(|error| error.to_string())?;
    archive_stats()
}

#[tauri::command]
fn prune_archive(
    notification_retention_days: Option<u64>,
) -> Result<notification_archive::ArchiveStats, String> {
    notification_archive::prune_archive(notification_retention_days.unwrap_or(90))
        .map_err(|error| error.to_string())?;
    archive_stats()
}

#[tauri::command]
fn system_delete_audit() -> Result<Vec<notification_archive::SystemDeleteAuditEntry>, String> {
    notification_archive::recent_system_delete_audit(50).map_err(|error| error.to_string())
}

#[tauri::command]
fn app_settings(deep_check: Option<bool>, state: State<AppState>) -> Result<SettingsInfo, String> {
    let notification_database_path =
        notification_database_path().map_err(|error| error.to_string())?;
    let settings_path = settings_path().map_err(|error| error.to_string())?;
    let notification_database_checked = deep_check.unwrap_or(false);
    let notification_database_error = if notification_database_checked {
        notification_database_diagnostic().err()
    } else {
        None
    };
    let settings = state
        .settings
        .lock()
        .map_err(|_| "设置状态已损坏".to_string())?
        .clone();
    let data_directory = data_dir_for_settings(&settings).map_err(|error| error.to_string())?;
    let notification_archive_path = data_directory.join("notifications.sqlite");
    let rules_path = data_directory.join("rules.json");
    let persistence_load_error = state
        .persistence_load_errors
        .lock()
        .map_err(|_| "持久化加载错误状态已损坏".to_string())
        .ok()
        .and_then(|errors| persistence_load_error_message(&errors));
    let last_record_id = state.last_record_id.load(Ordering::Relaxed);

    Ok(SettingsInfo {
        notification_database_path: notification_database_path.to_string_lossy().to_string(),
        notification_database_checked,
        notification_database_accessible: notification_database_checked
            && notification_database_error.is_none(),
        notification_database_error,
        data_directory: data_directory.to_string_lossy().to_string(),
        notification_archive_path: notification_archive_path.to_string_lossy().to_string(),
        rules_path: rules_path.to_string_lossy().to_string(),
        settings_path: settings_path.to_string_lossy().to_string(),
        launch_at_login: launch_at_login_enabled() || settings.launch_at_login,
        app_filter_mode: normalized_app_filter_mode(settings.app_filter_mode.as_deref())
            .to_string(),
        ignored_app_identifiers: settings.ignored_app_identifiers,
        watcher_running: state.watcher_running.load(Ordering::Relaxed),
        last_record_id,
        persistence_load_error,
    })
}

fn notification_database_diagnostic() -> Result<(), String> {
    max_record_id().map_err(|error| error.to_string())?;
    let count = record_count().map_err(|error| error.to_string())?;
    if count > 0 {
        return Ok(());
    }

    let archive_count = notification_archive::archive_stats()
        .map(|stats| stats.notification_count)
        .unwrap_or(0);
    let corrupt_backup_exists = notification_database_corrupt_backup_path()
        .ok()
        .and_then(|path| fs::metadata(path).ok())
        .map(|metadata| metadata.len() > 4096)
        .unwrap_or(false);
    if archive_count > 0 || corrupt_backup_exists {
        return Err(
            "系统通知数据库当前可打开但 record 表为空；检测到本地归档或 db.corrupt，macOS 可能已重建/损坏通知库。NoticeFlow 当前只能显示本地归档，无法读取重建后的新通知。建议重启 usernoted 或重启系统后再刷新。"
                .to_string(),
        );
    }

    Err("系统通知数据库当前没有可读取记录。".to_string())
}

#[tauri::command]
fn save_app_settings(
    update: SettingsUpdate,
    state: State<AppState>,
) -> Result<SettingsInfo, String> {
    ensure_persistence_loaded(&state, PersistenceDomain::Any)?;
    let previous_settings = state
        .settings
        .lock()
        .map_err(|_| "设置状态已损坏".to_string())?
        .clone();
    let mut next = AppSettings {
        launch_at_login: update.launch_at_login,
        data_directory: update
            .data_directory
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        app_filter_mode: update.app_filter_mode,
        ignored_app_identifiers: update.ignored_app_identifiers,
    };
    normalize_settings(&mut next);
    let target_data_directory = next
        .data_directory
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or(app_settings::default_data_dir().map_err(|error| error.to_string())?);
    let current_data_directory =
        data_dir_for_settings(&previous_settings).map_err(|error| error.to_string())?;
    validate_data_directory(&target_data_directory)
        .map_err(|error| format!("数据目录不安全，设置未保存：{}", error))?;
    ensure_data_directory_writable(&target_data_directory)
        .map_err(|error| format!("目标数据目录不可写，设置未保存：{error}"))?;

    let rules = state
        .rules
        .lock()
        .map_err(|_| "规则状态已损坏".to_string())?
        .clone();
    let target_plan =
        rules_for_target_data_directory(&current_data_directory, &target_data_directory, &rules)
            .map_err(|error| format!("目标数据目录规则处理失败，设置未保存：{error}"))?;
    let previous_launch_at_login = launch_at_login_enabled();
    set_launch_at_login(next.launch_at_login).map_err(|error| error.to_string())?;
    if let Err(error) = save_settings(&next) {
        if should_rollback_launch_at_login(previous_launch_at_login, next.launch_at_login) {
            let _ = set_launch_at_login(previous_launch_at_login);
        }
        return Err(error.to_string());
    }
    if let Err(error) = commit_target_data_directory(
        &current_data_directory,
        &target_data_directory,
        &target_plan,
    ) {
        let _ = save_settings(&previous_settings);
        if should_rollback_launch_at_login(previous_launch_at_login, next.launch_at_login) {
            let _ = set_launch_at_login(previous_launch_at_login);
        }
        return Err(format!("数据迁移失败，设置已回滚：{error}"));
    }
    *state
        .settings
        .lock()
        .map_err(|_| "设置状态已损坏".to_string())? = next;
    *state
        .rules
        .lock()
        .map_err(|_| "规则状态已损坏".to_string())? = target_plan.rules;
    if let Ok(mut pending_hit_counts) = state.pending_hit_counts.lock() {
        pending_hit_counts.clear();
    }
    if let Ok(mut cooldowns) = state.rule_cooldowns.lock() {
        cooldowns.clear();
    }
    app_settings(Some(true), state)
}

#[tauri::command]
fn reveal_path(path: String, state: State<AppState>) -> Result<(), String> {
    let path = PathBuf::from(path);
    validate_reveal_path(&path, &state)?;
    let status = Command::new("/usr/bin/open")
        .arg("-R")
        .arg(&path)
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("打开路径失败".to_string())
    }
}

fn validate_reveal_path(path: &Path, state: &AppState) -> Result<(), String> {
    if !path.is_absolute() || path_has_parent_component(path) {
        return Err("不允许打开未授权路径".to_string());
    }

    let data_dir = data_directory_for_state(state)?;
    let default_data_dir = app_settings::default_data_dir().map_err(|error| error.to_string())?;
    if path_is_inside_allowed_directory(path, &data_dir)
        || path_is_inside_allowed_directory(path, &default_data_dir)
    {
        return Ok(());
    }

    let allowed_files = [
        notification_database_path().map_err(|error| error.to_string())?,
        data_dir.join("notifications.sqlite"),
        data_dir.join("rules.json"),
        settings_path().map_err(|error| error.to_string())?,
    ];
    if allowed_files
        .iter()
        .any(|allowed_path| paths_refer_to_same_target(path, allowed_path))
    {
        return Ok(());
    }

    Err("不允许打开未授权路径".to_string())
}

fn path_has_parent_component(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
}

fn path_is_inside_allowed_directory(path: &Path, directory: &Path) -> bool {
    if !path.starts_with(directory) {
        return false;
    }
    let Ok(path_anchor) = canonical_existing_path_or_parent(path) else {
        return false;
    };
    let Ok(directory_anchor) = canonical_existing_path_or_parent(directory) else {
        return false;
    };
    path_anchor.starts_with(directory_anchor)
}

fn paths_refer_to_same_target(path: &Path, allowed_path: &Path) -> bool {
    if path == allowed_path {
        return true;
    }
    match (fs::canonicalize(path), fs::canonicalize(allowed_path)) {
        (Ok(path), Ok(allowed_path)) => path == allowed_path,
        _ => false,
    }
}

fn canonical_existing_path_or_parent(path: &Path) -> std::io::Result<PathBuf> {
    if path.exists() {
        return fs::canonicalize(path);
    }
    let parent = path.parent().unwrap_or(path);
    fs::canonicalize(parent)
}

#[tauri::command]
fn open_full_disk_access_settings() -> Result<(), String> {
    let status = Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("打开系统设置失败".to_string())
    }
}

#[tauri::command]
fn choose_data_directory() -> Result<Option<String>, String> {
    let output = Command::new("/usr/bin/osascript")
        .args([
            "-e",
            r#"POSIX path of (choose folder with prompt "选择 NoticeFlow 数据目录")"#,
        ])
        .output()
        .map_err(|error| error.to_string())?;

    if output.status.success() {
        return Ok(osascript_stdout_path(&output.stdout));
    }

    let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if error.contains("-128") || error.to_lowercase().contains("user canceled") {
        Ok(None)
    } else {
        Err(if error.is_empty() {
            "选择目录失败".to_string()
        } else {
            error
        })
    }
}

#[tauri::command]
fn choose_script_file() -> Result<Option<String>, String> {
    let output = Command::new("/usr/bin/osascript")
        .args([
            "-e",
            r#"POSIX path of (choose file with prompt "选择动作脚本文件")"#,
        ])
        .output()
        .map_err(|error| error.to_string())?;

    if output.status.success() {
        return Ok(osascript_stdout_path(&output.stdout));
    }

    let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if error.contains("-128") || error.to_lowercase().contains("user canceled") {
        Ok(None)
    } else {
        Err(if error.is_empty() {
            "选择脚本文件失败".to_string()
        } else {
            error
        })
    }
}

fn osascript_stdout_path(stdout: &[u8]) -> Option<String> {
    let path = String::from_utf8_lossy(stdout)
        .trim_end_matches(['\r', '\n'])
        .to_string();
    (!path.is_empty()).then_some(path)
}

#[tauri::command]
fn match_rule_names_for_notification(
    record_id: i64,
    state: State<AppState>,
) -> Result<Vec<String>, String> {
    let record = notification_record_by_id(record_id)?;
    let rules = state
        .rules
        .lock()
        .map_err(|_| "规则状态已损坏".to_string())?
        .clone();
    Ok(matching_rules(&rules, &record)
        .into_iter()
        .filter(|(rule, _)| rule_automation_block_reason(rule).is_none())
        .map(|(rule, _)| rule.name)
        .collect())
}

#[tauri::command]
fn test_rule_on_notification(
    rule_id: String,
    record_id: i64,
    state: State<AppState>,
) -> Result<Vec<String>, String> {
    let rules = state
        .rules
        .lock()
        .map_err(|_| "规则状态已损坏".to_string())?
        .clone();
    let rule = rules
        .into_iter()
        .find(|rule| rule.id == rule_id)
        .ok_or_else(|| "未找到规则".to_string())?;
    validate_rule_regexes(&rule)?;
    run_rule_on_record(rule, record_id, true, &state)
}

#[tauri::command]
fn test_rule_draft_on_notification(
    rule: AutomationRule,
    record_id: i64,
    state: State<AppState>,
) -> Result<Vec<String>, String> {
    validate_rule_regexes(&rule)?;
    run_rule_on_record(rule, record_id, true, &state)
}

#[tauri::command]
fn match_rule_draft_on_notification(
    rule: AutomationRule,
    record_id: i64,
    state: State<AppState>,
) -> Result<Vec<String>, String> {
    validate_rule_regexes(&rule)?;
    run_rule_on_record(rule, record_id, false, &state)
}

#[tauri::command]
fn explain_rule_draft_on_notification(
    rule: AutomationRule,
    record_id: i64,
) -> Result<MatchExplanation, String> {
    validate_rule_regexes(&rule)?;
    let record = notification_record_by_id(record_id)?;
    Ok(analyze_rule_match(&rule, &record).explanation)
}

fn run_rule_on_record(
    rule: AutomationRule,
    record_id: i64,
    execute_actions: bool,
    state: &State<AppState>,
) -> Result<Vec<String>, String> {
    if execute_actions {
        validate_rule_for_save(&rule).map_err(|error| format!("规则「{}」{error}", rule.name))?;
    }
    let record = notification_record_by_id(record_id)?;
    let analysis = analyze_rule_match(&rule, &record);
    if !analysis.explanation.matched {
        return Err(analysis.explanation.message);
    }
    if !execute_actions {
        return Ok(match_explanation_lines(&analysis.explanation));
    }
    let actions = rule.actions.clone().unwrap_or_default();
    let executions = action_runner::run_actions_detailed(&actions, &analysis.variables);
    record_action_history(
        state,
        &rule,
        &record,
        &executions,
        None,
        &analysis.variables,
    );
    let logs = executions
        .into_iter()
        .map(|execution| execution.message)
        .collect::<Vec<_>>();
    push_logs(state, logs.clone());
    Ok(logs)
}

#[tauri::command]
fn preview_variables(
    record_id: i64,
    custom_rules: Vec<VariableExtractionRule>,
) -> Result<VariablePreview, String> {
    let record = notification_record_by_id(record_id)?;
    let variables = extract_variables(&record, &custom_rules);
    Ok(VariablePreview {
        display_names: display_variable_names(&variables),
        variables,
    })
}

#[tauri::command]
fn hide_notification(record: NotificationRecordIdentity) -> Result<(), String> {
    if hide_archived_record_if_matches(&record)? {
        return Ok(());
    }

    let Some(system_record) = record_by_id(record.id).map_err(|error| error.to_string())? else {
        return Err("本地归档和系统通知库中都未找到这条通知，隐藏失败".to_string());
    };
    if !notification_identity_matches(&system_record, &record) {
        return Err("系统通知库中的同 ID 通知已变化，已取消隐藏操作".to_string());
    }
    notification_archive::upsert_records(std::slice::from_ref(&system_record))
        .map_err(|error| error.to_string())?;
    let changed =
        notification_archive::hide_record(system_record.id).map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("本地归档中未找到这条通知，隐藏失败".to_string());
    }
    Ok(())
}

#[tauri::command]
fn clear_hidden_notifications() -> Result<(), String> {
    notification_archive::clear_hidden_records().map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_system_notification(record: NotificationRecordIdentity) -> Result<(), String> {
    let selected_record = record_from_identity(&record)?;
    let Some(system_record) = record_by_id(record.id).map_err(|error| error.to_string())? else {
        let Some(archived_record) =
            notification_archive::record_by_id(record.id).map_err(|error| error.to_string())?
        else {
            return Err("系统通知库和本地归档中都未找到这条通知".to_string());
        };
        if !notification_identity_matches(&archived_record, &record) {
            return Err(
                "系统通知库中未找到这条记录，且本地归档中的同 ID 通知已变化，未改动本地归档"
                    .to_string(),
            );
        }
        let local_errors = mark_deleted_and_audit(&selected_record, 0);
        if local_errors.is_empty() {
            return Err("系统通知库中未找到这条记录，已从 NoticeFlow 列表隐藏".to_string());
        }
        return Err(format!(
            "系统通知库中未找到这条记录，且本地归档更新失败：{}",
            local_errors.join("；")
        ));
    };
    if !notification_identity_matches(&system_record, &record) {
        if hide_archived_record_if_matches(&record)? {
            return Err(
                "系统通知库中的同 ID 通知已变化，已隐藏列表中的旧记录，未删除系统通知".to_string(),
            );
        }
        return Err(
            "系统通知库中的同 ID 通知已变化，未删除系统通知；刷新后可查看最新记录".to_string(),
        );
    }

    let _ = notification_archive::upsert_records(std::slice::from_ref(&system_record));
    let changed =
        delete_system_notification_record(record.id).map_err(|error| error.to_string())?;
    let local_errors = mark_deleted_and_audit(&system_record, changed);
    if changed == 0 {
        if local_errors.is_empty() {
            return Err("系统通知库中未找到这条记录，已从 NoticeFlow 列表隐藏".to_string());
        }
        return Err(format!(
            "系统通知库中未找到这条记录，且本地归档更新失败：{}",
            local_errors.join("；")
        ));
    }
    if !local_errors.is_empty() {
        return Err(format!(
            "系统通知已删除，但本地归档/审计更新失败：{}",
            local_errors.join("；")
        ));
    }
    Ok(())
}

fn hide_archived_record_if_matches(record: &NotificationRecordIdentity) -> Result<bool, String> {
    let Some(archived_record) =
        notification_archive::record_by_id(record.id).map_err(|error| error.to_string())?
    else {
        return Ok(false);
    };
    if !notification_identity_matches(&archived_record, record) {
        return Ok(false);
    }
    let changed =
        notification_archive::hide_record(record.id).map_err(|error| error.to_string())?;
    Ok(changed > 0)
}

fn record_from_identity(record: &NotificationRecordIdentity) -> Result<NotificationRecord, String> {
    let delivered_at = chrono::DateTime::parse_from_rfc3339(&record.delivered_at)
        .map_err(|error| format!("通知时间格式无效：{error}"))?
        .with_timezone(&chrono::Utc);
    Ok(NotificationRecord {
        id: record.id,
        app_identifier: record.app_identifier.clone(),
        app_name: record.app_name.clone(),
        delivered_at,
        title: record.title.clone(),
        subtitle: record.subtitle.clone(),
        body: record.body.clone(),
    })
}

fn notification_identity_matches(
    record: &NotificationRecord,
    expected: &NotificationRecordIdentity,
) -> bool {
    if record.id != expected.id
        || !record
            .app_identifier
            .eq_ignore_ascii_case(&expected.app_identifier)
        || record.title != expected.title
        || record.subtitle != expected.subtitle
        || record.body != expected.body
    {
        return false;
    }
    chrono::DateTime::parse_from_rfc3339(&expected.delivered_at)
        .map(|delivered_at| delivered_at.with_timezone(&chrono::Utc) == record.delivered_at)
        .unwrap_or(false)
}

fn mark_deleted_and_audit(record: &NotificationRecord, changed: usize) -> Vec<String> {
    let mut errors = Vec::new();
    match notification_archive::mark_record_system_deleted(record.id) {
        Ok(0) => errors.push("归档标记失败：本地归档中未找到这条通知".to_string()),
        Ok(_) => {}
        Err(error) => errors.push(format!("归档标记失败：{error}")),
    }
    if let Err(error) = notification_archive::append_system_delete_audit(record, changed) {
        errors.push(format!("审计记录失败：{error}"));
    }
    errors
}

fn notification_record_by_id(record_id: i64) -> Result<NotificationRecord, String> {
    if let Some(record) = record_by_id(record_id).map_err(|error| error.to_string())? {
        let _ = notification_archive::upsert_records(std::slice::from_ref(&record));
        return Ok(record);
    }
    notification_archive::record_by_id(record_id)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "未找到通知记录".to_string())
}

fn main() {
    let (initial_settings, settings_load_error) = match load_settings() {
        Ok(settings) => (settings, None),
        Err(error) => (AppSettings::default(), Some(error.to_string())),
    };
    let (initial_rules, rules_load_error) = if settings_load_error.is_some() {
        (
            RuleFile { rules: Vec::new() }.rules,
            Some("settings.json 加载失败，已跳过规则加载".to_string()),
        )
    } else {
        match data_dir_for_settings(&initial_settings)
            .and_then(|data_directory| load_rules_from_dir(&data_directory))
        {
            Ok(rules) => (rules, None),
            Err(error) => (
                RuleFile { rules: Vec::new() }.rules,
                Some(error.to_string()),
            ),
        }
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            rules: Mutex::new(initial_rules),
            settings: Mutex::new(initial_settings),
            persistence_load_errors: Mutex::new(PersistenceLoadErrors {
                rules: rules_load_error,
                settings: settings_load_error,
            }),
            logs: Mutex::new(Vec::new()),
            action_history: Mutex::new(Vec::new()),
            action_queue: Mutex::new(VecDeque::new()),
            current_action_job: Mutex::new(None),
            rule_cooldowns: Mutex::new(HashMap::new()),
            pending_hit_counts: Mutex::new(HashMap::new()),
            last_hit_count_save: Mutex::new(Instant::now()),
            action_worker_running: AtomicBool::new(false),
            watcher_running: AtomicBool::new(false),
            last_record_id: AtomicI64::new(0),
        })
        .invoke_handler(tauri::generate_handler![
            list_notifications,
            list_applications,
            application_icon,
            application_icon_for_bundle,
            list_rules,
            save_rules,
            set_rule_enabled,
            preview_variables,
            automation_status,
            action_queue_status,
            action_history,
            clear_action_history,
            archive_stats,
            compact_archive,
            prune_archive,
            system_delete_audit,
            app_settings,
            save_app_settings,
            reveal_path,
            choose_data_directory,
            choose_script_file,
            hide_notification,
            clear_hidden_notifications,
            delete_system_notification,
            open_full_disk_access_settings,
            match_rule_names_for_notification,
            test_rule_on_notification,
            test_rule_draft_on_notification,
            match_rule_draft_on_notification,
            explain_rule_draft_on_notification,
            validate_regex
        ])
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            setup_tray(app)?;
            show_main_window(app.handle());
            start_action_worker(app.handle().clone());
            start_watcher(app.handle().clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build NoticeFlow")
        .run(|app, event| {
            if let RunEvent::ExitRequested { .. } = event {
                let state = app.state::<AppState>();
                flush_hit_counts(&state, true);
            }
            #[cfg(target_os = "macos")]
            if let RunEvent::Reopen { .. } = event {
                show_main_window(app);
            }
        });
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .text(TRAY_OPEN_MENU_ID, "打开 NoticeFlow")
        .text(TRAY_REFRESH_MENU_ID, "刷新通知")
        .separator()
        .text(TRAY_QUIT_MENU_ID, "退出 NoticeFlow")
        .build()?;
    let tray = TrayIconBuilder::with_id("noticeflow")
        .menu(&menu)
        .icon(tray_icon_image())
        .tooltip("NoticeFlow")
        .icon_as_template(true)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_OPEN_MENU_ID => show_main_window(app),
            TRAY_REFRESH_MENU_ID => {
                emit_automation_event(app, "manual_refresh", "正在刷新通知列表")
            }
            TRAY_QUIT_MENU_ID => {
                let state = app.state::<AppState>();
                flush_hit_counts(&state, true);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button,
                button_state,
                ..
            } = event
            {
                if button == MouseButton::Left && button_state == MouseButtonState::Up {
                    show_main_window(tray.app_handle());
                }
            }
        });
    tray.build(app)?;
    Ok(())
}

fn tray_icon_image() -> Image<'static> {
    const WIDTH: u32 = 64;
    const HEIGHT: u32 = 64;
    let mut rgba = vec![0; (WIDTH * HEIGHT * 4) as usize];
    for (x, y, width, height, radius) in [
        (8, 8, 48, 7, 4),
        (8, 19, 32, 7, 4),
        (8, 30, 44, 7, 4),
        (8, 41, 26, 7, 4),
        (8, 52, 20, 7, 4),
    ] {
        fill_rounded_rect(&mut rgba, WIDTH, x, y, width, height, radius);
    }
    Image::new_owned(rgba, WIDTH, HEIGHT)
}

fn fill_rounded_rect(
    rgba: &mut [u8],
    canvas_width: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    radius: u32,
) {
    for pixel_y in y..(y + height) {
        for pixel_x in x..(x + width) {
            if !inside_rounded_rect(pixel_x - x, pixel_y - y, width, height, radius) {
                continue;
            }
            let offset = ((pixel_y * canvas_width + pixel_x) * 4) as usize;
            rgba[offset] = 0;
            rgba[offset + 1] = 0;
            rgba[offset + 2] = 0;
            rgba[offset + 3] = 255;
        }
    }
}

fn inside_rounded_rect(local_x: u32, local_y: u32, width: u32, height: u32, radius: u32) -> bool {
    let inner_left = radius;
    let inner_right = width.saturating_sub(radius + 1);
    if local_x >= inner_left && local_x <= inner_right {
        return true;
    }
    let center_x = if local_x < inner_left {
        inner_left
    } else {
        inner_right
    };
    let center_y = height / 2;
    let dx = local_x as i64 - center_x as i64;
    let dy = local_y as i64 - center_y as i64;
    dx * dx + dy * dy <= (radius as i64) * (radius as i64)
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_title("NoticeFlow");
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn analyze_rule_match(rule: &AutomationRule, record: &NotificationRecord) -> RuleMatchAnalysis {
    let variables = extract_variables(record, rule.variable_extractions.as_deref().unwrap_or(&[]));
    let block_reason = rule_match_block_reason(rule);
    let app_matched = rule_matches_record_app(rule, &record.app_identifier);
    let time_matched = rule_trigger_time_matches(rule);
    let conditions = rule
        .match_conditions
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|condition| {
            let actual_value = variables
                .get(&condition.variable_name)
                .cloned()
                .unwrap_or_default();
            let expected_value = condition.expected_value.clone().unwrap_or_default();
            ConditionExplanation {
                variable_name: condition.variable_name.clone(),
                operator_type: condition.operator_type.clone(),
                expected_value,
                actual_value,
                case_sensitive: condition.case_sensitive,
                matched: condition.evaluate(&variables),
            }
        })
        .collect::<Vec<_>>();
    let condition_matched = conditions.iter().all(|condition| condition.matched);
    let matched = block_reason.is_none() && app_matched && time_matched && condition_matched;
    let message = match_message(
        rule,
        matched,
        app_matched,
        time_matched,
        &conditions,
        block_reason,
    );
    RuleMatchAnalysis {
        explanation: MatchExplanation {
            matched,
            rule_name: rule.name.clone(),
            app_matched,
            time_matched,
            variable_count: visible_variable_count(&variables),
            conditions,
            message,
        },
        variables,
    }
}

fn rule_match_block_reason(rule: &AutomationRule) -> Option<&'static str> {
    if !rule_has_trigger_app(rule) {
        return Some("缺少触发应用");
    }
    if !rule_has_match_conditions(rule) {
        return Some("缺少匹配条件");
    }
    None
}

fn match_message(
    rule: &AutomationRule,
    matched: bool,
    app_matched: bool,
    time_matched: bool,
    conditions: &[ConditionExplanation],
    block_reason: Option<&str>,
) -> String {
    if matched {
        return format!("匹配成功：{}，未执行动作", rule.name);
    }
    let failed_conditions = conditions
        .iter()
        .filter(|condition| !condition.matched)
        .count();
    let mut reasons = Vec::new();
    if !app_matched {
        reasons.push("触发应用不匹配".to_string());
    }
    if !time_matched {
        reasons.push("触发时间不匹配".to_string());
    }
    if let Some(reason) = block_reason {
        reasons.push(reason.to_string());
    }
    if failed_conditions > 0 {
        reasons.push(format!("{} 个条件未通过", failed_conditions));
    }
    if reasons.is_empty() {
        reasons.push("未满足规则".to_string());
    }
    format!("匹配失败：{}（{}）", rule.name, reasons.join("，"))
}

fn match_explanation_lines(explanation: &MatchExplanation) -> Vec<String> {
    let mut lines = vec![explanation.message.clone()];
    lines.push(format!(
        "应用：{}；时间：{}；变量：{} 个",
        pass_fail_label(explanation.app_matched),
        pass_fail_label(explanation.time_matched),
        explanation.variable_count
    ));
    for (index, condition) in explanation.conditions.iter().enumerate() {
        lines.push(format!(
            "条件 {}：{} {} {}，实际值：{}，{}",
            index + 1,
            condition.variable_name,
            condition.operator_type,
            condition.expected_value,
            truncate_log_value(&condition.actual_value),
            pass_fail_label(condition.matched)
        ));
    }
    lines
}

fn pass_fail_label(value: bool) -> &'static str {
    if value {
        "通过"
    } else {
        "未通过"
    }
}

fn truncate_log_value(value: &str) -> String {
    const MAX_LENGTH: usize = 120;
    let trimmed = value.trim();
    if trimmed.chars().count() <= MAX_LENGTH {
        return trimmed.to_string();
    }
    let mut result = trimmed.chars().take(MAX_LENGTH).collect::<String>();
    result.push_str("...");
    result
}

fn visible_variable_count(variables: &BTreeMap<String, String>) -> usize {
    variables
        .keys()
        .filter(|key| !key.starts_with("__"))
        .count()
}

fn visible_variables_json(variables: &BTreeMap<String, String>) -> Option<String> {
    let visible = variables
        .iter()
        .filter(|(key, _)| !key.starts_with("__"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    serde_json::to_string(&visible).ok()
}

fn rule_matches_record_app(rule: &AutomationRule, record_app_identifier: &str) -> bool {
    let Some(app_identifiers) = rule.app_identifiers.as_deref() else {
        return false;
    };
    let record_app_identifier = record_app_identifier.trim();
    !record_app_identifier.is_empty()
        && app_identifiers
            .iter()
            .map(|app_id| app_id.trim())
            .any(|app_id| !app_id.is_empty() && app_id.eq_ignore_ascii_case(record_app_identifier))
}

fn rule_trigger_time_matches(rule: &AutomationRule) -> bool {
    let Some(trigger_time) = rule
        .trigger_time
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return true;
    };
    chrono::Local::now().format("%H:%M").to_string() == trigger_time
}

fn enqueue_action_job(
    state: &AppState,
    rule: AutomationRule,
    record: NotificationRecord,
    variables: BTreeMap<String, String>,
) -> Result<ActionQueueItem, String> {
    let mut queue = state
        .action_queue
        .lock()
        .map_err(|_| "动作队列状态已损坏".to_string())?;
    if queue.len() >= MAX_ACTION_QUEUE_ENTRIES {
        return Err(format!(
            "动作队列已满（{} 条），请稍后再试",
            MAX_ACTION_QUEUE_ENTRIES
        ));
    }
    let job = ActionJob {
        id: uuid::Uuid::new_v4().to_string(),
        queued_at: chrono::Utc::now().to_rfc3339(),
        rule,
        record,
        variables,
    };
    let item = action_job_summary(&job);
    queue.push_back(job);
    Ok(item)
}

fn action_job_summary(job: &ActionJob) -> ActionQueueItem {
    ActionQueueItem {
        id: job.id.clone(),
        queued_at: job.queued_at.clone(),
        rule_id: job.rule.id.clone(),
        rule_name: job.rule.name.clone(),
        notification_id: job.record.id,
        notification_title: job.record.title.clone(),
        app_identifier: job.record.app_identifier.clone(),
        action_count: job.rule.actions.as_ref().map(Vec::len).unwrap_or(0),
    }
}

fn start_action_worker(app: tauri::AppHandle) {
    let state = app.state::<AppState>();
    if state.action_worker_running.swap(true, Ordering::Relaxed) {
        return;
    }

    thread::spawn(move || loop {
        let job = {
            let state = app.state::<AppState>();
            state
                .action_queue
                .lock()
                .ok()
                .and_then(|mut queue| queue.pop_front())
        };
        let Some(job) = job else {
            thread::sleep(ACTION_WORKER_IDLE_INTERVAL);
            continue;
        };

        let summary = action_job_summary(&job);
        {
            let state = app.state::<AppState>();
            if let Ok(mut current) = state.current_action_job.lock() {
                *current = Some(summary.clone());
            };
        }
        let start_message = format!(
            "动作开始：{} / {}",
            summary.rule_name, summary.notification_title
        );
        {
            let state = app.state::<AppState>();
            push_log(&state, start_message.clone());
        }
        emit_automation_event(&app, "queue", &start_message);

        let executions = action_runner::run_actions_detailed(
            &job.rule.actions.clone().unwrap_or_default(),
            &job.variables,
        );
        let logs = executions
            .iter()
            .map(|execution| execution.message.clone())
            .collect::<Vec<_>>();
        {
            let state = app.state::<AppState>();
            record_action_history(
                &state,
                &job.rule,
                &job.record,
                &executions,
                Some(&job.id),
                &job.variables,
            );
            push_logs(&state, logs.clone());
            if let Ok(mut current) = state.current_action_job.lock() {
                *current = None;
            };
        }

        let failed_count = executions
            .iter()
            .filter(|execution| !execution.success)
            .count();
        let success_count = executions.len().saturating_sub(failed_count);
        for message in logs {
            emit_automation_event(&app, "action", &message);
        }
        let done_message = if failed_count == 0 {
            format!(
                "动作完成：{} / {}（成功 {} 个）",
                summary.rule_name, summary.notification_title, success_count
            )
        } else {
            format!(
                "动作完成但有失败：{} / {}（成功 {} 个，失败 {} 个）",
                summary.rule_name, summary.notification_title, success_count, failed_count
            )
        };
        {
            let state = app.state::<AppState>();
            push_log(&state, done_message.clone());
        }
        emit_automation_event(&app, "queue", &done_message);
        emit_automation_event(
            &app,
            if failed_count == 0 {
                "action_success"
            } else {
                "action_error"
            },
            &done_message,
        );
        notify_automation_result(
            failed_count == 0,
            &summary.rule_name,
            &summary.notification_title,
            success_count,
            failed_count,
        );
    });
}

fn emit_automation_event(app: &tauri::AppHandle, kind: &str, message: &str) {
    let _ = app.emit(
        "noticeflow://automation",
        AutomationEvent {
            kind: kind.to_string(),
            message: message.to_string(),
        },
    );
}

fn notify_automation_result(
    success: bool,
    rule_name: &str,
    notification_title: &str,
    success_count: usize,
    failed_count: usize,
) {
    let title = if success {
        AUTOMATION_RESULT_SUCCESS_TITLE
    } else {
        AUTOMATION_RESULT_FAILURE_TITLE
    };
    let status = if success {
        format!("成功 {success_count} 个动作")
    } else {
        format!("成功 {success_count} 个，失败 {failed_count} 个")
    };
    let body = truncate_notification_body(&format!(
        "{} / {}：{}",
        rule_name, notification_title, status
    ));
    let script = format!(
        "display notification {} with title {} subtitle {}",
        applescript_quote(&body),
        applescript_quote(title),
        applescript_quote(AUTOMATION_RESULT_SUBTITLE)
    );
    let _ = Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .status();
}

fn truncate_notification_body(value: &str) -> String {
    const MAX_LENGTH: usize = 160;
    let trimmed = value.trim();
    if trimmed.chars().count() <= MAX_LENGTH {
        return trimmed.to_string();
    }
    let mut result = trimmed.chars().take(MAX_LENGTH).collect::<String>();
    result.push_str("...");
    result
}

fn applescript_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn start_watcher(app: tauri::AppHandle) {
    let state = app.state::<AppState>();
    if state.watcher_running.swap(true, Ordering::Relaxed) {
        return;
    }

    let baseline_ready = match max_record_id() {
        Ok(id) => {
            state.last_record_id.store(id, Ordering::Relaxed);
            push_log(&state, format!("监听已启动，起始通知记录 ID：{id}"));
            true
        }
        Err(error) => {
            push_log(&state, format!("监听启动失败：{error}"));
            false
        }
    };

    thread::spawn(move || {
        let mut baseline_ready = baseline_ready;
        let mut consecutive_errors = 0_u32;
        let mut last_reported_error = String::new();
        loop {
            thread::sleep(watcher_sleep_duration(consecutive_errors));
            let state = app.state::<AppState>();
            if !baseline_ready {
                match max_record_id() {
                    Ok(id) => {
                        state.last_record_id.store(id, Ordering::Relaxed);
                        baseline_ready = true;
                        consecutive_errors = 0;
                        last_reported_error.clear();
                        let message = format!("监听已恢复，新的起始通知记录 ID：{id}");
                        push_log(&state, message.clone());
                        let _ = app.emit(
                            "noticeflow://automation",
                            AutomationEvent {
                                kind: "status".to_string(),
                                message,
                            },
                        );
                    }
                    Err(error) => {
                        report_watcher_error(
                            &app,
                            &state,
                            format!("监听等待授权或数据库可用：{error}"),
                            &mut consecutive_errors,
                            &mut last_reported_error,
                        );
                    }
                }
                continue;
            }

            let mut saw_records = false;
            for _ in 0..MAX_WATCHER_BATCHES_PER_TICK {
                let last_id = state.last_record_id.load(Ordering::Relaxed);
                match records_after(last_id, WATCHER_RECORD_BATCH_LIMIT) {
                    Ok(records) => {
                        consecutive_errors = 0;
                        last_reported_error.clear();
                        if records.is_empty() {
                            match max_record_id() {
                                Ok(current_max_id) => {
                                    if let Some(reset_baseline_id) =
                                        notification_store_reset_baseline(last_id, current_max_id)
                                    {
                                        let message = format!(
                                            "检测到系统通知库记录 ID 从 {last_id} 回退到 {reset_baseline_id}，已重建监听基线，避免回放旧通知"
                                        );
                                        state
                                            .last_record_id
                                            .store(reset_baseline_id, Ordering::Relaxed);
                                        push_log(&state, message.clone());
                                        let _ = app.emit(
                                            "noticeflow://automation",
                                            AutomationEvent {
                                                kind: "status".to_string(),
                                                message,
                                            },
                                        );
                                    }
                                }
                                Err(error) => {
                                    baseline_ready = false;
                                    report_watcher_error(
                                        &app,
                                        &state,
                                        format!("检查通知库状态失败：{error}"),
                                        &mut consecutive_errors,
                                        &mut last_reported_error,
                                    );
                                }
                            }
                            break;
                        }
                        let should_continue = records.len() >= WATCHER_RECORD_BATCH_LIMIT;
                        saw_records = true;
                        process_new_records(&app, &state, records);
                        if !should_continue {
                            break;
                        }
                    }
                    Err(error) => {
                        baseline_ready = false;
                        report_watcher_error(
                            &app,
                            &state,
                            format!("轮询通知失败：{error}"),
                            &mut consecutive_errors,
                            &mut last_reported_error,
                        );
                        break;
                    }
                }
            }
            if !saw_records {
                flush_hit_counts(&state, false);
            }
        }
    });
}

fn process_new_records(app: &tauri::AppHandle, state: &AppState, records: Vec<NotificationRecord>) {
    if let Err(error) = notification_archive::upsert_records(&records) {
        push_log(state, format!("同步本地归档失败：{error}"));
    }
    if let Some(max_id) = records.iter().map(|record| record.id).max() {
        state.last_record_id.store(max_id, Ordering::Relaxed);
    }
    let filter_apps = filter_app_identifiers(state);
    let filter_mode = app_filter_mode(state);
    let visible_records = records
        .into_iter()
        .filter(|record| {
            is_visible_app(filter_mode, &filter_apps, &record.app_identifier)
                && !is_internal_automation_feedback_record(record)
        })
        .collect::<Vec<_>>();
    if visible_records.is_empty() {
        return;
    }
    let _ = app.emit(
        "noticeflow://automation",
        AutomationEvent {
            kind: "records".to_string(),
            message: format!("发现新通知：{} 条", visible_records.len()),
        },
    );
    let rules = state
        .rules
        .lock()
        .map(|rules| rules.clone())
        .unwrap_or_default();
    let mut hit_rule_ids = Vec::new();
    for record in visible_records {
        let matches = matching_rules(&rules, &record);
        for (rule, variables) in matches {
            if let Some(reason) = rule_automation_block_reason(&rule) {
                push_log(
                    state,
                    format!(
                        "跳过不完整规则：{} / {}（{}）",
                        rule.name, record.title, reason
                    ),
                );
                continue;
            }
            if automatic_action_is_in_flight(state, &rule, &record) {
                push_log(
                    state,
                    format!(
                        "跳过已在执行队列中的通知动作：{} / {}",
                        rule.name, record.title
                    ),
                );
                continue;
            }
            if let Some(remaining) = cooldown_remaining_seconds(state, &rule) {
                push_log(
                    state,
                    format!(
                        "规则冷却中，跳过动作：{}（剩余 {} 秒）",
                        rule.name, remaining
                    ),
                );
                continue;
            }
            hit_rule_ids.push(rule.id.clone());
            let message = format!("规则命中：{} / {}", rule.name, record.title);
            push_log(state, message.clone());
            let _ = app.emit(
                "noticeflow://automation",
                AutomationEvent {
                    kind: "match".to_string(),
                    message,
                },
            );
            match enqueue_action_job(state, rule.clone(), record.clone(), variables) {
                Ok(item) => {
                    commit_rule_cooldown(state, &rule);
                    let queue_message = format!(
                        "动作已入队：{} / {}（{} 个动作）",
                        item.rule_name, item.notification_title, item.action_count
                    );
                    push_log(state, queue_message.clone());
                    let _ = app.emit(
                        "noticeflow://automation",
                        AutomationEvent {
                            kind: "queue".to_string(),
                            message: queue_message,
                        },
                    );
                }
                Err(error) => {
                    let error_message =
                        format!("动作入队失败：{} / {}：{error}", rule.name, record.title);
                    push_log(state, error_message.clone());
                    let _ = app.emit(
                        "noticeflow://automation",
                        AutomationEvent {
                            kind: "error".to_string(),
                            message: error_message,
                        },
                    );
                }
            }
        }
    }
    if !hit_rule_ids.is_empty() {
        increment_hit_counts(state, &hit_rule_ids);
    }
}

fn is_internal_automation_feedback_record(record: &NotificationRecord) -> bool {
    if record.subtitle != AUTOMATION_RESULT_SUBTITLE {
        return false;
    }
    matches!(
        record.title.as_str(),
        AUTOMATION_RESULT_SUCCESS_TITLE | AUTOMATION_RESULT_FAILURE_TITLE
    )
}

fn rule_automation_block_reason(rule: &AutomationRule) -> Option<&'static str> {
    if !rule_has_trigger_app(rule) {
        return Some("缺少触发应用");
    }
    if !rule_has_match_conditions(rule) {
        return Some("缺少匹配条件");
    }
    if !rule_has_actions(rule) {
        return Some("缺少触发动作");
    }
    None
}

fn rule_has_trigger_app(rule: &AutomationRule) -> bool {
    rule.app_identifiers
        .as_deref()
        .is_some_and(|items| items.iter().any(|item| !item.trim().is_empty()))
}

fn rule_has_match_conditions(rule: &AutomationRule) -> bool {
    rule.match_conditions
        .as_deref()
        .is_some_and(|items| !items.is_empty())
}

fn rule_has_actions(rule: &AutomationRule) -> bool {
    rule.actions
        .as_deref()
        .is_some_and(|items| !items.is_empty())
}

fn automatic_action_is_in_flight(
    state: &AppState,
    rule: &AutomationRule,
    record: &NotificationRecord,
) -> bool {
    let current_matches = state
        .current_action_job
        .lock()
        .ok()
        .and_then(|current| current.clone())
        .is_some_and(|job| job.rule_id == rule.id && job.notification_id == record.id);
    if current_matches {
        return true;
    }

    state
        .action_queue
        .lock()
        .map(|queue| {
            queue
                .iter()
                .any(|job| job.rule.id == rule.id && job.record.id == record.id)
        })
        .unwrap_or(false)
}

fn watcher_sleep_duration(consecutive_errors: u32) -> Duration {
    match consecutive_errors {
        0 => WATCHER_IDLE_INTERVAL,
        1 => Duration::from_secs(3),
        2 => Duration::from_secs(8),
        3 => Duration::from_secs(15),
        4 => Duration::from_secs(30),
        _ => Duration::from_secs(60),
    }
}

fn notification_store_was_reset(last_record_id: i64, current_max_record_id: i64) -> bool {
    last_record_id > 0 && current_max_record_id < last_record_id
}

fn notification_store_reset_baseline(
    last_record_id: i64,
    current_max_record_id: i64,
) -> Option<i64> {
    notification_store_was_reset(last_record_id, current_max_record_id)
        .then_some(current_max_record_id)
}

fn report_watcher_error(
    app: &tauri::AppHandle,
    state: &AppState,
    message: String,
    consecutive_errors: &mut u32,
    last_reported_error: &mut String,
) {
    *consecutive_errors = consecutive_errors.saturating_add(1);
    let should_report = last_reported_error != &message || matches!(*consecutive_errors, 1 | 3 | 6);
    if !should_report {
        return;
    }
    *last_reported_error = message.clone();
    push_log(state, message.clone());
    let _ = app.emit(
        "noticeflow://automation",
        AutomationEvent {
            kind: "error".to_string(),
            message,
        },
    );
}

fn filter_app_identifiers(state: &AppState) -> Vec<String> {
    state
        .settings
        .lock()
        .map(|settings| settings.ignored_app_identifiers.clone())
        .unwrap_or_default()
}

fn app_filter_mode(state: &AppState) -> AppFilterMode {
    state
        .settings
        .lock()
        .map(
            |settings| match normalized_app_filter_mode(settings.app_filter_mode.as_deref()) {
                "include" => AppFilterMode::Include,
                _ => AppFilterMode::Exclude,
            },
        )
        .unwrap_or(AppFilterMode::Exclude)
}

fn normalized_app_filter_mode(mode: Option<&str>) -> &'static str {
    match mode.map(str::trim) {
        Some("include") => "include",
        _ => "exclude",
    }
}

fn is_visible_app(mode: AppFilterMode, app_identifiers: &[String], app_identifier: &str) -> bool {
    match mode {
        AppFilterMode::Exclude => !is_ignored_app(app_identifiers, app_identifier),
        AppFilterMode::Include => is_ignored_app(app_identifiers, app_identifier),
    }
}

fn is_ignored_app(ignored: &[String], app_identifier: &str) -> bool {
    let app_identifier = app_identifier.trim();
    if app_identifier.is_empty() {
        return false;
    }
    ignored
        .iter()
        .map(|ignored_app| ignored_app.trim())
        .filter(|ignored_app| !ignored_app.is_empty())
        .any(|ignored_app| ignored_app.eq_ignore_ascii_case(app_identifier))
}

fn cooldown_remaining_seconds(state: &AppState, rule: &AutomationRule) -> Option<u64> {
    let cooldown_seconds = rule.cooldown_seconds.unwrap_or(0);
    if cooldown_seconds == 0 {
        return None;
    }
    let cooldown = Duration::from_secs(cooldown_seconds);
    let now = Instant::now();
    let Ok(cooldowns) = state.rule_cooldowns.lock() else {
        return None;
    };
    if let Some(last_run) = cooldowns.get(&rule.id) {
        let elapsed = now.duration_since(*last_run);
        if elapsed < cooldown {
            return Some(cooldown.saturating_sub(elapsed).as_secs().max(1));
        }
    }
    None
}

fn commit_rule_cooldown(state: &AppState, rule: &AutomationRule) {
    if rule.cooldown_seconds.unwrap_or(0) == 0 {
        return;
    }
    if let Ok(mut cooldowns) = state.rule_cooldowns.lock() {
        cooldowns.insert(rule.id.clone(), Instant::now());
    }
}

fn increment_hit_counts(state: &AppState, rule_ids: &[String]) {
    if rule_ids.is_empty() {
        return;
    };

    let mut hit_counts = HashMap::new();
    for rule_id in rule_ids {
        *hit_counts.entry(rule_id.clone()).or_insert(0) += 1;
    }

    let Ok(mut rules) = state.rules.lock() else {
        return;
    };
    let Ok(mut pending_hit_counts) = state.pending_hit_counts.lock() else {
        return;
    };

    for (rule_id, count) in hit_counts {
        if let Some(rule) = rules.iter_mut().find(|rule| rule.id == rule_id) {
            rule.hit_count = Some(rule.hit_count.unwrap_or(0).saturating_add(count));
            *pending_hit_counts.entry(rule_id).or_insert(0) += count;
        }
    }

    drop(rules);
    drop(pending_hit_counts);
    flush_hit_counts(state, false);
}

fn flush_hit_counts(state: &AppState, force: bool) {
    let now = Instant::now();
    let elapsed = state
        .last_hit_count_save
        .lock()
        .map(|last_save| now.duration_since(*last_save))
        .unwrap_or(HIT_COUNT_FLUSH_INTERVAL);
    let Ok(rules) = state.rules.lock() else {
        return;
    };
    let Ok(mut pending_hit_counts) = state.pending_hit_counts.lock() else {
        return;
    };
    let pending_total = pending_hit_counts.values().sum::<u64>();
    if !should_flush_hit_counts(pending_total, elapsed, force) {
        return;
    }

    let data_directory = match data_directory_for_state(state) {
        Ok(path) => path,
        Err(error) => {
            push_log(state, format!("保存规则命中次数失败：{error}"));
            return;
        }
    };
    if let Err(error) = save_rule_file_in_dir(&data_directory, &rules) {
        push_log(state, format!("保存规则命中次数失败：{error}"));
        return;
    }
    pending_hit_counts.clear();
    if let Ok(mut last_save) = state.last_hit_count_save.lock() {
        *last_save = now;
    }
}

fn should_flush_hit_counts(pending_total: u64, elapsed: Duration, force: bool) -> bool {
    pending_total > 0
        && (force
            || pending_total >= HIT_COUNT_FLUSH_THRESHOLD
            || elapsed >= HIT_COUNT_FLUSH_INTERVAL)
}

fn should_rollback_launch_at_login(previous: bool, target: bool) -> bool {
    previous != target
}

struct TargetDataDirectoryPlan {
    rules: Vec<AutomationRule>,
    write_rules: bool,
}

fn rules_for_target_data_directory(
    current_data_directory: &Path,
    target_data_directory: &Path,
    current_rules: &[AutomationRule],
) -> Result<TargetDataDirectoryPlan, Box<dyn std::error::Error>> {
    let target_rules_path = target_data_directory.join("rules.json");
    if current_data_directory != target_data_directory && target_rules_path.exists() {
        return Ok(TargetDataDirectoryPlan {
            rules: load_rules_from_dir(target_data_directory)?,
            write_rules: false,
        });
    }
    Ok(TargetDataDirectoryPlan {
        rules: current_rules.to_vec(),
        write_rules: current_data_directory != target_data_directory,
    })
}

fn commit_target_data_directory(
    current_data_directory: &Path,
    target_data_directory: &Path,
    plan: &TargetDataDirectoryPlan,
) -> Result<(), String> {
    if current_data_directory == target_data_directory {
        return Ok(());
    }
    if plan.write_rules {
        save_rule_file_in_dir(target_data_directory, &plan.rules)
            .map_err(|error| error.to_string())?;
    }
    copy_data_file_if_missing(
        &current_data_directory.join("notifications.sqlite"),
        &target_data_directory.join("notifications.sqlite"),
    )
    .map_err(|error| error.to_string())
}

fn ensure_data_directory_writable(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)?;
    let test_path = path.join(format!(
        ".noticeflow-write-test-{}.tmp",
        uuid::Uuid::new_v4()
    ));
    fs::write(&test_path, b"ok")?;
    fs::remove_file(test_path)?;
    Ok(())
}

fn copy_data_file_if_missing(source: &Path, target: &Path) -> std::io::Result<()> {
    if !source.exists() || target.exists() {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let file_name = target
        .file_name()
        .and_then(|item| item.to_str())
        .unwrap_or("notifications.sqlite");
    let temp_path = target.with_file_name(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()));
    if let Err(error) = fs::copy(source, &temp_path).and_then(|_| {
        if target.exists() {
            return Ok(());
        }
        fs::rename(&temp_path, target)
    }) {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    let _ = fs::remove_file(temp_path);
    Ok(())
}

fn push_logs(state: &AppState, logs: Vec<String>) {
    for log in logs {
        push_log(state, log);
    }
}

fn record_action_history(
    state: &AppState,
    rule: &AutomationRule,
    record: &NotificationRecord,
    executions: &[action_runner::ActionExecution],
    queue_id: Option<&str>,
    variables: &BTreeMap<String, String>,
) {
    let variables_json = visible_variables_json(variables);
    let entries = executions
        .iter()
        .enumerate()
        .map(|(index, execution)| ActionHistoryEntry {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            queue_id: queue_id.map(str::to_string),
            rule_id: rule.id.clone(),
            rule_name: rule.name.clone(),
            notification_id: record.id,
            notification_title: record.title.clone(),
            app_identifier: record.app_identifier.clone(),
            action_index: index as u32,
            action_type: execution.action_type.clone(),
            success: execution.success,
            message: execution.message.clone(),
            duration_ms: execution.duration_ms.min(u64::MAX as u128) as u64,
            attempt_count: execution.attempt_count,
            variables_json: variables_json.clone(),
        })
        .collect::<Vec<_>>();
    if let Err(error) = notification_archive::append_action_history(&entries) {
        push_log(state, format!("保存动作历史失败：{error}"));
    }
    let Ok(mut history) = state.action_history.lock() else {
        return;
    };
    history.extend(entries);
    let overflow = history.len().saturating_sub(MAX_ACTION_HISTORY_ENTRIES);
    if overflow > 0 {
        history.drain(0..overflow);
    }
}

fn push_log(state: &AppState, log: String) {
    if let Ok(mut logs) = state.logs.lock() {
        if logs.last().map(|last| last == &log).unwrap_or(false) {
            return;
        }
        logs.push(log);
        let overflow = logs.len().saturating_sub(200);
        if overflow > 0 {
            logs.drain(0..overflow);
        }
    }
}

#[cfg(test)]
mod watcher_tests {
    use super::{
        analyze_rule_match, copy_data_file_if_missing, is_ignored_app,
        is_internal_automation_feedback_record, is_visible_app, normalized_app_filter_mode,
        normalized_notification_list_limit, notification_identity_matches,
        notification_store_reset_baseline, notification_store_was_reset, osascript_stdout_path,
        rule_automation_block_reason, rule_matches_record_app, rules_for_target_data_directory,
        should_flush_hit_counts, should_rollback_launch_at_login, validate_rule_for_save,
        validate_rules_for_save, watcher_sleep_duration, AppFilterMode, AutomationRule,
        NotificationRecordIdentity, AUTOMATION_RESULT_FAILURE_TITLE, AUTOMATION_RESULT_SUBTITLE,
        AUTOMATION_RESULT_SUCCESS_TITLE, WATCHER_IDLE_INTERVAL,
    };
    use crate::notification_db::NotificationRecord;
    use crate::rules::{ActionConfig, MatchCondition};
    use crate::variables::{
        VariableExtractionMethod, VariableExtractionRule, VariableExtractionSource,
    };
    use chrono::Utc;
    use std::collections::BTreeMap;
    use std::fs;
    use std::time::Duration;
    use uuid::Uuid;

    fn test_record(title: &str) -> NotificationRecord {
        NotificationRecord {
            id: 1,
            app_identifier: "com.example.App".to_string(),
            app_name: "Example".to_string(),
            delivered_at: Utc::now(),
            title: title.to_string(),
            subtitle: AUTOMATION_RESULT_SUBTITLE.to_string(),
            body: "body".to_string(),
        }
    }

    fn valid_rule() -> AutomationRule {
        AutomationRule {
            id: "rule-1".to_string(),
            name: "规则".to_string(),
            enabled: Some(true),
            trigger_time: None,
            cooldown_seconds: None,
            hit_count: None,
            app_identifiers: Some(vec!["com.example.App".to_string()]),
            match_conditions: Some(vec![MatchCondition {
                variable_name: "title".to_string(),
                operator_type: "contains".to_string(),
                expected_value: Some("审批".to_string()),
                case_sensitive: false,
            }]),
            variable_extractions: None,
            actions: Some(vec![ActionConfig {
                action_type: "send_notification".to_string(),
                parameters: BTreeMap::from([
                    ("title".to_string(), "{{title}}".to_string()),
                    ("body".to_string(), "{{body}}".to_string()),
                ]),
            }]),
        }
    }

    #[test]
    fn watcher_backoff_is_capped() {
        assert_eq!(watcher_sleep_duration(0), WATCHER_IDLE_INTERVAL);
        assert_eq!(watcher_sleep_duration(1), Duration::from_secs(3));
        assert_eq!(watcher_sleep_duration(2), Duration::from_secs(8));
        assert_eq!(watcher_sleep_duration(3), Duration::from_secs(15));
        assert_eq!(watcher_sleep_duration(4), Duration::from_secs(30));
        assert_eq!(watcher_sleep_duration(5), Duration::from_secs(60));
        assert_eq!(watcher_sleep_duration(50), Duration::from_secs(60));
    }

    #[test]
    fn notification_list_limit_is_capped() {
        assert_eq!(normalized_notification_list_limit(None), 200);
        assert_eq!(normalized_notification_list_limit(Some(0)), 0);
        assert_eq!(normalized_notification_list_limit(Some(400)), 400);
        assert_eq!(normalized_notification_list_limit(Some(10_000)), 1_000);
    }

    #[test]
    fn osascript_path_output_preserves_path_whitespace() {
        assert_eq!(
            osascript_stdout_path(b"/Users/example/script name \n"),
            Some("/Users/example/script name ".to_string())
        );
        assert_eq!(
            osascript_stdout_path(b"/Users/example/folder /\r\n"),
            Some("/Users/example/folder /".to_string())
        );
        assert_eq!(osascript_stdout_path(b"\n"), None);
    }

    #[test]
    fn watcher_detects_notification_store_id_reset() {
        assert!(notification_store_was_reset(57_317, 4));
        assert!(notification_store_was_reset(57_317, 0));
        assert!(!notification_store_was_reset(0, 0));
        assert!(!notification_store_was_reset(4, 57_317));
        assert!(!notification_store_was_reset(4, 4));
    }

    #[test]
    fn watcher_reset_baseline_uses_current_max_id() {
        assert_eq!(notification_store_reset_baseline(57_317, 4), Some(4));
        assert_eq!(notification_store_reset_baseline(57_317, 0), Some(0));
        assert_eq!(notification_store_reset_baseline(4, 57_317), None);
        assert_eq!(notification_store_reset_baseline(4, 4), None);
    }

    #[test]
    fn internal_automation_feedback_is_not_automation_input() {
        assert!(is_internal_automation_feedback_record(&test_record(
            AUTOMATION_RESULT_SUCCESS_TITLE
        )));
        assert!(is_internal_automation_feedback_record(&test_record(
            AUTOMATION_RESULT_FAILURE_TITLE
        )));
        assert!(!is_internal_automation_feedback_record(&test_record(
            "用户自己的通知"
        )));
        let mut same_title_without_internal_subtitle = test_record(AUTOMATION_RESULT_SUCCESS_TITLE);
        same_title_without_internal_subtitle.subtitle.clear();
        assert!(!is_internal_automation_feedback_record(
            &same_title_without_internal_subtitle
        ));
    }

    #[test]
    fn backend_validation_rejects_broad_or_empty_rules() {
        assert!(validate_rule_for_save(&valid_rule()).is_ok());

        let mut without_app = valid_rule();
        without_app.app_identifiers = Some(Vec::new());
        assert!(validate_rule_for_save(&without_app)
            .expect_err("missing app should be rejected")
            .contains("触发应用"));
        assert_eq!(
            rule_automation_block_reason(&without_app),
            Some("缺少触发应用")
        );

        let mut without_conditions = valid_rule();
        without_conditions.match_conditions = Some(Vec::new());
        assert!(validate_rule_for_save(&without_conditions)
            .expect_err("missing conditions should be rejected")
            .contains("匹配条件"));
        assert_eq!(
            rule_automation_block_reason(&without_conditions),
            Some("缺少匹配条件")
        );

        let mut without_actions = valid_rule();
        without_actions.actions = Some(Vec::new());
        assert!(validate_rule_for_save(&without_actions)
            .expect_err("missing actions should be rejected")
            .contains("触发动作"));
        assert_eq!(
            rule_automation_block_reason(&without_actions),
            Some("缺少触发动作")
        );

        let mut disabled_incomplete = valid_rule();
        disabled_incomplete.enabled = Some(false);
        disabled_incomplete.match_conditions = Some(Vec::new());
        assert!(validate_rules_for_save(&[disabled_incomplete]).is_ok());
    }

    #[test]
    fn backend_validation_rejects_invalid_rule_details() {
        let mut invalid_operator = valid_rule();
        invalid_operator.match_conditions.as_mut().unwrap()[0].operator_type =
            "unknown".to_string();
        assert!(validate_rule_for_save(&invalid_operator)
            .expect_err("invalid operator should be rejected")
            .contains("匹配方式"));

        let mut empty_expected_value = valid_rule();
        empty_expected_value.match_conditions.as_mut().unwrap()[0].expected_value =
            Some(" ".to_string());
        assert!(validate_rule_for_save(&empty_expected_value)
            .expect_err("empty expected value should be rejected")
            .contains("匹配值"));

        let mut is_empty_condition = valid_rule();
        let condition = &mut is_empty_condition.match_conditions.as_mut().unwrap()[0];
        condition.operator_type = "is_empty".to_string();
        condition.expected_value = None;
        assert!(validate_rule_for_save(&is_empty_condition).is_ok());

        let mut invalid_variable_name = valid_rule();
        invalid_variable_name.variable_extractions = Some(vec![VariableExtractionRule {
            name: "1bad".to_string(),
            source: VariableExtractionSource::Body,
            method: VariableExtractionMethod::Regex,
            pattern: Some("(.+)".to_string()),
            end_pattern: None,
            group_index: Some(1),
        }]);
        assert!(validate_rule_for_save(&invalid_variable_name)
            .expect_err("invalid variable name should be rejected")
            .contains("名称"));

        let mut empty_variable_regex = valid_rule();
        empty_variable_regex.variable_extractions = Some(vec![VariableExtractionRule {
            name: "target".to_string(),
            source: VariableExtractionSource::Body,
            method: VariableExtractionMethod::Regex,
            pattern: Some(" ".to_string()),
            end_pattern: None,
            group_index: Some(1),
        }]);
        assert!(validate_rule_for_save(&empty_variable_regex)
            .expect_err("empty variable regex should be rejected")
            .contains("正则"));

        let mut unknown_action = valid_rule();
        unknown_action.actions.as_mut().unwrap()[0].action_type = "unknown".to_string();
        assert!(validate_rule_for_save(&unknown_action)
            .expect_err("unknown action should be rejected")
            .contains("类型"));

        let mut invalid_shell = valid_rule();
        invalid_shell.actions = Some(vec![ActionConfig {
            action_type: "run_shell".to_string(),
            parameters: BTreeMap::from([
                ("shell".to_string(), "fish".to_string()),
                ("script".to_string(), "echo ok".to_string()),
            ]),
        }]);
        assert!(validate_rule_for_save(&invalid_shell)
            .expect_err("invalid shell should be rejected")
            .contains("Shell"));

        let mut invalid_env_json = valid_rule();
        invalid_env_json.actions.as_mut().unwrap()[0]
            .parameters
            .insert("env_json".to_string(), "{\"A\":1}".to_string());
        assert!(validate_rule_for_save(&invalid_env_json)
            .expect_err("invalid env json should be rejected")
            .contains("环境变量 JSON"));

        let mut invalid_http_headers = valid_rule();
        invalid_http_headers.actions = Some(vec![ActionConfig {
            action_type: "http_request".to_string(),
            parameters: BTreeMap::from([
                ("url".to_string(), "https://example.com".to_string()),
                ("headers".to_string(), "{\"Accept\":1}".to_string()),
            ]),
        }]);
        assert!(validate_rule_for_save(&invalid_http_headers)
            .expect_err("invalid http headers should be rejected")
            .contains("Headers JSON"));

        let mut invalid_http_retry = valid_rule();
        invalid_http_retry.actions = Some(vec![ActionConfig {
            action_type: "http_request".to_string(),
            parameters: BTreeMap::from([
                ("url".to_string(), "https://example.com".to_string()),
                ("retry_count".to_string(), "6".to_string()),
            ]),
        }]);
        assert!(validate_rule_for_save(&invalid_http_retry)
            .expect_err("invalid http retry should be rejected")
            .contains("重试次数"));
    }

    #[test]
    fn rule_match_explanation_trims_app_identifiers() {
        let mut rule = valid_rule();
        rule.app_identifiers = Some(vec![" COM.EXAMPLE.APP ".to_string(), " ".to_string()]);

        assert!(rule_matches_record_app(&rule, " com.example.app "));
        assert!(!rule_matches_record_app(&rule, " "));
    }

    #[test]
    fn rule_match_explanation_rejects_incomplete_drafts() {
        let record = test_record("审批");

        let mut without_app = valid_rule();
        without_app.app_identifiers = Some(Vec::new());
        let app_analysis = analyze_rule_match(&without_app, &record);
        assert!(!app_analysis.explanation.matched);
        assert!(app_analysis.explanation.message.contains("缺少触发应用"));

        let mut without_conditions = valid_rule();
        without_conditions.match_conditions = Some(Vec::new());
        let condition_analysis = analyze_rule_match(&without_conditions, &record);
        assert!(!condition_analysis.explanation.matched);
        assert!(condition_analysis
            .explanation
            .message
            .contains("缺少匹配条件"));
    }

    #[test]
    fn ignored_app_matching_is_normalized() {
        let ignored = vec![
            " com.apple.mail ".to_string(),
            "".to_string(),
            "COM.EXAMPLE.APP".to_string(),
        ];

        assert!(is_ignored_app(&ignored, "com.apple.Mail"));
        assert!(is_ignored_app(&ignored, " com.example.app "));
        assert!(!is_ignored_app(&ignored, ""));
        assert!(!is_ignored_app(&ignored, "com.example.other"));
    }

    #[test]
    fn app_filter_modes_include_or_exclude_selected_apps() {
        let apps = vec!["com.apple.mail".to_string()];

        assert_eq!(normalized_app_filter_mode(Some("include")), "include");
        assert_eq!(normalized_app_filter_mode(Some("bad")), "exclude");
        assert!(is_visible_app(
            AppFilterMode::Exclude,
            &apps,
            "com.example.other"
        ));
        assert!(!is_visible_app(
            AppFilterMode::Exclude,
            &apps,
            "com.apple.mail"
        ));
        assert!(is_visible_app(
            AppFilterMode::Include,
            &apps,
            "com.apple.mail"
        ));
        assert!(!is_visible_app(
            AppFilterMode::Include,
            &apps,
            "com.example.other"
        ));
    }

    #[test]
    fn notification_identity_requires_same_content_and_timestamp() {
        let record = NotificationRecord {
            id: 42,
            app_identifier: "com.example.App".to_string(),
            app_name: "Example".to_string(),
            delivered_at: Utc::now(),
            title: "标题".to_string(),
            subtitle: "副标题".to_string(),
            body: "内容".to_string(),
        };
        let identity = NotificationRecordIdentity {
            id: record.id,
            app_identifier: record.app_identifier.clone(),
            app_name: record.app_name.clone(),
            delivered_at: record.delivered_at.to_rfc3339(),
            title: record.title.clone(),
            subtitle: record.subtitle.clone(),
            body: record.body.clone(),
        };

        assert!(notification_identity_matches(&record, &identity));

        let mut different_app_case = identity.clone();
        different_app_case.app_identifier = "COM.EXAMPLE.APP".to_string();
        assert!(notification_identity_matches(&record, &different_app_case));

        let mut different_title = identity.clone();
        different_title.title = "其他标题".to_string();
        assert!(!notification_identity_matches(&record, &different_title));

        let mut different_time = identity;
        different_time.delivered_at =
            (record.delivered_at + chrono::Duration::seconds(1)).to_rfc3339();
        assert!(!notification_identity_matches(&record, &different_time));
    }

    #[test]
    fn hit_count_flush_requires_pending_work() {
        assert!(!should_flush_hit_counts(0, Duration::from_secs(60), true));
        assert!(!should_flush_hit_counts(0, Duration::from_secs(60), false));
    }

    #[test]
    fn hit_count_flush_uses_threshold_time_or_force() {
        assert!(should_flush_hit_counts(20, Duration::from_secs(0), false));
        assert!(should_flush_hit_counts(1, Duration::from_secs(5), false));
        assert!(should_flush_hit_counts(1, Duration::from_secs(0), true));
        assert!(!should_flush_hit_counts(1, Duration::from_secs(4), false));
    }

    #[test]
    fn launch_at_login_rollback_only_when_value_changed() {
        assert!(!should_rollback_launch_at_login(false, false));
        assert!(!should_rollback_launch_at_login(true, true));
        assert!(should_rollback_launch_at_login(false, true));
        assert!(should_rollback_launch_at_login(true, false));
    }

    #[test]
    fn copy_data_file_if_missing_preserves_existing_target() {
        let base = std::env::temp_dir().join(format!("noticeflow-copy-test-{}", Uuid::new_v4()));
        let source = base.join("source.sqlite");
        let target = base.join("nested").join("target.sqlite");
        fs::create_dir_all(&base).expect("temp directory should be created");
        fs::write(&source, "source").expect("source should be written");

        copy_data_file_if_missing(&source, &target).expect("file should copy");
        assert_eq!(fs::read_to_string(&target).ok().as_deref(), Some("source"));
        let leftover_temp_files = fs::read_dir(target.parent().unwrap())
            .expect("target directory should exist")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftover_temp_files, 0);

        fs::write(&source, "next").expect("source should be updated");
        fs::write(&target, "target").expect("target should be updated");
        copy_data_file_if_missing(&source, &target).expect("existing target should be kept");
        assert_eq!(fs::read_to_string(&target).ok().as_deref(), Some("target"));

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn target_data_directory_existing_rules_are_loaded_not_overwritten() {
        let directory =
            std::env::temp_dir().join(format!("noticeflow-target-rules-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("directory should be created");
        fs::write(
            directory.join("rules.json"),
            r#"{"rules":[{"id":"target-id","name":"目标规则"}]}"#,
        )
        .expect("target rules should be written");
        let current_rules = vec![AutomationRule {
            id: "current-id".to_string(),
            name: "当前规则".to_string(),
            enabled: Some(true),
            trigger_time: None,
            cooldown_seconds: None,
            hit_count: None,
            app_identifiers: None,
            match_conditions: None,
            variable_extractions: None,
            actions: None,
        }];

        let current_directory =
            std::env::temp_dir().join(format!("noticeflow-current-rules-{}", Uuid::new_v4()));
        let plan = rules_for_target_data_directory(&current_directory, &directory, &current_rules)
            .expect("target rules should be loaded");

        assert_eq!(plan.rules.len(), 1);
        assert_eq!(plan.rules[0].id, "target-id");
        assert!(!plan.write_rules);
        assert_eq!(
            fs::read_to_string(directory.join("rules.json"))
                .ok()
                .as_deref(),
            Some(r#"{"rules":[{"id":"target-id","name":"目标规则"}]}"#)
        );

        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn target_data_directory_new_rules_are_planned_not_written_immediately() {
        let current_directory =
            std::env::temp_dir().join(format!("noticeflow-current-rules-{}", Uuid::new_v4()));
        let target_directory =
            std::env::temp_dir().join(format!("noticeflow-target-rules-{}", Uuid::new_v4()));
        let current_rules = vec![AutomationRule {
            id: "current-id".to_string(),
            name: "当前规则".to_string(),
            enabled: Some(true),
            trigger_time: None,
            cooldown_seconds: None,
            hit_count: None,
            app_identifiers: None,
            match_conditions: None,
            variable_extractions: None,
            actions: None,
        }];

        let plan =
            rules_for_target_data_directory(&current_directory, &target_directory, &current_rules)
                .expect("target rules should be planned");

        assert_eq!(plan.rules.len(), 1);
        assert_eq!(plan.rules[0].id, "current-id");
        assert!(plan.write_rules);
        assert!(!target_directory.join("rules.json").exists());
    }
}

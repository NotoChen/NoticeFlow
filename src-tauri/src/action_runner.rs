use crate::rules::ActionConfig;
use crate::variables::replace_variables;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant, SystemTime};
use uuid::Uuid;

const DEFAULT_ACTION_TIMEOUT_SECONDS: u64 = 30;
const MAX_ACTION_TIMEOUT_SECONDS: u64 = 300;
const MAX_ACTION_OUTPUT_LOG_BYTES: usize = 16 * 1024;
const MAX_ACTION_OUTPUT_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ACTION_OUTPUT_SNIPPET_BYTES: usize = 8 * 1024;
const MAX_DRY_RUN_VALUE_CHARS: usize = 600;
const ACTION_OUTPUT_FILE_PREFIX: &str = "noticeflow-action-";
const ACTION_OUTPUT_FILE_SUFFIX: &str = ".log";
const STALE_ACTION_OUTPUT_AGE: Duration = Duration::from_secs(60 * 60 * 24);
static STALE_ACTION_OUTPUT_CLEANUP: OnceLock<()> = OnceLock::new();

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionExecution {
    pub action_type: String,
    pub success: bool,
    pub message: String,
    pub output: Option<String>,
    pub duration_ms: u64,
    pub attempt_count: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionDryRun {
    pub action_type: String,
    pub parameters: Vec<DryRunParameter>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DryRunParameter {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug)]
struct CommandOutput {
    stdout: String,
    stderr: String,
}

#[derive(Clone, Debug)]
struct ActionOutcome {
    message: String,
    output: Option<String>,
}

impl ActionOutcome {
    fn plain(message: String) -> Self {
        Self {
            message,
            output: None,
        }
    }
}

#[derive(Clone, Debug)]
struct ActionRunSuccess {
    message: String,
    output: Option<String>,
    attempt_count: u32,
}

#[derive(Clone, Debug)]
struct ActionRunFailure {
    message: String,
    attempt_count: u32,
}

pub fn run_actions_detailed(
    actions: &[ActionConfig],
    variables: &BTreeMap<String, String>,
) -> Vec<ActionExecution> {
    actions
        .iter()
        .map(|action| {
            let started_at = Instant::now();
            let result = run_action_with_details(action, variables);
            let duration_ms = started_at.elapsed().as_millis().min(u64::MAX as u128) as u64;
            match result {
                Ok(success) => ActionExecution {
                    action_type: action.action_type.clone(),
                    success: true,
                    message: success.message,
                    output: success.output,
                    duration_ms,
                    attempt_count: success.attempt_count.max(1),
                },
                Err(failure) => ActionExecution {
                    action_type: action.action_type.clone(),
                    success: false,
                    message: format!("动作失败：{}", failure.message),
                    output: None,
                    duration_ms,
                    attempt_count: failure.attempt_count.max(1),
                },
            }
        })
        .collect()
}

/// 只做变量替换，不执行任何动作；用于测试前预览实际会执行的内容。
pub fn dry_run_actions(
    actions: &[ActionConfig],
    variables: &BTreeMap<String, String>,
) -> Vec<ActionDryRun> {
    actions
        .iter()
        .map(|action| ActionDryRun {
            action_type: action.action_type.clone(),
            parameters: action
                .parameters
                .iter()
                .filter(|(_, value)| !value.trim().is_empty())
                .map(|(name, value)| DryRunParameter {
                    name: name.clone(),
                    value: truncate_display_value(&replace_variables(value, variables)),
                })
                .collect(),
        })
        .collect()
}

fn truncate_display_value(value: &str) -> String {
    if value.chars().count() <= MAX_DRY_RUN_VALUE_CHARS {
        return value.to_string();
    }
    let truncated = value.chars().take(MAX_DRY_RUN_VALUE_CHARS).collect::<String>();
    format!("{truncated}…（已截断）")
}

fn run_action_with_details(
    action: &ActionConfig,
    variables: &BTreeMap<String, String>,
) -> Result<ActionRunSuccess, ActionRunFailure> {
    if action.action_type == "http_request" {
        return http_request(action, variables);
    }
    run_action_inner(action, variables)
        .map(|outcome| ActionRunSuccess {
            message: outcome.message,
            output: outcome.output,
            attempt_count: 1,
        })
        .map_err(|message| ActionRunFailure {
            message,
            attempt_count: 1,
        })
}

fn run_action_inner(
    action: &ActionConfig,
    variables: &BTreeMap<String, String>,
) -> Result<ActionOutcome, String> {
    match action.action_type.as_str() {
        "open_url" => open_url(action, variables).map(ActionOutcome::plain),
        "run_shell" => run_shell(action, variables),
        "run_javascript" => run_process_script(
            "/usr/bin/osascript",
            &["-l", "JavaScript", "-e"],
            "code",
            action,
            variables,
            "JavaScript",
        ),
        "run_python" => run_python(action, variables),
        "run_applescript" => run_process_script(
            "/usr/bin/osascript",
            &["-e"],
            "script",
            action,
            variables,
            "AppleScript",
        ),
        "open_app" => open_app(action, variables).map(ActionOutcome::plain),
        "activate_app" => open_app(action, variables).map(ActionOutcome::plain),
        "send_notification" => send_notification(action, variables).map(ActionOutcome::plain),
        other => Err(format!("未知动作类型：{other}")),
    }
}

fn run_shell(
    action: &ActionConfig,
    variables: &BTreeMap<String, String>,
) -> Result<ActionOutcome, String> {
    let executable = shell_executable(action)?;
    let args = shell_args(action)?;
    run_process_script(executable, &args, "script", action, variables, "Shell")
}

fn shell_executable(action: &ActionConfig) -> Result<&'static str, String> {
    let shell = action
        .parameters
        .get("shell")
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "bash".to_string());
    match shell.as_str() {
        "bash" => Ok("/bin/bash"),
        "zsh" => Ok("/bin/zsh"),
        other => Err(format!("不支持的 Shell：{other}")),
    }
}

fn shell_args(action: &ActionConfig) -> Result<Vec<&'static str>, String> {
    let mode = action
        .parameters
        .get("shell_mode")
        .or_else(|| action.parameters.get("shellMode"))
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "standard".to_string());
    match mode.as_str() {
        "standard" => Ok(vec!["-c"]),
        "login" => Ok(vec!["-l", "-c"]),
        "interactive" => Ok(vec!["-i", "-c"]),
        "login_interactive" | "login-interactive" => Ok(vec!["-l", "-i", "-c"]),
        other => Err(format!("不支持的 Shell 模式：{other}")),
    }
}

fn open_url(action: &ActionConfig, variables: &BTreeMap<String, String>) -> Result<String, String> {
    let template = action
        .parameters
        .get("url")
        .or_else(|| action.parameters.get("url_pattern"))
        .cloned()
        .unwrap_or_else(|| "{{url}}".to_string());
    let url = replace_variables(&template, variables);
    let url = url.trim();
    if url.is_empty() {
        return Err("URL 为空".to_string());
    }
    if url.starts_with('-') {
        return Err("URL 不能以 - 开头".to_string());
    }

    let browser = action
        .parameters
        .get("browser")
        .cloned()
        .unwrap_or_default();
    let mut command = Command::new("/usr/bin/open");
    if !browser.is_empty() && browser != "default" {
        command.args(["-b", &browser]);
    }
    command.arg(url);
    run_command(command, "打开链接", action_timeout(action))?;
    Ok(format!("已打开链接：{url}"))
}

fn open_app(action: &ActionConfig, variables: &BTreeMap<String, String>) -> Result<String, String> {
    let bundle_id = action
        .parameters
        .get("bundle_id")
        .or_else(|| action.parameters.get("bundleId"))
        .ok_or_else(|| "未配置应用 Bundle ID".to_string())?;
    let bundle_id = replace_variables(bundle_id, variables);
    if bundle_id.trim().is_empty() {
        return Err("应用 Bundle ID 为空".to_string());
    }
    let mut command = Command::new("/usr/bin/open");
    command.args(["-b", &bundle_id]);
    run_command(command, "打开应用", action_timeout(action))?;
    Ok(format!("已打开应用：{bundle_id}"))
}

fn send_notification(
    action: &ActionConfig,
    variables: &BTreeMap<String, String>,
) -> Result<String, String> {
    let title = replace_variables(
        action
            .parameters
            .get("title")
            .map(String::as_str)
            .unwrap_or("NoticeFlow"),
        variables,
    );
    let body = replace_variables(
        action
            .parameters
            .get("body")
            .map(String::as_str)
            .unwrap_or(""),
        variables,
    );
    let script = format!(
        "display notification {} with title {}",
        applescript_quote(&body),
        applescript_quote(&title)
    );
    let mut command = Command::new("/usr/bin/osascript");
    command.args(["-e", &script]);
    run_command(command, "发送通知", action_timeout(action))?;
    Ok(format!("已发送通知：{title}"))
}

fn http_request(
    action: &ActionConfig,
    variables: &BTreeMap<String, String>,
) -> Result<ActionRunSuccess, ActionRunFailure> {
    let url = match action
        .parameters
        .get("url")
        .map(|value| replace_variables(value, variables))
    {
        Some(url) => url,
        None => return Err(action_failure("未配置 HTTP URL", 1)),
    };
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err(action_failure("HTTP URL 为空", 1));
    }
    if url.starts_with('-') {
        return Err(action_failure("HTTP URL 不能以 - 开头", 1));
    }
    let method = http_method(action);
    let retry_count = action_u64_parameter(action, &["retry_count", "retryCount"])
        .unwrap_or(0)
        .min(5);
    let retry_interval = Duration::from_secs(
        action_u64_parameter(action, &["retry_interval_seconds", "retryIntervalSeconds"])
            .unwrap_or(1)
            .min(60),
    );
    let response_contains = action
        .parameters
        .get("response_contains")
        .or_else(|| action.parameters.get("responseContains"))
        .map(|value| replace_variables(value, variables))
        .filter(|value| !value.is_empty());
    let mut last_error = None;
    for attempt in 0..=retry_count {
        let attempt_count = attempt.saturating_add(1) as u32;
        let command = match http_request_command(action, variables, &method, &url) {
            Ok(command) => command,
            Err(error) => return Err(action_failure(error, attempt_count)),
        };
        match run_command(command, "HTTP 请求", action_timeout(action)) {
            Ok(output) => {
                if let Some(expected) = &response_contains {
                    if !output.stdout.contains(expected.as_str()) {
                        last_error = Some(format!("HTTP 响应不包含期望内容：{expected}"));
                        if attempt < retry_count {
                            thread::sleep(retry_interval);
                            continue;
                        }
                        return Err(action_failure(
                            last_error.unwrap_or_else(|| "HTTP 响应断言失败".to_string()),
                            attempt_count,
                        ));
                    }
                }
                return Ok(ActionRunSuccess {
                    message: format!("HTTP 请求完成：{method} {url}"),
                    output: output_snippet(&output.stdout, &output.stderr),
                    attempt_count,
                });
            }
            Err(error) => {
                last_error = Some(error);
                if attempt < retry_count {
                    thread::sleep(retry_interval);
                }
            }
        }
    }
    Err(action_failure(
        last_error.unwrap_or_else(|| "HTTP 请求失败".to_string()),
        retry_count.saturating_add(1) as u32,
    ))
}

fn action_failure(message: impl Into<String>, attempt_count: u32) -> ActionRunFailure {
    ActionRunFailure {
        message: message.into(),
        attempt_count,
    }
}

fn http_method(action: &ActionConfig) -> String {
    action
        .parameters
        .get("method")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "GET".to_string())
}

fn http_request_command(
    action: &ActionConfig,
    variables: &BTreeMap<String, String>,
    method: &str,
    url: &str,
) -> Result<Command, String> {
    let mut command = Command::new("/usr/bin/curl");
    command.args(["-sS", "--fail", "-X", method]);
    if let Some(headers) = action.parameters.get("headers") {
        if !headers.trim().is_empty() {
            let items = serde_json::from_str::<BTreeMap<String, String>>(headers)
                .map_err(|error| format!("HTTP Headers JSON 格式错误：{error}"))?;
            for (key, value) in items {
                let header = format!("{key}: {}", replace_variables(&value, variables));
                command.args(["-H", &header]);
            }
        }
    }
    if let Some(body) = action.parameters.get("body") {
        command.args(["--data", &replace_variables(body, variables)]);
    }
    configure_process(&mut command, action, variables)?;
    command.args(["--", url]);
    Ok(command)
}

fn run_python(
    action: &ActionConfig,
    variables: &BTreeMap<String, String>,
) -> Result<ActionOutcome, String> {
    let python = action
        .parameters
        .get("python_path")
        .map(String::as_str)
        .unwrap_or("/usr/bin/python3");
    run_process_script(python, &["-c"], "script", action, variables, "Python")
}

fn run_process_script(
    executable: &str,
    prefix_args: &[&str],
    parameter_key: &str,
    action: &ActionConfig,
    variables: &BTreeMap<String, String>,
    label: &str,
) -> Result<ActionOutcome, String> {
    let script = action
        .parameters
        .get(parameter_key)
        .ok_or_else(|| format!("未配置 {label} 内容"))?;
    let script = replace_variables(script, variables);
    let mut command = Command::new(executable);
    command.args(prefix_args);
    command.arg(script);
    configure_process(&mut command, action, variables)?;
    let output = run_command(command, label, action_timeout(action))?;
    Ok(ActionOutcome {
        message: format!("{label} 执行完成"),
        output: output_snippet(&output.stdout, &output.stderr),
    })
}

/// 把 stdout/stderr 整理成可展示、可入库的片段；两者都为空时返回 None。
fn output_snippet(stdout: &str, stderr: &str) -> Option<String> {
    let stdout = stdout.trim();
    let stderr = stderr.trim();
    let mut sections = Vec::new();
    if !stdout.is_empty() {
        sections.push(truncate_output_text(stdout));
    }
    if !stderr.is_empty() {
        sections.push(format!("[stderr]\n{}", truncate_output_text(stderr)));
    }
    (!sections.is_empty()).then(|| sections.join("\n\n"))
}

fn truncate_output_text(text: &str) -> String {
    if text.len() <= MAX_ACTION_OUTPUT_SNIPPET_BYTES {
        return text.to_string();
    }
    let mut end = MAX_ACTION_OUTPUT_SNIPPET_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…（输出已截断）", &text[..end])
}

fn configure_process(
    command: &mut Command,
    action: &ActionConfig,
    variables: &BTreeMap<String, String>,
) -> Result<(), String> {
    if let Some(working_directory) = action
        .parameters
        .get("working_directory")
        .or_else(|| action.parameters.get("workingDirectory"))
        .map(|value| replace_variables(value, variables))
        .filter(|value| !value.trim().is_empty())
    {
        let path = Path::new(&working_directory);
        if !path.is_dir() {
            return Err(format!("工作目录不存在或不是目录：{working_directory}"));
        }
        command.current_dir(path);
    }
    if let Some(env_json) = action
        .parameters
        .get("env_json")
        .or_else(|| action.parameters.get("envJson"))
        .filter(|value| !value.trim().is_empty())
    {
        let items = serde_json::from_str::<BTreeMap<String, String>>(env_json)
            .map_err(|error| format!("环境变量 JSON 格式错误：{error}"))?;
        for (key, value) in items {
            if key.trim().is_empty() {
                continue;
            }
            command.env(key, replace_variables(&value, variables));
        }
    }
    Ok(())
}

fn run_command(
    mut command: Command,
    label: &str,
    timeout: Duration,
) -> Result<CommandOutput, String> {
    cleanup_stale_action_output_files_once();
    let stdout_path = temp_output_path("stdout");
    let stderr_path = temp_output_path("stderr");
    let stdout_file =
        File::create(&stdout_path).map_err(|error| format!("{label} 创建输出文件失败：{error}"))?;
    let stderr_file = File::create(&stderr_path).map_err(|error| {
        let _ = fs::remove_file(&stdout_path);
        format!("{label} 创建错误输出文件失败：{error}")
    })?;

    configure_process_group(&mut command);
    let mut child = match command
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            cleanup_output_files(&stdout_path, &stderr_path);
            return Err(format!("{label} 启动失败：{}", error));
        }
    };

    let started_at = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started_at.elapsed() >= timeout => {
                terminate_child_tree(&mut child);
                let _ = child.wait();
                cleanup_output_files(&stdout_path, &stderr_path);
                return Err(format!("{label} 执行超时（{} 秒）", timeout.as_secs()));
            }
            Ok(None) if output_files_exceed_limit(&stdout_path, &stderr_path) => {
                terminate_child_tree(&mut child);
                let _ = child.wait();
                cleanup_output_files(&stdout_path, &stderr_path);
                return Err(format!(
                    "{label} 输出过大（超过 {} MB）",
                    MAX_ACTION_OUTPUT_FILE_BYTES / 1024 / 1024
                ));
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                terminate_child_tree(&mut child);
                let _ = child.wait();
                cleanup_output_files(&stdout_path, &stderr_path);
                return Err(format!("{label} 等待失败：{error}"));
            }
        }
    };

    if output_files_exceed_limit(&stdout_path, &stderr_path) {
        cleanup_output_files(&stdout_path, &stderr_path);
        return Err(format!(
            "{label} 输出过大（超过 {} MB）",
            MAX_ACTION_OUTPUT_FILE_BYTES / 1024 / 1024
        ));
    }

    let stdout = read_and_remove_output(&stdout_path);
    let stderr = read_and_remove_output(&stderr_path);
    if status.success() {
        Ok(CommandOutput { stdout, stderr })
    } else {
        Err(format!(
            "{label} 退出码 {:?}：{}{}",
            status.code(),
            stdout.trim(),
            stderr.trim()
        ))
    }
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    // SAFETY: pre_exec runs in the child just before exec; setsid only touches process session state.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

fn terminate_child_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        terminate_process_group(child);
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
}

#[cfg(unix)]
fn terminate_process_group(child: &mut Child) {
    let process_group_id = child.id() as libc::pid_t;
    signal_process_group(process_group_id, libc::SIGTERM);
    let started_at = Instant::now();
    while started_at.elapsed() < Duration::from_millis(500) {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    signal_process_group(process_group_id, libc::SIGKILL);
    let _ = child.kill();
}

#[cfg(unix)]
fn signal_process_group(process_group_id: libc::pid_t, signal: libc::c_int) {
    if process_group_id <= 0 {
        return;
    }
    // SAFETY: kill is called with a positive process group id negated per POSIX.
    unsafe {
        libc::kill(-process_group_id, signal);
    }
}

fn temp_output_path(kind: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "{ACTION_OUTPUT_FILE_PREFIX}{kind}-{}{ACTION_OUTPUT_FILE_SUFFIX}",
        Uuid::new_v4()
    ))
}

fn read_and_remove_output(path: &Path) -> String {
    let output = read_limited_output(path).unwrap_or_default();
    let _ = fs::remove_file(path);
    output
}

fn read_limited_output(path: &Path) -> Result<String, std::io::Error> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::with_capacity(MAX_ACTION_OUTPUT_LOG_BYTES.min(8 * 1024));
    file.by_ref()
        .take((MAX_ACTION_OUTPUT_LOG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    let truncated = bytes.len() > MAX_ACTION_OUTPUT_LOG_BYTES;
    if truncated {
        bytes.truncate(MAX_ACTION_OUTPUT_LOG_BYTES);
    }
    let mut output = String::from_utf8_lossy(&bytes).to_string();
    if truncated {
        output.push_str("\n...输出过长，已截断");
    }
    Ok(output)
}

fn cleanup_output_files(stdout_path: &Path, stderr_path: &Path) {
    let _ = fs::remove_file(stdout_path);
    let _ = fs::remove_file(stderr_path);
}

fn cleanup_stale_action_output_files_once() {
    STALE_ACTION_OUTPUT_CLEANUP.get_or_init(|| {
        cleanup_stale_action_output_files_in_dir(&std::env::temp_dir(), SystemTime::now());
    });
}

fn cleanup_stale_action_output_files_in_dir(dir: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if should_remove_action_output_file(&path, modified, now, STALE_ACTION_OUTPUT_AGE) {
            let _ = fs::remove_file(path);
        }
    }
}

fn should_remove_action_output_file(
    path: &Path,
    modified: SystemTime,
    now: SystemTime,
    max_age: Duration,
) -> bool {
    is_action_output_file(path) && now.duration_since(modified).unwrap_or_default() > max_age
}

fn is_action_output_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|item| item.to_str())
        .map(|name| {
            name.starts_with(ACTION_OUTPUT_FILE_PREFIX) && name.ends_with(ACTION_OUTPUT_FILE_SUFFIX)
        })
        .unwrap_or(false)
}

fn output_files_exceed_limit(stdout_path: &Path, stderr_path: &Path) -> bool {
    output_file_size(stdout_path).saturating_add(output_file_size(stderr_path))
        > MAX_ACTION_OUTPUT_FILE_BYTES
}

fn output_file_size(path: &Path) -> u64 {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

fn action_timeout(action: &ActionConfig) -> Duration {
    let seconds = action_u64_parameter(action, &["timeout_seconds", "timeout"])
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_ACTION_TIMEOUT_SECONDS)
        .min(MAX_ACTION_TIMEOUT_SECONDS);
    Duration::from_secs(seconds)
}

fn action_u64_parameter(action: &ActionConfig, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| action.parameters.get(*key))
        .and_then(|value| value.trim().parse::<u64>().ok())
}

fn applescript_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action_with_timeout(value: &str) -> ActionConfig {
        let mut parameters = BTreeMap::new();
        parameters.insert("timeout_seconds".to_string(), value.to_string());
        ActionConfig {
            action_type: "run_shell".to_string(),
            parameters,
        }
    }

    #[test]
    fn action_timeout_uses_bounds() {
        assert_eq!(action_timeout(&action_with_timeout("2")).as_secs(), 2);
        assert_eq!(
            action_timeout(&action_with_timeout("999")).as_secs(),
            MAX_ACTION_TIMEOUT_SECONDS
        );
        assert_eq!(
            action_timeout(&action_with_timeout("0")).as_secs(),
            DEFAULT_ACTION_TIMEOUT_SECONDS
        );
        assert_eq!(
            action_timeout(&action_with_timeout("bad")).as_secs(),
            DEFAULT_ACTION_TIMEOUT_SECONDS
        );
    }

    #[test]
    fn shell_executable_defaults_to_bash_and_allows_zsh() {
        let mut parameters = BTreeMap::new();
        let mut action = ActionConfig {
            action_type: "run_shell".to_string(),
            parameters: parameters.clone(),
        };

        assert_eq!(shell_executable(&action).unwrap(), "/bin/bash");

        parameters.insert("shell".to_string(), "zsh".to_string());
        action.parameters = parameters;
        assert_eq!(shell_executable(&action).unwrap(), "/bin/zsh");
    }

    #[test]
    fn shell_executable_rejects_unknown_shells() {
        let mut parameters = BTreeMap::new();
        parameters.insert("shell".to_string(), "fish".to_string());
        let action = ActionConfig {
            action_type: "run_shell".to_string(),
            parameters,
        };

        assert!(shell_executable(&action).is_err());
    }

    #[test]
    fn shell_args_support_login_and_interactive_modes() {
        let mut parameters = BTreeMap::new();
        let mut action = ActionConfig {
            action_type: "run_shell".to_string(),
            parameters: parameters.clone(),
        };
        assert_eq!(shell_args(&action).unwrap(), vec!["-c"]);

        parameters.insert("shell_mode".to_string(), "interactive".to_string());
        action.parameters = parameters.clone();
        assert_eq!(shell_args(&action).unwrap(), vec!["-i", "-c"]);

        parameters.insert("shell_mode".to_string(), "login_interactive".to_string());
        action.parameters = parameters;
        assert_eq!(shell_args(&action).unwrap(), vec!["-l", "-i", "-c"]);
    }

    #[test]
    fn successful_script_execution_captures_stdout_and_stderr() {
        let action = ActionConfig {
            action_type: "run_shell".to_string(),
            parameters: BTreeMap::from([(
                "script".to_string(),
                "echo hello-stdout; echo hello-stderr >&2".to_string(),
            )]),
        };
        let executions = run_actions_detailed(&[action], &BTreeMap::new());

        assert_eq!(executions.len(), 1);
        assert!(executions[0].success);
        let output = executions[0].output.as_deref().expect("output captured");
        assert!(output.contains("hello-stdout"));
        assert!(output.contains("[stderr]"));
        assert!(output.contains("hello-stderr"));
    }

    #[test]
    fn output_snippet_skips_empty_and_truncates_large_text() {
        assert_eq!(output_snippet("", "  \n"), None);
        assert_eq!(output_snippet("ok", "").as_deref(), Some("ok"));

        let large = "x".repeat(MAX_ACTION_OUTPUT_SNIPPET_BYTES + 100);
        let snippet = output_snippet(&large, "").expect("snippet");
        assert!(snippet.contains("输出已截断"));
        assert!(snippet.len() < large.len());
    }

    #[test]
    fn dry_run_actions_resolves_variables_without_executing() {
        let marker = temp_output_path("dry-run-marker");
        let _ = fs::remove_file(&marker);
        let action = ActionConfig {
            action_type: "run_shell".to_string(),
            parameters: BTreeMap::from([
                (
                    "script".to_string(),
                    format!("echo {{{{title}}}} > {}", marker.to_string_lossy()),
                ),
                ("empty".to_string(), "  ".to_string()),
            ]),
        };
        let variables = BTreeMap::from([("title".to_string(), "构建完成".to_string())]);

        let previews = dry_run_actions(&[action], &variables);

        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].action_type, "run_shell");
        let script = previews[0]
            .parameters
            .iter()
            .find(|item| item.name == "script")
            .expect("script param");
        assert!(script.value.contains("构建完成"));
        assert!(
            !previews[0].parameters.iter().any(|item| item.name == "empty"),
            "blank parameters should be hidden from preview"
        );
        assert!(!marker.exists(), "dry run must not execute the script");
    }

    #[test]
    fn http_request_command_fails_on_http_error_status() {
        let action = ActionConfig {
            action_type: "http_request".to_string(),
            parameters: BTreeMap::from([
                (
                    "headers".to_string(),
                    r#"{"Accept":"application/json"}"#.to_string(),
                ),
                ("body".to_string(), r#"{"ok":true}"#.to_string()),
            ]),
        };
        let variables = BTreeMap::new();
        let command = http_request_command(&action, &variables, "GET", "https://example.com")
            .expect("command should be built");
        let args = command
            .get_args()
            .map(|item| item.to_string_lossy().to_string())
            .collect::<Vec<_>>();

        assert!(args.contains(&"--fail".to_string()));
        let url_index = args
            .iter()
            .position(|item| item == "https://example.com")
            .expect("url should be present");
        let separator_index = args
            .iter()
            .position(|item| item == "--")
            .expect("option separator should be present");
        assert_eq!(
            separator_index + 1,
            url_index,
            "the URL must be after -- to avoid curl option injection"
        );
        assert!(
            args.iter().position(|item| item == "-H").unwrap() < separator_index,
            "headers must stay before -- so curl still treats them as options"
        );
        assert!(
            args.iter().position(|item| item == "--data").unwrap() < separator_index,
            "body data must stay before -- so curl still treats it as an option"
        );
    }

    #[test]
    fn http_request_command_rejects_invalid_headers_json() {
        let mut parameters = BTreeMap::new();
        parameters.insert("headers".to_string(), "{bad".to_string());
        let action = ActionConfig {
            action_type: "http_request".to_string(),
            parameters,
        };
        let variables = BTreeMap::new();

        let error = http_request_command(&action, &variables, "GET", "https://example.com")
            .expect_err("invalid headers json should fail");

        assert!(error.contains("Headers JSON"));
    }

    #[test]
    fn open_url_rejects_option_like_urls() {
        let action = ActionConfig {
            action_type: "open_url".to_string(),
            parameters: BTreeMap::from([("url".to_string(), "-a Calculator".to_string())]),
        };
        let variables = BTreeMap::new();

        let error = open_url(&action, &variables).expect_err("option-like URL should fail");

        assert!(error.contains("URL 不能以 - 开头"));
    }

    #[test]
    fn http_request_rejects_option_like_urls() {
        let action = ActionConfig {
            action_type: "http_request".to_string(),
            parameters: BTreeMap::from([("url".to_string(), "-K config".to_string())]),
        };
        let variables = BTreeMap::new();

        let error = http_request(&action, &variables).expect_err("option-like URL should fail");

        assert!(error.message.contains("HTTP URL 不能以 - 开头"));
    }

    #[test]
    fn http_method_defaults_blank_values_to_get() {
        let mut action = ActionConfig {
            action_type: "http_request".to_string(),
            parameters: BTreeMap::new(),
        };
        assert_eq!(http_method(&action), "GET");

        action
            .parameters
            .insert("method".to_string(), "  ".to_string());
        assert_eq!(http_method(&action), "GET");

        action
            .parameters
            .insert("method".to_string(), " post ".to_string());
        assert_eq!(http_method(&action), "post");
    }

    #[test]
    fn run_command_times_out_long_running_process() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 2"]);
        let started_at = Instant::now();
        let result = run_command(command, "测试命令", Duration::from_secs(1));
        assert!(result.unwrap_err().contains("执行超时"));
        assert!(started_at.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn run_command_timeout_terminates_descendant_processes() {
        let marker_path = temp_output_path("descendant-marker");
        let _ = fs::remove_file(&marker_path);
        let script = format!(
            "(sleep 1; printf leaked > {}) & sleep 30",
            shell_quote(&marker_path.to_string_lossy())
        );
        let mut command = Command::new("/bin/sh");
        command.args(["-c", &script]);

        let error = run_command(command, "子进程命令", Duration::from_millis(100))
            .expect_err("command should time out");

        assert!(error.contains("执行超时"));
        thread::sleep(Duration::from_millis(1300));
        assert!(
            !marker_path.exists(),
            "descendant process should be terminated before writing marker"
        );
        let _ = fs::remove_file(marker_path);
    }

    #[test]
    fn run_command_handles_large_output_without_blocking() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "yes noticeflow | head -n 200000"]);
        run_command(command, "大量输出命令", Duration::from_secs(5))
            .expect("command should finish");
    }

    #[test]
    fn run_command_rejects_excessive_output_files() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "yes noticeflow | head -c 6000000"]);
        let error = run_command(command, "超大输出命令", Duration::from_secs(5))
            .expect_err("command should be rejected");

        assert!(error.contains("输出过大"));
    }

    #[test]
    fn run_command_truncates_large_failure_output() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "yes noticeflow-error | head -c 200000 >&2; exit 7"]);
        let error = run_command(command, "失败输出命令", Duration::from_secs(5))
            .expect_err("command should fail");

        assert!(error.contains("退出码 Some(7)"));
        assert!(error.contains("输出过长，已截断"));
        assert!(error.len() < MAX_ACTION_OUTPUT_LOG_BYTES + 512);
    }

    #[test]
    fn recognizes_noticeflow_action_output_files() {
        assert!(is_action_output_file(Path::new(
            "/tmp/noticeflow-action-stdout-abc.log"
        )));
        assert!(is_action_output_file(Path::new(
            "/tmp/noticeflow-action-stderr-abc.log"
        )));
        assert!(!is_action_output_file(Path::new(
            "/tmp/noticeflow-action-stdout-abc.txt"
        )));
        assert!(!is_action_output_file(Path::new(
            "/tmp/other-action-stdout-abc.log"
        )));
    }

    #[test]
    fn stale_output_cleanup_only_targets_old_noticeflow_logs() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(60 * 60 * 48);
        let old = now - STALE_ACTION_OUTPUT_AGE - Duration::from_secs(1);
        let fresh = now - Duration::from_secs(60);
        let action_log = Path::new("/tmp/noticeflow-action-stdout-abc.log");
        let unrelated_log = Path::new("/tmp/other-action-stdout-abc.log");

        assert!(should_remove_action_output_file(
            action_log,
            old,
            now,
            STALE_ACTION_OUTPUT_AGE
        ));
        assert!(!should_remove_action_output_file(
            action_log,
            fresh,
            now,
            STALE_ACTION_OUTPUT_AGE
        ));
        assert!(!should_remove_action_output_file(
            unrelated_log,
            old,
            now,
            STALE_ACTION_OUTPUT_AGE
        ));
    }

    fn shell_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

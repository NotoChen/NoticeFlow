use crate::notification_db::NotificationRecord;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

const URL_VALUES_STORAGE_KEY: &str = "__noticeflow_url_values";
const MAX_CUSTOM_REGEX_CACHE_ENTRIES: usize = 256;
static CUSTOM_REGEX_CACHE: OnceLock<Mutex<RegexCache>> = OnceLock::new();
static ESCAPED_SPECIAL_DOLLAR_REGEX: OnceLock<Regex> = OnceLock::new();
static URL_REGEX: OnceLock<Regex> = OnceLock::new();
static DYNAMIC_URL_REGEX: OnceLock<Regex> = OnceLock::new();
static JSON_VARIABLE_REGEX: OnceLock<Regex> = OnceLock::new();
static SHELL_VARIABLE_REGEX: OnceLock<Regex> = OnceLock::new();
static URLS_JOIN_REGEX: OnceLock<Regex> = OnceLock::new();

#[derive(Default)]
struct RegexCache {
    items: HashMap<String, Option<Regex>>,
    order: VecDeque<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableExtractionRule {
    pub name: String,
    pub source: VariableExtractionSource,
    pub method: VariableExtractionMethod,
    pub pattern: Option<String>,
    pub end_pattern: Option<String>,
    pub group_index: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableExtractionSource {
    Title,
    Subtitle,
    Body,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableExtractionMethod {
    Regex,
    Between,
}

pub fn extract_variables(
    notification: &NotificationRecord,
    custom_rules: &[VariableExtractionRule],
) -> BTreeMap<String, String> {
    let mut variables = BTreeMap::new();
    variables.insert("app_id".to_string(), notification.app_identifier.clone());
    variables.insert("app_name".to_string(), notification.app_name.clone());
    variables.insert("title".to_string(), notification.title.clone());
    variables.insert("subtitle".to_string(), notification.subtitle.clone());
    variables.insert("body".to_string(), notification.body.clone());
    variables.insert(
        "timestamp".to_string(),
        notification
            .delivered_at
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
    );

    let urls = extract_urls(notification);
    let urls_json = serde_json::to_string(&urls).unwrap_or_default();
    variables.insert("url".to_string(), urls.first().cloned().unwrap_or_default());
    variables.insert("url_count".to_string(), urls.len().to_string());
    variables.insert("urls".to_string(), urls.join(" "));
    variables.insert("urls_count".to_string(), urls.len().to_string());
    variables.insert("urls_json".to_string(), urls_json.clone());
    variables.insert(URL_VALUES_STORAGE_KEY.to_string(), urls_json);

    for rule in custom_rules {
        let name = rule.name.trim();
        if !is_valid_variable_name(name) {
            continue;
        }
        if let Some(value) = extract_custom_value(notification, rule) {
            variables.insert(name.to_string(), value);
        }
    }

    variables
}

pub fn display_variable_names(variables: &BTreeMap<String, String>) -> Vec<String> {
    let mut names = vec![
        "app_id".to_string(),
        "app_name".to_string(),
        "title".to_string(),
        "subtitle".to_string(),
        "body".to_string(),
        "url".to_string(),
        "url_count".to_string(),
        "urls".to_string(),
        "urls_count".to_string(),
        "urls_json".to_string(),
        "timestamp".to_string(),
    ];

    for name in variables.keys() {
        if is_displayable_variable_name(name) && !names.contains(name) {
            names.push(name.clone());
        }
    }

    names
}

pub fn replace_variables(template: &str, variables: &BTreeMap<String, String>) -> String {
    let nonce = Uuid::new_v4().to_string();
    let (mut result, mut protected_replacements) =
        protect_escaped_special_dollar_expressions(template, &nonce);
    result = replace_urls_join(&result, variables, &mut protected_replacements, &nonce);
    result = replace_dynamic_urls(&result, variables, &mut protected_replacements, &nonce);
    result = replace_json_variables(&result, variables, &mut protected_replacements, &nonce);
    result = replace_shell_variables(&result, variables, &mut protected_replacements, &nonce);
    let mut keys = variables.keys().collect::<Vec<_>>();
    keys.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
    for key in &keys {
        if key.starts_with("__") {
            continue;
        }
        let plain_token = protected_token(&nonce, "escaped_plain", protected_replacements.len());
        result = result.replace(&format!("$${key}"), &plain_token);
        protected_replacements.push((plain_token, format!("${key}")));

        let braced_token = protected_token(&nonce, "escaped_braced", protected_replacements.len());
        result = result.replace(&format!("$${{{key}}}"), &braced_token);
        protected_replacements.push((braced_token, format!("${{{key}}}")));
    }
    for key in keys {
        if key.starts_with("__") {
            continue;
        }
        let value = variables.get(key).cloned().unwrap_or_default();
        let token = protect_replacement(&mut protected_replacements, &nonce, "value", value);
        result = result.replace(&format!("{{{{{key}}}}}"), &token);
        result = result.replace(&format!("${{{key}}}"), &token);
        result = result.replace(&format!("${key}"), &token);
    }
    for (token, value) in protected_replacements {
        result = result.replace(&token, &value);
    }
    result
}

fn protect_replacement(
    protected_replacements: &mut Vec<(String, String)>,
    nonce: &str,
    kind: &str,
    value: String,
) -> String {
    let token = protected_token(nonce, kind, protected_replacements.len());
    protected_replacements.push((token.clone(), value));
    token
}

fn protected_token(nonce: &str, kind: &str, index: usize) -> String {
    format!("\u{1f}noticeflow_{nonce}_{kind}_{index}\u{1f}")
}

fn protect_escaped_special_dollar_expressions(
    template: &str,
    nonce: &str,
) -> (String, Vec<(String, String)>) {
    let regex = ESCAPED_SPECIAL_DOLLAR_REGEX.get_or_init(|| {
        Regex::new(r#"\$\$\{[^}]+\}|\$\$url_\d+\b"#)
            .expect("built-in escaped special dollar regex should compile")
    });
    let mut escaped = Vec::new();
    let result = regex
        .replace_all(template, |captures: &regex::Captures| {
            let matched = captures
                .get(0)
                .map(|item| item.as_str())
                .unwrap_or_default();
            let token = protected_token(nonce, "escaped_special", escaped.len());
            escaped.push((token.clone(), matched.replacen("$$", "$", 1)));
            token
        })
        .to_string();
    (result, escaped)
}

fn replace_json_variables(
    template: &str,
    variables: &BTreeMap<String, String>,
    protected_replacements: &mut Vec<(String, String)>,
    nonce: &str,
) -> String {
    let regex = JSON_VARIABLE_REGEX.get_or_init(|| {
        Regex::new(r#"(\{\{json:([A-Za-z_][A-Za-z0-9_]*)\}\}|\$\{json:([A-Za-z_][A-Za-z0-9_]*)\})"#)
            .expect("built-in JSON variable regex should compile")
    });
    regex
        .replace_all(template, |captures: &regex::Captures| {
            let name = captures
                .get(2)
                .or_else(|| captures.get(3))
                .map(|item| item.as_str())
                .unwrap_or_default();
            let value = variables.get(name).map(String::as_str).unwrap_or_default();
            protect_replacement(
                protected_replacements,
                nonce,
                "json",
                serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string()),
            )
        })
        .to_string()
}

fn replace_shell_variables(
    template: &str,
    variables: &BTreeMap<String, String>,
    protected_replacements: &mut Vec<(String, String)>,
    nonce: &str,
) -> String {
    let regex = SHELL_VARIABLE_REGEX.get_or_init(|| {
        Regex::new(
            r#"(\{\{shell:([A-Za-z_][A-Za-z0-9_]*)\}\}|\$\{shell:([A-Za-z_][A-Za-z0-9_]*)\})"#,
        )
        .expect("built-in shell variable regex should compile")
    });
    regex
        .replace_all(template, |captures: &regex::Captures| {
            let name = captures
                .get(2)
                .or_else(|| captures.get(3))
                .map(|item| item.as_str())
                .unwrap_or_default();
            protect_replacement(
                protected_replacements,
                nonce,
                "shell",
                shell_quote(variables.get(name).map(String::as_str).unwrap_or_default()),
            )
        })
        .to_string()
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn is_valid_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn extract_custom_value(
    notification: &NotificationRecord,
    rule: &VariableExtractionRule,
) -> Option<String> {
    match rule.method {
        VariableExtractionMethod::Regex => extract_regex_value(notification, rule),
        VariableExtractionMethod::Between => extract_between_value(notification, rule),
    }
}

fn extract_regex_value(
    notification: &NotificationRecord,
    rule: &VariableExtractionRule,
) -> Option<String> {
    let pattern = rule.pattern.as_ref()?.trim();
    if pattern.is_empty() {
        return None;
    }
    let regex = cached_custom_regex(pattern)?;
    let text = source_text(notification, &rule.source);
    let captures = regex.captures(text)?;
    let requested_group = rule.group_index.unwrap_or(1);
    let group_index = if requested_group < captures.len() {
        requested_group
    } else {
        0
    };
    captures
        .get(group_index)
        .map(|item| item.as_str().trim().to_string())
        .filter(|item| !item.is_empty())
}

fn extract_between_value(
    notification: &NotificationRecord,
    rule: &VariableExtractionRule,
) -> Option<String> {
    let text = source_text(notification, &rule.source);
    if text.is_empty() {
        return None;
    }

    let start_marker = rule.pattern.as_deref().unwrap_or_default();
    let end_marker = rule.end_pattern.as_deref().unwrap_or_default();
    let start = if start_marker.is_empty() {
        0
    } else {
        text.find(start_marker)? + start_marker.len()
    };
    let end = if end_marker.is_empty() {
        text.len()
    } else {
        text[start..].find(end_marker).map(|index| start + index)?
    };

    if start > end {
        return None;
    }

    Some(text[start..end].trim().to_string()).filter(|item| !item.is_empty())
}

fn source_text<'a>(
    notification: &'a NotificationRecord,
    source: &VariableExtractionSource,
) -> &'a str {
    match source {
        VariableExtractionSource::Title => &notification.title,
        VariableExtractionSource::Subtitle => &notification.subtitle,
        VariableExtractionSource::Body => &notification.body,
    }
}

fn extract_urls(notification: &NotificationRecord) -> Vec<String> {
    let text = [
        &notification.title,
        &notification.subtitle,
        &notification.body,
    ]
    .into_iter()
    .filter(|item| !item.is_empty())
    .cloned()
    .collect::<Vec<_>>()
    .join("\n");

    let regex = URL_REGEX.get_or_init(|| {
        Regex::new(r#"https?://[^\s<>"'，。！？、；：）)】\]]+"#)
            .expect("built-in URL regex should compile")
    });

    let mut urls = Vec::new();
    for matched in regex.find_iter(&text) {
        let url = matched.as_str().trim_end_matches(['.', ',', ';', ':']);
        if !url.is_empty() && !urls.iter().any(|item| item == url) {
            urls.push(url.to_string());
        }
    }
    urls
}

fn is_displayable_variable_name(name: &str) -> bool {
    is_valid_variable_name(name) && !name.starts_with("__") && name != "url_n"
}

fn replace_dynamic_urls(
    template: &str,
    variables: &BTreeMap<String, String>,
    protected_replacements: &mut Vec<(String, String)>,
    nonce: &str,
) -> String {
    let regex = DYNAMIC_URL_REGEX.get_or_init(|| {
        Regex::new(r#"(\{\{url_(\d+)\}\}|\$\{url_(\d+)\}|\$url_(\d+)\b)"#)
            .expect("built-in dynamic URL regex should compile")
    });
    regex
        .replace_all(template, |captures: &regex::Captures| {
            let index = captures
                .get(2)
                .or_else(|| captures.get(3))
                .or_else(|| captures.get(4))
                .and_then(|item| item.as_str().parse::<usize>().ok())
                .unwrap_or(1);
            protect_replacement(
                protected_replacements,
                nonce,
                "dynamic_url",
                dynamic_url_value(index, variables),
            )
        })
        .to_string()
}

fn replace_urls_join(
    template: &str,
    variables: &BTreeMap<String, String>,
    protected_replacements: &mut Vec<(String, String)>,
    nonce: &str,
) -> String {
    let regex = URLS_JOIN_REGEX.get_or_init(|| {
        Regex::new(r#"(\{\{urls_join:([^}]*)\}\}|\$\{urls_join:([^}]*)\})"#)
            .expect("built-in URLs join regex should compile")
    });
    regex
        .replace_all(template, |captures: &regex::Captures| {
            let separator = captures
                .get(2)
                .or_else(|| captures.get(3))
                .map(|item| decode_join_separator(item.as_str()))
                .unwrap_or_default();
            protect_replacement(
                protected_replacements,
                nonce,
                "urls_join",
                url_values(variables).join(&separator),
            )
        })
        .to_string()
}

fn cached_custom_regex(pattern: &str) -> Option<Regex> {
    let cache = CUSTOM_REGEX_CACHE.get_or_init(|| Mutex::new(RegexCache::default()));
    if let Ok(mut cache) = cache.lock() {
        if let Some(cached) = cache.items.get(pattern).cloned() {
            cache.order.retain(|item| item != pattern);
            cache.order.push_back(pattern.to_string());
            return cached;
        }
    }

    let compiled = Regex::new(pattern).ok();
    if let Ok(mut cache) = cache.lock() {
        if cache.items.contains_key(pattern) {
            cache.order.retain(|item| item != pattern);
        }
        cache.order.push_back(pattern.to_string());
        cache.items.insert(pattern.to_string(), compiled.clone());
        while cache.items.len() > MAX_CUSTOM_REGEX_CACHE_ENTRIES {
            let Some(oldest_pattern) = cache.order.pop_front() else {
                break;
            };
            cache.items.remove(&oldest_pattern);
        }
    }
    compiled
}

fn dynamic_url_value(requested_index: usize, variables: &BTreeMap<String, String>) -> String {
    let urls = url_values(variables);
    if urls.is_empty() || requested_index == 0 || requested_index > urls.len() {
        return String::new();
    }
    urls[requested_index - 1].clone()
}

fn url_values(variables: &BTreeMap<String, String>) -> Vec<String> {
    variables
        .get(URL_VALUES_STORAGE_KEY)
        .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
        .unwrap_or_default()
}

fn decode_join_separator(value: &str) -> String {
    let mut result = String::new();
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            result.push(character);
            continue;
        }
        match chars.next() {
            Some('n') => result.push('\n'),
            Some('r') => result.push('\r'),
            Some('t') => result.push('\t'),
            Some('\\') => result.push('\\'),
            Some(other) => {
                result.push('\\');
                result.push(other);
            }
            None => result.push('\\'),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn record(title: &str, subtitle: &str, body: &str) -> NotificationRecord {
        NotificationRecord {
            id: 1,
            app_identifier: "com.example.App".to_string(),
            app_name: "Example".to_string(),
            delivered_at: Utc::now(),
            title: title.to_string(),
            subtitle: subtitle.to_string(),
            body: body.to_string(),
        }
    }

    #[test]
    fn extracts_deduplicated_dynamic_urls() {
        let variables = extract_variables(
            &record(
                "审批 https://example.com/a",
                "",
                "查看 https://example.com/a 或 https://example.com/b",
            ),
            &[],
        );

        assert_eq!(
            variables.get("url").map(String::as_str),
            Some("https://example.com/a")
        );
        assert_eq!(variables.get("url_count").map(String::as_str), Some("2"));
        assert_eq!(variables.get("urls_count").map(String::as_str), Some("2"));
        assert_eq!(
            variables.get("urls").map(String::as_str),
            Some("https://example.com/a https://example.com/b")
        );
        assert_eq!(
            variables.get("urls_json").map(String::as_str),
            Some(r#"["https://example.com/a","https://example.com/b"]"#)
        );
        assert_eq!(
            replace_variables("{{url}} {{url_1}} {{url_2}} {{url_3}}", &variables),
            "https://example.com/a https://example.com/a https://example.com/b "
        );
        assert_eq!(
            replace_variables(
                "{{urls}} | {{urls_join:,}} | {{urls_join:\\n}} | ${urls_join: | }",
                &variables
            ),
            "https://example.com/a https://example.com/b | https://example.com/a,https://example.com/b | https://example.com/a\nhttps://example.com/b | https://example.com/a | https://example.com/b"
        );
    }

    #[test]
    fn hides_internal_url_storage_from_display_names() {
        let variables = extract_variables(&record("", "", "https://example.com"), &[]);
        let names = display_variable_names(&variables);

        assert!(names.contains(&"url".to_string()));
        assert!(names.contains(&"url_count".to_string()));
        assert!(names.contains(&"urls".to_string()));
        assert!(names.contains(&"urls_count".to_string()));
        assert!(names.contains(&"urls_json".to_string()));
        assert!(!names.iter().any(|name| name.starts_with("__")));
    }

    #[test]
    fn decodes_url_join_separator_escapes() {
        assert_eq!(decode_join_separator(r"\n"), "\n");
        assert_eq!(decode_join_separator(r"\r"), "\r");
        assert_eq!(decode_join_separator(r"\t"), "\t");
        assert_eq!(decode_join_separator(r"\\"), "\\");
        assert_eq!(decode_join_separator(r"\x"), r"\x");
    }

    #[test]
    fn escaped_dollar_variables_are_left_for_shell_scripts() {
        let variables = extract_variables(&record("Title", "", "https://example.com"), &[]);

        assert_eq!(
            replace_variables(
                "echo $$url $${title} $$url_1 $${url_2} $${urls_join:,} {{url}}",
                &variables
            ),
            "echo $url ${title} $url_1 ${url_2} ${urls_join:,} https://example.com"
        );
    }

    #[test]
    fn modifier_variables_escape_json_and_shell_values() {
        let variables = extract_variables(&record("Bob's \"approval\"\nready", "", ""), &[]);

        assert_eq!(
            replace_variables(r#"{"title":{{json:title}}}"#, &variables),
            r#"{"title":"Bob's \"approval\"\nready"}"#
        );
        assert_eq!(
            replace_variables("open {{shell:title}}", &variables),
            "open 'Bob'\\''s \"approval\"\nready'"
        );
        assert_eq!(
            replace_variables("echo $${json:title} $${shell:title}", &variables),
            "echo ${json:title} ${shell:title}"
        );
    }

    #[test]
    fn variable_values_are_not_recursively_expanded() {
        let variables = extract_variables(&record("$body {{body}}", "", "payload"), &[]);

        assert_eq!(
            replace_variables("{{title}} ${title} $title", &variables),
            "$body {{body}} $body {{body}} $body {{body}}"
        );
    }

    #[test]
    fn escaped_modifier_values_are_not_recursively_expanded() {
        let variables =
            extract_variables(&record("$body {{body}}", "", "'; touch /tmp/nope; '"), &[]);

        assert_eq!(
            replace_variables(r#"{"title":{{json:title}}}"#, &variables),
            r#"{"title":"$body {{body}}"}"#
        );
        assert_eq!(
            replace_variables("echo {{shell:title}}", &variables),
            "echo '$body {{body}}'"
        );
    }

    #[test]
    fn extracts_custom_regex_variable() {
        let variables = extract_variables(
            &record("", "", "订单号: NF-20260619 状态: 已通过"),
            &[VariableExtractionRule {
                name: "order_id".to_string(),
                source: VariableExtractionSource::Body,
                method: VariableExtractionMethod::Regex,
                pattern: Some(r#"订单号:\s*([A-Z]+-\d+)"#.to_string()),
                end_pattern: None,
                group_index: Some(1),
            }],
        );

        assert_eq!(
            variables.get("order_id").map(String::as_str),
            Some("NF-20260619")
        );
    }

    #[test]
    fn ignores_invalid_custom_regex_without_blocking_other_variables() {
        let variables = extract_variables(
            &record("", "", "审批编号 ABC-123"),
            &[
                VariableExtractionRule {
                    name: "broken".to_string(),
                    source: VariableExtractionSource::Body,
                    method: VariableExtractionMethod::Regex,
                    pattern: Some("(".to_string()),
                    end_pattern: None,
                    group_index: Some(1),
                },
                VariableExtractionRule {
                    name: "approval_id".to_string(),
                    source: VariableExtractionSource::Body,
                    method: VariableExtractionMethod::Regex,
                    pattern: Some(r#"审批编号\s+([A-Z]+-\d+)"#.to_string()),
                    end_pattern: None,
                    group_index: Some(1),
                },
            ],
        );

        assert!(!variables.contains_key("broken"));
        assert_eq!(
            variables.get("approval_id").map(String::as_str),
            Some("ABC-123")
        );
    }
}

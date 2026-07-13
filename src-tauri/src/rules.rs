use chrono::Local;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::fs;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

const MAX_REGEX_CACHE_ENTRIES: usize = 256;
static REGEX_CACHE: OnceLock<Mutex<RegexCache>> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RuleFile {
    pub rules: Vec<AutomationRule>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRule {
    pub id: String,
    pub name: String,
    pub enabled: Option<bool>,
    pub trigger_time: Option<String>,
    pub cooldown_seconds: Option<u64>,
    pub hit_count: Option<u64>,
    pub app_identifiers: Option<Vec<String>>,
    pub match_conditions: Option<Vec<MatchCondition>>,
    pub variable_extractions: Option<Vec<crate::variables::VariableExtractionRule>>,
    pub actions: Option<Vec<ActionConfig>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchCondition {
    pub variable_name: String,
    #[serde(rename = "operatorType")]
    pub operator_type: String,
    pub expected_value: Option<String>,
    pub case_sensitive: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ActionConfig {
    #[serde(rename = "type")]
    pub action_type: String,
    pub parameters: std::collections::BTreeMap<String, String>,
}

#[derive(Default)]
struct RegexCache {
    items: HashMap<String, Option<Regex>>,
    order: VecDeque<String>,
}

impl AutomationRule {
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
}

impl MatchCondition {
    pub fn evaluate(&self, variables: &std::collections::BTreeMap<String, String>) -> bool {
        let value = variables
            .get(&self.variable_name)
            .cloned()
            .unwrap_or_default();
        let expected = self.expected_value.clone().unwrap_or_default();
        let left = comparable(&value, self.case_sensitive);
        let right = comparable(&expected, self.case_sensitive);

        match self.operator_type.as_str() {
            "equals" => left == right,
            "not_equals" => left != right,
            "contains" => left.contains(&right),
            "not_contains" => !left.contains(&right),
            "starts_with" => left.starts_with(&right),
            "ends_with" => left.ends_with(&right),
            "regex" => regex_matches(&value, &expected, self.case_sensitive).unwrap_or(false),
            "not_regex" => regex_matches(&value, &expected, self.case_sensitive)
                .map(|item| !item)
                .unwrap_or(false),
            "is_empty" => value.trim().is_empty(),
            "is_not_empty" => !value.trim().is_empty(),
            _ => false,
        }
    }
}

pub fn matching_rules(
    rules: &[AutomationRule],
    record: &crate::notification_db::NotificationRecord,
) -> Vec<(AutomationRule, std::collections::BTreeMap<String, String>)> {
    let current_minute = if rules.iter().any(|rule| {
        rule.trigger_time
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
    }) {
        Some(Local::now().format("%H:%M").to_string())
    } else {
        None
    };
    let mut base_variables: Option<std::collections::BTreeMap<String, String>> = None;

    rules
        .iter()
        .filter(|rule| rule.is_enabled())
        .filter(|rule| {
            trigger_time_matches(rule.trigger_time.as_deref(), current_minute.as_deref())
        })
        .filter_map(|rule| {
            if !matches_record_app(rule.app_identifiers.as_deref(), &record.app_identifier) {
                return None;
            }

            let custom_rules = rule.variable_extractions.as_deref().unwrap_or(&[]);
            let variables = if custom_rules.is_empty() {
                base_variables
                    .get_or_insert_with(|| crate::variables::extract_variables(record, &[]))
                    .clone()
            } else {
                crate::variables::extract_variables(record, custom_rules)
            };
            let conditions = rule.match_conditions.as_deref().unwrap_or(&[]);
            if conditions
                .iter()
                .all(|condition| condition.evaluate(&variables))
            {
                Some((rule.clone(), variables))
            } else {
                None
            }
        })
        .collect()
}

fn matches_record_app(app_identifiers: Option<&[String]>, record_app_identifier: &str) -> bool {
    let Some(app_identifiers) = app_identifiers else {
        return false;
    };
    let record_app_identifier = record_app_identifier.trim();
    !record_app_identifier.is_empty()
        && app_identifiers
            .iter()
            .map(|app_id| app_id.trim())
            .any(|app_id| !app_id.is_empty() && app_id.eq_ignore_ascii_case(record_app_identifier))
}

fn trigger_time_matches(trigger_time: Option<&str>, current_minute: Option<&str>) -> bool {
    let Some(trigger_time) = trigger_time
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return true;
    };
    current_minute.is_some_and(|current_minute| current_minute == trigger_time)
}

fn comparable(value: &str, case_sensitive: bool) -> String {
    if case_sensitive {
        value.to_string()
    } else {
        value.to_lowercase()
    }
}

fn regex_matches(value: &str, pattern: &str, case_sensitive: bool) -> Option<bool> {
    if pattern.trim().is_empty() {
        return None;
    }
    let pattern = regex_pattern_for_case_sensitivity(pattern, case_sensitive);
    cached_regex(&pattern).map(|regex| regex.is_match(value))
}

pub fn validate_regex_pattern(pattern: &str, case_sensitive: bool) -> Result<(), String> {
    if pattern.trim().is_empty() {
        return Err("正则表达式为空".to_string());
    }
    let pattern = regex_pattern_for_case_sensitivity(pattern, case_sensitive);
    Regex::new(&pattern)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn regex_pattern_for_case_sensitivity(pattern: &str, case_sensitive: bool) -> String {
    if case_sensitive {
        pattern.to_string()
    } else {
        format!("(?i){pattern}")
    }
}

fn cached_regex(pattern: &str) -> Option<Regex> {
    let cache = REGEX_CACHE.get_or_init(|| Mutex::new(RegexCache::default()));
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
        while cache.items.len() > MAX_REGEX_CACHE_ENTRIES {
            let Some(oldest_pattern) = cache.order.pop_front() else {
                break;
            };
            cache.items.remove(&oldest_pattern);
        }
    }
    compiled
}

pub fn load_rules_from_dir(data_directory: &Path) -> Result<Vec<AutomationRule>, Box<dyn Error>> {
    load_rule_file_at(&data_directory.join("rules.json"))
}

fn load_rule_file_at(path: &Path) -> Result<Vec<AutomationRule>, Box<dyn Error>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let data = fs::read_to_string(path)?;
    let mut file: RuleFile = serde_json::from_str(&data)?;
    for rule in &mut file.rules {
        if rule.id.trim().is_empty() {
            rule.id = Uuid::new_v4().to_string();
        }
    }
    Ok(file.rules)
}

pub fn save_rule_file_in_dir(
    data_directory: &Path,
    rules: &[AutomationRule],
) -> Result<(), Box<dyn Error>> {
    save_rule_file_at(&data_directory.join("rules.json"), rules)
}

fn save_rule_file_at(path: &Path, rules: &[AutomationRule]) -> Result<(), Box<dyn Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let file = RuleFile {
        rules: rules.to_vec(),
    };
    let data = serde_json::to_string_pretty(&file)?;
    write_file_atomically(path, data.as_bytes())?;
    Ok(())
}

fn write_file_atomically(path: &Path, data: &[u8]) -> Result<(), Box<dyn Error>> {
    let file_name = path
        .file_name()
        .and_then(|item| item.to_str())
        .unwrap_or("rules.json");
    let temp_path = path.with_file_name(format!(".{file_name}.{}.tmp", Uuid::new_v4()));
    fs::write(&temp_path, data)?;
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(Box::new(error));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::collections::BTreeMap;
    use uuid::Uuid;

    fn record(
        app_identifier: &str,
        title: &str,
        body: &str,
    ) -> crate::notification_db::NotificationRecord {
        crate::notification_db::NotificationRecord {
            id: 1,
            app_identifier: app_identifier.to_string(),
            app_name: app_identifier.to_string(),
            delivered_at: Utc::now(),
            title: title.to_string(),
            subtitle: String::new(),
            body: body.to_string(),
        }
    }

    fn rule(app_identifiers: Vec<&str>, conditions: Vec<MatchCondition>) -> AutomationRule {
        AutomationRule {
            id: "rule-1".to_string(),
            name: "Rule".to_string(),
            enabled: Some(true),
            trigger_time: None,
            cooldown_seconds: None,
            hit_count: None,
            app_identifiers: Some(
                app_identifiers
                    .into_iter()
                    .map(ToString::to_string)
                    .collect(),
            ),
            match_conditions: Some(conditions),
            variable_extractions: None,
            actions: None,
        }
    }

    #[test]
    fn refuses_rules_without_specific_app() {
        let rules = vec![rule(
            Vec::new(),
            vec![MatchCondition {
                variable_name: "title".to_string(),
                operator_type: "contains".to_string(),
                expected_value: Some("审批".to_string()),
                case_sensitive: false,
            }],
        )];

        assert!(matching_rules(&rules, &record("com.example.App", "审批", "")).is_empty());
    }

    #[test]
    fn matches_specific_app_and_condition() {
        let rules = vec![rule(
            vec!["com.example.App"],
            vec![MatchCondition {
                variable_name: "body".to_string(),
                operator_type: "regex".to_string(),
                expected_value: Some("https://example\\.com/\\w+".to_string()),
                case_sensitive: false,
            }],
        )];

        let matches = matching_rules(
            &rules,
            &record("com.example.App", "", "打开 https://example.com/a"),
        );
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn regex_matching_respects_case_sensitivity() {
        let mut variables = BTreeMap::new();
        variables.insert("title".to_string(), "Approval Ready".to_string());

        assert!(MatchCondition {
            variable_name: "title".to_string(),
            operator_type: "regex".to_string(),
            expected_value: Some("approval".to_string()),
            case_sensitive: false,
        }
        .evaluate(&variables));

        assert!(!MatchCondition {
            variable_name: "title".to_string(),
            operator_type: "regex".to_string(),
            expected_value: Some("approval".to_string()),
            case_sensitive: true,
        }
        .evaluate(&variables));
    }

    #[test]
    fn invalid_regex_does_not_match_or_negate() {
        let mut variables = BTreeMap::new();
        variables.insert("title".to_string(), "Approval Ready".to_string());

        assert!(!MatchCondition {
            variable_name: "title".to_string(),
            operator_type: "regex".to_string(),
            expected_value: Some("[".to_string()),
            case_sensitive: false,
        }
        .evaluate(&variables));

        assert!(!MatchCondition {
            variable_name: "title".to_string(),
            operator_type: "not_regex".to_string(),
            expected_value: Some("[".to_string()),
            case_sensitive: false,
        }
        .evaluate(&variables));
    }

    #[test]
    fn regex_validation_uses_rust_regex_syntax() {
        assert!(validate_regex_pattern("approval", false).is_ok());
        assert!(validate_regex_pattern("foo(?=bar)", true).is_err());
        assert!(validate_regex_pattern(r"(foo)\1", true).is_err());
    }

    #[test]
    fn matches_record_app_requires_specific_app_identifier() {
        assert!(!matches_record_app(None, "com.example.App"));
        assert!(!matches_record_app(Some(&[]), "com.example.App"));
        assert!(!matches_record_app(Some(&[" ".to_string()]), ""));
        assert!(matches_record_app(
            Some(&[" COM.EXAMPLE.APP ".to_string()]),
            " com.example.app "
        ));
    }

    #[test]
    fn trigger_time_matches_supplied_current_minute() {
        assert!(trigger_time_matches(None, None));
        assert!(trigger_time_matches(Some(""), None));
        assert!(trigger_time_matches(Some("09:30"), Some("09:30")));
        assert!(!trigger_time_matches(Some("09:30"), Some("09:31")));
        assert!(!trigger_time_matches(Some("09:30"), None));
    }

    #[test]
    fn evaluates_empty_and_missing_variables() {
        let variables = BTreeMap::new();
        assert!(MatchCondition {
            variable_name: "missing".to_string(),
            operator_type: "is_empty".to_string(),
            expected_value: None,
            case_sensitive: false,
        }
        .evaluate(&variables));
    }

    #[test]
    fn saves_rule_file_in_explicit_data_directory() {
        let directory = std::env::temp_dir().join(format!("noticeflow-rules-{}", Uuid::new_v4()));
        let rules = vec![rule(vec!["com.example.App"], Vec::new())];

        save_rule_file_in_dir(&directory, &rules).expect("rules should be saved");

        let saved = fs::read_to_string(directory.join("rules.json")).expect("rules file exists");
        assert!(saved.contains("com.example.App"));
        let leftover_temp_files = fs::read_dir(&directory)
            .expect("directory exists")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftover_temp_files, 0);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn loads_rule_file_from_explicit_data_directory() {
        let directory =
            std::env::temp_dir().join(format!("noticeflow-load-rules-{}", Uuid::new_v4()));
        fs::create_dir_all(&directory).expect("directory should be created");
        fs::write(
            directory.join("rules.json"),
            r#"{"rules":[{"id":"","name":"导入规则","enabled":true}]}"#,
        )
        .expect("rules file should be written");

        let rules = load_rules_from_dir(&directory).expect("rules should load");

        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name, "导入规则");
        assert!(!rules[0].id.is_empty());

        let _ = fs::remove_dir_all(directory);
    }
}

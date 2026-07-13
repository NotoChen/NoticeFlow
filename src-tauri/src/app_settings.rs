use plist::{Dictionary, Value};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

const LAUNCH_AGENT_LABEL: &str = "io.github.notochen.noticeflow";
const LEGACY_LAUNCH_AGENT_LABELS: &[&str] = &["io.github.zangbaiwsh.noticeflow"];
const SYSTEM_DATA_DIRECTORY_PREFIXES: &[&str] = &[
    "/Applications",
    "/bin",
    "/cores",
    "/dev",
    "/etc",
    "/Library",
    "/private",
    "/sbin",
    "/System",
    "/usr",
];
const SHARED_DATA_DIRECTORY_PREFIXES: &[&str] = &[
    "/tmp",
    "/private/tmp",
    "/private/var/tmp",
    "/Users/Shared",
    "/var/tmp",
];

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub launch_at_login: bool,
    pub data_directory: Option<String>,
    pub app_filter_mode: Option<String>,
    pub ignored_app_identifiers: Vec<String>,
}

pub fn default_data_dir() -> Result<PathBuf, Box<dyn Error>> {
    let base = dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join("Library/Application Support")))
        .ok_or("无法读取应用支持目录")?;
    Ok(base.join("NoticeFlow"))
}

pub fn app_data_dir() -> Result<PathBuf, Box<dyn Error>> {
    let settings = load_settings()?;
    data_dir_for_settings(&settings)
}

pub fn data_dir_for_settings(settings: &AppSettings) -> Result<PathBuf, Box<dyn Error>> {
    let mut settings = settings.clone();
    normalize_settings(&mut settings);
    validate_settings(&settings)?;
    if let Some(path) = settings
        .data_directory
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        return Ok(PathBuf::from(path));
    }
    default_data_dir()
}

pub fn settings_path() -> Result<PathBuf, Box<dyn Error>> {
    Ok(default_data_dir()?.join("settings.json"))
}

pub fn load_settings() -> Result<AppSettings, Box<dyn Error>> {
    let path = settings_path()?;
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let data = fs::read_to_string(path)?;
    let mut settings: AppSettings = serde_json::from_str(&data)?;
    normalize_settings(&mut settings);
    validate_settings(&settings)?;
    Ok(settings)
}

pub fn save_settings(settings: &AppSettings) -> Result<(), Box<dyn Error>> {
    let path = settings_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut settings = settings.clone();
    normalize_settings(&mut settings);
    validate_settings(&settings)?;
    let data = serde_json::to_string_pretty(&settings)?;
    write_file_atomically(&path, data.as_bytes())?;
    Ok(())
}

pub fn normalize_settings(settings: &mut AppSettings) {
    settings.data_directory = settings
        .data_directory
        .as_deref()
        .map(clean_data_directory)
        .filter(|path| !path.is_empty());
    settings.app_filter_mode = Some(match settings.app_filter_mode.as_deref().map(str::trim) {
        Some("include") => "include".to_string(),
        _ => "exclude".to_string(),
    });
    settings.ignored_app_identifiers =
        normalize_ignored_app_identifiers(std::mem::take(&mut settings.ignored_app_identifiers));
}

fn validate_settings(settings: &AppSettings) -> Result<(), Box<dyn Error>> {
    if let Some(data_directory) = settings.data_directory.as_deref() {
        validate_data_directory(Path::new(data_directory))
            .map_err(|error| format!("settings.json 中的数据目录不安全：{error}"))?;
    }
    Ok(())
}

pub fn validate_data_directory(path: &Path) -> Result<(), Box<dyn Error>> {
    if let Some(reason) = unsafe_data_directory_reason(path) {
        return Err(reason.into());
    }

    let existing_path = nearest_existing_path(path).ok_or("数据目录没有可读取的已存在上级目录")?;
    let metadata = fs::metadata(&existing_path)
        .map_err(|error| format!("数据目录或其上级目录不可读取：{error}"))?;
    if !metadata.is_dir() {
        if existing_path == path {
            return Err("数据目录必须是文件夹".into());
        }
        return Err("数据目录的已存在上级路径必须是文件夹".into());
    }

    let canonical = fs::canonicalize(&existing_path)
        .map_err(|error| format!("无法解析数据目录真实路径：{error}"))?;
    if let Some(reason) = unsafe_data_directory_anchor_reason(&canonical, existing_path == path) {
        return Err(reason.into());
    }

    Ok(())
}

fn clean_data_directory(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed == "/" {
        return trimmed.to_string();
    }
    trimmed.trim_end_matches('/').to_string()
}

fn normalize_ignored_app_identifiers(items: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::<String>::new();
    for item in items {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        if normalized
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(item))
        {
            continue;
        }
        normalized.push(item.to_string());
    }
    normalized.sort_by_key(|item| item.to_lowercase());
    normalized
}

fn unsafe_data_directory_reason(path: &Path) -> Option<&'static str> {
    unsafe_data_directory_reason_inner(path, true)
}

fn unsafe_data_directory_anchor_reason(
    path: &Path,
    is_target_directory: bool,
) -> Option<&'static str> {
    unsafe_data_directory_reason_inner(path, is_target_directory)
}

fn unsafe_data_directory_reason_inner(
    path: &Path,
    reject_broad_user_directory: bool,
) -> Option<&'static str> {
    if !path.is_absolute() {
        return Some("数据目录必须使用绝对路径");
    }
    if path == Path::new("/") {
        return Some("不能把系统根目录作为数据目录");
    }
    if path == Path::new("/Users") {
        return Some("不能把用户目录根路径作为数据目录");
    }
    if reject_broad_user_directory && is_broad_user_directory(path) {
        return Some("不能把用户主目录或常用目录本身作为数据目录，请选择专用子目录");
    }
    if contains_app_bundle_component(path) {
        return Some("不能把 .app 应用包内部作为数据目录");
    }
    if SYSTEM_DATA_DIRECTORY_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
    {
        return Some("不能把系统目录作为数据目录");
    }
    if SHARED_DATA_DIRECTORY_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
    {
        return Some("不能把临时目录或共享目录作为数据目录");
    }
    if path == Path::new("/Volumes") || is_volume_root(path) {
        return Some("不能把磁盘卷根目录作为数据目录");
    }
    None
}

fn is_broad_user_directory(path: &Path) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    [
        home.clone(),
        home.join("Applications"),
        home.join("Desktop"),
        home.join("Documents"),
        home.join("Downloads"),
        home.join("Library"),
        home.join("Library").join("Application Support"),
    ]
    .iter()
    .any(|directory| path == directory)
}

fn contains_app_bundle_component(path: &Path) -> bool {
    path.components().any(|component| {
        let Component::Normal(name) = component else {
            return false;
        };
        name.to_string_lossy().to_lowercase().ends_with(".app")
    })
}

fn is_volume_root(path: &Path) -> bool {
    let mut components = path.components();
    matches!(components.next(), Some(Component::RootDir))
        && matches!(
            components.next(),
            Some(Component::Normal(name)) if name.to_string_lossy() == "Volumes"
        )
        && matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
}

fn nearest_existing_path(path: &Path) -> Option<PathBuf> {
    let mut current = path;
    loop {
        if current.exists() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

fn write_file_atomically(path: &Path, data: &[u8]) -> Result<(), Box<dyn Error>> {
    let file_name = path
        .file_name()
        .and_then(|item| item.to_str())
        .unwrap_or("settings.json");
    let temp_path = path.with_file_name(format!(".{file_name}.{}.tmp", Uuid::new_v4()));
    fs::write(&temp_path, data)?;
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(Box::new(error));
    }
    Ok(())
}

pub fn launch_agent_path() -> Result<PathBuf, Box<dyn Error>> {
    launch_agent_path_for_label(LAUNCH_AGENT_LABEL)
}

fn launch_agent_path_for_label(label: &str) -> Result<PathBuf, Box<dyn Error>> {
    let home = dirs::home_dir().ok_or("无法读取用户主目录")?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{label}.plist")))
}

pub fn set_launch_at_login(enabled: bool) -> Result<(), Box<dyn Error>> {
    let path = launch_agent_path()?;
    remove_legacy_launch_agents(&path);
    if enabled {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let executable = std::env::current_exe()?;
        let mut root = Dictionary::new();
        root.insert("Label".into(), Value::String(LAUNCH_AGENT_LABEL.into()));
        root.insert(
            "ProgramArguments".into(),
            Value::Array(vec![Value::String(
                executable.to_string_lossy().to_string(),
            )]),
        );
        root.insert("RunAtLoad".into(), Value::Boolean(true));
        root.insert("KeepAlive".into(), Value::Boolean(false));
        Value::Dictionary(root).to_file_xml(path)?;
    } else if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn launch_at_login_enabled() -> bool {
    launch_agent_path()
        .map(|path| path.exists())
        .unwrap_or(false)
        || legacy_launch_agent_paths().iter().any(|path| path.exists())
}

fn legacy_launch_agent_paths() -> Vec<PathBuf> {
    LEGACY_LAUNCH_AGENT_LABELS
        .iter()
        .filter_map(|label| launch_agent_path_for_label(label).ok())
        .collect()
}

fn remove_legacy_launch_agents(current_path: &Path) {
    for legacy_path in legacy_launch_agent_paths() {
        if legacy_path == current_path {
            continue;
        }
        if legacy_path.exists() {
            let _ = fs::remove_file(legacy_path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn normalizes_settings_before_use() {
        let mut settings = AppSettings {
            launch_at_login: false,
            data_directory: Some("  /tmp/noticeflow-data/  ".to_string()),
            app_filter_mode: Some("bad".to_string()),
            ignored_app_identifiers: vec![
                " com.apple.mail ".to_string(),
                "COM.APPLE.MAIL".to_string(),
                "".to_string(),
                "com.example.App".to_string(),
            ],
        };

        normalize_settings(&mut settings);

        assert_eq!(
            settings.data_directory.as_deref(),
            Some("/tmp/noticeflow-data")
        );
        assert_eq!(settings.app_filter_mode.as_deref(), Some("exclude"));
        assert_eq!(
            settings.ignored_app_identifiers,
            vec!["com.apple.mail".to_string(), "com.example.App".to_string()]
        );
    }

    #[test]
    fn removes_blank_data_directory() {
        let mut settings = AppSettings {
            launch_at_login: false,
            data_directory: Some("  ".to_string()),
            app_filter_mode: None,
            ignored_app_identifiers: Vec::new(),
        };

        normalize_settings(&mut settings);

        assert_eq!(settings.data_directory, None);
    }

    #[test]
    fn keeps_root_data_directory() {
        let mut settings = AppSettings {
            launch_at_login: false,
            data_directory: Some(" / ".to_string()),
            app_filter_mode: Some("include".to_string()),
            ignored_app_identifiers: Vec::new(),
        };

        normalize_settings(&mut settings);

        assert_eq!(settings.data_directory.as_deref(), Some("/"));
    }

    #[test]
    fn rejects_unsafe_loaded_settings_data_directory() {
        let mut settings = AppSettings {
            launch_at_login: false,
            data_directory: Some(" / ".to_string()),
            app_filter_mode: Some("exclude".to_string()),
            ignored_app_identifiers: Vec::new(),
        };

        normalize_settings(&mut settings);
        let error = validate_settings(&settings)
            .expect_err("unsafe loaded data directory should be rejected")
            .to_string();

        assert!(error.contains("数据目录不安全"));
    }

    #[test]
    fn data_dir_for_settings_uses_loaded_safe_directory() {
        let directory = std::env::current_dir()
            .unwrap()
            .join(format!(".noticeflow-data-{}", Uuid::new_v4()));
        let settings = AppSettings {
            launch_at_login: false,
            data_directory: Some(directory.to_string_lossy().to_string()),
            app_filter_mode: Some("exclude".to_string()),
            ignored_app_identifiers: Vec::new(),
        };

        assert_eq!(
            data_dir_for_settings(&settings).ok().as_deref(),
            Some(directory.as_path())
        );
    }

    #[test]
    fn data_dir_for_settings_rejects_unsafe_directory() {
        let settings = AppSettings {
            launch_at_login: false,
            data_directory: Some("/".to_string()),
            app_filter_mode: Some("exclude".to_string()),
            ignored_app_identifiers: Vec::new(),
        };

        assert!(data_dir_for_settings(&settings).is_err());
    }

    #[test]
    fn rejects_unsafe_data_directory_locations() {
        for path in [
            "/",
            "/Users",
            "/System/Library",
            "/Library/NoticeFlow",
            "/Applications/NoticeFlow.app/Data",
            "/tmp/noticeflow",
            "/Users/Shared/NoticeFlow",
            "/Volumes",
            "/Volumes/ExternalDrive",
        ] {
            assert!(
                unsafe_data_directory_reason(Path::new(path)).is_some(),
                "{path} should be rejected"
            );
        }

        if let Some(home) = dirs::home_dir() {
            for path in [
                home.clone(),
                home.join("Desktop"),
                home.join("Documents"),
                home.join("Downloads"),
                home.join("Library"),
                home.join("Library").join("Application Support"),
            ] {
                assert!(
                    unsafe_data_directory_reason(&path).is_some(),
                    "{} should be rejected",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn allows_dedicated_user_or_volume_data_directories() {
        for path in [
            "/Users/example/Documents/NoticeFlow",
            "/Users/example/Library/Application Support/NoticeFlow",
            "/Volumes/ExternalDrive/NoticeFlow",
        ] {
            assert!(
                unsafe_data_directory_reason(Path::new(path)).is_none(),
                "{path} should be allowed"
            );
        }
    }

    #[test]
    fn validates_existing_safe_data_directory() {
        let directory = std::env::current_dir()
            .unwrap()
            .join(format!(".noticeflow-data-{}", Uuid::new_v4()));
        fs::create_dir(&directory).expect("test directory should be created");

        let result = validate_data_directory(&directory);

        let _ = fs::remove_dir(&directory);
        assert!(result.is_ok());
    }

    #[test]
    fn allows_new_data_directory_under_safe_parent() {
        let directory = std::env::current_dir()
            .unwrap()
            .join(format!(".noticeflow-data-{}", Uuid::new_v4()));

        let result = validate_data_directory(&directory);

        assert!(result.is_ok());
        assert!(!directory.exists());
    }

    #[test]
    fn allows_new_dedicated_directory_under_user_document_folder() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        let documents = home.join("Documents");
        if !documents.is_dir() {
            return;
        }
        let directory = documents.join(format!("NoticeFlow-{}", Uuid::new_v4()));

        let result = validate_data_directory(&directory);

        assert!(result.is_ok());
        assert!(!directory.exists());
    }
}

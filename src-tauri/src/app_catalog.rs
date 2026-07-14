use base64::Engine;
use plist::Value;
use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

static APPLICATION_CACHE: OnceLock<Mutex<Option<Vec<ApplicationInfo>>>> = OnceLock::new();
static ICON_CACHE_READY: OnceLock<()> = OnceLock::new();
const MAX_ICON_CACHE_FILES: usize = 300;
const MAX_ICON_CACHE_AGE: Duration = Duration::from_secs(60 * 60 * 24 * 45);

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationInfo {
    pub name: String,
    /// Finder 展示的本地化名称（如钉钉），仅在与 `name` 不同时填充。
    pub localized_name: Option<String>,
    pub bundle_id: String,
    pub path: String,
    pub icon_path: Option<String>,
    pub icon_cache_key: Option<String>,
    pub icon_data_url: Option<String>,
}

impl ApplicationInfo {
    pub fn display_name(&self) -> &str {
        self.localized_name.as_deref().unwrap_or(&self.name)
    }
}

pub fn scan_applications() -> Result<Vec<ApplicationInfo>, Box<dyn Error>> {
    scan_applications_cached(false)
}

pub fn rescan_applications() -> Result<Vec<ApplicationInfo>, Box<dyn Error>> {
    scan_applications_cached(true)
}

fn scan_applications_cached(force_refresh: bool) -> Result<Vec<ApplicationInfo>, Box<dyn Error>> {
    let cache = APPLICATION_CACHE.get_or_init(|| Mutex::new(None));
    let Ok(mut guard) = cache.lock() else {
        return scan_applications_uncached();
    };
    if !force_refresh {
        if let Some(apps) = guard.as_ref() {
            return Ok(apps.clone());
        }
    }

    let apps = scan_applications_uncached()?;
    *guard = Some(apps.clone());
    Ok(apps)
}

fn scan_applications_uncached() -> Result<Vec<ApplicationInfo>, Box<dyn Error>> {
    let mut roots = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
    ];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join("Applications"));
    }

    let mut apps: BTreeMap<String, ApplicationInfo> = BTreeMap::new();
    for root in roots {
        if !root.exists() {
            continue;
        }

        let mut walker = WalkDir::new(root).max_depth(3).into_iter();
        while let Some(entry) = walker.next() {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            if !entry.file_type().is_dir() || !is_app_bundle(path) {
                continue;
            }

            if let Some(app) = read_application(path) {
                insert_application(&mut apps, app);
            }
            walker.skip_current_dir();
        }
    }

    let mut result: Vec<ApplicationInfo> = apps.into_values().collect();
    sort_applications(&mut result);
    Ok(result)
}

fn insert_application(apps: &mut BTreeMap<String, ApplicationInfo>, app: ApplicationInfo) {
    apps.entry(application_key(&app.bundle_id)).or_insert(app);
}

fn application_key(bundle_id: &str) -> String {
    bundle_id.to_lowercase()
}

fn sort_applications(apps: &mut [ApplicationInfo]) {
    apps.sort_by(|left, right| {
        left.display_name()
            .to_lowercase()
            .cmp(&right.display_name().to_lowercase())
            .then_with(|| {
                left.bundle_id
                    .to_lowercase()
                    .cmp(&right.bundle_id.to_lowercase())
            })
            .then_with(|| left.path.cmp(&right.path))
    });
}

pub fn render_application_icon(icon_path: &str) -> Result<Option<String>, Box<dyn Error>> {
    if icon_path.trim().is_empty() {
        return Ok(None);
    }

    let source = PathBuf::from(icon_path);
    if !source.exists() {
        return Ok(None);
    }
    let source_metadata = source.metadata()?;
    let source_modified = source_metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);

    let noticeflow_cache_dir = dirs::cache_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join("Library/Caches")))
        .ok_or("无法读取缓存目录")?
        .join("NoticeFlow");
    ICON_CACHE_READY.get_or_init(|| {
        let _ = fs::remove_dir_all(noticeflow_cache_dir.join("app-icons"));
        let _ = prune_icon_cache(&noticeflow_cache_dir.join("app-icons-v2"));
    });
    let cache_dir = noticeflow_cache_dir.join("app-icons-v2");
    fs::create_dir_all(&cache_dir)?;

    let output = cache_dir.join(icon_cache_file_name(
        icon_path,
        source_metadata.len(),
        source_modified,
    ));
    if output.exists() {
        return png_data_url(&output).map(Some);
    }

    let status = Command::new("/usr/bin/sips")
        .args(["-Z", "64", "-s", "format", "png"])
        .arg(&source)
        .args(["--out"])
        .arg(&output)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;

    if status.success() && output.exists() {
        png_data_url(&output).map(Some)
    } else {
        Ok(None)
    }
}

pub fn render_application_icon_for_bundle(
    bundle_id: &str,
) -> Result<Option<String>, Box<dyn Error>> {
    let bundle_id = bundle_id.trim();
    if bundle_id.is_empty() {
        return Ok(None);
    }
    let apps = scan_applications()?;
    Ok(apps
        .into_iter()
        .find(|app| app.bundle_id.eq_ignore_ascii_case(bundle_id))
        .and_then(|app| {
            app.icon_data_url.or_else(|| {
                app.icon_path
                    .and_then(|path| render_application_icon(&path).ok().flatten())
            })
        }))
}

pub fn is_known_application_icon_path(icon_path: &str) -> Result<bool, Box<dyn Error>> {
    let icon_path = icon_path.trim();
    if icon_path.is_empty() {
        return Ok(false);
    }
    let requested = Path::new(icon_path);
    let Ok(requested) = fs::canonicalize(requested) else {
        return Ok(false);
    };
    let apps = scan_applications()?;
    Ok(apps.into_iter().any(|app| {
        app.icon_path
            .as_deref()
            .and_then(|path| fs::canonicalize(path).ok())
            .is_some_and(|known| known == requested)
    }))
}

fn png_data_url(path: &Path) -> Result<String, Box<dyn Error>> {
    let bytes = fs::read(path)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:image/png;base64,{encoded}"))
}

fn icon_cache_file_name(icon_path: &str, source_len: u64, source_modified: SystemTime) -> String {
    let modified = source_modified
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    icon_path.hash(&mut hasher);
    source_len.hash(&mut hasher);
    modified.as_secs().hash(&mut hasher);
    modified.subsec_nanos().hash(&mut hasher);
    "noticeflow-icon-64-v3".hash(&mut hasher);
    format!("{:x}.png", hasher.finish())
}

fn prune_icon_cache(cache_dir: &Path) -> Result<(), Box<dyn Error>> {
    if !cache_dir.exists() {
        return Ok(());
    }

    let now = SystemTime::now();
    let mut entries = fs::read_dir(cache_dir)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            Some((entry.path(), modified))
        })
        .collect::<Vec<_>>();

    for (path, modified) in &entries {
        if now.duration_since(*modified).unwrap_or_default() > MAX_ICON_CACHE_AGE {
            let _ = fs::remove_file(path);
        }
    }

    entries.retain(|(path, _)| path.exists());
    entries.sort_by_key(|(_, modified)| *modified);
    let overflow = entries.len().saturating_sub(MAX_ICON_CACHE_FILES);
    for (path, _) in entries.into_iter().take(overflow) {
        let _ = fs::remove_file(path);
    }

    Ok(())
}

fn read_application(path: &Path) -> Option<ApplicationInfo> {
    let info_path = path.join("Contents/Info.plist");
    let value = Value::from_file(info_path).ok()?;
    let dictionary = value.as_dictionary()?;
    let bundle_id = dictionary
        .get("CFBundleIdentifier")?
        .as_string()?
        .to_string();
    let name = dictionary
        .get("CFBundleDisplayName")
        .and_then(Value::as_string)
        .or_else(|| dictionary.get("CFBundleName").and_then(Value::as_string))
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|item| item.to_str())
                .unwrap_or(&bundle_id)
                .to_string()
        });
    let localized_name = finder_display_name(path).filter(|candidate| candidate != &name);
    let icon_path = dictionary
        .get("CFBundleIconFile")
        .and_then(Value::as_string)
        .and_then(|icon| resolve_icon_path(path, icon));
    let icon_cache_key = icon_path.as_deref().and_then(icon_source_cache_key);
    Some(ApplicationInfo {
        name,
        localized_name,
        bundle_id,
        path: path.to_string_lossy().to_string(),
        icon_path,
        icon_cache_key,
        icon_data_url: None,
    })
}

/// 读取 Finder 实际展示的本地化名称（走 NSFileManager，随系统语言返回
/// InfoPlist.strings 里的翻译，例如 DingTalk -> 钉钉）。
fn finder_display_name(path: &Path) -> Option<String> {
    use objc2_foundation::{NSFileManager, NSString};

    let path_str = path.to_str()?;
    let ns_path = NSString::from_str(path_str);
    let manager = NSFileManager::defaultManager();
    let display = manager.displayNameAtPath(&ns_path);
    let display = display.to_string();
    let display = display.strip_suffix(".app").unwrap_or(&display).trim();
    (!display.is_empty()).then(|| display.to_string())
}

fn is_app_bundle(path: &Path) -> bool {
    path.extension()
        .and_then(|item| item.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
}

fn resolve_icon_path(app_path: &Path, icon_name: &str) -> Option<String> {
    let resource_dir = app_path.join("Contents/Resources");
    let resource_anchor = fs::canonicalize(&resource_dir).ok()?;
    let candidates = if icon_name.ends_with(".icns") {
        vec![resource_dir.join(icon_name)]
    } else {
        vec![
            resource_dir.join(format!("{icon_name}.icns")),
            resource_dir.join(icon_name),
        ]
    };

    candidates
        .into_iter()
        .find_map(|path| safe_icon_resource_path(&resource_anchor, &path))
}

fn safe_icon_resource_path(resource_anchor: &Path, path: &Path) -> Option<String> {
    let canonical = fs::canonicalize(path).ok()?;
    if !canonical.starts_with(resource_anchor) || !canonical.is_file() {
        return None;
    }
    Some(canonical.to_string_lossy().to_string())
}

fn icon_source_cache_key(icon_path: &str) -> Option<String> {
    let source = Path::new(icon_path);
    let metadata = source.metadata().ok()?;
    let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    Some(icon_cache_file_name(icon_path, metadata.len(), modified))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn app(name: &str, bundle_id: &str, path: &str) -> ApplicationInfo {
        ApplicationInfo {
            name: name.to_string(),
            localized_name: None,
            bundle_id: bundle_id.to_string(),
            path: path.to_string(),
            icon_path: None,
            icon_cache_key: None,
            icon_data_url: None,
        }
    }

    #[test]
    fn display_name_prefers_localized_name() {
        let mut item = app("DingTalk", "com.alibaba.DingTalk", "/Applications/DingTalk.app");
        assert_eq!(item.display_name(), "DingTalk");
        item.localized_name = Some("钉钉".to_string());
        assert_eq!(item.display_name(), "钉钉");
    }

    #[test]
    fn applications_sort_by_localized_display_name() {
        let mut zulu = app("Zulu", "com.example.zulu", "/Applications/Zulu.app");
        zulu.localized_name = Some("Aardvark".to_string());
        let alpha = app("Beta", "com.example.beta", "/Applications/Beta.app");
        let mut apps = vec![alpha, zulu];

        sort_applications(&mut apps);

        assert_eq!(apps[0].bundle_id, "com.example.zulu");
    }

    #[test]
    fn cached_application_icon_is_returned_as_data_url() {
        let Some(icon_path) = resolve_icon_path(Path::new("/Applications/WeChat.app"), "AppIcon")
            .or_else(|| resolve_icon_path(Path::new("/Applications/DingTalk.app"), "AppIcon"))
        else {
            return;
        };

        let first = render_application_icon(&icon_path).expect("first render should not fail");
        let second = render_application_icon(&icon_path).expect("cached render should not fail");

        assert!(first
            .as_deref()
            .unwrap_or_default()
            .starts_with("data:image/png;base64,"));
        assert!(second
            .as_deref()
            .unwrap_or_default()
            .starts_with("data:image/png;base64,"));
    }

    #[test]
    fn detects_app_bundles_case_insensitively() {
        assert!(is_app_bundle(Path::new("/Applications/Foo.app")));
        assert!(is_app_bundle(Path::new("/Applications/Foo.APP")));
        assert!(!is_app_bundle(Path::new("/Applications/Foo")));
        assert!(!is_app_bundle(Path::new("/Applications/Foo.appex")));
    }

    #[test]
    fn resolve_icon_path_keeps_icons_inside_resources() {
        let base =
            std::env::temp_dir().join(format!("noticeflow-app-icon-{}", uuid::Uuid::new_v4()));
        let app_path = base.join("Foo.app");
        let resources = app_path.join("Contents/Resources");
        fs::create_dir_all(&resources).expect("resources should be created");
        let icon_path = resources.join("AppIcon.icns");
        fs::write(&icon_path, b"icon").expect("icon should be written");

        let resolved = resolve_icon_path(&app_path, "AppIcon").expect("icon should resolve");

        assert_eq!(
            resolved,
            fs::canonicalize(&icon_path)
                .unwrap()
                .to_string_lossy()
                .to_string()
        );
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn resolve_icon_path_rejects_traversal_outside_resources() {
        let base =
            std::env::temp_dir().join(format!("noticeflow-app-icon-{}", uuid::Uuid::new_v4()));
        let app_path = base.join("Foo.app");
        let resources = app_path.join("Contents/Resources");
        fs::create_dir_all(&resources).expect("resources should be created");
        fs::write(base.join("outside.icns"), b"icon").expect("outside icon should be written");

        assert!(resolve_icon_path(&app_path, "../../../outside.icns").is_none());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn application_insert_deduplicates_bundle_ids_case_insensitively() {
        let mut apps = BTreeMap::new();
        insert_application(
            &mut apps,
            app("First", "com.Example.App", "/Applications/First.app"),
        );
        insert_application(
            &mut apps,
            app("Second", "com.example.app", "/Applications/Second.app"),
        );

        assert_eq!(apps.len(), 1);
        let only = apps.values().next().expect("application should be kept");
        assert_eq!(only.name, "First");
        assert_eq!(only.path, "/Applications/First.app");
    }

    #[test]
    fn application_sort_is_stable_by_name_bundle_and_path() {
        let mut apps = vec![
            app("Zulu", "com.example.zulu", "/Applications/Zulu.app"),
            app("Alpha", "com.example.beta", "/Applications/Beta.app"),
            app("alpha", "com.example.alpha", "/Applications/Alpha.app"),
            app("Alpha", "com.example.alpha", "/Applications/Alpha-2.app"),
        ];

        sort_applications(&mut apps);

        let ordered: Vec<&str> = apps.iter().map(|item| item.path.as_str()).collect();
        assert_eq!(
            ordered,
            vec![
                "/Applications/Alpha-2.app",
                "/Applications/Alpha.app",
                "/Applications/Beta.app",
                "/Applications/Zulu.app",
            ]
        );
    }

    #[test]
    fn icon_cache_file_name_tracks_source_changes() {
        let modified = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let same = icon_cache_file_name("/Applications/Foo.app/Icon.icns", 10, modified);

        assert_eq!(
            same,
            icon_cache_file_name("/Applications/Foo.app/Icon.icns", 10, modified)
        );
        assert_ne!(
            same,
            icon_cache_file_name("/Applications/Foo.app/Icon.icns", 11, modified)
        );
        assert_ne!(
            same,
            icon_cache_file_name(
                "/Applications/Foo.app/Icon.icns",
                10,
                modified + Duration::from_secs(1),
            )
        );
    }
}

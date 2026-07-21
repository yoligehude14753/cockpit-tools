use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

const PROFILE_ROOT_NAME: &str = "codex_web_profiles";
const PROFILE_MAPPING_FILE: &str = "codex_web_profiles.json";
const CODEX_WEB_URL: &str = "https://chatgpt.com/codex";
const VERIFICATION_MAILBOX_URL: &str = "https://chongzhi.art/mailbox";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexWebProfileRecord {
    pub account_key: String,
    pub display_label: String,
    pub profile_path: String,
    pub created_at: i64,
    pub last_opened_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CodexWebProfileState {
    NotCreated,
    Created,
    InUse,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexWebProfileStatus {
    pub state: CodexWebProfileState,
    pub account_key: String,
    pub display_label: String,
    pub profile_path: String,
    pub created_at: Option<i64>,
    pub last_opened_at: Option<i64>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CodexWebProfileMapping {
    #[serde(default)]
    profiles: Vec<CodexWebProfileRecord>,
}

pub fn codex_web_url() -> &'static str {
    CODEX_WEB_URL
}

pub fn verification_mailbox_url() -> &'static str {
    VERIFICATION_MAILBOX_URL
}

pub fn profile_account_key(account_id: &str) -> Result<String, String> {
    let account_id = account_id.trim();
    if account_id.is_empty() {
        return Err("Codex 账号 ID 为空".to_string());
    }
    let mut hasher = Sha256::new();
    hasher.update(account_id.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

fn profile_root(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(PROFILE_ROOT_NAME)
}

fn mapping_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(PROFILE_MAPPING_FILE)
}

fn valid_account_key(account_key: &str) -> bool {
    account_key.len() == 64 && account_key.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn canonical_profile_root(app_data_dir: &Path) -> Result<PathBuf, String> {
    let app_data_dir = app_data_dir
        .canonicalize()
        .map_err(|error| format!("解析 Cockpit 应用数据目录失败: {error}"))?;
    profile_root(&app_data_dir)
        .canonicalize()
        .map_err(|error| format!("解析 Web Profile 根目录失败: {error}"))
}

fn resolve_managed_profile_path(
    app_data_dir: &Path,
    profile_path: &str,
    expected_account_key: Option<&str>,
) -> Result<PathBuf, String> {
    let profile_path = profile_path.trim();
    let relative_path = Path::new(profile_path);
    if profile_path.is_empty()
        || relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("Web Profile 路径必须是受管根目录下的相对路径".to_string());
    }

    let app_data_dir = app_data_dir
        .canonicalize()
        .map_err(|error| format!("解析 Cockpit 应用数据目录失败: {error}"))?;
    let root = canonical_profile_root(&app_data_dir)?;
    let candidate = app_data_dir.join(relative_path);
    let root_prefix = app_data_dir.join(PROFILE_ROOT_NAME);
    if !candidate.starts_with(&root_prefix) {
        return Err("Web Profile 路径越过受管根目录".to_string());
    }

    let relative_to_root = candidate
        .strip_prefix(&root_prefix)
        .map_err(|_| "Web Profile 路径不在受管根目录下".to_string())?;
    if relative_to_root.as_os_str().is_empty()
        || relative_to_root.components().count() != 1
        || expected_account_key
            .map(|account_key| relative_to_root != Path::new(account_key))
            .unwrap_or(false)
    {
        return Err("Web Profile 路径格式不合法".to_string());
    }

    let canonical = candidate
        .canonicalize()
        .map_err(|error| format!("解析 Web Profile 路径失败: {error}"))?;
    if canonical == root || !canonical.starts_with(&root) {
        return Err("Web Profile 路径越过受管根目录".to_string());
    }
    Ok(canonical)
}

fn load_mapping(app_data_dir: &Path) -> Result<CodexWebProfileMapping, String> {
    let path = mapping_path(app_data_dir);
    if !path.exists() {
        return Ok(CodexWebProfileMapping::default());
    }
    let raw =
        fs::read_to_string(&path).map_err(|error| format!("读取 Web Profile 映射失败: {error}"))?;
    let mapping: CodexWebProfileMapping = serde_json::from_str(&raw)
        .map_err(|error| format!("解析 Web Profile 映射失败: {error}"))?;
    for record in &mapping.profiles {
        if !valid_account_key(&record.account_key) {
            return Err("Web Profile 映射包含不合法的账号 ID".to_string());
        }
        resolve_managed_profile_path(
            app_data_dir,
            &record.profile_path,
            Some(&record.account_key),
        )?;
    }
    Ok(mapping)
}

fn save_mapping(app_data_dir: &Path, mapping: &CodexWebProfileMapping) -> Result<(), String> {
    fs::create_dir_all(app_data_dir)
        .map_err(|error| format!("创建 Web Profile 数据目录失败: {error}"))?;
    let path = mapping_path(app_data_dir);
    let temp_path = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(mapping)
        .map_err(|error| format!("序列化 Web Profile 映射失败: {error}"))?;
    fs::write(&temp_path, raw).map_err(|error| format!("写入 Web Profile 映射失败: {error}"))?;
    fs::rename(&temp_path, &path).map_err(|error| format!("替换 Web Profile 映射失败: {error}"))
}

fn ensure_profile_dir(app_data_dir: &Path, account_key: &str) -> Result<PathBuf, String> {
    if !valid_account_key(account_key) {
        return Err("Web Profile ID 不合法".to_string());
    }
    fs::create_dir_all(app_data_dir)
        .map_err(|error| format!("创建 Web Profile 数据目录失败: {error}"))?;
    let root = profile_root(app_data_dir);
    fs::create_dir_all(&root).map_err(|error| format!("创建 Web Profile 根目录失败: {error}"))?;
    let root = root
        .canonicalize()
        .map_err(|error| format!("解析 Web Profile 根目录失败: {error}"))?;
    let profile_dir = root.join(account_key);
    if profile_dir.exists() && !profile_dir.is_dir() {
        return Err("Web Profile 路径不是目录".to_string());
    }
    fs::create_dir_all(&profile_dir)
        .map_err(|error| format!("创建 Web Profile 目录失败: {error}"))?;
    let profile_dir = profile_dir
        .canonicalize()
        .map_err(|error| format!("解析 Web Profile 目录失败: {error}"))?;
    if !profile_dir.starts_with(&root) {
        return Err("Web Profile 路径越过受管根目录".to_string());
    }
    Ok(profile_dir)
}

fn profile_path_string(app_data_dir: &Path, profile_dir: &Path) -> String {
    profile_dir
        .strip_prefix(app_data_dir)
        .unwrap_or(profile_dir)
        .to_string_lossy()
        .to_string()
}

fn record_for(
    mapping: &CodexWebProfileMapping,
    account_key: &str,
) -> Option<CodexWebProfileRecord> {
    mapping
        .profiles
        .iter()
        .find(|record| record.account_key == account_key)
        .cloned()
}

#[derive(Debug, Clone)]
struct ProcessSnapshot {
    name: OsString,
    exe: Option<PathBuf>,
    cmd: Vec<OsString>,
}

fn profile_in_use(profile_dir: &Path) -> Result<bool, String> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

    let expected = profile_dir
        .canonicalize()
        .map_err(|error| format!("解析 Firefox Profile 路径失败: {error}"))?;
    let mut system = System::new();
    system
        .refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cmd(UpdateKind::OnlyIfNotSet),
        )
        .map_err(|error| format!("查询 Firefox 进程失败: {error}"))?;

    let processes = system
        .processes()
        .values()
        .map(|process| ProcessSnapshot {
            name: process.name().to_os_string(),
            exe: process.exe().map(Path::to_path_buf),
            cmd: process.cmd().to_vec(),
        })
        .collect();
    profile_in_use_from_processes(Ok(processes), &expected)
}

fn profile_in_use_from_processes(
    processes: Result<Vec<ProcessSnapshot>, String>,
    expected: &Path,
) -> Result<bool, String> {
    let processes = processes?;
    Ok(processes.iter().any(|process| {
        is_firefox_process(&process.name, process.exe.as_deref(), &process.cmd)
            && command_line_has_firefox_profile(&process.cmd, expected)
    }))
}

fn is_firefox_process(name: &OsStr, exe: Option<&Path>, args: &[OsString]) -> bool {
    let mut candidates = vec![name.to_string_lossy().to_ascii_lowercase()];
    if let Some(exe_name) = exe.and_then(Path::file_name) {
        candidates.push(exe_name.to_string_lossy().to_ascii_lowercase());
    }
    if let Some(command_name) = args.first().and_then(|arg| Path::new(arg).file_name()) {
        candidates.push(command_name.to_string_lossy().to_ascii_lowercase());
    }
    candidates.iter().any(|candidate| {
        matches!(
            candidate.as_str(),
            "firefox" | "firefox-bin" | "firefox.exe"
        )
    })
}

fn command_line_has_firefox_profile(args: &[OsString], expected: &Path) -> bool {
    args.windows(2).any(|pair| {
        pair[0] == OsStr::new("-profile")
            && normalize_path(Path::new(&pair[1])) == normalize_path(expected)
    })
}

fn normalize_path(value: &Path) -> String {
    value
        .canonicalize()
        .unwrap_or_else(|_| value.to_path_buf())
        .to_string_lossy()
        .trim_end_matches(std::path::MAIN_SEPARATOR)
        .to_ascii_lowercase()
}

fn firefox_candidates() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let mut candidates = vec![PathBuf::from(
            "/Applications/Firefox.app/Contents/MacOS/firefox",
        )];
        if let Some(home) = dirs::home_dir() {
            candidates.push(home.join("Applications/Firefox.app/Contents/MacOS/firefox"));
        }
        candidates
    }

    #[cfg(target_os = "windows")]
    {
        vec![PathBuf::from("firefox.exe")]
    }

    #[cfg(target_os = "linux")]
    {
        vec![PathBuf::from("firefox"), PathBuf::from("firefox-esr")]
    }
}

fn launch_firefox(profile_dir: &Path) -> Result<(), String> {
    let profile_dir = profile_dir
        .canonicalize()
        .map_err(|error| format!("解析 Firefox Profile 路径失败: {error}"))?;
    let profile_dir = profile_dir.to_string_lossy().to_string();

    for candidate in firefox_candidates() {
        if cfg!(target_os = "macos") && !candidate.is_file() {
            continue;
        }
        let result = Command::new(&candidate)
            .args([
                "-profile",
                &profile_dir,
                "-no-remote",
                "-new-instance",
                CODEX_WEB_URL,
            ])
            .spawn();
        if result.is_ok() {
            return Ok(());
        }
    }

    Err("未找到可用的 Firefox 官方版。请安装 Firefox 后重试".to_string())
}

pub fn get_status(app_data_dir: &Path, account_id: &str) -> Result<CodexWebProfileStatus, String> {
    let account_key = profile_account_key(account_id)?;
    let mapping = load_mapping(app_data_dir)?;
    let record = record_for(&mapping, &account_key);
    let Some(record) = record else {
        return Ok(CodexWebProfileStatus {
            state: CodexWebProfileState::NotCreated,
            account_key,
            display_label: String::new(),
            profile_path: String::new(),
            created_at: None,
            last_opened_at: None,
        });
    };
    let profile_dir = resolve_managed_profile_path(
        app_data_dir,
        &record.profile_path,
        Some(&record.account_key),
    )?;
    let state = if profile_in_use(&profile_dir)? {
        CodexWebProfileState::InUse
    } else {
        CodexWebProfileState::Created
    };
    Ok(CodexWebProfileStatus {
        state,
        account_key,
        display_label: record.display_label,
        profile_path: record.profile_path,
        created_at: Some(record.created_at),
        last_opened_at: record.last_opened_at,
    })
}

pub fn open_profile(
    app_data_dir: &Path,
    account_id: &str,
) -> Result<CodexWebProfileStatus, String> {
    let account_key = profile_account_key(account_id)?;
    let mut mapping = load_mapping(app_data_dir)?;
    let profile_dir = ensure_profile_dir(app_data_dir, &account_key)?;
    if profile_in_use(&profile_dir)? {
        return Err(
            "该 Firefox 网页会话 Profile 正在使用，请先在对应 Firefox 窗口中操作".to_string(),
        );
    }

    let now = Utc::now().timestamp();
    let record = record_for(&mapping, &account_key).unwrap_or_else(|| CodexWebProfileRecord {
        account_key: account_key.clone(),
        display_label: format!("Codex Web {}", &account_key[..12]),
        profile_path: profile_path_string(app_data_dir, &profile_dir),
        created_at: now,
        last_opened_at: None,
    });
    launch_firefox(&profile_dir)?;
    let mut updated = record;
    updated.last_opened_at = Some(now);
    mapping
        .profiles
        .retain(|item| item.account_key != account_key);
    mapping.profiles.push(updated);
    mapping
        .profiles
        .sort_by(|left, right| left.account_key.cmp(&right.account_key));
    save_mapping(app_data_dir, &mapping)?;
    get_status(app_data_dir, account_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn account_key_is_stable_and_non_email_shaped() {
        let key = profile_account_key("account-id-1").unwrap();
        assert_eq!(key.len(), 64);
        assert!(!key.contains('@'));
        assert_eq!(key, profile_account_key("account-id-1").unwrap());
    }

    #[test]
    fn profile_directory_stays_inside_root() {
        let root = std::env::temp_dir().join(format!("cockpit-web-profile-{}", std::process::id()));
        let key = profile_account_key("account-id-2").unwrap();
        let profile = ensure_profile_dir(&root, &key).unwrap();
        assert!(profile.starts_with(root.join(PROFILE_ROOT_NAME).canonicalize().unwrap()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_path_like_profile_ids() {
        assert!(ensure_profile_dir(Path::new("/tmp"), "../outside").is_err());
    }

    #[test]
    fn command_line_matching_requires_exact_profile_path() {
        let path = "/tmp/cockpit-profile";
        let args = |items: &[&str]| {
            items
                .iter()
                .map(|item| OsString::from(item))
                .collect::<Vec<_>>()
        };
        assert!(command_line_has_firefox_profile(
            &args(&[
                "/Applications/Firefox.app/Contents/MacOS/firefox",
                "-profile",
                path,
                "-no-remote",
            ]),
            Path::new(path),
        ));
        assert!(!command_line_has_firefox_profile(
            &args(&[
                "/Applications/Firefox.app/Contents/MacOS/firefox",
                "-profile",
                "/tmp/cockpit-profile-other",
            ]),
            Path::new(path),
        ));
        assert!(command_line_has_firefox_profile(
            &args(&[
                "/Applications/Firefox.app/Contents/MacOS/firefox",
                "-profile",
                "/tmp/cockpit profile",
            ]),
            Path::new("/tmp/cockpit profile"),
        ));
    }

    #[test]
    fn mapping_rejects_profile_path_outside_managed_root() {
        let root = std::env::temp_dir().join(format!(
            "cockpit-web-profile-mapping-{}",
            std::process::id()
        ));
        let key = profile_account_key("account-id-3").unwrap();
        let managed = root.join(PROFILE_ROOT_NAME).join(&key);
        fs::create_dir_all(&managed).unwrap();
        let mapping = CodexWebProfileMapping {
            profiles: vec![CodexWebProfileRecord {
                account_key: key,
                display_label: "test".to_string(),
                profile_path: "../outside".to_string(),
                created_at: 0,
                last_opened_at: None,
            }],
        };
        fs::write(mapping_path(&root), serde_json::to_vec(&mapping).unwrap()).unwrap();
        assert!(load_mapping(&root).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn process_query_error_fails_closed() {
        let result = profile_in_use_from_processes(
            Err("test failure".to_string()),
            Path::new("/tmp/cockpit-profile"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn process_detection_requires_firefox_binary() {
        let expected = Path::new("/tmp/cockpit-profile");
        let firefox = ProcessSnapshot {
            name: OsString::from("firefox"),
            exe: Some(PathBuf::from(
                "/Applications/Firefox.app/Contents/MacOS/firefox",
            )),
            cmd: [
                "/Applications/Firefox.app/Contents/MacOS/firefox",
                "-profile",
                "/tmp/cockpit-profile",
            ]
            .into_iter()
            .map(OsString::from)
            .collect(),
        };
        let other_browser = ProcessSnapshot {
            name: OsString::from("Other Browser"),
            exe: Some(PathBuf::from(
                "/Applications/Other Browser.app/Contents/MacOS/browser",
            )),
            cmd: firefox.cmd.clone(),
        };
        assert!(profile_in_use_from_processes(Ok(vec![firefox]), expected).unwrap());
        assert!(!profile_in_use_from_processes(Ok(vec![other_browser]), expected).unwrap());
    }
}

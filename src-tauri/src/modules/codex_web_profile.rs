use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

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

fn profile_in_use_from_ps_output(
    output: std::io::Result<Output>,
    expected: &str,
) -> Result<bool, String> {
    let output = output.map_err(|error| format!("查询 Chrome 进程失败: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "查询 Chrome 进程失败，退出码 {:?}",
            output.status.code()
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("读取 Chrome 进程信息失败: {error}"))?;
    Ok(stdout.lines().any(|line| {
        let lower = line.to_ascii_lowercase();
        let is_chrome = lower.contains("google chrome") || lower.contains("chromium");
        is_chrome && command_line_has_user_data_dir(line, expected)
    }))
}

fn profile_in_use(profile_dir: &Path) -> Result<bool, String> {
    let expected = profile_dir.to_string_lossy().to_string();
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("ps").args(["-axo", "command="]).output();
        return profile_in_use_from_ps_output(output, &expected);
    }

    #[cfg(not(target_os = "macos"))]
    {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
        let mut system = System::new();
        system
            .refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing().with_cmd(UpdateKind::OnlyIfNotSet),
            )
            .map_err(|error| format!("查询 Chrome 进程失败: {error}"))?;
        Ok(system.processes().values().any(|process| {
            let command = process
                .cmd()
                .iter()
                .map(|arg| arg.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ");
            let lower = command.to_ascii_lowercase();
            (lower.contains("chrome") || lower.contains("chromium"))
                && command_line_has_user_data_dir(&command, &expected)
        }))
    }
}

fn command_line_has_user_data_dir(command_line: &str, expected: &str) -> bool {
    let expected = Path::new(expected);
    let args = command_line.split_whitespace().collect::<Vec<_>>();
    args.windows(2).any(|pair| {
        pair[0] == "--user-data-dir"
            && normalize_path(pair[1]) == normalize_path(expected.to_string_lossy().as_ref())
    }) || args.iter().any(|arg| {
        arg.strip_prefix("--user-data-dir=")
            .map(|value| {
                normalize_path(value) == normalize_path(expected.to_string_lossy().as_ref())
            })
            .unwrap_or(false)
    })
}

fn normalize_path(value: &str) -> String {
    Path::new(value)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(value))
        .to_string_lossy()
        .trim_end_matches(std::path::MAIN_SEPARATOR)
        .to_ascii_lowercase()
}

fn launch_chrome(profile_dir: &Path) -> Result<(), String> {
    let profile_dir = profile_dir.to_string_lossy().to_string();
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &[
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ]
    } else if cfg!(target_os = "windows") {
        &["chrome.exe", "chromium.exe"]
    } else {
        &[
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
        ]
    };

    for candidate in candidates {
        let path_candidate = Path::new(candidate);
        if cfg!(target_os = "macos") && !path_candidate.is_file() {
            continue;
        }
        let result = Command::new(candidate)
            .args([
                "--user-data-dir",
                &profile_dir,
                "--new-window",
                CODEX_WEB_URL,
            ])
            .spawn();
        if result.is_ok() {
            return Ok(());
        }
    }

    Err("未找到可用的 Chrome/Chromium。请安装 Google Chrome 后重试".to_string())
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
        return Err("该 Web Profile 正在使用，请先在对应 Chrome 窗口中操作".to_string());
    }

    let now = Utc::now().timestamp();
    let record = record_for(&mapping, &account_key).unwrap_or_else(|| CodexWebProfileRecord {
        account_key: account_key.clone(),
        display_label: format!("Codex Web {}", &account_key[..12]),
        profile_path: profile_path_string(app_data_dir, &profile_dir),
        created_at: now,
        last_opened_at: None,
    });
    launch_chrome(&profile_dir)?;
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
        assert!(command_line_has_user_data_dir(
            "Google Chrome --user-data-dir=/tmp/cockpit-profile --new-window",
            path
        ));
        assert!(!command_line_has_user_data_dir(
            "Google Chrome --user-data-dir=/tmp/cockpit-profile-other",
            path
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
        let result = profile_in_use_from_ps_output(
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "test failure",
            )),
            "/tmp/cockpit-profile",
        );
        assert!(result.is_err());
    }
}

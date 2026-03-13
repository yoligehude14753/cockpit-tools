//! 托盘平台布局配置
//! 用于控制托盘中平台的显示与排序模式

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const TRAY_LAYOUT_FILE: &str = "tray_layout.json";

pub const PLATFORM_ANTIGRAVITY: &str = "antigravity";
pub const PLATFORM_CODEX: &str = "codex";
pub const PLATFORM_GITHUB_COPILOT: &str = "github-copilot";
pub const PLATFORM_WINDSURF: &str = "windsurf";
pub const PLATFORM_KIRO: &str = "kiro";
pub const PLATFORM_CURSOR: &str = "cursor";
pub const PLATFORM_GEMINI: &str = "gemini";
pub const PLATFORM_CODEBUDDY: &str = "codebuddy";
pub const PLATFORM_CODEBUDDY_CN: &str = "codebuddy_cn";
pub const PLATFORM_QODER: &str = "qoder";
pub const PLATFORM_TRAE: &str = "trae";

pub const SUPPORTED_PLATFORM_IDS: [&str; 11] = [
    PLATFORM_ANTIGRAVITY,
    PLATFORM_CODEX,
    PLATFORM_GITHUB_COPILOT,
    PLATFORM_WINDSURF,
    PLATFORM_KIRO,
    PLATFORM_CURSOR,
    PLATFORM_GEMINI,
    PLATFORM_CODEBUDDY,
    PLATFORM_CODEBUDDY_CN,
    PLATFORM_QODER,
    PLATFORM_TRAE,
];

pub const SORT_MODE_AUTO: &str = "auto";
pub const SORT_MODE_MANUAL: &str = "manual";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayLayoutConfig {
    #[serde(default = "default_sort_mode")]
    pub sort_mode: String,
    #[serde(default = "default_order")]
    pub ordered_platform_ids: Vec<String>,
    #[serde(default = "default_tray_platforms")]
    pub tray_platform_ids: Vec<String>,
}

fn default_sort_mode() -> String {
    SORT_MODE_AUTO.to_string()
}

fn default_order() -> Vec<String> {
    SUPPORTED_PLATFORM_IDS
        .iter()
        .map(|id| (*id).to_string())
        .collect()
}

fn default_tray_platforms() -> Vec<String> {
    default_order()
}

impl Default for TrayLayoutConfig {
    fn default() -> Self {
        Self {
            sort_mode: default_sort_mode(),
            ordered_platform_ids: default_order(),
            tray_platform_ids: default_tray_platforms(),
        }
    }
}

fn get_tray_layout_path() -> Result<PathBuf, String> {
    Ok(crate::modules::account::get_data_dir()?.join(TRAY_LAYOUT_FILE))
}

fn is_supported_platform_id(id: &str) -> bool {
    SUPPORTED_PLATFORM_IDS.contains(&id)
}

fn sanitize_platform_ids(ids: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    for id in ids {
        let trimmed = id.trim();
        if trimmed.is_empty() || !is_supported_platform_id(trimmed) {
            continue;
        }
        if result.iter().any(|existing| existing == trimmed) {
            continue;
        }
        result.push(trimmed.to_string());
    }
    result
}

fn normalize_order(ids: &[String]) -> Vec<String> {
    let mut ordered = sanitize_platform_ids(ids);
    for default_id in SUPPORTED_PLATFORM_IDS {
        if !ordered.iter().any(|id| id == default_id) {
            ordered.push(default_id.to_string());
        }
    }
    ordered
}

fn contains_platform(ids: &[String], target: &str) -> bool {
    ids.iter().any(|id| id == target)
}

fn normalize_tray_platforms(ids: &[String], raw_order_has_new: &[&str]) -> Vec<String> {
    let mut sanitized = sanitize_platform_ids(ids);

    let has_legacy_all = contains_platform(&sanitized, PLATFORM_ANTIGRAVITY)
        && contains_platform(&sanitized, PLATFORM_CODEX)
        && contains_platform(&sanitized, PLATFORM_GITHUB_COPILOT)
        && contains_platform(&sanitized, PLATFORM_WINDSURF);

    for &new_platform in &[
        PLATFORM_KIRO,
        PLATFORM_CURSOR,
        PLATFORM_GEMINI,
        PLATFORM_CODEBUDDY,
        PLATFORM_CODEBUDDY_CN,
        PLATFORM_QODER,
        PLATFORM_TRAE,
    ] {
        let already_present = contains_platform(&sanitized, new_platform);
        let was_in_raw_order = raw_order_has_new.contains(&new_platform);
        let looks_like_old_default = !already_present
            && has_legacy_all
            && sanitized.len() <= SUPPORTED_PLATFORM_IDS.len().saturating_sub(1);

        if !was_in_raw_order && !already_present && looks_like_old_default {
            sanitized.push(new_platform.to_string());
        }
    }

    sanitized
}

fn normalize_sort_mode(raw: &str) -> String {
    match raw.trim() {
        SORT_MODE_MANUAL => SORT_MODE_MANUAL.to_string(),
        _ => SORT_MODE_AUTO.to_string(),
    }
}

fn normalize_config(config: TrayLayoutConfig) -> TrayLayoutConfig {
    let raw_order_new_platforms: Vec<&str> = [
        PLATFORM_KIRO,
        PLATFORM_CURSOR,
        PLATFORM_GEMINI,
        PLATFORM_CODEBUDDY,
        PLATFORM_CODEBUDDY_CN,
        PLATFORM_QODER,
        PLATFORM_TRAE,
    ]
    .iter()
    .filter(|&&p| config.ordered_platform_ids.iter().any(|id| id.trim() == p))
    .copied()
    .collect();
    TrayLayoutConfig {
        sort_mode: normalize_sort_mode(&config.sort_mode),
        ordered_platform_ids: normalize_order(&config.ordered_platform_ids),
        tray_platform_ids: normalize_tray_platforms(
            &config.tray_platform_ids,
            &raw_order_new_platforms,
        ),
    }
}

pub fn load_tray_layout() -> TrayLayoutConfig {
    let path = match get_tray_layout_path() {
        Ok(path) => path,
        Err(_) => return TrayLayoutConfig::default(),
    };

    if !path.exists() {
        return TrayLayoutConfig::default();
    }

    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(_) => return TrayLayoutConfig::default(),
    };

    match serde_json::from_str::<TrayLayoutConfig>(&content) {
        Ok(config) => normalize_config(config),
        Err(_) => TrayLayoutConfig::default(),
    }
}

pub fn save_tray_layout(
    sort_mode: String,
    ordered_platform_ids: Vec<String>,
    tray_platform_ids: Vec<String>,
) -> Result<TrayLayoutConfig, String> {
    let normalized = normalize_config(TrayLayoutConfig {
        sort_mode,
        ordered_platform_ids,
        tray_platform_ids,
    });

    let path = get_tray_layout_path()?;
    let content = serde_json::to_string_pretty(&normalized)
        .map_err(|e| format!("序列化托盘布局配置失败: {}", e))?;
    fs::write(&path, content).map_err(|e| format!("保存托盘布局配置失败: {}", e))?;
    Ok(normalized)
}

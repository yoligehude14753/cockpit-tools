use std::path::PathBuf;

use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::modules::codex_web_profile::{self, CodexWebProfileStatus};

fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|error| format!("解析 Cockpit 应用数据目录失败: {error}"))
}

#[tauri::command]
pub fn get_codex_web_profile_status(
    app: AppHandle,
    account_id: String,
) -> Result<CodexWebProfileStatus, String> {
    codex_web_profile::get_status(&app_data_dir(&app)?, &account_id)
}

#[tauri::command]
pub fn open_codex_web_profile(
    app: AppHandle,
    account_id: String,
) -> Result<CodexWebProfileStatus, String> {
    codex_web_profile::open_profile(&app_data_dir(&app)?, &account_id)
}

#[tauri::command]
pub fn open_codex_verification_mailbox(app: AppHandle) -> Result<(), String> {
    app.opener()
        .open_url(
            codex_web_profile::verification_mailbox_url(),
            None::<String>,
        )
        .map_err(|error| format!("打开验证邮箱失败: {error}"))
}

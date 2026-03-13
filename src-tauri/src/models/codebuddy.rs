use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodebuddyQuotaRequestHeaders {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accept: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accept_language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub referer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sec_fetch_site: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sec_fetch_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sec_fetch_dest: Option<String>,
}

impl CodebuddyQuotaRequestHeaders {
    pub fn is_empty(&self) -> bool {
        self.accept.is_none()
            && self.accept_language.is_none()
            && self.content_type.is_none()
            && self.origin.is_none()
            && self.referer.is_none()
            && self.user_agent.is_none()
            && self.sec_fetch_site.is_none()
            && self.sec_fetch_mode.is_none()
            && self.sec_fetch_dest.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodebuddyQuotaBinding {
    pub cookie_header: String,
    pub product_code: String,
    pub status: Vec<i32>,
    pub package_end_time_range_begin: String,
    pub package_end_time_range_end: String,
    pub page_number: i32,
    pub page_size: i32,
    pub updated_at: i64,
    /// 采集到的真实 User-Agent，用于 reqwest 重放时保持一致
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    /// 采集到的请求头快照（用于重放关键鉴权头）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_headers: Option<CodebuddyQuotaRequestHeaders>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodebuddyAccount {
    pub id: String,
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enterprise_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enterprise_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,

    pub access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dosage_notify_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dosage_notify_zh: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dosage_notify_en: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payment_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota_raw: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_raw: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_raw: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_raw: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota_binding: Option<CodebuddyQuotaBinding>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota_query_last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota_query_last_error_at: Option<i64>,

    pub created_at: i64,
    pub last_used: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodebuddyAccountSummary {
    pub id: String,
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_type: Option<String>,
    pub created_at: i64,
    pub last_used: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodebuddyAccountIndex {
    pub version: String,
    pub accounts: Vec<CodebuddyAccountSummary>,
}

impl CodebuddyAccountIndex {
    pub fn new() -> Self {
        Self {
            version: "1.0".to_string(),
            accounts: Vec::new(),
        }
    }
}

impl Default for CodebuddyAccountIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodebuddyOAuthStartResponse {
    pub login_id: String,
    pub verification_uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval_seconds: u64,
}

#[derive(Debug, Clone)]
pub struct CodebuddyOAuthCompletePayload {
    pub email: String,
    pub uid: Option<String>,
    pub nickname: Option<String>,
    pub enterprise_id: Option<String>,
    pub enterprise_name: Option<String>,

    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: Option<String>,
    pub expires_at: Option<i64>,
    pub domain: Option<String>,

    pub plan_type: Option<String>,
    pub dosage_notify_code: Option<String>,
    pub dosage_notify_zh: Option<String>,
    pub dosage_notify_en: Option<String>,
    pub payment_type: Option<String>,

    pub quota_raw: Option<serde_json::Value>,
    pub auth_raw: Option<serde_json::Value>,
    pub profile_raw: Option<serde_json::Value>,
    pub usage_raw: Option<serde_json::Value>,
    pub quota_binding: Option<CodebuddyQuotaBinding>,

    pub status: Option<String>,
    pub status_reason: Option<String>,
}

impl CodebuddyAccount {
    pub fn summary(&self) -> CodebuddyAccountSummary {
        CodebuddyAccountSummary {
            id: self.id.clone(),
            email: self.email.clone(),
            tags: self.tags.clone(),
            plan_type: self.plan_type.clone(),
            created_at: self.created_at,
            last_used: self.last_used,
        }
    }
}

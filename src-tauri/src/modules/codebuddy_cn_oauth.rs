use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

use crate::models::codebuddy::{
    CodebuddyOAuthCompletePayload, CodebuddyOAuthStartResponse, CodebuddyQuotaRequestHeaders,
};
use crate::modules::logger;

const CODEBUDDY_API_ENDPOINT: &str = "https://copilot.tencent.com";
const CODEBUDDY_API_PREFIX: &str = "/v2/plugin";
const CODEBUDDY_PLATFORM: &str = "ide";
const OAUTH_TIMEOUT_SECONDS: u64 = 600;
const OAUTH_POLL_INTERVAL_MS: u64 = 1500;
const USER_RESOURCE_MAX_RETRIES: usize = 1;
const USER_RESOURCE_BASE_DELAY_MS: u64 = 1_000;
const USER_RESOURCE_MAX_DELAY_MS: u64 = 10_000;
const USER_RESOURCE_BACKOFF_FACTOR: u64 = 2;
const USERCENTER_LOGIN_PATH_HINT: &str = "/auth/realms/copilot/protocol/openid-connect/auth";

#[derive(Clone)]
struct PendingOAuthState {
    login_id: String,
    expires_at: i64,
    state: String,
    cancelled: bool,
}

#[derive(Debug, Clone, Default)]
struct ConsoleAccountContext {
    uid: Option<String>,
    uin: Option<String>,
    account_type: Option<String>,
    nickname: Option<String>,
    email: Option<String>,
    enterprise_id: Option<String>,
    enterprise_name: Option<String>,
    profile_raw: Option<Value>,
}

#[derive(Debug, Clone)]
enum ConsoleAccountsFetchOutcome {
    Success {
        payload: Value,
        refreshed_cookie: Option<String>,
    },
    NeedLogin {
        reason: String,
        refreshed_cookie: Option<String>,
    },
}

lazy_static::lazy_static! {
    static ref PENDING_OAUTH_STATE: Arc<Mutex<Option<PendingOAuthState>>> = Arc::new(Mutex::new(None));
}

fn now_timestamp() -> i64 {
    chrono::Utc::now().timestamp()
}

fn generate_login_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..16).map(|_| rng.gen::<u8>()).collect();
    format!(
        "cb_{}",
        bytes
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>()
    )
}

fn build_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))
}

fn build_client_without_redirect() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))
}

fn build_body_preview(text: &str, max_chars: usize) -> String {
    let preview = text.replace('\n', "\\n");
    let mut out = String::new();
    let mut count = 0usize;
    for ch in preview.chars() {
        if count >= max_chars {
            out.push_str("...<truncated>");
            break;
        }
        out.push(ch);
        count += 1;
    }
    out
}

fn normalize_non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
}

fn value_to_non_empty_string(value: &Value) -> Option<String> {
    match value {
        Value::String(v) => normalize_non_empty(Some(v)),
        Value::Number(v) => Some(v.to_string()),
        _ => None,
    }
}

fn extract_profile_uin(profile_raw: Option<&Value>) -> Option<String> {
    profile_raw.and_then(|profile| {
        profile
            .get("uin")
            .and_then(value_to_non_empty_string)
            .or_else(|| {
                profile
                    .get("data")
                    .and_then(|v| v.get("uin"))
                    .and_then(value_to_non_empty_string)
            })
    })
}

fn extract_profile_account_type(profile_raw: Option<&Value>) -> Option<String> {
    profile_raw.and_then(|profile| {
        profile
            .get("type")
            .and_then(value_to_non_empty_string)
            .or_else(|| {
                profile
                    .get("data")
                    .and_then(|v| v.get("type"))
                    .and_then(value_to_non_empty_string)
            })
    })
}

fn is_personal_account_type(account_type: Option<&str>) -> bool {
    account_type
        .map(|v| v.trim().eq_ignore_ascii_case("personal"))
        .unwrap_or(false)
}

fn normalize_product_code(value: Option<&str>) -> String {
    normalize_non_empty(value).unwrap_or_else(|| "p_tcaca".to_string())
}

fn normalize_user_resource_status(status: &[i32]) -> Vec<i32> {
    let mut normalized: Vec<i32> = status.iter().copied().filter(|v| *v >= 0).collect();
    if normalized.is_empty() {
        return vec![0, 3];
    }
    normalized.sort_unstable();
    normalized.dedup();
    normalized
}

fn build_default_user_resource_time_range() -> (String, String) {
    let now = chrono::Local::now();
    let begin = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let end = (now + chrono::Duration::days(365 * 101))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    (begin, end)
}

fn parse_origin_from_url(value: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(value).ok()?;
    let host = parsed.host_str()?;
    let mut origin = format!("{}://{}", parsed.scheme(), host);
    if let Some(port) = parsed.port() {
        origin.push(':');
        origin.push_str(port.to_string().as_str());
    }
    Some(origin)
}

fn resolve_cookie_api_endpoint(request_headers: Option<&CodebuddyQuotaRequestHeaders>) -> String {
    if let Some(origin) = request_headers
        .and_then(|h| h.origin.as_deref())
        .and_then(|v| normalize_non_empty(Some(v)))
    {
        return origin;
    }
    if let Some(origin) = request_headers
        .and_then(|h| h.referer.as_deref())
        .and_then(parse_origin_from_url)
    {
        return origin;
    }
    CODEBUDDY_API_ENDPOINT.to_string()
}

fn looks_like_login_url(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains(USERCENTER_LOGIN_PATH_HINT)
        || lower.contains("/login?platform=usercenter")
        || lower.contains("/console/accounts/.apisix/redirect")
}

fn response_indicates_login_required(
    api_name: &str,
    status_code: reqwest::StatusCode,
    location_header: Option<&str>,
    final_url: Option<&str>,
    content_type: Option<&str>,
    body: &str,
) -> Option<String> {
    let location = normalize_non_empty(location_header).unwrap_or_default();
    let final_url_norm = normalize_non_empty(final_url).unwrap_or_default();
    let content_type_norm = normalize_non_empty(content_type).unwrap_or_default();
    let body_lower = body.to_ascii_lowercase();
    let is_html = content_type_norm.to_ascii_lowercase().contains("text/html")
        || body_lower.contains("<!doctype html")
        || body_lower.contains("<html");
    let has_login_hint = (!location.is_empty() && looks_like_login_url(location.as_str()))
        || (!final_url_norm.is_empty() && looks_like_login_url(final_url_norm.as_str()))
        || body_lower.contains(USERCENTER_LOGIN_PATH_HINT)
        || body_lower.contains("/login?platform=usercenter");

    if status_code.is_redirection() && has_login_hint {
        return Some(format!(
            "need login: {} 返回登录重定向，http={}, location={}",
            api_name,
            status_code.as_u16(),
            if location.is_empty() {
                "<empty>".to_string()
            } else {
                location
            }
        ));
    }

    if status_code.is_success() && is_html && has_login_hint {
        return Some(format!(
            "need login: {} 返回登录页面，http={}, final_url={}, content_type={}",
            api_name,
            status_code.as_u16(),
            if final_url_norm.is_empty() {
                "<empty>"
            } else {
                final_url_norm.as_str()
            },
            if content_type_norm.is_empty() {
                "<empty>"
            } else {
                content_type_norm.as_str()
            }
        ));
    }

    if status_code.is_success() && is_html && api_name.contains("user resource") {
        return Some(format!(
            "need login: {} 返回 HTML 页面（疑似会话失效），http={}, final_url={}, content_type={}",
            api_name,
            status_code.as_u16(),
            if final_url_norm.is_empty() {
                "<empty>"
            } else {
                final_url_norm.as_str()
            },
            if content_type_norm.is_empty() {
                "<empty>"
            } else {
                content_type_norm.as_str()
            }
        ));
    }

    None
}

fn identities_match_ignore_case(left: Option<&str>, right: Option<&str>) -> bool {
    let left_norm = left.map(|v| v.trim()).filter(|v| !v.is_empty());
    let right_norm = right.map(|v| v.trim()).filter(|v| !v.is_empty());
    match (left_norm, right_norm) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        _ => false,
    }
}

fn pick_console_account_from_list(
    accounts: &[Value],
    preferred_uid: Option<&str>,
    preferred_enterprise_id: Option<&str>,
) -> Option<Value> {
    if let Some(found) = accounts.iter().find(|item| {
        let account_type = item
            .get("type")
            .and_then(value_to_non_empty_string)
            .unwrap_or_default();
        if account_type.eq_ignore_ascii_case("personal") {
            let uid = item.get("uid").and_then(value_to_non_empty_string);
            return identities_match_ignore_case(uid.as_deref(), preferred_uid);
        }
        let enterprise_id = item.get("enterpriseId").and_then(value_to_non_empty_string);
        identities_match_ignore_case(enterprise_id.as_deref(), preferred_enterprise_id)
    }) {
        return Some(found.clone());
    }

    if let Some(found) = accounts.iter().find(|item| {
        item.get("lastLogin")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }) {
        return Some(found.clone());
    }

    accounts.first().cloned()
}

fn parse_console_account_context(
    payload: &Value,
    preferred_uid: Option<&str>,
    preferred_enterprise_id: Option<&str>,
) -> Option<ConsoleAccountContext> {
    let accounts = payload
        .get("data")
        .and_then(|v| v.get("accounts"))
        .and_then(|v| v.as_array())?;
    let selected = pick_console_account_from_list(
        accounts.as_slice(),
        preferred_uid,
        preferred_enterprise_id,
    )?;
    let uid = selected.get("uid").and_then(value_to_non_empty_string);
    let uin = selected.get("uin").and_then(value_to_non_empty_string);
    let account_type = selected.get("type").and_then(value_to_non_empty_string);
    let nickname = selected.get("nickname").and_then(value_to_non_empty_string);
    let email = selected.get("email").and_then(value_to_non_empty_string);
    let enterprise_id = selected
        .get("enterpriseId")
        .and_then(value_to_non_empty_string);
    let enterprise_name = selected
        .get("enterpriseName")
        .and_then(value_to_non_empty_string);
    Some(ConsoleAccountContext {
        uid,
        uin,
        account_type,
        nickname,
        email,
        enterprise_id,
        enterprise_name,
        profile_raw: Some(selected),
    })
}

fn compute_retry_delay_ms(attempt: usize) -> u64 {
    use rand::Rng;
    let exponent = u32::try_from(attempt).unwrap_or(u32::MAX).min(16);
    let base = USER_RESOURCE_BASE_DELAY_MS
        .saturating_mul(USER_RESOURCE_BACKOFF_FACTOR.saturating_pow(exponent))
        .min(USER_RESOURCE_MAX_DELAY_MS);
    let mut rng = rand::thread_rng();
    let jitter = rng.gen_range(0.5f64..1.5f64);
    let delay = ((base as f64) * jitter).round() as u64;
    delay.min(USER_RESOURCE_MAX_DELAY_MS).max(200)
}

fn parse_cookie_header_pairs(cookie_header: &str) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    for segment in cookie_header.split(';') {
        let part = segment.trim();
        if part.is_empty() {
            continue;
        }
        let mut kv = part.splitn(2, '=');
        let key = kv.next().unwrap_or("").trim();
        let value = kv.next().unwrap_or("").trim();
        if key.is_empty() {
            continue;
        }
        if let Some((_, existing_value)) = pairs
            .iter_mut()
            .find(|(existing_key, _)| existing_key.eq_ignore_ascii_case(key))
        {
            *existing_value = value.to_string();
        } else {
            pairs.push((key.to_string(), value.to_string()));
        }
    }
    pairs
}

fn parse_set_cookie_pair(raw: &str) -> Option<(String, String)> {
    let first = raw.split(';').next()?.trim();
    if first.is_empty() {
        return None;
    }
    let mut kv = first.splitn(2, '=');
    let key = kv.next()?.trim();
    if key.is_empty() {
        return None;
    }
    let value = kv.next().unwrap_or("").trim();
    Some((key.to_string(), value.to_string()))
}

fn merge_cookie_header_with_set_cookie(
    cookie_header: &str,
    set_cookie_headers: &[String],
) -> Option<String> {
    if set_cookie_headers.is_empty() {
        return None;
    }
    let mut pairs = parse_cookie_header_pairs(cookie_header);
    let mut changed = false;

    for raw in set_cookie_headers {
        let Some((key, value)) = parse_set_cookie_pair(raw) else {
            continue;
        };
        if value.is_empty() {
            let before = pairs.len();
            pairs.retain(|(existing_key, _)| !existing_key.eq_ignore_ascii_case(&key));
            if pairs.len() != before {
                changed = true;
            }
            continue;
        }

        if let Some((_, existing_value)) = pairs
            .iter_mut()
            .find(|(existing_key, _)| existing_key.eq_ignore_ascii_case(&key))
        {
            if *existing_value != value {
                *existing_value = value;
                changed = true;
            }
        } else {
            pairs.push((key, value));
            changed = true;
        }
    }

    if !changed {
        return None;
    }
    if pairs.is_empty() {
        return None;
    }

    Some(
        pairs
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("; "),
    )
}

async fn fetch_console_accounts_with_cookie(
    cookie_header: &str,
    request_headers: Option<&CodebuddyQuotaRequestHeaders>,
    user_agent_fallback: Option<&str>,
) -> Result<ConsoleAccountsFetchOutcome, String> {
    let client = build_client_without_redirect()?;
    let endpoint = resolve_cookie_api_endpoint(request_headers);
    let url = format!("{}/console/accounts", endpoint);

    let accept = request_headers
        .and_then(|h| h.accept.as_deref())
        .unwrap_or("application/json, text/plain, */*");
    let accept_language = request_headers
        .and_then(|h| h.accept_language.as_deref())
        .unwrap_or("zh-CN,zh;q=0.9");
    let referer = request_headers
        .and_then(|h| h.referer.as_deref())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{}/profile/usage", endpoint));
    let sec_fetch_site = request_headers
        .and_then(|h| h.sec_fetch_site.as_deref())
        .unwrap_or("same-origin");
    let sec_fetch_mode = request_headers
        .and_then(|h| h.sec_fetch_mode.as_deref())
        .unwrap_or("cors");
    let sec_fetch_dest = request_headers
        .and_then(|h| h.sec_fetch_dest.as_deref())
        .unwrap_or("empty");
    let effective_ua = request_headers
        .and_then(|h| h.user_agent.as_deref())
        .or(user_agent_fallback)
        .unwrap_or(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36",
        );

    let resp = client
        .get(&url)
        .header("Accept", accept)
        .header("Accept-Language", accept_language)
        .header("Referer", referer)
        .header("Sec-Fetch-Site", sec_fetch_site)
        .header("Sec-Fetch-Mode", sec_fetch_mode)
        .header("Sec-Fetch-Dest", sec_fetch_dest)
        .header("User-Agent", effective_ua)
        .header("Cookie", cookie_header)
        .send()
        .await
        .map_err(|e| format!("请求 /console/accounts 失败: {}", e))?;

    let status_code = resp.status();
    let location_header = resp
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());
    let set_cookie_headers: Vec<String> = resp
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok().map(|s| s.to_string()))
        .collect();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());
    let final_url = resp.url().to_string();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取 /console/accounts 响应失败: {}", e))?;
    let refreshed_cookie = merge_cookie_header_with_set_cookie(cookie_header, &set_cookie_headers);

    if let Some(reason) = response_indicates_login_required(
        "/console/accounts",
        status_code,
        location_header.as_deref(),
        Some(final_url.as_str()),
        content_type.as_deref(),
        text.as_str(),
    ) {
        return Ok(ConsoleAccountsFetchOutcome::NeedLogin {
            reason,
            refreshed_cookie,
        });
    }

    if !status_code.is_success() {
        return Err(format!(
            "请求 /console/accounts 失败: http={}, body={}",
            status_code.as_u16(),
            text
        ));
    }

    let payload: Value = serde_json::from_str(&text).map_err(|e| {
        format!(
            "解析 /console/accounts 响应失败: {}, http={}, body_preview={}",
            e,
            status_code.as_u16(),
            build_body_preview(&text, 600)
        )
    })?;

    let code = payload.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 && code != 200 {
        let msg = payload
            .get("msg")
            .or_else(|| payload.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        return Err(format!(
            "/console/accounts 返回失败: code={}, msg={}",
            code, msg
        ));
    }

    Ok(ConsoleAccountsFetchOutcome::Success {
        payload,
        refreshed_cookie,
    })
}

async fn register_cloud_session_for_quota_cookie(
    uid: &str,
    cookie_header: &str,
    request_headers: Option<&CodebuddyQuotaRequestHeaders>,
    user_agent_fallback: Option<&str>,
) -> Result<Option<String>, String> {
    let client = build_client()?;
    let endpoint = resolve_cookie_api_endpoint(request_headers);
    let url = format!("{}/auth/realms/copilot/overseas/user/register", endpoint);

    let accept = request_headers
        .and_then(|h| h.accept.as_deref())
        .unwrap_or("application/json, text/plain, */*");
    let accept_language = request_headers
        .and_then(|h| h.accept_language.as_deref())
        .unwrap_or("zh-CN,zh;q=0.9");
    let origin = request_headers
        .and_then(|h| h.origin.as_deref())
        .unwrap_or(endpoint.as_str());
    let referer = request_headers
        .and_then(|h| h.referer.as_deref())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{}/profile/usage", endpoint));
    let sec_fetch_site = request_headers
        .and_then(|h| h.sec_fetch_site.as_deref())
        .unwrap_or("same-origin");
    let sec_fetch_mode = request_headers
        .and_then(|h| h.sec_fetch_mode.as_deref())
        .unwrap_or("cors");
    let sec_fetch_dest = request_headers
        .and_then(|h| h.sec_fetch_dest.as_deref())
        .unwrap_or("empty");
    let effective_ua = request_headers
        .and_then(|h| h.user_agent.as_deref())
        .or(user_agent_fallback)
        .unwrap_or(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36",
        );

    let resp = client
        .get(&url)
        .query(&[("userId", uid)])
        .header("Accept", accept)
        .header("Accept-Language", accept_language)
        .header("Origin", origin)
        .header("Referer", referer)
        .header("Sec-Fetch-Site", sec_fetch_site)
        .header("Sec-Fetch-Mode", sec_fetch_mode)
        .header("Sec-Fetch-Dest", sec_fetch_dest)
        .header("User-Agent", effective_ua)
        .header("Cookie", cookie_header)
        .send()
        .await
        .map_err(|e| format!("请求 register 预热失败: {}", e))?;

    let status_code = resp.status();
    let location_header = resp
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());
    let final_url = resp.url().to_string();
    let set_cookie_headers: Vec<String> = resp
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok().map(|s| s.to_string()))
        .collect();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取 register 预热响应失败: {}", e))?;

    if let Some(reason) = response_indicates_login_required(
        "register 预热",
        status_code,
        location_header.as_deref(),
        Some(final_url.as_str()),
        content_type.as_deref(),
        text.as_str(),
    ) {
        return Err(reason);
    }

    if !status_code.is_success() {
        return Err(format!(
            "register 预热返回非 2xx: http={}, body={}",
            status_code.as_u16(),
            text
        ));
    }

    let result: Value = serde_json::from_str(&text).map_err(|e| {
        format!(
            "解析 register 预热响应失败: {}, http={}, body_preview={}",
            e,
            status_code.as_u16(),
            build_body_preview(&text, 600)
        )
    })?;

    let code = result.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 && code != 200 {
        let msg = result
            .get("msg")
            .or_else(|| result.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        return Err(format!("register 预热失败: code={}, msg={}", code, msg));
    }

    Ok(merge_cookie_header_with_set_cookie(
        cookie_header,
        &set_cookie_headers,
    ))
}

fn clear_pending_login(login_id: &str) -> Result<(), String> {
    let mut pending = PENDING_OAUTH_STATE
        .lock()
        .map_err(|_| "获取锁失败".to_string())?;
    if pending
        .as_ref()
        .map(|s| s.login_id == login_id)
        .unwrap_or(false)
    {
        *pending = None;
    }
    Ok(())
}

pub fn clear_pending_oauth_login(login_id: &str) -> Result<(), String> {
    clear_pending_login(login_id)
}

pub async fn start_login() -> Result<CodebuddyOAuthStartResponse, String> {
    let client = build_client()?;
    let url = format!(
        "{}{}/auth/state?platform={}",
        CODEBUDDY_API_ENDPOINT, CODEBUDDY_API_PREFIX, CODEBUDDY_PLATFORM
    );

    logger::log_info(&format!("[CodeBuddy OAuth] 请求 auth/state: {}", url));

    let resp = client
        .post(&url)
        .json(&json!({}))
        .send()
        .await
        .map_err(|e| format!("请求 auth/state 失败: {}", e))?;

    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 auth/state 响应失败: {}", e))?;

    let data = body
        .get("data")
        .ok_or_else(|| format!("auth/state 响应缺少 data 字段: {}", body))?;

    let state = data
        .get("state")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "auth/state 响应缺少 state".to_string())?
        .to_string();

    let auth_url = data
        .get("authUrl")
        .or_else(|| data.get("auth_url"))
        .or_else(|| data.get("url"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let login_id = generate_login_id();

    let verification_uri = if auth_url.is_empty() {
        format!("{}/login?state={}", CODEBUDDY_API_ENDPOINT, state)
    } else {
        auth_url.clone()
    };

    {
        let mut pending = PENDING_OAUTH_STATE
            .lock()
            .map_err(|_| "获取锁失败".to_string())?;
        *pending = Some(PendingOAuthState {
            login_id: login_id.clone(),
            expires_at: now_timestamp() + OAUTH_TIMEOUT_SECONDS as i64,
            state: state.clone(),
            cancelled: false,
        });
    }

    logger::log_info(&format!(
        "[CodeBuddy OAuth] 登录已启动: login_id={}, state={}",
        login_id, state
    ));

    Ok(CodebuddyOAuthStartResponse {
        login_id,
        verification_uri: verification_uri.clone(),
        verification_uri_complete: Some(verification_uri),
        expires_in: OAUTH_TIMEOUT_SECONDS,
        interval_seconds: OAUTH_POLL_INTERVAL_MS / 1000 + 1,
    })
}

pub async fn complete_login(login_id: &str) -> Result<CodebuddyOAuthCompletePayload, String> {
    let client = build_client()?;
    let start = now_timestamp();

    loop {
        let state_info = {
            let pending = PENDING_OAUTH_STATE
                .lock()
                .map_err(|_| "获取锁失败".to_string())?;
            match pending.as_ref() {
                None => return Err("没有待处理的登录请求".to_string()),
                Some(s) => {
                    if s.login_id != login_id {
                        return Err("login_id 不匹配".to_string());
                    }
                    if s.cancelled {
                        return Err("登录已取消".to_string());
                    }
                    if now_timestamp() > s.expires_at {
                        return Err("登录超时".to_string());
                    }
                    s.clone()
                }
            }
        };

        let url = format!(
            "{}{}/auth/token?state={}",
            CODEBUDDY_API_ENDPOINT, CODEBUDDY_API_PREFIX, state_info.state
        );

        match client.get(&url).send().await {
            Ok(resp) => {
                if let Ok(body) = resp.json::<Value>().await {
                    let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);

                    if code == 0 || code == 200 {
                        if let Some(data) = body.get("data") {
                            let access_token = data
                                .get("accessToken")
                                .or_else(|| data.get("access_token"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();

                            if !access_token.is_empty() {
                                logger::log_info("[CodeBuddy OAuth] 获取 token 成功");

                                let refresh_token = data
                                    .get("refreshToken")
                                    .or_else(|| data.get("refresh_token"))
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());

                                let expires_at = data
                                    .get("expiresAt")
                                    .or_else(|| data.get("expires_at"))
                                    .and_then(|v| v.as_i64());

                                let domain = data
                                    .get("domain")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());

                                let token_type = data
                                    .get("tokenType")
                                    .or_else(|| data.get("token_type"))
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());

                                let auth_raw = Some(data.clone());

                                let account_info = fetch_account_info(
                                    &client,
                                    &access_token,
                                    &state_info.state,
                                    domain.as_deref(),
                                )
                                .await;

                                let (
                                    uid,
                                    nickname,
                                    email,
                                    enterprise_id,
                                    enterprise_name,
                                    profile_raw,
                                ) = match account_info {
                                    Ok(info) => info,
                                    Err(e) => {
                                        logger::log_warn(&format!(
                                            "[CodeBuddy OAuth] 获取账号信息失败: {}",
                                            e
                                        ));
                                        (None, None, String::new(), None, None, None)
                                    }
                                };

                                return Ok(CodebuddyOAuthCompletePayload {
                                    email,
                                    uid,
                                    nickname,
                                    enterprise_id,
                                    enterprise_name,
                                    access_token,
                                    refresh_token,
                                    token_type,
                                    expires_at,
                                    domain,
                                    plan_type: None,
                                    dosage_notify_code: None,
                                    dosage_notify_zh: None,
                                    dosage_notify_en: None,
                                    payment_type: None,
                                    quota_raw: None,
                                    auth_raw,
                                    profile_raw,
                                    usage_raw: None,
                                    quota_binding: None,
                                    status: Some("normal".to_string()),
                                    status_reason: None,
                                });
                            }
                        }
                    }
                }
            }
            Err(e) => {
                logger::log_warn(&format!("[CodeBuddy OAuth] 轮询 token 请求失败: {}", e));
            }
        }

        if now_timestamp() - start > OAUTH_TIMEOUT_SECONDS as i64 {
            let mut pending = PENDING_OAUTH_STATE
                .lock()
                .map_err(|_| "获取锁失败".to_string())?;
            *pending = None;
            return Err("登录超时".to_string());
        }

        tokio::time::sleep(std::time::Duration::from_millis(OAUTH_POLL_INTERVAL_MS)).await;
    }
}

pub fn cancel_login(login_id: Option<&str>) -> Result<(), String> {
    let mut pending = PENDING_OAUTH_STATE
        .lock()
        .map_err(|_| "获取锁失败".to_string())?;
    if let Some(state) = pending.as_mut() {
        if login_id.is_none() || login_id == Some(state.login_id.as_str()) {
            state.cancelled = true;
            *pending = None;
        }
    }
    Ok(())
}

async fn fetch_account_info(
    client: &reqwest::Client,
    access_token: &str,
    state: &str,
    domain: Option<&str>,
) -> Result<
    (
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        Option<Value>,
    ),
    String,
> {
    let url = format!(
        "{}{}/login/account?state={}",
        CODEBUDDY_API_ENDPOINT, CODEBUDDY_API_PREFIX, state
    );

    let mut req = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", access_token));

    if let Some(d) = domain {
        req = req.header("X-Domain", d);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("请求 login/account 失败: {}", e))?;

    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 login/account 响应失败: {}", e))?;

    let data = body.get("data").cloned().unwrap_or(json!({}));

    let uid = data
        .get("uid")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let nickname = data
        .get("nickname")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let email = data
        .get("email")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let enterprise_id = data
        .get("enterpriseId")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let enterprise_name = data
        .get("enterpriseName")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let email_final = if email.is_empty() {
        nickname.clone().or_else(|| uid.clone()).unwrap_or_default()
    } else {
        email
    };

    Ok((
        uid,
        nickname,
        email_final,
        enterprise_id,
        enterprise_name,
        Some(data),
    ))
}

pub async fn refresh_token(
    access_token: &str,
    refresh_token: &str,
    domain: Option<&str>,
) -> Result<Value, String> {
    let client = build_client()?;
    let url = format!(
        "{}{}/auth/token/refresh",
        CODEBUDDY_API_ENDPOINT, CODEBUDDY_API_PREFIX
    );

    let mut req = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("X-Refresh-Token", refresh_token)
        .json(&json!({}));

    if let Some(d) = domain {
        req = req.header("X-Domain", d);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("刷新 token 失败: {}", e))?;

    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("解析刷新响应失败: {}", e))?;

    let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 && code != 200 {
        let msg = body
            .get("message")
            .or_else(|| body.get("msg"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        return Err(format!("刷新 token 失败 (code={}): {}", code, msg));
    }

    body.get("data")
        .cloned()
        .ok_or_else(|| "刷新响应缺少 data 字段".to_string())
}

pub async fn fetch_dosage_notify(
    access_token: &str,
    uid: Option<&str>,
    enterprise_id: Option<&str>,
    domain: Option<&str>,
) -> Result<Value, String> {
    let client = build_client()?;
    let url = format!(
        "{}/v2/billing/meter/get-dosage-notify",
        CODEBUDDY_API_ENDPOINT
    );

    let mut req = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json");

    if let Some(u) = uid {
        req = req.header("X-User-Id", u);
    }
    if let Some(eid) = enterprise_id {
        req = req.header("X-Enterprise-Id", eid);
        req = req.header("X-Tenant-Id", eid);
    }
    if let Some(d) = domain {
        req = req.header("X-Domain", d);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("请求 dosage notify 失败: {}", e))?;

    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 dosage 响应失败: {}", e))?;

    Ok(body)
}

pub async fn fetch_payment_type(
    access_token: &str,
    uid: Option<&str>,
    enterprise_id: Option<&str>,
    domain: Option<&str>,
) -> Result<Value, String> {
    let client = build_client()?;
    let url = format!(
        "{}/v2/billing/meter/get-payment-type",
        CODEBUDDY_API_ENDPOINT
    );

    let mut req = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json");

    if let Some(u) = uid {
        req = req.header("X-User-Id", u);
    }
    if let Some(eid) = enterprise_id {
        req = req.header("X-Enterprise-Id", eid);
        req = req.header("X-Tenant-Id", eid);
    }
    if let Some(d) = domain {
        req = req.header("X-Domain", d);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("请求 payment type 失败: {}", e))?;

    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 payment type 响应失败: {}", e))?;

    Ok(body)
}

pub async fn fetch_user_resource_with_cookie(
    cookie_header: &str,
    product_code: &str,
    status: &[i32],
    package_end_time_range_begin: &str,
    package_end_time_range_end: &str,
    page_number: i32,
    page_size: i32,
    request_headers: Option<&CodebuddyQuotaRequestHeaders>,
    user_agent_fallback: Option<&str>,
) -> Result<(Value, Option<String>), String> {
    let client = build_client()?;
    let endpoint = resolve_cookie_api_endpoint(request_headers);
    let url = format!("{}/billing/meter/get-user-resource", endpoint);

    let body = json!({
        "PageNumber": page_number,
        "PageSize": page_size,
        "ProductCode": product_code,
        "Status": status,
        "PackageEndTimeRangeBegin": package_end_time_range_begin,
        "PackageEndTimeRangeEnd": package_end_time_range_end
    });

    let accept = request_headers
        .and_then(|h| h.accept.as_deref())
        .unwrap_or("application/json, text/plain, */*");
    let accept_language = request_headers
        .and_then(|h| h.accept_language.as_deref())
        .unwrap_or("zh-CN,zh;q=0.9");
    let content_type = request_headers
        .and_then(|h| h.content_type.as_deref())
        .unwrap_or("application/json");
    let origin = request_headers
        .and_then(|h| h.origin.as_deref())
        .unwrap_or(endpoint.as_str());
    let referer = request_headers
        .and_then(|h| h.referer.as_deref())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{}/profile/usage", endpoint));
    let sec_fetch_site = request_headers
        .and_then(|h| h.sec_fetch_site.as_deref())
        .unwrap_or("same-origin");
    let sec_fetch_mode = request_headers
        .and_then(|h| h.sec_fetch_mode.as_deref())
        .unwrap_or("cors");
    let sec_fetch_dest = request_headers
        .and_then(|h| h.sec_fetch_dest.as_deref())
        .unwrap_or("empty");
    let effective_ua = request_headers
        .and_then(|h| h.user_agent.as_deref())
        .or(user_agent_fallback)
        .unwrap_or(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36",
        );

    let resp = client
        .post(&url)
        .header("Accept", accept)
        .header("Accept-Language", accept_language)
        .header("Content-Type", content_type)
        .header("Origin", origin)
        .header("Referer", referer)
        .header("Sec-Fetch-Site", sec_fetch_site)
        .header("Sec-Fetch-Mode", sec_fetch_mode)
        .header("Sec-Fetch-Dest", sec_fetch_dest)
        .header("User-Agent", effective_ua)
        .header("Cookie", cookie_header)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求 user resource（Cookie）失败: {}", e))?;

    let status_code = resp.status();
    let location_header = resp
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());
    let set_cookie_headers: Vec<String> = resp
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok().map(|s| s.to_string()))
        .collect();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());
    let final_url = resp.url().to_string();

    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取 user resource（Cookie）响应失败: {}", e))?;

    if let Some(reason) = response_indicates_login_required(
        "user resource（Cookie）",
        status_code,
        location_header.as_deref(),
        Some(final_url.as_str()),
        content_type.as_deref(),
        text.as_str(),
    ) {
        return Err(reason);
    }

    let text_snippet = build_body_preview(&text, 600);

    let result: Value = serde_json::from_str(&text).map_err(|e| {
        format!(
            "解析 user resource（Cookie）响应失败: {}, http={}, body_preview={}",
            e,
            status_code.as_u16(),
            text_snippet
        )
    })?;

    if !status_code.is_success() {
        return Err(format!(
            "请求 user resource（Cookie）失败: http={}, body={}",
            status_code.as_u16(),
            text
        ));
    }

    let code = result.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 && code != 200 {
        let msg = result
            .get("msg")
            .or_else(|| result.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        return Err(format!(
            "user resource（Cookie）返回失败: code={}, msg={}",
            code, msg
        ));
    }

    Ok((
        result,
        merge_cookie_header_with_set_cookie(cookie_header, &set_cookie_headers),
    ))
}

pub async fn fetch_user_resource_with_cookie_retry_register(
    uid: Option<&str>,
    should_register_prewarm: bool,
    cookie_header: &str,
    product_code: &str,
    status: &[i32],
    package_end_time_range_begin: &str,
    package_end_time_range_end: &str,
    page_number: i32,
    page_size: i32,
    request_headers: Option<&CodebuddyQuotaRequestHeaders>,
    user_agent_fallback: Option<&str>,
) -> Result<(Value, Option<String>), String> {
    let Some(uid_value) = uid.and_then(|v| normalize_non_empty(Some(v))) else {
        return Err("need login: uid 为空，无法查询配额".to_string());
    };

    let mut refreshed_cookie_for_persist: Option<String> = None;
    let mut cookie_for_request = cookie_header.to_string();

    if should_register_prewarm {
        let refreshed_cookie = register_cloud_session_for_quota_cookie(
            uid_value.as_str(),
            cookie_for_request.as_str(),
            request_headers,
            user_agent_fallback,
        )
        .await
        .map_err(|err| format!("查询前 register 预热失败: {}", err))?;
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        if let Some(updated_cookie) = refreshed_cookie {
            cookie_for_request = updated_cookie.clone();
            refreshed_cookie_for_persist = Some(updated_cookie);
        }
    }

    let mut last_err = String::new();
    for attempt in 0..=USER_RESOURCE_MAX_RETRIES {
        match fetch_user_resource_with_cookie(
            cookie_for_request.as_str(),
            product_code,
            status,
            package_end_time_range_begin,
            package_end_time_range_end,
            page_number,
            page_size,
            request_headers,
            user_agent_fallback,
        )
        .await
        {
            Ok((payload, refreshed_cookie)) => {
                if let Some(updated_cookie) = refreshed_cookie {
                    refreshed_cookie_for_persist = Some(updated_cookie);
                }
                return Ok((payload, refreshed_cookie_for_persist));
            }
            Err(err) => {
                if err.trim_start().starts_with("need login:") {
                    return Err(err);
                }
                last_err = err;
                if attempt >= USER_RESOURCE_MAX_RETRIES {
                    break;
                }
                let delay_ms = compute_retry_delay_ms(attempt);
                logger::log_warn(&format!(
                    "[CodeBuddy] user_resource 请求失败，准备重试: attempt={}/{}, delay={}ms, uid={}, error={}",
                    attempt + 1,
                    USER_RESOURCE_MAX_RETRIES + 1,
                    delay_ms,
                    uid_value,
                    last_err
                ));
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
        }
    }

    Err(format!(
        "{}; 重试 {} 次后仍失败",
        last_err, USER_RESOURCE_MAX_RETRIES
    ))
}

pub async fn fetch_user_resource_with_raw_request(
    request_url: &str,
    request_method: &str,
    request_headers: &[(String, String)],
    request_body: Option<&str>,
) -> Result<Value, String> {
    let client = build_client()?;
    let method = reqwest::Method::from_bytes(request_method.trim().as_bytes())
        .map_err(|e| format!("cURL 请求方法无效: {}", e))?;
    let url = request_url.trim();
    if url.is_empty() {
        return Err("cURL 缺少请求 URL".to_string());
    }

    let mut req = client.request(method, url);
    for (name_raw, value_raw) in request_headers {
        let name = name_raw.trim();
        let value = value_raw.trim();
        if name.is_empty() || value.is_empty() {
            continue;
        }
        if name.eq_ignore_ascii_case("host") || name.eq_ignore_ascii_case("content-length") {
            continue;
        }
        let header_name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|e| format!("cURL 请求头无效（{}）: {}", name, e))?;
        let header_value = reqwest::header::HeaderValue::from_str(value)
            .map_err(|e| format!("cURL 请求头值无效（{}）: {}", name, e))?;
        req = req.header(header_name, header_value);
    }

    if let Some(body) = request_body {
        if !body.trim().is_empty() {
            req = req.body(body.to_string());
        }
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("重放 cURL 请求 user resource 失败: {}", e))?;

    let status_code = resp.status();
    let location_header = resp
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_string());
    let final_url = resp.url().to_string();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取 user resource（cURL）响应失败: {}", e))?;

    if let Some(reason) = response_indicates_login_required(
        "user resource（cURL）",
        status_code,
        location_header.as_deref(),
        Some(final_url.as_str()),
        content_type.as_deref(),
        text.as_str(),
    ) {
        return Err(reason);
    }

    let text_snippet = {
        let preview = text.replace('\n', "\\n");
        let max_chars = 600usize;
        let mut out = String::new();
        let mut count = 0usize;
        for ch in preview.chars() {
            if count >= max_chars {
                out.push_str("...<truncated>");
                break;
            }
            out.push(ch);
            count += 1;
        }
        out
    };

    let result: Value = serde_json::from_str(&text).map_err(|e| {
        format!(
            "解析 user resource（cURL）响应失败: {}, http={}, body_preview={}",
            e,
            status_code.as_u16(),
            text_snippet
        )
    })?;

    if !status_code.is_success() {
        return Err(format!(
            "请求 user resource（cURL）失败: http={}, body={}",
            status_code.as_u16(),
            text
        ));
    }

    let code = result.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
    if code != 0 && code != 200 {
        let msg = result
            .get("msg")
            .or_else(|| result.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        return Err(format!(
            "user resource（cURL）返回失败: code={}, msg={}",
            code, msg
        ));
    }

    Ok(result)
}

async fn refresh_payload_for_account_inner(
    account: &crate::models::codebuddy::CodebuddyAccount,
    require_user_resource: bool,
) -> Result<CodebuddyOAuthCompletePayload, String> {
    let mut new_access_token = account.access_token.clone();
    let mut new_refresh_token = account.refresh_token.clone();
    let mut new_expires_at = account.expires_at;
    let mut new_domain = account.domain.clone();

    if let Some(refresh_tk) = account.refresh_token.as_deref() {
        match refresh_token(&account.access_token, refresh_tk, account.domain.as_deref()).await {
            Ok(token_data) => {
                new_access_token = token_data
                    .get("accessToken")
                    .or_else(|| token_data.get("access_token"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(&account.access_token)
                    .to_string();

                new_refresh_token = token_data
                    .get("refreshToken")
                    .or_else(|| token_data.get("refresh_token"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| account.refresh_token.clone());

                new_expires_at = token_data
                    .get("expiresAt")
                    .or_else(|| token_data.get("expires_at"))
                    .and_then(|v| v.as_i64())
                    .or(account.expires_at);

                new_domain = token_data
                    .get("domain")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| account.domain.clone());
            }
            Err(e) => {
                logger::log_warn(&format!(
                    "[CodeBuddy] Token 刷新失败，将使用现有 token 查询配额: {}",
                    e
                ));
            }
        }
    }

    let mut resolved_email =
        normalize_non_empty(Some(account.email.as_str())).unwrap_or_else(|| account.email.clone());
    let mut resolved_uid = account.uid.clone();
    let mut resolved_nickname = account.nickname.clone();
    let mut resolved_enterprise_id = account.enterprise_id.clone();
    let mut resolved_enterprise_name = account.enterprise_name.clone();
    let mut resolved_profile_raw = account.profile_raw.clone();
    let mut resolved_account_type = extract_profile_account_type(account.profile_raw.as_ref());
    let mut resolved_uin = extract_profile_uin(account.profile_raw.as_ref());

    let mut refreshed_quota_binding = account.quota_binding.clone();
    if let Some(binding) = refreshed_quota_binding.as_mut() {
        let console_cookie_header = binding.cookie_header.clone();
        let console_request_headers = binding.request_headers.clone();
        let console_user_agent = binding.user_agent.clone();
        match fetch_console_accounts_with_cookie(
            console_cookie_header.as_str(),
            console_request_headers.as_ref(),
            console_user_agent.as_deref(),
        )
        .await
        {
            Ok(outcome) => match outcome {
                ConsoleAccountsFetchOutcome::Success {
                    payload: console_payload,
                    refreshed_cookie,
                } => {
                    if let Some(new_cookie_header) = refreshed_cookie {
                        binding.cookie_header = new_cookie_header;
                        binding.updated_at = chrono::Utc::now().timestamp_millis();
                    }
                    if let Some(console_context) = parse_console_account_context(
                        &console_payload,
                        resolved_uid.as_deref(),
                        resolved_enterprise_id.as_deref(),
                    ) {
                        if let Some(uid) = console_context.uid {
                            resolved_uid = Some(uid);
                        }
                        if let Some(uin) = console_context.uin {
                            resolved_uin = Some(uin);
                        }
                        if let Some(account_type) = console_context.account_type {
                            resolved_account_type = Some(account_type);
                        }
                        if let Some(nickname) = console_context.nickname {
                            resolved_nickname = Some(nickname);
                        }
                        if let Some(email) = console_context.email {
                            resolved_email = email;
                        }
                        if let Some(enterprise_id) = console_context.enterprise_id {
                            resolved_enterprise_id = Some(enterprise_id);
                        }
                        if let Some(enterprise_name) = console_context.enterprise_name {
                            resolved_enterprise_name = Some(enterprise_name);
                        }
                        if let Some(profile_raw) = console_context.profile_raw {
                            resolved_profile_raw = Some(profile_raw);
                        }
                        logger::log_info(&format!(
                            "[CodeBuddy] /console/accounts 同步身份成功: uid={}, uin_present={}, type={}",
                            resolved_uid.clone().unwrap_or_default(),
                            resolved_uin
                                .as_deref()
                                .map(|v| !v.trim().is_empty())
                                .unwrap_or(false),
                            resolved_account_type.clone().unwrap_or_default()
                        ));
                    } else {
                        logger::log_warn(
                            "[CodeBuddy] /console/accounts 未解析到可用账号，继续使用本地缓存身份信息",
                        );
                    }
                }
                ConsoleAccountsFetchOutcome::NeedLogin {
                    reason,
                    refreshed_cookie,
                } => {
                    if let Some(new_cookie_header) = refreshed_cookie {
                        binding.cookie_header = new_cookie_header;
                        binding.updated_at = chrono::Utc::now().timestamp_millis();
                    }
                    return Err(format!("使用绑定参数刷新 user_resource 失败: {}", reason));
                }
            },
            Err(err) => {
                logger::log_warn(&format!(
                    "[CodeBuddy] /console/accounts 同步身份失败，继续使用本地缓存身份信息: {}",
                    err
                ));
            }
        }
    }

    let dosage = fetch_dosage_notify(
        &new_access_token,
        resolved_uid.as_deref(),
        resolved_enterprise_id.as_deref(),
        new_domain.as_deref(),
    )
    .await
    .ok();

    let payment = fetch_payment_type(
        &new_access_token,
        resolved_uid.as_deref(),
        resolved_enterprise_id.as_deref(),
        new_domain.as_deref(),
    )
    .await
    .ok();

    let is_personal = if resolved_account_type.is_some() {
        is_personal_account_type(resolved_account_type.as_deref())
    } else {
        resolved_enterprise_id
            .as_deref()
            .map(|v| v.trim().is_empty())
            .unwrap_or(true)
    };
    let uin_missing = resolved_uin
        .as_deref()
        .map(|v| v.trim().is_empty())
        .unwrap_or(true);
    let should_register_prewarm = resolved_uid
        .as_deref()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
        && is_personal
        && uin_missing;

    let user_resource = if let Some(binding) = refreshed_quota_binding.as_mut() {
        let cookie_header = binding.cookie_header.clone();
        let product_code = normalize_product_code(Some(binding.product_code.as_str()));
        let status = normalize_user_resource_status(binding.status.as_slice());
        let (package_end_time_range_begin, package_end_time_range_end) =
            build_default_user_resource_time_range();
        let page_number = 1;
        let page_size = 100;
        let request_headers = binding.request_headers.clone();
        let user_agent = binding.user_agent.clone();

        match fetch_user_resource_with_cookie_retry_register(
            resolved_uid.as_deref(),
            should_register_prewarm,
            cookie_header.as_str(),
            product_code.as_str(),
            &status,
            package_end_time_range_begin.as_str(),
            package_end_time_range_end.as_str(),
            page_number,
            page_size,
            request_headers.as_ref(),
            user_agent.as_deref(),
        )
        .await
        {
            Ok((payload, refreshed_cookie)) => {
                if let Some(new_cookie_header) = refreshed_cookie {
                    binding.cookie_header = new_cookie_header;
                }
                binding.product_code = product_code;
                binding.status = status;
                binding.package_end_time_range_begin = package_end_time_range_begin;
                binding.package_end_time_range_end = package_end_time_range_end;
                binding.page_number = page_number;
                binding.page_size = page_size;
                binding.updated_at = chrono::Utc::now().timestamp_millis();
                Some(payload)
            }
            Err(err) => {
                if require_user_resource {
                    return Err(format!("使用绑定参数刷新 user_resource 失败: {}", err));
                }
                logger::log_warn(&format!(
                    "[CodeBuddy] 使用绑定参数刷新 user_resource 失败: {}",
                    err
                ));
                None
            }
        }
    } else {
        if require_user_resource {
            return Err(
                "未配置查询配额绑定参数，无法刷新 user_resource（需要 session/session_2）"
                    .to_string(),
            );
        }
        logger::log_warn(
            "[CodeBuddy] 未配置查询配额绑定参数，跳过 user_resource 刷新（请先完成一次配额绑定）",
        );
        None
    };

    let dosage_data = dosage.as_ref().and_then(|v| v.get("data"));
    let dosage_notify_code = dosage_data
        .and_then(|d| d.get("dosageNotifyCode"))
        .map(|v| match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            _ => v.to_string(),
        });
    let dosage_notify_zh = dosage_data
        .and_then(|d| d.get("dosageNotifyZh"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let dosage_notify_en = dosage_data
        .and_then(|d| d.get("dosageNotifyEn"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let payment_data = payment.as_ref().and_then(|v| v.get("data"));
    let payment_type = payment_data
        .and_then(|d| {
            d.as_str().map(|s| s.to_string()).or_else(|| {
                d.get("paymentType")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
        })
        .or_else(|| account.payment_type.clone());

    let mut combined_quota = serde_json::Map::new();
    if let Some(d) = &dosage {
        combined_quota.insert("dosage".to_string(), d.clone());
    }
    if let Some(p) = &payment {
        combined_quota.insert("payment".to_string(), p.clone());
    }
    if let Some(r) = &user_resource {
        combined_quota.insert("userResource".to_string(), r.clone());
    }

    let quota_raw = if combined_quota.is_empty() {
        account.quota_raw.clone()
    } else {
        Some(Value::Object(combined_quota))
    };

    let final_email =
        normalize_non_empty(Some(resolved_email.as_str())).unwrap_or_else(|| account.email.clone());

    Ok(CodebuddyOAuthCompletePayload {
        email: final_email,
        uid: resolved_uid,
        nickname: resolved_nickname,
        enterprise_id: resolved_enterprise_id,
        enterprise_name: resolved_enterprise_name,
        access_token: new_access_token,
        refresh_token: new_refresh_token,
        token_type: account.token_type.clone(),
        expires_at: new_expires_at,
        domain: new_domain,
        plan_type: account.plan_type.clone(),
        dosage_notify_code,
        dosage_notify_zh,
        dosage_notify_en,
        payment_type,
        quota_raw,
        auth_raw: account.auth_raw.clone(),
        profile_raw: resolved_profile_raw,
        usage_raw: user_resource.or_else(|| account.usage_raw.clone()),
        quota_binding: refreshed_quota_binding,
        status: account.status.clone(),
        status_reason: account.status_reason.clone(),
    })
}

pub async fn refresh_payload_for_account(
    account: &crate::models::codebuddy::CodebuddyAccount,
) -> Result<CodebuddyOAuthCompletePayload, String> {
    refresh_payload_for_account_inner(account, false).await
}

pub async fn refresh_payload_for_account_strict(
    account: &crate::models::codebuddy::CodebuddyAccount,
) -> Result<CodebuddyOAuthCompletePayload, String> {
    refresh_payload_for_account_inner(account, true).await
}

pub async fn build_payload_from_token(
    access_token: &str,
) -> Result<CodebuddyOAuthCompletePayload, String> {
    let client = build_client()?;

    let url = format!(
        "{}{}/accounts",
        CODEBUDDY_API_ENDPOINT, CODEBUDDY_API_PREFIX
    );

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .send()
        .await
        .map_err(|e| format!("请求 accounts 失败: {}", e))?;

    let body: Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 accounts 响应失败: {}", e))?;

    let accounts = body
        .get("data")
        .and_then(|d| d.get("accounts"))
        .and_then(|a| a.as_array());

    let account_data = accounts
        .and_then(|arr| {
            arr.iter().find(|a| {
                a.get("lastLogin")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            })
        })
        .or_else(|| accounts.and_then(|arr| arr.first()))
        .cloned()
        .unwrap_or(json!({}));

    let uid = account_data
        .get("uid")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let nickname = account_data
        .get("nickname")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let email = account_data
        .get("email")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let enterprise_id = account_data
        .get("enterpriseId")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let enterprise_name = account_data
        .get("enterpriseName")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let email_final = if email.is_empty() {
        nickname
            .clone()
            .or_else(|| uid.clone())
            .unwrap_or_else(|| "unknown".to_string())
    } else {
        email
    };

    Ok(CodebuddyOAuthCompletePayload {
        email: email_final,
        uid,
        nickname,
        enterprise_id,
        enterprise_name,
        access_token: access_token.to_string(),
        refresh_token: None,
        token_type: Some("Bearer".to_string()),
        expires_at: None,
        domain: None,
        plan_type: None,
        dosage_notify_code: None,
        dosage_notify_zh: None,
        dosage_notify_en: None,
        payment_type: None,
        quota_raw: None,
        auth_raw: None,
        profile_raw: Some(account_data),
        usage_raw: None,
        quota_binding: None,
        status: Some("normal".to_string()),
        status_reason: None,
    })
}

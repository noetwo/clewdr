use std::{
    sync::LazyLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use axum_auth::AuthBearer;
use moka::sync::Cache;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{error, info, warn};
use wreq::StatusCode;

use super::error::ApiError;
use crate::{
    VERSION_INFO,
    claude_code_state::ClaudeCodeState,
    claude_web_state::ClaudeWebState,
    config::{CLEWDR_CONFIG, CookieStatus},
    services::cookie_actor::CookieActorHandle,
    types::model::AvailableModel,
    types::usage::{QuotaSummary, extract_quota_summary},
};

/// Cache entry for cookie status responses
#[derive(Clone)]
struct CookieStatusCache {
    data: Value,
    timestamp: u64,
}

/// Query parameters for cookie status endpoint
#[derive(Deserialize)]
pub struct CookieStatusQuery {
    #[serde(default)]
    refresh: bool,
}

/// Global cache for cookie status responses (TTL: 5 minutes)
static COOKIES_CACHE: LazyLock<Cache<String, CookieStatusCache>> = LazyLock::new(|| {
    Cache::builder()
        .max_capacity(1)
        .time_to_live(Duration::from_secs(300)) // 5 minutes
        .build()
});

/// Cache key for cookie status
const COOKIE_STATUS_CACHE_KEY: &str = "all_cookies";

/// API endpoint to submit a new cookie
/// Validates and adds the cookie to the cookie manager
///
/// # Arguments
/// * `s` - Application state containing event sender
/// * `t` - Auth bearer token for admin authentication
/// * `c` - Cookie status to be submitted
///
/// # Returns
/// * `StatusCode` - HTTP status code indicating success or failure
pub async fn api_post_cookie(
    State(s): State<CookieActorHandle>,
    AuthBearer(t): AuthBearer,
    Json(mut c): Json<CookieStatus>,
) -> Result<StatusCode, ApiError> {
    if !CLEWDR_CONFIG.load().admin_auth(&t) {
        return Err(ApiError::unauthorized());
    }
    c.reset_time = None;
    info!("Cookie accepted: {}", c.cookie);
    match s.submit(c).await {
        Ok(_) => {
            info!("Cookie submitted successfully");
            // Clear cache to ensure fresh data on next request
            COOKIES_CACHE.invalidate(COOKIE_STATUS_CACHE_KEY);
            MODELS_CACHE.invalidate_all();
            info!("Cookie status cache invalidated after adding new cookie");
            Ok(StatusCode::OK)
        }
        Err(e) => {
            error!("Failed to submit cookie: {}", e);
            Err(ApiError::internal(format!(
                "Failed to submit cookie: {}",
                e
            )))
        }
    }
}

/// API endpoint to retrieve all cookies and their status
/// Gets information about valid, exhausted, and invalid cookies
///
/// # Arguments
/// * `s` - Application state containing event sender
/// * `t` - Auth bearer token for admin authentication
/// * `query` - Query parameters including optional refresh flag
///
/// # Returns
/// * `Result<(HeaderMap, Json<Value>), ApiError>` - Response with cache headers and cookie status
pub async fn api_get_cookies(
    State(s): State<CookieActorHandle>,
    AuthBearer(t): AuthBearer,
    Query(query): Query<CookieStatusQuery>,
) -> Result<(HeaderMap, Json<Value>), ApiError> {
    if !CLEWDR_CONFIG.load().admin_auth(&t) {
        return Err(ApiError::unauthorized());
    }

    let mut headers = HeaderMap::new();

    // Check cache if not force refreshing
    if !query.refresh
        && let Some(cached) = COOKIES_CACHE.get(COOKIE_STATUS_CACHE_KEY)
    {
        headers.insert("X-Cache-Status", HeaderValue::from_static("HIT"));
        headers.insert(
            "X-Cache-Timestamp",
            HeaderValue::from_str(&cached.timestamp.to_string())
                .unwrap_or_else(|_| HeaderValue::from_static("0")),
        );
        info!("Cookie status served from cache");
        return Ok((headers, Json(cached.data)));
    }

    // Cache miss or force refresh - fetch fresh data
    match s.get_status().await {
        Ok(status) => {
            let valid = augment_utilization(status.valid, s.clone()).await;
            let exhausted = augment_utilization(status.exhausted, s.clone()).await;
            let invalid = status
                .invalid
                .into_iter()
                .map(|u| serde_json::to_value(u).unwrap_or(json!({})))
                .collect::<Vec<_>>();

            let response_data = json!({
                "valid": valid,
                "exhausted": exhausted,
                "invalid": invalid,
            });

            // Store in cache
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_else(|e| {
                    warn!("System time error: {}, using fallback timestamp", e);
                    Duration::from_secs(0)
                })
                .as_secs();

            COOKIES_CACHE.insert(
                COOKIE_STATUS_CACHE_KEY.to_string(),
                CookieStatusCache {
                    data: response_data.clone(),
                    timestamp,
                },
            );

            headers.insert("X-Cache-Status", HeaderValue::from_static("MISS"));
            headers.insert(
                "X-Cache-Timestamp",
                HeaderValue::from_str(&timestamp.to_string())
                    .unwrap_or_else(|_| HeaderValue::from_static("0")),
            );

            if query.refresh {
                info!("Cookie status force refreshed");
            } else {
                info!("Cookie status fetched and cached");
            }

            Ok((headers, Json(response_data)))
        }
        Err(e) => Err(ApiError::internal(format!(
            "Failed to get cookie status: {}",
            e
        ))),
    }
}

/// API endpoint to delete a specific cookie
/// Removes the cookie from all collections in the cookie manager
///
/// # Arguments
/// * `s` - Application state containing event sender
/// * `t` - Auth bearer token for admin authentication
/// * `c` - Cookie status to be deleted
///
/// # Returns
/// * `Result<StatusCode, (StatusCode, Json<serde_json::Value>)>` - Success status or error
pub async fn api_delete_cookie(
    State(s): State<CookieActorHandle>,
    AuthBearer(t): AuthBearer,
    Json(c): Json<CookieStatus>,
) -> Result<StatusCode, ApiError> {
    if !CLEWDR_CONFIG.load().admin_auth(&t) {
        return Err(ApiError::unauthorized());
    }

    match s.delete_cookie(c.to_owned()).await {
        Ok(_) => {
            info!("Cookie deleted successfully: {}", c.cookie);
            // Clear cache to ensure fresh data on next request
            COOKIES_CACHE.invalidate(COOKIE_STATUS_CACHE_KEY);
            MODELS_CACHE.invalidate_all();
            info!("Cookie status cache invalidated");
            Ok(StatusCode::NO_CONTENT)
        }
        Err(e) => {
            error!("Failed to delete cookie: {}", e);
            Err(ApiError::internal(format!(
                "Failed to delete cookie: {}",
                e
            )))
        }
    }
}

/// API endpoint to get the application version information
///
/// # Returns
/// * `String` - Version information string
pub async fn api_version() -> String {
    VERSION_INFO.to_string()
}

/// API endpoint to verify authentication
/// Checks if the provided token is valid for admin access
///
/// # Arguments
/// * `t` - Auth bearer token to verify
///
/// # Returns
/// * `StatusCode` - OK if authorized, UNAUTHORIZED otherwise
pub async fn api_auth(AuthBearer(t): AuthBearer) -> StatusCode {
    if !CLEWDR_CONFIG.load().admin_auth(&t) {
        return StatusCode::UNAUTHORIZED;
    }
    info!("Auth token accepted,");
    StatusCode::OK
}

const FALLBACK_MODEL_LIST: [&str; 7] = [
    "claude-fable-5",
    "claude-sonnet-5",
    "claude-opus-4-8",
    "claude-opus-4-7",
    "claude-opus-4-6",
    "claude-sonnet-4-6",
    "claude-haiku-4-5-20251001",
];

const WEB_MODELS_CACHE_KEY: &str = "web_models";
const CODE_MODELS_CACHE_KEY: &str = "code_models";

static MODELS_CACHE: LazyLock<Cache<String, Value>> = LazyLock::new(|| {
    Cache::builder()
        .max_capacity(2)
        .time_to_live(Duration::from_secs(300))
        .build()
});

fn fallback_models() -> Vec<AvailableModel> {
    FALLBACK_MODEL_LIST
        .iter()
        .map(|id| AvailableModel {
            id: (*id).to_string(),
            ..Default::default()
        })
        .collect()
}

fn models_response(mut models: Vec<AvailableModel>) -> Value {
    if models.is_empty() {
        models = fallback_models();
    }
    models.sort_by(|left, right| {
        left.overflow
            .cmp(&right.overflow)
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut seen = std::collections::HashSet::new();
    let mut data = Vec::new();
    for model in models {
        if !seen.insert(model.id.clone()) {
            continue;
        }
        data.push(model.openai_value());
    }

    json!({
        "object": "list",
        "data": data,
    })
}

async fn collect_web_models(handle: CookieActorHandle) -> Vec<AvailableModel> {
    let Ok(status) = handle.get_status().await else {
        return Vec::new();
    };
    let cookies = status.valid.into_iter().chain(status.exhausted);
    stream::iter(cookies.map(|cookie| {
        let handle = handle.clone();
        async move { ClaudeWebState::fetch_web_models(handle, cookie).await }
    }))
    .buffer_unordered(5)
    .filter_map(|models| async move { models })
    .flat_map(|models| stream::iter(models))
    .collect()
    .await
}

async fn collect_code_models(handle: CookieActorHandle) -> Vec<AvailableModel> {
    let Ok(status) = handle.get_status().await else {
        return Vec::new();
    };
    let cookies = status.valid.into_iter().chain(status.exhausted);
    stream::iter(cookies.map(|cookie| {
        let handle = handle.clone();
        async move {
            let Ok(mut state) = ClaudeCodeState::from_cookie(handle, cookie) else {
                return None;
            };
            let models = state.fetch_available_models().await.ok();
            state.return_cookie(None).await;
            models
        }
    }))
    .buffer_unordered(5)
    .filter_map(|models| async move { models })
    .flat_map(|models| stream::iter(models))
    .collect()
    .await
}

pub async fn api_get_web_models(State(handle): State<CookieActorHandle>) -> Json<Value> {
    if let Some(cached) = MODELS_CACHE.get(WEB_MODELS_CACHE_KEY) {
        return Json(cached);
    }
    let response = models_response(collect_web_models(handle).await);
    MODELS_CACHE.insert(WEB_MODELS_CACHE_KEY.to_string(), response.clone());
    Json(response)
}

pub async fn api_get_code_models(State(handle): State<CookieActorHandle>) -> Json<Value> {
    if let Some(cached) = MODELS_CACHE.get(CODE_MODELS_CACHE_KEY) {
        return Json(cached);
    }
    let mut models = collect_code_models(handle.clone()).await;
    if models.is_empty() {
        models = collect_web_models(handle).await;
    }
    let response = models_response(models);
    MODELS_CACHE.insert(CODE_MODELS_CACHE_KEY.to_string(), response.clone());
    Json(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_list_does_not_synthesize_thinking_aliases() {
        let response = models_response(vec![AvailableModel {
            id: "claude-opus-4-6".into(),
            ..Default::default()
        }]);
        let ids = response["data"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|model| model["id"].as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, vec!["claude-opus-4-6"]);
    }
}

// ------------------------------
// Ephemeral org usage enrichment
// ------------------------------
use futures::{StreamExt, TryFutureExt, stream};
use http::HeaderValue;

async fn augment_utilization(cookies: Vec<CookieStatus>, handle: CookieActorHandle) -> Vec<Value> {
    let concurrency = 5usize;
    stream::iter(cookies.into_iter().map(move |cookie| {
        let handle = handle.clone();
        async move {
            let base = serde_json::to_value(&cookie).unwrap_or(json!({}));
            match fetch_usage_percent(cookie, handle).await {
                Some(summary) => {
                    let mut obj = base;
                    obj["session_quota"] = json!(summary.session);
                    obj["weekly_quota"] = json!(summary.weekly);
                    obj["model_quota"] = json!(summary.model);
                    obj
                }
                None => base,
            }
        }
    }))
    .buffer_unordered(concurrency)
    .collect::<Vec<_>>()
    .await
}

async fn fetch_usage_percent(
    cookie: CookieStatus,
    handle: CookieActorHandle,
) -> Option<QuotaSummary> {
    let oauth_handle = handle.clone();
    let fallback_cookie = cookie.clone();
    let fallback_cookie_name = fallback_cookie.cookie.to_string();
    let usage = try_oauth_usage(&cookie, &oauth_handle)
        .or_else(|_| async move {
            info!(
                "OAuth usage unavailable for {}, trying web fallback",
                fallback_cookie_name
            );
            ClaudeWebState::fetch_web_usage(handle, fallback_cookie)
                .await
                .ok_or_else(|| {
                    warn!(
                        "Web usage fallback also failed for {}",
                        fallback_cookie_name
                    );
                })
        })
        .await
        .ok()?;

    extract_quota_summary(&usage)
}

/// Try the OAuth endpoint (`api.anthropic.com/api/oauth/usage`)
async fn try_oauth_usage(
    cookie: &CookieStatus,
    handle: &CookieActorHandle,
) -> Result<serde_json::Value, ()> {
    let Ok(mut state) = ClaudeCodeState::from_cookie(handle.clone(), cookie.clone()) else {
        warn!("try_oauth_usage: from_cookie failed for {}", cookie.cookie);
        return Err(());
    };
    let result = state.fetch_usage_metrics().await;
    state.return_cookie(None).await;
    result
        .inspect_err(|e| {
            warn!("try_oauth_usage: fetch failed for {}: {}", cookie.cookie, e);
        })
        .map_err(|_| ())
}

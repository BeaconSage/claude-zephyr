use hyper::{HeaderMap, Method};
use std::collections::HashSet;
use tracing::{debug, error, info, warn};

use crate::config::DetailLevel;

/// Log categories for better visual distinction
pub mod log_cat {
    pub const HEALTH: &str = "🏥";
    pub const PROXY: &str = "🔄";
    pub const SWITCH: &str = "🔀";
    pub const SERVER: &str = "🚀";
    pub const CONFIG: &str = "⚙️";
    pub const ERROR: &str = "❌";
    pub const SUCCESS: &str = "✅";
    pub const RETRY: &str = "🔁";
    #[allow(dead_code)]
    pub const PERFORMANCE: &str = "📊";
}

/// Security filtering for sensitive data
mod security {
    use super::*;

    /// Headers that should be filtered out for security
    const SENSITIVE_HEADERS: &[&str] = &[
        "authorization",
        "anthropic-api-key",
        "x-api-key",
        "cookie",
        "set-cookie",
        "x-auth-token",
        "bearer",
        "x-anthropic-api-key",
    ];

    /// Filter sensitive headers from HeaderMap
    pub fn filter_sensitive_headers(headers: &HeaderMap) -> Vec<(String, String)> {
        let sensitive_set: HashSet<String> =
            SENSITIVE_HEADERS.iter().map(|h| h.to_lowercase()).collect();

        headers
            .iter()
            .map(|(name, value)| {
                let name_lower = name.as_str().to_lowercase();
                if sensitive_set.contains(&name_lower) {
                    (name.as_str().to_string(), "[FILTERED]".to_string())
                } else if let Ok(value_str) = value.to_str() {
                    (name.as_str().to_string(), value_str.to_string())
                } else {
                    (name.as_str().to_string(), "[BINARY]".to_string())
                }
            })
            .collect()
    }

    /// Filter sensitive data from request/response body
    pub fn filter_sensitive_body(body: &[u8], max_size: usize) -> String {
        let body_str = if body.len() > max_size {
            format!("[TRUNCATED {} bytes]", body.len())
        } else if let Ok(s) = std::str::from_utf8(body) {
            // Basic filtering - replace potential API keys
            s.replace(|c: char| c.is_ascii_control() && c != '\n' && c != '\t', "")
                .lines()
                .map(|line| {
                    if line.contains("sk-") || line.contains("anthropic") || line.contains("token")
                    {
                        "[SENSITIVE_DATA_FILTERED]"
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            "[BINARY_DATA]".to_string()
        };

        if body_str.len() > max_size {
            format!(
                "{}...[TRUNCATED]",
                &body_str[..max_size.min(body_str.len())]
            )
        } else {
            body_str
        }
    }
}

/// Enhanced proxy request logging based on detail level
pub fn log_proxy_request_detailed(
    endpoint: &str,
    method: &Method,
    path: &str,
    connection_id: &str,
    headers: Option<&HeaderMap>,
    body: Option<&[u8]>,
    detail_level: &DetailLevel,
) {
    match detail_level {
        DetailLevel::Basic => {
            log_proxy_request(endpoint);
        }
        DetailLevel::Standard => {
            info!(
                "{} Request → {} {} {} (conn: {})",
                log_cat::PROXY,
                method,
                path,
                endpoint,
                connection_id
            );
        }
        DetailLevel::Detailed => {
            info!(
                "{} Request → {} {} {} (conn: {})",
                log_cat::PROXY,
                method,
                path,
                endpoint,
                connection_id
            );
            if let Some(headers) = headers {
                let filtered_headers = security::filter_sensitive_headers(headers);
                if !filtered_headers.is_empty() {
                    let headers_str = filtered_headers
                        .iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect::<Vec<_>>()
                        .join(", ");
                    info!("{}   Headers: {}", log_cat::PROXY, headers_str);
                }
            }
        }
        DetailLevel::Debug => {
            info!(
                "{} Request → {} {} {} (conn: {})",
                log_cat::PROXY,
                method,
                path,
                endpoint,
                connection_id
            );
            if let Some(headers) = headers {
                let filtered_headers = security::filter_sensitive_headers(headers);
                if !filtered_headers.is_empty() {
                    let headers_str = filtered_headers
                        .iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect::<Vec<_>>()
                        .join(", ");
                    debug!("{}   Headers: {}", log_cat::PROXY, headers_str);
                }
            }
            if let Some(body) = body {
                let filtered_body = security::filter_sensitive_body(body, 4096);
                if !filtered_body.is_empty() {
                    debug!("{}   Body: {}", log_cat::PROXY, filtered_body);
                }
            }
        }
    }
}

/// Enhanced proxy response logging based on detail level
pub fn log_proxy_response_detailed(
    endpoint: &str,
    status_code: u16,
    connection_id: &str,
    duration_ms: u64,
    headers: Option<&HeaderMap>,
    body: Option<&[u8]>,
    detail_level: &DetailLevel,
) {
    match detail_level {
        DetailLevel::Basic => {
            // Basic level doesn't log responses
        }
        DetailLevel::Standard => {
            info!(
                "{} Response ← {} {} ({}ms, conn: {})",
                log_cat::PROXY,
                status_code,
                endpoint,
                duration_ms,
                connection_id
            );
        }
        DetailLevel::Detailed => {
            info!(
                "{} Response ← {} {} ({}ms, conn: {})",
                log_cat::PROXY,
                status_code,
                endpoint,
                duration_ms,
                connection_id
            );
            if let Some(headers) = headers {
                let filtered_headers = security::filter_sensitive_headers(headers);
                if !filtered_headers.is_empty() {
                    let headers_str = filtered_headers
                        .iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect::<Vec<_>>()
                        .join(", ");
                    info!("{}   Headers: {}", log_cat::PROXY, headers_str);
                }
            }
        }
        DetailLevel::Debug => {
            info!(
                "{} Response ← {} {} ({}ms, conn: {})",
                log_cat::PROXY,
                status_code,
                endpoint,
                duration_ms,
                connection_id
            );
            if let Some(headers) = headers {
                let filtered_headers = security::filter_sensitive_headers(headers);
                if !filtered_headers.is_empty() {
                    let headers_str = filtered_headers
                        .iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect::<Vec<_>>()
                        .join(", ");
                    debug!("{}   Headers: {}", log_cat::PROXY, headers_str);
                }
            }
            if let Some(body) = body {
                let filtered_body = security::filter_sensitive_body(body, 4096);
                if !filtered_body.is_empty() {
                    debug!("{}   Body: {}", log_cat::PROXY, filtered_body);
                }
            }
        }
    }
}

/// Health check related logs
pub fn log_health_start(endpoint: &str) {
    info!("{} Health check starting: {}", log_cat::HEALTH, endpoint);
}

pub fn log_health_success(endpoint: &str, latency: u64) {
    info!(
        "{} {} Endpoint healthy: {} ({}ms)",
        log_cat::HEALTH,
        log_cat::SUCCESS,
        endpoint,
        latency
    );
}

pub fn log_health_failed(endpoint: &str, error: &str) {
    warn!(
        "{} {} Endpoint failed: {} - {}",
        log_cat::HEALTH,
        log_cat::ERROR,
        endpoint,
        error
    );
}

/// Proxy related logs
pub fn log_proxy_request(endpoint: &str) {
    info!("{} Request → {}", log_cat::PROXY, endpoint);
}

pub fn log_proxy_error(endpoint: &str, error: &str) {
    error!(
        "{} {} Request failed: {} - {}",
        log_cat::PROXY,
        log_cat::ERROR,
        endpoint,
        error
    );
}

/// Switch related logs
pub fn log_endpoint_switch(from: &str, to: &str, from_latency: u64, to_latency: u64) {
    info!("{} ⚡ SWITCHING ENDPOINT ⚡", log_cat::SWITCH);
    info!(
        "{} From: {} ({}ms) → To: {} ({}ms)",
        log_cat::SWITCH,
        from,
        from_latency,
        to,
        to_latency
    );
    info!(
        "{} ╰─ Performance improvement: {}ms",
        log_cat::SWITCH,
        from_latency.saturating_sub(to_latency)
    );
}

/// Server related logs
pub fn log_server_start(port: u16) {
    info!("{} ══════════════════════════════════", log_cat::SERVER);
    info!("{} 🎯 Claude Zephyr", log_cat::SERVER);
    info!("{} ⚡ Server: http://localhost:{}", log_cat::SERVER, port);
    info!(
        "{} 📊 Status: http://localhost:{}/status",
        log_cat::SERVER,
        port
    );
    info!(
        "{} 🔍 Health: http://localhost:{}/health",
        log_cat::SERVER,
        port
    );
    info!("{} ══════════════════════════════════", log_cat::SERVER);
}

pub fn log_server_error(error: &str) {
    error!(
        "{} {} Server error: {}",
        log_cat::SERVER,
        log_cat::ERROR,
        error
    );
}

/// Configuration related logs
pub fn log_config_loaded(endpoint_count: usize) {
    info!(
        "{} Configuration loaded: {} endpoints",
        log_cat::CONFIG,
        endpoint_count
    );
}

#[allow(dead_code)]
pub fn log_config_error(error: &str) {
    error!(
        "{} {} Configuration error: {}",
        log_cat::CONFIG,
        log_cat::ERROR,
        error
    );
}

/// Retry related logs
pub fn log_retry_attempt(endpoint: &str, attempt: u32, max_attempts: u32, reason: &str) {
    warn!(
        "{} Retry attempt {}/{} for {}: {}",
        log_cat::RETRY,
        attempt,
        max_attempts,
        endpoint,
        reason
    );
}

pub fn log_retry_success(endpoint: &str, attempt: u32, total_delay_ms: u64) {
    info!(
        "{} {} Retry succeeded for {} on attempt {} (total delay: {}ms)",
        log_cat::RETRY,
        log_cat::SUCCESS,
        endpoint,
        attempt,
        total_delay_ms
    );
}

pub fn log_retry_exhausted(endpoint: &str, max_attempts: u32, final_error: &str) {
    error!(
        "{} {} All {} retry attempts exhausted for {}: {}",
        log_cat::RETRY,
        log_cat::ERROR,
        max_attempts,
        endpoint,
        final_error
    );
}

pub fn log_retry_delay(endpoint: &str, attempt: u32, delay_ms: u64) {
    info!(
        "{} Waiting {}ms before retry attempt {} for {}",
        log_cat::RETRY,
        delay_ms,
        attempt,
        endpoint
    );
}

/// Performance related logs
#[allow(dead_code)]
pub fn log_request_start(endpoint: &str, connection_id: &str) {
    info!(
        "{} Request started: {} (connection: {})",
        log_cat::PERFORMANCE,
        endpoint,
        connection_id
    );
}

#[allow(dead_code)]
pub fn log_request_completed(endpoint: &str, connection_id: &str, duration_ms: u64, status: u16) {
    info!(
        "{} Request completed: {} (connection: {}, {}ms, status: {})",
        log_cat::PERFORMANCE,
        endpoint,
        connection_id,
        duration_ms,
        status
    );
}

#[allow(dead_code)]
pub fn log_request_failed(endpoint: &str, connection_id: &str, duration_ms: u64, error: &str) {
    warn!(
        "{} Request failed: {} (connection: {}, {}ms, error: {})",
        log_cat::PERFORMANCE,
        endpoint,
        connection_id,
        duration_ms,
        error
    );
}

// DEBUG MODULE REMOVED FOR SECURITY
//
// The debug module has been removed to prevent potential information leakage in production.
// Debug functions previously included:
// - log_health_check_details: Could expose endpoint URLs and configuration details
// - log_connection_count: Could reveal internal connection patterns
// - log_request_details: Could leak request paths and endpoint information
//
// For development debugging, consider using structured logging with appropriate filters
// or temporary debug prints that are removed before production deployment.

// Uncomment the following module only for development debugging:
/*
pub mod debug {
    use super::log_cat;
    use tracing::debug;

    #[allow(dead_code)]
    pub fn log_health_check_details(endpoint: &str, timeout: u64) {
        debug!(
            "{} Health check details: {} (timeout: {}s)",
            log_cat::HEALTH,
            endpoint,
            timeout
        );
    }

    #[allow(dead_code)]
    pub fn log_connection_count(endpoint: &str, count: u32) {
        debug!(
            "{} Active connections to {}: {}",
            log_cat::PROXY,
            endpoint,
            count
        );
    }

    #[allow(dead_code)]
    pub fn log_request_details(method: &str, path: &str, endpoint: &str) {
        debug!("{} {} {} → {}", log_cat::PROXY, method, path, endpoint);
    }
}
*/

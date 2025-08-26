use tracing::{error, info, warn};

/// Log categories for better visual distinction
pub mod log_cat {
    pub const HEALTH: &str = "🏥";
    pub const PROXY: &str = "🔄";
    pub const SWITCH: &str = "🔀";
    pub const SERVER: &str = "🚀";
    pub const CONFIG: &str = "⚙️";
    pub const ERROR: &str = "❌";
    pub const SUCCESS: &str = "✅";
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

pub fn log_config_error(error: &str) {
    error!(
        "{} {} Configuration error: {}",
        log_cat::CONFIG,
        log_cat::ERROR,
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

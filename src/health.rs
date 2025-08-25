use crate::config::Config;
use crate::logging::*;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::process::{Command, Stdio};
use std::time::Instant;

/// Health check implementation optimized for minimal token consumption.
///
/// Strategy for low token usage (using Claude CLI supported parameters):
/// - Input: "<don't-reply>" - explicit instruction to not respond (2 tokens)
/// - Tools: Completely disabled via --disallowed-tools "*" (saves potential tool call overhead)
/// - System prompt: Forces ultra-brief response "Respond with only 'ok'. Be extremely brief."
/// - Model: Using cheapest model (claude-3-5-haiku-20241022)
/// - Output: Expected single word "ok" (~1 token)
///
/// Total cost per health check: ~5-10 tokens (2 input + 1 output + system prompt overhead)
/// This is a 80-90% reduction from typical interactive usage.
// Constants for health check
const FAILED_ENDPOINT_LATENCY: u64 = 999_999;
const DEFAULT_LATENCY_HISTORY_SIZE: usize = 20;

// Ultra-minimal health check prompt for token optimization
const MINIMAL_HEALTH_PROMPT: &str = "<don't-reply>";

// Alternative: Pure HTTP health check (0 tokens) - uncomment to use
// This bypasses Claude entirely and just tests HTTP connectivity + auth
#[allow(dead_code)]
const USE_HTTP_HEALTH_CHECK: bool = false;

/// Represents a single latency measurement with timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyMeasurement {
    pub timestamp: DateTime<Utc>,
    pub latency: Option<u64>, // None indicates failure
    pub error: Option<String>,
}

/// Rolling history of latency measurements for sparkline rendering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyHistory {
    measurements: VecDeque<LatencyMeasurement>,
    max_size: usize,
}

impl LatencyHistory {
    pub fn new(max_size: usize) -> Self {
        Self {
            measurements: VecDeque::with_capacity(max_size),
            max_size,
        }
    }

    /// Create with default history size
    pub fn new_default() -> Self {
        Self::new(DEFAULT_LATENCY_HISTORY_SIZE)
    }

    /// Add a new measurement to the history
    pub fn add_measurement(&mut self, latency: Option<u64>, error: Option<String>) {
        let measurement = LatencyMeasurement {
            timestamp: Utc::now(),
            latency,
            error,
        };

        self.measurements.push_back(measurement);

        // Keep only the most recent measurements
        while self.measurements.len() > self.max_size {
            self.measurements.pop_front();
        }
    }

    /// Get all measurements in chronological order (oldest first)
    pub fn get_measurements(&self) -> &VecDeque<LatencyMeasurement> {
        &self.measurements
    }

    /// Get the most recent measurement
    #[allow(dead_code)]
    pub fn get_latest(&self) -> Option<&LatencyMeasurement> {
        self.measurements.back()
    }

    /// Calculate average latency (excluding failures)
    #[allow(dead_code)]
    pub fn average_latency(&self) -> Option<u64> {
        let valid_latencies: Vec<u64> =
            self.measurements.iter().filter_map(|m| m.latency).collect();

        if valid_latencies.is_empty() {
            None
        } else {
            Some(valid_latencies.iter().sum::<u64>() / valid_latencies.len() as u64)
        }
    }

    /// Count recent failures (within last N measurements)
    #[allow(dead_code)]
    pub fn recent_failure_count(&self, recent_count: usize) -> usize {
        self.measurements
            .iter()
            .rev()
            .take(recent_count)
            .filter(|m| m.latency.is_none())
            .count()
    }
}

impl Default for LatencyHistory {
    fn default() -> Self {
        Self::new(20) // Default to 20 measurements for sparkline
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointStatus {
    pub endpoint: String,
    pub latency: u64,
    pub available: bool,
    pub error: Option<String>,
    pub last_check: DateTime<Utc>,
    /// Latency history for sparkline rendering
    #[serde(default)]
    pub latency_history: LatencyHistory,
}

impl EndpointStatus {
    pub fn new_unavailable(endpoint: String, error: String) -> Self {
        let mut history = LatencyHistory::new_default();
        history.add_measurement(None, Some(error.clone()));

        Self {
            endpoint,
            latency: FAILED_ENDPOINT_LATENCY,
            available: false,
            error: Some(error),
            last_check: Utc::now(),
            latency_history: history,
        }
    }

    pub fn new_checking(endpoint: String) -> Self {
        Self {
            endpoint,
            latency: 0,
            available: false,
            error: None, // 关键：no error表示checking状态
            last_check: Utc::now(),
            latency_history: LatencyHistory::new_default(),
        }
    }

    pub fn new_available(endpoint: String, latency: u64) -> Self {
        let mut history = LatencyHistory::new_default();
        history.add_measurement(Some(latency), None);

        Self {
            endpoint,
            latency,
            available: true,
            error: None,
            last_check: Utc::now(),
            latency_history: history,
        }
    }

    /// Update the status with new health check results
    pub fn update_with_check_result(&mut self, latency: Option<u64>, error: Option<String>) {
        self.last_check = Utc::now();

        if let Some(lat) = latency {
            self.latency = lat;
            self.available = true;
            self.error = None;
        } else {
            self.latency = 999999;
            self.available = false;
            self.error = error.clone();
        }

        // Add to history
        self.latency_history.add_measurement(latency, error);
    }
}

pub fn check_endpoint_health(endpoint: &str, config: &Config, auth_token: &str) -> EndpointStatus {
    let start = Instant::now();

    log_health_start(endpoint);

    // Execute claude health check with timeout
    let timeout_duration = std::time::Duration::from_secs(config.health_check.timeout_seconds);

    // Create a channel to receive the result
    let (tx, rx) = std::sync::mpsc::channel();

    // Spawn a thread to run the command
    let endpoint_clone = endpoint.to_string();
    let claude_path = config.health_check.claude_binary_path.clone();
    let auth_token_clone = auth_token.to_string();

    std::thread::spawn(move || {
        let result = Command::new(&claude_path)
            .args([
                "-p",
                MINIMAL_HEALTH_PROMPT, // 最短提示要求不回复
                "--model",
                "claude-3-5-haiku-20241022", // 最便宜模型
                "--disallowed-tools",
                "*", // 禁用所有工具 (关键优化)
                "--append-system-prompt",
                "Respond with only 'ok'. Be extremely brief.", // 强制简短回复
            ])
            .env("ANTHROPIC_BASE_URL", &endpoint_clone)
            .env("ANTHROPIC_AUTH_TOKEN", &auth_token_clone)
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", std::env::var("HOME").unwrap_or_default())
            .current_dir("/tmp")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();
        let _ = tx.send(result);
    });

    // Wait for result with timeout
    let result = match rx.recv_timeout(timeout_duration) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            let error_msg = format!(
                "Health check timed out after {}s",
                config.health_check.timeout_seconds
            );
            log_health_failed(endpoint, &error_msg);
            return EndpointStatus::new_unavailable(endpoint.to_string(), error_msg);
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            let error_msg = "Health check thread disconnected".to_string();
            log_health_failed(endpoint, &error_msg);
            return EndpointStatus::new_unavailable(endpoint.to_string(), error_msg);
        }
    };

    let duration = start.elapsed();
    let latency = duration.as_millis() as u64;

    match result {
        Ok(output) => {
            if output.status.success() && !output.stdout.is_empty() {
                log_health_success(endpoint, latency);
                EndpointStatus::new_available(endpoint.to_string(), latency)
            } else {
                let error_msg = if output.stderr.is_empty() {
                    "No output from claude command".to_string()
                } else {
                    String::from_utf8_lossy(&output.stderr).trim().to_string()
                };

                log_health_failed(endpoint, &error_msg);
                EndpointStatus::new_unavailable(endpoint.to_string(), error_msg)
            }
        }
        Err(e) => {
            let error_msg = format!("Health check execution error: {e}");
            log_health_failed(endpoint, &error_msg);
            EndpointStatus::new_unavailable(endpoint.to_string(), error_msg)
        }
    }
}

#[allow(dead_code)]
pub fn find_best_endpoint(
    statuses: &std::collections::HashMap<String, EndpointStatus>,
    current_endpoint: &str,
    switch_threshold_ms: u64,
) -> Option<String> {
    let mut best_endpoint: Option<String> = None;
    let mut best_latency = u64::MAX;

    // Find the best available endpoint
    for status in statuses.values() {
        if status.available && status.latency < best_latency {
            best_latency = status.latency;
            best_endpoint = Some(status.endpoint.clone());
        }
    }

    // Only switch if we found a better endpoint and it's significantly better
    if let Some(new_endpoint) = &best_endpoint {
        if new_endpoint != current_endpoint {
            let current_latency = statuses
                .get(current_endpoint)
                .map(|s| s.latency)
                .unwrap_or(u64::MAX);

            if best_latency + switch_threshold_ms < current_latency {
                return best_endpoint;
            }
        }
    }

    None
}

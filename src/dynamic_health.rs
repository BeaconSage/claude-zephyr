use crate::config::Config;
use crate::connection_tracker::SharedConnectionTracker;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Load level classification for dynamic health check intervals
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoadLevel {
    Idle,   // 0 connections, system quiet
    Low,    // 1-3 connections
    Medium, // 4-10 connections
    High,   // >10 connections
}

/// Tracks load metrics for dynamic health check adjustment
#[derive(Debug)]
pub struct LoadMetrics {
    recent_requests: VecDeque<Instant>,
    current_load_level: LoadLevel,
    last_load_change: Instant,
}

impl LoadMetrics {
    pub fn new() -> Self {
        Self {
            recent_requests: VecDeque::new(),
            current_load_level: LoadLevel::Idle,
            last_load_change: Instant::now(),
        }
    }

    /// Update load metrics based on current connection tracker state
    pub fn update(&mut self, tracker: &SharedConnectionTracker) {
        let now = Instant::now();

        // Clean up old requests (keep last 5 minutes)
        let five_minutes_ago = now - Duration::from_secs(300);
        while let Some(&front_time) = self.recent_requests.front() {
            if front_time < five_minutes_ago {
                self.recent_requests.pop_front();
            } else {
                break;
            }
        }

        // Get current active connection count
        let active_count = if let Ok(tracker_guard) = tracker.lock() {
            tracker_guard.get_active_count()
        } else {
            0
        };

        // Determine new load level based on active connections
        // Since each connection typically represents a request, connection count is a good proxy for load
        // We still maintain the request rate capability for future enhancements
        let requests_last_minute = self.get_request_rate();
        let new_load_level = if active_count > 10 {
            // High load: Many concurrent connections (>10 indicates heavy usage)
            LoadLevel::High
        } else if active_count >= 4 {
            // Medium load: Moderate concurrent connections (4-10 indicates active usage)
            LoadLevel::Medium
        } else if active_count >= 1 {
            // Low load: Few connections (1-3 indicates light usage)
            LoadLevel::Low
        } else {
            // No active connections, check if we have recent explicit request records
            if requests_last_minute >= 10.0 {
                LoadLevel::Medium // High request rate but no current connections
            } else if requests_last_minute >= 2.0 {
                LoadLevel::Low // Some request activity
            } else {
                LoadLevel::Idle // Truly idle
            }
        };

        // Update if load level changed
        if new_load_level != self.current_load_level {
            self.current_load_level = new_load_level;
            self.last_load_change = now;
        }
    }

    /// Record a new request for load tracking (reserved for future use)
    #[allow(dead_code)]
    pub fn record_request(&mut self) {
        self.recent_requests.push_back(Instant::now());
    }

    /// Get current load level
    pub fn get_load_level(&self) -> LoadLevel {
        self.current_load_level
    }

    /// Get request rate (requests per minute)
    pub fn get_request_rate(&self) -> f64 {
        let now = Instant::now();
        let one_minute_ago = now - Duration::from_secs(60);

        let recent_count = self
            .recent_requests
            .iter()
            .filter(|&&time| time >= one_minute_ago)
            .count();

        recent_count as f64
    }
}

/// Dynamic health check interval calculator
pub struct DynamicHealthChecker {
    load_metrics: LoadMetrics,
    base_interval: Duration,
    min_interval: Duration,
    max_interval: Duration,
    dynamic_enabled: bool,
    #[allow(dead_code)]
    last_interval_change: Instant,
}

impl DynamicHealthChecker {
    pub fn new(config: &Config) -> Self {
        Self {
            load_metrics: LoadMetrics::new(),
            base_interval: config.health_check_interval(),
            min_interval: config.min_health_check_interval(),
            max_interval: config.max_health_check_interval(),
            dynamic_enabled: config.is_dynamic_scaling_enabled(),
            last_interval_change: Instant::now(),
        }
    }

    /// Calculate the optimal health check interval based on current load
    pub fn calculate_interval(&mut self, tracker: &SharedConnectionTracker) -> Duration {
        if !self.dynamic_enabled {
            return self.base_interval;
        }

        // Update load metrics
        self.load_metrics.update(tracker);

        let load_level = self.load_metrics.get_load_level();
        let request_rate = self.load_metrics.get_request_rate();

        // Calculate scaling factor based on load and request rate
        let scaling_factor = match load_level {
            LoadLevel::High => {
                // High load: use minimum interval directly (no scaling)
                return self.min_interval;
            }
            LoadLevel::Medium => {
                // Medium load: slight increase
                if request_rate > 5.0 {
                    1.2
                } else {
                    1.5
                }
            }
            LoadLevel::Low => {
                // Low load: moderate increase
                if request_rate > 2.0 {
                    2.0
                } else {
                    2.5
                }
            }
            LoadLevel::Idle => {
                // Idle: progressive scaling from base_interval to max_interval based on idle duration
                let idle_duration = Instant::now() - self.load_metrics.last_load_change;

                // Define idle duration thresholds for progressive scaling
                let idle_secs = idle_duration.as_secs();
                let base_secs = self.base_interval.as_secs() as f64;
                let max_secs = self.max_interval.as_secs() as f64;

                // Progressive scaling based on idle duration
                if idle_secs <= 60 {
                    // First minute: use base interval
                    1.0
                } else if idle_secs <= 300 {
                    // 1-5 minutes: gentle increase (base * 1.5)
                    1.0 + 0.5 * ((idle_secs - 60) as f64 / 240.0)
                } else if idle_secs <= 900 {
                    // 5-15 minutes: moderate increase (base * 1.5 to base * 3.0)
                    1.5 + 1.5 * ((idle_secs - 300) as f64 / 600.0)
                } else if idle_secs <= 1800 {
                    // 15-30 minutes: significant increase (base * 3.0 to base * 8.0)
                    3.0 + 5.0 * ((idle_secs - 900) as f64 / 900.0)
                } else {
                    // 30+ minutes: approach maximum interval
                    // Gradually approach max_secs/base_secs ratio
                    let max_scaling = max_secs / base_secs;
                    let progress = ((idle_secs - 1800) as f64 / 1800.0).min(1.0); // Cap at 1.0
                    8.0 + (max_scaling - 8.0) * progress
                }
            }
        };

        // Apply scaling factor
        let calculated_interval =
            Duration::from_secs((self.base_interval.as_secs() as f64 * scaling_factor) as u64);

        // Ensure interval respects configurable minimum and maximum bounds
        if calculated_interval < self.min_interval {
            self.min_interval
        } else if calculated_interval > self.max_interval {
            self.max_interval
        } else {
            calculated_interval
        }
    }

    /// Record a new request for load tracking (reserved for future use)
    #[allow(dead_code)]
    pub fn record_request(&mut self) {
        self.load_metrics.record_request();
    }

    /// Get current load level for debugging/monitoring
    pub fn get_load_level(&self) -> LoadLevel {
        self.load_metrics.get_load_level()
    }

    /// Get current request rate for debugging/monitoring  
    #[allow(dead_code)]
    pub fn get_request_rate(&self) -> f64 {
        self.load_metrics.get_request_rate()
    }
}

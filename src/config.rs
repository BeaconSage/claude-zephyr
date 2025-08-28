use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::Path;
use std::time::Duration;

use crate::i18n::Language;

/// Detail level for proxy request/response logging
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DetailLevel {
    /// Basic logging: endpoint URL only (current behavior)
    #[default]
    Basic,
    /// Standard logging: add method, path, status code, timing
    Standard,
    /// Detailed logging: add request/response headers
    Detailed,
    /// Debug logging: add request/response bodies (with security filtering)
    Debug,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    /// Modern group configuration with environment variable references
    pub groups: Vec<Group>,
    /// Global health check config (can be overridden per group)
    pub health_check: HealthCheckConfig,
    /// Retry configuration for proxy requests
    #[serde(default)]
    pub retry: RetryConfig,
    /// Logging configuration
    #[serde(default)]
    pub logging: LoggingConfig,
    /// UI and display settings
    #[serde(default)]
    pub ui: UiConfig,
}

/// Group of endpoints sharing the same auth token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    /// Group name for identification
    pub name: String,
    /// Environment variable name containing the auth token
    pub auth_token_env: String,
    /// Endpoints in this group (simplified format)
    pub endpoints: Vec<SimpleEndpoint>,
    /// Whether this is the default group
    #[serde(default)]
    pub default: Option<bool>,
    /// Optional group-specific health check settings
    #[serde(default)]
    pub health_check: Option<HealthCheckConfig>,
}

/// Individual endpoint configuration (legacy compatibility)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointConfig {
    /// Endpoint URL
    pub url: String,
    /// Display name for this endpoint
    pub name: String,
    /// Whether this endpoint is the default one
    #[serde(default)]
    pub default: Option<bool>,
}

/// Simplified endpoint configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleEndpoint {
    /// Endpoint URL
    pub url: String,
    /// Display name for this endpoint
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
    /// Minimum improvement in milliseconds to trigger endpoint switch
    #[serde(default = "default_switch_threshold")]
    pub switch_threshold_ms: u64,
    /// Maximum time to wait for graceful endpoint switch
    #[serde(default = "default_graceful_timeout")]
    pub graceful_switch_timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    /// Base health check interval in seconds
    pub interval_seconds: u64,
    /// Minimum health check interval (for dynamic scaling)
    #[serde(default)]
    pub min_interval_seconds: Option<u64>,
    /// Maximum health check interval (for dynamic scaling)
    #[serde(default)]
    pub max_interval_seconds: Option<u64>,
    /// Enable dynamic interval scaling based on connection load
    #[serde(default)]
    pub dynamic_scaling: bool,
    /// Timeout for each health check in seconds
    pub timeout_seconds: u64,
    /// Path to Claude CLI binary
    pub claude_binary_path: String,
}

/// Retry configuration for proxy requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Enable retry functionality
    #[serde(default = "default_retry_enabled")]
    pub enabled: bool,
    /// Maximum number of retry attempts (including initial attempt)
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    /// Base delay between retries in milliseconds
    #[serde(default = "default_base_delay_ms")]
    pub base_delay_ms: u64,
    /// Multiplier for exponential backoff
    #[serde(default = "default_backoff_multiplier")]
    pub backoff_multiplier: f32,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            enabled: default_retry_enabled(),
            max_attempts: default_max_attempts(),
            base_delay_ms: default_base_delay_ms(),
            backoff_multiplier: default_backoff_multiplier(),
        }
    }
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub level: String,
    /// Detail level for proxy request/response logging
    #[serde(default)]
    pub detail_level: DetailLevel,
    /// Enable console output
    #[serde(default = "default_console_enabled")]
    pub console_enabled: bool,
    /// Enable file output
    #[serde(default = "default_file_enabled")]
    pub file_enabled: bool,
    /// Log file path
    #[serde(default = "default_file_path")]
    pub file_path: String,
    /// Maximum log file size in bytes
    #[serde(default = "default_max_file_size")]
    pub max_file_size: u64,
    /// Maximum number of log files to keep
    #[serde(default = "default_max_files")]
    pub max_files: u32,
    /// Use JSON format for structured logging
    #[serde(default = "default_json_format")]
    pub json_format: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            detail_level: DetailLevel::default(),
            console_enabled: default_console_enabled(),
            file_enabled: default_file_enabled(),
            file_path: default_file_path(),
            max_file_size: default_max_file_size(),
            max_files: default_max_files(),
            json_format: default_json_format(),
        }
    }
}

/// UI configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UiConfig {
    /// Language setting for the interface
    #[serde(default)]
    pub language: Language,
}

// Default values
fn default_switch_threshold() -> u64 {
    50
}
fn default_graceful_timeout() -> u64 {
    30000
}
fn default_retry_enabled() -> bool {
    true
}
fn default_max_attempts() -> u32 {
    3
}
fn default_base_delay_ms() -> u64 {
    1000
}
fn default_backoff_multiplier() -> f32 {
    2.0
}
fn default_log_level() -> String {
    "info".to_string()
}
fn default_console_enabled() -> bool {
    true
}
fn default_file_enabled() -> bool {
    false
}
fn default_file_path() -> String {
    "logs/claude-zephyr.log".to_string()
}
fn default_max_file_size() -> u64 {
    104_857_600 // 100MB
}
fn default_max_files() -> u32 {
    10
}
fn default_json_format() -> bool {
    false
}

impl Config {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        // Load .env file if it exists
        if Path::new(".env").exists() {
            dotenv::dotenv().ok();
            println!("📋 Loading environment variables from .env file");
        }

        let content = fs::read_to_string(&path).map_err(|e| {
            anyhow::anyhow!(
                "❌ Failed to read config file: {}\n💡 Make sure the file exists and is readable",
                e
            )
        })?;

        let config: Config = toml::from_str(&content).map_err(|e| {
            anyhow::anyhow!("❌ Failed to parse config file: {}\n💡 Check your TOML syntax. Common issues:\n  • Missing quotes around strings\n  • Invalid endpoint_groups structure\n  • See config.toml.template for examples", e)
        })?;

        config.validate()?;
        Ok(config)
    }

    pub fn load_default() -> anyhow::Result<Self> {
        // Load .env file if it exists
        if Path::new(".env").exists() {
            dotenv::dotenv().ok();
            println!("📋 Loading environment variables from .env file");
        }

        let config_paths = ["config.toml", "config.toml.template"];

        for path in &config_paths {
            if Path::new(path).exists() {
                println!("📋 Loading configuration from: {path}");
                return Self::load_from_file(path);
            }
        }

        Err(anyhow::anyhow!(
            "❌ No configuration file found!\n\
             💡 Please create a config.toml file. You can:\n\
             🔧 Copy config.toml.template to config.toml\n\
             📝 Update the auth token and Claude binary path\n\
             ⚡ The old 'endpoints' array format is no longer supported"
        ))
    }

    /// Validate configuration - modern format only
    fn validate(&self) -> anyhow::Result<()> {
        // Ensure we have at least one group
        if self.groups.is_empty() {
            return Err(anyhow::anyhow!(
                "❌ No endpoint groups configured!\n\
                 💡 Please use the modern groups format in your config.toml.\n\
                 📖 See config.toml.example for examples.\n\
                 🔗 Copy config.toml.example to config.toml and update AUTH_TOKEN in .env"
            ));
        }

        // Validate each group
        for group in &self.groups {
            if group.endpoints.is_empty() {
                return Err(anyhow::anyhow!(
                    "❌ Group '{}' has no endpoints configured",
                    group.name
                ));
            }

            if group.auth_token_env.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "❌ Group '{}' is missing auth_token_env reference",
                    group.name
                ));
            }

            // Check if the environment variable exists
            if env::var(&group.auth_token_env).is_err() {
                return Err(anyhow::anyhow!(
                    "❌ Environment variable '{}' for group '{}' is not set.\n💡 Please check your .env file or set the environment variable",
                    group.auth_token_env, group.name
                ));
            }

            // Get the actual token value to validate it
            let token_value = env::var(&group.auth_token_env).unwrap_or_default();
            if token_value.contains("your-claude-auth-token-here")
                || token_value.contains("your-anthropic-auth-token-here")
            {
                return Err(anyhow::anyhow!(
                    "❌ Please replace the placeholder auth token in '{}' environment variable with your real Claude auth token", 
                    group.auth_token_env
                ));
            }
        }

        // Validate that we have at least one default group
        let has_default = self
            .groups
            .iter()
            .any(|group| group.default.unwrap_or(false));

        if !has_default {
            println!("⚠️  No default group specified, using first group as default");
        }

        // Validate unique endpoint names across all groups
        let mut names = std::collections::HashSet::new();
        for group in &self.groups {
            for endpoint in &group.endpoints {
                if !names.insert(&endpoint.name) {
                    return Err(anyhow::anyhow!(
                        "❌ Duplicate endpoint name '{}' found.\n💡 Each endpoint must have a unique name across all groups.", 
                        endpoint.name
                    ));
                }
            }
        }

        // Validate Claude binary paths
        for group in &self.groups {
            let health_config = group.health_check.as_ref().unwrap_or(&self.health_check);
            if !Path::new(&health_config.claude_binary_path).exists() {
                // Try to find claude in PATH if the specified path doesn't exist
                if std::process::Command::new(&health_config.claude_binary_path)
                    .arg("--version")
                    .output()
                    .is_err()
                {
                    return Err(anyhow::anyhow!(
                        "❌ Claude binary not found at: {}\n💡 Please install Claude CLI or update the path in your config",
                        health_config.claude_binary_path
                    ));
                }
            }
        }

        // Validate health check intervals
        self.validate_health_check_intervals()?;

        println!("✅ Configuration validated successfully!");
        println!(
            "🚀 Found {} groups with {} total endpoints",
            self.groups.len(),
            self.groups.iter().map(|g| g.endpoints.len()).sum::<usize>()
        );

        Ok(())
    }

    /// Validate health check interval constraints
    fn validate_health_check_intervals(&self) -> anyhow::Result<()> {
        // Check global health check config
        self.validate_single_health_check_config(&self.health_check, "global")?;

        // Check group-specific health check configs
        for group in &self.groups {
            if let Some(group_health_config) = &group.health_check {
                self.validate_single_health_check_config(
                    group_health_config,
                    &format!("group '{}'", group.name),
                )?;
            }
        }

        Ok(())
    }

    fn validate_single_health_check_config(
        &self,
        config: &HealthCheckConfig,
        context: &str,
    ) -> anyhow::Result<()> {
        // Validate basic intervals
        if config.interval_seconds == 0 {
            return Err(anyhow::anyhow!(
                "Health check interval cannot be 0 for {}",
                context
            ));
        }

        if config.timeout_seconds == 0 {
            return Err(anyhow::anyhow!(
                "Health check timeout cannot be 0 for {}",
                context
            ));
        }

        if config.timeout_seconds >= config.interval_seconds {
            return Err(anyhow::anyhow!(
                "Health check timeout ({}s) should be less than interval ({}s) for {}",
                config.timeout_seconds,
                config.interval_seconds,
                context
            ));
        }

        // Validate dynamic scaling settings if enabled
        if config.dynamic_scaling {
            if let Some(min_interval) = config.min_interval_seconds {
                if min_interval == 0 {
                    return Err(anyhow::anyhow!(
                        "Minimum interval cannot be 0 when dynamic scaling is enabled for {}",
                        context
                    ));
                }

                if min_interval > config.interval_seconds {
                    return Err(anyhow::anyhow!(
                        "Minimum interval ({}s) cannot be greater than base interval ({}s) for {}",
                        min_interval,
                        config.interval_seconds,
                        context
                    ));
                }

                if config.timeout_seconds >= min_interval {
                    return Err(anyhow::anyhow!(
                        "Health check timeout ({}s) should be less than minimum interval ({}s) for {}",
                        config.timeout_seconds, min_interval, context
                    ));
                }
            }

            if let Some(max_interval) = config.max_interval_seconds {
                if max_interval < config.interval_seconds {
                    return Err(anyhow::anyhow!(
                        "Maximum interval ({}s) cannot be less than base interval ({}s) for {}",
                        max_interval,
                        config.interval_seconds,
                        context
                    ));
                }
            }
        }

        Ok(())
    }

    pub fn health_check_interval(&self) -> Duration {
        Duration::from_secs(self.health_check.interval_seconds)
    }

    pub fn min_health_check_interval(&self) -> Duration {
        Duration::from_secs(self.health_check.min_interval_seconds.unwrap_or(30))
    }

    pub fn max_health_check_interval(&self) -> Duration {
        Duration::from_secs(
            self.health_check.max_interval_seconds.unwrap_or(3600), // Default to 1 hour - reasonable for idle periods
        )
    }

    pub fn is_dynamic_scaling_enabled(&self) -> bool {
        self.health_check.dynamic_scaling
    }

    /// Get the configured default group, if any
    pub fn get_default_group(&self) -> Option<&Group> {
        self.groups
            .iter()
            .find(|group| group.default.unwrap_or(false))
    }

    /// Get the configured default endpoint from default group, if any
    pub fn get_default_endpoint(&self) -> Option<(String, SimpleEndpoint)> {
        if let Some(default_group) = self.get_default_group() {
            if let Some(first_endpoint) = default_group.endpoints.first() {
                if let Ok(auth_token) = env::var(&default_group.auth_token_env) {
                    return Some((auth_token, first_endpoint.clone()));
                }
            }
        }
        None
    }

    /// Get all endpoints with their auth tokens and group names (legacy compatibility)
    /// Returns: Vec<(auth_token, endpoint_config, group_name)>
    pub fn get_all_endpoints_legacy(&self) -> Vec<(String, EndpointConfig, String)> {
        let mut all_endpoints = Vec::new();

        for group in &self.groups {
            if let Ok(auth_token) = env::var(&group.auth_token_env) {
                for endpoint in &group.endpoints {
                    all_endpoints.push((
                        auth_token.clone(),
                        endpoint.clone().into(), // Convert SimpleEndpoint to EndpointConfig
                        group.name.clone(),
                    ));
                }
            }
        }

        all_endpoints
    }

    /// Get all endpoints with their auth tokens and group names
    /// Returns: Vec<(auth_token, endpoint, group_name)>
    pub fn get_all_endpoints(&self) -> Vec<(String, SimpleEndpoint, String)> {
        let mut all_endpoints = Vec::new();

        for group in &self.groups {
            if let Ok(auth_token) = env::var(&group.auth_token_env) {
                for endpoint in &group.endpoints {
                    all_endpoints.push((auth_token.clone(), endpoint.clone(), group.name.clone()));
                }
            }
        }

        all_endpoints
    }
}

impl From<SimpleEndpoint> for EndpointConfig {
    fn from(simple: SimpleEndpoint) -> Self {
        EndpointConfig {
            url: simple.url,
            name: simple.name,
            default: None,
        }
    }
}

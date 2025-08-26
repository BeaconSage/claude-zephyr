use crate::dynamic_health::LoadLevel;
use crate::health::EndpointStatus;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Endpoint selection mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SelectionMode {
    /// Automatically select the fastest available endpoint
    #[default]
    Auto,
    /// Manually select endpoint by user choice
    Manual,
}

impl std::fmt::Display for SelectionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SelectionMode::Auto => write!(f, "AUTO"),
            SelectionMode::Manual => write!(f, "MANUAL"),
        }
    }
}

/// Events that can occur in the proxy system
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum ProxyEvent {
    /// A new connection has started
    ConnectionStarted(ActiveConnection),
    /// A connection has completed
    ConnectionCompleted(String), // connection_id
    /// A new request has been received (for load tracking)
    RequestReceived {
        endpoint: String,
        timestamp: std::time::Instant,
    },
    /// Load level has been recalculated (for health check interval adjustment)
    LoadLevelUpdated {
        load_level: LoadLevel,
        request_rate: f64,
        active_connections: u32,
    },
    /// Health check cycle started
    HealthCheckStarted {
        actual_interval: Duration,
        next_check_time: Instant, // 真正的下次检查时间
        load_level: LoadLevel,
        active_connections: u32,
    },
    /// Health check cycle is actively running
    HealthCheckRunning {
        started_at: Instant,
        estimated_duration: Duration,
    },
    /// Health check cycle completed
    HealthCheckCompleted { duration: Duration },
    /// Health check completed for an endpoint  
    HealthUpdate(EndpointStatus),
    /// Endpoint switch occurred
    EndpointSwitch {
        from: String,
        to: String,
        from_latency: u64,
        to_latency: u64,
    },
    /// Selection mode changed
    SelectionModeChanged { mode: SelectionMode },
    /// Manual endpoint selection
    ManualEndpointSelected {
        endpoint: String,
        endpoint_index: usize,
    },
    /// Server started
    ServerStarted { port: u16 },
    /// Configuration loaded
    ConfigLoaded { endpoint_count: usize },
    /// System health monitoring paused
    SystemPaused,
    /// System health monitoring resumed
    SystemResumed,
    /// Manual refresh/health check triggered
    ManualRefreshTriggered,
}

/// Represents an active connection being tracked
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveConnection {
    pub id: String,
    pub endpoint: String,
    pub start_time: DateTime<Utc>,
    pub status: ConnectionStatus,
    pub request_info: Option<RequestInfo>,
}

/// Status of an active connection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConnectionStatus {
    Connecting,
    Processing,
    Finishing,
}

/// Optional request information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestInfo {
    pub method: String,
    pub path: String,
    pub user_agent: Option<String>,
}

impl ActiveConnection {
    pub fn new(id: String, endpoint: String) -> Self {
        Self {
            id,
            endpoint,
            start_time: Utc::now(),
            status: ConnectionStatus::Connecting,
            request_info: None,
        }
    }

    pub fn duration(&self) -> u64 {
        let now = Utc::now();
        (now - self.start_time).num_milliseconds() as u64
    }

    pub fn update_status(&mut self, status: ConnectionStatus) {
        self.status = status;
    }
}

impl std::fmt::Display for ConnectionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionStatus::Connecting => write!(f, "Connecting..."),
            ConnectionStatus::Processing => write!(f, "Processing"),
            ConnectionStatus::Finishing => write!(f, "Finishing"),
        }
    }
}

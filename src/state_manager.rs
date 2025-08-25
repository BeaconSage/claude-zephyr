use crate::config::Config;
use crate::health::EndpointStatus;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;

/// Modern state machine for proxy state with optimized locking
#[derive(Debug, Clone)]
pub enum ProxyStateTransition {
    EndpointHealthUpdated {
        endpoint: String,
        status: EndpointStatus,
    },
    EndpointSwitched {
        from: String,
        to: String,
        reason: SwitchReason,
    },
    ConfigReloaded {
        config: Config,
    },
}

#[derive(Debug, Clone)]
pub enum SwitchReason {
    LatencyImprovement { improvement_ms: u64 },
    FailoverRecovery,
    ManualSwitch,
    InitialSelection,
}

/// Optimized proxy state with read-write lock separation
pub struct ProxyStateManager {
    /// Read-heavy data optimized with RwLock
    endpoint_status: Arc<RwLock<HashMap<String, EndpointStatus>>>,
    current_endpoint: Arc<RwLock<String>>,

    /// Configuration (rarely changes)
    config: Arc<RwLock<Config>>,

    /// State machine metadata
    last_switch_time: Arc<RwLock<Instant>>,
    switch_count: Arc<RwLock<u64>>,
    state_version: Arc<RwLock<u64>>,
}

impl ProxyStateManager {
    pub fn new(config: Config) -> Self {
        // Initialize current endpoint
        let current_endpoint = if let Some((_, default_endpoint)) = config.get_default_endpoint() {
            default_endpoint.url.clone()
        } else {
            config
                .get_all_endpoints()
                .first()
                .map(|(_, e, _)| e.url.clone())
                .unwrap_or_default()
        };

        // Initialize endpoint status map
        let mut endpoint_status = HashMap::new();
        for (_, endpoint_config, _) in config.get_all_endpoints() {
            endpoint_status.insert(
                endpoint_config.url.clone(),
                EndpointStatus::new_unavailable(
                    endpoint_config.url.clone(),
                    "Not checked yet".to_string(),
                ),
            );
        }

        Self {
            endpoint_status: Arc::new(RwLock::new(endpoint_status)),
            current_endpoint: Arc::new(RwLock::new(current_endpoint)),
            config: Arc::new(RwLock::new(config)),
            last_switch_time: Arc::new(RwLock::new(Instant::now())),
            switch_count: Arc::new(RwLock::new(0)),
            state_version: Arc::new(RwLock::new(1)),
        }
    }

    /// Fast read access to current endpoint (no contention)
    pub fn get_current_endpoint(&self) -> Result<String, StateError> {
        self.current_endpoint
            .read()
            .map_err(|_| StateError::LockPoisoned("current_endpoint"))
            .map(|guard| guard.clone())
    }

    /// Fast read access to endpoint status (concurrent safe)
    pub fn get_endpoint_status(
        &self,
        endpoint: &str,
    ) -> Result<Option<EndpointStatus>, StateError> {
        self.endpoint_status
            .read()
            .map_err(|_| StateError::LockPoisoned("endpoint_status"))
            .map(|guard| guard.get(endpoint).cloned())
    }

    /// Get all endpoint statuses (optimized for dashboard)
    pub fn get_all_endpoint_status(&self) -> Result<HashMap<String, EndpointStatus>, StateError> {
        self.endpoint_status
            .read()
            .map_err(|_| StateError::LockPoisoned("endpoint_status"))
            .map(|guard| guard.clone())
    }

    /// Atomic state transition with proper error handling
    pub fn apply_transition(&self, transition: ProxyStateTransition) -> Result<(), StateError> {
        match transition {
            ProxyStateTransition::EndpointHealthUpdated { endpoint, status } => {
                self.update_endpoint_health(endpoint, status)
            }
            ProxyStateTransition::EndpointSwitched {
                from: _,
                to,
                reason,
            } => self.switch_endpoint_atomic(to, reason),
            ProxyStateTransition::ConfigReloaded { config } => self.reload_config(config),
        }
    }

    /// Check if endpoint switch should happen based on latency threshold
    pub fn should_switch_endpoint(
        &self,
        new_endpoint: &str,
        new_latency: u64,
    ) -> Result<Option<SwitchDecision>, StateError> {
        let current_endpoint = self.get_current_endpoint()?;
        if current_endpoint.is_empty() {
            return Ok(Some(SwitchDecision {
                from: current_endpoint,
                to: new_endpoint.to_string(),
                reason: SwitchReason::InitialSelection,
                improvement_ms: 0,
            }));
        }

        if current_endpoint == new_endpoint {
            return Ok(None); // Same endpoint, no switch needed
        }

        let endpoint_status_guard = self
            .endpoint_status
            .read()
            .map_err(|_| StateError::LockPoisoned("endpoint_status"))?;

        let config_guard = self
            .config
            .read()
            .map_err(|_| StateError::LockPoisoned("config"))?;

        let threshold = config_guard.server.switch_threshold_ms;

        if let Some(current_status) = endpoint_status_guard.get(&current_endpoint) {
            if !current_status.available {
                // Current endpoint failed, switch immediately
                return Ok(Some(SwitchDecision {
                    from: current_endpoint,
                    to: new_endpoint.to_string(),
                    reason: SwitchReason::FailoverRecovery,
                    improvement_ms: current_status.latency.saturating_sub(new_latency),
                }));
            }

            let improvement = current_status.latency.saturating_sub(new_latency);
            if improvement >= threshold {
                return Ok(Some(SwitchDecision {
                    from: current_endpoint,
                    to: new_endpoint.to_string(),
                    reason: SwitchReason::LatencyImprovement {
                        improvement_ms: improvement,
                    },
                    improvement_ms: improvement,
                }));
            }
        } else {
            // Current endpoint not found, switch
            return Ok(Some(SwitchDecision {
                from: current_endpoint,
                to: new_endpoint.to_string(),
                reason: SwitchReason::InitialSelection,
                improvement_ms: 999999_u64.saturating_sub(new_latency),
            }));
        }

        Ok(None)
    }

    /// Get state machine statistics for monitoring
    pub fn get_state_stats(&self) -> Result<StateStats, StateError> {
        Ok(StateStats {
            switch_count: *self
                .switch_count
                .read()
                .map_err(|_| StateError::LockPoisoned("switch_count"))?,
            last_switch_time: *self
                .last_switch_time
                .read()
                .map_err(|_| StateError::LockPoisoned("last_switch_time"))?,
            state_version: *self
                .state_version
                .read()
                .map_err(|_| StateError::LockPoisoned("state_version"))?,
            total_endpoints: {
                let status_guard = self
                    .endpoint_status
                    .read()
                    .map_err(|_| StateError::LockPoisoned("endpoint_status"))?;
                status_guard.len()
            },
        })
    }

    // Private implementation methods
    fn update_endpoint_health(
        &self,
        endpoint: String,
        status: EndpointStatus,
    ) -> Result<(), StateError> {
        let mut status_guard = self
            .endpoint_status
            .write()
            .map_err(|_| StateError::LockPoisoned("endpoint_status"))?;

        status_guard.insert(endpoint, status);
        self.increment_version()?;
        Ok(())
    }

    fn switch_endpoint_atomic(
        &self,
        new_endpoint: String,
        _reason: SwitchReason,
    ) -> Result<(), StateError> {
        {
            let mut current_guard = self
                .current_endpoint
                .write()
                .map_err(|_| StateError::LockPoisoned("current_endpoint"))?;

            if *current_guard != new_endpoint {
                *current_guard = new_endpoint;
            } else {
                return Ok(()); // No switch needed
            }
        }

        // Update metadata atomically
        {
            let mut switch_time_guard = self
                .last_switch_time
                .write()
                .map_err(|_| StateError::LockPoisoned("last_switch_time"))?;
            *switch_time_guard = Instant::now();
        }

        {
            let mut switch_count_guard = self
                .switch_count
                .write()
                .map_err(|_| StateError::LockPoisoned("switch_count"))?;
            *switch_count_guard += 1;
        }

        self.increment_version()?;
        Ok(())
    }

    fn reload_config(&self, new_config: Config) -> Result<(), StateError> {
        let mut config_guard = self
            .config
            .write()
            .map_err(|_| StateError::LockPoisoned("config"))?;

        *config_guard = new_config;
        self.increment_version()?;
        Ok(())
    }

    fn increment_version(&self) -> Result<(), StateError> {
        let mut version_guard = self
            .state_version
            .write()
            .map_err(|_| StateError::LockPoisoned("state_version"))?;
        *version_guard += 1;
        Ok(())
    }

    /// Get configuration (rarely accessed, safe to clone)
    pub fn get_config(&self) -> Result<Config, StateError> {
        self.config
            .read()
            .map_err(|_| StateError::LockPoisoned("config"))
            .map(|guard| guard.clone())
    }
}

#[derive(Debug)]
pub struct SwitchDecision {
    pub from: String,
    pub to: String,
    pub reason: SwitchReason,
    pub improvement_ms: u64,
}

#[derive(Debug)]
pub struct StateStats {
    pub switch_count: u64,
    pub last_switch_time: Instant,
    pub state_version: u64,
    pub total_endpoints: usize,
}

#[derive(Debug)]
pub enum StateError {
    LockPoisoned(&'static str),
    InvalidTransition(String),
    EndpointNotFound(String),
}

impl std::fmt::Display for StateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StateError::LockPoisoned(name) => write!(f, "Lock was poisoned: {name}"),
            StateError::InvalidTransition(msg) => write!(f, "Invalid state transition: {msg}"),
            StateError::EndpointNotFound(endpoint) => write!(f, "Endpoint not found: {endpoint}"),
        }
    }
}

impl std::error::Error for StateError {}

/// Shared type alias for the new state manager
pub type SharedStateManager = Arc<ProxyStateManager>;

use crate::health::EndpointStatus;
use crate::proxy::{ProxyState, SharedState};
use crate::state_manager::{
    ProxyStateManager, ProxyStateTransition, SharedStateManager, StateError, SwitchReason,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Migration adapter to gradually replace old Mutex-based system with RwLock-based state manager
pub struct MigrationAdapter {
    // Legacy system (will be phased out)
    legacy_state: Option<SharedState>,

    // New optimized system
    state_manager: SharedStateManager,

    // Migration mode flag
    migration_complete: bool,
}

impl MigrationAdapter {
    /// Create adapter with both systems running
    pub fn new_with_legacy(legacy_state: SharedState, state_manager: SharedStateManager) -> Self {
        Self {
            legacy_state: Some(legacy_state),
            state_manager,
            migration_complete: false,
        }
    }

    /// Create adapter with only new system (post-migration)
    pub fn new_optimized(state_manager: SharedStateManager) -> Self {
        Self {
            legacy_state: None,
            state_manager,
            migration_complete: true,
        }
    }

    /// Get current endpoint using optimized path or fallback to legacy
    pub fn get_current_endpoint(&self) -> Result<String, String> {
        if self.migration_complete {
            // Use optimized path
            self.state_manager
                .get_current_endpoint()
                .map_err(|e| format!("State manager error: {}", e))
        } else if let Some(ref legacy) = self.legacy_state {
            // Fallback to legacy system
            legacy
                .lock()
                .map_err(|_| "Legacy lock poisoned".to_string())
                .map(|guard| guard.current_endpoint.clone())
        } else {
            Err("No state system available".to_string())
        }
    }

    /// Update endpoint health in both systems during migration
    pub fn update_endpoint_health(
        &self,
        endpoint: String,
        status: EndpointStatus,
    ) -> Result<(), String> {
        let transition = ProxyStateTransition::EndpointHealthUpdated {
            endpoint: endpoint.clone(),
            status: status.clone(),
        };

        // Update new system
        self.state_manager
            .apply_transition(transition)
            .map_err(|e| format!("State manager error: {}", e))?;

        // Update legacy system if still present
        if !self.migration_complete {
            if let Some(ref legacy) = self.legacy_state {
                let mut guard = legacy
                    .lock()
                    .map_err(|_| "Legacy lock poisoned".to_string())?;

                guard.endpoint_status.insert(endpoint, status);
            }
        }

        Ok(())
    }

    /// Switch endpoint with improved logic
    pub fn switch_endpoint(&self, new_endpoint: String) -> Result<bool, String> {
        // Check if switch is needed using optimized logic
        let current_endpoint = self.get_current_endpoint()?;
        if current_endpoint == new_endpoint {
            return Ok(false); // No switch needed
        }

        let transition = ProxyStateTransition::EndpointSwitched {
            from: current_endpoint.clone(),
            to: new_endpoint.clone(),
            reason: SwitchReason::ManualSwitch,
        };

        // Apply to new system
        self.state_manager
            .apply_transition(transition)
            .map_err(|e| format!("State manager error: {}", e))?;

        // Update legacy system if still present
        if !self.migration_complete {
            if let Some(ref legacy) = self.legacy_state {
                let mut guard = legacy
                    .lock()
                    .map_err(|_| "Legacy lock poisoned".to_string())?;

                guard.current_endpoint = new_endpoint;
            }
        }

        Ok(true)
    }

    /// Get all endpoint status optimized
    pub fn get_all_endpoint_status(&self) -> Result<HashMap<String, EndpointStatus>, String> {
        if self.migration_complete {
            // Use optimized path
            self.state_manager
                .get_all_endpoint_status()
                .map_err(|e| format!("State manager error: {}", e))
        } else if let Some(ref legacy) = self.legacy_state {
            // Fallback to legacy
            legacy
                .lock()
                .map_err(|_| "Legacy lock poisoned".to_string())
                .map(|guard| guard.endpoint_status.clone())
        } else {
            Err("No state system available".to_string())
        }
    }

    /// Complete migration by dropping legacy system
    pub fn complete_migration(&mut self) -> Result<(), String> {
        if self.migration_complete {
            return Ok(()); // Already completed
        }

        // Sync final state from legacy to new system
        if let Some(ref legacy) = self.legacy_state {
            let guard = legacy
                .lock()
                .map_err(|_| "Legacy lock poisoned during migration".to_string())?;

            // Ensure current endpoint is synced
            let current = guard.current_endpoint.clone();
            if !current.is_empty() {
                let transition = ProxyStateTransition::EndpointSwitched {
                    from: String::new(),
                    to: current,
                    reason: SwitchReason::InitialSelection,
                };

                self.state_manager
                    .apply_transition(transition)
                    .map_err(|e| format!("Failed to sync current endpoint: {}", e))?;
            }

            // Sync all endpoint statuses
            for (endpoint, status) in guard.endpoint_status.iter() {
                let transition = ProxyStateTransition::EndpointHealthUpdated {
                    endpoint: endpoint.clone(),
                    status: status.clone(),
                };

                self.state_manager
                    .apply_transition(transition)
                    .map_err(|e| format!("Failed to sync endpoint status: {}", e))?;
            }
        }

        // Drop legacy system
        self.legacy_state = None;
        self.migration_complete = true;

        Ok(())
    }

    /// Check if endpoint switch should happen (optimized decision making)
    pub fn evaluate_endpoint_switch(
        &self,
        endpoint: &str,
        latency: u64,
    ) -> Result<Option<String>, String> {
        let decision = self
            .state_manager
            .should_switch_endpoint(endpoint, latency)
            .map_err(|e| format!("Switch evaluation error: {}", e))?;

        if let Some(switch_decision) = decision {
            Ok(Some(switch_decision.to))
        } else {
            Ok(None)
        }
    }

    /// Get state manager for advanced operations
    pub fn get_state_manager(&self) -> &SharedStateManager {
        &self.state_manager
    }

    /// Check if migration is complete
    pub fn is_migration_complete(&self) -> bool {
        self.migration_complete
    }

    /// Get performance statistics
    pub fn get_performance_stats(&self) -> Result<MigrationStats, String> {
        let state_stats = self
            .state_manager
            .get_state_stats()
            .map_err(|e| format!("Failed to get state stats: {}", e))?;

        Ok(MigrationStats {
            migration_complete: self.migration_complete,
            switch_count: state_stats.switch_count,
            total_endpoints: state_stats.total_endpoints,
            state_version: state_stats.state_version,
        })
    }
}

#[derive(Debug)]
pub struct MigrationStats {
    pub migration_complete: bool,
    pub switch_count: u64,
    pub total_endpoints: usize,
    pub state_version: u64,
}

/// Create migration adapter from legacy proxy state
pub fn create_migration_adapter(
    config: crate::config::Config,
    legacy_state: SharedState,
) -> MigrationAdapter {
    let state_manager = Arc::new(ProxyStateManager::new(config));
    MigrationAdapter::new_with_legacy(legacy_state, state_manager)
}

/// Create optimized adapter without legacy system
pub fn create_optimized_adapter(config: crate::config::Config) -> MigrationAdapter {
    let state_manager = Arc::new(ProxyStateManager::new(config));
    MigrationAdapter::new_optimized(state_manager)
}

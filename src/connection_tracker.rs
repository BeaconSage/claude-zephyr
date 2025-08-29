use crate::events::{ActiveConnection, ConnectionStatus};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tokio::sync::mpsc;

/// Global counter for unique connection IDs
static CONNECTION_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Diagnostic information about connection tracker state
#[derive(Debug, Clone)]
pub struct ConnectionDiagnostics {
    pub total_active: u32,
    pub endpoint_counts: HashMap<String, u32>,
    pub duration_stats: Vec<u64>,
    pub completed_count: u64,
    pub peak_concurrent: u32,
}

/// Tracks active connections and provides statistics
#[derive(Debug)]
pub struct ConnectionTracker {
    active: HashMap<String, ActiveConnection>,
    completed_count: u64,
    peak_concurrent: u32,
    endpoint_distribution: HashMap<String, u32>,
}

impl ConnectionTracker {
    pub fn new() -> Self {
        Self {
            active: HashMap::new(),
            completed_count: 0,
            peak_concurrent: 0,
            endpoint_distribution: HashMap::new(),
        }
    }

    pub fn start_connection(&mut self, id: String, endpoint: String) -> ActiveConnection {
        let connection = ActiveConnection::new(id.clone(), endpoint.clone());

        // Update statistics
        self.active.insert(id, connection.clone());
        *self.endpoint_distribution.entry(endpoint).or_insert(0) += 1;

        // Track peak concurrent connections
        if self.active.len() as u32 > self.peak_concurrent {
            self.peak_concurrent = self.active.len() as u32;
        }

        connection
    }

    pub fn update_connection_status(
        &mut self,
        id: &str,
        status: ConnectionStatus,
    ) -> Option<ActiveConnection> {
        if let Some(connection) = self.active.get_mut(id) {
            connection.update_status(status);
            Some(connection.clone())
        } else {
            None
        }
    }

    pub fn complete_connection(&mut self, id: &str) -> Option<ActiveConnection> {
        if let Some(connection) = self.active.remove(id) {
            self.completed_count += 1;

            // Update endpoint distribution
            if let Some(count) = self.endpoint_distribution.get_mut(&connection.endpoint) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    self.endpoint_distribution.remove(&connection.endpoint);
                }
            }

            Some(connection)
        } else {
            None
        }
    }

    pub fn get_active_connections(&self) -> &HashMap<String, ActiveConnection> {
        &self.active
    }

    pub fn get_active_count(&self) -> u32 {
        self.active.len() as u32
    }

    pub fn get_completed_count(&self) -> u64 {
        self.completed_count
    }

    pub fn get_peak_concurrent(&self) -> u32 {
        self.peak_concurrent
    }

    #[allow(dead_code)]
    pub fn get_endpoint_distribution(&self) -> &HashMap<String, u32> {
        &self.endpoint_distribution
    }

    #[allow(dead_code)]
    pub fn get_connections_for_endpoint(&self, endpoint: &str) -> Vec<&ActiveConnection> {
        self.active
            .values()
            .filter(|conn| conn.endpoint == endpoint)
            .collect()
    }

    /// Get diagnostic information about connections
    pub fn get_connection_diagnostics(&self) -> ConnectionDiagnostics {
        let current_time = chrono::Utc::now();
        let mut endpoint_counts = HashMap::new();
        let mut duration_stats = Vec::new();

        for conn in self.active.values() {
            // Count connections per endpoint
            *endpoint_counts.entry(conn.endpoint.clone()).or_insert(0) += 1;

            // Calculate connection duration
            let duration_seconds = (current_time - conn.start_time).num_seconds() as u64;
            duration_stats.push(duration_seconds);
        }

        ConnectionDiagnostics {
            total_active: self.active.len() as u32,
            endpoint_counts,
            duration_stats,
            completed_count: self.completed_count,
            peak_concurrent: self.peak_concurrent,
        }
    }

    /// Clean up connections for endpoints that are no longer active
    pub fn cleanup_orphaned_connections(&mut self, current_active_endpoint: &str) -> Vec<String> {
        let mut orphaned_connections = Vec::new();

        // Find connections to endpoints that don't match the current active endpoint
        let orphaned_ids: Vec<String> = self
            .active
            .iter()
            .filter(|(_, conn)| conn.endpoint != current_active_endpoint)
            .map(|(id, _)| id.clone())
            .collect();

        // Remove orphaned connections
        for id in orphaned_ids {
            if let Some(connection) = self.active.remove(&id) {
                // Update endpoint distribution
                if let Some(count) = self.endpoint_distribution.get_mut(&connection.endpoint) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        self.endpoint_distribution.remove(&connection.endpoint);
                    }
                }
                orphaned_connections.push(id);
            }
        }

        orphaned_connections
    }

    /// Force cleanup all active connections (for shutdown scenarios)
    pub fn force_cleanup_all_connections(&mut self) -> Vec<String> {
        let connection_ids: Vec<String> = self.active.keys().cloned().collect();

        for id in &connection_ids {
            self.active.remove(id);
        }

        // Clear endpoint distribution
        self.endpoint_distribution.clear();

        connection_ids
    }

    /// Check for connections that should be considered abandoned (longer idle time)
    pub fn cleanup_abandoned_connections(&mut self, max_idle_seconds: u64) -> Vec<String> {
        let current_time = chrono::Utc::now();
        let mut abandoned = Vec::new();

        let abandoned_ids: Vec<String> = self
            .active
            .iter()
            .filter(|(_, conn)| {
                let idle_seconds = (current_time - conn.start_time).num_seconds() as u64;
                idle_seconds > max_idle_seconds
            })
            .map(|(id, _)| id.clone())
            .collect();

        for id in abandoned_ids {
            if let Some(connection) = self.active.remove(&id) {
                // Update endpoint distribution
                if let Some(count) = self.endpoint_distribution.get_mut(&connection.endpoint) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        self.endpoint_distribution.remove(&connection.endpoint);
                    }
                }
                abandoned.push(id);
            }
        }

        abandoned
    }

    /// Clean up connections that have been running for too long (safety mechanism)
    pub fn cleanup_stale_connections(&mut self, max_duration_seconds: u64) -> Vec<String> {
        let mut stale_connections = Vec::new();
        let current_time = chrono::Utc::now();

        // Find connections that have been running longer than max_duration_seconds
        let stale_ids: Vec<String> = self
            .active
            .iter()
            .filter(|(_, conn)| {
                let duration_seconds = (current_time - conn.start_time).num_seconds() as u64;
                duration_seconds > max_duration_seconds
            })
            .map(|(id, _)| id.clone())
            .collect();

        // Remove stale connections
        for id in stale_ids {
            if let Some(connection) = self.active.remove(&id) {
                // Update endpoint distribution
                if let Some(count) = self.endpoint_distribution.get_mut(&connection.endpoint) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        self.endpoint_distribution.remove(&connection.endpoint);
                    }
                }
                stale_connections.push(id);
            }
        }

        stale_connections
    }
}

/// Thread-safe wrapper for ConnectionTracker
pub type SharedConnectionTracker = Arc<Mutex<ConnectionTracker>>;

/// Utility to generate unique connection IDs
pub fn generate_connection_id() -> String {
    let counter = CONNECTION_COUNTER.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_else(|_| {
            // Fallback to a default timestamp if system time is invalid
            std::time::Duration::from_secs(1700000000) // Some reasonable epoch time
        })
        .as_millis();
    format!("req_{timestamp}_{counter}")
}

/// Event sender for dashboard communication
pub type EventSender = mpsc::UnboundedSender<crate::events::ProxyEvent>;
pub type EventReceiver = mpsc::UnboundedReceiver<crate::events::ProxyEvent>;

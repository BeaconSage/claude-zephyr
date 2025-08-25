use crate::events::{ActiveConnection, ConnectionStatus};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tokio::sync::mpsc;

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
    format!(
        "req_{}",
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    )
}

/// Event sender for dashboard communication
pub type EventSender = mpsc::UnboundedSender<crate::events::ProxyEvent>;
pub type EventReceiver = mpsc::UnboundedReceiver<crate::events::ProxyEvent>;

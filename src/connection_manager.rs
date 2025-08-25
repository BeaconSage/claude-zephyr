use crate::events::{ActiveConnection, ConnectionStatus};
use chrono::Utc;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Optimized connection tracking with RwLock for better concurrency
pub struct ConnectionManager {
    /// Active connections (read-heavy operations)
    active_connections: Arc<RwLock<HashMap<String, ActiveConnection>>>,

    /// Connection statistics (read-optimized)
    stats: Arc<RwLock<ConnectionStats>>,

    /// Endpoint distribution (read-heavy for dashboard)
    endpoint_distribution: Arc<RwLock<HashMap<String, u32>>>,
}

#[derive(Debug, Clone, Default)]
pub struct ConnectionStats {
    pub total_completed: u64,
    pub total_failed: u64,
    pub average_duration: Duration,
    pub peak_concurrent: u32,
    pub last_activity: Option<Instant>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            active_connections: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(ConnectionStats::default())),
            endpoint_distribution: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start tracking a new connection (write lock minimal scope)
    pub fn start_connection(
        &self,
        connection_id: String,
        endpoint: String,
    ) -> Result<ActiveConnection, ConnectionError> {
        let connection = ActiveConnection {
            id: connection_id.clone(),
            endpoint: endpoint.clone(),
            start_time: Utc::now(),
            status: ConnectionStatus::Connecting,
            request_info: None,
        };

        // Minimal write lock scope
        {
            let mut active_guard = self
                .active_connections
                .write()
                .map_err(|_| ConnectionError::LockPoisoned("active_connections"))?;

            active_guard.insert(connection_id.clone(), connection.clone());
        }

        // Update endpoint distribution
        {
            let mut dist_guard = self
                .endpoint_distribution
                .write()
                .map_err(|_| ConnectionError::LockPoisoned("endpoint_distribution"))?;

            *dist_guard.entry(endpoint).or_insert(0) += 1;
        }

        // Update peak concurrent connections
        self.update_peak_concurrent()?;
        self.update_last_activity()?;

        Ok(connection)
    }

    /// Update connection status (minimal write lock)
    pub fn update_connection_status(
        &self,
        connection_id: &str,
        status: ConnectionStatus,
    ) -> Result<(), ConnectionError> {
        let mut active_guard = self
            .active_connections
            .write()
            .map_err(|_| ConnectionError::LockPoisoned("active_connections"))?;

        if let Some(connection) = active_guard.get_mut(connection_id) {
            connection.status = status;
        }

        Ok(())
    }

    /// Complete a connection and update statistics
    pub fn complete_connection(
        &self,
        connection_id: &str,
    ) -> Result<Option<Duration>, ConnectionError> {
        let (connection, duration) = {
            let mut active_guard = self
                .active_connections
                .write()
                .map_err(|_| ConnectionError::LockPoisoned("active_connections"))?;

            if let Some(connection) = active_guard.remove(connection_id) {
                let duration = chrono::Utc::now()
                    .signed_duration_since(connection.start_time)
                    .to_std()
                    .unwrap_or(Duration::ZERO);
                (Some(connection), Some(duration))
            } else {
                (None, None)
            }
        };

        if let (Some(conn), Some(dur)) = (connection, duration) {
            // Update endpoint distribution
            {
                let mut dist_guard = self
                    .endpoint_distribution
                    .write()
                    .map_err(|_| ConnectionError::LockPoisoned("endpoint_distribution"))?;

                if let Some(count) = dist_guard.get_mut(&conn.endpoint) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        dist_guard.remove(&conn.endpoint);
                    }
                }
            }

            // Update statistics
            {
                let mut stats_guard = self
                    .stats
                    .write()
                    .map_err(|_| ConnectionError::LockPoisoned("stats"))?;

                stats_guard.total_completed += 1;

                // Update running average duration
                let total_requests = stats_guard.total_completed;
                if total_requests == 1 {
                    stats_guard.average_duration = dur;
                } else {
                    let current_avg = stats_guard.average_duration.as_millis() as u64;
                    let new_duration = dur.as_millis() as u64;
                    let new_avg =
                        (current_avg * (total_requests - 1) + new_duration) / total_requests;
                    stats_guard.average_duration = Duration::from_millis(new_avg);
                }
            }

            self.update_last_activity()?;
            Ok(Some(dur))
        } else {
            Ok(None)
        }
    }

    /// Fast read access to active connection count
    pub fn get_active_count(&self) -> Result<usize, ConnectionError> {
        self.active_connections
            .read()
            .map_err(|_| ConnectionError::LockPoisoned("active_connections"))
            .map(|guard| guard.len())
    }

    /// Fast read access to active connections (for dashboard)
    pub fn get_active_connections(&self) -> Result<Vec<ActiveConnection>, ConnectionError> {
        self.active_connections
            .read()
            .map_err(|_| ConnectionError::LockPoisoned("active_connections"))
            .map(|guard| guard.values().cloned().collect())
    }

    /// Fast read access to endpoint distribution
    pub fn get_endpoint_distribution(&self) -> Result<HashMap<String, u32>, ConnectionError> {
        self.endpoint_distribution
            .read()
            .map_err(|_| ConnectionError::LockPoisoned("endpoint_distribution"))
            .map(|guard| guard.clone())
    }

    /// Get connection statistics for monitoring
    pub fn get_stats(&self) -> Result<ConnectionStats, ConnectionError> {
        self.stats
            .read()
            .map_err(|_| ConnectionError::LockPoisoned("stats"))
            .map(|guard| guard.clone())
    }

    /// Clean up stale connections (connections that have been active too long)
    pub fn cleanup_stale_connections(
        &self,
        max_age: Duration,
    ) -> Result<Vec<String>, ConnectionError> {
        let now = Instant::now();
        let mut stale_connections = Vec::new();

        // Find stale connections
        {
            let active_guard = self
                .active_connections
                .read()
                .map_err(|_| ConnectionError::LockPoisoned("active_connections"))?;

            for (id, connection) in active_guard.iter() {
                let connection_age = chrono::Utc::now()
                    .signed_duration_since(connection.start_time)
                    .to_std()
                    .unwrap_or(Duration::ZERO);
                if connection_age > max_age {
                    stale_connections.push(id.clone());
                }
            }
        }

        // Remove stale connections
        if !stale_connections.is_empty() {
            let mut active_guard = self
                .active_connections
                .write()
                .map_err(|_| ConnectionError::LockPoisoned("active_connections"))?;

            let mut stats_guard = self
                .stats
                .write()
                .map_err(|_| ConnectionError::LockPoisoned("stats"))?;

            for id in &stale_connections {
                if let Some(connection) = active_guard.remove(id) {
                    // Update endpoint distribution
                    let mut dist_guard = self
                        .endpoint_distribution
                        .write()
                        .map_err(|_| ConnectionError::LockPoisoned("endpoint_distribution"))?;

                    if let Some(count) = dist_guard.get_mut(&connection.endpoint) {
                        *count = count.saturating_sub(1);
                        if *count == 0 {
                            dist_guard.remove(&connection.endpoint);
                        }
                    }

                    stats_guard.total_failed += 1;
                }
            }
        }

        Ok(stale_connections)
    }

    // Private helper methods
    fn update_peak_concurrent(&self) -> Result<(), ConnectionError> {
        let active_count = self.get_active_count()?;

        let mut stats_guard = self
            .stats
            .write()
            .map_err(|_| ConnectionError::LockPoisoned("stats"))?;

        if active_count as u32 > stats_guard.peak_concurrent {
            stats_guard.peak_concurrent = active_count as u32;
        }

        Ok(())
    }

    fn update_last_activity(&self) -> Result<(), ConnectionError> {
        let mut stats_guard = self
            .stats
            .write()
            .map_err(|_| ConnectionError::LockPoisoned("stats"))?;

        stats_guard.last_activity = Some(Instant::now());
        Ok(())
    }
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub enum ConnectionError {
    LockPoisoned(&'static str),
    ConnectionNotFound(String),
}

impl std::fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionError::LockPoisoned(name) => write!(f, "Lock was poisoned: {}", name),
            ConnectionError::ConnectionNotFound(id) => write!(f, "Connection not found: {}", id),
        }
    }
}

impl std::error::Error for ConnectionError {}

/// Generate unique connection ID using timestamp + counter
pub fn generate_connection_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let counter = COUNTER.fetch_add(1, Ordering::SeqCst);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("conn_{}_{}", timestamp, counter)
}

/// Shared type alias for the optimized connection manager
pub type SharedConnectionManager = Arc<ConnectionManager>;

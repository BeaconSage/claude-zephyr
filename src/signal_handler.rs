use crate::connection_tracker::SharedConnectionTracker;
use crate::events::ProxyEvent;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Graceful shutdown handler for the proxy server
pub struct GracefulShutdown {
    pub shutdown_flag: Arc<AtomicBool>,
    connection_tracker: SharedConnectionTracker,
    event_sender: mpsc::UnboundedSender<ProxyEvent>,
}

impl GracefulShutdown {
    pub fn new(
        connection_tracker: SharedConnectionTracker,
        event_sender: mpsc::UnboundedSender<ProxyEvent>,
    ) -> Self {
        Self {
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            connection_tracker,
            event_sender,
        }
    }

    /// Wait for shutdown signals and perform graceful cleanup
    pub async fn wait_for_shutdown(&self) {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("📨 Received SIGINT (Ctrl+C), performing graceful shutdown...");
                self.perform_graceful_shutdown("SIGINT").await;
            }
            _ = self.wait_for_sigterm() => {
                println!("📨 Received SIGTERM, performing graceful shutdown...");
                self.perform_graceful_shutdown("SIGTERM").await;
            }
        }
    }

    /// Wait for SIGTERM signal (Unix only)
    #[cfg(unix)]
    async fn wait_for_sigterm(&self) {
        use tokio::signal::unix::{signal, SignalKind};

        if let Ok(mut sigterm) = signal(SignalKind::terminate()) {
            sigterm.recv().await;
        }
    }

    /// For non-Unix systems, this will never trigger
    #[cfg(not(unix))]
    async fn wait_for_sigterm(&self) {
        std::future::pending::<()>().await;
    }

    /// Perform graceful shutdown cleanup
    async fn perform_graceful_shutdown(&self, signal: &str) {
        // Set shutdown flag
        self.shutdown_flag.store(true, Ordering::Relaxed);

        println!("🧹 Cleaning up all active connections due to {signal} signal...");

        // Force cleanup all connections
        let cleaned_connections = {
            if let Ok(mut tracker) = self.connection_tracker.lock() {
                tracker.force_cleanup_all_connections()
            } else {
                Vec::new()
            }
        };

        if !cleaned_connections.is_empty() {
            println!(
                "🧹 Cleaned up {} active connections",
                cleaned_connections.len()
            );

            // Send cleanup events for all connections
            for connection_id in cleaned_connections {
                let _ = self
                    .event_sender
                    .send(ProxyEvent::ConnectionCompleted(connection_id));
            }
        }

        println!("✅ Graceful shutdown completed");
    }

    /// Check if shutdown has been requested (reserved for future use)
    #[allow(dead_code)]
    pub fn is_shutdown_requested(&self) -> bool {
        self.shutdown_flag.load(Ordering::Relaxed)
    }
}

/// Emergency connection cleanup function (reserved for future use)
#[allow(dead_code)]
pub async fn emergency_connection_cleanup(
    connection_tracker: &SharedConnectionTracker,
    event_sender: &mpsc::UnboundedSender<ProxyEvent>,
    reason: &str,
) {
    println!("⚠️ Emergency connection cleanup triggered: {reason}");

    let cleaned_connections = {
        if let Ok(mut tracker) = connection_tracker.lock() {
            tracker.force_cleanup_all_connections()
        } else {
            return;
        }
    };

    if !cleaned_connections.is_empty() {
        println!(
            "🧹 Emergency cleanup: removed {} connections",
            cleaned_connections.len()
        );

        for connection_id in cleaned_connections {
            let _ = event_sender.send(ProxyEvent::ConnectionCompleted(connection_id));
        }
    }
}

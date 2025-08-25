mod config;
mod connection_manager;
mod connection_tracker;
mod dashboard;
mod dev_tools;
mod dynamic_health;
mod events;
mod health;
mod health_orchestrator;
mod logging;
mod migration_adapter;
mod proxy;
mod state_manager;

use clap::Parser;
use config::Config;
use connection_tracker::{ConnectionTracker, SharedConnectionTracker};
use dashboard::Dashboard;
use events::ProxyEvent;
use futures::future;
use health_orchestrator::{HealthCheckOrchestrator, OrchestratorCommand};
use logging::*;
use proxy::{ProxyState, SharedState};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Parser)]
#[command(name = "claude-zephyr")]
#[command(
    about = "Automatic endpoint switching for Claude API"
)]
struct Args {
    /// Enable TUI dashboard mode
    #[arg(long, help = "Run with interactive dashboard")]
    dashboard: bool,

    /// Run timing self-test
    #[arg(long, help = "Run health check timing self-test")]
    test_timing: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Run timing test if requested
    if args.test_timing {
        return dev_tools::test_health_check_timing().await;
    }

    // Initialize logging based on mode
    if !args.dashboard {
        // Normal mode: enable beautiful logging
        tracing_subscriber::fmt::init();
    }
    // Dashboard mode: no console logging to avoid interfering with TUI

    // Load configuration
    let config = Config::load_default().map_err(|e| {
        if !args.dashboard {
            log_config_error(&format!("Failed to load configuration: {e}"));
        }
        eprintln!("Please create a config.toml file or ensure the auth token is properly set.");
        e
    })?;

    if !args.dashboard {
        let total_endpoints: usize = config.groups.iter().map(|g| g.endpoints.len()).sum();
        log_config_loaded(total_endpoints);
    }

    // Create connection tracker and event system
    let connection_tracker = Arc::new(Mutex::new(ConnectionTracker::new()));
    let (event_sender, event_receiver) = mpsc::unbounded_channel::<ProxyEvent>();

    // Send initial config event
    let total_endpoints: usize = config.groups.iter().map(|g| g.endpoints.len()).sum();
    let _ = event_sender.send(ProxyEvent::ConfigLoaded {
        endpoint_count: total_endpoints,
    });

    let state = Arc::new(Mutex::new(ProxyState::new(config.clone())));

    // Check if dashboard mode is enabled
    if args.dashboard {
        // Run in dashboard mode
        run_with_dashboard(
            config,
            state,
            connection_tracker,
            event_sender,
            event_receiver,
        )
        .await
    } else {
        // Run in normal mode (existing behavior)
        run_normal_mode(config, state, connection_tracker, event_sender).await
    }
}

async fn run_with_dashboard(
    config: Config,
    state: SharedState,
    connection_tracker: SharedConnectionTracker,
    event_sender: mpsc::UnboundedSender<ProxyEvent>,
    event_receiver: mpsc::UnboundedReceiver<ProxyEvent>,
) -> anyhow::Result<()> {
    // Create dashboard before moving config into spawned tasks
    let dashboard_interval = config.health_check_interval();
    let mut dashboard = Dashboard::new(&config, dashboard_interval);

    // Start health check orchestrator (dashboard mode - no console logs)
    let health_state = state.clone();
    let health_config = config.clone();
    let health_sender = event_sender.clone();
    let health_tracker = connection_tracker.clone();

    let (health_orchestrator, orchestrator_command_sender) = HealthCheckOrchestrator::new(
        health_config,
        health_state,
        health_sender,
        true, // dashboard mode
        Some(health_tracker),
    );

    tokio::spawn(async move {
        if let Err(e) = health_orchestrator.run().await {
            tracing::error!("Health check orchestrator error: {}", e);
        }
    });

    // Start proxy server (dashboard mode - no console logs)
    let proxy_sender = event_sender.clone();
    let proxy_tracker = connection_tracker.clone();
    let proxy_state = state.clone(); // Clone for proxy server
    tokio::spawn(async move {
        let _ = proxy::start_proxy_server_with_events_dashboard(
            config,
            proxy_state,
            proxy_tracker,
            proxy_sender,
        )
        .await;
    });

    // Run dashboard
    dashboard
        .run(
            event_receiver,
            connection_tracker,
            state,
            orchestrator_command_sender,
        )
        .await
}

async fn run_normal_mode(
    config: Config,
    state: SharedState,
    connection_tracker: SharedConnectionTracker,
    event_sender: mpsc::UnboundedSender<ProxyEvent>,
) -> anyhow::Result<()> {
    // Start health check orchestrator (normal mode - with console logs)
    let health_state = state.clone();
    let health_config = config.clone();
    let health_sender = event_sender.clone();
    let health_tracker = connection_tracker.clone();

    let (_health_orchestrator, _orchestrator_command_sender) = HealthCheckOrchestrator::new(
        health_config,
        health_state,
        health_sender,
        false, // normal mode
        Some(health_tracker),
    );

    tokio::spawn(async move {
        if let Err(e) = _health_orchestrator.run().await {
            tracing::error!("Health check orchestrator error: {}", e);
        }
    });

    // Start proxy server (existing behavior with events)
    proxy::start_proxy_server_with_events(config, state, connection_tracker, event_sender).await
}

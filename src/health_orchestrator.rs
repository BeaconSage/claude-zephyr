use crate::config::Config;
use crate::connection_tracker::SharedConnectionTracker;
use crate::dynamic_health::DynamicHealthChecker;
use crate::events::{ProxyEvent, SelectionMode};
use crate::health::{self, EndpointStatus};
use crate::proxy::SharedState;
use futures::future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Commands to control the health orchestrator
#[derive(Debug, Clone)]
pub enum OrchestratorCommand {
    Pause,
    Resume,
    ManualRefresh,
}

/// Modern health check orchestrator with clear separation of concerns
pub struct HealthCheckOrchestrator {
    config: Config,
    state: SharedState,
    event_sender: mpsc::UnboundedSender<ProxyEvent>,
    connection_tracker: Option<SharedConnectionTracker>,
    dynamic_checker: Option<DynamicHealthChecker>,
    dashboard_mode: bool,
    // Track if someone in current cycle has already won the race
    cycle_winner_chosen: std::sync::Arc<std::sync::Mutex<bool>>,
    // System pause state
    is_paused: Arc<Mutex<bool>>,
    // Command receiver for pause/resume/refresh
    command_receiver: mpsc::UnboundedReceiver<OrchestratorCommand>,
    // Command sender (for returning to caller)
    #[allow(dead_code)]
    command_sender: mpsc::UnboundedSender<OrchestratorCommand>,
}

impl HealthCheckOrchestrator {
    pub fn new(
        config: Config,
        state: SharedState,
        event_sender: mpsc::UnboundedSender<ProxyEvent>,
        dashboard_mode: bool,
        connection_tracker: Option<SharedConnectionTracker>,
    ) -> (Self, mpsc::UnboundedSender<OrchestratorCommand>) {
        let dynamic_checker = connection_tracker
            .as_ref()
            .map(|_| DynamicHealthChecker::new(&config));

        let (command_sender, command_receiver) = mpsc::unbounded_channel();

        let orchestrator = Self {
            config,
            state,
            event_sender,
            connection_tracker,
            dynamic_checker,
            dashboard_mode,
            cycle_winner_chosen: std::sync::Arc::new(std::sync::Mutex::new(false)),
            is_paused: Arc::new(Mutex::new(false)),
            command_receiver,
            command_sender: command_sender.clone(),
        };

        (orchestrator, command_sender)
    }

    /// Main orchestration loop - supports pause/resume and manual refresh
    pub async fn run(mut self) -> anyhow::Result<()> {
        let mut current_interval = self.config.health_check_interval();
        // Start immediately instead of waiting for the first interval
        let mut next_check = tokio::time::Instant::now();

        loop {
            // Handle commands and check pause state
            tokio::select! {
                // Handle orchestrator commands (pause/resume/manual refresh)
                command = self.command_receiver.recv() => {
                    if let Some(cmd) = command {
                        match cmd {
                            OrchestratorCommand::Pause => {
                                self.handle_pause().await;
                            },
                            OrchestratorCommand::Resume => {
                                self.handle_resume(&mut next_check, current_interval).await;
                            },
                            OrchestratorCommand::ManualRefresh => {
                                self.handle_manual_refresh(&mut current_interval).await?;
                            }
                        }
                    }
                }

                // Regular health check cycle (only if not paused and time is reached)
                _ = tokio::time::sleep_until(next_check) => {
                    let is_paused = self.is_paused.lock().map(|guard| *guard).unwrap_or(true);
                    if !is_paused {
                        // Calculate optimal check interval
                        let check_interval = self.calculate_optimal_interval(&mut current_interval);

                        // Execute health check cycle
                        let cycle_result = self.execute_health_cycle(check_interval).await;

                        // Handle cycle results and update state
                        self.process_cycle_results(cycle_result).await?;

                        // Cleanup and prepare for next cycle
                        self.prepare_next_cycle();

                        // Schedule next check
                        next_check = tokio::time::Instant::now() + check_interval;
                    } else {
                        // If paused, just sleep a short time and check again
                        next_check = tokio::time::Instant::now() + Duration::from_secs(1);
                    }
                }
            }
        }
    }

    /// Calculate optimal check interval based on current conditions
    fn calculate_optimal_interval(&mut self, current_interval: &mut Duration) -> Duration {
        if let (Some(ref mut checker), Some(ref tracker)) =
            (&mut self.dynamic_checker, &self.connection_tracker)
        {
            let new_interval = checker.calculate_interval(tracker);
            let current_val = *current_interval;
            let load_level = checker.get_load_level();

            // Update display interval if significantly changed
            let should_update =
                self.should_update_interval_display(current_val, new_interval, load_level);

            if should_update {
                *current_interval = new_interval;
                if !self.dashboard_mode {
                    println!(
                        "🔄 Health check interval adjusted to {}s (Load: {:?})",
                        current_interval.as_secs(),
                        load_level
                    );
                }
            }

            new_interval
        } else {
            *current_interval
        }
    }

    /// Execute a complete health check cycle
    async fn execute_health_cycle(&self, interval: Duration) -> HealthCycleResult {
        let cycle_start = Instant::now();
        let next_check_time = cycle_start + interval;

        // Reset race winner flag for this cycle
        if let Ok(mut winner_chosen) = self.cycle_winner_chosen.lock() {
            *winner_chosen = false;
        }

        // Send cycle start event
        self.send_cycle_start_event(interval, next_check_time).await;

        // Mark all endpoints as checking (best effort)
        let _ = self.mark_endpoints_as_checking().await;

        // Execute parallel health checks
        let check_results = self.execute_parallel_checks(cycle_start).await;

        HealthCycleResult {
            start_time: cycle_start,
            results: check_results,
            duration: cycle_start.elapsed(),
        }
    }

    /// Execute health checks for all endpoints in parallel
    async fn execute_parallel_checks(&self, cycle_start: Instant) -> Vec<EndpointStatus> {
        let all_endpoints = self.config.get_all_endpoints_legacy();

        // Send running event
        let _ = self.event_sender.send(ProxyEvent::HealthCheckRunning {
            started_at: cycle_start,
            estimated_duration: Duration::from_secs(self.config.health_check.timeout_seconds + 5),
        });

        // Create parallel check tasks
        let check_futures: Vec<_> = all_endpoints
            .iter()
            .map(|(auth_token, endpoint_config, _)| {
                self.create_endpoint_check_task(
                    auth_token,
                    endpoint_config.clone(),
                    self.cycle_winner_chosen.clone(),
                )
            })
            .collect();

        // Execute with timeout
        let timeout_duration = Duration::from_secs(self.config.health_check.timeout_seconds + 5);
        match tokio::time::timeout(timeout_duration, future::join_all(check_futures)).await {
            Ok(results) => results.into_iter().flatten().collect(),
            Err(_) => {
                if !self.dashboard_mode {
                    println!(
                        "⚠️  Health check cycle timed out after {}s",
                        timeout_duration.as_secs()
                    );
                }
                Vec::new()
            }
        }
    }

    /// Create a health check task for a single endpoint
    async fn create_endpoint_check_task(
        &self,
        auth_token: &str,
        endpoint_config: crate::config::EndpointConfig,
        cycle_winner_chosen: std::sync::Arc<std::sync::Mutex<bool>>,
    ) -> Option<EndpointStatus> {
        let endpoint_url = endpoint_config.url.clone();
        let endpoint_url_clone = endpoint_url.clone(); // For error handling
        let auth_token = auth_token.to_string();
        let config = self.config.clone();
        let state = self.state.clone();
        let event_sender = self.event_sender.clone();
        let dashboard_mode = self.dashboard_mode;

        // Spawn health check task
        let check_result = tokio::task::spawn_blocking(move || {
            health::check_endpoint_health(&endpoint_url, &config, &auth_token)
        })
        .await;

        let new_status = check_result.unwrap_or_else(|e| {
            if !dashboard_mode {
                println!("⚠️  Health check task error for {endpoint_url_clone}: {e}");
            }
            health::EndpointStatus::new_unavailable(endpoint_url_clone, format!("Task error: {e}"))
        });

        // Update state and check for race winner (first available wins)
        self.update_endpoint_state(&new_status, &state, &event_sender, cycle_winner_chosen)
            .await
    }

    /// Update endpoint state without switching (for batch processing)
    #[allow(dead_code)]
    async fn update_endpoint_state_only(
        &self,
        new_status: &EndpointStatus,
        state: &SharedState,
        event_sender: &mpsc::UnboundedSender<ProxyEvent>,
    ) -> Option<EndpointStatus> {
        // Update state with preserved history
        let final_status = self.merge_with_existing_status(new_status, state).await?;

        // Send health update event
        let _ = event_sender.send(ProxyEvent::HealthUpdate(final_status.clone()));

        // Don't perform switch here - will be handled in batch after all checks complete

        Some(final_status)
    }

    /// Update endpoint state and check for switches
    async fn update_endpoint_state(
        &self,
        new_status: &EndpointStatus,
        state: &SharedState,
        event_sender: &mpsc::UnboundedSender<ProxyEvent>,
        cycle_winner_chosen: std::sync::Arc<std::sync::Mutex<bool>>,
    ) -> Option<EndpointStatus> {
        // Update state with preserved history
        let final_status = self.merge_with_existing_status(new_status, state).await?;

        // Update the state with the merged status (important for history preservation)
        {
            if let Ok(mut state_guard) = state.lock() {
                state_guard
                    .endpoint_status
                    .insert(final_status.endpoint.clone(), final_status.clone());
            }
        }

        // Send health update event
        let _ = event_sender.send(ProxyEvent::HealthUpdate(final_status.clone()));

        // Check for race winner: first available endpoint wins
        self.check_race_winner(&final_status, state, event_sender, cycle_winner_chosen)
            .await;

        Some(final_status)
    }

    /// Process results from a completed health cycle
    async fn process_cycle_results(&self, cycle_result: HealthCycleResult) -> anyhow::Result<()> {
        // Send cycle completion event
        let _ = self.event_sender.send(ProxyEvent::HealthCheckCompleted {
            duration: cycle_result.duration,
        });

        if !self.dashboard_mode {
            println!(
                "✅ Health check completed in {}ms, found {} endpoints",
                cycle_result.duration.as_millis(),
                cycle_result.results.len()
            );
        }

        Ok(())
    }

    /// Check if this endpoint wins the race (first available wins) - only in Auto mode
    async fn check_race_winner(
        &self,
        status: &EndpointStatus,
        state: &SharedState,
        event_sender: &mpsc::UnboundedSender<ProxyEvent>,
        cycle_winner_chosen: std::sync::Arc<std::sync::Mutex<bool>>,
    ) {
        // Only available endpoints can win the race
        if !status.available {
            return;
        }

        // Check selection mode from proxy state - skip auto-switching in manual mode
        let is_auto_mode = {
            if let Ok(state_guard) = state.lock() {
                state_guard.selection_mode == SelectionMode::Auto
            } else {
                false // Default to no switching if lock fails
            }
        };

        if !is_auto_mode {
            // In manual mode, don't perform automatic switching
            return;
        }

        // Try to claim the race winner spot
        let won_race = {
            if let Ok(mut winner_chosen) = cycle_winner_chosen.lock() {
                if !*winner_chosen {
                    // This endpoint is first available one, it wins!
                    *winner_chosen = true;
                    true
                } else {
                    // Someone already won, this endpoint is too late
                    false
                }
            } else {
                false
            }
        };

        // If this endpoint won the race, switch to it
        if won_race {
            let switch_info = {
                let state_guard = match state.lock() {
                    Ok(guard) => guard,
                    Err(_) => return,
                };

                self.calculate_switch_decision(status, &state_guard)
            };

            if let Some((from_endpoint, from_latency, to_latency)) = switch_info {
                self.perform_endpoint_switch(
                    status,
                    from_endpoint,
                    from_latency,
                    to_latency,
                    state,
                    event_sender,
                )
                .await;
            }
        }
    }

    /// Helper methods for internal operations
    fn should_update_interval_display(
        &self,
        current: Duration,
        new: Duration,
        load_level: crate::dynamic_health::LoadLevel,
    ) -> bool {
        if new == current {
            return false;
        }

        let ratio = if new > current {
            new.as_secs() as f64 / current.as_secs() as f64
        } else {
            current.as_secs() as f64 / new.as_secs() as f64
        };

        ratio > 1.1
            || (load_level == crate::dynamic_health::LoadLevel::High && new < current)
            || (load_level == crate::dynamic_health::LoadLevel::Idle && new > current)
    }

    async fn send_cycle_start_event(&self, interval: Duration, next_check: Instant) {
        let load_level = self
            .dynamic_checker
            .as_ref()
            .map(|c| c.get_load_level())
            .unwrap_or(crate::dynamic_health::LoadLevel::Idle);

        let active_connections = self
            .connection_tracker
            .as_ref()
            .and_then(|t| t.lock().ok())
            .map(|t| t.get_active_count())
            .unwrap_or(0);

        let _ = self.event_sender.send(ProxyEvent::HealthCheckStarted {
            actual_interval: interval,
            next_check_time: next_check,
            load_level,
            active_connections,
        });
    }

    async fn mark_endpoints_as_checking(&self) -> anyhow::Result<()> {
        let mut state_guard = self
            .state
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire state lock: {}", e))?;

        for (_, endpoint_config, _) in self.config.get_all_endpoints() {
            if !state_guard
                .endpoint_status
                .contains_key(&endpoint_config.url)
            {
                let checking_status =
                    health::EndpointStatus::new_checking(endpoint_config.url.clone());
                state_guard
                    .endpoint_status
                    .insert(endpoint_config.url.clone(), checking_status.clone());
                let _ = self
                    .event_sender
                    .send(ProxyEvent::HealthUpdate(checking_status));
            } else if let Some(existing_status) =
                state_guard.endpoint_status.get_mut(&endpoint_config.url)
            {
                existing_status.available = false;
                existing_status.error = None;
                let _ = self
                    .event_sender
                    .send(ProxyEvent::HealthUpdate(existing_status.clone()));
            }
        }

        Ok(())
    }

    async fn merge_with_existing_status(
        &self,
        new_status: &EndpointStatus,
        state: &SharedState,
    ) -> Option<EndpointStatus> {
        let state_guard = state.lock().ok()?;

        if let Some(existing_status) = state_guard.endpoint_status.get(&new_status.endpoint) {
            let mut updated_status = existing_status.clone();
            if new_status.available {
                updated_status.update_with_check_result(Some(new_status.latency), None);
            } else {
                updated_status.update_with_check_result(None, new_status.error.clone());
            }
            Some(updated_status)
        } else {
            // First time seeing this endpoint - use new status but ensure it has the measurement
            let mut first_time_status = new_status.clone();
            if new_status.available {
                first_time_status.update_with_check_result(Some(new_status.latency), None);
            } else {
                first_time_status.update_with_check_result(None, new_status.error.clone());
            }
            Some(first_time_status)
        }
    }

    #[allow(dead_code)]
    async fn check_and_perform_switch(
        &self,
        status: &EndpointStatus,
        state: &SharedState,
        event_sender: &mpsc::UnboundedSender<ProxyEvent>,
    ) {
        if !status.available {
            return;
        }

        let switch_info = {
            let state_guard = match state.lock() {
                Ok(guard) => guard,
                Err(_) => return,
            };

            self.calculate_switch_decision(status, &state_guard)
        };

        if let Some((from_endpoint, from_latency, to_latency)) = switch_info {
            self.perform_endpoint_switch(
                status,
                from_endpoint,
                from_latency,
                to_latency,
                state,
                event_sender,
            )
            .await;
        }
    }

    fn calculate_switch_decision(
        &self,
        status: &EndpointStatus,
        state_guard: &crate::proxy::ProxyState,
    ) -> Option<(String, u64, u64)> {
        let current = &state_guard.current_endpoint;
        let threshold = self.config.server.switch_threshold_ms;

        // Only consider available endpoints for switching
        if !status.available {
            return None;
        }

        // Switch immediately if:
        // 1. No current endpoint, OR
        // 2. Current endpoint is not available, OR
        // 3. This endpoint is significantly faster than current
        if current.is_empty() {
            Some((String::new(), 999999, status.latency))
        } else if let Some(current_status) = state_guard.endpoint_status.get(current) {
            if !current_status.available {
                // Current is down, switch immediately
                Some((current.clone(), current_status.latency, status.latency))
            } else if current_status.latency.saturating_sub(status.latency) >= threshold {
                // This endpoint is significantly faster than current
                Some((current.clone(), current_status.latency, status.latency))
            } else {
                None
            }
        } else {
            // Current endpoint has no status, switch to this one
            Some((current.clone(), 999999, status.latency))
        }
    }

    async fn perform_endpoint_switch(
        &self,
        status: &EndpointStatus,
        from_endpoint: String,
        from_latency: u64,
        to_latency: u64,
        state: &SharedState,
        event_sender: &mpsc::UnboundedSender<ProxyEvent>,
    ) {
        if let Ok(mut state_guard) = state.lock() {
            if self.dashboard_mode {
                state_guard.switch_endpoint_silent(status.endpoint.clone());
            } else {
                state_guard.switch_endpoint(status.endpoint.clone());
            }

            let _ = event_sender.send(ProxyEvent::EndpointSwitch {
                from: from_endpoint,
                to: status.endpoint.clone(),
                from_latency,
                to_latency,
            });
        }
    }

    /// Handle system pause command
    async fn handle_pause(&self) {
        if let Ok(mut is_paused) = self.is_paused.lock() {
            *is_paused = true;
        }

        let _ = self.event_sender.send(ProxyEvent::SystemPaused);

        if !self.dashboard_mode {
            println!("⏸️  Health monitoring paused - manual refresh available with 'R'");
        }
    }

    /// Handle system resume command
    async fn handle_resume(
        &self,
        next_check: &mut tokio::time::Instant,
        _current_interval: Duration,
    ) {
        if let Ok(mut is_paused) = self.is_paused.lock() {
            *is_paused = false;
        }

        // Schedule immediate check on resume
        *next_check = tokio::time::Instant::now();

        let _ = self.event_sender.send(ProxyEvent::SystemResumed);

        if !self.dashboard_mode {
            println!("▶️  Health monitoring resumed");
        }
    }

    /// Handle manual refresh command - can work in both paused and running states
    async fn handle_manual_refresh(
        &mut self,
        current_interval: &mut Duration,
    ) -> anyhow::Result<()> {
        let _ = self.event_sender.send(ProxyEvent::ManualRefreshTriggered);

        if !self.dashboard_mode {
            println!("🔄 Manual health check triggered...");
        }

        // Calculate optimal check interval
        let check_interval = self.calculate_optimal_interval(current_interval);

        // Execute health check cycle
        let cycle_result = self.execute_health_cycle(check_interval).await;

        // Handle cycle results and update state
        self.process_cycle_results(cycle_result).await?;

        // Cleanup and prepare for next cycle
        self.prepare_next_cycle();

        if !self.dashboard_mode {
            println!("✅ Manual health check completed");
        }

        Ok(())
    }

    fn prepare_next_cycle(&self) {
        // Future: Add any cleanup or preparation logic here
    }
}

/// Result of a health check cycle
struct HealthCycleResult {
    #[allow(dead_code)]
    start_time: Instant,
    results: Vec<EndpointStatus>,
    duration: Duration,
}

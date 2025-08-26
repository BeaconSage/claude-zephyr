use crate::config::{Config, EndpointConfig};
use crate::connection_tracker::{EventReceiver, SharedConnectionTracker};
use crate::dynamic_health::LoadLevel;
use crate::events::{ActiveConnection, ConnectionStatus, ProxyEvent, SelectionMode};
use crate::health::{EndpointStatus, LatencyHistory};
use crate::proxy::SharedState;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Text,
    widgets::{Block, Borders, List, ListItem, Paragraph, Row, Table, Wrap},
    Frame, Terminal,
};
use std::collections::{HashMap, VecDeque};
use std::io;
use std::time::{Duration, Instant};
use tokio::time::interval;

/// Main dashboard application state
pub struct Dashboard {
    /// All configured endpoints (for pre-filling)
    all_endpoints: Vec<String>,
    /// Endpoint configuration mapping (URL -> Config)
    endpoint_configs: HashMap<String, EndpointConfig>,
    /// Current endpoint health status
    endpoint_health: HashMap<String, EndpointStatus>,
    /// Current active endpoint
    current_endpoint: String,
    /// Active connections from tracker
    active_connections: Vec<ActiveConnection>,
    /// Connection statistics
    total_connections: u32,
    peak_connections: u32,
    completed_connections: u64,
    /// Last endpoint switch info
    last_switch: Option<SwitchInfo>,
    /// Health check timing
    next_health_check: Instant,
    health_check_interval: Duration,
    /// Health check running status
    health_check_running: Option<(Instant, Duration)>, // (started_at, estimated_duration)
    /// Load status information
    current_load_level: LoadLevel,
    active_connections_count: u32,
    /// Selection mode and manual selection state
    selection_mode: SelectionMode,
    manual_selected_index: Option<usize>, // Index in all_endpoints for manual selection
    /// Proxy server information
    proxy_port: u16,
    /// UI state
    should_quit: bool,
    paused: bool,
    scroll_offset: usize,
    /// Cursor position for endpoint selection (replaces direct key selection)
    cursor_index: usize,
    /// Request tracking for improved load calculation
    recent_requests: VecDeque<Instant>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct SwitchInfo {
    from: String,
    to: String,
    from_latency: u64,
    to_latency: u64,
    improvement: u64,
}

impl Dashboard {
    pub fn new(config: &Config, health_check_interval: Duration) -> Self {
        let mut endpoint_health = HashMap::new();
        let mut endpoint_configs = HashMap::new();
        let mut all_endpoints = Vec::new();

        // Extract all endpoints and their configs
        for (_, endpoint_config, _) in config.get_all_endpoints_legacy() {
            let url = endpoint_config.url.clone();
            all_endpoints.push(url.clone());
            endpoint_configs.insert(url.clone(), endpoint_config);

            // Pre-fill with checking status
            endpoint_health.insert(url.clone(), EndpointStatus::new_checking(url));
        }

        // Set default current endpoint
        let default_endpoint = if let Some((_, default_endpoint)) = config.get_default_endpoint() {
            default_endpoint.url.clone()
        } else {
            all_endpoints.first().cloned().unwrap_or_default()
        };

        Self {
            all_endpoints,
            endpoint_configs,
            endpoint_health,
            current_endpoint: default_endpoint,
            active_connections: Vec::new(),
            total_connections: 0,
            peak_connections: 0,
            completed_connections: 0,
            last_switch: None,
            next_health_check: Instant::now(), // Will be properly set by first HealthCheckStarted event
            health_check_interval,
            health_check_running: None, // No health check running initially
            current_load_level: LoadLevel::Idle,
            active_connections_count: 0,
            selection_mode: SelectionMode::Auto, // Start with auto mode
            manual_selected_index: None,         // No manual selection initially
            proxy_port: config.server.port,
            should_quit: false,
            paused: false,
            scroll_offset: 0,
            cursor_index: 0,
            recent_requests: VecDeque::new(),
        }
    }

    /// Run the main dashboard loop
    pub async fn run(
        &mut self,
        mut event_receiver: EventReceiver,
        connection_tracker: SharedConnectionTracker,
        proxy_state: SharedState,
        orchestrator_command_sender: tokio::sync::mpsc::UnboundedSender<
            crate::health_orchestrator::OrchestratorCommand,
        >,
    ) -> anyhow::Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let mut tick_interval = interval(Duration::from_millis(250)); // 4 FPS

        loop {
            // Handle events
            tokio::select! {
                // Handle proxy events - always process to stay in sync
                event = event_receiver.recv() => {
                    if let Some(event) = event {
                        self.handle_proxy_event(event);
                    }
                }

                // Update active connections from tracker - always run regardless of pause state
                // Users expect to see real-time connection monitoring even when health checks are paused
                _ = tick_interval.tick() => {
                    self.update_from_tracker(&connection_tracker);
                }

                // Handle keyboard input
                _ = tokio::time::sleep(Duration::from_millis(16)) => {
                    if event::poll(Duration::from_millis(0))? {
                        if let Event::Key(key) = event::read()? {
                            match key.code {
                                KeyCode::Char('q') => break,
                                KeyCode::Char('r') => {
                                    // Manual refresh - trigger health check
                                    let _ = orchestrator_command_sender.send(crate::health_orchestrator::OrchestratorCommand::ManualRefresh);
                                    self.update_from_tracker(&connection_tracker);
                                }
                                KeyCode::Char('p') => {
                                    // Toggle system pause/resume
                                    self.paused = !self.paused;
                                    if self.paused {
                                        let _ = orchestrator_command_sender.send(crate::health_orchestrator::OrchestratorCommand::Pause);
                                    } else {
                                        let _ = orchestrator_command_sender.send(crate::health_orchestrator::OrchestratorCommand::Resume);
                                    }
                                }
                                KeyCode::Char('m') => {
                                    // Toggle selection mode
                                    self.toggle_selection_mode(&proxy_state);
                                }
                                KeyCode::Up => {
                                    // Move cursor up (with wraparound)
                                    if self.cursor_index > 0 {
                                        self.cursor_index -= 1;
                                    } else {
                                        self.cursor_index = self.all_endpoints.len().saturating_sub(1);
                                    }

                                    // Auto-adjust scroll offset to follow cursor
                                    if self.cursor_index < self.scroll_offset {
                                        self.scroll_offset = self.cursor_index;
                                    }
                                }
                                KeyCode::Down => {
                                    // Move cursor down (with wraparound)
                                    if self.cursor_index < self.all_endpoints.len().saturating_sub(1) {
                                        self.cursor_index += 1;
                                    } else {
                                        self.cursor_index = 0;
                                    }

                                    // Auto-adjust scroll offset to follow cursor
                                    // Assuming ~10 visible rows, adjust as needed
                                    if self.cursor_index >= self.scroll_offset + 10 {
                                        self.scroll_offset = self.cursor_index.saturating_sub(9);
                                    }
                                }
                                KeyCode::Enter => {
                                    // Confirm endpoint selection (only in manual mode)
                                    if self.selection_mode == SelectionMode::Manual {
                                        self.handle_manual_endpoint_selection_by_index(self.cursor_index, &proxy_state);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

            // Render UI
            terminal.draw(|f| self.render(f))?;

            if self.should_quit {
                break;
            }
        }

        // Cleanup terminal
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        Ok(())
    }

    fn handle_proxy_event(&mut self, event: ProxyEvent) {
        match event {
            ProxyEvent::HealthUpdate(status) => {
                self.endpoint_health.insert(status.endpoint.clone(), status);
                // Don't reset countdown for individual health updates
                // Let the health check cycle event handle timing
            }
            ProxyEvent::HealthCheckStarted {
                actual_interval,
                next_check_time,
                load_level,
                active_connections,
            } => {
                // Use the actual next check time from the main loop, not calculated time
                self.next_health_check = next_check_time;
                self.health_check_interval = actual_interval;
                self.current_load_level = load_level;
                self.active_connections_count = active_connections;
                self.health_check_running = None; // Health check hasn't started executing yet
            }
            ProxyEvent::HealthCheckRunning {
                started_at,
                estimated_duration,
            } => {
                self.health_check_running = Some((started_at, estimated_duration));
            }
            ProxyEvent::HealthCheckCompleted { duration: _ } => {
                // Health check completed, clear running status
                self.health_check_running = None;
            }
            ProxyEvent::EndpointSwitch {
                from,
                to,
                from_latency,
                to_latency,
            } => {
                self.current_endpoint = to.clone();
                // Calculate improvement: positive when switching to faster endpoint
                let improvement = from_latency.saturating_sub(to_latency);

                self.last_switch = Some(SwitchInfo {
                    from,
                    to,
                    from_latency,
                    to_latency,
                    improvement,
                });
            }
            ProxyEvent::SelectionModeChanged { mode } => {
                self.selection_mode = mode;
            }
            ProxyEvent::ManualEndpointSelected {
                endpoint,
                endpoint_index,
            } => {
                self.current_endpoint = endpoint;
                self.manual_selected_index = Some(endpoint_index);
            }
            ProxyEvent::ServerStarted { .. } => {}
            ProxyEvent::ConfigLoaded { .. } => {}
            ProxyEvent::SystemPaused => {
                // System is now truly paused - health checks stopped
                self.paused = true;
            }
            ProxyEvent::SystemResumed => {
                // System is now running - health checks resumed
                self.paused = false;
            }
            ProxyEvent::ManualRefreshTriggered => {
                // Manual refresh was triggered - no special UI action needed
                // The actual health check results will come via HealthUpdate events
            }
            ProxyEvent::RequestReceived { timestamp, .. } => {
                // Record the request timestamp for load calculation
                self.recent_requests.push_back(timestamp);

                // Clean old requests (keep only last 5 minutes of data)
                let five_minutes_ago = timestamp - Duration::from_secs(300);
                while let Some(&front_time) = self.recent_requests.front() {
                    if front_time < five_minutes_ago {
                        self.recent_requests.pop_front();
                    } else {
                        break;
                    }
                }

                // Recalculate load level based on both active connections and request rate
                self.recalculate_load_level();
            }
            _ => {} // Connection events are handled via tracker updates
        }
    }

    /// Recalculate load level based on both active connections and request frequency
    fn recalculate_load_level(&mut self) {
        let now = Instant::now();

        // Calculate request rate per minute
        let one_minute_ago = now - Duration::from_secs(60);
        let requests_last_minute = self
            .recent_requests
            .iter()
            .filter(|&&timestamp| timestamp >= one_minute_ago)
            .count() as f64;

        // Get current active connections count
        let active_connections = self.active_connections_count;

        // Improved load level calculation that considers both metrics
        let new_load_level = match (active_connections, requests_last_minute as u32) {
            // High load: Many concurrent connections OR high request rate
            (conn, _) if conn > 10 => LoadLevel::High,
            (_, req_rate) if req_rate > 30 => LoadLevel::High, // >30 requests/minute

            // Medium load: Moderate concurrent connections OR moderate request rate
            (conn, req_rate) if conn >= 4 || req_rate >= 10 => LoadLevel::Medium,

            // Low load: Few connections but some request activity
            (conn, req_rate) if conn > 0 || req_rate >= 2 => LoadLevel::Low,

            // Idle: No connections and very few or no requests
            _ => LoadLevel::Idle,
        };

        // Update load level if it changed
        if new_load_level != self.current_load_level {
            self.current_load_level = new_load_level;
        }
    }

    /// Toggle between auto and manual selection modes
    fn toggle_selection_mode(&mut self, proxy_state: &SharedState) {
        self.selection_mode = match self.selection_mode {
            SelectionMode::Auto => SelectionMode::Manual,
            SelectionMode::Manual => SelectionMode::Auto,
        };

        // When switching to manual mode, set current endpoint as the manual selection
        if self.selection_mode == SelectionMode::Manual {
            if let Some(index) = self
                .all_endpoints
                .iter()
                .position(|ep| ep == &self.current_endpoint)
            {
                self.manual_selected_index = Some(index);
            }
        } else {
            // When switching to auto mode, clear manual selection
            self.manual_selected_index = None;
        }

        // Store selection mode in proxy state for health orchestrator to read
        if let Ok(mut state_guard) = proxy_state.lock() {
            state_guard.selection_mode = self.selection_mode;
        }
    }

    /// Handle manual endpoint selection by index (0-based)
    fn handle_manual_endpoint_selection_by_index(
        &mut self,
        index: usize,
        proxy_state: &SharedState,
    ) {
        // Only process in manual mode
        if self.selection_mode != SelectionMode::Manual {
            return;
        }

        // Check if index is valid
        if index < self.all_endpoints.len() {
            let endpoint = &self.all_endpoints[index];

            // Only switch if it's a different endpoint
            if endpoint != &self.current_endpoint {
                self.current_endpoint = endpoint.clone();
                self.manual_selected_index = Some(index);

                // Directly switch endpoint in proxy state
                if let Ok(mut state_guard) = proxy_state.lock() {
                    state_guard.switch_endpoint_silent(endpoint.clone());
                }
            }
        }
    }

    fn update_from_tracker(&mut self, tracker: &SharedConnectionTracker) {
        if let Ok(tracker_guard) = tracker.lock() {
            self.active_connections = tracker_guard
                .get_active_connections()
                .values()
                .cloned()
                .collect();
            self.active_connections
                .sort_by(|a, b| b.start_time.cmp(&a.start_time)); // Newest first

            self.total_connections = tracker_guard.get_active_count();
            self.peak_connections = tracker_guard.get_peak_concurrent();
            self.completed_connections = tracker_guard.get_completed_count();
        }
    }

    fn render(&self, f: &mut Frame) {
        // Main layout: split vertically first to reserve space for status bar
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),    // Main content area
                Constraint::Length(1), // Status bar at bottom
            ])
            .split(f.size());

        // Split main content area horizontally: left (health) and right (connections)
        // Left:Right = 2.5:1 ratio for wider left panel
        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(71), Constraint::Percentage(29)]) // 71:29 ≈ 2.5:1
            .split(main_chunks[0]);

        // Render left panel (health monitoring)
        self.render_health_panel(f, content_chunks[0]);

        // Render right panel (active connections)
        self.render_connections_panel(f, content_chunks[1]);

        // Render status bar at bottom (using the reserved space)
        self.render_status_bar(f, main_chunks[1]);
    }

    fn render_health_panel(&self, f: &mut Frame, area: Rect) {
        // Left panel: title with proxy info, subtitle with status info, and endpoints
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4), // Main title with proxy info (increased height)
                Constraint::Length(3), // Enhanced subtitle with status info
                Constraint::Min(0),    // Endpoints table - takes all remaining space
            ])
            .split(area);

        // Main title with proxy information
        let proxy_url = format!("http://localhost:{}", self.proxy_port);
        let title_text = format!("🏥 Health Monitor\n🔗 Proxy: {proxy_url}");
        let title = Paragraph::new(title_text)
            .block(Block::default().borders(Borders::ALL))
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .wrap(Wrap { trim: true });
        f.render_widget(title, chunks[0]);

        // Enhanced subtitle with timing, load, and system status
        let subtitle_text = self.build_subtitle_text();
        // Calculate available width: total width - borders (2) - padding (2) - safety margin (2)
        let available_width = chunks[1].width.saturating_sub(6).max(20) as usize; // Minimum 20 chars
        let truncated_subtitle = self.truncate_text_safely(&subtitle_text, available_width);

        let subtitle = Paragraph::new(truncated_subtitle)
            .block(Block::default().borders(Borders::ALL))
            .style(Style::default().fg(Color::Yellow));
        f.render_widget(subtitle, chunks[1]);

        // Endpoints table - now has more space
        self.render_endpoints_table(f, chunks[2]);
    }

    fn render_endpoints_table(&self, f: &mut Frame, area: Rect) {
        // Ensure we show all endpoints, even if they haven't been health-checked yet
        let mut rows: Vec<Row> = Vec::new();

        for (index, endpoint_url) in self
            .all_endpoints
            .iter()
            .enumerate()
            .skip(self.scroll_offset)
        {
            let status = self.endpoint_health.get(endpoint_url);
            let is_current = endpoint_url == &self.current_endpoint;
            let endpoint_config = self.endpoint_configs.get(endpoint_url);

            // Determine highlighting - cursor position takes precedence for visual feedback
            let is_cursor_position = index == self.cursor_index;
            let is_current_endpoint = endpoint_url == &self.current_endpoint;
            let is_manually_selected = match self.selection_mode {
                SelectionMode::Manual => self.manual_selected_index == Some(index),
                SelectionMode::Auto => false,
            };

            let (status_char, latency_text) = if let Some(status) = status {
                if status.available {
                    ("OK", format!("{}ms", status.latency))
                } else if status.error.is_none() {
                    ("--", "checking...".to_string())
                } else {
                    (
                        "XX",
                        status
                            .error
                            .as_ref()
                            .map(|e| {
                                if e.contains("timeout") {
                                    "timeout"
                                } else {
                                    "error"
                                }
                            })
                            .unwrap_or("error")
                            .to_string(),
                    )
                }
            } else {
                ("--", "checking...".to_string())
            };

            // Build status column with status and markers only
            let current_marker = if is_current { "*" } else { "" }; // ASCII star for active
            let manual_marker =
                if self.selection_mode == SelectionMode::Manual && is_manually_selected {
                    ">" // ASCII arrow for manual selection
                } else {
                    ""
                };

            // Use custom name if available, otherwise fallback to generated
            let endpoint_name = if let Some(config) = endpoint_config {
                config.name.clone()
            } else {
                // Fallback for old format or missing config
                endpoint_url
                    .replace("https://", "")
                    .split('.')
                    .next()
                    .unwrap_or("")
                    .to_uppercase()
            };

            // Generate proper Unicode sparkline for this endpoint
            let raw_sparkline = if let Some(status) = status {
                let sparkline_result = self.generate_sparkline(&status.latency_history);
                if sparkline_result.is_empty() {
                    "▁▁▁▁▁".to_string()
                } else {
                    sparkline_result
                }
            } else {
                "▁▁▁▁▁".to_string() // Default when no data
            };

            let sparkline = raw_sparkline;

            // Build clean status column - only essential status info
            let mut status_content = status_char.to_string();

            // Add important markers
            if !current_marker.is_empty() {
                status_content.push(' ');
                status_content.push_str(current_marker);
            }
            if !manual_marker.is_empty() {
                status_content.push(' ');
                status_content.push_str(manual_marker);
            }

            let row = Row::new(vec![
                ratatui::widgets::Cell::from(status_content),
                ratatui::widgets::Cell::from(endpoint_name),
                ratatui::widgets::Cell::from(latency_text),
                ratatui::widgets::Cell::from(sparkline),
            ]);

            // Apply different highlight styles based on endpoint state
            let styled_row = if is_cursor_position {
                // Cursor position - blue background for navigation
                row.style(
                    Style::default()
                        .bg(Color::Blue)
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )
            } else if is_current_endpoint {
                // Currently active endpoint - green text
                row.style(
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )
            } else if is_manually_selected {
                // Manually selected endpoint - yellow text
                row.style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                // Normal endpoint - default style
                row
            };

            rows.push(styled_row);
        }

        // Optimized column width distribution - status column is now much cleaner
        let constraints = [
            Constraint::Ratio(1, 10), // Status column gets 10% (simplified)
            Constraint::Ratio(3, 10), // Endpoint name gets 30%
            Constraint::Ratio(2, 10), // Latency gets 20%
            Constraint::Ratio(4, 10), // Sparkline gets 40%
        ];

        let table = Table::new(rows)
            .widths(&constraints)
            .header(
                Row::new(vec![
                    ratatui::widgets::Cell::from("Status"),
                    ratatui::widgets::Cell::from("Endpoint"),
                    ratatui::widgets::Cell::from("Latency"),
                    ratatui::widgets::Cell::from(
                        ratatui::text::Line::from("Trend").alignment(Alignment::Center),
                    ),
                ])
                .style(Style::default().add_modifier(Modifier::BOLD)),
            )
            .column_spacing(1) // Minimal spacing between columns
            .block(Block::default().borders(Borders::ALL).title("Endpoints"));

        f.render_widget(table, area);
    }

    fn render_connections_panel(&self, f: &mut Frame, area: Rect) {
        let title = format!("🔗 Active Connections ({})", self.active_connections.len());

        if self.active_connections.is_empty() {
            let no_connections = Paragraph::new("No active connections")
                .block(Block::default().borders(Borders::ALL).title(title))
                .style(Style::default().fg(Color::Gray));
            f.render_widget(no_connections, area);
            return;
        }

        let items: Vec<ListItem> = self
            .active_connections
            .iter()
            .map(|conn| {
                // Get custom name for this endpoint
                let endpoint_name = if let Some(config) = self.endpoint_configs.get(&conn.endpoint)
                {
                    config.name.clone()
                } else {
                    // Fallback for old format
                    conn.endpoint
                        .replace("https://", "")
                        .split('.')
                        .next()
                        .unwrap_or("")
                        .to_uppercase()
                };

                let duration = conn.duration();

                // Show real connection status instead of fake progress
                let status_indicator = match conn.status {
                    ConnectionStatus::Connecting => "🔗 Connecting...",
                    ConnectionStatus::Processing => "⚡ Processing...",
                    ConnectionStatus::Finishing => "✅ Finishing...",
                };

                // Simple duration-based activity indicator
                let activity_dots = match (duration / 500) % 4 {
                    0 => "   ",
                    1 => ".  ",
                    2 => ".. ",
                    3 => "...",
                    _ => "   ",
                };

                let content = format!(
                    "{} → {} ({:.1}s)\n├─ Status: {}\n└─ Active: {}{}",
                    &conn.id[4..10], // Short ID
                    endpoint_name,
                    duration as f64 / 1000.0,
                    status_indicator,
                    if duration < 60000 { "🟢" } else { "🟡" }, // Green for < 1min, yellow for longer
                    activity_dots
                );

                ListItem::new(Text::from(content)).style(Style::default().fg(Color::White))
            })
            .collect();

        let connections_list =
            List::new(items).block(Block::default().borders(Borders::ALL).title(title));

        f.render_widget(connections_list, area);
    }

    fn render_status_bar(&self, f: &mut Frame, area: Rect) {
        // Build mode indicator with current selection
        let mode_indicator = match self.selection_mode {
            SelectionMode::Auto => "🤖 AUTO".to_string(),
            SelectionMode::Manual => {
                if let Some(index) = self.manual_selected_index {
                    format!("🎯 MANUAL[{}]", index + 1)
                } else {
                    "🎯 MANUAL".to_string()
                }
            }
        };

        let scroll_hint = if self.all_endpoints.len() > 10 {
            " │ [↑↓] 滚动端点"
        } else {
            ""
        };

        let selection_hint = match self.selection_mode {
            SelectionMode::Auto => "",
            SelectionMode::Manual => " │ [↑↓] 选择 [Enter] 确认",
        };

        let status_text = if self.paused {
            match self.selection_mode {
                SelectionMode::Auto => {
                    format!("⏸️  健康检查已暂停 {mode_indicator} │ [Q] 退出 │ [R] 手动检查 │ [P] 恢复健康检查 │ [M] 切换到手动{scroll_hint}")
                }
                SelectionMode::Manual => {
                    format!("⏸️  健康检查已暂停 {mode_indicator} │ [Q] 退出 │ [R] 手动检查 │ [P] 恢复健康检查 │ [M] 切换到自动{selection_hint}")
                }
            }
        } else {
            match self.selection_mode {
                SelectionMode::Auto => {
                    format!("🟢 正在监控 {mode_indicator} │ [Q] 退出 │ [R] 手动检查 │ [P] 暂停健康检查 │ [M] 切换到手动{scroll_hint}")
                }
                SelectionMode::Manual => {
                    format!("🟢 正在监控 {mode_indicator} │ [Q] 退出 │ [R] 手动检查 │ [P] 暂停健康检查 │ [M] 切换到自动{selection_hint}")
                }
            }
        };

        let status =
            Paragraph::new(status_text).style(Style::default().bg(Color::Blue).fg(Color::White));

        f.render_widget(status, area);
    }

    /// Generate a compact Unicode sparkline showing latency trend
    fn generate_sparkline(&self, history: &LatencyHistory) -> String {
        let measurements = history.get_measurements();

        // If we don't have enough data, show a simple waiting indicator
        if measurements.is_empty() {
            return "     ".to_string(); // Empty space, clean look
        }

        if measurements.len() < 2 {
            return "  ▁  ".to_string(); // Simple low bar indicating loading
        }

        // Extract recent latency values (ignore failures for sparkline)
        let recent_latencies: Vec<u64> = measurements
            .iter()
            .filter_map(|m| m.latency)
            .rev() // Most recent first
            .take(6) // Use last 6 measurements for sparkline
            .collect();

        if recent_latencies.len() < 2 {
            return "  ▁  ".to_string(); // Still loading successful measurements
        }

        // Find min and max for normalization
        let min_latency = *recent_latencies.iter().min().unwrap_or(&0);
        let max_latency = *recent_latencies.iter().max().unwrap_or(&100);

        // Avoid division by zero
        let range = if max_latency > min_latency {
            max_latency - min_latency
        } else {
            1
        };

        // Unicode sparkline characters (8 levels)
        let sparkline_chars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

        let mut sparkline = String::new();

        // Generate sparkline from oldest to newest (left to right)
        for latency in recent_latencies.iter().rev() {
            // Normalize to 0-7 range
            let normalized = ((latency - min_latency) * 7 / range) as usize;
            let char_index = normalized.min(7);
            sparkline.push(sparkline_chars[char_index]);
        }

        // Pad to consistent width
        while sparkline.chars().count() < 5 {
            sparkline.push('▁');
        }

        sparkline
    }

    /// Extract endpoint display name from URL and config
    fn get_endpoint_name(&self, endpoint_url: &str) -> String {
        self.endpoint_configs
            .get(endpoint_url)
            .map(|c| c.name.clone())
            .unwrap_or_else(|| {
                // Fallback: extract from URL
                endpoint_url
                    .replace("https://", "")
                    .split('.')
                    .next()
                    .unwrap_or("")
                    .to_uppercase()
            })
    }

    /// Build the subtitle text with status, load, mode, and optional switch info
    fn build_subtitle_text(&self) -> String {
        // If paused, show paused indicator
        if self.paused {
            return "⏸️  健康检查已暂停 - 连接监控继续运行，自动切换已停止".to_string();
        }

        let time_until_next = self
            .next_health_check
            .saturating_duration_since(Instant::now());
        let countdown_secs = time_until_next.as_secs();

        // Check if health check is currently running
        let status_text = if let Some((started_at, estimated_duration)) = self.health_check_running
        {
            let running_time = started_at.elapsed();
            let remaining = estimated_duration.saturating_sub(running_time);
            format!("CHECKING... ({}s left)", remaining.as_secs())
        } else if countdown_secs == 0 {
            "READY".to_string()
        } else {
            format!("Next: {countdown_secs}s")
        };

        // Format load status with icon and connection count
        let (load_icon, load_text) = match self.current_load_level {
            LoadLevel::High => ("🔴", format!("High:{}", self.active_connections_count)),
            LoadLevel::Medium => ("🟡", format!("Med:{}", self.active_connections_count)),
            LoadLevel::Low => ("🟢", format!("Low:{}", self.active_connections_count)),
            LoadLevel::Idle => ("⚪", "Idle".to_string()),
        };

        // Format selection mode indicator
        let mode_text = match self.selection_mode {
            SelectionMode::Auto => "🤖AUTO".to_string(),
            SelectionMode::Manual => {
                if let Some(index) = self.manual_selected_index {
                    format!("🎯MAN[{}]", index + 1) // Simple 1-based numbering
                } else {
                    "🎯MAN".to_string()
                }
            }
        };

        // Add recent switch info if available (dynamic display)
        if let Some(switch) = &self.last_switch {
            let from_name = self.get_endpoint_name(&switch.from);
            let to_name = self.get_endpoint_name(&switch.to);

            // Don't show improvement if from_latency is a placeholder (999999)
            let improvement_text = if switch.from_latency >= 999999 {
                "New".to_string() // Initial connection, no meaningful improvement
            } else if switch.improvement > 0 {
                format!("↓{}ms", switch.improvement)
            } else {
                "±0ms".to_string() // No improvement or got worse
            };

            format!(
                "{status_text} • {load_icon}{load_text} • {mode_text} • 🔄{from_name}→{to_name} ({improvement_text})"
            )
        } else {
            format!("{status_text} • {load_icon}{load_text} • {mode_text}")
        }
    }

    /// Safely truncate text at Unicode boundaries
    fn truncate_text_safely(&self, text: &str, max_len: usize) -> String {
        if text.chars().count() <= max_len {
            return text.to_string();
        }

        let truncate_len = max_len.saturating_sub(3); // Reserve space for "..."
        let truncated: String = text.chars().take(truncate_len).collect();
        format!("{truncated}...")
    }
}

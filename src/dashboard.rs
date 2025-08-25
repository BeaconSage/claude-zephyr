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
use std::collections::HashMap;
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

                // Update active connections from tracker
                _ = tick_interval.tick() => {
                    if !self.paused {
                        self.update_from_tracker(&connection_tracker);
                    }
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
                                KeyCode::Char(c) if c.is_ascii_digit() => {
                                    // Manual endpoint selection (1-9)
                                    if let Some(digit) = c.to_digit(10) {
                                        self.handle_manual_endpoint_selection_by_number(digit as usize, &proxy_state);
                                    }
                                }
                                KeyCode::Char(c) if c.is_ascii_alphabetic() => {
                                    // Manual endpoint selection (A-Z for 10+)
                                    let letter = c.to_ascii_uppercase();
                                    if letter.is_ascii_uppercase() {
                                        let index = (letter as u8 - b'A') as usize + 10; // A=10, B=11, etc.
                                        self.handle_manual_endpoint_selection_by_index(index, &proxy_state);
                                    }
                                }
                                KeyCode::Up => {
                                    self.scroll_offset = self.scroll_offset.saturating_sub(1);
                                }
                                KeyCode::Down => {
                                    if self.scroll_offset < self.active_connections.len().saturating_sub(1) {
                                        self.scroll_offset += 1;
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
            _ => {} // Connection events are handled via tracker updates
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

    /// Handle manual endpoint selection by number (1-based)
    fn handle_manual_endpoint_selection_by_number(
        &mut self,
        digit: usize,
        proxy_state: &SharedState,
    ) {
        // Convert 1-based to 0-based index
        let index = digit.saturating_sub(1);
        self.handle_manual_endpoint_selection_by_index(index, proxy_state);
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

    /// Get endpoint key for display (1-9, then A-Z)
    fn get_endpoint_key(&self, index: usize) -> String {
        if index < 9 {
            (index + 1).to_string() // 1-9
        } else {
            let letter_index = index - 9; // A=0, B=1, etc.
            if letter_index < 26 {
                ((b'A' + letter_index as u8) as char).to_string()
            } else {
                "?".to_string() // Fallback for too many endpoints
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
        let title_text = format!("🏥 Health Monitor\n🔗 Proxy: {}", proxy_url);
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

        for (index, endpoint_url) in self.all_endpoints.iter().enumerate() {
            let status = self.endpoint_health.get(endpoint_url);
            let is_current = endpoint_url == &self.current_endpoint;
            let endpoint_config = self.endpoint_configs.get(endpoint_url);

            // Determine if this endpoint should be highlighted based on selection mode
            let is_highlighted = match self.selection_mode {
                SelectionMode::Auto => is_current,
                SelectionMode::Manual => {
                    // In manual mode, highlight the manually selected endpoint
                    self.manual_selected_index == Some(index)
                }
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

            // Build status column with endpoint number, status, country, and markers
            let endpoint_key = self.get_endpoint_key(index); // 1-9, then A-Z
            let current_marker = if is_current { "*" } else { "" }; // ASCII star for active
            let manual_marker = if self.selection_mode == SelectionMode::Manual && is_highlighted {
                ">" // ASCII arrow for manual selection
            } else {
                ""
            };

            // Use custom name and country code if available, otherwise fallback to generated
            let (endpoint_name, country_code) = if let Some(config) = endpoint_config {
                let country = extract_country_code_from_name(&config.name);
                (config.name.clone(), country)
            } else {
                // Fallback for old format or missing config
                let name = endpoint_url
                    .replace("https://", "")
                    .split('.')
                    .next()
                    .unwrap_or("")
                    .to_uppercase();
                let country = extract_country_code_from_name(&name);
                (name, country)
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

            // Build clean ASCII-only status column
            let mut status_content = format!("[{}] {} {}", endpoint_key, status_char, country_code);

            // Add ASCII markers
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

            // Highlight based on selection mode
            let styled_row = if is_highlighted {
                row.style(
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                row
            };

            rows.push(styled_row);
        }

        // Optimized column width distribution
        let constraints = [
            Constraint::Ratio(3, 10), // Status column gets 30% of width
            Constraint::Ratio(2, 10), // Endpoint name gets 20%
            Constraint::Ratio(2, 10), // Latency gets 20%
            Constraint::Ratio(3, 10), // Sparkline gets 30%
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
            .skip(self.scroll_offset)
            .map(|conn| {
                // Get custom name and use simple icon for this endpoint
                let (endpoint_name, flag) =
                    if let Some(config) = self.endpoint_configs.get(&conn.endpoint) {
                        (config.name.clone(), config.display_flag())
                    } else {
                        // Fallback for old format
                        let name = conn
                            .endpoint
                            .replace("https://", "")
                            .split('.')
                            .next()
                            .unwrap_or("")
                            .to_uppercase();
                        (name, "🌐".to_string())
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
                    "{} → {} {}({:.1}s)\n├─ Status: {}\n└─ Active: {}{}",
                    &conn.id[4..10], // Short ID
                    flag,
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

        let status_text = if self.paused {
            format!("⏸️  SYSTEM PAUSED {} │ [Q] 退出 │ [R] 手动检查 │ [P] 恢复监控 │ [M] 模式 │ [1-9A-Z] 选择 │ [↑↓] 滚动", mode_indicator)
        } else {
            format!("🟢 MONITORING {} │ [Q] 退出 │ [R] 手动检查 │ [P] 暂停监控 │ [M] 模式 │ [1-9A-Z] 选择 │ [↑↓] 滚动", mode_indicator)
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

    /// Calculate the actual display width of text in terminal (considering emoji width)
    fn display_width(&self, text: &str) -> usize {
        let mut width = 0;
        let mut chars = text.chars().peekable();

        while let Some(ch) = chars.next() {
            width += match ch {
                // Common single emoji characters - most take 2 terminal columns
                '✅' | '❌' | '⏳' | '⭐' | '🎯' | '🔴' | '🟡' | '🟢' | '⚪' | '🌐' => {
                    2
                }
                // Regional indicator symbols (for country flags like 🇨🇳)
                '\u{1F1E6}'..='\u{1F1FF}' => {
                    // Country flags are made of two regional indicator symbols
                    // Skip the next character if it's also a regional indicator
                    if let Some(next_ch) = chars.peek() {
                        if ('\u{1F1E6}'..='\u{1F1FF}').contains(next_ch) {
                            chars.next(); // Consume the second part of the flag
                        }
                    }
                    2 // Full flag takes 2 terminal columns
                }
                // Other symbols and regular characters
                _ => {
                    if ch.is_ascii() {
                        1
                    } else {
                        2
                    }
                }
            };
        }
        width
    }

    /// Create a fixed-width status column with proper padding
    fn format_status_column(
        &self,
        endpoint_key: String,
        status_icon: String,
        flag: String,
        current_marker: &str,
        manual_marker: &str,
        target_width: usize,
    ) -> String {
        // Build the base content
        let content = format!(
            "[{}] {} {} {} {}",
            endpoint_key, status_icon, flag, current_marker, manual_marker
        );

        // Calculate actual display width
        let actual_width = self.display_width(&content);

        // Pad or truncate to target width
        if actual_width >= target_width {
            // Truncate if too long, but keep essential parts
            let truncated = format!("[{}] {} {}", endpoint_key, status_icon, flag);
            let trunc_width = self.display_width(&truncated);
            if trunc_width <= target_width {
                // Pad the truncated version
                let padding = target_width - trunc_width;
                format!("{}{}", truncated, " ".repeat(padding))
            } else {
                // Extreme truncation - just key and icon
                format!("[{}] {}", endpoint_key, status_icon)
            }
        } else {
            // Pad with spaces to reach target width
            let padding = target_width - actual_width;
            format!("{}{}", content, " ".repeat(padding))
        }
    }

    /// Center-align text within a given width (with proper Unicode width handling)
    fn center_align_text(&self, text: &str, width: usize) -> String {
        let text_width = self.display_width(text);
        if text_width >= width {
            return text.to_string();
        }

        let padding = width - text_width;
        let left_padding = padding / 2;
        let right_padding = padding - left_padding;

        format!(
            "{}{}{}",
            " ".repeat(left_padding),
            text,
            " ".repeat(right_padding)
        )
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
            return "⏸️  SYSTEM PAUSED - Health checks and auto-switching stopped".to_string();
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
                    format!("🎯MAN[{}]", self.get_endpoint_key(index))
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
                "{} • {}{} • {} • 🔄{}→{} ({})",
                status_text, load_icon, load_text, mode_text, from_name, to_name, improvement_text
            )
        } else {
            format!(
                "{} • {}{} • {}",
                status_text, load_icon, load_text, mode_text
            )
        }
    }

    /// Safely truncate text at Unicode boundaries
    fn truncate_text_safely(&self, text: &str, max_len: usize) -> String {
        if text.chars().count() <= max_len {
            return text.to_string();
        }

        let truncate_len = max_len.saturating_sub(3); // Reserve space for "..."
        let truncated: String = text.chars().take(truncate_len).collect();
        format!("{}...", truncated)
    }
}

/// Extract country code from endpoint name
fn extract_country_code_from_name(name: &str) -> String {
    match name.to_uppercase().as_str() {
        s if s.contains("CN") => "CN".to_string(),
        s if s.contains("HK") => "HK".to_string(),
        s if s.contains("JP") => "JP".to_string(),
        s if s.contains("SG") => "SG".to_string(),
        s if s.contains("US") => "US".to_string(),
        s if s.contains("UK") => "UK".to_string(),
        s if s.contains("DE") => "DE".to_string(),
        s if s.contains("FR") => "FR".to_string(),
        _ => "XX".to_string(),
    }
}

# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**IMPORTANT**: Always ask for explicit permission before running `git push` - never push to remote repository without user confirmation.

## Project Overview

**Claude Zephyr** is an automatic endpoint switching tool for Claude API built in **Rust**. It provides automatic endpoint selection, health monitoring, and graceful failover capabilities. The tool automatically routes requests to the best available endpoint based on real-time latency measurements.

## Architecture

### Core Components
- **src/main.rs**: Application entry point with CLI argument handling
- **src/config.rs**: Configuration management and validation for endpoint groups
- **src/health.rs**: Health check logic using Claude CLI
- **src/proxy.rs**: HTTP proxy server and request handling
- **src/dashboard.rs**: TUI dashboard for real-time monitoring
- **src/health_orchestrator.rs**: Health check orchestration and endpoint switching logic
- **src/connection_tracker.rs**: Active connection tracking and management with intelligent cleanup
- **src/signal_handler.rs**: Signal handling and graceful shutdown management
- **config.toml**: Configuration file for endpoint groups, auth tokens, and settings

### Key Features
- **Real API Validation**: Uses Claude CLI for accurate health checks
- **TUI Dashboard**: Interactive terminal dashboard for real-time monitoring
- **Automatic Switching**: Automatic endpoint selection based on latency
- **Manual Control**: Switch between auto and manual endpoint selection modes
- **Graceful Failover**: Seamless switching with active connection tracking
- **Multi-Group Support**: Configure multiple endpoint groups with different auth tokens
- **Dynamic Health Checks**: Adaptive check frequency based on connection load
  - Configurable min/max intervals with smooth scaling between them
  - High load: uses min_interval for quick detection
  - Idle periods: gradually increases to max_interval for cost savings (up to 1 hour by default)
- **Connection Tracking**: Monitor active connections and their status
- **Intelligent Connection Cleanup**: Automatic detection and cleanup of interrupted connections
- **Graceful Shutdown**: SIGINT/SIGTERM signal handling with proper cleanup
- **Configuration-Driven**: All settings in TOML configuration file

## Setup and Configuration

### Prerequisites
1. **Rust** (latest stable version)
2. **Claude CLI** installed and accessible
3. **Valid Anthropic Auth Token**

### Configuration
Create a `config.toml` file in the project root:

```toml
[server]
port = 8080
switch_threshold_ms = 50
graceful_switch_timeout_ms = 30000

[[groups]]
name = "primary-provider"
auth_token_env = "AUTH_TOKEN_MAIN"
default = true
endpoints = [
    { url = "https://api.provider-a.com", name = "Provider-A-1" },
    { url = "https://api2.provider-a.com", name = "Provider-A-2" }
]

[health_check]
interval_seconds = 120
timeout_seconds = 15
auth_token = "your-anthropic-auth-token-here"
claude_binary_path = "/path/to/claude/binary"
```

## Development Commands

### Build and Run
```bash
# Build in debug mode
cargo build

# Build optimized release
cargo build --release

# Run the server
cargo run

# Run optimized version (command line mode)
./target/release/claude-zephyr

# Run with TUI dashboard
./target/release/claude-zephyr --dashboard

# Run health check timing test
./target/release/claude-zephyr --test-timing
```

### Development
```bash
# Check code formatting
cargo fmt --check

# Run linter
cargo clippy

# Run tests (when available)
cargo test
```

## Usage

### Server Operations
```bash
# Start the proxy server (uses config.toml)
cargo run

# Start with TUI dashboard
cargo run -- --dashboard

# Check server status
curl http://localhost:8080/status

# Health check endpoint
curl http://localhost:8080/health
```

### Monitoring
```bash
# View detailed endpoint status
curl http://localhost:8080/status | jq .

# Monitor logs
tail -f /var/log/claude-zephyr.log

# Use the monitoring script
./monitor.sh
```

## Configuration Reference

### Server Section
- `port`: Server listening port (default: 8080)
- `switch_threshold_ms`: Minimum latency improvement to trigger switch (default: 50ms)
- `graceful_switch_timeout_ms`: Max time to wait for graceful switch (default: 30s)

### Health Check Section
- `interval_seconds`: Health check frequency (default: 120s)
- `min_interval_seconds`: Minimum interval for dynamic scaling (default: 30s)  
- `max_interval_seconds`: Maximum interval for dynamic scaling (default: 3600s / 1 hour)
- `timeout_seconds`: Health check timeout (default: 15s)
- `dynamic_scaling`: Enable adaptive check frequency based on connection load (default: false)
- `claude_binary_path`: Path to Claude CLI binary (default: "claude")

### Endpoints
- Array of API endpoint URLs to proxy to
- Listed in order of preference
- All endpoints are checked regularly

## Health Check Mechanism

The system uses **real Claude CLI calls** for health validation:
- Executes `claude -p "test"` against each endpoint
- Measures response latency and validates success
- Automatically switches to the fastest available endpoint
- Handles authentication failures and API errors

## Endpoint Management

### Automatic Switching
- Continuous monitoring of all endpoints
- Latency-based selection with configurable threshold
- Graceful switching waits for active requests to complete
- Immediate failover for completely failed endpoints

### Status Information
The `/status` endpoint provides:
- Current active endpoint
- Health status of all endpoints
- Response latencies
- Active connection count
- Configuration summary

## Environment Setup

For Claude Code integration:
```bash
export ANTHROPIC_BASE_URL="http://localhost:8080"
export ANTHROPIC_AUTH_TOKEN="your-auth-token-here"
```

## Project Structure
```
claude-zephyr/
├── src/
│   ├── main.rs          # Application entry point
│   ├── config.rs        # Configuration management
│   ├── health.rs        # Health check logic
│   └── proxy.rs         # HTTP proxy implementation
├── config.toml          # Configuration file
├── Cargo.toml           # Rust project configuration
├── CLAUDE.md            # This documentation
├── README.md            # Project overview
├── monitor.sh           # Monitoring script
└── build.sh             # Build script
```

## Error Handling

The system handles various error conditions:
- **Network timeouts**: Automatic retry with other endpoints
- **API authentication failures**: Logged and endpoint marked as failed
- **Claude CLI errors**: Graceful degradation and error reporting
- **Configuration errors**: Validation on startup with clear error messages

## Logging

The server provides structured logging:
- **INFO**: Normal operations, endpoint switches, health check results
- **WARN**: Health check failures, degraded performance
- **ERROR**: Critical failures, configuration issues

## Important Notes

- The server requires a valid Claude CLI installation
- Auth tokens are stored in the configuration file (ensure proper security)
- Health checks consume minimal API tokens (using shortest possible prompts)
- The system is designed for high availability and automatic recovery
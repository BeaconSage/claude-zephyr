# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0-alpha.1] - 2025-08-27

### Added
- Dashboard mode is now the default user experience
- Added `--headless` flag for development and automation scenarios

### Changed  
- **BREAKING**: Default behavior now launches interactive dashboard instead of headless mode
- Simplified user documentation to focus on dashboard interface
- Removed complex command-line usage examples from user-facing docs

### Documentation
- Updated README.md and README.zh.md for new default behavior
- Added comprehensive release process documentation (RELEASE_PROCESS.md)
- Established develop branch for ongoing development

## [0.1.0] - TBD

### Added
- Initial release of Claude Zephyr
- Automatic endpoint switching based on latency measurement
- Real-time health monitoring using Claude CLI
- Interactive TUI dashboard for monitoring
- Multi-language support (English/Chinese) 
- Configurable endpoint groups with separate auth tokens
- Dynamic health check intervals based on system load
- Connection tracking and active request monitoring
- Manual and automatic endpoint selection modes
- Cost optimization through intelligent check frequency scaling

### Technical Features
- Built with Rust for performance and safety
- Async/await architecture using Tokio
- HTTP proxy implementation with Hyper
- Terminal UI built with Ratatui
- TOML-based configuration management
- Environment variable support for sensitive data
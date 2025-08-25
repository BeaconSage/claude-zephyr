# Contributing to Claude Zephyr

Thank you for your interest in contributing to Claude Zephyr!

## How to Contribute

### Reporting Issues
- Check existing issues before creating new ones
- Provide clear steps to reproduce the problem
- Include system information (OS, Rust version, Claude CLI version)

### Submitting Code Changes
1. Fork the repository
2. Create a feature branch: `git checkout -b feature-name`
3. Make your changes
4. Test your changes: `cargo test`
5. Format code: `cargo fmt`
6. Check code: `cargo clippy`
7. Commit with clear messages
8. Submit a pull request

### Code Guidelines
- Follow Rust naming conventions
- Add comments for complex logic
- Write tests for new features
- Update documentation if needed

### Development Setup
```bash
# Clone and setup
git clone your-fork-url
cd claude-zephyr
cargo build

# Run tests
cargo test

# Run with dashboard
cargo run -- --dashboard
```

## Questions?

Feel free to open an issue for any questions about contributing.
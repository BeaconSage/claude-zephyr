# Contributing to Claude Zephyr

Thank you for your interest in contributing to Claude Zephyr!

## Git Flow Workflow

We use Git Flow with the following branch structure:

### Main Branches
- **`master`**: Production releases only
- **`develop`**: Main development branch

### Supporting Branches
- **`feature/xxx`**: New feature development
- **`release/x.x.x`**: Release preparation
- **`hotfix/xxx`**: Critical bug fixes

### Basic Workflow
1. **Feature Development**: Create feature branches from `develop`
2. **Release Preparation**: Create release branches from `develop`
3. **Production Release**: Merge release branches to `master`
4. **Emergency Fixes**: Create hotfix branches from `master`

## How to Contribute

### Reporting Issues
- Check existing issues before creating new ones
- Provide clear steps to reproduce the problem
- Include system information (OS, Rust version, Claude CLI version)

### Submitting Code Changes
1. Fork the repository
2. Create a feature branch from `develop`: `git checkout develop && git checkout -b feature-name`
3. Make your changes
4. Test your changes: `cargo test`
5. Format code: `cargo fmt`
6. Check code: `cargo clippy`
7. Commit with clear messages
8. Submit a pull request to `develop`

### Code Quality
- Pre-commit hooks automatically run format and quality checks
- All commits must pass: `cargo fmt`, `cargo clippy`, `cargo check`, and tests
- Follow Rust naming conventions
- Add comments for complex logic
- Write tests for new features
- Update documentation if needed

### Development Setup
```bash
# Clone and setup
git clone your-fork-url
cd claude-zephyr

# Setup pre-commit hooks (one-time)
./scripts/setup-hooks.sh

# Build and test
cargo build
cargo test

# Run with default dashboard mode
cargo run
```

## Questions?

Feel free to open an issue for any questions about contributing.
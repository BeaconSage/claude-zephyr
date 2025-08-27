# Release Process

This document outlines the standardized release process for Claude Zephyr.

## Version Strategy

We follow [Semantic Versioning (SemVer)](https://semver.org/) with pre-release identifiers:

### Version Format
```
MAJOR.MINOR.PATCH[-PRERELEASE[.BUILD]]
```

### Pre-release Lifecycle
```
0.1.0-alpha.1    → Alpha phase (new features, breaking changes possible)
0.1.0-alpha.2    → Alpha iterations
0.1.0-beta.1     → Beta phase (feature freeze, stability focus)
0.1.0-rc.1       → Release candidate (final testing)
0.1.0            → Stable release
```

## Branch Strategy

### Main Branches
- **`master`**: Stable releases only. Production-ready code.
- **`develop`**: Main development branch. Integration of new features.

### Supporting Branches
- **`feature/xxx`**: New feature development
- **`release/x.x.x`**: Release preparation (optional)
- **`hotfix/xxx`**: Critical bug fixes

## Release Workflow

### 1. Alpha Release Process

**From develop branch:**
```bash
# 1. Ensure develop is up to date
git checkout develop
git pull origin develop

# 2. Update version in Cargo.toml
# version = "0.1.0-alpha.1"

# 3. Update CHANGELOG.md
# Add new changes under [Unreleased] → [0.1.0-alpha.1]

# 4. Commit version bump
git add .
git commit -m "chore: bump version to 0.1.0-alpha.1"

# 5. Merge to master for release
git checkout master
git merge develop --no-ff -m "merge: prepare 0.1.0-alpha.1 release"

# 6. Create and push tag
git tag -a v0.1.0-alpha.1 -m "Release v0.1.0-alpha.1

## Features
- Dashboard mode is now default
- Added --headless flag for development
- Improved user experience and documentation

## Alpha Notice
This is an alpha release. Features may change and bugs are expected.
Please report issues on GitHub."

git push origin master
git push origin v0.1.0-alpha.1

# 7. Create GitHub Release (see below)
```

### 2. Beta/RC Release Process

**Similar to alpha, but:**
- Feature freeze (no new features)
- Focus on bug fixes and stability
- More thorough testing

### 3. Stable Release Process

**Final release:**
- All tests passing
- Documentation updated
- No known critical bugs
- Community feedback addressed

## GitHub Release Creation

### Release Template

**Title**: `Claude Zephyr v0.1.0-alpha.1`

**Body**:
```markdown
## 🚀 What's New

- **Dashboard Mode Default**: Claude Zephyr now starts with the interactive dashboard by default
- **Developer Mode**: Added `--headless` flag for development and automation scenarios
- **Improved Documentation**: Streamlined usage instructions focusing on the dashboard experience

## 📋 Features

- ✅ Real-time endpoint monitoring with TUI dashboard
- ✅ Automatic endpoint switching based on latency
- ✅ Manual endpoint selection with keyboard controls
- ✅ Connection tracking and health monitoring
- ✅ Dynamic health check intervals

## 🔧 Usage

```bash
# Default: Interactive dashboard mode
./target/release/claude-zephyr

# Development mode
./target/release/claude-zephyr --headless
```

## ⚠️ Alpha Notice

This is an **alpha release** intended for early testing and feedback. 

**What this means:**
- Core functionality is implemented and working
- Some features may change in future releases
- Please report any issues you encounter
- Not recommended for production use yet

## 🐛 Known Issues

- [List any known issues or limitations]

## 📥 Installation

[Installation instructions]

## 🤝 Contributing

We welcome feedback and contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## 📜 Changelog

See [CHANGELOG.md](CHANGELOG.md) for detailed changes.
```

### Release Checklist

- [ ] Version updated in Cargo.toml
- [ ] CHANGELOG.md updated
- [ ] All tests passing
- [ ] Documentation updated
- [ ] Git tag created
- [ ] GitHub Release published
- [ ] Announcement (if applicable)

## Hotfix Process

For critical bugs in production:

```bash
# 1. Create hotfix branch from master
git checkout master
git checkout -b hotfix/critical-bug-fix

# 2. Make the fix
# ...

# 3. Update version (patch increment)
# 0.1.0 → 0.1.1

# 4. Merge to master and develop
git checkout master
git merge hotfix/critical-bug-fix --no-ff
git tag v0.1.1
git checkout develop
git merge master

# 5. Push and create release
git push origin master develop
git push origin v0.1.1
```

## Automation Goals

Future improvements:
- [ ] Automated version bumping
- [ ] Automated changelog generation
- [ ] CI/CD integration for releases
- [ ] Automated testing before releases

## Contact

For questions about the release process, please open an issue or contact the maintainers.
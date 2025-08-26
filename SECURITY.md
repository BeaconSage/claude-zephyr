# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

If you discover a security vulnerability, please report it by emailing the maintainers directly rather than opening a public issue.

**Please do not report security vulnerabilities through public GitHub issues.**

When reporting a vulnerability, please include:

- Description of the vulnerability
- Steps to reproduce the issue
- Potential impact
- Any suggested fixes

We will respond to security reports within 48 hours and will keep you updated on the progress of fixing the issue.

## Security Considerations

### Configuration Security
- Never commit actual API tokens to the repository
- Use `.env` files for sensitive configuration
- Ensure proper file permissions for configuration files

### Network Security
- All API communications use HTTPS
- Local proxy server runs on localhost by default
- No external network access beyond configured API endpoints

### Dependencies
- All dependencies are regularly updated
- We use `cargo audit` to check for known vulnerabilities
- Dependencies are locked with `Cargo.lock` for reproducible builds
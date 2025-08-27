#!/bin/bash
# Setup development hooks for Claude Zephyr

echo "🔧 Setting up pre-commit hooks..."

# Copy pre-commit hook
cp scripts/pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit

echo "✅ Pre-commit hooks installed successfully!"
echo "📝 All commits will now run format and quality checks automatically."
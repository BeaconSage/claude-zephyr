#!/bin/bash

# Claude Zephyr 构建脚本

set -e

echo "🚀 开始构建 Claude Zephyr..."

# 检查 Rust 是否安装
if ! command -v cargo &> /dev/null; then
    echo "❌ 错误: 需要安装 Rust。请访问 https://rustup.rs/ 安装 Rust。"
    exit 1
fi

# 检查配置文件是否存在
if [ ! -f "config.toml" ]; then
    echo "⚠️  警告: config.toml 文件不存在，将创建示例配置文件"
    cat > config.toml << 'EOF'
# Claude Zephyr 配置文件

[server]
port = 8080
switch_threshold_ms = 50
graceful_switch_timeout_ms = 30000

# API 端点列表 - 按优先级排序
[[groups]]
name = "primary-provider"
auth_token_env = "AUTH_TOKEN_MAIN"
default = true
endpoints = [
    { url = "https://api.your-provider.com", name = "Provider-Main" },
    { url = "https://backup.your-provider.com", name = "Provider-Backup" }
]

[health_check]
interval_seconds = 120
timeout_seconds = 15
auth_token = "your-claude-auth-token-here"
claude_binary_path = "/Users/tella/.claude/local/claude"
EOF
    echo "📝 已创建 config.toml 示例文件，请修改其中的 auth token 和 Claude 二进制路径"
fi

# 检查 Claude CLI 是否可用
if ! command -v claude &> /dev/null; then
    echo "⚠️  警告: Claude CLI 未找到，请确保已安装 Claude CLI 并在 PATH 中"
fi

echo "🔧 正在检查代码格式..."
cargo fmt --check || {
    echo "📝 自动格式化代码..."
    cargo fmt
}

echo "🔍 运行代码检查..."
cargo clippy -- -D warnings || {
    echo "❌ 代码检查失败，请修复上述警告"
    exit 1
}

echo "🏗️  构建发布版本..."
cargo build --release

echo "✅ 构建完成！"
echo ""
echo "📋 下一步："
echo "1. 编辑 config.toml 文件，设置正确的 auth token 和 Claude 路径"
echo "2. 运行服务: cargo run 或 ./target/release/claude-zephyr"
echo "3. 设置环境变量: export ANTHROPIC_BASE_URL=\"http://localhost:8080\""
echo ""
echo "🔗 监控页面:"
echo "- 状态: http://localhost:8080/status"
echo "- 健康检查: http://localhost:8080/health"
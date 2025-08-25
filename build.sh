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
    echo "⚠️  警告: config.toml 文件不存在，将从示例文件创建"
    if [ -f "config.toml.example" ]; then
        cp config.toml.example config.toml
        echo "📝 已从 config.toml.example 创建 config.toml，请修改其中的配置"
    else
        echo "❌ 错误: 未找到 config.toml.example 文件"
        exit 1
    fi
fi

# 检查 .env 文件
if [ ! -f ".env" ]; then
    echo "⚠️  警告: .env 文件不存在，将从示例文件创建"
    if [ -f ".env.example" ]; then
        cp .env.example .env
        echo "📝 已从 .env.example 创建 .env，请填入你的认证令牌"
    else
        echo "❌ 错误: 未找到 .env.example 文件"
        exit 1
    fi
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
# 只检查关键错误，允许格式相关的警告
cargo clippy --all-targets --all-features -- -D clippy::correctness -D clippy::suspicious -D clippy::complexity -W clippy::perf -A dead_code -A unused -A clippy::uninlined_format_args -A clippy::empty_line_after_doc_comments || {
    echo "❌ 代码检查失败，请修复上述警告"
    exit 1
}

echo "🏗️  构建发布版本..."
cargo build --release

echo "✅ 构建完成！"
echo ""
echo "📋 下一步："
echo "1. 编辑 .env 文件，填入你的认证令牌"
echo "2. 运行服务: ./target/release/claude-zephyr --dashboard (推荐)"
echo "3. 设置环境变量: export ANTHROPIC_BASE_URL=\"http://localhost:8080\""
echo ""
echo "🔗 监控页面:"
echo "- 状态: http://localhost:8080/status"
echo "- 健康检查: http://localhost:8080/health"
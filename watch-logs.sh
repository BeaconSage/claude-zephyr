#!/bin/bash

# 快速启动日志监控
# 直接运行: ./watch-logs.sh

# 自动查找当前日志文件
LOG_DIR="logs"
TODAY_LOG="$LOG_DIR/claude-zephyr.$(date +%Y-%m-%d)"
MAIN_LOG="$LOG_DIR/claude-zephyr.log"

# 确定使用哪个日志文件
if [ -f "$TODAY_LOG" ]; then
    LOG_FILE="$TODAY_LOG"
elif [ -f "$MAIN_LOG" ]; then
    LOG_FILE="$MAIN_LOG"
else
    LOG_FILE="$TODAY_LOG"  # 默认等待今天的日志文件
fi

# 检查日志文件
if [ ! -f "$LOG_FILE" ]; then
    echo "❌ 日志文件不存在，正在等待服务启动..."
    echo "💡 请在另一个终端运行:"
    echo "   cargo run                  # TUI仪表板模式"
    echo "   cargo run -- --headless    # 控制台模式"
    echo ""
    echo "⏰ 等待日志文件创建: $LOG_FILE"
    
    # 等待日志文件创建，最多等待60秒
    count=0
    while [ ! -f "$LOG_FILE" ] && [ $count -lt 60 ]; do
        sleep 1
        count=$((count + 1))
        echo -n "."
        
        # 每10秒检查一次是否有其他日志文件
        if [ $((count % 10)) -eq 0 ]; then
            if [ -f "$TODAY_LOG" ] && [ "$LOG_FILE" != "$TODAY_LOG" ]; then
                LOG_FILE="$TODAY_LOG"
                break
            elif [ -f "$MAIN_LOG" ] && [ "$LOG_FILE" != "$MAIN_LOG" ]; then
                LOG_FILE="$MAIN_LOG" 
                break
            fi
        fi
    done
    echo ""
    
    if [ ! -f "$LOG_FILE" ]; then
        echo ""
        echo "⚠️  超时等待日志文件创建"
        echo "📋 请检查配置文件 config.toml 中的日志设置:"
        echo "   [logging]"
        echo "   file_enabled = true"
        echo ""
        echo "🔍 查找现有日志文件:"
        find logs -name "claude-zephyr*" -type f 2>/dev/null | head -5
        exit 1
    fi
    
    echo "✅ 检测到日志文件，开始监控..."
    echo ""
fi

# 清屏并显示标题
clear
echo "🔍 Claude Zephyr 实时日志"
echo "========================="
echo "📝 文件: $LOG_FILE"
echo "⏰ 时间: $(date)"
echo "📺 按 Ctrl+C 退出"
echo ""

# 显示最近的日志统计
if [ -f "$LOG_FILE" ]; then
    retry_count=$(tail -n 100 "$LOG_FILE" 2>/dev/null | grep -c "🔁" || echo "0")
    success_count=$(tail -n 100 "$LOG_FILE" 2>/dev/null | grep -c "✅" || echo "0") 
    error_count=$(tail -n 100 "$LOG_FILE" 2>/dev/null | grep -c "❌" || echo "0")
    
    echo "📊 最近活动 (100行): 🔁$retry_count 次重试, ✅$success_count 次成功, ❌$error_count 次错误"
fi

echo "------------------------"
echo ""

# 开始实时监控（彩色输出）
tail -f "$LOG_FILE" | sed -E \
    -e "s/(🔁)/$(echo -e '\033[0;33m')\1$(echo -e '\033[0m')/g" \
    -e "s/(✅)/$(echo -e '\033[0;32m')\1$(echo -e '\033[0m')/g" \
    -e "s/(❌)/$(echo -e '\033[0;31m')\1$(echo -e '\033[0m')/g" \
    -e "s/(⚠️)/$(echo -e '\033[0;33m')\1$(echo -e '\033[0m')/g" \
    -e "s/(🔄)/$(echo -e '\033[0;35m')\1$(echo -e '\033[0m')/g" \
    -e "s/(🚀)/$(echo -e '\033[0;36m')\1$(echo -e '\033[0m')/g" \
    -e "s/(🏥)/$(echo -e '\033[0;34m')\1$(echo -e '\033[0m')/g" \
    -e "s/(🔗)/$(echo -e '\033[0;32m')\1$(echo -e '\033[0m')/g" \
    -e "s/(ERROR)/$(echo -e '\033[0;31m')\1$(echo -e '\033[0m')/g" \
    -e "s/(WARN)/$(echo -e '\033[0;33m')\1$(echo -e '\033[0m')/g" \
    -e "s/(INFO)/$(echo -e '\033[0;32m')\1$(echo -e '\033[0m')/g"
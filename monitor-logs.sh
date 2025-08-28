#!/bin/bash

# Claude Zephyr 实时日志监控脚本
# 使用方法: ./monitor-logs.sh [选项]

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

LOG_FILE="logs/claude-zephyr.log"

# 显示帮助信息
show_help() {
    echo -e "${CYAN}🔍 Claude Zephyr 日志监控工具${NC}"
    echo -e "${CYAN}================================${NC}"
    echo ""
    echo "使用方法: $0 [选项]"
    echo ""
    echo "选项:"
    echo "  -h, --help     显示此帮助信息"
    echo "  -a, --all      显示所有日志 (默认)"
    echo "  -r, --retry    只显示重试相关日志"
    echo "  -e, --error    只显示错误日志"
    echo "  -s, --switch   只显示端点切换日志"
    echo "  -p, --proxy    只显示代理请求日志"
    echo "  -j, --json     JSON格式输出 (需要jq)"
    echo "  -n, --lines N  显示最近N行 (默认: 50)"
    echo ""
    echo "示例:"
    echo "  $0              # 显示所有实时日志"
    echo "  $0 -r           # 只看重试日志"
    echo "  $0 -e           # 只看错误日志"
    echo "  $0 -n 100       # 显示最近100行后开始跟踪"
    echo ""
}

# 检查日志文件
check_log_file() {
    if [ ! -f "$LOG_FILE" ]; then
        echo -e "${RED}❌ 日志文件 $LOG_FILE 不存在${NC}"
        echo -e "${YELLOW}💡 请确保:${NC}"
        echo "   1. 配置文件中 file_enabled = true"
        echo "   2. Claude Zephyr 服务已启动"
        echo "   3. 日志目录有写入权限"
        echo ""
        echo -e "${CYAN}🚀 启动服务: cargo run -- --headless${NC}"
        exit 1
    fi
}

# 显示日志统计
show_stats() {
    echo -e "${CYAN}📊 日志统计 (最近1000行):${NC}"
    local retry_count=$(tail -n 1000 "$LOG_FILE" 2>/dev/null | grep -c "🔁" || echo "0")
    local success_count=$(tail -n 1000 "$LOG_FILE" 2>/dev/null | grep -c "✅" || echo "0")
    local error_count=$(tail -n 1000 "$LOG_FILE" 2>/dev/null | grep -c "❌" || echo "0")
    local switch_count=$(tail -n 1000 "$LOG_FILE" 2>/dev/null | grep -c "🔄" || echo "0")
    
    echo -e "  🔁 重试: ${YELLOW}$retry_count${NC}次"
    echo -e "  ✅ 成功: ${GREEN}$success_count${NC}次"
    echo -e "  ❌ 错误: ${RED}$error_count${NC}次"
    echo -e "  🔄 切换: ${PURPLE}$switch_count${NC}次"
    echo -e "${CYAN}================================${NC}"
}

# 彩色输出函数
colorize_log() {
    sed -E \
        -e "s/(🔁)/\\${YELLOW}\1\\${NC}/g" \
        -e "s/(✅)/\\${GREEN}\1\\${NC}/g" \
        -e "s/(❌)/\\${RED}\1\\${NC}/g" \
        -e "s/(⚠️)/\\${YELLOW}\1\\${NC}/g" \
        -e "s/(🔄)/\\${PURPLE}\1\\${NC}/g" \
        -e "s/(🚀)/\\${CYAN}\1\\${NC}/g" \
        -e "s/(🏥)/\\${BLUE}\1\\${NC}/g" \
        -e "s/(🔗)/\\${GREEN}\1\\${NC}/g" \
        -e "s/(⚙️)/\\${CYAN}\1\\${NC}/g" \
        -e "s/(ERROR)/\\${RED}\1\\${NC}/g" \
        -e "s/(WARN)/\\${YELLOW}\1\\${NC}/g" \
        -e "s/(INFO)/\\${GREEN}\1\\${NC}/g" \
        -e "s/(DEBUG)/\\${BLUE}\1\\${NC}/g"
}

# 默认参数
FILTER=""
LINES=50
JSON_MODE=false

# 解析命令行参数
while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            show_help
            exit 0
            ;;
        -a|--all)
            FILTER=""
            shift
            ;;
        -r|--retry)
            FILTER="🔁"
            shift
            ;;
        -e|--error)
            FILTER="❌|ERROR"
            shift
            ;;
        -s|--switch)
            FILTER="🔄"
            shift
            ;;
        -p|--proxy)
            FILTER="🔗|Proxying"
            shift
            ;;
        -j|--json)
            JSON_MODE=true
            shift
            ;;
        -n|--lines)
            LINES="$2"
            shift 2
            ;;
        *)
            echo -e "${RED}未知选项: $1${NC}"
            echo "使用 -h 或 --help 查看帮助"
            exit 1
            ;;
    esac
done

# 主程序开始
clear
echo -e "${CYAN}🔍 Claude Zephyr 实时日志监控${NC}"
echo -e "${CYAN}==============================${NC}"

# 检查日志文件
check_log_file

# 显示统计信息
show_stats
echo ""

# 显示监控信息
if [ -n "$FILTER" ]; then
    echo -e "${YELLOW}🎯 过滤器: $FILTER${NC}"
else
    echo -e "${GREEN}📺 显示所有日志${NC}"
fi
echo -e "${BLUE}📝 日志文件: $LOG_FILE${NC}"
echo -e "${PURPLE}⏰ 开始时间: $(date)${NC}"
echo -e "${CYAN}──────────────────────────────${NC}"
echo ""

# 开始监控日志
if [ "$JSON_MODE" = true ]; then
    # JSON 模式 (需要 jq)
    if ! command -v jq &> /dev/null; then
        echo -e "${RED}❌ JSON模式需要安装 jq${NC}"
        echo "安装: brew install jq"
        exit 1
    fi
    
    if [ -n "$FILTER" ]; then
        tail -n "$LINES" -f "$LOG_FILE" | grep --line-buffered -E "$FILTER" | jq -r '.'
    else
        tail -n "$LINES" -f "$LOG_FILE" | jq -r '.'
    fi
else
    # 普通模式
    if [ -n "$FILTER" ]; then
        tail -n "$LINES" -f "$LOG_FILE" | grep --line-buffered -E "$FILTER" | colorize_log
    else
        tail -n "$LINES" -f "$LOG_FILE" | colorize_log
    fi
fi
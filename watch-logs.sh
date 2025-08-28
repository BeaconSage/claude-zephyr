#!/bin/bash

# Claude Zephyr 智能日志监控工具
# 简单模式: ./watch-logs.sh
# 高级模式: ./watch-logs.sh [选项]

# 颜色定义
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# 自动查找当前日志文件
LOG_DIR="logs"
TODAY_LOG="$LOG_DIR/claude-zephyr.$(date +%Y-%m-%d)"
MAIN_LOG="$LOG_DIR/claude-zephyr.log"

# 默认参数
FILTER=""
LINES=50
JSON_MODE=false
SHOW_STATS=true
SIMPLE_MODE=true

# 确定使用哪个日志文件
determine_log_file() {
    if [ -f "$TODAY_LOG" ]; then
        LOG_FILE="$TODAY_LOG"
    elif [ -f "$MAIN_LOG" ]; then
        LOG_FILE="$MAIN_LOG"
    else
        LOG_FILE="$TODAY_LOG"  # 默认等待今天的日志文件
    fi
}

# 显示帮助信息
show_help() {
    echo -e "${CYAN}🔍 Claude Zephyr 智能日志监控工具${NC}"
    echo -e "${CYAN}=====================================${NC}"
    echo ""
    echo -e "${GREEN}简单模式（零配置）:${NC}"
    echo "  $0                  # 直接启动，查看所有日志"
    echo ""
    echo -e "${BLUE}高级模式（过滤选项）:${NC}"
    echo "  -h, --help         显示此帮助信息"
    echo "  -a, --all          显示所有日志 (默认)"
    echo "  -p, --proxy        只显示代理请求日志 (🔄)"
    echo "  -r, --retry        只显示重试相关日志 (🔁)"
    echo "  -e, --error        只显示错误日志 (❌)"
    echo "  -s, --switch       只显示端点切换日志 (🔀)"
    echo "  -H, --health       只显示健康检查日志 (🏥)"
    echo ""
    echo -e "${PURPLE}分析模式:${NC}"
    echo "  --proxy-stats      显示详细代理统计分析"
    echo "  --error-analysis   显示错误分析统计"
    echo "  --performance      显示性能分析"
    echo ""
    echo -e "${YELLOW}输出选项:${NC}"
    echo "  -n, --lines N      显示最近N行 (默认: 50)"
    echo "  -j, --json         JSON格式输出"
    echo "  --no-stats         禁用统计显示"
    echo ""
    echo -e "${CYAN}使用示例:${NC}"
    echo "  $0                 # 快速启动，查看所有日志"
    echo "  $0 -p              # 只看代理请求"
    echo "  $0 -r -n 100       # 最近100行重试日志"
    echo "  $0 --proxy-stats   # 代理统计分析"
    echo ""
    echo -e "${GREEN}Emoji图例:${NC}"
    echo "  🔄 代理请求   🔁 重试   ❌ 错误   🔀 端点切换"
    echo "  🏥 健康检查   ✅ 成功   ⚠️  警告   🚀 系统启动"
    echo ""
}

# 检查日志文件
check_log_file() {
    if [ ! -f "$LOG_FILE" ]; then
        echo -e "${RED}❌ 日志文件不存在: $LOG_FILE${NC}"
        echo -e "${YELLOW}💡 请确保:${NC}"
        echo "   1. 配置文件中 file_enabled = true"
        echo "   2. Claude Zephyr 服务已启动"
        echo "   3. 日志目录有写入权限"
        echo ""
        echo -e "${CYAN}🚀 启动命令:${NC}"
        echo "   cargo run                  # TUI仪表板模式"
        echo "   cargo run -- --headless    # 控制台模式"
        echo ""
        echo -e "${BLUE}🔍 查找现有日志文件:${NC}"
        find logs -name "claude-zephyr*" -type f 2>/dev/null | head -5 || echo "   无日志文件"
        return 1
    fi
    return 0
}

# 等待日志文件创建
wait_for_log_file() {
    echo -e "${YELLOW}⏰ 等待日志文件创建: $LOG_FILE${NC}"
    
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
        echo -e "${RED}⚠️  超时等待日志文件创建${NC}"
        check_log_file
        return 1
    fi
    
    echo -e "${GREEN}✅ 检测到日志文件，开始监控...${NC}"
    echo ""
    return 0
}

# 显示日志统计
show_stats() {
    if [ "$SHOW_STATS" = false ]; then
        return
    fi
    
    echo -e "${CYAN}📊 日志统计 (最近1000行):${NC}"
    local retry_count=$(tail -n 1000 "$LOG_FILE" 2>/dev/null | grep -c "🔁" || echo "0")
    local success_count=$(tail -n 1000 "$LOG_FILE" 2>/dev/null | grep -c "✅" || echo "0") 
    local error_count=$(tail -n 1000 "$LOG_FILE" 2>/dev/null | grep -c "❌" || echo "0")
    local proxy_count=$(tail -n 1000 "$LOG_FILE" 2>/dev/null | grep -c "🔄.*Request →" || echo "0")
    local switch_count=$(tail -n 1000 "$LOG_FILE" 2>/dev/null | grep -c "🔀" || echo "0")
    local health_count=$(tail -n 1000 "$LOG_FILE" 2>/dev/null | grep -c "🏥" || echo "0")
    
    echo -e "  🔄 代理请求: ${GREEN}$proxy_count${NC} 次"
    echo -e "  🔁 重试: ${YELLOW}$retry_count${NC} 次"
    echo -e "  ✅ 成功: ${GREEN}$success_count${NC} 次"
    echo -e "  ❌ 错误: ${RED}$error_count${NC} 次"
    echo -e "  🔀 端点切换: ${PURPLE}$switch_count${NC} 次"
    echo -e "  🏥 健康检查: ${BLUE}$health_count${NC} 次"
    echo ""
}

# 代理统计分析
show_proxy_stats() {
    echo -e "${CYAN}🔄 代理请求详细统计分析${NC}"
    echo -e "${CYAN}================================${NC}"
    
    if [ ! -f "$LOG_FILE" ]; then
        echo -e "${RED}❌ 日志文件不存在${NC}"
        return 1
    fi
    
    # 代理请求统计
    local total_requests=$(grep -c "🔄.*Request →" "$LOG_FILE" 2>/dev/null || echo "0")
    local failed_requests=$(grep -c "🔄.*❌.*Request failed" "$LOG_FILE" 2>/dev/null || echo "0")
    local success_rate=0
    
    if [ "$total_requests" -gt 0 ] && [ "$failed_requests" -ge 0 ]; then
        success_rate=$(( (total_requests - failed_requests) * 100 / total_requests ))
    fi
    
    echo -e "${BLUE}📈 总体统计:${NC}"
    echo -e "  总请求数: ${GREEN}$total_requests${NC}"
    echo -e "  失败请求: ${RED}$failed_requests${NC}"
    echo -e "  成功率: ${GREEN}$success_rate%${NC}"
    echo ""
    
    # 端点统计
    echo -e "${BLUE}🎯 端点请求分布:${NC}"
    grep "🔄.*Request →" "$LOG_FILE" 2>/dev/null | \
        sed -E 's/.*Request → ([^ ]+).*/\1/' | \
        sort | uniq -c | sort -nr | head -10 | \
        while read count endpoint; do
            echo -e "  ${GREEN}$count${NC} 次 → $endpoint"
        done
    echo ""
    
    # 错误类型统计
    echo -e "${BLUE}❌ 错误类型分析:${NC}"
    grep "🔄.*❌.*Request failed" "$LOG_FILE" 2>/dev/null | \
        sed -E 's/.*Request failed: [^ ]+ - (.*)/\1/' | \
        sort | uniq -c | sort -nr | head -5 | \
        while read count error; do
            echo -e "  ${RED}$count${NC} 次 → $error"
        done
    echo ""
    
    # 最近的失败请求
    echo -e "${BLUE}🕒 最近的失败请求 (最近5条):${NC}"
    grep "🔄.*❌.*Request failed" "$LOG_FILE" 2>/dev/null | tail -5 | \
        while IFS= read -r line; do
            echo -e "  ${RED}•${NC} $(echo "$line" | sed -E 's/.*([0-9]{2}:[0-9]{2}:[0-9]{2}).*Request failed: ([^ ]+) - (.*)/\1 → \2 (\3)/')"
        done
    
    return 0
}

# 错误分析
show_error_analysis() {
    echo -e "${CYAN}❌ 错误统计分析${NC}"
    echo -e "${CYAN}==================${NC}"
    
    if [ ! -f "$LOG_FILE" ]; then
        echo -e "${RED}❌ 日志文件不存在${NC}"
        return 1
    fi
    
    local total_errors=$(grep -c "❌" "$LOG_FILE" 2>/dev/null || echo "0")
    
    echo -e "${BLUE}📊 错误总览:${NC}"
    echo -e "  总错误数: ${RED}$total_errors${NC}"
    echo ""
    
    echo -e "${BLUE}🏥 健康检查错误:${NC}"
    grep "🏥.*❌" "$LOG_FILE" 2>/dev/null | \
        sed -E 's/.*❌ Endpoint failed: ([^ ]+) - (.*)/\1: \2/' | \
        sort | uniq -c | sort -nr | head -5 | \
        while read count error; do
            echo -e "  ${RED}$count${NC} 次 → $error"
        done
    echo ""
    
    echo -e "${BLUE}🔄 代理错误:${NC}"
    grep "🔄.*❌" "$LOG_FILE" 2>/dev/null | \
        sed -E 's/.*Request failed: ([^ ]+) - (.*)/\1: \2/' | \
        sort | uniq -c | sort -nr | head -5 | \
        while read count error; do
            echo -e "  ${RED}$count${NC} 次 → $error"
        done
}

# 彩色输出函数
colorize_log() {
    sed -E \
        -e "s/(🔁)/\\${YELLOW}\1\\${NC}/g" \
        -e "s/(✅)/\\${GREEN}\1\\${NC}/g" \
        -e "s/(❌)/\\${RED}\1\\${NC}/g" \
        -e "s/(⚠️)/\\${YELLOW}\1\\${NC}/g" \
        -e "s/(🔄)/\\${PURPLE}\1\\${NC}/g" \
        -e "s/(🔀)/\\${PURPLE}\1\\${NC}/g" \
        -e "s/(🚀)/\\${CYAN}\1\\${NC}/g" \
        -e "s/(🏥)/\\${BLUE}\1\\${NC}/g" \
        -e "s/(⚙️)/\\${CYAN}\1\\${NC}/g" \
        -e "s/(ERROR)/\\${RED}\1\\${NC}/g" \
        -e "s/(WARN)/\\${YELLOW}\1\\${NC}/g" \
        -e "s/(INFO)/\\${GREEN}\1\\${NC}/g" \
        -e "s/(DEBUG)/\\${BLUE}\1\\${NC}/g"
}

# 解析命令行参数
while [[ $# -gt 0 ]]; do
    SIMPLE_MODE=false  # 有参数时进入高级模式
    case $1 in
        -h|--help)
            show_help
            exit 0
            ;;
        -a|--all)
            FILTER=""
            shift
            ;;
        -p|--proxy)
            FILTER="🔄.*Request|🔄.*❌.*Request failed"
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
            FILTER="🔀"
            shift
            ;;
        -H|--health)
            FILTER="🏥"
            shift
            ;;
        --proxy-stats)
            determine_log_file
            show_proxy_stats
            exit $?
            ;;
        --error-analysis)
            determine_log_file
            show_error_analysis
            exit $?
            ;;
        --performance)
            echo -e "${YELLOW}性能分析功能开发中...${NC}"
            exit 0
            ;;
        -j|--json)
            JSON_MODE=true
            shift
            ;;
        --no-stats)
            SHOW_STATS=false
            shift
            ;;
        -n|--lines)
            LINES="$2"
            if ! [[ "$LINES" =~ ^[0-9]+$ ]]; then
                echo -e "${RED}错误: --lines 参数必须是数字${NC}"
                exit 1
            fi
            shift 2
            ;;
        *)
            echo -e "${RED}未知选项: $1${NC}"
            echo "使用 -h 或 --help 查看帮助"
            exit 1
            ;;
    esac
done

# 确定日志文件
determine_log_file

# 检查日志文件
if ! check_log_file; then
    if ! wait_for_log_file; then
        exit 1
    fi
fi

# 显示标题
clear
if [ "$SIMPLE_MODE" = true ]; then
    echo -e "${CYAN}🔍 Claude Zephyr 实时日志${NC}"
    echo -e "${CYAN}=========================${NC}"
else
    echo -e "${CYAN}🔍 Claude Zephyr 日志监控 (高级模式)${NC}"
    echo -e "${CYAN}====================================${NC}"
fi

echo -e "${BLUE}📝 文件: $LOG_FILE${NC}"
echo -e "${PURPLE}⏰ 时间: $(date)${NC}"

if [ -n "$FILTER" ]; then
    echo -e "${YELLOW}🎯 过滤器: $FILTER${NC}"
fi

echo -e "${GREEN}📺 按 Ctrl+C 退出${NC}"
echo ""

# 显示统计信息
show_stats

echo -e "${CYAN}────────────────────────────────────${NC}"
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
    # 普通彩色模式
    if [ -n "$FILTER" ]; then
        tail -n "$LINES" -f "$LOG_FILE" | grep --line-buffered -E "$FILTER" | colorize_log
    else
        tail -n "$LINES" -f "$LOG_FILE" | colorize_log
    fi
fi
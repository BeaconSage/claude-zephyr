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
CUSTOM_LOG_FILE=""

# 新增的查看模式参数
VIEW_MODE="tail"        # tail, head, all, around
FOLLOW_MODE=true        # 是否实时跟踪
USE_PAGER=false         # 是否使用分页器
AROUND_LINE=""          # --around 选项的行号

# 确定使用哪个日志文件
determine_log_file() {
    if [ -n "$CUSTOM_LOG_FILE" ]; then
        LOG_FILE="$CUSTOM_LOG_FILE"
    elif [ -f "$TODAY_LOG" ]; then
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
    echo "  -f, --file PATH    指定日志文件路径"
    echo "  -n, --lines N      显示最近N行 (默认: 50)"
    echo "  -j, --json         JSON格式输出"
    echo "  --no-stats         禁用统计显示"
    echo ""
    echo -e "${PURPLE}查看模式:${NC}"
    echo "  --history          查看完整历史日志（不跟踪新内容）"
    echo "  --all-content      显示完整内容然后跟踪新内容"
    echo "  --from-start       从文件开头开始显示"
    echo "  --head N           显示文件开头N行"
    echo "  --around LINE      显示指定行周围内容"
    echo "  --pager            使用分页器浏览（支持搜索和滚动）"
    echo "  --no-follow        仅查看内容，不跟踪新日志"
    echo ""
    echo -e "${CYAN}使用示例:${NC}"
    echo "  $0                 # 快速启动，查看所有日志"
    echo "  $0 -p              # 只看代理请求"
    echo "  $0 -r -n 100       # 最近100行重试日志"
    echo "  $0 -f logs/claude-zephyr.2025-08-27  # 监控指定日期的日志"
    echo "  $0 --file custom.log -p              # 指定文件+过滤代理请求"
    echo ""
    echo -e "${CYAN}历史日志查看:${NC}"
    echo "  $0 -f old.log --history --pager      # 分页浏览完整历史日志"
    echo "  $0 -f old.log --head 100 -p          # 查看开头100行代理请求"
    echo "  $0 -f old.log --all-content -e       # 完整内容+跟踪错误日志"
    echo "  $0 -f old.log --around 500 --no-follow  # 查看第500行周围内容"
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
        if [ -n "$CUSTOM_LOG_FILE" ]; then
            # 用户指定了自定义文件但文件不存在
            echo -e "${RED}❌ 指定的日志文件不存在: $LOG_FILE${NC}"
            echo -e "${YELLOW}💡 请检查:${NC}"
            echo "   1. 文件路径是否正确"
            echo "   2. 文件是否存在和可读"
            echo "   3. 是否有访问权限"
            echo ""
            
            # 尝试提供一些有用的建议
            local dir_name=$(dirname "$LOG_FILE")
            if [ -d "$dir_name" ]; then
                echo -e "${BLUE}🔍 在目录 $dir_name 中找到的日志文件:${NC}"
                find "$dir_name" -name "*log*" -type f 2>/dev/null | head -5 || echo "   无相关日志文件"
            else
                echo -e "${BLUE}🔍 在当前目录中找到的日志文件:${NC}"
                find . -name "*log*" -type f 2>/dev/null | head -5 || echo "   无日志文件"
            fi
        else
            # 自动检测模式下文件不存在
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
        fi
        return 1
    fi
    
    # 检查文件是否可读
    if [ ! -r "$LOG_FILE" ]; then
        echo -e "${RED}❌ 无法读取日志文件: $LOG_FILE${NC}"
        echo -e "${YELLOW}💡 权限问题，请检查文件访问权限${NC}"
        return 1
    fi
    
    return 0
}

# 等待日志文件创建
wait_for_log_file() {
    # 如果用户指定了自定义文件，不要等待，直接返回失败
    if [ -n "$CUSTOM_LOG_FILE" ]; then
        echo -e "${RED}⚠️  指定的日志文件不存在，无法等待创建${NC}"
        return 1
    fi
    
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
        -f|--file)
            CUSTOM_LOG_FILE="$2"
            if [ -z "$CUSTOM_LOG_FILE" ]; then
                echo -e "${RED}错误: --file 选项需要指定文件路径${NC}"
                exit 1
            fi
            shift 2
            ;;
        --history)
            VIEW_MODE="all"
            FOLLOW_MODE=false
            shift
            ;;
        --all-content)
            VIEW_MODE="all"
            FOLLOW_MODE=true
            shift
            ;;
        --from-start)
            VIEW_MODE="all"
            shift
            ;;
        --head)
            VIEW_MODE="head"
            LINES="$2"
            if ! [[ "$LINES" =~ ^[0-9]+$ ]]; then
                echo -e "${RED}错误: --head 参数必须是数字${NC}"
                exit 1
            fi
            shift 2
            ;;
        --around)
            VIEW_MODE="around"
            AROUND_LINE="$2"
            if ! [[ "$AROUND_LINE" =~ ^[0-9]+$ ]]; then
                echo -e "${RED}错误: --around 参数必须是数字${NC}"
                exit 1
            fi
            shift 2
            ;;
        --pager)
            USE_PAGER=true
            shift
            ;;
        --no-follow)
            FOLLOW_MODE=false
            shift
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

# 显示查看模式信息
case "$VIEW_MODE" in
    "head")
        echo -e "${GREEN}📖 查看模式: 显示开头 $LINES 行${NC}"
        ;;
    "around")
        echo -e "${GREEN}📖 查看模式: 显示第 $AROUND_LINE 行周围内容${NC}"
        ;;
    "all")
        if [ "$FOLLOW_MODE" = true ]; then
            echo -e "${GREEN}📖 查看模式: 完整内容 + 实时跟踪${NC}"
        else
            echo -e "${GREEN}📖 查看模式: 完整历史内容${NC}"
        fi
        ;;
    "tail"|*)
        if [ "$FOLLOW_MODE" = true ]; then
            echo -e "${GREEN}📖 查看模式: 最后 $LINES 行 + 实时跟踪${NC}"
        else
            echo -e "${GREEN}📖 查看模式: 最后 $LINES 行${NC}"
        fi
        ;;
esac

if [ "$USE_PAGER" = true ]; then
    echo -e "${BLUE}📄 使用分页器浏览（按 q 退出，/ 搜索）${NC}"
fi

if [ -n "$FILTER" ]; then
    echo -e "${YELLOW}🎯 过滤器: $FILTER${NC}"
fi

if [ "$FOLLOW_MODE" = false ]; then
    echo -e "${CYAN}📺 静态查看模式（不跟踪新内容）${NC}"
else
    echo -e "${GREEN}📺 按 Ctrl+C 退出${NC}"
fi
echo ""

# 显示统计信息
show_stats

# 根据查看模式显示日志内容
display_log_content() {
    local log_file="$1"
    local filter="$2"
    
    # 检查是否使用分页器
    if [ "$USE_PAGER" = true ]; then
        if ! command -v less &> /dev/null; then
            echo -e "${YELLOW}⚠️  less 未安装，使用标准输出${NC}"
            USE_PAGER=false
        fi
    fi
    
    # 构建基础命令
    local base_cmd=""
    local follow_cmd=""
    
    case "$VIEW_MODE" in
        "head")
            base_cmd="head -n $LINES"
            ;;
        "around")
            local start_line=$((AROUND_LINE - 25))
            local end_line=$((AROUND_LINE + 25))
            if [ $start_line -lt 1 ]; then start_line=1; fi
            base_cmd="sed -n '${start_line},${end_line}p'"
            ;;
        "all")
            base_cmd="cat"
            ;;
        "tail"|*)
            base_cmd="tail -n $LINES"
            ;;
    esac
    
    # 添加跟踪模式
    if [ "$FOLLOW_MODE" = true ] && [ "$VIEW_MODE" != "head" ] && [ "$VIEW_MODE" != "around" ]; then
        if [ "$VIEW_MODE" = "all" ]; then
            # 对于全内容模式，先显示完整内容，再跟踪新内容
            follow_cmd="&& tail -n 0 -f"
        else
            # 对于 tail 模式，直接使用 -f
            base_cmd="tail -n $LINES -f"
        fi
    fi
    
    # 构建完整命令
    local display_cmd="$base_cmd \"$log_file\""
    if [ -n "$follow_cmd" ]; then
        display_cmd="$base_cmd \"$log_file\" $follow_cmd \"$log_file\""
    fi
    
    # 添加过滤器
    if [ -n "$filter" ]; then
        if [ "$FOLLOW_MODE" = true ] && [ "$VIEW_MODE" = "all" ]; then
            # 特殊处理全内容+跟踪模式的过滤
            display_cmd="($base_cmd \"$log_file\" | grep -E \"$filter\") && (tail -n 0 -f \"$log_file\" | grep --line-buffered -E \"$filter\")"
        else
            display_cmd="$display_cmd | grep --line-buffered -E \"$filter\""
        fi
    fi
    
    # 添加颜色化
    if [ "$JSON_MODE" = true ]; then
        # JSON模式处理
        if ! command -v jq &> /dev/null; then
            echo -e "${RED}❌ JSON模式需要安装 jq${NC}"
            echo "安装: brew install jq"
            exit 1
        fi
        display_cmd="$display_cmd | jq -r '.'"
    else
        display_cmd="$display_cmd | colorize_log"
    fi
    
    # 添加分页器
    if [ "$USE_PAGER" = true ] && [ "$FOLLOW_MODE" = false ]; then
        display_cmd="$display_cmd | less -R"
    fi
    
    # 执行命令
    eval "$display_cmd"
}

echo -e "${CYAN}────────────────────────────────────${NC}"
echo ""

# 开始监控日志
display_log_content "$LOG_FILE" "$FILTER"
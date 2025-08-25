# Claude Zephyr

解决Claude API端点不稳定问题。自动检测最快可用端点并切换。

## 解决的问题

- Claude API端点经常超时或响应慢
- 手动切换端点很麻烦
- 不知道哪个端点当前最快
- 需要一个稳定的代理来处理这些问题

## 快速开始

### 1. 准备工作
- 安装Rust编译环境
- 确保已安装Claude CLI工具
- 准备Claude认证令牌

### 2. 配置
复制配置文件模板：
```bash
cp config.toml.template config.toml
cp .env.template .env
```

编辑 `.env` 文件，填入你的认证令牌：
```
AUTH_TOKEN_YAYA=your-claude-auth-token-here
AUTH_TOKEN_GPT600=another-auth-token-if-needed
```

### 3. 启动服务
```bash
# 编译项目
cargo build --release

# 命令行模式（后台运行，查看日志）
./target/release/claude-zephyr

# 仪表板模式（实时监控界面）
./target/release/claude-zephyr --dashboard
```

### 4. 使用代理
设置环境变量，让Claude CLI使用代理：
```bash
export ANTHROPIC_BASE_URL="http://localhost:8088"
claude -p "Hello Claude"
```

## 使用方式

### 命令行模式
后台运行，通过日志查看状态：
- 自动检测各端点健康状态
- 自动切换到最快的可用端点
- 显示详细的切换日志

### 仪表板模式
实时图形监控界面：
- 查看所有端点状态和延迟
- 手动选择特定端点（按1-9A-Z键）
- 监控活跃连接情况
- 切换自动/手动模式（按M键）

## 配置说明

### 基本配置
只需要配置两个文件：

**config.toml** - 端点和服务器设置：
```toml
[server]
port = 8088

[[groups]]
name = "主要端点"
auth_token_env = "AUTH_TOKEN_YAYA"
default = true
endpoints = [
    { url = "https://cn1.example.com", name = "CN1", flag = "🇨🇳" },
    { url = "https://hk.example.com", name = "HK", flag = "🇭🇰" }
]
```

**.env** - 认证令牌：
```
AUTH_TOKEN_YAYA=sk-your-auth-token-here
```

### 高级选项
- `switch_threshold_ms`: 切换端点的最小延迟改善（默认50ms）
- `dynamic_scaling`: 根据负载自动调整检查频率
- 支持多个端点组，每组使用不同的认证令牌

## 工作原理

1. **定期检查**: 使用真实的Claude API调用测试所有端点
2. **延迟测量**: 记录每个端点的响应时间
3. **自动切换**: 选择延迟最低且可用的端点
4. **优雅处理**: 等待活跃连接完成后再切换

## 监控

### 状态页面
访问 http://localhost:8088/status 查看：
- 当前使用的端点
- 所有端点的健康状态
- 响应延迟统计
- 活跃连接数

### 仪表板快捷键
- `Q`: 退出
- `R`: 手动刷新健康检查
- `P`: 暂停/恢复监控
- `M`: 切换自动/手动模式
- `1-9A-Z`: 手动选择端点
- `↑↓`: 滚动连接列表

## 常见问题

**Q: 所有端点都显示错误怎么办？**
A: 检查认证令牌是否正确，确保Claude CLI能正常工作。

**Q: 如何添加新的端点？**
A: 编辑config.toml文件，在endpoints数组中添加新条目，重启服务。

**Q: 为什么切换到了更慢的端点？**
A: 可能是之前的快速端点暂时不可用，系统自动切换到可用的端点。

**Q: 可以禁用自动切换吗？**
A: 在仪表板模式下按M键切换到手动模式，然后用1-9A-Z选择固定端点。

## 开发

```bash
# 格式化代码
cargo fmt

# 检查代码
cargo clippy

# 运行测试
cargo test

# 健康检查时序测试
./target/release/claude-zephyr --test-timing
```

## 许可证

MIT License - 详见 LICENSE 文件

## 贡献

欢迎提交问题报告和功能请求。提交代码前请确保通过所有测试。
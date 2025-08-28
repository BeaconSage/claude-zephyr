# pre-commit-check

## Description
通用的Rust项目提交前检查清单，确保代码质量、功能完整性和项目规范。

## Usage
```
/pre-commit-check
```

## Checklist

### 1. 代码构建检查
- [ ] `cargo build` - 确保debug模式编译通过
- [ ] `cargo build --release` - 确保release模式编译通过
- [ ] 检查编译warnings，确认已处理或有合理说明

### 2. 代码质量检查
- [ ] `cargo fmt --check` - 检查代码格式规范
- [ ] `cargo clippy -- -D warnings` - 运行linter，确保无warnings
- [ ] `cargo test` - 运行测试套件（如果存在）
- [ ] 检查代码注释和文档完整性

### 3. 功能验证
- [ ] 核心功能手动测试：验证新增/修改功能工作正常
- [ ] 回归测试：确认现有功能不受影响
- [ ] 配置文件验证：测试配置加载和解析
- [ ] 错误场景测试：验证错误处理逻辑

### 4. 项目文档
- [ ] README.md或项目文档已更新（如需要）
- [ ] CLAUDE.md项目指引已更新（如需要）
- [ ] 配置示例文件已同步更新
- [ ] API变更或breaking changes已记录

### 5. 项目结构整理
- [ ] 删除临时测试文件和调试代码
- [ ] 整理不必要的依赖
- [ ] 确认文件组织结构合理
- [ ] 清理过期的注释和TODO项

### 6. Git提交准备
- [ ] `git status` - 确认要提交的文件列表正确
- [ ] `git diff` - 检查所有变更内容合理
- [ ] `git log --oneline -5` - 查看最近提交，保持风格一致
- [ ] 准备清晰的commit message（遵循约定式提交格式）

### 7. 兼容性检查
- [ ] 向后兼容性：确保不破坏现有API或配置
- [ ] 依赖版本：检查Cargo.toml依赖更新合理性
- [ ] 运行时兼容：确认在目标环境下正常运行

### 8. 安全审查
- [ ] 敏感信息处理：确保不泄露密钥、token等
- [ ] 输入验证：检查用户输入的安全处理
- [ ] 错误信息：确保错误不暴露敏感信息

### 9. 性能考量
- [ ] 资源使用：检查内存泄漏、文件句柄等
- [ ] 异步代码：确认async/await使用正确
- [ ] 并发安全：验证多线程场景下的正确性

### 10. 最终检查
- [ ] 在clean环境重新构建和测试
- [ ] 确认所有checklist项目已完成
- [ ] 准备好回答code review可能的问题

## Commit Message 格式建议
```
<type>(<scope>): <description>

[optional body]

[optional footer(s)]
```

### 类型 (type)
- `feat`: 新功能
- `fix`: 修复bug
- `docs`: 文档更改
- `style`: 代码格式（不影响代码运行）
- `refactor`: 重构（既不是新增功能，也不是修复bug）
- `perf`: 性能优化
- `test`: 添加测试
- `chore`: 构建过程或辅助工具的变动

### 示例
```
feat(logging): implement configurable detail levels

- Add DetailLevel enum with 4 progressive levels
- Implement security filtering for sensitive data
- Enhance monitoring capabilities with historical viewing
```

## Notes
- 根据项目具体情况调整checklist项目
- 大型功能可考虑分多次提交
- 重要变更建议先在分支测试，再合并主分支
- 保持提交历史整洁，避免过多的修复性提交
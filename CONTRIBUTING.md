# 贡献说明

感谢你愿意帮助改进 NoticeFlow。

## 本地开发

```bash
npm install
npm run tauri:dev
```

## 提交前检查

提交 pull request 前建议运行：

```bash
npm run build
cd src-tauri && cargo test
cd src-tauri && cargo clippy --all-targets -- -D warnings
```

## Pull Request 建议

- 保持改动聚焦，避免把无关重构混在一起。
- 修改 Rust 逻辑时，尽量补充对应测试。
- 不要提交本地通知数据库、备份、动作日志、构建产物或包含个人通知内容的截图。
- 涉及用户可见行为变化时，同步更新 README 或发布说明。
- 如果改动影响本地脚本执行、完全磁盘访问、通知删除或 HTTP 动作，请在说明中明确写出影响。

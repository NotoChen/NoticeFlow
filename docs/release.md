# NoticeFlow 发布检查

## 构建配置

- Rust release profile 当前使用 `opt-level = 3` 和 `debug = "line-tables-only"`，兼顾体积、性能和崩溃定位。
- 不建议在当前 macOS/Rust 工具链上直接启用 `debug = false`、`strip` 或全量 LTO；之前会触发 proc-macro 动态库构建问题。
- Tauri CSP 保持前端资源收敛，但后端动作仍由 Rust 命令执行，不依赖放宽 WebView CSP。

## 发布前验证

```bash
npm run build
cd src-tauri && cargo test
cd src-tauri && cargo clippy --all-targets -- -D warnings
```

CI 使用 GitHub Actions 的 macOS runner 验证前端构建、Rust 测试和 Clippy。

## macOS 权限

- 读取通知历史需要完全磁盘访问。
- 删除系统通知记录会写入 macOS `usernoted` 数据库，发布说明中应明确这是显式用户操作。
- 本地通知归档保存在 NoticeFlow 数据目录下的 `notifications.sqlite`，用于列表隐藏、归档和执行历史。

## 打包

```bash
npm run tauri:build:mac:arm    # Apple Silicon (aarch64)
npm run tauri:build:mac:intel  # Intel (x86_64)
```

当前只考虑 macOS 分发，`tauri.conf.json` 的 bundle target 收敛为 `dmg`，按芯片架构分别打包。

本地构建带 updater artifacts 的 macOS release：

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat secrets/noticeflow-updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
npm run tauri:build:mac:release
```

快速构建不带 updater artifacts 的未签名 DMG：

```bash
npm run tauri:build:mac:unsigned
```

GitHub 发包流程：

```bash
git tag v0.1.0
git push origin v0.1.0
```

推送 `v*` tag 会触发 `.github/workflows/release-macos.yml`，分别构建 Apple Silicon 和 Intel macOS DMG，并发布到 GitHub Releases。

发布 workflow 依赖 GitHub Actions Secret：

- `TAURI_SIGNING_PRIVATE_KEY`：updater private key 内容。
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：updater private key 密码；当前 key 未设置密码，可留空。

发布资产包含：

- `NoticeFlow_<version>_aarch64.dmg`：Apple Silicon 用户下载安装包。
- `NoticeFlow_<version>_x64.dmg`：Intel 用户下载安装包。
- `NoticeFlow_aarch64.app.tar.gz` / `NoticeFlow_x64.app.tar.gz`：Tauri updater 使用的更新包。
- `NoticeFlow_aarch64.app.tar.gz.sig` / `NoticeFlow_x64.app.tar.gz.sig`：更新包签名。
- `latest.json`：Tauri updater 读取的静态更新元数据。

## 签名与公证

GitHub Releases 可以先发布未签名 DMG，但用户会遇到 macOS Gatekeeper 提示，需要手动允许打开。

如果要让下载体验更接近正式软件，需要接入：

- Apple Developer Program
- Developer ID Application 证书
- notarization
- staple

这些不是当前 GitHub 发包的硬阻塞。

## 自动更新

自动更新已接入 Tauri updater，设置页提供手动检查更新入口。updater 包必须签名，当前 key pair 保存在本机 `secrets/` 目录，私钥不提交到仓库。

重新生成 updater key pair：

```bash
npm run tauri signer generate
```

如果重新生成 key，需要同步更新：

- 把 private key 配到 GitHub Actions secret：`TAURI_SIGNING_PRIVATE_KEY`
- 如有密码，再配置：`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- 把 public key 写入 `tauri.conf.json` 的 updater 配置

已安装旧 public key 版本的用户无法使用新 key 签名的更新包升级，因此 private key 必须长期妥善保存。

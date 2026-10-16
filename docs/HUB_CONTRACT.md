# Hub Runtime API v1 接入边界

宠物是独立进程，启动、授权及设备管理由 Hub 负责。单独构建与模拟测试不需要 Hub、设备 SDK 或固件源码；实际推送需要运行 Hub。

## Hub 注入的环境变量

| 变量 | 用途 |
| --- | --- |
| `ZECTRIX_HUB_URL` | 本机 HTTP 根地址，仅接受 localhost / 127.0.0.1 |
| `ZECTRIX_APP_TOKEN` | 当前应用私有凭据，仅保留在后端 |
| `ZECTRIX_APP_DATA` | 本应用独立可写数据目录 |
| `ZECTRIX_APP_PORT` | 网页服务端口；兼容旧变量 ZECTRIX_VOCAB_PORT |

## 使用的接口

| 接口 | 用途 |
| --- | --- |
| GET `/runtime/status` | active、lease 和设备状态 |
| GET `/runtime/events?after=...` | 激活、停用、设备按键等增量事件 |
| POST `/runtime/render` | 提交 400×300 layout、lease、present，返回 PNG |
| POST `/runtime/audio` | 提交 WAV 音频，附带 x-lease、x-volume |

以上请求使用 `Authorization: Bearer <ZECTRIX_APP_TOKEN>`。客户端关闭代理和重定向，凭据不进入网页。

显示和音频只在应用前台且设备连接时进行。lease 过期时由 Hub 拒绝请求，切换回来后使用新租约。宠物目前省略 render 的 format，保留默认黑白像素风格；Hub 可独立支持 gray4 / gray16。

## 编译和打包

只需要 Rust stable 与 Python 3。所有 Rust 依赖来自 crates.io 并由 Cargo.lock 锁定，无本地 path 或 git 依赖。当前发行平台为 Apple Silicon macOS，其他平台尚未实机验证。

```sh
cargo test --locked
cargo build --release --locked
python3 tests/integration.py --binary target/release/zectrix-agent-pet
python3 scripts/package.py --binary target/release/zectrix-agent-pet --out dist
```

集成测试使用临时目录及模拟 Hub，不访问真实设备或实际 Codex 会话。打包脚本仅收集明确列出的程序、说明和 Hook 脚本，生成含文件 SHA-256 清单的 Hub ZIP。

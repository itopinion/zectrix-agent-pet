# ZECTRIX Agent 宠物

让像素工作搭子显示真实 Agent 任务状态。作为独立应用运行于 ZECTRIX Hub，当前支持 Apple Silicon Mac、Hub Runtime API v1 和 Note 4。设备固件必须与 Hub 内置的设备 SDK 配套；20261016 版 SDK 对应固件 3.7.2。Note 4C 不在本版兼容范围内。

## 与 Hub / SDK 的边界

本仓库可单独克隆、编译、测试和打包。它没有 `zectrix-sdk`、`zectrix-cards`、相邻源码目录或设备驱动依赖；运行时通过本机 HTTP 调用 [ZECTRIX Hub](https://github.com/itopinion/zectrix-hub) 的 Runtime API v1。

- 宠物仓库负责：任务状态、Codex 跟随、权限提醒、像素宠物、网页设置与提示音。
- Hub 负责：应用安装与切换、前台租约、画面渲染、设备连接、图片和音频发送。
- 设备 SDK 由 Hub 单独维护，负责通信协议及灰阶转换。本仓库不携带 SDK 或固件；更新 Hub 内的 SDK 不需要重新编译宠物。

`src/hub.rs` 是本应用的轻量 HTTP 适配层，不是设备 SDK。详细环境变量和接口见 [docs/HUB_CONTRACT.md](docs/HUB_CONTRACT.md)。

## 安装

1. 从 [GitHub Releases](https://github.com/itopinion/zectrix-agent-pet/releases/latest) 下载 `zectrix-agent-pet-0.3.4-macos-aarch64.zip`，在 ZECTRIX Hub 的应用安装入口选择这个 ZIP。
2. 打开「Agent 宠物」，连接设备，并在 Hub 中切到该应用。
3. 新安装会自动开启本机 Codex 跟随，有任务时自动显示状态，无需再开一次开关。没有任务时显示「等待任务 / Codex 跟随已开启」。要接入其他 Agent，可在应用网页的「接入 Agent」复制说明。

升级保留现有设置；之前关闭过 Codex 跟随的用户，可在「接入 Agent」中重新开启。应用需要在 Hub 中运行并切到前台，设备保持连接。

应用不直接连接设备、不安装语音模型、不需要推理 API Key。网页关闭后，只要 Hub 与应用进程还在运行，就会继续接收状态。切到其他应用后，停止向设备推送，任务记录继续接收。

## 状态与操作

- 待命：尚无任务。
- 工作：Agent 报告工作中；可显示明确提供的进度。未知进度不生成百分比。
- 等你处理：主动上报的等待状态，或识别到 Codex 结构化提问；回到对应 Agent 窗口处理。
- 完成、出错、停止：来自明确的生命周期事件。
- 状态待确认：超过设定时间无更新或应用重启后尚未收到新消息。不会自动判定成功。

设备左侧显示任务名称；列出进行中和等待确认的任务，每页最多四个，每个任务有独立的名称行和状态行。列表顺序按会话标识固定，心跳和状态更新不会换位，手动选中任务也不会隐藏其他运行任务。长名称在设备上省略，网页保留完整名称。同一个 Codex 聊天只显示最新一轮。超过四项时上下键翻页（末页循环到首页），不自动轮播；四项以内上下键选择宠物对应任务，列表保持显示。确认键恢复首页与自动跟随。工作动画每次画面处理完成后至少间隔 2.5 秒，不保证设备达到固定帧率。可关闭动画，仅在状态变化或手动翻页时更新。

等待确认时播放双音，任务完成时播放上行三音；默认音量 90%，可在设置中调节、关闭或试听。仅前台且设备已连接时播放。重复状态、表情演示和启动时的历史记录不会响铃；播放失败不重试，切回前台不补响。

「显示与设置」中的表情演示带有明确演示标记，不写入任务历史；如果应用在前台，演示画面也会推送到设备。

## 本机 Codex 跟随

0.3.4 仅跟随用户聊天，排除子 Agent 会话及其继承的父聊天记录，子任务结束不会覆盖主任务状态。升级时会重建旧版 Codex 状态缓存，保留设置与通用 Agent 上报的任务记录。

新安装默认开启，可在网页关闭；升级保留已有开关选择。读取本机 `CODEX_HOME/sessions`（未配置时使用 `~/.codex/sessions`）最近七天修改的最多 24 个会话文件，处理开始、结束、中止、活动，以及 `request_user_input` / `request_user_input_async` 的调用与答复信号。仅核对答复信封与调用 ID，不保留提问或答案。从相邻的 `session_index.jsonl` 读取聊天名称，缺少时回退工作目录名。名称保存于本机并显示到设备，请勿在名称中放置秘密。不会上传会话内容，不展示对话、推理、工具参数、代码或凭据。

这是对本地会话文件格式的适配，并非 Codex 官方稳定状态 API。格式变化、文件不在本机、会话超出扫描范围或缺少明确生命周期事件时，可能无法跟随。超时只表示需要确认。Codex 完成表示一轮结束，不等于产物验收或整个目标完成。权限审批通过单独的官方生命周期通知接入，需先安装并信任脚本，详见 [APPROVAL_REMINDERS.md](APPROVAL_REMINDERS.md)。普通文本提问及其他未知工具不会推测为等待确认。自动跟随不估算百分比。

开发测试可用 `ZECTRIX_CODEX_SESSIONS` 指定专用会话目录，不修改实际 CODEX_HOME。

## 其他 Agent

在「接入 Agent」复制说明，包含当前安装路径与连接文件位置。也可以使用同一可执行程序的 `send` / `run` 命令。完整协议见 [AGENT_INTEGRATION.md](AGENT_INTEGRATION.md)。`run` 包装命令的完成状态只表示命令正常退出。

## 数据

数据存于 Hub 分配的 `app-data/zectrix-agent-pet/`：

- `state.json`：设置和最近 100 条任务；更新应用保留。
- `agent-connection.json`：本机地址及上报凭据，启动时更新，正常退出时删除；不要分享或上传。

程序发行文件与数据分离。通用 Agent 上报的标题和详情会保存，因此不要上报个人信息或秘密。Codex 跟随只提取上述元数据，并可随时关闭。

## 开发

```sh
python3 scripts/check_independence.py
cargo test --locked
cargo build --release --locked
python3 scripts/package.py --binary target/release/zectrix-agent-pet --out dist
```

不依赖相邻工程源码。UI 嵌入二进制；400×300 画面由 Hub 公共渲染器生成。几何宠物沿用此前 AI 工作搭子的耳朵、面部和键盘；没有复制静态演示中的示例任务文案或字体位图。应用包不携带字体、模型、个人路径、会话文件或固件。Hub 可选的小字号字体由 Hub 自己管理。

开发启动需要 Hub 注入 `ZECTRIX_HUB_URL`、`ZECTRIX_APP_TOKEN`、`ZECTRIX_APP_DATA` 与网页端口。优先读取通用 `ZECTRIX_APP_PORT`，兼容现有 Hub 的 `ZECTRIX_VOCAB_PORT`。私有 Hub token 始终留在后端。

可用 Python 标准库运行模拟 Hub 集成测试：

```sh
python3 tests/integration.py --binary target/release/zectrix-agent-pet
```

Windows 和 Linux 尚未验证及打包。本版没有向 Agent 回复的功能；设备确认键只恢复自动跟随，不会批准 Agent 操作。

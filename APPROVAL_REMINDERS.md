# Codex 权限审批提醒

使用官方 `PermissionRequest` 生命周期通知。收到审批请求后，设备显示“等待权限审批”、聊天名称及简短类型（执行命令、修改文件或工具访问），并按当前设置响铃，默认音量 90%。

通知脚本始终静默退出，不返回 allow、deny、审批决定或更新权限，不替用户批准操作。请求仍在 Codex 原来的界面处理。

## 首次启用

1. 在 Hub 运行本应用，并开启本机 Codex 跟随。
2. 运行包内 `scripts/install_approval_hooks.py`，传入本机 Codex 目录和应用页面给出的连接文件：

```sh
python3 scripts/install_approval_hooks.py \
  --codex-home "$HOME/.codex" \
  --connection '/应用数据目录/agent-connection.json'
```

安装器会备份已有 `hooks.json`，保留其他脚本，把本应用的通知脚本放到应用数据目录，并注册五个事件：PermissionRequest、PostToolUse、Stop、Interrupt、SessionEnd。不会修改审批策略、沙箱权限或信任记录。

3. 在 Codex CLI 的 `/hooks` 中审核并信任标有“ZECTRIX 宠物审批提醒”的五项定义。这是 Codex 的信任检查，安装器不会跳过。已开启的聊天可能需要在下一轮或重新打开后加载配置。升级时若定义发生变化，需要再次审核。

## 行为与边界

- 同一待处理工具请求重复发送不会重复响铃。若同一轮里相同工具和参数在前一调用返回后再次触发审批，会视为新请求。
- 常规工作心跳不会抹掉待审批标记。PostToolUse 表明对应工具调用已返回时解除等待；Stop、Interrupt、SessionEnd 清理该轮请求。不是在用户点击批准的瞬间清除；长时间运行的工具可能要等到返回才解除。
- 多个审批请求分开记录。只清除对应请求；仍有其他审批时继续显示等待。Codex 本轮明确结束后也不再显示其待审批状态。
- 应用离线、关闭跟随、在后台或关闭声音时不会补响历史通知。重启不会重播已有审批的声音。
- 脚本只转交会话/轮次标识、请求标识、任务名称、工具名和时间。工具参数仅在内存中计算摘要以配对，不保存或转交命令、参数、结果或审批决定。配对缓存仅保存摘要、标识和时间。
- 遵循当前 Codex 的 hook 覆盖范围；特殊工具若不触发 PermissionRequest，则无法靠这个通知入口提醒。对于内置结构化提问，继续沿用本地会话适配。
- 这不是审批界面：设备确认键只恢复任务跟随。

官方协议依据：[Codex Hooks](https://learn.chatgpt.com/docs/hooks#permissionrequest)。

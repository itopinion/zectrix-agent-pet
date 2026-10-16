# Agent 状态接入 v1

先通过 Hub 启动应用。网页「接入 Agent」会生成本机可直接使用的命令。无须获取或使用 Hub 私有 token。

## 命令

```sh
zectrix-agent-pet send --connection '/应用数据目录/agent-connection.json' \
  --task unique-task-001 --seq 1 --state working --title '整理资料' --agent 'My Agent'
zectrix-agent-pet send --connection '/应用数据目录/agent-connection.json' \
  --task unique-task-001 --seq 2 --state working --title '整理资料' --detail '完成 3 / 5 项' --progress 60
zectrix-agent-pet send --connection '/应用数据目录/agent-connection.json' \
  --task unique-task-001 --seq 3 --state done --title '整理资料' --detail '结果已生成'
```

每次上报是完整状态，不是增量字段合并。未知进度省略 `--progress`。ID 可含字母、数字及 `- _ : .`，最多 160 字节；seq 为大于零且单调增加的整数。相同或较旧序号被忽略；结束的任务不能重新进入工作，新一轮使用新 ID。每分钟上报一次心跳（同状态、新序号），避免长期任务被标记待确认。最近 100 条任务之外的 ID 不用于重放保护，ID 应永久唯一。

状态：`working`、`waiting`、`done`、`error`、`stopped`、`unknown`。只有 Agent 明确完成才上报 done；工具返回、等待用户回复、网络断开均不等同于任务成功。

进入 waiting 或 done 时，前台应用按设置播放一次提示音（默认 90%）。同状态心跳不会重复播放；恢复 working 后再次 waiting 是一次新的提醒。声音播放异步，HTTP 接受不代表播放完成。

## 跟随一个命令

```sh
zectrix-agent-pet run --connection '/应用数据目录/agent-connection.json' \
  --title '本次 Agent 任务' -- your-agent-command argument
```

每次生成唯一 ID，启动前上报工作中，每 15 秒心跳，正常退出报完成，非零退出报错误。命令找不到也会报错；包装进程异常退出后由超时转为待确认。此完成只说明子进程退出码为 0，不能替代业务结果验收。参数直接传给程序，不经过 shell；保留原命令的标准输入输出。

## 本机 HTTP

从连接文件读取 `url` 与 `token`，向 `url + api/events` POST JSON，并使用 `Authorization: Bearer <token>`。客户端必须禁用代理和重定向，仅允许本机 HTTP 地址。不要将 token 发给网站或写入命令示例、日志和发布包。

```json
{"task_id":"unique-task-001","seq":1,"status":"working","title":"整理资料","detail":"正在检查输入","progress":null,"agent":"My Agent"}
```

标题最多 120 字符、详情 240 字符、Agent 名称 40 字符，请主动去除敏感信息。成功返回 `{"ok":true,"applied":true}`；旧序号返回 applied=false。该响应表示应用接受状态，不代表设备实际显示完成。应用进程重启后重新读取连接文件，不缓存旧端口或 token。

浏览器控制页和 Agent 上报使用不同凭据，Agent 凭据只能写入 `/api/events` 与审批通知入口 `/api/approvals`。应用仅监听本机地址，拒绝不匹配的 Host 和跨站 Origin。不要把这个 HTTP 服务直接暴露到局域网或互联网。

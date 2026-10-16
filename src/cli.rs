use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use zectrix_agent_pet::model::{Event, Status};

#[derive(Deserialize)]
struct Connection {
    url: String,
    token: String,
}
fn option(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|p| p[0] == name).map(|p| p[1].clone())
}
fn connection(args: &[String]) -> Result<Connection> {
    let path = option(args, "--connection")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("ZECTRIX_APP_DATA")
                .map(PathBuf::from)
                .map(|p| p.join("agent-connection.json"))
        })
        .context("请用 --connection 指定应用页面给出的 agent-connection.json")?;
    let c: Connection = serde_json::from_slice(&std::fs::read(path)?)?;
    let url = reqwest::Url::parse(&c.url)?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("127.0.0.1") | Some("localhost"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("连接地址须为本机 HTTP 根地址")
    }
    Ok(c)
}
async fn send(client: &reqwest::Client, c: &Connection, e: &Event) -> Result<serde_json::Value> {
    let r = client
        .post(format!("{}/api/events", c.url.trim_end_matches('/')))
        .bearer_auth(&c.token)
        .json(e)
        .send()
        .await?;
    let status = r.status();
    let body: serde_json::Value = r.json().await?;
    if !status.is_success() {
        bail!("上报失败：{}", body["error"])
    }
    Ok(body)
}
pub async fn run(args: Vec<String>) -> Result<i32> {
    if args[0] == "--help" || args[0] == "help" {
        println!("Agent 宠物由 Hub 启动。\n\n上报状态：\n  zectrix-agent-pet send --connection FILE --task ID --seq 1 --state working --title 标题 [--detail 说明] [--progress 40] [--agent Agent]\n\n跟随命令（开始、心跳、退出）：\n  zectrix-agent-pet run --connection FILE --title 标题 -- PROGRAM ARG...\n\nstate: working / waiting / done / error / stopped / unknown\n同一 task 的 seq 必须递增；结束后开启新 task。未知进度请省略。\nrun 的完成只表示命令正常退出，并非业务结果验收。");
        return Ok(0);
    }
    let c = connection(&args)?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(3))
        .build()?;
    let title = option(&args, "--title").context("缺少 --title")?;
    if args[0] == "send" {
        let state = option(&args, "--state").context("缺少 --state")?;
        let e = Event {
            task_id: option(&args, "--task").context("缺少 --task")?,
            thread_id: None,
            seq: option(&args, "--seq").context("缺少 --seq")?.parse()?,
            status: serde_json::from_value(serde_json::json!(state))?,
            title,
            detail: option(&args, "--detail").unwrap_or_default(),
            progress: option(&args, "--progress").map(|s| s.parse()).transpose()?,
            agent: option(&args, "--agent").unwrap_or_else(|| "Agent".into()),
        };
        println!("{}", send(&client, &c, &e).await?);
        return Ok(0);
    }
    if args[0] != "run" {
        bail!("未知命令，使用 --help 查看用法")
    }
    let split = args
        .iter()
        .position(|s| s == "--")
        .context("用 -- 分隔需要跟随的命令")?;
    let cmd = args.get(split + 1).context("缺少命令")?;
    let mut e = Event {
        task_id: uuid::Uuid::new_v4().to_string(),
        thread_id: None,
        seq: 1,
        status: Status::Working,
        title,
        detail: "命令运行中".into(),
        progress: None,
        agent: option(&args[..split], "--agent").unwrap_or_else(|| "Agent CLI".into()),
    };
    send(&client, &c, &e).await?;
    let mut child = match tokio::process::Command::new(cmd)
        .args(&args[split + 2..])
        .kill_on_drop(true)
        .spawn()
    {
        Ok(c) => c,
        Err(err) => {
            e.seq += 1;
            e.status = Status::Error;
            e.detail = "命令未能启动".into();
            let _ = send(&client, &c, &e).await;
            return Err(err.into());
        }
    };
    let result = loop {
        tokio::select! {
            r=child.wait()=>break r?,
            _=tokio::time::sleep(std::time::Duration::from_secs(15))=>{e.seq+=1;if let Err(err)=send(&client,&c,&e).await{eprintln!("宠物状态未送达：{err}");}},
        }
    };
    e.seq += 1;
    e.status = if result.success() {
        Status::Done
    } else {
        Status::Error
    };
    e.detail = if result.success() {
        "命令已正常退出".into()
    } else {
        format!("命令退出：{result}")
    };
    if let Err(err) = send(&client, &c, &e).await {
        eprintln!("最终状态未送达：{err}");
    }
    Ok(result.code().unwrap_or(1))
}

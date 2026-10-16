mod cli;
mod hub;
use anyhow::{Context, Result};
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Request, State},
    http::{header, Method, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{mpsc, Mutex, RwLock};
use zectrix_agent_pet::{
    approval::Signal,
    codex::Watcher,
    layout,
    model::{Event, Model, Settings, Status},
    sound,
};

type Shared = Arc<App>;
struct App {
    model: Mutex<Model>,
    hub: hub::Hub,
    runtime: RwLock<hub::Runtime>,
    png: RwLock<Vec<u8>>,
    health: RwLock<Value>,
    root: PathBuf,
    port: u16,
    browser_token: String,
    agent_token: String,
    executable: String,
    codex_root: PathBuf,
    notices: mpsc::Sender<(String, Status, i64)>,
    sound_gate: Mutex<()>,
}
struct ApiError(anyhow::Error);
impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        Self(e.into())
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":self.0.to_string()})),
        )
            .into_response()
    }
}
type Api<T> = std::result::Result<Json<T>, ApiError>;
fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let tmp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    std::fs::rename(tmp, path)?;
    Ok(())
}
fn save(a: &App, m: &Model) -> Result<()> {
    write_private(&a.root.join("state.json"), &serde_json::to_vec_pretty(m)?)
}
async fn guard(State(a): State<Shared>, req: Request, next: Next) -> Response {
    let allowed = [
        format!("127.0.0.1:{}", a.port),
        format!("localhost:{}", a.port),
    ];
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|x| x.to_str().ok())
        .unwrap_or("");
    if !allowed.iter().any(|v| v == host) {
        return (StatusCode::FORBIDDEN, "只允许本机访问").into_response();
    }
    if let Some(origin) = req.headers().get(header::ORIGIN) {
        if !origin
            .to_str()
            .ok()
            .is_some_and(|o| allowed.iter().any(|v| o == format!("http://{v}")))
        {
            return (StatusCode::FORBIDDEN, "拒绝跨站请求").into_response();
        }
    }
    if req.method() != Method::GET {
        let browser = req
            .headers()
            .get("x-app-token")
            .and_then(|x| x.to_str().ok())
            == Some(&a.browser_token);
        let agent = matches!(req.uri().path(), "/api/events" | "/api/approvals")
            && req
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|x| x.to_str().ok())
                .and_then(|x| x.strip_prefix("Bearer "))
                == Some(&a.agent_token);
        if !browser && !agent {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error":"缺少本机应用凭据"})),
            )
                .into_response();
        }
    }
    let mut response = next.run(req).await;
    let h = response.headers_mut();
    h.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    h.insert("x-content-type-options", "nosniff".parse().unwrap());
    h.insert("content-security-policy","default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'".parse().unwrap());
    response
}
async fn boot(State(a): State<Shared>) -> Json<Value> {
    Json(
        json!({"token":a.browser_token,"app":"zectrix-agent-pet","version":env!("CARGO_PKG_VERSION")}),
    )
}
async fn status(State(a): State<Shared>) -> Json<Value> {
    let m = a.model.lock().await;
    let r = a.runtime.read().await;
    Json(
        json!({"current":m.current(now()),"tasks":m.visible(now()),"running":m.running(now()),"settings":m.settings,"selected":m.selected,"demo":m.demo,"runtime":{"active":r.active,"connected":r.device["connected"],"hub_url":a.hub.url()},"health":a.health.read().await.clone(),"connection_file":a.root.join("agent-connection.json"),"executable":a.executable}),
    )
}
async fn screen(State(a): State<Shared>) -> Response {
    let p = a.png.read().await.clone();
    if p.is_empty() {
        return (StatusCode::SERVICE_UNAVAILABLE, "正在等待 Hub 渲染").into_response();
    }
    ([(header::CONTENT_TYPE, "image/png")], Body::from(p)).into_response()
}
async fn event(State(a): State<Shared>, Json(e): Json<Event>) -> Api<Value> {
    let mut m = a.model.lock().await;
    let mut next = m.clone();
    let previous = m
        .tasks
        .iter()
        .find(|t| t.source == "agent" && t.event.task_id == e.task_id)
        .map(|t| t.event.status);
    let notice = (format!("agent:{}", e.task_id), e.status, now());
    let applied = next.apply(e, "agent", now())?;
    if applied {
        save(&a, &next)?;
        *m = next;
        if sound::transition(previous, notice.1) {
            let _ = a.notices.try_send(notice);
        }
    }
    Ok(Json(json!({"ok":true,"applied":applied})))
}
async fn approval(State(a): State<Shared>, Json(s): Json<Signal>) -> Api<Value> {
    let mut m = a.model.lock().await;
    if !m.settings.codex_enabled {
        return Ok(Json(json!({"ok":true,"applied":false})));
    }
    let mut next = m.clone();
    let alert = next.approvals.apply(&s, now())?;
    if alert {
        next.selected = None;
        next.demo = None;
    }
    save(&a, &next)?;
    *m = next;
    if alert {
        let _ = a
            .notices
            .try_send((format!("codex:{}", s.turn_id), Status::Waiting, now()));
    }
    a.health.write().await["approval"] = json!({"event":s.event,"at":now()});
    Ok(Json(json!({"ok":true,"notified":alert})))
}
async fn settings(State(a): State<Shared>, Json(s): Json<Settings>) -> Api<Value> {
    if !(1..=240).contains(&s.stale_minutes) || s.volume > 100 {
        return Err(anyhow::anyhow!("状态超时须为 1–240 分钟，音量须为 0–100").into());
    }
    let mut m = a.model.lock().await;
    let mut next = m.clone();
    next.settings = s;
    save(&a, &next)?;
    *m = next;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
struct Select {
    key: Option<String>,
}
async fn select(State(a): State<Shared>, Json(s): Json<Select>) -> Api<Value> {
    let mut m = a.model.lock().await;
    if s.key
        .as_ref()
        .is_some_and(|k| !m.visible(now()).iter().any(|t| &t.key() == k))
    {
        return Err(anyhow::anyhow!("任务不存在").into());
    }
    m.selected = s.key;
    m.demo = None;
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
struct Demo {
    status: Option<Status>,
}
async fn demo(State(a): State<Shared>, Json(d): Json<Demo>) -> Json<Value> {
    a.model.lock().await.demo = d.status;
    Json(json!({"ok":true}))
}
async fn play_sound(a: &Shared, state: Status, volume: u8) -> Result<Value> {
    let _gate = a
        .sound_gate
        .try_lock()
        .context("提示音正在播放，请稍后再试")?;
    let r = a.runtime.read().await.clone();
    anyhow::ensure!(
        r.active && r.device["connected"] == true,
        "请先连接设备并将宠物切到前台"
    );
    let result = a.hub.play(sound::wav(state), r.lease, volume).await?;
    *a.health.write().await.get_mut("sound").unwrap() =
        json!({"state":state,"volume":volume,"at":now(),"result":result});
    Ok(result)
}
async fn sound_preview(State(a): State<Shared>, Json(d): Json<Demo>) -> Api<Value> {
    let state = d.status.context("请选择等待或完成提示音")?;
    if !matches!(state, Status::Waiting | Status::Done) {
        return Err(anyhow::anyhow!("仅支持等待和完成提示音").into());
    }
    let volume = a.model.lock().await.settings.volume;
    Ok(Json(play_sound(&a, state, volume).await?))
}
async fn sound_loop(a: Shared, mut receiver: mpsc::Receiver<(String, Status, i64)>) {
    while let Some((key, state, at)) = receiver.recv().await {
        let m = a.model.lock().await.clone();
        let r = a.runtime.read().await.clone();
        if !m.settings.sound_enabled
            || m.settings.volume == 0
            || m.demo.is_some()
            || !r.active
            || r.device["connected"] != true
            || now() - at > 10
            || !m
                .visible(now())
                .iter()
                .any(|t| t.key() == key && sound::still_relevant(t, state))
        {
            continue;
        }
        if let Err(e) = play_sound(&a, state, m.settings.volume).await {
            a.health.write().await["sound"] = json!({"error":e.to_string(),"at":now()});
        }
        // Never retry uncertain audio results or accumulate a long chime backlog.
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
async fn runtime_loop(a: Shared) {
    let mut cursor = 0;
    loop {
        let result = async {
            let r = a.hub.status().await?;
            *a.runtime.write().await = r;
            let batch = a.hub.events(cursor).await?;
            let r = a.hub.status().await?;
            cursor = batch.cursor;
            {
                let mut m = a.model.lock().await;
                for item in batch.events {
                    if !r.active || item.lease != r.lease {
                        continue;
                    }
                    if item.event["type"] == "key" && item.event["action"] == "Short" {
                        match item.event["key"].as_str() {
                            Some("Up") => m.cycle(-1, now()),
                            Some("Down") => m.cycle(1, now()),
                            Some("Confirm") => {
                                m.selected = None;
                                m.demo = None;
                            }
                            _ => {}
                        }
                    }
                }
            }
            *a.runtime.write().await = r;
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if let Err(e) = result {
            let mut r = a.runtime.write().await;
            r.active = false;
            r.device = json!({"connected":false});
            drop(r);
            a.health.write().await["hub"] = json!(e.to_string());
            tokio::time::sleep(Duration::from_secs(2)).await;
        } else {
            a.health.write().await["hub"] = json!("ready");
        }
    }
}
async fn draw_loop(a: Shared) {
    let mut previous = String::new();
    let mut frame = 0;
    loop {
        let m = a.model.lock().await.clone();
        let r = a.runtime.read().await.clone();
        // One request at a time; the 2.5s pause comes after the previous render finishes.
        // No inactive animation and no queued backlog for an e-ink display.
        let moving = r.active
            && m.settings.animation
            && m.current(now())
                .is_some_and(|t| t.status() == Status::Working);
        let v = layout::draw(&m, now(), if moving { frame } else { 0 });
        let signature = format!("{}:{}:{}", r.active, r.lease, v);
        if signature != previous {
            match a.hub.render(v, r.lease, r.active).await {
                Ok(p) => {
                    *a.png.write().await = p;
                    previous = signature;
                    a.health.write().await["render"] = json!("ready");
                }
                Err(e) => {
                    a.health.write().await["render"] = json!(e.to_string());
                }
            }
        }
        frame = (frame + 1) % 6;
        tokio::time::sleep(Duration::from_millis(2500)).await;
    }
}
async fn codex_loop(a: Shared) {
    let mut watcher = Watcher::new(a.codex_root.clone());
    let mut enabled = false;
    let mut watch_started = now();
    loop {
        let on = a.model.lock().await.settings.codex_enabled;
        if on {
            if !enabled {
                watcher.reset();
                watch_started = now();
            }
            match watcher.scan() {
                Ok(events) => {
                    let mut m = a.model.lock().await;
                    if m.settings.codex_enabled {
                        let mut next = m.clone();
                        let mut changed = false;
                        let mut notices = vec![];
                        for (e, at) in events {
                            let previous = next
                                .tasks
                                .iter()
                                .find(|t| t.source == "codex" && t.event.task_id == e.task_id)
                                .map(|t| t.event.status);
                            // Refresh labels even when replay has the same event sequence.
                            if let Some(t) = next
                                .tasks
                                .iter_mut()
                                .find(|t| t.source == "codex" && t.event.task_id == e.task_id)
                            {
                                if t.event.title != e.title || t.event.thread_id != e.thread_id {
                                    t.event.title = e.title.clone();
                                    t.event.thread_id = e.thread_id.clone();
                                    changed = true;
                                }
                            }
                            let notice = (format!("codex:{}", e.task_id), e.status, at);
                            let applied = next.apply(e, "codex", at).unwrap_or(false);
                            if applied
                                && enabled
                                && at >= watch_started
                                && sound::transition(previous, notice.1)
                            {
                                notices.push(notice);
                            }
                            changed |= applied;
                        }
                        if changed {
                            match save(&a, &next) {
                                Ok(()) => {
                                    *m = next;
                                    for notice in notices {
                                        let _ = a.notices.try_send(notice);
                                    }
                                }
                                Err(e) => {
                                    a.health.write().await["storage"] = json!(e.to_string());
                                }
                            }
                        }
                    }
                    a.health.write().await["codex"] = json!("watching");
                }
                Err(e) => {
                    a.health.write().await["codex"] = json!(e.to_string());
                }
            }
        } else {
            a.health.write().await["codex"] = json!("disabled");
        }
        enabled = on;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        std::process::exit(cli::run(args).await?);
    }
    let hub = hub::Hub::from_env()?;
    let root =
        PathBuf::from(std::env::var("ZECTRIX_APP_DATA").context("请从 Hub 打开 Agent 宠物")?);
    std::fs::create_dir_all(&root)?;
    let root = root.canonicalize()?;
    let port: u16 = std::env::var("ZECTRIX_APP_PORT")
        .or_else(|_| std::env::var("ZECTRIX_VOCAB_PORT"))
        .context("Hub 未分配应用端口")?
        .parse()?;
    if port == 0 {
        anyhow::bail!("Hub 端口不能为 0")
    }
    let mut model: Model = if root.join("state.json").exists() {
        serde_json::from_slice(&std::fs::read(root.join("state.json"))?)
            .context("状态文件损坏；原文件已保留，请先备份后修复")?
    } else {
        Model::default()
    };
    for t in &mut model.tasks {
        if !t.event.status.terminal() {
            t.stale = true;
        }
    }
    let runtime = hub.status().await.context("无法连接 Hub")?;
    let agent_token = uuid::Uuid::new_v4().to_string();
    let browser_token = uuid::Uuid::new_v4().to_string();
    let codex_home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".codex")
        });
    let codex_root = std::env::var_os("ZECTRIX_CODEX_SESSIONS")
        .map(PathBuf::from)
        .unwrap_or_else(|| codex_home.join("sessions"));
    let (notices_tx, notices_rx) = mpsc::channel(16);
    let a = Arc::new(App {
        model: Mutex::new(model),
        hub,
        runtime: RwLock::new(runtime),
        png: RwLock::new(vec![]),
        health: RwLock::new(
            json!({"hub":"ready","render":"loading","codex":"disabled","sound":null}),
        ),
        root: root.clone(),
        port,
        browser_token,
        agent_token,
        executable: std::env::current_exe()?.to_string_lossy().into(),
        codex_root,
        notices: notices_tx,
        sound_gate: Mutex::new(()),
    });
    let app = Router::new()
        .route(
            "/",
            get(|| async { Html(include_str!("../web/index.html")) }),
        )
        .route(
            "/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../web/app.js"),
                )
            }),
        )
        .route(
            "/style.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("../web/style.css"),
                )
            }),
        )
        .route("/api/boot", get(boot))
        .route("/api/status", get(status))
        .route("/screen.png", get(screen))
        .route("/api/events", post(event))
        .route("/api/approvals", post(approval))
        .route("/api/settings", post(settings))
        .route("/api/select", post(select))
        .route("/api/demo", post(demo))
        .route("/api/sound", post(sound_preview))
        .layer(DefaultBodyLimit::max(16384))
        .layer(middleware::from_fn_with_state(a.clone(), guard))
        .with_state(a.clone());
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    write_private(
        &root.join("agent-connection.json"),
        &serde_json::to_vec_pretty(
            &json!({"url":format!("http://127.0.0.1:{port}/"),"token":a.agent_token}),
        )?,
    )?;
    let jobs = [
        tokio::spawn(runtime_loop(a.clone())),
        tokio::spawn(draw_loop(a.clone())),
        tokio::spawn(codex_loop(a.clone())),
        tokio::spawn(sound_loop(a.clone(), notices_rx)),
    ];
    println!("Agent 宠物：http://127.0.0.1:{port}/");
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            #[cfg(unix)]
            {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .unwrap();
                tokio::select! {_=term.recv()=>{},_=tokio::signal::ctrl_c()=>{}}
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
        })
        .await;
    for j in jobs {
        j.abort();
    }
    let _ = std::fs::remove_file(root.join("agent-connection.json"));
    result?;
    Ok(())
}

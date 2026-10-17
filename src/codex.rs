//! Optional local adapter: consume lifecycle metadata only, never publish messages,
//! prompts, tool arguments, reasoning, file contents or credentials.
use crate::model::{clean, Event, Status};
use anyhow::{Context, Result};
use chrono::DateTime;
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
struct Cursor {
    offset: u64,
    turn: Option<String>,
    title: String,
    state: Option<Status>,
    discarding: bool,
    thread: Option<String>,
    pending: HashMap<String, bool>,
}
pub struct Watcher {
    root: PathBuf,
    cursors: HashMap<PathBuf, Cursor>,
}
impl Watcher {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            cursors: HashMap::new(),
        }
    }
    pub fn reset(&mut self) {
        self.cursors.clear();
    }
    pub fn scan(&mut self) -> Result<Vec<(Event, i64)>> {
        if !self.root.is_dir() {
            anyhow::bail!("未找到本机 Codex 会话目录")
        }
        let mut paths = vec![];
        collect(&self.root, 0, &mut paths)?;
        paths.sort_by_key(|(_, m)| std::cmp::Reverse(*m));
        paths.truncate(24);
        self.cursors
            .retain(|p, _| paths.iter().any(|(x, _)| x == p));
        let mut out = vec![];
        let titles = read_titles(self.root.parent().unwrap_or(&self.root));
        for (path, _) in paths {
            let size = fs::metadata(&path)?.len();
            let c = self.cursors.entry(path.clone()).or_default();
            if size < c.offset {
                *c = Cursor::default();
            }
            let mut f = File::open(&path).context("无法读取 Codex 会话")?;
            if c.title.is_empty() {
                let mut first = vec![];
                (&mut f).take(1024 * 1024).read_to_end(&mut first)?;
                if let Some(line) = first.split(|b| *b == b'\n').next() {
                    if let Ok(v) = serde_json::from_slice::<Value>(line) {
                        c.thread = v["payload"]["id"].as_str().map(str::to_owned);
                        let cwd = v["payload"]["cwd"].as_str().unwrap_or("");
                        let name = Path::new(cwd)
                            .file_name()
                            .and_then(|x| x.to_str())
                            .unwrap_or("本机任务");
                        c.title = format!("Codex · {}", clean(name, 50));
                    }
                }
                if c.title.is_empty() {
                    c.title = "Codex 本机任务".into();
                }
                // Avoid blocking startup on old multi-gigabyte history. A tail without
                // an explicit lifecycle event is left unknown, never guessed as done.
                if size > 4 * 1024 * 1024 {
                    c.offset = size - 4 * 1024 * 1024;
                    c.discarding = true;
                }
            }
            if let Some(title) = c.thread.as_ref().and_then(|id| titles.get(id)) {
                c.title = clean(title, 120);
            }
            f.seek(SeekFrom::Start(c.offset))?;
            let mut data = vec![];
            f.take(2 * 1024 * 1024).read_to_end(&mut data)?;
            let mut consumed = 0;
            for part in data.split_inclusive(|b| *b == b'\n') {
                if !part.ends_with(b"\n") {
                    break;
                }
                consumed += part.len();
                let seq = c.offset + consumed as u64;
                if c.discarding {
                    c.discarding = false;
                    continue;
                }
                if part.len() > 1024 * 1024 {
                    continue;
                }
                let Ok(v) = serde_json::from_slice::<Value>(part) else {
                    continue;
                };
                let p = &v["payload"];
                let kind = p["type"].as_str().unwrap_or("");
                let at = v["timestamp"]
                    .as_str()
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|d| d.timestamp())
                    .unwrap_or(0);
                let turn = p["turn_id"].as_str();
                let state = match (v["type"].as_str().unwrap_or(""), kind) {
                    ("event_msg", "task_started") => {
                        let Some(id) = turn else { continue };
                        c.turn = Some(id.into());
                        c.pending.clear();
                        Status::Working
                    }
                    ("event_msg", "task_complete" | "task_completed") => {
                        let Some(id) = turn.or(c.turn.as_deref()) else {
                            continue;
                        };
                        c.turn = Some(id.into());
                        if c.pending.is_empty() {
                            Status::Done
                        } else {
                            Status::Waiting
                        }
                    }
                    ("event_msg", "turn_aborted") => {
                        if let Some(id) = turn {
                            c.turn = Some(id.into());
                        }
                        c.pending.clear();
                        Status::Stopped
                    }
                    ("event_msg", "token_count" | "item_completed")
                        if c.state == Some(Status::Working) =>
                    {
                        Status::Working
                    }
                    ("response_item", "function_call")
                        if !c.state.is_some_and(Status::terminal) =>
                    {
                        let name = p["name"]
                            .as_str()
                            .unwrap_or("")
                            .rsplit('.')
                            .next()
                            .unwrap_or("");
                        if !matches!(name, "request_user_input" | "request_user_input_async") {
                            continue;
                        }
                        let Some(id) = p["call_id"].as_str() else {
                            continue;
                        };
                        c.pending.insert(id.into(), name.ends_with("_async"));
                        Status::Waiting
                    }
                    ("response_item", "function_call_output")
                        if c.state == Some(Status::Waiting) =>
                    {
                        let id = p["call_id"].as_str().unwrap_or("");
                        if c.pending.get(id) != Some(&false) {
                            continue;
                        }
                        c.pending.remove(id);
                        if c.pending.is_empty() {
                            Status::Working
                        } else {
                            Status::Waiting
                        }
                    }
                    ("response_item", "message")
                        if p["role"] == "user" && c.state == Some(Status::Waiting) =>
                    {
                        // Use only the reply envelope and call id; never retain the answer text.
                        let content = p["content"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|v| v["text"].as_str())
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            })
                            .unwrap_or_default();
                        if !content.contains("<send_user_message_question_reply>") {
                            continue;
                        }
                        c.pending.retain(|id, _| !content.contains(id.as_str()));
                        if c.pending.is_empty() {
                            Status::Working
                        } else {
                            Status::Waiting
                        }
                    }
                    _ => continue,
                };
                let Some(id) = c.turn.clone() else { continue };
                // Some files contain trailing telemetry after completion. It cannot
                // reopen the completed turn because heartbeat uses c.state above.
                c.state = Some(state);
                let detail = match state {
                    Status::Working => "正在处理本机任务",
                    Status::Done => "本轮 Agent 已结束",
                    Status::Stopped => "本轮任务已中止",
                    Status::Waiting => "需要你确认，请回到 Codex",
                    _ => "",
                };
                out.push((
                    Event {
                        task_id: id,
                        thread_id: c.thread.clone(),
                        seq,
                        status: state,
                        title: c.title.clone(),
                        detail: detail.into(),
                        progress: None,
                        agent: "Codex".into(),
                    },
                    at,
                ));
            }
            if consumed == 0 && data.len() == 2 * 1024 * 1024 {
                c.offset += data.len() as u64;
                c.discarding = true;
            } else {
                c.offset += consumed as u64;
            }
        }
        // Collapse heartbeat bursts; a newest terminal state is sufficient on replay.
        let mut newest: HashMap<String, (Event, i64)> = HashMap::new();
        for (e, t) in out {
            let replace = newest
                .get(&e.task_id)
                .is_none_or(|(old, _)| e.seq > old.seq);
            if replace {
                newest.insert(e.task_id.clone(), (e, t));
            }
        }
        Ok(newest.into_values().collect())
    }
}
fn read_titles(home: &Path) -> HashMap<String, String> {
    let mut result = HashMap::new();
    if let Ok(mut file) = File::open(home.join("session_index.jsonl")) {
        let size = file.metadata().map(|m| m.len()).unwrap_or(0);
        let _ = file.seek(SeekFrom::Start(size.saturating_sub(4 * 1024 * 1024)));
        let mut data = String::new();
        if file.take(4 * 1024 * 1024).read_to_string(&mut data).is_ok() {
            for line in data.lines() {
                if let Ok(v) = serde_json::from_str::<Value>(line) {
                    if let (Some(id), Some(title)) = (v["id"].as_str(), v["thread_name"].as_str()) {
                        if !title.trim().is_empty() {
                            result.insert(id.into(), title.into());
                        }
                    }
                }
            }
        }
    }
    result
}
fn collect(root: &Path, depth: usize, out: &mut Vec<(PathBuf, u64)>) -> Result<()> {
    if depth > 3 {
        return Ok(());
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    for ent in fs::read_dir(root)? {
        let ent = ent?;
        let ft = ent.file_type()?;
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            if ent
                .file_name()
                .to_string_lossy()
                .chars()
                .all(|c| c.is_ascii_digit())
            {
                collect(&ent.path(), depth + 1, out)?;
            }
            continue;
        }
        if !ft.is_file() || ent.path().extension().is_none_or(|s| s != "jsonl") {
            continue;
        }
        let m = ent
            .metadata()?
            .modified()?
            .duration_since(UNIX_EPOCH)?
            .as_secs();
        if now.saturating_sub(m) < 7 * 86400 && !is_subagent_session(&ent.path())? {
            out.push((ent.path(), m));
        }
    }
    Ok(())
}
fn is_subagent_session(path: &Path) -> Result<bool> {
    // Only the first metadata record owns the file. Spawned agents can contain
    // copied parent metadata, lifecycle events and synthetic parent aborts.
    // Exclude them before the 24-chat limit as well as before event ingestion.
    let mut first = Vec::new();
    BufReader::new(File::open(path)?)
        .take(1024 * 1024)
        .read_until(b'\n', &mut first)?;
    let Ok(meta) = serde_json::from_slice::<Value>(&first) else {
        return Ok(false);
    };
    let source = &meta["payload"]["source"];
    Ok(meta["type"] == "session_meta"
        && (source.get("subagent").is_some() || source.as_str() == Some("subagent")))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subagent_history_cannot_stop_parent_or_fill_recent_chat_slots() {
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&root).unwrap();
        let line = |kind: &str, payload: Value| {
            format!(
                "{}\n",
                serde_json::json!({"type":kind,"timestamp":"2026-10-17T03:00:00Z","payload":payload})
            )
        };
        let parent_meta = line(
            "session_meta",
            serde_json::json!({"id":"parent-chat","source":"vscode"}),
        );
        let parent_start = line(
            "event_msg",
            serde_json::json!({"type":"task_started","turn_id":"parent-turn"}),
        );
        fs::write(
            root.join("parent.jsonl"),
            format!("{parent_meta}{parent_start}"),
        )
        .unwrap();
        // A real spawned-agent log starts with its own metadata, followed by
        // copied parent history and a synthetic abort of the parent's turn.
        for i in 0..25 {
            let child_meta = line(
                "session_meta",
                serde_json::json!({
                    "id":format!("child-{i}"),
                    "source":{"subagent":{"thread_spawn":{"parent_thread_id":"parent-chat"}}},
                    "forked_from_id":"parent-chat"
                }),
            );
            let abort = line(
                "event_msg",
                serde_json::json!({"type":"turn_aborted","turn_id":"parent-turn"}),
            );
            let own_start = line(
                "event_msg",
                serde_json::json!({"type":"task_started","turn_id":format!("child-turn-{i}")}),
            );
            let done = line(
                "event_msg",
                serde_json::json!({"type":"task_complete","turn_id":format!("child-turn-{i}")}),
            );
            fs::write(
                root.join(format!("child-{i}.jsonl")),
                format!("{child_meta}{parent_meta}{parent_start}{abort}{own_start}{done}"),
            )
            .unwrap();
        }
        let mut watcher = Watcher::new(root.clone());
        let events = watcher.scan().unwrap();
        assert_eq!(
            events.len(),
            1,
            "Only the user-facing parent chat should be tracked"
        );
        assert_eq!(events[0].0.task_id, "parent-turn");
        assert_eq!(events[0].0.thread_id.as_deref(), Some("parent-chat"));
        assert_eq!(events[0].0.status, Status::Working);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uses_chat_title_and_waits_for_async_answer_without_publishing_it() {
        use std::io::Write;
        let home = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let root = home.join("sessions");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            home.join("session_index.jsonl"),
            "{\"id\":\"chat-1\",\"thread_name\":\"改进宠物应用\"}\n",
        )
        .unwrap();
        let p = root.join("chat.jsonl");
        let mut f = File::create(&p).unwrap();
        let mut append = |kind: &str, payload: Value| {
            writeln!(f, "{}", serde_json::json!({"type":kind,"timestamp":"2026-10-14T00:00:00Z","payload":payload})).unwrap();
        };
        append(
            "session_meta",
            serde_json::json!({"id":"chat-1","cwd":"/generic/folder","base_instructions":"x".repeat(20000)}),
        );
        append(
            "event_msg",
            serde_json::json!({"type":"task_started","turn_id":"turn-1"}),
        );
        let mut w = Watcher::new(root);
        let first = w.scan().unwrap();
        assert_eq!(first[0].0.title, "改进宠物应用");
        assert_eq!(first[0].0.thread_id.as_deref(), Some("chat-1"));
        append(
            "response_item",
            serde_json::json!({"type":"function_call","name":"request_user_input_async","call_id":"call-1","arguments":"PRIVATE QUESTION"}),
        );
        assert_eq!(w.scan().unwrap()[0].0.status, Status::Waiting);
        append(
            "response_item",
            serde_json::json!({"type":"function_call_output","call_id":"call-1","output":"{\"accepted\":true}"}),
        );
        append("event_msg", serde_json::json!({"type":"token_count"}));
        assert!(w.scan().unwrap().is_empty());
        append(
            "event_msg",
            serde_json::json!({"type":"task_complete","turn_id":"turn-1"}),
        );
        assert_eq!(w.scan().unwrap()[0].0.status, Status::Waiting);
        append(
            "response_item",
            serde_json::json!({"type":"message","role":"user","content":[{"text":"<send_user_message_question_reply>call-1 PRIVATE ANSWER</send_user_message_question_reply>"}]}),
        );
        let resumed = w.scan().unwrap();
        assert_eq!(resumed[0].0.status, Status::Working);
        assert!(!serde_json::to_string(&resumed[0].0)
            .unwrap()
            .contains("PRIVATE"));
        append(
            "event_msg",
            serde_json::json!({"type":"task_complete","turn_id":"turn-1"}),
        );
        assert_eq!(w.scan().unwrap()[0].0.status, Status::Done);
        fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn follows_actual_lifecycle_handles_partial_lines_without_exposing_content() {
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&root).unwrap();
        let p = root.join("session.jsonl");
        let start="{\"type\":\"event_msg\",\"timestamp\":\"2026-10-14T00:00:00Z\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"turn-1\"}}\n";
        fs::write(&p,format!("{start}{{\"type\":\"response_item\",\"payload\":{{\"text\":\"private-secret\"}}}}\n{{\"type\":")).unwrap();
        let mut w = Watcher::new(root.clone());
        let events = w.scan().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0.status, Status::Working);
        assert!(!serde_json::to_string(&events[0].0)
            .unwrap()
            .contains("private-secret"));
        use std::io::Write;
        let mut f = fs::OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(b"\"event_msg\",\"timestamp\":\"2026-10-14T00:01:00Z\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"turn-1\"}}\n").unwrap();
        assert_eq!(w.scan().unwrap()[0].0.status, Status::Done);
        assert!(w.scan().unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}

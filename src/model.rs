use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Working,
    Waiting,
    Done,
    Error,
    Stopped,
    Unknown,
}
impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Working => "正在工作",
            Self::Waiting => "等你处理",
            Self::Done => "任务完成",
            Self::Error => "遇到问题",
            Self::Stopped => "已停止",
            Self::Unknown => "状态待确认",
        }
    }
    pub fn terminal(self) -> bool {
        matches!(self, Self::Done | Self::Error | Self::Stopped)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub seq: u64,
    pub status: Status,
    pub title: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub progress: Option<u8>,
    #[serde(default)]
    pub agent: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Task {
    pub event: Event,
    pub source: String,
    pub updated_at: i64,
    pub stale: bool,
}
impl Task {
    pub fn label(&self) -> &'static str {
        if self.status() == Status::Waiting && self.event.detail.starts_with("权限审批") {
            "等待权限审批"
        } else {
            self.status().label()
        }
    }
    pub fn status(&self) -> Status {
        if self.stale && !self.event.status.terminal() {
            Status::Unknown
        } else {
            self.event.status
        }
    }
    pub fn key(&self) -> String {
        format!("{}:{}", self.source, self.event.task_id)
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub codex_enabled: bool,
    pub animation: bool,
    pub stale_minutes: u32,
    pub sound_enabled: bool,
    pub volume: u8,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            codex_enabled: false,
            animation: true,
            stale_minutes: 15,
            sound_enabled: true,
            volume: 90,
        }
    }
}
#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct Model {
    #[serde(default)]
    pub approvals: crate::approval::Approvals,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(skip)]
    pub selected: Option<String>,
    #[serde(skip)]
    pub demo: Option<Status>,
}
fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 160
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_:.".contains(&b))
}
pub fn clean(s: &str, max: usize) -> String {
    s.chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(max)
        .collect()
}
pub const TASKS_PER_PAGE: usize = 4;

impl Model {
    pub fn apply(&mut self, mut event: Event, source: &str, at: i64) -> Result<bool> {
        if !valid_id(&event.task_id) || event.seq == 0 {
            bail!("task_id 须为字母、数字或 -_: .，seq 须大于 0")
        }
        if event.title.trim().is_empty()
            || event.title.chars().count() > 120
            || event.detail.chars().count() > 240
            || event.agent.chars().count() > 40
        {
            bail!("任务标题不能为空，标题/详情/Agent 名称最多 120/240/40 个字符")
        }
        if event.progress.is_some_and(|p| p > 100) {
            bail!("进度必须为 0–100，未知时省略")
        }
        event.title = clean(event.title.trim(), 120);
        event.detail = clean(&event.detail, 240);
        event.agent = clean(&event.agent, 40);
        if event.title.is_empty() {
            bail!("任务标题无有效字符")
        }
        if let Some(t) = self
            .tasks
            .iter_mut()
            .find(|t| t.source == source && t.event.task_id == event.task_id)
        {
            if event.seq <= t.event.seq {
                return Ok(false);
            }
            if t.event.status.terminal() {
                bail!("任务已经结束；新任务请使用新的 task_id")
            }
            *t = Task {
                event,
                source: source.into(),
                updated_at: at,
                stale: false,
            };
        } else {
            self.tasks.push(Task {
                event,
                source: source.into(),
                updated_at: at,
                stale: false,
            });
        }
        self.tasks
            .sort_by_key(|t| std::cmp::Reverse((t.updated_at, t.event.seq)));
        self.tasks.truncate(100);
        Ok(true)
    }
    pub fn visible(&self, now: i64) -> Vec<Task> {
        let mut tasks: Vec<_> = self
            .tasks
            .iter()
            .filter(|t| t.source != "codex" || self.settings.codex_enabled)
            .cloned()
            .collect();
        if self.settings.codex_enabled {
            for approval in self.approvals.pending() {
                if let Some(t) = tasks
                    .iter_mut()
                    .find(|t| t.source == "codex" && t.event.task_id == approval.turn_id)
                {
                    if t.event.status.terminal() {
                        continue;
                    }
                    t.event.status = Status::Waiting;
                    t.event.detail = approval.detail().into();
                    t.updated_at = t.updated_at.max(approval.at);
                    t.stale = false;
                } else {
                    tasks.push(Task {
                        event: Event {
                            task_id: approval.turn_id.clone(),
                            thread_id: Some(approval.thread_id.clone()),
                            seq: 0,
                            status: Status::Waiting,
                            title: if approval.title.is_empty() {
                                "Codex 权限审批".into()
                            } else {
                                approval.title.clone()
                            },
                            detail: approval.detail().into(),
                            progress: None,
                            agent: "Codex".into(),
                        },
                        source: "codex".into(),
                        updated_at: approval.at,
                        stale: false,
                    });
                }
            }
        }
        tasks.sort_by_key(|t| std::cmp::Reverse((t.updated_at, t.event.seq)));
        for t in &mut tasks {
            if now.saturating_sub(t.updated_at) > self.settings.stale_minutes as i64 * 60 {
                t.stale = true;
            }
        }
        tasks
    }
    /// Latest turn per Codex chat; old unfinished turns must not appear as parallel work.
    pub fn live(&self, now: i64) -> Vec<Task> {
        let mut seen = std::collections::HashSet::new();
        self.visible(now)
            .into_iter()
            .filter(|t| {
                if t.source == "codex" {
                    if let Some(id) = &t.event.thread_id {
                        if !seen.insert(id.clone()) {
                            return false;
                        }
                    }
                }
                true
            })
            .collect()
    }
    pub fn running(&self, now: i64) -> Vec<Task> {
        let mut tasks: Vec<_> = self
            .live(now)
            .into_iter()
            .filter(|t| matches!(t.status(), Status::Working | Status::Waiting))
            .collect();
        // Heartbeats and waiting transitions must not move a task between rows.
        tasks.sort_by(|a, b| {
            (
                &a.source,
                a.event.thread_id.as_ref().unwrap_or(&a.event.task_id),
            )
                .cmp(&(
                    &b.source,
                    b.event.thread_id.as_ref().unwrap_or(&b.event.task_id),
                ))
        });
        tasks
    }
    pub fn current(&self, now: i64) -> Option<Task> {
        if let Some(status) = self.demo {
            return Some(Task {
                event: Event {
                    task_id: "demo".into(),
                    thread_id: None,
                    seq: 1,
                    status,
                    title: "整理今天的待办".into(),
                    detail: "状态演示，不代表真实任务".into(),
                    progress: if status == Status::Working {
                        Some(60)
                    } else {
                        None
                    },
                    agent: "演示".into(),
                },
                source: "demo".into(),
                updated_at: now,
                stale: false,
            });
        }
        let active = self.running(now);
        let tasks = if active.len() > 1 {
            active
        } else {
            self.visible(now)
        };
        if let Some(key) = &self.selected {
            if let Some(t) = tasks.iter().find(|t| &t.key() == key) {
                return Some(t.clone());
            }
        }
        let tasks = self.live(now);
        tasks
            .iter()
            .find(|t| t.status() == Status::Waiting)
            .or_else(|| tasks.iter().find(|t| t.status() == Status::Working))
            .or_else(|| tasks.first())
            .cloned()
    }
    pub fn cycle(&mut self, step: i32, now: i64) {
        self.demo = None;
        let active = self.running(now);
        if active.len() > TASKS_PER_PAGE {
            let page = self
                .selected
                .as_ref()
                .and_then(|key| active.iter().position(|t| &t.key() == key))
                .unwrap_or(0)
                / TASKS_PER_PAGE;
            let pages = active.len().div_ceil(TASKS_PER_PAGE);
            let next = (page as i32 + step).rem_euclid(pages as i32) as usize;
            self.selected = Some(active[next * TASKS_PER_PAGE].key());
            return;
        }
        let tasks = if active.len() > 1 {
            active
        } else {
            self.visible(now)
        };
        if tasks.is_empty() {
            self.selected = None;
            return;
        }
        let key = self.current(now).map(|t| t.key());
        let idx = tasks.iter().position(|t| Some(t.key()) == key).unwrap_or(0) as i32;
        self.selected = Some(tasks[(idx + step).rem_euclid(tasks.len() as i32) as usize].key());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ev(seq: u64, status: Status) -> Event {
        Event {
            task_id: "test".into(),
            thread_id: None,
            seq,
            status,
            title: "真实任务".into(),
            detail: String::new(),
            progress: None,
            agent: "Agent".into(),
        }
    }
    #[test]
    fn approval_keeps_task_name_and_survives_work_heartbeats() {
        let mut m = Model::default();
        m.settings.codex_enabled = true;
        let mut e = ev(1, Status::Working);
        e.task_id = "turn-a".into();
        e.thread_id = Some("chat-a".into());
        m.apply(e.clone(), "codex", 100).unwrap();
        let mut signal = crate::approval::Signal {
            event: "PermissionRequest".into(),
            thread_id: "chat-a".into(),
            turn_id: "turn-a".into(),
            request_id: "request-a".into(),
            tool_name: "Bash".into(),
            title: "fallback".into(),
            at: 100,
        };
        m.approvals.apply(&signal, 100).unwrap();
        assert_eq!(m.current(100).unwrap().event.title, "真实任务");
        assert_eq!(m.current(100).unwrap().label(), "等待权限审批");
        e.seq = 2;
        m.apply(e, "codex", 101).unwrap();
        assert_eq!(m.current(101).unwrap().status(), Status::Waiting);
        assert_eq!(m.running(101).len(), 1);
        signal.event = "PostToolUse".into();
        m.approvals.apply(&signal, 101).unwrap();
        assert_eq!(m.current(101).unwrap().status(), Status::Working);
    }
    #[test]
    fn running_shows_multiple_chats_but_not_old_turns_or_stale_work() {
        let mut m = Model::default();
        m.settings.codex_enabled = true;
        let mut e = ev(1, Status::Working);
        e.thread_id = Some("chat-a".into());
        m.apply(e.clone(), "codex", 10).unwrap();
        e.task_id = "new-turn".into();
        e.seq = 2;
        e.status = Status::Done;
        m.apply(e.clone(), "codex", 10).unwrap();
        assert!(m.running(11).is_empty());
        e.task_id = "other".into();
        e.thread_id = Some("chat-b".into());
        e.status = Status::Working;
        m.apply(e.clone(), "codex", 12).unwrap();
        e.task_id = "third".into();
        e.thread_id = Some("chat-c".into());
        e.status = Status::Waiting;
        m.apply(e, "codex", 13).unwrap();
        assert_eq!(m.running(14).len(), 2);
        assert_eq!(m.running(14)[0].event.task_id, "other");
        assert_eq!(m.running(14)[1].status(), Status::Waiting);
        assert_eq!(m.current(14).unwrap().status(), Status::Waiting);
        m.selected = Some("codex:new-turn".into());
        assert_eq!(m.current(14).unwrap().status(), Status::Waiting);
        let mut update = m.running(14)[0].event.clone();
        update.seq += 1;
        update.status = Status::Waiting;
        m.apply(update, "codex", 15).unwrap();
        assert_eq!(m.running(15)[0].event.task_id, "other");
        assert!(m.running(2000).is_empty());
    }
    #[test]
    fn ordered_idempotent_and_terminal() {
        let mut m = Model::default();
        assert!(m.apply(ev(2, Status::Working), "agent", 10).unwrap());
        assert!(!m.apply(ev(1, Status::Done), "agent", 11).unwrap());
        assert!(m.apply(ev(3, Status::Done), "agent", 12).unwrap());
        assert!(m.apply(ev(4, Status::Working), "agent", 13).is_err());
        assert_eq!(m.current(14).unwrap().status(), Status::Done);
    }
    #[test]
    fn stale_never_becomes_success_and_unknown_progress_stays_unknown() {
        let mut m = Model::default();
        m.apply(ev(1, Status::Working), "agent", 10).unwrap();
        let t = m.current(1000).unwrap();
        assert_eq!(t.status(), Status::Unknown);
        assert_eq!(t.event.progress, None);
        m.apply(ev(2, Status::Done), "agent", 1001).unwrap();
        assert_eq!(m.current(3000).unwrap().status(), Status::Done);
    }
    #[test]
    fn sources_are_isolated_and_demo_is_not_persisted() {
        let mut m = Model::default();
        m.apply(ev(1, Status::Working), "codex", 10).unwrap();
        assert!(m.current(11).is_none());
        m.apply(ev(1, Status::Waiting), "agent", 11).unwrap();
        m.demo = Some(Status::Done);
        assert_eq!(m.current(12).unwrap().source, "demo");
        let saved = serde_json::to_string(&m).unwrap();
        let restored: Model = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored.current(12).unwrap().status(), Status::Waiting);
    }
}

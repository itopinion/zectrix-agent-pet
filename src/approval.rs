use crate::model::clean;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signal {
    pub event: String,
    pub thread_id: String,
    #[serde(default)]
    pub turn_id: String,
    #[serde(default)]
    pub request_id: String,
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub title: String,
    pub at: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pending {
    pub thread_id: String,
    pub turn_id: String,
    pub request_id: String,
    pub title: String,
    pub tool_name: String,
    pub at: i64,
    pub resolved: bool,
}
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct Approvals {
    #[serde(default)]
    pub requests: Vec<Pending>,
    #[serde(default)]
    pub closed_turns: Vec<(String, String, i64)>,
}
fn id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 160
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_: .".contains(&c))
}
impl Approvals {
    pub fn apply(&mut self, s: &Signal, now: i64) -> Result<bool> {
        ensure!(
            id(&s.thread_id) && (s.turn_id.is_empty() || id(&s.turn_id)),
            "审批事件标识无效"
        );
        ensure!(s.at <= now + 10 && s.at >= now - 30, "审批通知已过期");
        ensure!(
            matches!(
                s.event.as_str(),
                "PermissionRequest" | "PostToolUse" | "Stop" | "Interrupt" | "SessionEnd"
            ),
            "不支持的审批通知"
        );
        if matches!(s.event.as_str(), "PermissionRequest" | "PostToolUse") {
            ensure!(id(&s.turn_id) && id(&s.request_id), "审批通知缺少标识");
        }
        if s.event == "PermissionRequest" {
            if self.closed_turns.iter().any(|(thread, turn, at)| {
                thread == &s.thread_id && (turn.is_empty() || turn == &s.turn_id) && *at >= s.at
            }) {
                return Ok(false);
            }
            if self.requests.iter().any(|r| {
                r.thread_id == s.thread_id && r.turn_id == s.turn_id && r.request_id == s.request_id
            }) {
                return Ok(false);
            }
            self.requests.push(Pending {
                thread_id: s.thread_id.clone(),
                turn_id: s.turn_id.clone(),
                request_id: s.request_id.clone(),
                title: clean(&s.title, 120),
                tool_name: clean(&s.tool_name, 80),
                at: s.at,
                resolved: false,
            });
            if self.requests.len() > 200 {
                self.requests.remove(0);
            }
            return Ok(true);
        }
        for r in &mut self.requests {
            if r.thread_id == s.thread_id
                && (s.turn_id.is_empty() || r.turn_id == s.turn_id)
                && (s.event != "PostToolUse" || r.request_id == s.request_id)
            {
                r.resolved = true;
            }
        }
        if s.event != "PostToolUse" {
            self.closed_turns
                .push((s.thread_id.clone(), s.turn_id.clone(), s.at));
            if self.closed_turns.len() > 100 {
                self.closed_turns.remove(0);
            }
        }
        Ok(false)
    }
    pub fn pending(&self) -> impl Iterator<Item = &Pending> {
        self.requests.iter().filter(|r| !r.resolved)
    }
}
impl Pending {
    pub fn detail(&self) -> &'static str {
        match self.tool_name.as_str() {
            "Bash" => "权限审批 · 执行命令",
            "apply_patch" => "权限审批 · 修改文件",
            _ => "权限审批 · 工具访问",
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn signal(event: &str, request: &str) -> Signal {
        Signal {
            event: event.into(),
            thread_id: "thread-a".into(),
            turn_id: "turn-a".into(),
            request_id: request.into(),
            tool_name: "Bash".into(),
            title: "任务名称".into(),
            at: 100,
        }
    }
    #[test]
    fn deduplicates_and_resolves_only_the_matching_approval() {
        let mut a = Approvals::default();
        assert!(a.apply(&signal("PermissionRequest", "a"), 100).unwrap());
        assert!(!a.apply(&signal("PermissionRequest", "a"), 100).unwrap());
        assert!(a.apply(&signal("PermissionRequest", "b"), 100).unwrap());
        a.apply(&signal("PostToolUse", "a"), 100).unwrap();
        assert_eq!(a.pending().count(), 1);
        a.apply(&signal("Stop", ""), 100).unwrap();
        assert_eq!(a.pending().count(), 0);
        assert!(!a.apply(&signal("PermissionRequest", "b"), 100).unwrap());
        assert!(!a.apply(&signal("PermissionRequest", "late"), 100).unwrap());
        assert!(a.apply(&signal("PermissionRequest", "stale"), 200).is_err());
    }
}

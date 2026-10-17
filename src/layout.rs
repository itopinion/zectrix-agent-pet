use crate::model::{Model, Status, TASKS_PER_PAGE};
use serde_json::{json, Value};

// Original pixel companion from the earlier AI 工作搭子 prototype, expressed as
// horizontal runs rather than a full-screen bitmap to stay within Hub v1's 200 elements.
const PET: [&str; 16] = [
    "..###......###..",
    "..#..#....#..#..",
    "..#...####...#..",
    ".#............#.",
    "#..............#",
    "#..............#",
    "#..............#",
    "#..............#",
    ".#............#.",
    "..#..........#..",
    "...##########...",
    "....#......#....",
    "....#......#....",
    "...#........#...",
    "..############..",
    "..##........##..",
];
fn rect(e: &mut Vec<Value>, x: i32, y: i32, w: i32, h: i32, fill: Option<u8>) {
    let mut v = json!({"type":"rect","x":x,"y":y,"w":w,"h":h});
    if let Some(f) = fill {
        v["fill"] = json!(f);
    }
    e.push(v);
}
// Restore the original raised arms, smiling face and three sparkles. Rasterize
// only these small strokes into merged rectangles supported by Hub v1.
fn cheer(e: &mut Vec<Value>, x: i32, y: i32, p: i32) {
    let mut pixels = std::collections::BTreeSet::new();
    let mut stroke = |points: &[(i32, i32)], width: i32| {
        for segment in points.windows(2) {
            let ((x0, y0), (x1, y1)) = (segment[0], segment[1]);
            let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
            for i in 0..=steps {
                let px = x0 + ((x1 - x0) as f64 * i as f64 / steps as f64).round() as i32;
                let py = y0 + ((y1 - y0) as f64 * i as f64 / steps as f64).round() as i32;
                for dy in 0..width {
                    for dx in 0..width {
                        pixels.insert((py + dy - width / 2, px + dx - width / 2));
                    }
                }
            }
        }
    };
    for ex in [x + 4 * p, x + 10 * p] {
        stroke(
            &[
                (ex, y + 6 * p),
                (ex + p, y + 5 * p),
                (ex + 2 * p, y + 6 * p),
            ],
            2,
        );
    }
    stroke(
        &[
            (x + 6 * p, y + 8 * p),
            (x + 7 * p, y + 9 * p),
            (x + 9 * p, y + 9 * p),
            (x + 10 * p, y + 8 * p),
        ],
        2,
    );
    stroke(&[(x + 3 * p, y + 12 * p), (x - p, y + 9 * p)], 3);
    stroke(&[(x + 13 * p, y + 12 * p), (x + 17 * p, y + 9 * p)], 3);
    for (xx, yy) in [(x - 7, y - 10), (x + 100, y - 11), (x + 115, y + 30)] {
        stroke(&[(xx, yy), (xx + 7, yy + 7)], 2);
        stroke(&[(xx + 7, yy), (xx, yy + 7)], 2);
    }
    let mut spans: Vec<(i32, i32, i32)> = vec![];
    for (py, px) in pixels {
        if let Some((row, start, width)) = spans.last_mut() {
            if *row == py && *start + *width == px {
                *width += 1;
                continue;
            }
        }
        spans.push((py, px, 1));
    }
    let mut blocks: Vec<(i32, i32, i32, i32)> = vec![];
    for (py, px, width) in spans {
        if let Some(block) = blocks
            .iter_mut()
            .rev()
            .find(|b| b.0 == px && b.2 == width && b.1 + b.3 == py)
        {
            block.3 += 1;
        } else {
            blocks.push((px, py, width, 1));
        }
    }
    for (px, py, w, h) in blocks {
        rect(e, px, py, w, h, Some(0));
    }
}
fn text(e: &mut Vec<Value>, s: &str, x: i32, y: i32, size: i32, width: i32) {
    e.push(json!({"type":"text","content":s,"x":x,"y":y,"size":size,"width_limit":width,"weight":if size>=24 {"bold"} else {"regular"}}));
}
fn title_lines(value: &str, width: usize, max: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    let mut used = 0;
    for ch in value.chars() {
        let w = if ch.is_ascii() { 1 } else { 2 };
        if used + w > width {
            if lines.len() == max {
                let last = lines.last_mut().unwrap();
                last.pop();
                last.push('…');
                break;
            }
            lines.push(String::new());
            used = 0;
        }
        lines.last_mut().unwrap().push(ch);
        used += w;
    }
    lines
}
pub fn draw(model: &Model, now: i64, frame: usize) -> Value {
    let t = model.current(now);
    let state = t.as_ref().map(|t| t.status());
    let working = state == Some(Status::Working);
    let animate = working && model.settings.animation;
    let phase = if animate { frame % 6 } else { 0 };
    let mut e = vec![];
    text(&mut e, "ZECTRIX / AGENT PET", 16, 19, 12, 260);
    text(
        &mut e,
        if model.demo.is_some() {
            "演示"
        } else {
            "实时"
        },
        341,
        19,
        12,
        44,
    );
    text(&mut e, "AI 工作搭子", 16, 59, 28, 340);
    rect(&mut e, 16, 82, 368, 0, None);
    let active = model.running(now);
    if active.len() > 1 && model.demo.is_none() {
        let pages = active.len().div_ceil(TASKS_PER_PAGE);
        let page = model
            .selected
            .as_ref()
            .and_then(|key| active.iter().position(|t| &t.key() == key))
            .unwrap_or(0)
            / TASKS_PER_PAGE;
        let heading = if pages > 1 {
            format!("{} 个任务 · {}/{}", active.len(), page + 1, pages)
        } else {
            format!("{} 个任务", active.len())
        };
        text(&mut e, &heading, 20, 112, 20, 224);
        let row_height = match active.len() {
            2 => 62,
            3 => 45,
            _ => 36,
        };
        for (i, task) in active
            .iter()
            .skip(page * TASKS_PER_PAGE)
            .take(TASKS_PER_PAGE)
            .enumerate()
        {
            let y = 143 + i as i32 * row_height;
            let title = title_lines(&task.event.title, 24, 1).join("");
            text(&mut e, &title, 20, y, 17, 220);
            let status = match task.event.progress {
                Some(p) => format!("{} · {}%", task.label(), p),
                None => task.label().into(),
            };
            text(&mut e, &status, 20, y + 16, 11, 220);
        }
    } else {
        let headline = t.as_ref().map(|t| t.label()).unwrap_or("等待任务");
        text(&mut e, headline, 20, 116, 22, 224);
        let title =
            t.as_ref()
                .map(|t| t.event.title.as_str())
                .unwrap_or(if model.settings.codex_enabled {
                    "Codex 跟随已开启"
                } else {
                    "本机 Codex 跟随已关闭"
                });
        for (line, title) in title_lines(title, 24, 3).iter().enumerate() {
            text(&mut e, title, 20, 146 + line as i32 * 21, 18, 220);
        }
        let detail = t
            .as_ref()
            .map(|t| {
                if t.stale && !t.event.status.terminal() {
                    "一段时间未收到更新"
                } else if t.event.detail.starts_with("权限审批") {
                    t.event.detail.as_str()
                } else if t.status() == Status::Waiting {
                    "请回到 Codex / Agent 确认"
                } else if t.status() == Status::Done {
                    "本轮任务已结束"
                } else {
                    t.event.detail.as_str()
                }
            })
            .unwrap_or(if model.settings.codex_enabled {
                "在本机 Codex 中开始一个任务"
            } else {
                "请在网页开启跟随或接入 Agent"
            });
        text(
            &mut e,
            &title_lines(detail, 30, 1).join(""),
            20,
            213,
            13,
            220,
        );
        if let Some(p) = t.as_ref().and_then(|t| t.event.progress) {
            rect(&mut e, 20, 228, 212, 13, None);
            if p > 0 {
                rect(
                    &mut e,
                    23,
                    231,
                    (206u32 * p as u32 / 100) as i32,
                    7,
                    Some(0),
                );
            }
            text(&mut e, &format!("Agent 上报进度 {p}%"), 20, 258, 12, 218);
        } else if working {
            text(&mut e, "进行中 · 未提供百分比", 20, 248, 12, 218);
        }
    }
    let x = 254;
    let y = 103;
    let p = 7;
    let dx = if animate {
        [0, 1, 2, 1, 0, -1][phase]
    } else {
        0
    };
    for (row, line) in PET.iter().enumerate() {
        let bytes = line.as_bytes();
        let mut col = 0;
        while col < bytes.len() {
            if bytes[col] != b'#' {
                col += 1;
                continue;
            }
            let start = col;
            while col < bytes.len() && bytes[col] == b'#' {
                col += 1;
            }
            rect(
                &mut e,
                x + start as i32 * p + if row <= 10 { dx } else { 0 },
                y + row as i32 * p,
                (col - start) as i32 * p - 1,
                p - 1,
                Some(0),
            );
        }
    }
    if state == Some(Status::Done) {
        cheer(&mut e, x, y, p);
    } else {
        for ex in [x + 4 * p + dx, x + 10 * p + dx] {
            if state == Some(Status::Error) {
                rect(&mut e, ex, y + 5 * p, p * 2, 3, Some(0));
                rect(&mut e, ex + p, y + 5 * p, 3, p * 2, Some(0));
            } else {
                rect(
                    &mut e,
                    ex,
                    y + 5 * p,
                    p,
                    if phase == 3 { 2 } else { p * 2 },
                    Some(0),
                );
            }
        }
        rect(
            &mut e,
            x + 7 * p + dx,
            y + 8 * p,
            p * 2,
            if state == Some(Status::Waiting) {
                p * 2
            } else {
                p
            },
            Some(0),
        );
    }
    if working {
        rect(&mut e, x + 2 * p, y + 13 * p, 12 * p, 3 * p, Some(255));
        rect(&mut e, x + 2 * p, y + 13 * p, 12 * p, 3 * p, None);
        for col in 0..4 {
            rect(&mut e, x + (4 + 2 * col) * p, y + 14 * p, p, 1, Some(0));
        }
        rect(
            &mut e,
            285,
            if phase % 2 == 0 { 193 } else { 185 },
            8,
            7,
            Some(0),
        );
        rect(
            &mut e,
            327,
            if phase % 2 == 1 { 193 } else { 185 },
            8,
            7,
            Some(0),
        );
    }
    if state == Some(Status::Done) {
        text(&mut e, "完成！", 274, 246, 18, 105);
    }
    if matches!(state, Some(Status::Waiting | Status::Unknown)) {
        text(&mut e, "?", 359, 99, 24, 20);
    }
    rect(&mut e, 16, 271, 368, 0, None);
    let source = t
        .as_ref()
        .map(|t| {
            if t.source == "demo" {
                "演示数据"
            } else if t.source == "codex" {
                "Codex 本机"
            } else if t.event.agent.is_empty() {
                "Agent 上报"
            } else {
                t.event.agent.as_str()
            }
        })
        .unwrap_or("本地运行");
    text(&mut e, source, 16, 285, 11, 143);
    text(
        &mut e,
        if active.len() > TASKS_PER_PAGE && model.demo.is_none() {
            "上下翻页 · 确认首页"
        } else {
            "上下选任务 · 确认跟随"
        },
        173,
        285,
        11,
        212,
    );
    json!({"canvas":{"w":400,"h":300},"elements":e})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_rows_survive_heartbeats_selection_and_time() {
        for count in [2usize, 3, 4, 5, 9, 10] {
            let mut m = Model::default();
            for i in 0..count {
                m.apply(
                    crate::model::Event {
                        task_id: format!("task-{i:03}"),
                        thread_id: None,
                        seq: 1,
                        status: Status::Working,
                        title: format!("任务名称{i}"),
                        detail: String::new(),
                        progress: None,
                        agent: "Test".into(),
                    },
                    "agent",
                    1,
                )
                .unwrap();
            }
            let baseline = draw(&m, 1, 0);
            assert_eq!(baseline, draw(&m, 60, 0), "no automatic rotation");
            let mut heartbeat = m.running(1)[0].event.clone();
            heartbeat.seq = 2;
            m.apply(heartbeat, "agent", 61).unwrap();
            assert_eq!(baseline, draw(&m, 61, 0), "heartbeat must not reorder rows");
            let pages = count.div_ceil(TASKS_PER_PAGE);
            for page in 0..pages {
                let v = draw(&m, 62, 0);
                let e = v["elements"].as_array().unwrap();
                let titles: Vec<_> = e
                    .iter()
                    .filter(|e| {
                        e["content"]
                            .as_str()
                            .is_some_and(|s| s.starts_with("任务名称"))
                    })
                    .collect();
                assert_eq!(
                    titles.len(),
                    (count - page * TASKS_PER_PAGE).min(TASKS_PER_PAGE)
                );
                for (row, title) in titles.iter().enumerate() {
                    assert_eq!(
                        title["content"],
                        format!("任务名称{}", page * TASKS_PER_PAGE + row)
                    );
                }
                assert!(titles
                    .windows(2)
                    .all(|w| w[0]["y"].as_i64() < w[1]["y"].as_i64()));
                assert!(!v.to_string().contains('%'));
                assert!(e.len() <= 200);
                if pages > 1 {
                    m.cycle(1, 62);
                }
            }
            assert_eq!(baseline, draw(&m, 62, 0), "page navigation wraps");
            m.selected = Some(m.running(62)[1].key());
            if count <= TASKS_PER_PAGE {
                assert_eq!(
                    baseline,
                    draw(&m, 62, 0),
                    "selection keeps all rows visible"
                );
            } else {
                m.selected = None;
                m.cycle(-1, 62);
                assert!(draw(&m, 62, 0)
                    .to_string()
                    .contains(&format!("{}/{}", pages, pages)));
            }
        }
    }
    #[test]
    fn layouts_obey_hub_limits() {
        let mut m = Model::default();
        for s in [
            Status::Working,
            Status::Waiting,
            Status::Done,
            Status::Error,
            Status::Stopped,
            Status::Unknown,
        ] {
            m.demo = Some(s);
            for frame in 0..6 {
                let v = draw(&m, 100, frame);
                let e = v["elements"].as_array().unwrap();
                assert!(e.len() <= 200);
                for v in e {
                    assert!(v["x"].as_i64().unwrap() >= 0);
                    assert!(v["y"].as_i64().unwrap() < 300);
                }
            }
        }
    }
}

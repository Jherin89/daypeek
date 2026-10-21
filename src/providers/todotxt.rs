//! [todo.txt](https://github.com/todotxt/todo.txt) files.
//!
//! `(A)` → p1, `(B)` → p2, `(C)` → p3, `(D)`–`(Z)` → p4. Due dates use the
//! `due:YYYY-MM-DD` extension. Completed lines (`x …`) are skipped.

use chrono::NaiveDate;
use serde::Deserialize;

use super::{settings, Ctx, Kind, Output, Provider};
use crate::config::expand_tilde;
use crate::error::{Error, Result};
use crate::model::{Priority, Task};

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub path: String,
    /// Show undated tasks too (e.g. everything with a priority).
    pub include_undated: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            path: "~/todo.txt".into(),
            include_undated: false,
        }
    }
}

pub struct TodoTxt {
    id: String,
    s: Settings,
}

pub fn build(id: &str, table: toml::Table) -> Result<Box<dyn Provider>> {
    Ok(Box::new(TodoTxt {
        id: id.into(),
        s: settings(id, table)?,
    }))
}

pub fn priority(c: char) -> Priority {
    match c {
        'A' => Priority::Urgent,
        'B' => Priority::High,
        'C' => Priority::Medium,
        'D'..='Z' => Priority::Low,
        _ => Priority::None,
    }
}

/// Parse one todo.txt line into (title, due, priority, first +project).
pub fn parse_line(line: &str) -> Option<(String, Option<NaiveDate>, Priority, Option<String>)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with("x ") || line == "x" {
        return None;
    }
    let mut rest = line;
    let mut prio = Priority::None;
    let b = rest.as_bytes();
    if b.len() >= 4 && b[0] == b'(' && b[2] == b')' && b[3] == b' ' && b[1].is_ascii_uppercase() {
        prio = priority(b[1] as char);
        rest = &rest[4..];
    }
    let mut words: Vec<&str> = rest.split_whitespace().collect();
    // Optional creation date right after the priority.
    if words
        .first()
        .is_some_and(|w| NaiveDate::parse_from_str(w, "%Y-%m-%d").is_ok())
    {
        words.remove(0);
    }
    let mut due = None;
    let mut project = None;
    let mut title = Vec::new();
    for w in words {
        if let Some(d) = w.strip_prefix("due:") {
            due = NaiveDate::parse_from_str(d, "%Y-%m-%d").ok();
        } else if w.starts_with("t:") || w.starts_with("rec:") || w.starts_with("pri:") {
            // other common key:value extensions are metadata
        } else {
            if project.is_none() && w.len() > 1 && w.starts_with('+') {
                project = Some(w[1..].to_string());
            }
            title.push(w);
        }
    }
    let title = title.join(" ");
    (!title.is_empty()).then_some((title, due, prio, project))
}

pub fn parse(text: &str, source: &str, include_undated: bool) -> Vec<Task> {
    text.lines()
        .filter_map(parse_line)
        .filter(|(_, due, _, _)| due.is_some() || include_undated)
        .map(|(title, due, prio, project)| {
            let mut t = Task::new(title, due, prio).with_context(project.unwrap_or_default());
            t.sources.push(source.to_string());
            t
        })
        .collect()
}

impl Provider for TodoTxt {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind_id(&self) -> &'static str {
        "todotxt"
    }
    fn name(&self) -> &'static str {
        "todo.txt"
    }
    fn kind(&self) -> Kind {
        Kind::Local
    }

    fn check(&self) -> Result<()> {
        let p = expand_tilde(&self.s.path);
        if p.is_file() {
            Ok(())
        } else {
            Err(Error::Config(format!("{} not found", p.display())))
        }
    }

    fn fetch(&self, _ctx: &Ctx) -> Result<Output> {
        let text = std::fs::read_to_string(expand_tilde(&self.s.path))?;
        Ok(Output::tasks(parse(
            &text,
            &self.id,
            self.s.include_undated,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture() {
        let tasks = parse(
            include_str!("../../tests/fixtures/todo.txt"),
            "todotxt",
            false,
        );
        let titles: Vec<_> = tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            titles,
            vec![
                "Call Mom +family @phone",
                "Renew car registration +errands",
                "Plan sprint"
            ]
        );
        assert_eq!(tasks[0].priority, Priority::Urgent);
        assert_eq!(tasks[0].context.as_deref(), Some("family"));
        assert_eq!(tasks[1].priority, Priority::High);
        assert_eq!(tasks[1].due, NaiveDate::from_ymd_opt(2026, 10, 18));
        assert_eq!(tasks[2].priority, Priority::None);
    }

    #[test]
    fn undated_opt_in() {
        let all = parse(include_str!("../../tests/fixtures/todo.txt"), "t", true);
        assert!(all.iter().any(|t| t.title.starts_with("Someday")));
    }

    #[test]
    fn priority_letters() {
        assert_eq!(priority('A'), Priority::Urgent);
        assert_eq!(priority('C'), Priority::Medium);
        assert_eq!(priority('Q'), Priority::Low);
        assert!(parse_line("x 2026-10-20 done thing due:2026-10-20").is_none());
        assert!(parse_line("(a) lowercase is not a priority").unwrap().2 == Priority::None);
    }
}

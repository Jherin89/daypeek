//! Apple Reminders (macOS) through a small bundled JXA script run by
//! `osascript`. The first run triggers macOS's "Terminal wants to control
//! Reminders" prompt; allow it once.

use serde::Deserialize;

use super::{leading_date, settings, Ctx, Kind, Output, Provider};
use crate::error::{Error, Result};
use crate::model::{Priority, Task};

/// The bridge script, embedded in the binary.
pub const BRIDGE_JS: &str = include_str!("../bridge/reminders.js");

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// List names to include; empty = all lists.
    pub lists: Vec<String>,
    /// Reminders can be slow to answer with large libraries.
    pub timeout_secs: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            lists: Vec::new(),
            timeout_secs: 10,
        }
    }
}

pub struct Reminders {
    id: String,
    s: Settings,
}

pub fn build(id: &str, table: toml::Table) -> Result<Box<dyn Provider>> {
    Ok(Box::new(Reminders {
        id: id.into(),
        s: settings(id, table)?,
    }))
}

#[derive(Debug, Deserialize)]
pub struct BridgeItem {
    pub title: String,
    #[serde(default)]
    pub list: String,
    pub due: String,
    #[serde(default)]
    pub priority: u8,
}

/// Reminders: 1–4 high, 5 medium, 6–9 low, 0 none.
pub fn priority(p: u8) -> Priority {
    match p {
        1..=4 => Priority::Urgent,
        5 => Priority::High,
        6..=9 => Priority::Medium,
        _ => Priority::None,
    }
}

/// Parse the bridge's JSON output.
pub fn parse(json: &str, source: &str) -> Result<Vec<Task>> {
    let items: Vec<BridgeItem> = serde_json::from_str(json.trim())?;
    Ok(items
        .into_iter()
        .map(|i| {
            let mut t =
                Task::new(i.title, leading_date(&i.due), priority(i.priority)).with_context(i.list);
            t.sources.push(source.to_string());
            t
        })
        .collect())
}

#[cfg(target_os = "macos")]
fn run_bridge(lists: &[String], timeout: std::time::Duration) -> Result<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let mut child = Command::new("/usr/bin/osascript")
        .args(["-l", "JavaScript", "-e", BRIDGE_JS])
        .args(lists)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            let mut out = String::new();
            let mut err = String::new();
            if let Some(mut s) = child.stdout.take() {
                s.read_to_string(&mut out)?;
            }
            if let Some(mut s) = child.stderr.take() {
                s.read_to_string(&mut err)?;
            }
            if !status.success() {
                if err.contains("-1743") || err.contains("Not authorized") {
                    return Err(Error::AuthMissing(
                        "allow your terminal to control Reminders in System Settings → Privacy & Security → Automation".into(),
                    ));
                }
                return Err(Error::Parse(format!("osascript: {}", err.trim())));
            }
            return Ok(out);
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            return Err(Error::Http(format!(
                "Reminders did not answer within {}s",
                timeout.as_secs()
            )));
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

#[cfg(not(target_os = "macos"))]
fn run_bridge(_lists: &[String], _timeout: std::time::Duration) -> Result<String> {
    Err(Error::Unsupported("Apple Reminders is macOS-only".into()))
}

impl Provider for Reminders {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind_id(&self) -> &'static str {
        "reminders"
    }
    fn name(&self) -> &'static str {
        "Apple Reminders"
    }
    fn kind(&self) -> Kind {
        Kind::Macos
    }

    fn check(&self) -> Result<()> {
        if cfg!(target_os = "macos") {
            Ok(())
        } else {
            Err(Error::Unsupported("Apple Reminders is macOS-only".into()))
        }
    }

    fn fetch(&self, _ctx: &Ctx) -> Result<Output> {
        let json = run_bridge(
            &self.s.lists,
            std::time::Duration::from_secs(self.s.timeout_secs.max(1)),
        )?;
        Ok(Output::tasks(parse(&json, &self.id)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn parses_bridge_fixture() {
        let tasks = parse(
            include_str!("../../tests/fixtures/reminders_bridge.json"),
            "reminders",
        )
        .unwrap();
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].title, "Take out recycling");
        assert_eq!(tasks[0].priority, Priority::Urgent);
        assert_eq!(tasks[0].context.as_deref(), Some("Home"));
        assert_eq!(tasks[1].priority, Priority::High);
        assert_eq!(tasks[2].priority, Priority::None);
        assert_eq!(tasks[2].due, NaiveDate::from_ymd_opt(2026, 10, 14));
    }

    #[test]
    fn bridge_script_is_read_only() {
        for verb in ["delete(", ".make(", "completed = ", "push(app"] {
            assert!(!BRIDGE_JS.contains(verb), "bridge must not mutate: {verb}");
        }
        assert!(BRIDGE_JS.contains("function run(argv)"));
    }

    #[test]
    fn garbage_output_is_an_error_not_a_panic() {
        assert!(parse("execution error: -1743", "r").is_err());
    }
}

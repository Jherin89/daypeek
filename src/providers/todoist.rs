//! Todoist via the unified API v1 (`/api/v1/tasks/filter`).
//!
//! Token: Todoist → Settings → Integrations → Developer → API token.

use serde::Deserialize;

use super::{leading_date, settings, Ctx, Kind, Output, Provider};
use crate::error::{Error, Result};
use crate::http::enc;
use crate::model::{Priority, Task};
use crate::secrets;

pub const TOKEN_ENV: &str = "TODOIST_API_TOKEN";

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Inline token (discouraged; prefer the env var or Keychain).
    pub token: Option<String>,
    pub token_env: String,
    /// Todoist filter query.
    pub filter: String,
    pub base_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            token: None,
            token_env: TOKEN_ENV.into(),
            filter: "today | overdue".into(),
            base_url: "https://api.todoist.com".into(),
        }
    }
}

pub struct Todoist {
    id: String,
    s: Settings,
}

pub fn build(id: &str, table: toml::Table) -> Result<Box<dyn Provider>> {
    Ok(Box::new(Todoist {
        id: id.into(),
        s: settings(id, table)?,
    }))
}

#[derive(Debug, Deserialize)]
pub struct Page {
    pub results: Vec<ApiTask>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ApiTask {
    pub content: String,
    #[serde(default = "one")]
    pub priority: u8,
    #[serde(default)]
    pub due: Option<ApiDue>,
    #[serde(default)]
    pub checked: bool,
}

fn one() -> u8 {
    1
}

#[derive(Debug, Deserialize)]
pub struct ApiDue {
    pub date: String,
}

/// Todoist: 4 = p1 (urgent) … 1 = p4 (none).
pub fn priority(p: u8) -> Priority {
    match p {
        4 => Priority::Urgent,
        3 => Priority::High,
        2 => Priority::Medium,
        _ => Priority::None,
    }
}

pub fn to_tasks(page: Page, source: &str) -> Vec<Task> {
    page.results
        .into_iter()
        .filter(|t| !t.checked)
        .map(|t| {
            let mut task = Task::new(
                t.content,
                t.due.as_ref().and_then(|d| leading_date(&d.date)),
                priority(t.priority),
            );
            task.sources.push(source.to_string());
            task
        })
        .collect()
}

impl Todoist {
    fn token(&self) -> Option<String> {
        secrets::resolve(
            &self.id,
            "token",
            &self.s.token_env,
            self.s.token.as_deref(),
        )
    }
}

impl Provider for Todoist {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind_id(&self) -> &'static str {
        "todoist"
    }
    fn name(&self) -> &'static str {
        "Todoist"
    }
    fn kind(&self) -> Kind {
        Kind::Cloud
    }

    fn check(&self) -> Result<()> {
        self.token()
            .map(|_| ())
            .ok_or_else(|| Error::AuthMissing(secrets::hint(&self.id, "token", &self.s.token_env)))
    }

    fn fetch(&self, ctx: &Ctx) -> Result<Output> {
        let token = self.token().ok_or_else(|| {
            Error::AuthMissing(secrets::hint(&self.id, "token", &self.s.token_env))
        })?;
        let mut tasks = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..10 {
            let mut url = format!(
                "{}/api/v1/tasks/filter?query={}&limit=200",
                self.s.base_url.trim_end_matches('/'),
                enc(&self.s.filter)
            );
            if let Some(c) = &cursor {
                url.push_str(&format!("&cursor={}", enc(c)));
            }
            let page: Page = ctx.http.get_json(&url, Some(&token))?;
            cursor = page.next_cursor.clone();
            tasks.extend(to_tasks(page, &self.id));
            if cursor.is_none() {
                break;
            }
        }
        Ok(Output::tasks(tasks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture() {
        let page: Page =
            serde_json::from_str(include_str!("../../tests/fixtures/todoist_tasks.json")).unwrap();
        assert!(page.next_cursor.is_none());
        let tasks = to_tasks(page, "todoist");
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].title, "Submit expense report");
        assert_eq!(tasks[0].priority, Priority::Urgent);
        assert_eq!(tasks[0].due, leading_date("2026-10-19"));
        assert_eq!(tasks[1].due, leading_date("2026-10-21"), "datetime due");
        assert_eq!(tasks[2].priority, Priority::None);
        assert_eq!(tasks[2].sources, vec!["todoist"]);
    }

    #[test]
    fn priority_mapping() {
        assert_eq!(priority(4), Priority::Urgent);
        assert_eq!(priority(3), Priority::High);
        assert_eq!(priority(2), Priority::Medium);
        assert_eq!(priority(1), Priority::None);
    }

    #[test]
    fn missing_token_is_auth_missing() {
        let p = Todoist {
            id: "todoist-test-none".into(),
            s: Settings {
                token_env: "TODAY_TEST_NO_SUCH_VAR".into(),
                ..Settings::default()
            },
        };
        assert!(matches!(p.check(), Err(Error::AuthMissing(_))));
    }
}

//! Todoist via the unified API v1 (`/api/v1/tasks/filter`).
//!
//! Token: Todoist → Settings → Integrations → Developer → API token.

use serde::Deserialize;

use super::{leading_date, settings, Ctx, Kind, Output, Provider};
use crate::error::{Error, Result};
use crate::http::{enc, Http};
use crate::model::{Priority, Task, TaskRef};
use crate::secrets;

pub const TOKEN_ENV: &str = "DAYPEEK_TODOIST_TOKEN";
/// The conventional Todoist variable, accepted as a fallback.
pub const FALLBACK_ENV: &str = "TODOIST_API_TOKEN";

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
    #[serde(default)]
    pub id: String,
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
            )
            .with_ref(source, &t.id, None);
            task.sources.push(source.to_string());
            task
        })
        .collect()
}

impl Todoist {
    fn token(&self) -> Option<String> {
        secrets::resolve(&self.id, "token", &self.s.token_env, None)
            .or_else(|| secrets::resolve(&self.id, "token", FALLBACK_ENV, self.s.token.as_deref()))
    }

    fn hint(&self) -> String {
        format!(
            "{} (or ${FALLBACK_ENV})",
            secrets::hint(&self.id, "token", &self.s.token_env)
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
            .ok_or_else(|| Error::AuthMissing(self.hint()))
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
                enc(&query_for(&self.s.filter, ctx))
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

    fn can_complete(&self) -> bool {
        true
    }

    /// `POST /api/v1/tasks/{id}/close`: done (recurring tasks move to their
    /// next date, like in the app).
    fn complete(&self, http: &Http, task: &TaskRef) -> Result<()> {
        let token = self
            .token()
            .ok_or_else(|| Error::AuthMissing(self.hint()))?;
        http.post_empty(
            &format!(
                "{}/api/v1/tasks/{}/close",
                self.s.base_url.trim_end_matches('/'),
                enc(&task.id)
            ),
            &format!("Bearer {token}"),
        )
    }
}

/// The Todoist filter for the current view. Your `filter` setting drives the
/// default (today) view; `tomorrow` and `week` use Todoist's own date
/// keywords: <https://todoist.com/help/articles/introduction-to-filters-V98wIH>
pub fn query_for(filter: &str, ctx: &Ctx) -> String {
    match ctx.days() {
        1 => filter.to_string(),
        n => format!("overdue | {n} days"),
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
        assert_eq!(tasks[0].refs[0].id, "6X7rM8997g3RQmvh");
    }

    #[test]
    fn complete_closes_the_task() {
        let server = crate::testserver::serve(|_| crate::testserver::Response::new(204, ""));
        let p = Todoist {
            id: "todoist".into(),
            s: Settings {
                token: Some("tok".into()),
                token_env: "DAYPEEK_TEST_TODOIST_UNSET".into(),
                base_url: server.url.clone(),
                ..Settings::default()
            },
        };
        assert!(p.can_complete());
        let http = Http::new(std::time::Duration::from_secs(5));
        let r = TaskRef {
            source: "todoist".into(),
            id: "6X7rM8997g3RQmvh".into(),
            project: None,
        };
        p.complete(&http, &r).unwrap();
        let req = &server.requests()[0];
        assert_eq!(req.method, "POST");
        assert_eq!(req.path, "/api/v1/tasks/6X7rM8997g3RQmvh/close");
        assert_eq!(req.header("authorization"), Some("Bearer tok"));
    }

    #[test]
    fn priority_mapping() {
        assert_eq!(priority(4), Priority::Urgent);
        assert_eq!(priority(3), Priority::High);
        assert_eq!(priority(2), Priority::Medium);
        assert_eq!(priority(1), Priority::None);
    }

    #[test]
    fn fallback_env_is_accepted() {
        std::env::set_var("DAYPEEK_TEST_TODOIST_PRIMARY", "primary");
        let p = Todoist {
            id: "todoist-test-fallback".into(),
            s: Settings {
                token_env: "DAYPEEK_TEST_TODOIST_PRIMARY".into(),
                ..Settings::default()
            },
        };
        assert_eq!(p.token().as_deref(), Some("primary"));
        std::env::remove_var("DAYPEEK_TEST_TODOIST_PRIMARY");
        assert_eq!(TOKEN_ENV, "DAYPEEK_TODOIST_TOKEN");
        assert_eq!(FALLBACK_ENV, "TODOIST_API_TOKEN");
    }

    #[test]
    fn missing_token_is_auth_missing() {
        let p = Todoist {
            id: "todoist-test-none".into(),
            s: Settings {
                token_env: "DAYPEEK_TEST_NO_SUCH_VAR".into(),
                ..Settings::default()
            },
        };
        if std::env::var_os(FALLBACK_ENV).is_none() {
            assert!(matches!(p.check(), Err(Error::AuthMissing(_))));
        }
    }
}

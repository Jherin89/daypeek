//! TickTick via the official Open API (`api.ticktick.com/open/v1`).
//!
//! Register an app at <https://developer.ticktick.com/manage>, then run
//! `daypeek auth ticktick` (or paste an access token into `$TICKTICK_ACCESS_TOKEN`).
//! TickTick access tokens are long-lived (about six months).

use chrono::{DateTime, NaiveDate};
use chrono_tz::Tz;
use serde::Deserialize;

use super::{settings, Ctx, Kind, Output, Provider};
use crate::error::{Error, Result};
use crate::http::{enc, Http};
use crate::model::{Priority, Task, TaskRef};
use crate::secrets;

pub const TOKEN_ENV: &str = "TICKTICK_ACCESS_TOKEN";

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub token: Option<String>,
    pub token_env: String,
    /// Your TickTick app's client id (only used by `daypeek auth ticktick`).
    pub client_id: Option<String>,
    /// Project names to include; empty = inbox + every open project.
    pub projects: Vec<String>,
    /// `https://api.ticktick.com` (or `https://api.dida365.com`).
    pub base_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            token: None,
            token_env: TOKEN_ENV.into(),
            client_id: None,
            projects: Vec::new(),
            base_url: "https://api.ticktick.com".into(),
        }
    }
}

pub struct TickTick {
    id: String,
    s: Settings,
}

pub fn build(id: &str, table: toml::Table) -> Result<Box<dyn Provider>> {
    Ok(Box::new(TickTick {
        id: id.into(),
        s: settings(id, table)?,
    }))
}

#[derive(Debug, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub closed: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ProjectData {
    #[serde(default)]
    pub tasks: Vec<ApiTask>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTask {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub project_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub priority: u8,
    #[serde(default)]
    pub status: u8,
    #[serde(default)]
    pub time_zone: Option<String>,
}

/// TickTick: 5 high, 3 medium, 1 low, 0 none.
pub fn priority(p: u8) -> Priority {
    match p {
        5 => Priority::Urgent,
        3 => Priority::High,
        1 => Priority::Medium,
        _ => Priority::None,
    }
}

/// `2026-10-21T04:00:00.000+0000` → local date in the task's (or user's) zone.
pub fn due_date(s: &str, task_tz: Option<&str>, user_tz: Tz) -> Option<NaiveDate> {
    let dt = DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f%z")
        .or_else(|_| DateTime::parse_from_rfc3339(s))
        .ok()?;
    let tz = task_tz
        .and_then(|t| t.parse::<Tz>().ok())
        .unwrap_or(user_tz);
    Some(dt.with_timezone(&tz).date_naive())
}

/// `project_id` is the project the tasks were fetched from (completing a
/// TickTick task needs it).
pub fn to_tasks(
    data: ProjectData,
    project: &str,
    project_id: &str,
    source: &str,
    tz: Tz,
) -> Vec<Task> {
    data.tasks
        .into_iter()
        .filter(|t| t.status == 0)
        .map(|t| {
            let due = t
                .due_date
                .as_deref()
                .and_then(|d| due_date(d, t.time_zone.as_deref(), tz));
            let pid = t.project_id.as_deref().unwrap_or(project_id);
            let mut task = Task::new(t.title, due, priority(t.priority))
                .with_context(project)
                .with_ref(source, &t.id, Some(pid));
            task.sources.push(source.to_string());
            task
        })
        .filter(|t| t.due.is_some())
        .collect()
}

impl TickTick {
    fn token(&self) -> Option<String> {
        secrets::resolve(
            &self.id,
            "token",
            &self.s.token_env,
            self.s.token.as_deref(),
        )
    }
}

impl Provider for TickTick {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind_id(&self) -> &'static str {
        "ticktick"
    }
    fn name(&self) -> &'static str {
        "TickTick"
    }
    fn kind(&self) -> Kind {
        Kind::Cloud
    }

    fn check(&self) -> Result<()> {
        self.token().map(|_| ()).ok_or_else(|| {
            Error::AuthMissing(format!(
                "{} (or run `daypeek auth ticktick`)",
                secrets::hint(&self.id, "token", &self.s.token_env)
            ))
        })
    }

    fn fetch(&self, ctx: &Ctx) -> Result<Output> {
        self.check()?;
        let token = self.token().unwrap_or_default();
        let base = format!("{}/open/v1", self.s.base_url.trim_end_matches('/'));
        let projects: Vec<Project> = ctx
            .http
            .get_json(&format!("{base}/project"), Some(&token))?;
        let mut wanted: Vec<(String, String)> = vec![("inbox".into(), "Inbox".into())];
        wanted.extend(
            projects
                .into_iter()
                .filter(|p| p.closed != Some(true))
                .map(|p| (p.id, p.name)),
        );
        if !self.s.projects.is_empty() {
            wanted.retain(|(_, name)| self.s.projects.iter().any(|w| w.eq_ignore_ascii_case(name)));
        }
        let results: Vec<Result<Vec<Task>>> = std::thread::scope(|scope| {
            let handles: Vec<_> = wanted
                .iter()
                .map(|(pid, name)| {
                    let url = format!("{base}/project/{pid}/data");
                    let token = &token;
                    scope.spawn(move || -> Result<Vec<Task>> {
                        let data: ProjectData = ctx.http.get_json(&url, Some(token))?;
                        Ok(to_tasks(data, name, pid, &self.id, ctx.tz))
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| {
                    h.join()
                        .unwrap_or_else(|_| Err(Error::Http("worker panicked".into())))
                })
                .collect()
        });
        let mut tasks = Vec::new();
        for r in results {
            tasks.extend(r?);
        }
        Ok(Output::tasks(tasks))
    }

    fn can_complete(&self) -> bool {
        true
    }

    /// `POST /open/v1/project/{projectId}/task/{taskId}/complete`. Needs a
    /// token with the `tasks:write` scope (`daypeek auth ticktick` asks for
    /// it once `allow_write = true` is set).
    fn complete(&self, http: &Http, task: &TaskRef) -> Result<()> {
        self.check()?;
        let token = self.token().unwrap_or_default();
        let project = task
            .project
            .as_deref()
            .ok_or_else(|| Error::Parse("TickTick task has no project id".into()))?;
        http.post_empty(
            &format!(
                "{}/open/v1/project/{}/task/{}/complete",
                self.s.base_url.trim_end_matches('/'),
                enc(project),
                enc(&task.id)
            ),
            &format!("Bearer {token}"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture() {
        let data: ProjectData = serde_json::from_str(include_str!(
            "../../tests/fixtures/ticktick_project_data.json"
        ))
        .unwrap();
        let tasks = to_tasks(
            data,
            "Inbox",
            "inbox",
            "ticktick",
            chrono_tz::America::New_York,
        );
        // Completed and undated tasks are dropped.
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].title, "Renew passport");
        assert_eq!(tasks[0].priority, Priority::Urgent);
        // 04:00Z is midnight in New York → the 21st, not the 20th.
        assert_eq!(tasks[0].due, NaiveDate::from_ymd_opt(2026, 10, 21));
        assert_eq!(tasks[0].context.as_deref(), Some("Inbox"));
        assert_eq!(tasks[1].priority, Priority::Medium);
        assert_eq!(tasks[1].due, NaiveDate::from_ymd_opt(2026, 10, 18));
        // The task's own projectId wins over the "inbox" alias it was fetched by.
        assert_eq!(tasks[0].refs[0].project.as_deref(), Some("inbox114478622"));
    }

    #[test]
    fn complete_posts_to_the_project_task() {
        let server = crate::testserver::serve(|_| crate::testserver::Response::new(200, ""));
        let p = TickTick {
            id: "ticktick".into(),
            s: Settings {
                token: Some("tt".into()),
                token_env: "DAYPEEK_TEST_TICKTICK_UNSET".into(),
                base_url: server.url.clone(),
                ..Settings::default()
            },
        };
        let http = Http::new(std::time::Duration::from_secs(5));
        let r = TaskRef {
            source: "ticktick".into(),
            id: "63b7".into(),
            project: Some("inbox114478622".into()),
        };
        p.complete(&http, &r).unwrap();
        let req = &server.requests()[0];
        assert_eq!(
            (req.method.as_str(), req.path.as_str()),
            ("POST", "/open/v1/project/inbox114478622/task/63b7/complete")
        );
        assert_eq!(req.header("authorization"), Some("Bearer tt"));
        let no_project = TaskRef { project: None, ..r };
        assert!(p.complete(&http, &no_project).is_err());
    }

    #[test]
    fn priority_mapping() {
        assert_eq!(priority(5), Priority::Urgent);
        assert_eq!(priority(3), Priority::High);
        assert_eq!(priority(1), Priority::Medium);
        assert_eq!(priority(0), Priority::None);
    }
}

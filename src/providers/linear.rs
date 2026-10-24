//! Linear: issues assigned to you that aren't done or canceled.
//!
//! Create a personal API key in Linear → Settings → Security & access →
//! Personal API keys, and put it in `$LINEAR_API_KEY` (or the Keychain item
//! `daypeek-linear`). Read-only: daypeek only runs one GraphQL query.
//! API docs: <https://linear.app/developers/graphql>

use serde::Deserialize;
use serde_json::json;

use super::{leading_date, settings, Ctx, Kind, Output, Provider};
use crate::error::{Error, Result};
use crate::model::{Priority, Task};
use crate::secrets;

pub const TOKEN_ENV: &str = "LINEAR_API_KEY";

/// Assigned issues that aren't finished, 100 per page.
pub const QUERY: &str = r#"query DaypeekAssigned($after: String) {
  viewer {
    assignedIssues(first: 100, after: $after, filter: { state: { type: { nin: ["completed", "canceled"] } } }) {
      nodes { id identifier title priority dueDate state { type } team { key } }
      pageInfo { hasNextPage endCursor }
    }
  }
}"#;

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Inline key (discouraged; prefer the env var or Keychain).
    pub token: Option<String>,
    pub token_env: String,
    /// Team keys to include, e.g. `["ENG"]`; empty = every team.
    pub teams: Vec<String>,
    /// Show issues that are "In Progress" (started) even without a due date.
    pub include_in_progress: bool,
    /// Show every open assigned issue without a due date (can be a lot).
    pub include_undated: bool,
    pub base_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            token: None,
            token_env: TOKEN_ENV.into(),
            teams: Vec::new(),
            include_in_progress: true,
            include_undated: false,
            base_url: "https://api.linear.app".into(),
        }
    }
}

pub struct Linear {
    id: String,
    s: Settings,
}

pub fn build(id: &str, table: toml::Table) -> Result<Box<dyn Provider>> {
    Ok(Box::new(Linear {
        id: id.into(),
        s: settings(id, table)?,
    }))
}

#[derive(Debug, Deserialize)]
pub struct Response {
    #[serde(default)]
    pub data: Option<Data>,
    #[serde(default)]
    pub errors: Vec<GqlError>,
}

#[derive(Debug, Deserialize)]
pub struct GqlError {
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct Data {
    pub viewer: Viewer,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewer {
    pub assigned_issues: Connection,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub nodes: Vec<Issue>,
    pub page_info: PageInfo,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    pub has_next_page: bool,
    #[serde(default)]
    pub end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub id: String,
    pub identifier: String,
    pub title: String,
    #[serde(default)]
    pub priority: f64,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub state: Option<State>,
    #[serde(default)]
    pub team: Option<Team>,
}

#[derive(Debug, Deserialize)]
pub struct State {
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Deserialize)]
pub struct Team {
    pub key: String,
}

/// Linear: 1 urgent, 2 high, 3 medium, 4 low, 0 none.
pub fn priority(p: f64) -> Priority {
    match p as i64 {
        1 => Priority::Urgent,
        2 => Priority::High,
        3 => Priority::Medium,
        4 => Priority::Low,
        _ => Priority::None,
    }
}

/// Turn issues into tasks, applying the team and undated settings.
pub fn to_tasks(issues: Vec<Issue>, s: &Settings, source: &str) -> Vec<Task> {
    issues
        .into_iter()
        .filter(|i| {
            s.teams.is_empty()
                || i.team
                    .as_ref()
                    .is_some_and(|t| s.teams.iter().any(|k| k.eq_ignore_ascii_case(&t.key)))
        })
        .filter_map(|i| {
            let due = i.due_date.as_deref().and_then(leading_date);
            let started = i.state.as_ref().is_some_and(|st| st.kind == "started");
            if due.is_none() && !s.include_undated && !(s.include_in_progress && started) {
                return None;
            }
            let mut task = Task::new(
                format!("{} {}", i.identifier, i.title),
                due,
                priority(i.priority),
            )
            .with_context(i.team.map(|t| t.key).unwrap_or_default())
            .with_ref(source, &i.id, None);
            task.sources.push(source.to_string());
            Some(task)
        })
        .collect()
}

impl Linear {
    fn token(&self) -> Option<String> {
        secrets::resolve(
            &self.id,
            "token",
            &self.s.token_env,
            self.s.token.as_deref(),
        )
    }
}

impl Provider for Linear {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind_id(&self) -> &'static str {
        "linear"
    }
    fn name(&self) -> &'static str {
        "Linear"
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
        self.check()?;
        let key = self.token().unwrap_or_default();
        let url = format!("{}/graphql", self.s.base_url.trim_end_matches('/'));
        let mut issues = Vec::new();
        let mut after: Option<String> = None;
        for _ in 0..5 {
            let body = json!({ "query": QUERY, "variables": { "after": after } });
            // Personal API keys go in the header as-is (no "Bearer").
            let resp: Response = ctx.http.post_json_auth(&url, &key, &body)?;
            if let Some(e) = resp.errors.first() {
                return Err(Error::Http(format!("Linear: {}", e.message)));
            }
            let conn = resp
                .data
                .ok_or_else(|| Error::Parse("Linear: empty response".into()))?
                .viewer
                .assigned_issues;
            issues.extend(conn.nodes);
            if !conn.page_info.has_next_page {
                break;
            }
            after = conn.page_info.end_cursor;
        }
        Ok(Output::tasks(to_tasks(issues, &self.s, &self.id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Http;
    use crate::testserver::{serve, Response as R};
    use chrono::NaiveDate;

    const FIXTURE: &str = include_str!("../../tests/fixtures/linear_issues.json");

    fn issues() -> Vec<Issue> {
        let r: Response = serde_json::from_str(FIXTURE).unwrap();
        r.data.unwrap().viewer.assigned_issues.nodes
    }

    #[test]
    fn parses_fixture_and_skips_undated_backlog() {
        let tasks = to_tasks(issues(), &Settings::default(), "linear");
        let titles: Vec<_> = tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            titles,
            vec![
                "ENG-142 Fix login redirect loop",
                "ENG-150 Ship onboarding checklist",
                "OPS-7 Rotate staging certificates",
            ]
        );
        assert_eq!(tasks[0].priority, Priority::Urgent);
        assert_eq!(tasks[0].due, NaiveDate::from_ymd_opt(2026, 10, 21));
        assert_eq!(tasks[0].context.as_deref(), Some("ENG"));
        assert_eq!(tasks[0].refs[0].id, "a1b2c3d4-0000-4000-8000-000000000142");
        // In progress, no due date: shown (undated) by default.
        assert_eq!(tasks[1].due, None);
        assert_eq!(tasks[2].priority, Priority::Low);
    }

    #[test]
    fn team_and_undated_settings() {
        let s = Settings {
            teams: vec!["ops".into()],
            ..Settings::default()
        };
        assert_eq!(to_tasks(issues(), &s, "linear").len(), 1);
        let s = Settings {
            include_in_progress: false,
            ..Settings::default()
        };
        assert_eq!(to_tasks(issues(), &s, "linear").len(), 2);
        let s = Settings {
            include_undated: true,
            ..Settings::default()
        };
        assert_eq!(to_tasks(issues(), &s, "linear").len(), 4);
    }

    #[test]
    fn fetch_sends_the_key_without_bearer_and_reports_graphql_errors() {
        let server = serve(|req| {
            if req.body.contains("DaypeekAssigned") {
                R::new(200, FIXTURE)
            } else {
                R::new(400, "{}")
            }
        });
        let p = Linear {
            id: "linear".into(),
            s: Settings {
                token: Some("lin_api_test".into()),
                token_env: "DAYPEEK_TEST_LINEAR_UNSET".into(),
                base_url: server.url.clone(),
                ..Settings::default()
            },
        };
        let http = Http::new(std::time::Duration::from_secs(5));
        let today = NaiveDate::from_ymd_opt(2026, 10, 21).unwrap();
        let ctx = Ctx {
            http: &http,
            today,
            until: today,
            tz: chrono_tz::America::New_York,
        };
        let out = p.fetch(&ctx).unwrap();
        assert_eq!(out.tasks.len(), 3);
        let req = &server.requests()[0];
        assert_eq!(req.method, "POST");
        assert_eq!(req.path, "/graphql");
        assert_eq!(req.header("authorization"), Some("lin_api_test"));

        let bad = serve(|_| R::new(200, r#"{"errors":[{"message":"Authentication required"}]}"#));
        let p = Linear {
            id: "linear".into(),
            s: Settings {
                base_url: bad.url.clone(),
                ..p.s
            },
        };
        let err = p.fetch(&ctx).unwrap_err().to_string();
        assert!(err.contains("Authentication required"), "{err}");
    }
}

//! Asana: your incomplete tasks ("My tasks") in every workspace, or one.
//!
//! Create a personal access token in Asana → Settings → Apps → Developer apps
//! (or <https://app.asana.com/0/my-apps>) and put it in `$ASANA_TOKEN` (or the
//! Keychain item `daypeek-asana`). Read-only.
//! API docs: <https://developers.asana.com/reference/gettasks>

use chrono::DateTime;
use serde::Deserialize;

use super::{leading_date, settings, Ctx, Kind, Output, Provider};
use crate::error::{Error, Result};
use crate::http::enc;
use crate::model::{Priority, Task};
use crate::secrets;

pub const TOKEN_ENV: &str = "ASANA_TOKEN";

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Inline token (discouraged; prefer the env var or Keychain).
    pub token: Option<String>,
    pub token_env: String,
    /// A workspace name or gid; empty = every workspace you belong to.
    pub workspace: Option<String>,
    /// Also show incomplete tasks with no due date (can be a lot).
    pub include_undated: bool,
    pub base_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            token: None,
            token_env: TOKEN_ENV.into(),
            workspace: None,
            include_undated: false,
            base_url: "https://app.asana.com/api/1.0".into(),
        }
    }
}

pub struct Asana {
    id: String,
    s: Settings,
}

pub fn build(id: &str, table: toml::Table) -> Result<Box<dyn Provider>> {
    Ok(Box::new(Asana {
        id: id.into(),
        s: settings(id, table)?,
    }))
}

#[derive(Debug, Deserialize)]
pub struct Me {
    pub data: MeData,
}

#[derive(Debug, Deserialize)]
pub struct MeData {
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
}

#[derive(Debug, Deserialize)]
pub struct Workspace {
    pub gid: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct Page {
    pub data: Vec<ApiTask>,
    #[serde(default)]
    pub next_page: Option<NextPage>,
}

#[derive(Debug, Deserialize)]
pub struct NextPage {
    pub offset: String,
}

#[derive(Debug, Deserialize)]
pub struct ApiTask {
    pub gid: String,
    pub name: String,
    #[serde(default)]
    pub completed: bool,
    #[serde(default)]
    pub due_on: Option<String>,
    #[serde(default)]
    pub due_at: Option<String>,
    #[serde(default)]
    pub projects: Vec<Named>,
}

#[derive(Debug, Deserialize)]
pub struct Named {
    #[serde(default)]
    pub name: String,
}

/// Asana tasks have no built-in priority, so everything is "none".
pub fn to_tasks(page: Page, include_undated: bool, source: &str, tz: chrono_tz::Tz) -> Vec<Task> {
    page.data
        .into_iter()
        .filter(|t| !t.completed && !t.name.trim().is_empty())
        .filter_map(|t| {
            // `due_at` (a UTC instant) is more precise than `due_on` near midnight.
            let due = t
                .due_at
                .as_deref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&tz).date_naive())
                .or_else(|| t.due_on.as_deref().and_then(leading_date));
            if due.is_none() && !include_undated {
                return None;
            }
            let project = t.projects.first().map(|p| p.name.clone());
            let mut task = Task::new(t.name, due, Priority::None)
                .with_context(project.unwrap_or_default())
                .with_ref(source, &t.gid, None);
            task.sources.push(source.to_string());
            Some(task)
        })
        .collect()
}

impl Asana {
    fn token(&self) -> Option<String> {
        secrets::resolve(
            &self.id,
            "token",
            &self.s.token_env,
            self.s.token.as_deref(),
        )
    }

    fn base(&self) -> &str {
        self.s.base_url.trim_end_matches('/')
    }

    /// The workspaces to read: the configured one (by gid or name) or all.
    fn workspaces(&self, ctx: &Ctx, token: &str) -> Result<Vec<String>> {
        if let Some(w) = self.s.workspace.as_deref().map(str::trim) {
            if !w.is_empty() && w.chars().all(|c| c.is_ascii_digit()) {
                return Ok(vec![w.to_string()]);
            }
        }
        let me: Me = ctx.http.get_json(
            &format!("{}/users/me?opt_fields=workspaces.name", self.base()),
            Some(token),
        )?;
        let all = me.data.workspaces;
        match self.s.workspace.as_deref().map(str::trim) {
            Some(name) if !name.is_empty() => {
                let names: Vec<String> = all.iter().map(|w| w.name.clone()).collect();
                let found: Vec<String> = all
                    .into_iter()
                    .filter(|w| w.name.eq_ignore_ascii_case(name))
                    .map(|w| w.gid)
                    .collect();
                if found.is_empty() {
                    return Err(Error::Config(format!(
                        "[providers.{}]: no Asana workspace named {name:?} (yours: {})",
                        self.id,
                        names.join(", ")
                    )));
                }
                Ok(found)
            }
            _ => Ok(all.into_iter().map(|w| w.gid).collect()),
        }
    }
}

impl Provider for Asana {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind_id(&self) -> &'static str {
        "asana"
    }
    fn name(&self) -> &'static str {
        "Asana"
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
        let token = self.token().unwrap_or_default();
        let mut tasks = Vec::new();
        for ws in self.workspaces(ctx, &token)? {
            let mut offset: Option<String> = None;
            for _ in 0..10 {
                let mut url = format!(
                    "{}/tasks?assignee=me&workspace={}&completed_since=now&limit=100&opt_fields={}",
                    self.base(),
                    enc(&ws),
                    enc("name,completed,due_on,due_at,projects.name")
                );
                if let Some(o) = &offset {
                    url.push_str(&format!("&offset={}", enc(o)));
                }
                let page: Page = ctx.http.get_json(&url, Some(&token))?;
                offset = page.next_page.as_ref().map(|n| n.offset.clone());
                tasks.extend(to_tasks(page, self.s.include_undated, &self.id, ctx.tz));
                if offset.is_none() {
                    break;
                }
            }
        }
        Ok(Output::tasks(tasks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Http;
    use crate::testserver::{serve, Response as R};
    use chrono::NaiveDate;

    const FIXTURE: &str = include_str!("../../tests/fixtures/asana_tasks.json");

    #[test]
    fn parses_fixture() {
        let page: Page = serde_json::from_str(FIXTURE).unwrap();
        let tasks = to_tasks(page, false, "asana", chrono_tz::America::New_York);
        let titles: Vec<_> = tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, vec!["Draft Q4 roadmap", "Book venue for offsite"]);
        assert_eq!(tasks[0].due, NaiveDate::from_ymd_opt(2026, 10, 20));
        assert_eq!(tasks[0].context.as_deref(), Some("Planning"));
        // 03:30Z on the 22nd is still the 21st in New York.
        assert_eq!(tasks[1].due, NaiveDate::from_ymd_opt(2026, 10, 21));
        assert_eq!(tasks[1].refs[0].id, "1208000000000002");
        let page: Page = serde_json::from_str(FIXTURE).unwrap();
        assert_eq!(to_tasks(page, true, "asana", chrono_tz::UTC).len(), 3);
    }

    #[test]
    fn fetch_walks_workspaces_and_pages() {
        let server = serve(|req| {
            if req.path.starts_with("/users/me") {
                R::new(
                    200,
                    r#"{"data":{"gid":"1","workspaces":[{"gid":"111","name":"Acme"},{"gid":"222","name":"Home"}]}}"#,
                )
            } else if req.path.contains("workspace=111") && !req.path.contains("offset=") {
                R::new(
                    200,
                    r#"{"data":[{"gid":"9","name":"Page one","completed":false,"due_on":"2026-10-21"}],"next_page":{"offset":"abc","path":"","uri":""}}"#,
                )
            } else if req.path.contains("offset=abc") {
                R::new(
                    200,
                    r#"{"data":[{"gid":"10","name":"Page two","completed":false,"due_on":"2026-10-21"}],"next_page":null}"#,
                )
            } else {
                R::new(200, r#"{"data":[],"next_page":null}"#)
            }
        });
        let http = Http::new(std::time::Duration::from_secs(5));
        let today = NaiveDate::from_ymd_opt(2026, 10, 21).unwrap();
        let ctx = Ctx {
            http: &http,
            today,
            until: today,
            tz: chrono_tz::UTC,
        };
        let mk = |workspace: Option<&str>| Asana {
            id: "asana".into(),
            s: Settings {
                token: Some("2/test".into()),
                token_env: "DAYPEEK_TEST_ASANA_UNSET".into(),
                workspace: workspace.map(str::to_string),
                base_url: server.url.clone(),
                ..Settings::default()
            },
        };
        let out = mk(None).fetch(&ctx).unwrap();
        let titles: Vec<_> = out.tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, vec!["Page one", "Page two"]);
        let reqs = server.requests();
        assert_eq!(reqs[0].header("authorization"), Some("Bearer 2/test"));
        assert!(
            reqs[1].path.contains("assignee=me") && reqs[1].path.contains("completed_since=now")
        );
        // A named workspace only reads that one; a missing name is a clear error.
        assert_eq!(mk(Some("home")).fetch(&ctx).unwrap().tasks.len(), 0);
        let err = mk(Some("Nope")).fetch(&ctx).unwrap_err().to_string();
        assert!(err.contains("no Asana workspace named"), "{err}");
    }
}

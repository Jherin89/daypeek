//! Google Tasks via the Tasks REST API v1.
//!
//! Bring your own Google Cloud OAuth client ("Desktop app" type). Google
//! requires the client secret even for desktop apps; it is *your* secret, so
//! keep it in `$GTASKS_CLIENT_SECRET` or the Keychain, never in the repo. Run
//! `daypeek auth gtasks` once to get a refresh token for `$GTASKS_REFRESH_TOKEN`.

use serde::Deserialize;

use super::{leading_date, settings, Ctx, Kind, Output, Provider};
use crate::error::{Error, Result};
use crate::http::enc;
use crate::model::{Priority, Task};
use crate::{oauth, secrets};

pub const ACCESS_ENV: &str = "GTASKS_ACCESS_TOKEN";
pub const REFRESH_ENV: &str = "GTASKS_REFRESH_TOKEN";
pub const SECRET_ENV: &str = "GTASKS_CLIENT_SECRET";
pub const SCOPE: &str = "https://www.googleapis.com/auth/tasks.readonly";
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub client_id: Option<String>,
    /// List titles to include; empty = all lists.
    pub lists: Vec<String>,
    pub access_token_env: String,
    pub refresh_token_env: String,
    pub client_secret_env: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            client_id: None,
            lists: Vec::new(),
            access_token_env: ACCESS_ENV.into(),
            refresh_token_env: REFRESH_ENV.into(),
            client_secret_env: SECRET_ENV.into(),
        }
    }
}

pub struct GTasks {
    id: String,
    pub s: Settings,
}

pub fn build(id: &str, table: toml::Table) -> Result<Box<dyn Provider>> {
    Ok(Box::new(GTasks {
        id: id.into(),
        s: settings(id, table)?,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    #[serde(default = "Vec::new")]
    pub items: Vec<T>,
    #[serde(default)]
    pub next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TaskList {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Deserialize)]
pub struct ApiTask {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub status: String,
    /// RFC 3339; Google only stores the date part.
    #[serde(default)]
    pub due: Option<String>,
    #[serde(default)]
    pub deleted: bool,
}

/// Google Tasks has no priority; everything maps to `None`.
pub fn to_tasks(tasks: Vec<ApiTask>, list: &str, source: &str) -> Vec<Task> {
    tasks
        .into_iter()
        .filter(|t| t.status != "completed" && !t.deleted && !t.title.trim().is_empty())
        .filter_map(|t| {
            let due = t.due.as_deref().and_then(leading_date)?;
            let mut task = Task::new(t.title, Some(due), Priority::None).with_context(list);
            task.sources.push(source.to_string());
            Some(task)
        })
        .collect()
}

impl GTasks {
    fn missing(&self) -> Error {
        Error::AuthMissing(format!(
            "set ${} or client_id + ${} + ${} (run `daypeek auth {}`)",
            self.s.access_token_env, self.s.client_secret_env, self.s.refresh_token_env, self.id
        ))
    }

    fn access_token(&self, ctx: &Ctx) -> Result<String> {
        if let Some(t) = secrets::resolve(&self.id, "access_token", &self.s.access_token_env, None)
        {
            return Ok(t);
        }
        let secret = secrets::resolve(&self.id, "client_secret", &self.s.client_secret_env, None);
        let refresh = secrets::resolve(&self.id, "refresh_token", &self.s.refresh_token_env, None);
        match (&self.s.client_id, secret, refresh) {
            (Some(cid), Some(cs), Some(rt)) => {
                oauth::refresh(ctx.http, TOKEN_URL, cid, Some(&cs), &rt, None)
            }
            _ => Err(self.missing()),
        }
    }
}

impl Provider for GTasks {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind_id(&self) -> &'static str {
        "gtasks"
    }
    fn name(&self) -> &'static str {
        "Google Tasks"
    }
    fn kind(&self) -> Kind {
        Kind::Cloud
    }

    fn check(&self) -> Result<()> {
        let access = secrets::resolve(&self.id, "access_token", &self.s.access_token_env, None);
        let refresh = self.s.client_id.is_some()
            && secrets::resolve(&self.id, "client_secret", &self.s.client_secret_env, None)
                .is_some()
            && secrets::resolve(&self.id, "refresh_token", &self.s.refresh_token_env, None)
                .is_some();
        if access.is_some() || refresh {
            Ok(())
        } else {
            Err(self.missing())
        }
    }

    fn fetch(&self, ctx: &Ctx) -> Result<Output> {
        let token = self.access_token(ctx)?;
        let base = "https://tasks.googleapis.com/tasks/v1";
        let lists: Page<TaskList> = ctx.http.get_json(
            &format!("{base}/users/@me/lists?maxResults=100"),
            Some(&token),
        )?;
        let due_max = format!("{}T23:59:59.000Z", ctx.until.format("%Y-%m-%d"));
        let mut tasks = Vec::new();
        for list in lists.items {
            if !self.s.lists.is_empty()
                && !self
                    .s
                    .lists
                    .iter()
                    .any(|w| w.eq_ignore_ascii_case(&list.title))
            {
                continue;
            }
            let mut page_token: Option<String> = None;
            for _ in 0..10 {
                let mut url = format!(
                    "{base}/lists/{}/tasks?showCompleted=false&showHidden=false&maxResults=100&dueMax={}",
                    enc(&list.id),
                    enc(&due_max)
                );
                if let Some(p) = &page_token {
                    url.push_str(&format!("&pageToken={}", enc(p)));
                }
                let page: Page<ApiTask> = ctx.http.get_json(&url, Some(&token))?;
                tasks.extend(to_tasks(page.items, &list.title, &self.id));
                page_token = page.next_page_token;
                if page_token.is_none() {
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
    use chrono::NaiveDate;

    #[test]
    fn parses_fixture() {
        let page: Page<ApiTask> =
            serde_json::from_str(include_str!("../../tests/fixtures/gtasks_tasks.json")).unwrap();
        let tasks = to_tasks(page.items, "My Tasks", "gtasks");
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].title, "Pick up dry cleaning");
        assert_eq!(tasks[0].due, NaiveDate::from_ymd_opt(2026, 10, 21));
        assert_eq!(tasks[0].context.as_deref(), Some("My Tasks"));
        assert_eq!(tasks[1].due, NaiveDate::from_ymd_opt(2026, 10, 17));
    }
}

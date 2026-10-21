//! Microsoft To Do via Microsoft Graph (`/me/todo/lists/{id}/tasks`).
//!
//! Bring your own Azure app registration (public client, no secret):
//! `client_id` in config, then `today auth mstodo` prints a refresh token to
//! store in `$MSTODO_REFRESH_TOKEN` or the Keychain. A short-lived access token
//! in `$MSTODO_ACCESS_TOKEN` also works (handy with `az account get-access-token`).

use chrono::{NaiveDate, NaiveDateTime, Timelike};
use chrono_tz::Tz;
use serde::Deserialize;

use super::{settings, Ctx, Kind, Output, Provider};
use crate::error::{Error, Result};
use crate::model::{Priority, Task};
use crate::{oauth, secrets};

pub const ACCESS_ENV: &str = "MSTODO_ACCESS_TOKEN";
pub const REFRESH_ENV: &str = "MSTODO_REFRESH_TOKEN";
pub const SCOPE: &str = "Tasks.Read offline_access";

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub client_id: Option<String>,
    /// `common`, `consumers` (personal accounts), `organizations`, or a tenant id.
    pub tenant: String,
    /// List names to include; empty = all lists.
    pub lists: Vec<String>,
    pub access_token_env: String,
    pub refresh_token_env: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            client_id: None,
            tenant: "common".into(),
            lists: Vec::new(),
            access_token_env: ACCESS_ENV.into(),
            refresh_token_env: REFRESH_ENV.into(),
        }
    }
}

pub struct MsTodo {
    id: String,
    pub s: Settings,
}

pub fn build(id: &str, table: toml::Table) -> Result<Box<dyn Provider>> {
    Ok(Box::new(MsTodo {
        id: id.into(),
        s: settings(id, table)?,
    }))
}

pub fn token_url(tenant: &str) -> String {
    format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token")
}

pub fn authorize_url(tenant: &str) -> String {
    format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/authorize")
}

#[derive(Debug, Deserialize)]
pub struct Collection<T> {
    pub value: Vec<T>,
    #[serde(rename = "@odata.nextLink", default)]
    pub next_link: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoList {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTask {
    pub title: String,
    #[serde(default)]
    pub importance: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub due_date_time: Option<DateTimeTz>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DateTimeTz {
    pub date_time: String,
    pub time_zone: String,
}

pub fn priority(importance: &str) -> Priority {
    match importance {
        "high" => Priority::Urgent,
        "low" => Priority::Low,
        _ => Priority::None,
    }
}

/// To Do due dates are dates; Graph encodes them as midnight in some zone.
/// Midnight → take the date as-is; otherwise convert to the user's zone.
pub fn due_date(d: &DateTimeTz, user_tz: Tz) -> Option<NaiveDate> {
    let trimmed = d.date_time.split('.').next().unwrap_or(&d.date_time);
    let naive = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S").ok()?;
    if naive.hour() == 0 && naive.minute() == 0 {
        return Some(naive.date());
    }
    let zone: Tz = d.time_zone.parse().unwrap_or(chrono_tz::UTC);
    let utc = crate::calendar::ics::to_utc(zone, naive);
    Some(utc.with_timezone(&user_tz).date_naive())
}

pub fn to_tasks(tasks: Vec<ApiTask>, list: &str, source: &str, tz: Tz) -> Vec<Task> {
    tasks
        .into_iter()
        .filter(|t| t.status != "completed")
        .filter_map(|t| {
            let due = t.due_date_time.as_ref().and_then(|d| due_date(d, tz))?;
            let mut task =
                Task::new(t.title, Some(due), priority(&t.importance)).with_context(list);
            task.sources.push(source.to_string());
            Some(task)
        })
        .collect()
}

impl MsTodo {
    fn access_token(&self, ctx: &Ctx) -> Result<String> {
        if let Some(t) = secrets::resolve(&self.id, "access_token", &self.s.access_token_env, None)
        {
            return Ok(t);
        }
        let refresh = secrets::resolve(&self.id, "refresh_token", &self.s.refresh_token_env, None);
        match (&self.s.client_id, refresh) {
            (Some(cid), Some(rt)) => oauth::refresh(
                ctx.http,
                &token_url(&self.s.tenant),
                cid,
                None,
                &rt,
                Some(SCOPE),
            ),
            _ => Err(self.missing()),
        }
    }

    fn missing(&self) -> Error {
        Error::AuthMissing(format!(
            "set ${} or client_id + ${} (run `today auth {}`)",
            self.s.access_token_env, self.s.refresh_token_env, self.id
        ))
    }
}

impl Provider for MsTodo {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind_id(&self) -> &'static str {
        "mstodo"
    }
    fn name(&self) -> &'static str {
        "Microsoft To Do"
    }
    fn kind(&self) -> Kind {
        Kind::Cloud
    }

    fn check(&self) -> Result<()> {
        let has_access =
            secrets::resolve(&self.id, "access_token", &self.s.access_token_env, None).is_some();
        let has_refresh = self.s.client_id.is_some()
            && secrets::resolve(&self.id, "refresh_token", &self.s.refresh_token_env, None)
                .is_some();
        if has_access || has_refresh {
            Ok(())
        } else {
            Err(self.missing())
        }
    }

    fn fetch(&self, ctx: &Ctx) -> Result<Output> {
        let token = self.access_token(ctx)?;
        let base = "https://graph.microsoft.com/v1.0/me/todo/lists";
        let lists: Collection<TodoList> = ctx.http.get_json(base, Some(&token))?;
        let mut tasks = Vec::new();
        for list in lists.value {
            if !self.s.lists.is_empty()
                && !self
                    .s
                    .lists
                    .iter()
                    .any(|w| w.eq_ignore_ascii_case(&list.display_name))
            {
                continue;
            }
            let mut url = Some(format!(
                "{base}/{}/tasks?$filter={}&$top=100",
                list.id,
                crate::http::enc("status ne 'completed'")
            ));
            let mut pages = 0;
            while let Some(u) = url.take() {
                let page: Collection<ApiTask> = ctx.http.get_json(&u, Some(&token))?;
                tasks.extend(to_tasks(page.value, &list.display_name, &self.id, ctx.tz));
                pages += 1;
                if pages < 10 {
                    url = page.next_link;
                }
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
        let page: Collection<ApiTask> =
            serde_json::from_str(include_str!("../../tests/fixtures/mstodo_tasks.json")).unwrap();
        let tasks = to_tasks(page.value, "Tasks", "mstodo", chrono_tz::America::New_York);
        assert_eq!(tasks.len(), 2, "completed + undated dropped");
        assert_eq!(tasks[0].title, "Send invoice to Acme");
        assert_eq!(tasks[0].priority, Priority::Urgent);
        assert_eq!(tasks[0].due, NaiveDate::from_ymd_opt(2026, 10, 21));
        // 2026-10-20T04:00Z is midnight in New York on the 20th.
        assert_eq!(tasks[1].due, NaiveDate::from_ymd_opt(2026, 10, 20));
        assert_eq!(tasks[1].priority, Priority::None);
    }

    #[test]
    fn importance_mapping() {
        assert_eq!(priority("high"), Priority::Urgent);
        assert_eq!(priority("normal"), Priority::None);
        assert_eq!(priority("low"), Priority::Low);
    }
}

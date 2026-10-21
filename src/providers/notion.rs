//! Notion: a database with a title, a date, and a "done" property.
//!
//! Create an internal connection in Notion's developer portal
//! (<https://developers.notion.com/guides/get-started/internal-connections>),
//! give it access to the database, and put its API token in `$NOTION_TOKEN`.
//! Property names are configurable because every task database is different.

use serde::Deserialize;
use serde_json::{json, Value};

use super::{leading_date, settings, Ctx, Kind, Output, Provider};
use crate::error::{Error, Result};
use crate::http::decode;
use crate::model::{Priority, Task};
use crate::secrets;

pub const TOKEN_ENV: &str = "NOTION_TOKEN";
pub const NOTION_VERSION: &str = "2022-06-28";

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub token: Option<String>,
    pub token_env: String,
    pub database_id: String,
    pub title_property: String,
    pub date_property: String,
    /// A checkbox property, or a status/select property when `done_values` is set.
    pub done_property: String,
    /// For status/select "done" properties: values that mean done.
    pub done_values: Vec<String>,
    /// Optional select/status property mapped onto priorities.
    pub priority_property: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            token: None,
            token_env: TOKEN_ENV.into(),
            database_id: String::new(),
            title_property: "Name".into(),
            date_property: "Due".into(),
            done_property: "Done".into(),
            done_values: Vec::new(),
            priority_property: None,
        }
    }
}

pub struct Notion {
    id: String,
    s: Settings,
}

pub fn build(id: &str, table: toml::Table) -> Result<Box<dyn Provider>> {
    let s: Settings = settings(id, table)?;
    if s.database_id.trim().is_empty() {
        return Err(Error::Config(format!("[providers.{id}] needs database_id")));
    }
    Ok(Box::new(Notion { id: id.into(), s }))
}

/// Map common priority labels (case-insensitive) onto our scale.
pub fn priority(label: &str) -> Priority {
    match label.trim().to_lowercase().as_str() {
        "urgent" | "critical" | "highest" | "p1" | "p0" | "!!!" => Priority::Urgent,
        "high" | "p2" | "!!" => Priority::High,
        "medium" | "med" | "normal" | "p3" | "!" => Priority::Medium,
        "low" | "lowest" | "p4" => Priority::Low,
        _ => Priority::None,
    }
}

/// The query filter: date on or before today and not done.
pub fn filter(s: &Settings, today: &str) -> Value {
    let date = json!({ "property": s.date_property, "date": { "on_or_before": today } });
    if s.done_values.is_empty() {
        json!({ "and": [date, { "property": s.done_property, "checkbox": { "equals": false } }] })
    } else {
        // Status vs. select filters differ by property type, so done rows are dropped client-side.
        json!({ "and": [date] })
    }
}

fn plain_text(v: &Value) -> String {
    v.as_array()
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p["plain_text"].as_str())
                .collect::<String>()
        })
        .unwrap_or_default()
}

fn select_name(prop: &Value) -> Option<&str> {
    prop["select"]["name"]
        .as_str()
        .or_else(|| prop["status"]["name"].as_str())
}

/// Turn a query response page into tasks.
pub fn to_tasks(page: &Value, s: &Settings, source: &str) -> Vec<Task> {
    let Some(results) = page["results"].as_array() else {
        return Vec::new();
    };
    results
        .iter()
        .filter_map(|r| {
            let props = &r["properties"];
            let title_prop = &props[&s.title_property];
            let title = plain_text(&title_prop["title"]);
            if title.trim().is_empty() {
                return None;
            }
            let done = &props[&s.done_property];
            let is_done = done["checkbox"].as_bool().unwrap_or(false)
                || select_name(done)
                    .is_some_and(|v| s.done_values.iter().any(|d| d.eq_ignore_ascii_case(v)));
            if is_done {
                return None;
            }
            let due = props[&s.date_property]["date"]["start"]
                .as_str()
                .and_then(leading_date);
            let prio = s
                .priority_property
                .as_ref()
                .and_then(|p| select_name(&props[p]))
                .map(priority)
                .unwrap_or(Priority::None);
            let mut t = Task::new(title.trim(), due, prio);
            t.sources.push(source.to_string());
            Some(t)
        })
        .collect()
}

impl Notion {
    fn token(&self) -> Option<String> {
        secrets::resolve(
            &self.id,
            "token",
            &self.s.token_env,
            self.s.token.as_deref(),
        )
    }
}

impl Provider for Notion {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind_id(&self) -> &'static str {
        "notion"
    }
    fn name(&self) -> &'static str {
        "Notion"
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
        let db = self.s.database_id.replace('-', "");
        let url = format!("https://api.notion.com/v1/databases/{db}/query");
        let today = ctx.today.format("%Y-%m-%d").to_string();
        let mut body = json!({ "filter": filter(&self.s, &today), "page_size": 100 });
        let mut tasks = Vec::new();
        for _ in 0..10 {
            let resp = ctx
                .http
                .post(&url)
                .set("Authorization", &format!("Bearer {token}"))
                .set("Notion-Version", NOTION_VERSION)
                .set("Content-Type", "application/json")
                .send_string(&body.to_string())?;
            let page: Value = decode(resp)?;
            tasks.extend(to_tasks(&page, &self.s, &self.id));
            match page["next_cursor"].as_str() {
                Some(c) if page["has_more"].as_bool() == Some(true) => {
                    body["start_cursor"] = json!(c);
                }
                _ => break,
            }
        }
        Ok(Output::tasks(tasks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../../tests/fixtures/notion_query.json")).unwrap()
    }

    #[test]
    fn parses_fixture_with_checkbox() {
        let s = Settings {
            priority_property: Some("Priority".into()),
            ..Settings::default()
        };
        let tasks = to_tasks(&fixture(), &s, "notion");
        assert_eq!(tasks.len(), 2, "done row skipped");
        assert_eq!(tasks[0].title, "Draft blog post");
        assert_eq!(tasks[0].priority, Priority::High);
        assert_eq!(tasks[0].due, NaiveDate::from_ymd_opt(2026, 10, 21));
        assert_eq!(tasks[1].priority, Priority::Urgent);
        assert_eq!(tasks[1].due, NaiveDate::from_ymd_opt(2026, 10, 19));
    }

    #[test]
    fn status_done_values() {
        let s = Settings {
            done_property: "Status".into(),
            done_values: vec!["Done".into()],
            ..Settings::default()
        };
        let tasks = to_tasks(&fixture(), &s, "notion");
        let titles: Vec<_> = tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, vec!["Draft blog post", "Fix login bug"]);
    }

    #[test]
    fn filter_shape() {
        let f = filter(&Settings::default(), "2026-10-21");
        assert_eq!(f["and"][0]["date"]["on_or_before"], "2026-10-21");
        assert_eq!(f["and"][1]["checkbox"]["equals"], false);
    }

    #[test]
    fn priority_labels() {
        assert_eq!(priority("High"), Priority::High);
        assert_eq!(priority("P1"), Priority::Urgent);
        assert_eq!(priority("whatever"), Priority::None);
    }
}

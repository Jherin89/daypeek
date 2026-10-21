//! Task providers: one module per app, all behind the [`Provider`] trait.
//!
//! Adding a provider is three steps (see CONTRIBUTING.md): write a module that
//! implements [`Provider`], register it in [`REGISTRY`], and add fixtures.

pub mod gtasks;
pub mod markdown;
pub mod mdtasks;
pub mod mstodo;
pub mod notion;
pub mod obsidian;
pub mod reminders;
pub mod things;
pub mod ticktick;
pub mod todoist;
pub mod todotxt;

use chrono::NaiveDate;
use chrono_tz::Tz;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::{Error, Result};
use crate::http::Http;
use crate::model::{NoteExcerpt, Task};

/// Where a provider gets its data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A web API (needs network + credentials).
    Cloud,
    /// Files on disk.
    Local,
    /// A macOS app's local data or scripting bridge.
    Macos,
}

/// Everything a provider needs to run.
pub struct Ctx<'a> {
    pub http: &'a Http,
    /// "Today" in the user's zone.
    pub today: NaiveDate,
    /// Last day the current view covers (`today` for the default view,
    /// tomorrow for `daypeek tomorrow`, today + 6 for `daypeek week`).
    /// Providers that filter server-side fetch through this date.
    pub until: NaiveDate,
    pub tz: Tz,
}

impl Ctx<'_> {
    /// Days covered from today through `until`, inclusive (1 for today).
    pub fn days(&self) -> i64 {
        (self.until - self.today).num_days().max(0) + 1
    }
}

/// What a provider returns.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Output {
    pub tasks: Vec<Task>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<NoteExcerpt>,
}

impl Output {
    pub fn tasks(tasks: Vec<Task>) -> Self {
        Output { tasks, note: None }
    }
}

/// Result of `daypeek providers`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "status", content = "detail")]
pub enum Health {
    Ok(String),
    AuthMissing(String),
    Unsupported(String),
    Error(String),
}

/// A source of tasks (and optionally a daily-note excerpt).
pub trait Provider: Send + Sync {
    /// Instance id: the `[providers.<id>]` key. Used as the task's source badge.
    fn id(&self) -> &str;
    /// Provider type, e.g. `"todoist"`.
    fn kind_id(&self) -> &'static str;
    /// Display name, e.g. `"Todoist"`.
    fn name(&self) -> &'static str;
    fn kind(&self) -> Kind;

    /// Cheap pre-flight: are credentials / paths present? No network.
    fn check(&self) -> Result<()> {
        Ok(())
    }

    /// Fetch open tasks due today or overdue (and undated ones if configured).
    fn fetch(&self, ctx: &Ctx) -> Result<Output>;

    /// Full health check used by `daypeek providers`.
    fn health(&self, ctx: &Ctx) -> Health {
        if let Err(e) = self.check() {
            return health_from_error(e);
        }
        match self.fetch(ctx) {
            Ok(out) => {
                let mut msg = format!(
                    "{} task{} due",
                    out.tasks.len(),
                    if out.tasks.len() == 1 { "" } else { "s" }
                );
                if out.note.is_some() {
                    msg.push_str(" + daily note");
                }
                Health::Ok(msg)
            }
            Err(e) => health_from_error(e),
        }
    }
}

pub fn health_from_error(e: Error) -> Health {
    match e {
        Error::AuthMissing(m) => Health::AuthMissing(m),
        Error::Unsupported(m) => Health::Unsupported(m),
        other => Health::Error(other.to_string()),
    }
}

type Builder = fn(id: &str, table: toml::Table) -> Result<Box<dyn Provider>>;

/// `(type id, display name, kind, builder)` for every built-in provider.
pub const REGISTRY: &[(&str, &str, Kind, Builder)] = &[
    ("todoist", "Todoist", Kind::Cloud, todoist::build),
    ("ticktick", "TickTick", Kind::Cloud, ticktick::build),
    ("mstodo", "Microsoft To Do", Kind::Cloud, mstodo::build),
    ("gtasks", "Google Tasks", Kind::Cloud, gtasks::build),
    ("things", "Things 3", Kind::Macos, things::build),
    (
        "reminders",
        "Apple Reminders",
        Kind::Macos,
        reminders::build,
    ),
    ("notion", "Notion", Kind::Cloud, notion::build),
    ("obsidian", "Obsidian", Kind::Local, obsidian::build),
    ("todotxt", "todo.txt", Kind::Local, todotxt::build),
    ("markdown", "Markdown", Kind::Local, markdown::build),
];

/// A provider that failed to build (bad config); still listed by `daypeek providers`.
pub struct Broken {
    pub id: String,
    pub error: Error,
}

/// Build every enabled provider from `[providers.*]`.
pub fn build_all(cfg: &Config) -> (Vec<Box<dyn Provider>>, Vec<Broken>) {
    let mut ok = Vec::new();
    let mut broken = Vec::new();
    for (id, table) in &cfg.providers {
        let mut table = table.clone();
        let enabled = match table.remove("enabled") {
            None => true,
            Some(toml::Value::Boolean(b)) => b,
            Some(_) => {
                broken.push(Broken {
                    id: id.clone(),
                    error: Error::Config("`enabled` must be true or false".into()),
                });
                continue;
            }
        };
        let kind = match table.remove("type") {
            Some(toml::Value::String(s)) => s,
            Some(_) => {
                broken.push(Broken {
                    id: id.clone(),
                    error: Error::Config("`type` must be a string".into()),
                });
                continue;
            }
            None => id.clone(),
        };
        if !enabled {
            continue;
        }
        match REGISTRY.iter().find(|(k, ..)| *k == kind) {
            Some((_, _, _, build)) => match build(id, table) {
                Ok(p) => ok.push(p),
                Err(error) => broken.push(Broken {
                    id: id.clone(),
                    error,
                }),
            },
            None => broken.push(Broken {
                id: id.clone(),
                error: Error::Config(format!(
                    "unknown provider type {kind:?} (known: {})",
                    REGISTRY.iter().map(|r| r.0).collect::<Vec<_>>().join(", ")
                )),
            }),
        }
    }
    (ok, broken)
}

/// Deserialize a provider's settings table with a helpful error.
pub fn settings<T: DeserializeOwned>(id: &str, table: toml::Table) -> Result<T> {
    toml::Value::Table(table)
        .try_into()
        .map_err(|e| Error::Config(format!("[providers.{id}]: {e}")))
}

/// Parse the leading `YYYY-MM-DD` of an API date or datetime string.
pub fn leading_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_ids_are_unique() {
        let mut ids: Vec<_> = REGISTRY.iter().map(|r| r.0).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), REGISTRY.len());
    }

    #[test]
    fn build_all_handles_types_disabled_and_unknown() {
        let cfg = Config::parse(
            r#"
            [providers.todoist]
            [providers.work]
            type = "markdown"
            paths = ["/tmp/x.md"]
            [providers.off]
            type = "todotxt"
            enabled = false
            [providers.wat]
            [providers.notion]
            bogus_key = 1
            "#,
        )
        .unwrap();
        let (ok, broken) = build_all(&cfg);
        let ok_ids: Vec<_> = ok.iter().map(|p| (p.id(), p.kind_id())).collect();
        assert!(ok_ids.contains(&("todoist", "todoist")));
        assert!(ok_ids.contains(&("work", "markdown")));
        assert!(!ok_ids.iter().any(|(id, _)| *id == "off"));
        let broken_ids: Vec<_> = broken.iter().map(|b| b.id.as_str()).collect();
        assert!(broken_ids.contains(&"wat"));
        assert!(broken_ids.contains(&"notion"));
    }

    #[test]
    fn leading_dates() {
        assert_eq!(
            leading_date("2026-10-21"),
            NaiveDate::from_ymd_opt(2026, 10, 21)
        );
        assert_eq!(
            leading_date("2026-10-21T09:00:00Z"),
            NaiveDate::from_ymd_opt(2026, 10, 21)
        );
        assert_eq!(leading_date("10/21"), None);
    }
}

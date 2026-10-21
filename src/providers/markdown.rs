//! Plain Markdown files (or folders of them) with `- [ ]` checkboxes.
//! Same date syntax as the Obsidian provider; undated tasks are shown by default.

use serde::Deserialize;

use super::mdtasks;
use super::obsidian::markdown_files;
use super::{settings, Ctx, Kind, Output, Provider};
use crate::config::expand_tilde;
use crate::error::{Error, Result};

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Files or directories (scanned recursively for `*.md`).
    pub paths: Vec<String>,
    pub include_undated: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            paths: vec!["~/.config/daypeek/tasks.md".into()],
            include_undated: true,
        }
    }
}

pub struct Markdown {
    id: String,
    s: Settings,
}

pub fn build(id: &str, table: toml::Table) -> Result<Box<dyn Provider>> {
    Ok(Box::new(Markdown {
        id: id.into(),
        s: settings(id, table)?,
    }))
}

impl Provider for Markdown {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind_id(&self) -> &'static str {
        "markdown"
    }
    fn name(&self) -> &'static str {
        "Markdown"
    }
    fn kind(&self) -> Kind {
        Kind::Local
    }

    fn check(&self) -> Result<()> {
        let missing: Vec<_> = self
            .s
            .paths
            .iter()
            .filter(|p| !expand_tilde(p).exists())
            .cloned()
            .collect();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(Error::Config(format!("not found: {}", missing.join(", "))))
        }
    }

    fn fetch(&self, _ctx: &Ctx) -> Result<Output> {
        self.check()?;
        let mut tasks = Vec::new();
        for p in &self.s.paths {
            let path = expand_tilde(p);
            let files = if path.is_dir() {
                markdown_files(&path, &[], 20_000)
            } else {
                vec![path.clone()]
            };
            for f in files {
                let text = std::fs::read_to_string(&f)?;
                let ctx = f
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                tasks.extend(mdtasks::to_tasks(
                    mdtasks::parse_document(&text),
                    &self.id,
                    &ctx,
                    self.s.include_undated,
                    None,
                ));
            }
        }
        Ok(Output::tasks(tasks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Http;
    use crate::model::Priority;
    use chrono::NaiveDate;

    #[test]
    fn reads_fixture_file() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tasks.md");
        let mut t = toml::Table::new();
        t.insert("paths".into(), toml::Value::Array(vec![path.into()]));
        let p = build("markdown", t).unwrap();
        let http = Http::new(std::time::Duration::from_secs(1));
        let ctx = Ctx {
            http: &http,
            today: NaiveDate::from_ymd_opt(2026, 10, 21).unwrap(),
            until: NaiveDate::from_ymd_opt(2026, 10, 21).unwrap(),
            tz: chrono_tz::UTC,
        };
        let tasks = p.fetch(&ctx).unwrap().tasks;
        let titles: Vec<_> = tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["Buy coffee beans", "Fix bike tire", "Read chapter 4"]
        );
        assert_eq!(tasks[1].priority, Priority::High);
        assert_eq!(tasks[1].due, NaiveDate::from_ymd_opt(2026, 10, 20));
        assert_eq!(tasks[0].context.as_deref(), Some("tasks"));
    }
}

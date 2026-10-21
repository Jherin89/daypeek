//! Obsidian: scans a vault for open checkbox tasks with due dates (Tasks
//! plugin / Dataview syntax) and shows an excerpt of today's daily note.
//! Undated tasks inside today's daily note count as due today.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::mdtasks;
use super::{settings, Ctx, Kind, Output, Provider};
use crate::config::expand_tilde;
use crate::error::{Error, Result};
use crate::model::NoteExcerpt;

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub vault: String,
    /// Folder (relative to the vault) holding daily notes; `""` = vault root.
    pub daily_notes_folder: String,
    /// strftime pattern for daily-note file names (without `.md`).
    pub daily_note_format: String,
    /// Lines of the daily note to show (0 disables the excerpt).
    pub excerpt_lines: usize,
    /// Also show tasks with no date anywhere in the vault (noisy; off by default).
    pub include_undated: bool,
    /// Folder names to skip (dot-folders are always skipped).
    pub ignore: Vec<String>,
    /// Stop after this many markdown files.
    pub max_files: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            vault: String::new(),
            daily_notes_folder: String::new(),
            daily_note_format: "%Y-%m-%d".into(),
            excerpt_lines: 4,
            include_undated: false,
            ignore: vec!["templates".into(), "Templates".into()],
            max_files: 20_000,
        }
    }
}

pub struct Obsidian {
    id: String,
    s: Settings,
}

pub fn build(id: &str, table: toml::Table) -> Result<Box<dyn Provider>> {
    let s: Settings = settings(id, table)?;
    if s.vault.trim().is_empty() {
        return Err(Error::Config(format!(
            "[providers.{id}] needs vault = \"path\""
        )));
    }
    Ok(Box::new(Obsidian { id: id.into(), s }))
}

/// Collect `*.md` files under `root`, skipping dot-folders and `ignore`.
pub fn markdown_files(root: &Path, ignore: &[String], max: usize) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if !ignore.iter().any(|i| i == &name) {
                    stack.push(e.path());
                }
            } else if name.ends_with(".md") {
                out.push(e.path());
                if out.len() >= max {
                    return out;
                }
            }
        }
    }
    out.sort();
    out
}

/// First `n` meaningful lines of a note: no frontmatter, blanks, or tasks.
pub fn excerpt(text: &str, n: usize) -> Vec<String> {
    mdtasks::strip_frontmatter(text)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != "---" && !l.starts_with('#'))
        .filter(|l| {
            mdtasks::parse_line(l).is_none() && !l.starts_with("- [x]") && !l.starts_with("- [X]")
        })
        .take(n)
        .map(|l| {
            let l = l.trim_start_matches('#').trim();
            if l.chars().count() > 100 {
                format!("{}…", l.chars().take(99).collect::<String>())
            } else {
                l.to_string()
            }
        })
        .filter(|l| !l.is_empty())
        .collect()
}

impl Obsidian {
    fn vault(&self) -> PathBuf {
        expand_tilde(&self.s.vault)
    }

    pub fn daily_note_path(&self, day: chrono::NaiveDate) -> PathBuf {
        let mut p = self.vault();
        if !self.s.daily_notes_folder.is_empty() {
            p = p.join(&self.s.daily_notes_folder);
        }
        p.join(format!("{}.md", day.format(&self.s.daily_note_format)))
    }
}

impl Provider for Obsidian {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind_id(&self) -> &'static str {
        "obsidian"
    }
    fn name(&self) -> &'static str {
        "Obsidian"
    }
    fn kind(&self) -> Kind {
        Kind::Local
    }

    fn check(&self) -> Result<()> {
        let v = self.vault();
        if v.is_dir() {
            Ok(())
        } else {
            Err(Error::Config(format!("vault {} not found", v.display())))
        }
    }

    fn fetch(&self, ctx: &Ctx) -> Result<Output> {
        self.check()?;
        let vault = self.vault();
        let daily = self.daily_note_path(ctx.today);
        let mut tasks = Vec::new();
        for file in markdown_files(&vault, &self.s.ignore, self.s.max_files) {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let rel = file.strip_prefix(&vault).unwrap_or(&file);
            let context = rel.with_extension("").to_string_lossy().to_string();
            let is_daily = file == daily;
            tasks.extend(mdtasks::to_tasks(
                mdtasks::parse_document(&text),
                &self.id,
                &context,
                self.s.include_undated,
                is_daily.then_some(ctx.today),
            ));
        }
        let note = if self.s.excerpt_lines > 0 {
            std::fs::read_to_string(&daily).ok().and_then(|text| {
                let lines = excerpt(&text, self.s.excerpt_lines);
                (!lines.is_empty()).then(|| NoteExcerpt {
                    title: daily
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    lines,
                    source: self.id.clone(),
                })
            })
        } else {
            None
        };
        Ok(Output { tasks, note })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Http;
    use crate::model::Priority;
    use chrono::NaiveDate;

    fn fixture_vault() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault")
    }

    #[test]
    fn scans_fixture_vault() {
        let mut t = toml::Table::new();
        t.insert(
            "vault".into(),
            fixture_vault().to_string_lossy().to_string().into(),
        );
        t.insert("daily_notes_folder".into(), "Daily".into());
        let p = build("obsidian", t).unwrap();
        let http = Http::new(std::time::Duration::from_secs(1));
        let ctx = Ctx {
            http: &http,
            today: NaiveDate::from_ymd_opt(2026, 10, 21).unwrap(),
            tz: chrono_tz::America::New_York,
        };
        let out = p.fetch(&ctx).unwrap();
        let mut titles: Vec<_> = out.tasks.iter().map(|t| t.title.as_str()).collect();
        titles.sort();
        assert_eq!(
            titles,
            vec![
                "Call the vet",                // undated, but in today's daily note
                "Email Sam the slides",        // Dataview due
                "Future task",                 // future dates are filtered later, by tasks::merge
                "Outline talk for Augusta.rs", // 📅 today
                "Update resume",               // overdue
            ]
        );
        assert!(
            !titles.contains(&"Template task"),
            "templates folder ignored"
        );
        assert!(!titles.contains(&"Hidden"), "dot-folders ignored");
        let talk = out
            .tasks
            .iter()
            .find(|t| t.title.starts_with("Outline"))
            .unwrap();
        assert_eq!(talk.priority, Priority::High);
        assert_eq!(talk.context.as_deref(), Some("Projects/Talks"));
        let note = out.note.unwrap();
        assert_eq!(note.title, "2026-10-21");
        assert_eq!(
            note.lines[0],
            "Slept well. Big focus today: the talk outline."
        );
        assert!(note.lines.iter().all(|l| !l.contains("[ ]")));
    }
}

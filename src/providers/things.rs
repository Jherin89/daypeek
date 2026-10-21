//! Things 3 (macOS): reads the app's local SQLite database **read-only**.
//!
//! Shows to-dos in Things' *Today* list (start date ≤ today) plus anything
//! with a deadline today or earlier. Nothing is ever written back.

use std::path::{Path, PathBuf};

use chrono::{Datelike, NaiveDate};
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;

use super::{settings, Ctx, Kind, Output, Provider};
use crate::config::expand_tilde;
use crate::error::{Error, Result};
use crate::model::{Priority, Task};

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Override the database path (auto-detected on macOS).
    pub db_path: Option<String>,
}

pub struct Things {
    id: String,
    s: Settings,
}

pub fn build(id: &str, table: toml::Table) -> Result<Box<dyn Provider>> {
    Ok(Box::new(Things {
        id: id.into(),
        s: settings(id, table)?,
    }))
}

/// Things 3.13+ packs dates as `year << 16 | month << 12 | day << 7`.
pub fn pack_date(d: NaiveDate) -> i64 {
    (i64::from(d.year()) << 16) | (i64::from(d.month()) << 12) | (i64::from(d.day()) << 7)
}

pub fn unpack_date(v: i64) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(
        (v >> 16) as i32,
        ((v >> 12) & 0xF) as u32,
        ((v >> 7) & 0x1F) as u32,
    )
}

/// `~/Library/Group Containers/JLMPQHK86H.com.culturedcode.ThingsMac/ThingsData-*/Things Database.thingsdatabase/main.sqlite`
pub fn default_db_path() -> Option<PathBuf> {
    let group = expand_tilde("~/Library/Group Containers/JLMPQHK86H.com.culturedcode.ThingsMac");
    let tail = Path::new("Things Database.thingsdatabase").join("main.sqlite");
    if let Ok(entries) = std::fs::read_dir(&group) {
        for e in entries.flatten() {
            let name = e.file_name();
            if name.to_string_lossy().starts_with("ThingsData-") {
                let p = e.path().join(&tail);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    let legacy = group.join(&tail);
    legacy.exists().then_some(legacy)
}

/// Query an open Things database for today's to-dos.
pub fn query(conn: &Connection, today: NaiveDate, source: &str) -> Result<Vec<Task>> {
    let t = pack_date(today);
    let mut stmt = conn.prepare(
        "SELECT t.title, t.startDate, t.deadline, COALESCE(p.title, a.title, '')
           FROM TMTask t
           LEFT JOIN TMTask p ON p.uuid = t.project
           LEFT JOIN TMArea a ON a.uuid = t.area
          WHERE t.type = 0 AND t.status = 0 AND t.trashed = 0
            AND ((t.start = 1 AND t.startDate IS NOT NULL AND t.startDate <= ?1)
                 OR (t.deadline IS NOT NULL AND t.deadline <= ?1))
          ORDER BY t.todayIndex, t.\"index\"",
    )?;
    let rows = stmt.query_map([t], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?,
            r.get::<_, Option<i64>>(1)?,
            r.get::<_, Option<i64>>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (title, start, deadline, ctx) = row?;
        let Some(title) = title.filter(|t| !t.trim().is_empty()) else {
            continue;
        };
        let deadline = deadline.and_then(unpack_date);
        let start = start.and_then(unpack_date);
        // A to-do in Today with a future deadline is still "today".
        let due = match deadline {
            Some(d) if d <= today => Some(d),
            _ => start.map(|s| s.min(today)),
        };
        let mut task = Task::new(title, due, Priority::None).with_context(ctx);
        task.sources.push(source.to_string());
        out.push(task);
    }
    Ok(out)
}

impl Things {
    fn db_path(&self) -> Result<PathBuf> {
        if let Some(p) = &self.s.db_path {
            return Ok(expand_tilde(p));
        }
        if !cfg!(target_os = "macos") {
            return Err(Error::Unsupported(
                "Things 3 is macOS-only (set db_path to test)".into(),
            ));
        }
        default_db_path().ok_or_else(|| Error::Config("Things 3 database not found".into()))
    }
}

impl Provider for Things {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind_id(&self) -> &'static str {
        "things"
    }
    fn name(&self) -> &'static str {
        "Things 3"
    }
    fn kind(&self) -> Kind {
        Kind::Macos
    }

    fn check(&self) -> Result<()> {
        let p = self.db_path()?;
        if p.exists() {
            Ok(())
        } else {
            Err(Error::Config(format!("{} not found", p.display())))
        }
    }

    fn fetch(&self, ctx: &Ctx) -> Result<Output> {
        let path = self.db_path()?;
        let conn = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.busy_timeout(std::time::Duration::from_millis(500))?;
        Ok(Output::tasks(query(&conn, ctx.today, &self.id)?))
    }
}

/// uuid, title, type, status, start, startDate, deadline, project
#[cfg(test)]
type FixtureRow<'a> = (
    &'a str,
    &'a str,
    i64,
    i64,
    i64,
    Option<i64>,
    Option<i64>,
    Option<&'a str>,
);

/// Build a minimal Things-shaped database (tests + docs).
#[cfg(test)]
pub fn fixture_db(path: &Path, today: NaiveDate) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(include_str!("../../tests/fixtures/things_schema.sql"))
        .unwrap();
    let d = |offset: i64| pack_date(today + chrono::Duration::days(offset));
    let rows: &[FixtureRow] = &[
        // uuid, title, type, status, start, startDate, deadline, project
        ("P1", "Home", 1, 0, 1, None, None, None),
        (
            "T1",
            "Fix the porch light",
            0,
            0,
            1,
            Some(d(0)),
            None,
            Some("P1"),
        ),
        ("T2", "Pay water bill", 0, 0, 2, None, Some(d(-2)), None),
        ("T3", "Plan weekend", 0, 0, 1, Some(d(-1)), Some(d(5)), None),
        ("T4", "Future thing", 0, 0, 1, Some(d(3)), None, None),
        ("T5", "Done thing", 0, 3, 1, Some(d(0)), None, None),
        ("T6", "Someday", 0, 0, 2, None, None, None),
    ];
    for (i, (uuid, title, ty, status, start, sd, dl, proj)) in rows.iter().enumerate() {
        conn.execute(
            "INSERT INTO TMTask (uuid, title, type, status, trashed, start, startDate, deadline, project, area, todayIndex, \"index\")
             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, ?8, NULL, ?9, ?9)",
            rusqlite::params![uuid, title, ty, status, start, sd, dl, proj, i as i64],
        )
        .unwrap();
    }
    conn
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_dates_round_trip() {
        let d = NaiveDate::from_ymd_opt(2026, 10, 21).unwrap();
        assert_eq!(pack_date(d), 132_819_584);
        assert_eq!(unpack_date(pack_date(d)), Some(d));
    }

    #[test]
    fn reads_fixture_database_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.sqlite");
        let today = NaiveDate::from_ymd_opt(2026, 10, 21).unwrap();
        drop(fixture_db(&path, today));

        let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let tasks = query(&conn, today, "things").unwrap();
        let titles: Vec<_> = tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["Fix the porch light", "Pay water bill", "Plan weekend"]
        );
        assert_eq!(tasks[0].context.as_deref(), Some("Home"));
        assert_eq!(tasks[1].due, Some(today - chrono::Duration::days(2)));
        assert_eq!(tasks[2].due, Some(today - chrono::Duration::days(1)));
        // Read-only really is read-only.
        assert!(conn.execute("DELETE FROM TMTask", []).is_err());
    }
}

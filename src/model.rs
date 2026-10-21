//! Core data types shared by every section of the dashboard.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// Normalized task priority. Every provider maps its own scale onto these four
/// levels, so a Todoist `p1`, a TickTick "high", and an Obsidian `⏫` all sort
/// and color the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    None = 0,
    Low = 1,
    Medium = 2,
    High = 3,
    Urgent = 4,
}

impl Priority {
    /// Short label used in the rendered list.
    pub fn label(self) -> &'static str {
        match self {
            Priority::Urgent => "p1",
            Priority::High => "p2",
            Priority::Medium => "p3",
            Priority::Low => "p4",
            Priority::None => "  ",
        }
    }

    /// Terminal color as an RGB triple (`None` = default foreground).
    pub fn rgb(self) -> Option<(u8, u8, u8)> {
        match self {
            Priority::Urgent => Some((235, 87, 87)),  // red
            Priority::High => Some((242, 153, 74)),   // orange
            Priority::Medium => Some((86, 156, 214)), // blue
            Priority::Low => Some((140, 140, 140)),   // grey
            Priority::None => None,
        }
    }
}

/// One task, after normalization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub title: String,
    /// Due (or scheduled) date in the user's local calendar.
    pub due: Option<NaiveDate>,
    pub priority: Priority,
    /// Provider instance ids that reported this task (more than one after de-dup).
    pub sources: Vec<String>,
    /// Project / list / file the task lives in, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

impl Task {
    pub fn new(title: impl Into<String>, due: Option<NaiveDate>, priority: Priority) -> Self {
        Task {
            title: title.into(),
            due,
            priority,
            sources: Vec::new(),
            context: None,
        }
    }

    pub fn with_context(mut self, ctx: impl Into<String>) -> Self {
        let c = ctx.into();
        if !c.is_empty() {
            self.context = Some(c);
        }
        self
    }

    /// Days overdue relative to `today` (0 when due today or undated).
    pub fn overdue_days(&self, today: NaiveDate) -> i64 {
        self.due.map(|d| (today - d).num_days().max(0)).unwrap_or(0)
    }
}

/// A short excerpt of today's daily note (Obsidian, plain markdown, ...).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteExcerpt {
    pub title: String,
    pub lines: Vec<String>,
    pub source: String,
}

/// A calendar event occurring (at least partly) today.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub title: String,
    pub all_day: bool,
    /// Start / end in UTC; for all-day events these span the local day(s).
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    pub calendar: String,
}

/// Current conditions plus today's range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Weather {
    pub place: String,
    pub temperature: f64,
    pub high: f64,
    pub low: f64,
    pub code: u8,
    pub condition: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precip_chance: Option<u8>,
    /// "°F" or "°C".
    pub unit: String,
    /// Daily forecast starting today (up to 7 days), for `tomorrow` / `week`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub days: Vec<DayForecast>,
}

/// One day of the forecast.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DayForecast {
    pub date: NaiveDate,
    pub high: f64,
    pub low: f64,
    pub code: u8,
    pub condition: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precip_chance: Option<u8>,
}

impl Weather {
    /// The forecast for `date`, if the fetched range covers it.
    pub fn day(&self, date: NaiveDate) -> Option<&DayForecast> {
        self.days.iter().find(|d| d.date == date)
    }
}

/// Freshness metadata attached to each section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "state")]
pub enum Freshness {
    /// Fetched just now.
    Live,
    /// Fetched successfully a few minutes ago and reused (see
    /// `general.cache_minutes`); as trustworthy as live, so no marker.
    Cached { fetched_at: DateTime<Utc> },
    /// Fetch failed; showing a cached copy from `fetched_at`.
    Stale {
        fetched_at: DateTime<Utc>,
        error: String,
    },
    /// Fetch failed and there is no cache.
    Failed { error: String },
}

impl Freshness {
    pub fn is_live(&self) -> bool {
        matches!(self, Freshness::Live)
    }

    /// Live or recently cached: the data is current, show no stale marker.
    pub fn is_ok(&self) -> bool {
        matches!(self, Freshness::Live | Freshness::Cached { .. })
    }
}

/// Data from one source plus how fresh it is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sourced<T> {
    pub source: String,
    pub freshness: Freshness,
    pub data: Option<T>,
}

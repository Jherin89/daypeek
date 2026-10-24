//! Gathering every section concurrently into one [`Snapshot`].

use std::time::Duration;

use chrono::{DateTime, Datelike, Days, NaiveDate, Utc};
use chrono_tz::Tz;
use serde::Serialize;

use crate::cache::Cache;
use crate::calendar;
use crate::config::Config;
use crate::error::Error;
use crate::http::Http;
use crate::model::{Event, Freshness, NoteExcerpt, Sourced, Task, Weather};
use crate::providers::{self, Ctx, Output};
use crate::tasks;
use crate::weather;

/// Date header info.
#[derive(Debug, Clone, Serialize)]
pub struct DateInfo {
    pub date: NaiveDate,
    pub weekday: String,
    pub iso_week: u32,
    pub day_of_year: u32,
    pub days_in_year: u32,
    pub timezone: String,
}

impl DateInfo {
    pub fn new(now: DateTime<Tz>) -> Self {
        let d = now.date_naive();
        let days_in_year = if d.leap_year() { 366 } else { 365 };
        DateInfo {
            date: d,
            weekday: d.format("%A").to_string(),
            iso_week: d.iso_week().week(),
            day_of_year: d.ordinal(),
            days_in_year,
            timezone: now.timezone().name().to_string(),
        }
    }
}

/// Which days the dashboard covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum View {
    /// `daypeek`: today, with overdue tasks and live now/next markers.
    #[default]
    Today,
    /// `daypeek tomorrow`: just tomorrow's events and tasks due tomorrow.
    Tomorrow,
    /// `daypeek week`: today through the next six days, grouped by day.
    Week,
}

impl View {
    /// First and last local day covered, inclusive.
    pub fn range(self, today: NaiveDate) -> (NaiveDate, NaiveDate) {
        match self {
            View::Today => (today, today),
            View::Tomorrow => (today + Days::new(1), today + Days::new(1)),
            View::Week => (today, today + Days::new(6)),
        }
    }

    pub const ALL: [View; 3] = [View::Today, View::Tomorrow, View::Week];

    /// Task results differ per view, so each view caches separately.
    pub fn cache_suffix(self) -> &'static str {
        match self {
            View::Today => "",
            View::Tomorrow => "@tomorrow",
            View::Week => "@week",
        }
    }

    /// Every day in the view, in order.
    pub fn days(self, today: NaiveDate) -> Vec<NaiveDate> {
        let (start, end) = self.range(today);
        start.iter_days().take_while(|d| *d <= end).collect()
    }
}

/// Status of one source (a calendar feed or task provider).
#[derive(Debug, Clone, Serialize)]
pub struct SourceStatus {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub freshness: Freshness,
}

/// Everything the dashboard shows, ready to render or serialize.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub now: DateTime<Tz>,
    pub date: DateInfo,
    pub view: View,
    /// First and last day shown (both today for the default view).
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub events: Vec<Event>,
    pub calendars: Vec<SourceStatus>,
    pub tasks: Vec<Task>,
    pub providers: Vec<SourceStatus>,
    pub notes: Vec<NoteExcerpt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weather: Option<Sourced<Weather>>,
    /// Provider ids `daypeek done` may change (`allow_write = true`). Tasks
    /// from these get numbers on screen. Empty = read-only (the default).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub writable: Vec<String>,
    pub demo: bool,
}

/// Cache keys holding provider instance `id`'s task lists (one per view).
pub fn provider_cache_keys(id: &str) -> Vec<String> {
    View::ALL
        .iter()
        .map(|v| format!("provider-{id}{}", v.cache_suffix()))
        .collect()
}

/// Resolve the user's time zone: config → system → UTC.
pub fn resolve_tz(cfg: &Config) -> Tz {
    cfg.general
        .timezone
        .as_deref()
        .and_then(|t| t.parse().ok())
        .unwrap_or_else(system_tz)
}

/// The system time zone, or UTC if it can't be determined.
pub fn system_tz() -> Tz {
    iana_time_zone::get_timezone()
        .ok()
        .and_then(|t| t.parse().ok())
        .unwrap_or(chrono_tz::UTC)
}

/// A warning when `general.timezone` is set but isn't a valid IANA name
/// (it would otherwise be ignored silently).
pub fn timezone_warning(cfg: &Config) -> Option<String> {
    let name = cfg.general.timezone.as_deref()?;
    if name.parse::<Tz>().is_ok() {
        return None;
    }
    Some(format!(
        "unknown timezone {name:?} in [general] (use an IANA name like \"America/New_York\"); using {} instead",
        system_tz().name()
    ))
}

/// Fetch every configured source in parallel. Never fails: problems are
/// reported per source.
pub fn gather(cfg: &Config, cache: &Cache, now: DateTime<Utc>, view: View) -> Snapshot {
    let tz = resolve_tz(cfg);
    let now = now.with_timezone(&tz);
    let today = now.date_naive();
    let (start, end) = view.range(today);
    let http = Http::new(Duration::from_secs(cfg.general.timeout_secs.max(1)));
    let ctx = Ctx {
        http: &http,
        today,
        until: end,
        tz,
    };
    let (built, broken) = providers::build_all(cfg);
    let writable: Vec<String> = built
        .iter()
        .filter(|p| p.can_complete() && providers::write_allowed(cfg, p.id()))
        .map(|p| p.id().to_string())
        .collect();

    let (cal_results, provider_results, weather) = std::thread::scope(|s| {
        let cal_handles: Vec<_> = cfg
            .calendars
            .iter()
            .map(|c| {
                let http = &http;
                s.spawn(move || {
                    let key = format!("calendar-{}", c.name);
                    let network = calendar::is_remote_source(c);
                    // Like providers: a missing password isn't a network
                    // problem, so say so even offline.
                    let feed: Sourced<String> = match calendar::check_source(c) {
                        Err(e) => Sourced {
                            source: c.name.clone(),
                            freshness: Freshness::Failed {
                                error: e.to_string(),
                            },
                            data: None,
                        },
                        Ok(()) => cache.fetch_with(&key, &c.name, network, || {
                            calendar::fetch_source(http, c, today, tz)
                        }),
                    };
                    (c, feed)
                })
            })
            .collect();
        let prov_handles: Vec<_> = built
            .iter()
            .map(|p| {
                let ctx = &ctx;
                s.spawn(move || {
                    let key = provider_cache_keys(p.id())[view as usize].clone();
                    let network = p.kind() == providers::Kind::Cloud;
                    // Missing credentials / wrong platform aren't network
                    // problems: report them even in --offline mode.
                    let out: Sourced<Output> = match p.check() {
                        Err(e) => Sourced {
                            source: p.id().to_string(),
                            freshness: Freshness::Failed {
                                error: e.to_string(),
                            },
                            data: None,
                        },
                        Ok(()) => cache.fetch_with(&key, p.id(), network, || p.fetch(ctx)),
                    };
                    (p.id().to_string(), p.name().to_string(), out)
                })
            })
            .collect();
        let weather_handle = cfg.weather().map(|w| {
            let http = &http;
            s.spawn(move || cache.fetch("weather", "Open-Meteo", || weather::fetch(http, cache, w)))
        });
        let cals: Vec<_> = cal_handles
            .into_iter()
            .filter_map(|h| h.join().ok())
            .collect();
        let provs: Vec<_> = prov_handles
            .into_iter()
            .filter_map(|h| h.join().ok())
            .collect();
        let weather = weather_handle.and_then(|h| h.join().ok());
        (cals, provs, weather)
    });

    let mut events = Vec::new();
    let mut calendars = Vec::new();
    for (c, feed) in cal_results {
        if let Some(text) = &feed.data {
            events.extend(calendar::events_between(text, &c.name, start, end, tz));
        }
        calendars.push(SourceStatus {
            id: c.name.clone(),
            name: calendar::source_label(c).into(),
            freshness: feed.freshness,
        });
    }
    calendar::sort_events(&mut events);

    let mut lists = Vec::new();
    let mut notes = Vec::new();
    let mut statuses = Vec::new();
    for (id, name, out) in provider_results {
        if let Some(o) = out.data {
            lists.push(o.tasks);
            // Today's daily note only belongs on today's dashboard.
            if out.freshness.is_ok() && view == View::Today {
                notes.extend(o.note);
            }
        }
        statuses.push(SourceStatus {
            id,
            name,
            freshness: out.freshness,
        });
    }
    for b in broken {
        statuses.push(SourceStatus {
            id: b.id,
            name: "?".into(),
            freshness: Freshness::Failed {
                error: b.error.to_string(),
            },
        });
    }

    Snapshot {
        now,
        date: DateInfo::new(now),
        view,
        start,
        end,
        events,
        calendars,
        tasks: tasks::merge_window(lists, today, start, end),
        providers: statuses,
        notes,
        weather,
        writable,
        demo: false,
    }
}

/// Is a failure just "not configured yet"? (rendered more quietly)
pub fn is_auth_missing(f: &Freshness) -> bool {
    match f {
        Freshness::Failed { error } | Freshness::Stale { error, .. } => {
            error.starts_with(&Error::AuthMissing(String::new()).to_string())
        }
        Freshness::Live | Freshness::Cached { .. } => false,
    }
}

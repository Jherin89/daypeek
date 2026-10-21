//! Gathering every section concurrently into one [`Snapshot`].

use std::time::Duration;

use chrono::{DateTime, Datelike, NaiveDate, Utc};
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
    pub events: Vec<Event>,
    pub calendars: Vec<SourceStatus>,
    pub tasks: Vec<Task>,
    pub providers: Vec<SourceStatus>,
    pub notes: Vec<NoteExcerpt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weather: Option<Sourced<Weather>>,
    pub demo: bool,
}

/// Resolve the user's time zone: config → system → UTC.
pub fn resolve_tz(cfg: &Config) -> Tz {
    cfg.general
        .timezone
        .as_deref()
        .and_then(|t| t.parse().ok())
        .or_else(|| iana_time_zone::get_timezone().ok()?.parse().ok())
        .unwrap_or(chrono_tz::UTC)
}

/// Fetch every configured source in parallel. Never fails: problems are
/// reported per source.
pub fn gather(cfg: &Config, cache: &Cache, now: DateTime<Utc>) -> Snapshot {
    let tz = resolve_tz(cfg);
    let now = now.with_timezone(&tz);
    let today = now.date_naive();
    let http = Http::new(Duration::from_secs(cfg.general.timeout_secs.max(1)));
    let ctx = Ctx {
        http: &http,
        today,
        tz,
    };
    let (built, broken) = providers::build_all(cfg);

    let (cal_results, provider_results, weather) = std::thread::scope(|s| {
        let cal_handles: Vec<_> = cfg
            .calendars
            .iter()
            .map(|c| {
                let http = &http;
                s.spawn(move || {
                    let key = format!("calendar-{}", c.name);
                    let network = calendar::is_remote(&c.url);
                    let feed = cache.fetch_with(&key, &c.name, network, || {
                        calendar::fetch_feed(http, &c.url)
                    });
                    (c, feed)
                })
            })
            .collect();
        let prov_handles: Vec<_> = built
            .iter()
            .map(|p| {
                let ctx = &ctx;
                s.spawn(move || {
                    let key = format!("provider-{}", p.id());
                    let network = p.kind() == providers::Kind::Cloud;
                    let out: Sourced<Output> = cache.fetch_with(&key, p.id(), network, || {
                        p.check()?;
                        p.fetch(ctx)
                    });
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
            events.extend(calendar::events_on(text, &c.name, today, tz));
        }
        calendars.push(SourceStatus {
            id: c.name.clone(),
            name: "iCal".into(),
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
            if out.freshness.is_live() {
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
        events,
        calendars,
        tasks: tasks::merge(lists, today),
        providers: statuses,
        notes,
        weather,
        demo: false,
    }
}

/// Is a failure just "not configured yet"? (rendered more quietly)
pub fn is_auth_missing(f: &Freshness) -> bool {
    match f {
        Freshness::Failed { error } | Freshness::Stale { error, .. } => {
            error.starts_with(&Error::AuthMissing(String::new()).to_string())
        }
        Freshness::Live => false,
    }
}

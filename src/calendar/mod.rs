//! Calendar section: iCal feeds → today's events.

pub mod ics;
pub mod rrule;

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc};
use chrono_tz::Tz;

use crate::config::expand_tilde;
use crate::error::{Error, Result};
use crate::http::Http;
use crate::model::Event;
use ics::{IcsTime, RawEvent};

/// Is this feed fetched over the network?
pub fn is_remote(url: &str) -> bool {
    let u = url.trim();
    u.starts_with("http://") || u.starts_with("https://") || u.starts_with("webcal://")
}

/// Download (or read) a feed. `webcal://` is treated as `https://`.
pub fn fetch_feed(http: &Http, url: &str) -> Result<String> {
    let url = url.trim();
    if let Some(rest) = url.strip_prefix("webcal://") {
        return http.get_text(&format!("https://{rest}"));
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return http.get_text(url);
    }
    let path = url.strip_prefix("file://").unwrap_or(url);
    std::fs::read_to_string(expand_tilde(path))
        .map_err(|e| Error::Config(format!("calendar file {path}: {e}")))
}

/// The UTC window covering local day `day` in `tz`.
pub fn day_window(day: NaiveDate, tz: Tz) -> (DateTime<Utc>, DateTime<Utc>) {
    let start = ics::to_utc(tz, day.and_time(NaiveTime::MIN));
    let end = ics::to_utc(tz, (day + Duration::days(1)).and_time(NaiveTime::MIN));
    (start, end)
}

/// Every event from `text` that overlaps local `day`, sorted (all-day first).
pub fn events_on(text: &str, calendar: &str, day: NaiveDate, tz: Tz) -> Vec<Event> {
    let raw = ics::parse(text, tz);
    let (win_start, win_end) = day_window(day, tz);

    // RECURRENCE-ID overrides replace the matching instance of their master.
    let mut overridden: HashMap<String, HashSet<DateTime<Utc>>> = HashMap::new();
    for ev in &raw {
        if let (Some(uid), Some(rid)) = (&ev.uid, &ev.recurrence_id) {
            overridden
                .entry(uid.clone())
                .or_default()
                .insert(rid.to_utc());
        }
    }

    let mut out = Vec::new();
    for ev in &raw {
        if ev.status.as_deref() == Some("CANCELLED") {
            continue;
        }
        let Some(start) = ev.dtstart else { continue };
        let length = event_length(ev, &start);
        let skip = ev
            .uid
            .as_ref()
            .filter(|_| ev.recurrence_id.is_none())
            .and_then(|u| overridden.get(u));

        for occ in occurrences(ev, &start, length, day) {
            let occ_utc = occ.to_utc();
            if skip.is_some_and(|s| s.contains(&occ_utc)) {
                continue;
            }
            if ev.exdates.iter().any(|x| same_instance(x, &occ)) {
                continue;
            }
            let (s, e) = if occ.date_only {
                // All-day events float: they cover the same *dates* everywhere.
                let first = occ.date();
                let days = length.num_days().max(1);
                let (s, _) = day_window(first, tz);
                let (_, e) = day_window(first + Duration::days(days - 1), tz);
                (s, e)
            } else {
                (occ_utc, occ_utc + length)
            };
            let overlaps = if s == e {
                s >= win_start && s < win_end
            } else {
                s < win_end && e > win_start
            };
            if overlaps {
                out.push(Event {
                    title: ev
                        .summary
                        .clone()
                        .filter(|t| !t.is_empty())
                        .unwrap_or_else(|| "(busy)".into()),
                    all_day: occ.date_only,
                    start: s,
                    end: e,
                    location: ev.location.clone(),
                    calendar: calendar.to_string(),
                });
            }
        }
    }
    sort_events(&mut out);
    out.dedup_by(|a, b| a.title == b.title && a.start == b.start && a.end == b.end);
    out
}

/// Every event overlapping any local day in `start..=end`, each listed once.
pub fn events_between(
    text: &str,
    calendar: &str,
    start: NaiveDate,
    end: NaiveDate,
    tz: Tz,
) -> Vec<Event> {
    let mut out: Vec<Event> = Vec::new();
    let mut day = start;
    while day <= end {
        for e in events_on(text, calendar, day, tz) {
            if !out.contains(&e) {
                out.push(e);
            }
        }
        day += Duration::days(1);
    }
    sort_events(&mut out);
    out
}

/// Does `e` overlap local day `day`? (Used to group a week's events.)
pub fn overlaps_day(e: &Event, day: NaiveDate, tz: Tz) -> bool {
    let (ws, we) = day_window(day, tz);
    if e.start == e.end {
        e.start >= ws && e.start < we
    } else {
        e.start < we && e.end > ws
    }
}

pub fn sort_events(events: &mut [Event]) {
    events.sort_by(|a, b| {
        b.all_day
            .cmp(&a.all_day)
            .then(a.start.cmp(&b.start))
            .then(a.title.cmp(&b.title))
    });
}

fn event_length(ev: &RawEvent, start: &IcsTime) -> Duration {
    if let Some(end) = ev.dtend {
        if start.date_only {
            return Duration::days((end.date() - start.date()).num_days().max(1));
        }
        let d = end.to_utc() - start.to_utc();
        return d.max(Duration::zero());
    }
    if let Some(d) = ev.duration {
        return d.max(Duration::zero());
    }
    if start.date_only {
        Duration::days(1)
    } else {
        Duration::zero()
    }
}

fn same_instance(a: &IcsTime, b: &IcsTime) -> bool {
    if a.date_only || b.date_only {
        a.date() == b.date()
    } else {
        a.to_utc() == b.to_utc()
    }
}

/// Occurrence start times that could touch `day`.
fn occurrences(ev: &RawEvent, start: &IcsTime, length: Duration, day: NaiveDate) -> Vec<IcsTime> {
    let Some(rule_text) = &ev.rrule else {
        let mut v = vec![*start];
        v.extend(ev.rdates.iter().copied());
        return v;
    };
    let rule = match rrule::Rule::parse(rule_text) {
        Ok(r) => r,
        Err(_) => return vec![*start], // unsupported: show the first instance only
    };
    // Look back far enough to catch multi-day occurrences, and ±1 day for
    // zone differences between the event and the viewer.
    let span = length.num_days() + 1;
    let from = day - Duration::days(span + 1);
    let to = day + Duration::days(1);
    let time = start.naive.time();
    let mut v: Vec<IcsTime> = rule
        .dates_between(start.date(), from, to)
        .into_iter()
        .map(|d| IcsTime {
            naive: d.and_time(time),
            ..*start
        })
        .filter(|occ| match rule.until {
            Some(rrule::Until::Instant(u)) => occ.to_utc() <= u,
            _ => true,
        })
        .collect();
    v.extend(ev.rdates.iter().copied());
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::America::New_York;

    const FIXTURE: &str = include_str!("../../tests/fixtures/calendar.ics");

    fn titles(day: NaiveDate, tz: Tz) -> Vec<String> {
        events_on(FIXTURE, "Test", day, tz)
            .into_iter()
            .map(|e| e.title)
            .collect()
    }

    fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn fixture_wednesday_agenda() {
        let t = titles(ymd(2026, 10, 21), New_York);
        assert_eq!(
            t,
            vec![
                "Conference (day 2 of 3)",
                "Mom's birthday",
                "Standup",
                "Dentist",
                "Design review (moved)",
                "Call with London",
                "Late UTC sync",
            ]
        );
    }

    #[test]
    fn timezones_convert_to_viewer_zone() {
        let evs = events_on(FIXTURE, "Test", ymd(2026, 10, 21), New_York);
        let london = evs.iter().find(|e| e.title == "Call with London").unwrap();
        // 20:00 Europe/London (BST) == 15:00 New York (EDT) == 19:00 UTC
        assert_eq!(london.start.format("%H:%M").to_string(), "19:00");
        let late = evs.iter().find(|e| e.title == "Late UTC sync").unwrap();
        // 01:30Z on the 22nd is still the evening of the 21st in New York.
        assert_eq!(late.start.format("%d %H:%M").to_string(), "22 01:30");
    }

    #[test]
    fn exdate_and_cancelled_are_hidden() {
        // Standup is EXDATE'd on Thursday the 22nd; the cancelled event is never shown.
        let t = titles(ymd(2026, 10, 22), New_York);
        assert!(!t.contains(&"Standup".to_string()));
        assert!(!t.iter().any(|x| x.contains("Cancelled")));
    }

    #[test]
    fn override_replaces_master_instance() {
        let t = titles(ymd(2026, 10, 21), New_York);
        assert!(t.contains(&"Design review (moved)".to_string()));
        assert!(!t.contains(&"Design review".to_string()));
        // The following week the master instance is back.
        let t = titles(ymd(2026, 10, 28), New_York);
        assert!(t.contains(&"Design review".to_string()));
    }

    #[test]
    fn all_day_end_is_exclusive() {
        // Birthday is a single all-day event on the 21st only.
        assert!(!titles(ymd(2026, 10, 22), New_York).contains(&"Mom's birthday".to_string()));
        // The 3-day conference covers 20–22 but not the 23rd.
        assert!(titles(ymd(2026, 10, 22), New_York)
            .iter()
            .any(|t| t.starts_with("Conference")));
        assert!(!titles(ymd(2026, 10, 23), New_York)
            .iter()
            .any(|t| t.starts_with("Conference")));
    }

    #[test]
    fn empty_or_garbage_feed_is_empty() {
        assert!(events_on("", "x", ymd(2026, 10, 21), New_York).is_empty());
        assert!(events_on("<html>login</html>", "x", ymd(2026, 10, 21), New_York).is_empty());
    }
}

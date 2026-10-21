//! `--demo`: a fully fake dashboard that needs no accounts or network.
//!
//! The fake data is pushed through the *real* parsers (iCal, Todoist JSON,
//! Reminders bridge JSON, Obsidian markdown), so the demo doubles as a smoke
//! test of the whole pipeline.

use chrono::{DateTime, Duration, NaiveDate};
use chrono_tz::Tz;

use crate::calendar;
use crate::model::{DayForecast, Freshness, NoteExcerpt, Priority, Sourced, Task, Weather};
use crate::providers::{mdtasks, reminders, todoist, Health};
use crate::snapshot::{DateInfo, Snapshot, SourceStatus, View};
use crate::tasks;

fn ymd(d: NaiveDate) -> String {
    d.format("%Y%m%d").to_string()
}

fn iso(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn demo_ics(today: NaiveDate, tz: Tz) -> String {
    let tzid = tz.name();
    // A timed event `off` days from today.
    let ev = |off: i64, uid: &str, title: &str, start: &str, end: &str, extra: &str| {
        let t = ymd(today + Duration::days(off));
        format!(
            "BEGIN:VEVENT\r\nUID:{uid}@demo\r\nSUMMARY:{title}\r\nDTSTART;TZID={tzid}:{t}T{start}00\r\nDTEND;TZID={tzid}:{t}T{end}00\r\n{extra}END:VEVENT\r\n"
        )
    };
    let mut s = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//daypeek//demo//EN\r\n");
    s.push_str(&format!(
        "BEGIN:VEVENT\r\nUID:bday@demo\r\nSUMMARY:Mom's birthday\r\nDTSTART;VALUE=DATE:{}\r\nDTEND;VALUE=DATE:{}\r\nRRULE:FREQ=YEARLY\r\nEND:VEVENT\r\n",
        ymd(today.with_year_or(1961)),
        ymd(today.with_year_or(1961) + Duration::days(1)),
    ));
    // Today.
    s.push_str(&ev(
        0,
        "standup",
        "Team standup",
        "0930",
        "0945",
        "LOCATION:Zoom\r\n",
    ));
    s.push_str(&ev(0, "one-on-one", "1:1 with Priya", "1030", "1100", ""));
    s.push_str(&ev(
        0,
        "focus",
        "Focus block: portfolio site",
        "1100",
        "1230",
        "",
    ));
    s.push_str(&ev(
        0,
        "lunch",
        "Lunch with Sam",
        "1230",
        "1330",
        "LOCATION:Boll Weevil Cafe\\, Augusta\r\n",
    ));
    s.push_str(&ev(
        0,
        "review",
        "Design review",
        "1500",
        "1545",
        "LOCATION:Zoom\r\n",
    ));
    s.push_str(&ev(
        0,
        "soccer",
        "Kids' soccer practice",
        "1730",
        "1830",
        "LOCATION:Evans Towne Center Park\r\n",
    ));
    // The rest of the week (for `daypeek tomorrow` / `daypeek week`).
    s.push_str(&ev(
        1,
        "dentist",
        "Dentist",
        "0900",
        "0945",
        "LOCATION:Bright Smiles Dental\r\n",
    ));
    s.push_str(&ev(
        1,
        "standup-2",
        "Team standup",
        "0930",
        "0945",
        "LOCATION:Zoom\r\n",
    ));
    s.push_str(&ev(
        1,
        "planning",
        "Sprint planning",
        "1300",
        "1400",
        "LOCATION:Zoom\r\n",
    ));
    s.push_str(&ev(
        2,
        "coffee",
        "Coffee with Maya",
        "0830",
        "0915",
        "LOCATION:Buona Caffe\r\n",
    ));
    s.push_str(&ev(2, "demo-day", "Demo day", "1600", "1700", ""));
    s.push_str(&format!(
        "BEGIN:VEVENT\r\nUID:trip@demo\r\nSUMMARY:Weekend in Savannah\r\nDTSTART;VALUE=DATE:{}\r\nDTEND;VALUE=DATE:{}\r\nEND:VEVENT\r\n",
        ymd(today + Duration::days(3)),
        ymd(today + Duration::days(5)),
    ));
    s.push_str(&ev(
        4,
        "brunch",
        "Brunch with the Hendersons",
        "1100",
        "1230",
        "",
    ));
    s.push_str(&ev(
        5,
        "standup-mon",
        "Team standup",
        "0930",
        "0945",
        "LOCATION:Zoom\r\n",
    ));
    s.push_str(&ev(6, "gym", "Gym", "0700", "0800", ""));
    s.push_str(&ev(
        6,
        "standup-3",
        "Team standup",
        "0930",
        "0945",
        "LOCATION:Zoom\r\n",
    ));
    s.push_str("END:VCALENDAR\r\n");
    s
}

trait WithYear {
    fn with_year_or(self, y: i32) -> NaiveDate;
}

impl WithYear for NaiveDate {
    fn with_year_or(self, y: i32) -> NaiveDate {
        use chrono::Datelike;
        self.with_year(y).unwrap_or(self)
    }
}

fn demo_todoist(today: NaiveDate) -> String {
    let d = |off: i64| iso(today + Duration::days(off));
    format!(
        r#"{{"results":[
          {{"content":"Submit expense report","priority":4,"due":{{"date":"{}"}}}},
          {{"content":"Review Alex's pull request","priority":3,"due":{{"date":"{}T15:00:00"}}}},
          {{"content":"Call the pharmacy","priority":2,"due":{{"date":"{}"}}}},
          {{"content":"Water the plants","priority":1,"due":{{"date":"{}"}}}},
          {{"content":"Pick up dry cleaning","priority":2,"due":{{"date":"{}"}}}},
          {{"content":"Send invoice to Acme","priority":4,"due":{{"date":"{}"}}}},
          {{"content":"Pack for Savannah","priority":3,"due":{{"date":"{}"}}}},
          {{"content":"Plan next week's sprint","priority":1,"due":{{"date":"{}"}}}}
        ],"next_cursor":null}}"#,
        d(-2),
        d(0),
        d(0),
        d(0),
        d(1),
        d(1),
        d(2),
        d(5)
    )
}

fn demo_reminders(today: NaiveDate) -> String {
    format!(
        r#"[{{"title":"Call the pharmacy","list":"Errands","due":"{0}","priority":1}},
            {{"title":"Take out recycling","list":"Home","due":"{0}","priority":0}}]"#,
        iso(today)
    )
}

const DEMO_DAILY_NOTE: &str = "---\ntags: [daily]\n---\n# Today\n\
Big rock: ship the first cut of `daypeek` and write the README.\n\
- [ ] Outline Augusta.rs lightning talk ⏫\n\
Remember: dentist moved to tomorrow at 9.\n";

/// Build the demo snapshot for `now`.
pub fn snapshot(now: DateTime<Tz>, view: View) -> Snapshot {
    let tz = now.timezone();
    let today = now.date_naive();
    let (start, end) = view.range(today);

    let events = calendar::events_between(&demo_ics(today, tz), "Personal", start, end, tz);

    let todo = todoist::to_tasks(
        serde_json::from_str(&demo_todoist(today)).expect("demo todoist json"),
        "todoist",
    );
    let rem = reminders::parse(&demo_reminders(today), "reminders").expect("demo reminders json");
    let obs = mdtasks::to_tasks(
        mdtasks::parse_document(DEMO_DAILY_NOTE),
        "obsidian",
        "Daily",
        false,
        Some(today),
    );
    let mut things = Task::new(
        "Book flights for Thanksgiving",
        Some(today - Duration::days(1)),
        Priority::None,
    )
    .with_context("Family");
    things.sources.push("things".into());
    let mut ticktick = Task::new("Renew car registration", Some(today), Priority::Medium);
    ticktick.sources.push("ticktick".into());

    let merged = tasks::merge_window(
        vec![todo, rem, obs, vec![things], vec![ticktick]],
        today,
        start,
        end,
    );

    let live = |id: &str, name: &str| SourceStatus {
        id: id.into(),
        name: name.into(),
        freshness: Freshness::Live,
    };
    let providers = vec![
        live("todoist", "Todoist"),
        live("reminders", "Apple Reminders"),
        live("obsidian", "Obsidian"),
        live("things", "Things 3"),
        SourceStatus {
            id: "ticktick".into(),
            name: "TickTick".into(),
            freshness: Freshness::Stale {
                fetched_at: (now - Duration::minutes(95)).to_utc(),
                error: "network: timed out".into(),
            },
        },
    ];

    let note = NoteExcerpt {
        title: iso(today),
        lines: crate::providers::obsidian::excerpt(DEMO_DAILY_NOTE, 3),
        source: "obsidian".into(),
    };

    Snapshot {
        now,
        date: DateInfo::new(now),
        view,
        start,
        end,
        events,
        calendars: vec![live("Personal", "iCal")],
        tasks: merged,
        providers,
        notes: if view == View::Today {
            vec![note]
        } else {
            Vec::new()
        },
        weather: Some(Sourced {
            source: "Open-Meteo".into(),
            freshness: Freshness::Live,
            data: Some(Weather {
                place: "Evans, Georgia".into(),
                temperature: 68.0,
                high: 76.0,
                low: 51.0,
                code: 2,
                condition: "Partly cloudy".into(),
                precip_chance: Some(10),
                unit: "°F".into(),
                days: demo_forecast(today),
            }),
        }),
        demo: true,
    }
}

fn demo_forecast(today: NaiveDate) -> Vec<DayForecast> {
    let days: [(f64, f64, u8, Option<u8>); 7] = [
        (76.0, 51.0, 2, Some(10)),
        (71.0, 50.0, 63, Some(70)),
        (68.0, 47.0, 3, Some(20)),
        (70.0, 45.0, 0, Some(0)),
        (74.0, 50.0, 1, Some(5)),
        (69.0, 52.0, 80, Some(55)),
        (72.0, 49.0, 2, Some(10)),
    ];
    days.iter()
        .enumerate()
        .map(|(i, &(high, low, code, precip_chance))| DayForecast {
            date: today + Duration::days(i as i64),
            high,
            low,
            code,
            condition: crate::weather::describe(code).0.into(),
            precip_chance,
        })
        .collect()
}

/// The demo with the network "down": cloud sources show cached data.
pub fn offline_snapshot(now: DateTime<Tz>, view: View) -> Snapshot {
    let mut s = snapshot(now, view);
    let stale = |mins: i64, error: &str| Freshness::Stale {
        fetched_at: (now - Duration::minutes(mins)).to_utc(),
        error: error.into(),
    };
    for c in &mut s.calendars {
        c.freshness = stale(184, "network: could not resolve host");
    }
    for p in &mut s.providers {
        if matches!(p.id.as_str(), "todoist" | "ticktick") {
            p.freshness = stale(184, "network: could not resolve host");
        }
    }
    if let Some(w) = &mut s.weather {
        w.freshness = stale(184, "network: could not resolve host");
    }
    s
}

/// Fake `daypeek providers` rows for `daypeek --demo providers`.
pub fn provider_rows() -> Vec<(String, String, String, Health)> {
    let r = |id: &str, name: &str, kind: &str, h: Health| (id.into(), name.into(), kind.into(), h);
    vec![
        r(
            "todoist",
            "Todoist",
            "cloud",
            Health::Ok("4 tasks due".into()),
        ),
        r(
            "ticktick",
            "TickTick",
            "cloud",
            Health::Error("network: timed out".into()),
        ),
        r(
            "mstodo",
            "Microsoft To Do",
            "cloud",
            Health::AuthMissing("run `daypeek auth mstodo`".into()),
        ),
        r(
            "things",
            "Things 3",
            "macos",
            Health::Ok("1 task due".into()),
        ),
        r(
            "reminders",
            "Apple Reminders",
            "macos",
            Health::Ok("2 tasks due".into()),
        ),
        r(
            "obsidian",
            "Obsidian",
            "local",
            Health::Ok("1 task due + daily note".into()),
        ),
        r(
            "Personal",
            "iCal calendar",
            "cloud",
            Health::Ok("7 events today".into()),
        ),
        r(
            "weather",
            "Open-Meteo",
            "cloud",
            Health::Ok("68°F Partly cloudy".into()),
        ),
    ]
}

/// Demo health rows for what the user picked in `daypeek --demo setup`.
pub fn provider_rows_for(a: &crate::setup::Answers) -> Vec<(String, String, String, Health)> {
    let canned = provider_rows();
    let mut rows = Vec::new();
    for app in &a.apps {
        let id = app.id();
        let row = canned
            .iter()
            .find(|r| r.0 == id)
            .cloned()
            .map(|mut r| {
                // Everything the user just set up looks healthy in the demo.
                if !matches!(r.3, Health::Ok(_)) {
                    r.3 = Health::Ok("3 tasks due".into());
                }
                r
            })
            .unwrap_or_else(|| {
                let (name, kind) = crate::providers::REGISTRY
                    .iter()
                    .find(|r| r.0 == id)
                    .map(|r| {
                        let kind = match r.2 {
                            crate::providers::Kind::Cloud => "cloud",
                            crate::providers::Kind::Local => "local",
                            crate::providers::Kind::Macos => "macos",
                        };
                        (r.1, kind)
                    })
                    .unwrap_or((id, "cloud"));
                (
                    id.into(),
                    name.into(),
                    kind.into(),
                    Health::Ok("2 tasks due".into()),
                )
            });
        rows.push(row);
    }
    for (name, _) in &a.calendars {
        rows.push((
            name.clone(),
            "iCal calendar".into(),
            "cloud".into(),
            Health::Ok("7 events today".into()),
        ));
    }
    if a.city.is_some() {
        rows.push((
            "weather".into(),
            "Open-Meteo".into(),
            "cloud".into(),
            Health::Ok("68°F Partly cloudy".into()),
        ));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn demo_is_rich_and_deduplicated() {
        let tz = chrono_tz::America::New_York;
        let now = tz.with_ymd_and_hms(2026, 10, 21, 10, 58, 0).unwrap();
        let s = snapshot(now, View::Today);
        assert_eq!(s.events.len(), 7);
        assert!(s.events[0].all_day);
        let pharmacy = s
            .tasks
            .iter()
            .find(|t| t.title == "Call the pharmacy")
            .unwrap();
        assert_eq!(pharmacy.sources, vec!["todoist", "reminders"]);
        assert_eq!(
            pharmacy.priority,
            Priority::Urgent,
            "max of merged priorities"
        );
        assert_eq!(s.tasks[0].priority, Priority::Urgent);
        assert!(s.tasks.iter().any(|t| t.sources == vec!["obsidian"]));
        assert_eq!(s.notes[0].lines.len(), 2);
        // Later tasks stay out of today's list.
        assert!(!s.tasks.iter().any(|t| t.title == "Pick up dry cleaning"));
    }

    #[test]
    fn demo_tomorrow_and_week() {
        let tz = chrono_tz::America::New_York;
        let now = tz.with_ymd_and_hms(2026, 10, 21, 10, 58, 0).unwrap();
        let today = now.date_naive();

        let t = snapshot(now, View::Tomorrow);
        assert_eq!(
            (t.start, t.end),
            (today + Duration::days(1), today + Duration::days(1))
        );
        let titles: Vec<_> = t.events.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, vec!["Dentist", "Team standup", "Sprint planning"]);
        assert!(
            t.tasks.iter().all(|x| x.due == Some(t.start)),
            "only due tomorrow"
        );
        assert_eq!(t.tasks.len(), 2);
        assert!(t.notes.is_empty(), "today's note is not tomorrow's");

        let w = snapshot(now, View::Week);
        assert_eq!(w.end, today + Duration::days(6));
        // The two-day trip is listed once, not once per day.
        assert_eq!(
            w.events
                .iter()
                .filter(|e| e.title == "Weekend in Savannah")
                .count(),
            1
        );
        assert!(w.tasks.iter().any(|x| x.title == "Plan next week's sprint"));
        assert!(w.tasks.iter().any(|x| x.overdue_days(today) > 0));
        let days = &w.weather.as_ref().unwrap().data.as_ref().unwrap().days;
        assert_eq!(days.len(), 7);
    }
}

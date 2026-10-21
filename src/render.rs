//! Rendering a [`Snapshot`]: the full dashboard and the `--compact` view,
//! for the today, tomorrow and week views.

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use chrono_tz::Tz;
use colored::{ColoredString, Colorize};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::calendar::overlaps_day;
use crate::config::TimeFormat;
use crate::model::{Event, Freshness, Priority, Task, Weather};
use crate::snapshot::{is_auth_missing, Snapshot, SourceStatus, View};
use crate::weather::describe;

/// Layout options.
pub struct Style {
    pub width: usize,
    pub time_format: TimeFormat,
    pub max_tasks: usize,
}

fn clock(t: DateTime<Tz>, f: TimeFormat) -> String {
    match f {
        TimeFormat::H12 => t.format("%-I:%M %p").to_string(),
        TimeFormat::H24 => t.format("%H:%M").to_string(),
    }
}

/// "3h 05m", "12m", "2d".
pub fn span(d: Duration) -> String {
    let m = d.num_minutes().max(0);
    if m >= 24 * 60 {
        format!("{}d", m / (24 * 60))
    } else if m >= 60 {
        format!("{}h {:02}m", m / 60, m % 60)
    } else {
        format!("{m}m")
    }
}

/// "3h ago", "2d ago" for stale markers.
pub fn ago(t: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let d = now - t;
    if d.num_minutes() < 1 {
        "just now".into()
    } else {
        format!("{} ago", span(d)).replace(" 00m", "")
    }
}

/// Truncate to a display width with an ellipsis.
pub fn fit(s: &str, width: usize) -> String {
    if s.width() <= width {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw + 1 > width {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

fn pad(s: &str, width: usize) -> String {
    let w = s.width();
    if w >= width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(width - w))
    }
}

/// Paint text in a task priority's color.
pub fn priority_paint(p: Priority, s: &str) -> ColoredString {
    match p.rgb() {
        Some((r, g, b)) => {
            let c = s.truecolor(r, g, b);
            if p >= Priority::High {
                c.bold()
            } else {
                c
            }
        }
        None => s.normal(),
    }
}

fn section(title: &str, extra: &str) -> String {
    if extra.is_empty() {
        format!("  {}", title.bold())
    } else {
        format!("  {}  {}", title.bold(), extra.dimmed())
    }
}

fn stale_note(f: &Freshness, now: DateTime<Utc>) -> Option<String> {
    match f {
        Freshness::Stale { fetched_at, .. } => Some(format!("cached {}", ago(*fetched_at, now))),
        _ => None,
    }
}

/// Lines describing sources that failed or are stale.
fn source_problems(sources: &[SourceStatus], now: DateTime<Utc>, w: usize) -> Vec<String> {
    // "    ↻ id " then the message, kept on one line.
    let room = |id: &str| w.saturating_sub(8 + id.width()).max(12);
    sources
        .iter()
        .filter_map(|s| match &s.freshness {
            Freshness::Live | Freshness::Cached { .. } => None,
            Freshness::Stale { fetched_at, error } => Some(format!(
                "    {} {} {}",
                "↻".yellow(),
                s.id.yellow(),
                fit(
                    &format!(
                        "offline, showing cache from {} ({})",
                        ago(*fetched_at, now),
                        short(error)
                    ),
                    room(&s.id)
                )
                .dimmed()
            )),
            Freshness::Failed { error } => {
                let mark = if is_auth_missing(&s.freshness) || error.starts_with("unsupported") {
                    "○".dimmed()
                } else {
                    "!".red()
                };
                Some(format!(
                    "    {} {} {}",
                    mark,
                    s.id,
                    fit(error, room(&s.id)).dimmed()
                ))
            }
        })
        .collect()
}

fn short(e: &str) -> String {
    fit(e, 70)
}

/// Shown wherever there is nothing configured yet.
pub const SETUP_HINT: &str =
    "No sources configured yet. Run `daypeek setup`, or try `daypeek --demo`.";

/// The full dashboard.
pub fn full(s: &Snapshot, st: &Style) -> String {
    let w = st.width.clamp(40, 110);
    let mut out = match s.view {
        View::Week => week(s, st, w),
        View::Today | View::Tomorrow => day(s, st, w),
    };
    if s.calendars.is_empty() && s.providers.is_empty() && s.weather.is_none() {
        out.push(String::new());
        out.push(format!("    {}", SETUP_HINT.dimmed()));
    }
    out.push(String::new());
    out.join("\n")
}

/// Header: big date line, a dim sub line, and a rule.
fn header(s: &Snapshot, w: usize, left: String, right: String) -> Vec<String> {
    let mut out = vec![String::new()];
    let gap = w.saturating_sub(4 + left.width() + right.width()).max(2);
    if right.is_empty() {
        out.push(format!("  {}", left.bold()));
    } else {
        out.push(format!(
            "  {}{}{}",
            left.bold(),
            " ".repeat(gap),
            right.bold()
        ));
    }
    let d = s.start;
    let (iso_week, day_of_year) = (d.iso_week().week(), d.ordinal());
    let days_in_year = if d.leap_year() { 366 } else { 365 };
    let demo = if s.demo { "demo data" } else { "" };
    let long = format!(
        "{} · week {} · day {} · {} days left",
        d.format("%Y"),
        iso_week,
        day_of_year,
        days_in_year - day_of_year
    );
    // Narrow terminal: drop the least useful part instead of wrapping.
    let sub = if 4 + long.width() + 2 + demo.width() <= w {
        long
    } else {
        format!(
            "{} · week {} · day {}",
            d.format("%Y"),
            iso_week,
            day_of_year
        )
    };
    let gap = w.saturating_sub(4 + sub.width() + demo.width()).max(2);
    if demo.is_empty() {
        out.push(format!("  {}", sub.dimmed()));
    } else {
        out.push(format!(
            "  {}{}{}",
            sub.dimmed(),
            " ".repeat(gap),
            demo.magenta()
        ));
    }
    out.push(format!("  {}", "─".repeat(w.saturating_sub(4)).dimmed()));
    out
}

/// Today (live, with now/next markers) or tomorrow (a plan).
fn day(s: &Snapshot, st: &Style, w: usize) -> Vec<String> {
    let now_utc = s.now.with_timezone(&Utc);
    let live = s.view == View::Today;
    let date = s.start;
    let (left, right) = if live {
        (
            s.now.format("%A, %B %-d").to_string(),
            format!("{} {}", clock(s.now, st.time_format), s.now.format("%Z")),
        )
    } else {
        (
            format!("Tomorrow · {}", date.format("%A, %B %-d")),
            String::new(),
        )
    };
    let mut out = header(s, w, left, right);

    // ── Calendar ───────────────────────────────────────────────────────────
    if !s.calendars.is_empty() {
        out.push(String::new());
        let events: Vec<&Event> = s.events.iter().collect();
        let count = events.len();
        let mut extra = format!("{count} event{}", if count == 1 { "" } else { "s" });
        if let Some(n) = s
            .calendars
            .iter()
            .find_map(|c| stale_note(&c.freshness, now_utc))
        {
            extra.push_str(&format!(" · {n}"));
        }
        out.push(section("CALENDAR", &extra));
        if events.is_empty() {
            let msg = if live {
                "Nothing scheduled. Enjoy the open day."
            } else {
                "Nothing scheduled tomorrow."
            };
            out.push(format!("    {}", msg.dimmed()));
        }
        out.extend(event_rows(&events, date, live, s, st, w));
        out.extend(source_problems(&s.calendars, now_utc, w));
    }

    // ── Tasks ──────────────────────────────────────────────────────────────
    if !s.providers.is_empty() {
        out.push(String::new());
        let overdue = s
            .tasks
            .iter()
            .filter(|t| t.overdue_days(s.date.date) > 0)
            .count();
        let mut extra = format!(
            "{} due{}",
            s.tasks.len(),
            if live { "" } else { " tomorrow" }
        );
        if overdue > 0 {
            extra.push_str(&format!(" · {overdue} overdue"));
        }
        out.push(section("TASKS", &extra));
        if s.tasks.is_empty() && s.providers.iter().any(|p| p.freshness.is_ok()) {
            let msg = if live {
                "All clear. ✓"
            } else {
                "Nothing due tomorrow. ✓"
            };
            out.push(format!("    {}", msg.green()));
        }
        let shown: Vec<&Task> = s.tasks.iter().take(st.max_tasks).collect();
        out.extend(task_rows(&shown, s, w));
        if s.tasks.len() > st.max_tasks {
            out.push(format!(
                "    {}",
                format!(
                    "… and {} more (daypeek --json for all)",
                    s.tasks.len() - st.max_tasks
                )
                .dimmed()
            ));
        }
        out.extend(source_problems(&s.providers, now_utc, w));
    }

    // ── Weather ────────────────────────────────────────────────────────────
    if let Some(ws) = &s.weather {
        out.push(String::new());
        let place = ws
            .data
            .as_ref()
            .map(|x| x.place.clone())
            .unwrap_or_default();
        let mut extra = place;
        if let Some(n) = stale_note(&ws.freshness, now_utc) {
            extra.push_str(&format!(" · {n}"));
        }
        out.push(section("WEATHER", &extra));
        match &ws.data {
            Some(x) if live => {
                let (_, glyph) = describe(x.code);
                let temp = format!("{:.0}{}", x.temperature, x.unit);
                let range = format!("H {:.0}°  L {:.0}°", x.high, x.low);
                let mut line = format!(
                    "    {}  {}  {}  {}",
                    glyph,
                    temp.bold(),
                    x.condition,
                    range.dimmed()
                );
                let plain = 4
                    + glyph.width()
                    + 2
                    + temp.width()
                    + 2
                    + x.condition.width()
                    + 2
                    + range.width();
                if let Some(p) = x.precip_chance {
                    let rain = format!("{p}% rain");
                    if plain + 2 + rain.width() <= w {
                        line.push_str(&format!("  {}", rain.dimmed()));
                    }
                }
                out.push(line);
            }
            Some(x) => match x.day(date) {
                Some(f) => {
                    let (_, glyph) = describe(f.code);
                    let range = format!("H {:.0}{}  L {:.0}{}", f.high, x.unit, f.low, x.unit);
                    let mut line = format!("    {}  {}  {}", glyph, range.bold(), f.condition);
                    let plain = 4 + glyph.width() + 2 + range.width() + 2 + f.condition.width();
                    if let Some(p) = f.precip_chance {
                        let rain = format!("{p}% rain");
                        if plain + 2 + rain.width() <= w {
                            line.push_str(&format!("  {}", rain.dimmed()));
                        }
                    }
                    out.push(line);
                }
                None => out.push(format!(
                    "    {}",
                    "No forecast for tomorrow yet (try `daypeek tomorrow --refresh`).".dimmed()
                )),
            },
            None => {
                if let Freshness::Failed { error } = &ws.freshness {
                    out.push(format!("    {} {}", "!".red(), short(error).dimmed()));
                }
            }
        }
    }

    // ── Daily note ─────────────────────────────────────────────────────────
    for n in &s.notes {
        out.push(String::new());
        out.push(section("NOTE", &format!("{} · {}", n.title, n.source)));
        for l in &n.lines {
            out.push(format!("    {}", fit(l, w.saturating_sub(6)).italic()));
        }
    }
    out
}

/// Tasks that belong under `day` in the week view: today's block also holds
/// overdue and undated tasks.
fn tasks_on(s: &Snapshot, day: NaiveDate) -> Vec<&Task> {
    let today = s.date.date;
    s.tasks
        .iter()
        .filter(|t| match t.due {
            None => day == today,
            Some(d) => d == day || (day == today && d < today),
        })
        .collect()
}

fn day_forecast(x: &Weather, day: NaiveDate) -> Option<String> {
    let f = x.day(day)?;
    let (_, glyph) = describe(f.code);
    let mut s = format!("{glyph} {:.0}° / {:.0}°", f.high, f.low);
    if let Some(p) = f.precip_chance.filter(|p| *p >= 20) {
        s.push_str(&format!("  {p}% rain"));
    }
    Some(s)
}

/// The week view: seven day blocks.
fn week(s: &Snapshot, st: &Style, w: usize) -> Vec<String> {
    let now_utc = s.now.with_timezone(&Utc);
    let tz = s.now.timezone();
    let today = s.date.date;
    let left = format!(
        "This week · {} – {}",
        s.start.format("%b %-d"),
        s.end.format("%b %-d")
    );
    let right = format!("{} {}", clock(s.now, st.time_format), s.now.format("%Z"));
    let mut out = header(s, w, left, right);
    let weather = s.weather.as_ref().and_then(|ws| ws.data.as_ref());

    for d in s.view.days(today) {
        out.push(String::new());
        let mut title = d.format("%a %b %-d").to_string().to_uppercase();
        let tag = if d == today {
            "today"
        } else if d == today + Duration::days(1) {
            "tomorrow"
        } else {
            ""
        };
        let fc = weather.and_then(|x| day_forecast(x, d)).unwrap_or_default();
        let gap = w
            .saturating_sub(4 + title.width() + 2 + tag.width() + fc.width())
            .max(2);
        title = format!(
            "  {}  {}{}{}",
            title.bold(),
            tag.cyan(),
            " ".repeat(gap),
            fc.dimmed()
        );
        out.push(title);
        let events: Vec<&Event> = s.events.iter().filter(|e| overlaps_day(e, d, tz)).collect();
        let tasks = tasks_on(s, d);
        if events.is_empty() && tasks.is_empty() {
            out.push(format!("    {}", "nothing planned".dimmed()));
        }
        out.extend(event_rows(&events, d, d == today, s, st, w));
        let shown: Vec<&Task> = tasks.iter().take(st.max_tasks).copied().collect();
        out.extend(task_rows(&shown, s, w));
        if tasks.len() > st.max_tasks {
            out.push(format!(
                "    {}",
                format!("… and {} more", tasks.len() - st.max_tasks).dimmed()
            ));
        }
    }

    let mut problems = source_problems(&s.calendars, now_utc, w);
    problems.extend(source_problems(&s.providers, now_utc, w));
    if let Some(ws) = &s.weather {
        problems.extend(source_problems(
            &[SourceStatus {
                id: "weather".into(),
                name: ws.source.clone(),
                freshness: ws.freshness.clone(),
            }],
            now_utc,
            w,
        ));
    }
    if !problems.is_empty() {
        out.push(String::new());
        out.extend(problems);
    }
    out
}

/// Column widths for event rows, shrunk to fit narrow terminals: location
/// goes first, then the status column narrows; the title always keeps room.
#[derive(Debug, PartialEq)]
struct EventCols {
    title: usize,
    status: usize,
    loc: usize,
}

impl EventCols {
    /// "  ▶ " + a 10-wide time column + a space.
    const FIXED: usize = 15;
    const MIN_TITLE: usize = 16;

    fn new(w: usize, status_max: usize, any_loc: bool) -> Self {
        let usable = w.saturating_sub(Self::FIXED + 2);
        let gap = |n: usize| if n > 0 { n + 2 } else { 0 };
        let mut status = status_max.min(20);
        let mut loc = if any_loc { 16.min(w / 4) } else { 0 };
        if Self::MIN_TITLE + gap(status) + gap(loc) > usable {
            loc = 0;
        }
        if Self::MIN_TITLE + gap(status) > usable {
            status = usable.saturating_sub(Self::MIN_TITLE + 2);
            if status < 6 {
                status = 0;
            }
        }
        let title = usable.saturating_sub(gap(status) + gap(loc)).max(8);
        EventCols { title, status, loc }
    }
}

/// (event, time label, status, past, current)
type EventRow<'a, S> = (&'a Event, String, S, bool, bool);

fn event_rows(
    events: &[&Event],
    day: NaiveDate,
    live: bool,
    s: &Snapshot,
    st: &Style,
    w: usize,
) -> Vec<String> {
    let now = s.now.with_timezone(&Utc);
    let tz = s.now.timezone();
    let next = events
        .iter()
        .filter(|e| live && !e.all_day && e.start > now)
        .map(|e| e.start)
        .min();
    let rows: Vec<EventRow<(String, String)>> = events
        .iter()
        .map(|e| {
            let when = if e.all_day {
                "all day".to_string()
            } else {
                let start = e.start.with_timezone(&tz);
                // Started the day before? Show the time it ends instead.
                if start.date_naive() < day {
                    format!("→ {}", clock(e.end.with_timezone(&tz), st.time_format))
                } else {
                    clock(start, st.time_format)
                }
            };
            let past = live
                && !e.all_day
                && if e.end > e.start {
                    e.end <= now
                } else {
                    e.start < now
                };
            let current = live && !e.all_day && e.start <= now && now < e.end;
            // (roomy, narrow) wording
            let status = if current {
                (
                    format!("now · ends in {}", span(e.end - now)),
                    format!("now · {} left", span(e.end - now)),
                )
            } else if Some(e.start) == next {
                let s = format!("in {}", span(e.start - now));
                (s.clone(), s)
            } else {
                (String::new(), String::new())
            };
            (*e, when, status, past, current)
        })
        .collect();
    let status_max = rows.iter().map(|r| r.2 .0.width()).max().unwrap_or(0);
    let any_loc = rows
        .iter()
        .any(|r| r.0.location.as_deref().is_some_and(|l| !l.is_empty()));
    let mut cols = EventCols::new(w, status_max, any_loc);
    let narrow = cols.status < status_max;
    if narrow {
        let short_max = rows.iter().map(|r| r.2 .1.width()).max().unwrap_or(0);
        cols = EventCols::new(w, short_max, any_loc);
    }
    let rows: Vec<EventRow<String>> = rows
        .into_iter()
        .map(|(e, when, (long, short), past, current)| {
            (e, when, if narrow { short } else { long }, past, current)
        })
        .collect();

    rows.iter()
        .map(|(e, when, status, past, current)| {
            let marker = if *current {
                "▶".green().to_string()
            } else {
                " ".into()
            };
            let time_col = pad(when, 10);
            let title_fit = fit(&e.title, cols.title);
            let title_pad = " ".repeat(cols.title.saturating_sub(title_fit.width()));
            let status_col = if cols.status > 0 {
                format!("  {}", pad(&fit(status, cols.status), cols.status))
            } else {
                String::new()
            };
            let loc_col = match (&e.location, cols.loc) {
                (Some(l), n) if n > 0 && !l.is_empty() => format!("  {}", fit(l, n)),
                _ => String::new(),
            };
            let loc_col = loc_col.dimmed();
            let line = if *past {
                format!(
                    "  {} {} {}{}{}{}",
                    marker,
                    time_col.dimmed(),
                    title_fit.dimmed().strikethrough(),
                    title_pad,
                    status_col.dimmed(),
                    loc_col
                )
            } else if *current {
                format!(
                    "  {} {} {}{}{}{}",
                    marker,
                    time_col.green().bold(),
                    title_fit.bold(),
                    title_pad,
                    status_col.green(),
                    loc_col
                )
            } else {
                format!(
                    "  {} {} {}{}{}{}",
                    marker,
                    time_col.cyan(),
                    title_fit,
                    title_pad,
                    status_col.cyan(),
                    loc_col
                )
            };
            line.trim_end().to_string()
        })
        .collect()
}

/// Column widths for task rows: the source badge goes first when narrow.
#[derive(Debug, PartialEq)]
struct TaskCols {
    title: usize,
    due: usize,
    badge: usize,
}

impl TaskCols {
    /// "    ● p1 ".
    const FIXED: usize = 9;
    const MIN_TITLE: usize = 12;

    fn new(w: usize, due_max: usize, badge_max: usize) -> Self {
        let usable = w.saturating_sub(Self::FIXED + 2);
        let gap = |n: usize| if n > 0 { n + 1 } else { 0 };
        let due = due_max;
        let mut badge = badge_max.min(18);
        if Self::MIN_TITLE + gap(due) + gap(badge) > usable {
            badge = 0;
        }
        let title = usable.saturating_sub(gap(due) + gap(badge)).max(8);
        TaskCols { title, due, badge }
    }
}

fn due_label(t: &Task, today: NaiveDate) -> String {
    match t.overdue_days(today) {
        0 => String::new(),
        1 => "yesterday".into(),
        n => format!("{n}d overdue"),
    }
}

fn task_rows(tasks: &[&Task], s: &Snapshot, w: usize) -> Vec<String> {
    let today = s.date.date;
    let due_max = tasks
        .iter()
        .map(|t| due_label(t, today).width())
        .max()
        .unwrap_or(0);
    let badge_max = tasks
        .iter()
        .map(|t| t.sources.join("+").width())
        .max()
        .unwrap_or(0);
    let cols = TaskCols::new(w, due_max, badge_max);
    tasks
        .iter()
        .map(|t| {
            let dot = match t.priority {
                Priority::None => "○".dimmed(),
                p => priority_paint(p, "●"),
            };
            let label = priority_paint(t.priority, t.priority.label());
            let title = pad(&fit(&t.title, cols.title), cols.title);
            let title = if t.priority >= Priority::High {
                priority_paint(t.priority, &title)
            } else {
                title.normal()
            };
            let mut line = format!("    {} {} {}", dot, label, title);
            if cols.due > 0 {
                line.push_str(&format!(" {}", pad(&due_label(t, today), cols.due).red()));
            }
            if cols.badge > 0 {
                line.push_str(&format!(
                    " {}",
                    fit(&t.sources.join("+"), cols.badge).dimmed()
                ));
            }
            line.trim_end().to_string()
        })
        .collect()
}

/// The compact suffix describing sources that aren't working: `↻2` stale,
/// `!1` failed, and sources that still need setup (instead of hiding them).
fn compact_problems(s: &Snapshot) -> String {
    let all = || s.providers.iter().chain(&s.calendars);
    let stale = all()
        .filter(|p| matches!(p.freshness, Freshness::Stale { .. }))
        .count();
    let failed = all()
        .filter(|p| {
            matches!(&p.freshness, Freshness::Failed { error } if !error.starts_with("unsupported"))
                && !is_auth_missing(&p.freshness)
        })
        .count();
    let setup: Vec<&str> = all()
        .filter(|p| {
            matches!(p.freshness, Freshness::Failed { .. }) && is_auth_missing(&p.freshness)
        })
        .map(|p| p.id.as_str())
        .collect();
    let mut parts = Vec::new();
    if stale > 0 {
        parts.push(format!("↻{stale}").yellow().to_string());
    }
    if failed > 0 {
        parts.push(format!("!{failed}").red().to_string());
    }
    match setup.as_slice() {
        [] => {}
        [one] => parts.push(format!("○ {one} needs setup").yellow().to_string()),
        many => parts.push(format!("○ {} need setup", many.len()).yellow().to_string()),
    }
    parts.join(" ")
}

/// The short `--compact` view for shell startup (≤ 3 lines; one line per
/// day for the week).
pub fn compact(s: &Snapshot, st: &Style) -> String {
    let mut lines = match s.view {
        View::Week => compact_week(s, st),
        View::Today | View::Tomorrow => compact_day(s, st),
    };
    if s.calendars.is_empty() && s.providers.is_empty() && s.weather.is_none() {
        lines.push(format!("{} {}", "▸".cyan(), SETUP_HINT.dimmed()));
    }
    lines.join("\n")
}

/// Title budget for compact lines on this terminal width.
fn compact_title_w(st: &Style, max: usize) -> usize {
    (st.width.saturating_sub(16) / 2).clamp(12, max)
}

fn compact_day(s: &Snapshot, st: &Style) -> Vec<String> {
    let now_utc = s.now.with_timezone(&Utc);
    let live = s.view == View::Today;
    let tz = s.now.timezone();
    let title_w = compact_title_w(st, 32);
    let mut first = if live {
        format!(
            "{} {}",
            s.now.format("%a %b %-d").to_string().bold(),
            clock(s.now, st.time_format).dimmed()
        )
    } else {
        format!("Tomorrow · {}", s.start.format("%a %b %-d"))
            .bold()
            .to_string()
    };
    if let Some(x) = s.weather.as_ref().and_then(|w| w.data.as_ref()) {
        if live {
            let (_, glyph) = describe(x.code);
            first.push_str(&format!(
                "  {} {:.0}{} {}",
                glyph,
                x.temperature,
                x.unit,
                x.condition.to_lowercase().dimmed()
            ));
        } else if let Some(f) = x.day(s.start) {
            let (_, glyph) = describe(f.code);
            first.push_str(&format!(
                "  {} {:.0}°/{:.0}° {}",
                glyph,
                f.high,
                f.low,
                f.condition.to_lowercase().dimmed()
            ));
        }
    }
    let mut lines = vec![first];
    let at = |e: &Event| clock(e.start.with_timezone(&tz), st.time_format);

    if !s.calendars.is_empty() {
        let line = if live {
            let upcoming: Vec<&Event> = s
                .events
                .iter()
                .filter(|e| !e.all_day && e.end > now_utc)
                .collect();
            let mut parts: Vec<String> = Vec::new();
            let mut rest = upcoming.iter().peekable();
            if let Some(e) = rest.peek().filter(|e| e.start <= now_utc) {
                parts.push(format!(
                    "{} {}",
                    "now".green().bold(),
                    fit(&e.title, title_w)
                ));
                rest.next();
            }
            if let Some(e) = rest.next() {
                parts.push(format!(
                    "{} {}",
                    format!("{} (in {})", at(e), span(e.start - now_utc)).cyan(),
                    fit(&e.title, title_w)
                ));
            }
            let more = rest.count();
            if parts.is_empty() {
                format!("{} {}", "▸".cyan(), "no more events today".dimmed())
            } else {
                let mut l = format!("{} {}", "▸".cyan(), parts.join(" · "));
                if more > 0 {
                    l.push_str(&format!(" {}", format!("+{more} more").dimmed()));
                }
                l
            }
        } else {
            let mut parts: Vec<String> = s
                .events
                .iter()
                .take(2)
                .map(|e| {
                    let when = if e.all_day { "all day".into() } else { at(e) };
                    format!("{} {}", when.cyan(), fit(&e.title, title_w))
                })
                .collect();
            let more = s.events.len().saturating_sub(parts.len());
            if parts.is_empty() {
                parts.push("no events tomorrow".dimmed().to_string());
            }
            let mut l = format!("{} {}", "▸".cyan(), parts.join(" · "));
            if more > 0 {
                l.push_str(&format!(" {}", format!("+{more} more").dimmed()));
            }
            l
        };
        lines.push(line);
    }

    let problems = compact_problems(s);
    // Only claim "0 due" when at least one task source actually answered.
    let any_answered = s
        .providers
        .iter()
        .any(|p| p.freshness.is_ok() || matches!(p.freshness, Freshness::Stale { .. }));
    if !s.providers.is_empty() && (any_answered || !s.tasks.is_empty()) {
        let overdue = s
            .tasks
            .iter()
            .filter(|t| t.overdue_days(s.date.date) > 0)
            .count();
        let mut line = format!(
            "{} {} due{}",
            "✓".green(),
            s.tasks.len(),
            if live { "" } else { " tomorrow" }
        );
        if overdue > 0 {
            line.push_str(&format!(" ({})", format!("{overdue} overdue").red()));
        }
        if let Some(top) = s.tasks.first() {
            line.push_str(&format!(
                " · {} {}",
                priority_paint(top.priority, top.priority.label().trim()),
                fit(&top.title, compact_title_w(st, 40))
            ));
        }
        if !problems.is_empty() {
            line.push_str(&format!(" {problems}"));
        }
        lines.push(line);
    } else if !problems.is_empty() {
        lines.push(problems);
    }
    lines
}

fn compact_week(s: &Snapshot, st: &Style) -> Vec<String> {
    let tz = s.now.timezone();
    let today = s.date.date;
    let title_w = compact_title_w(st, 28);
    let weather = s.weather.as_ref().and_then(|w| w.data.as_ref());
    let mut head = format!(
        "This week · {} – {}",
        s.start.format("%b %-d"),
        s.end.format("%b %-d")
    )
    .bold()
    .to_string();
    let problems = compact_problems(s);
    if !problems.is_empty() {
        head.push_str(&format!("  {problems}"));
    }
    let mut lines = vec![head];
    for d in s.view.days(today) {
        let mut line = format!("{}", d.format("%a %-d").to_string().bold());
        if d.format("%-d").to_string().len() == 1 {
            line.push(' ');
        }
        if let Some(f) = weather.and_then(|x| x.day(d)) {
            let (_, glyph) = describe(f.code);
            line.push_str(&format!("  {} {:>3.0}°", glyph, f.high));
        }
        if !s.calendars.is_empty() {
            let events: Vec<&Event> = s.events.iter().filter(|e| overlaps_day(e, d, tz)).collect();
            // Today: the next thing still to come, not one that's over.
            let now = s.now.with_timezone(&Utc);
            let pick = events
                .iter()
                .find(|e| !e.all_day && (d != today || e.end > now))
                .or(events.first());
            match pick {
                Some(e) => {
                    let when = if e.all_day {
                        "all day".to_string()
                    } else {
                        clock(e.start.with_timezone(&tz), st.time_format)
                    };
                    line.push_str(&format!(
                        "  {} {} {}",
                        "▸".cyan(),
                        when.cyan(),
                        fit(&e.title, title_w)
                    ));
                    if events.len() > 1 {
                        line.push_str(&format!(" {}", format!("+{}", events.len() - 1).dimmed()));
                    }
                }
                None => line.push_str(&format!("  {} {}", "▸".cyan(), "free".dimmed())),
            }
        }
        if !s.providers.is_empty() {
            let tasks = tasks_on(s, d);
            if !tasks.is_empty() {
                line.push_str(&format!("  {} {}", "✓".green(), tasks.len()));
                let overdue = tasks.iter().filter(|t| t.overdue_days(today) > 0).count();
                if overdue > 0 {
                    line.push_str(&format!(" ({})", format!("{overdue} overdue").red()));
                }
            }
        }
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Priority;

    #[test]
    fn priority_colors_are_distinct_and_ordered() {
        let colors: Vec<_> = [
            Priority::Urgent,
            Priority::High,
            Priority::Medium,
            Priority::Low,
        ]
        .iter()
        .map(|p| p.rgb().unwrap())
        .collect();
        let mut dedup = colors.clone();
        dedup.dedup();
        assert_eq!(dedup.len(), 4);
        assert_eq!(Priority::None.rgb(), None);
        assert_eq!(Priority::Urgent.rgb(), Some((235, 87, 87)), "p1 is red");
        assert!(Priority::Urgent > Priority::High && Priority::High > Priority::Medium);
    }

    #[test]
    fn priority_paint_uses_truecolor() {
        std::env::set_var("COLORTERM", "truecolor");
        colored::control::set_override(true);
        let s = priority_paint(Priority::Urgent, "x").to_string();
        assert!(s.contains("38;2;235;87;87"), "{s:?}");
        assert_eq!(priority_paint(Priority::None, "x").to_string(), "x");
        colored::control::unset_override();
    }

    #[test]
    fn columns_shrink_location_first_then_status() {
        let roomy = EventCols::new(80, 20, true);
        assert!(
            roomy.loc > 0 && roomy.status == 20 && roomy.title >= 20,
            "{roomy:?}"
        );
        let narrow = EventCols::new(50, 20, true);
        assert_eq!(narrow.loc, 0, "location goes first");
        assert!(narrow.title >= EventCols::MIN_TITLE, "{narrow:?}");
        assert!(EventCols::FIXED + narrow.title + narrow.status + 2 <= 50 - 2);
        let tiny = EventCols::new(40, 20, true);
        assert!(tiny.title >= 8, "{tiny:?}");
        let tasks = TaskCols::new(50, 10, 17);
        assert_eq!(tasks.badge, 0, "badge goes first");
        assert!(tasks.title >= TaskCols::MIN_TITLE);
    }

    #[test]
    fn spans_and_fit() {
        assert_eq!(span(Duration::minutes(5)), "5m");
        assert_eq!(span(Duration::minutes(185)), "3h 05m");
        assert_eq!(span(Duration::days(2)), "2d");
        assert_eq!(fit("hello world", 8), "hello w…");
        assert_eq!(fit("short", 8), "short");
    }
}

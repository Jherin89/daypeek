//! Rendering a [`Snapshot`]: the full dashboard and the `--compact` view.

use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use colored::{ColoredString, Colorize};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::config::TimeFormat;
use crate::model::{Event, Freshness, Priority, Task};
use crate::snapshot::{is_auth_missing, Snapshot, SourceStatus};
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
fn source_problems(sources: &[SourceStatus], now: DateTime<Utc>) -> Vec<String> {
    sources
        .iter()
        .filter_map(|s| match &s.freshness {
            Freshness::Live => None,
            Freshness::Stale { fetched_at, error } => Some(format!(
                "    {} {} {}",
                "↻".yellow(),
                s.id.yellow(),
                format!(
                    "offline, showing cache from {} ({})",
                    ago(*fetched_at, now),
                    short(error)
                )
                .dimmed()
            )),
            Freshness::Failed { error } => {
                let mark = if is_auth_missing(&s.freshness) || error.starts_with("unsupported") {
                    "○".dimmed()
                } else {
                    "!".red()
                };
                Some(format!("    {} {} {}", mark, s.id, short(error).dimmed()))
            }
        })
        .collect()
}

fn short(e: &str) -> String {
    fit(e, 70)
}

/// The full dashboard.
pub fn full(s: &Snapshot, st: &Style) -> String {
    let w = st.width.clamp(40, 110);
    let now_utc = s.now.with_timezone(&Utc);
    let mut out: Vec<String> = Vec::new();

    // ── Date ───────────────────────────────────────────────────────────────
    let left = s.now.format("%A, %B %-d").to_string();
    let right = format!("{} {}", clock(s.now, st.time_format), s.now.format("%Z"));
    let gap = w.saturating_sub(4 + left.width() + right.width()).max(2);
    out.push(String::new());
    out.push(format!(
        "  {}{}{}",
        left.bold(),
        " ".repeat(gap),
        right.bold()
    ));
    let left_days = s.date.days_in_year - s.date.day_of_year;
    let sub = format!(
        "{} · week {} · day {} · {} days left",
        s.now.format("%Y"),
        s.date.iso_week,
        s.date.day_of_year,
        left_days
    );
    let demo = if s.demo { "demo data" } else { "" };
    let gap = w.saturating_sub(4 + sub.width() + demo.width()).max(2);
    out.push(format!(
        "  {}{}{}",
        sub.dimmed(),
        " ".repeat(gap),
        demo.magenta()
    ));
    out.push(format!("  {}", "─".repeat(w.saturating_sub(4)).dimmed()));

    // ── Calendar ───────────────────────────────────────────────────────────
    if !s.calendars.is_empty() {
        out.push(String::new());
        let count = s.events.len();
        let mut extra = format!("{count} event{}", if count == 1 { "" } else { "s" });
        if let Some(n) = s
            .calendars
            .iter()
            .find_map(|c| stale_note(&c.freshness, now_utc))
        {
            extra.push_str(&format!(" · {n}"));
        }
        out.push(section("CALENDAR", &extra));
        if s.events.is_empty() {
            out.push(format!(
                "    {}",
                "Nothing scheduled. Enjoy the open day.".dimmed()
            ));
        }
        let next = s
            .events
            .iter()
            .filter(|e| !e.all_day && e.start > now_utc)
            .map(|e| e.start)
            .min();
        for e in &s.events {
            out.push(event_line(e, s, st, w, next));
        }
        out.extend(source_problems(&s.calendars, now_utc));
    }

    // ── Tasks ──────────────────────────────────────────────────────────────
    if !s.providers.is_empty() {
        out.push(String::new());
        let overdue = s
            .tasks
            .iter()
            .filter(|t| t.overdue_days(s.date.date) > 0)
            .count();
        let mut extra = format!("{} due", s.tasks.len());
        if overdue > 0 {
            extra.push_str(&format!(" · {overdue} overdue"));
        }
        out.push(section("TASKS", &extra));
        if s.tasks.is_empty() && s.providers.iter().any(|p| p.freshness.is_live()) {
            out.push(format!("    {}", "All clear. ✓".green()));
        }
        for t in s.tasks.iter().take(st.max_tasks) {
            out.push(task_line(t, s, w));
        }
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
        out.extend(source_problems(&s.providers, now_utc));
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
            Some(x) => {
                let (_, glyph) = describe(x.code);
                let mut line = format!(
                    "    {}  {}  {}  {}",
                    glyph,
                    format!("{:.0}{}", x.temperature, x.unit).bold(),
                    x.condition,
                    format!("H {:.0}°  L {:.0}°", x.high, x.low).dimmed()
                );
                if let Some(p) = x.precip_chance {
                    line.push_str(&format!("  {}", format!("{p}% rain").dimmed()));
                }
                out.push(line);
            }
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

    if s.calendars.is_empty() && s.providers.is_empty() && s.weather.is_none() {
        out.push(String::new());
        out.push(format!(
            "    {}",
            "No sources configured yet. Run `daypeek --init`, or try `daypeek --demo`.".dimmed()
        ));
    }
    out.push(String::new());
    out.join("\n")
}

fn event_line(
    e: &Event,
    s: &Snapshot,
    st: &Style,
    w: usize,
    next: Option<DateTime<Utc>>,
) -> String {
    let now = s.now.with_timezone(&Utc);
    let tz = s.now.timezone();
    let when = if e.all_day {
        "all day".to_string()
    } else {
        let start = e.start.with_timezone(&tz);
        // Started yesterday? Show the time it ends instead.
        if start.date_naive() < s.date.date {
            format!("→ {}", clock(e.end.with_timezone(&tz), st.time_format))
        } else {
            clock(start, st.time_format)
        }
    };
    let past = !e.all_day
        && if e.end > e.start {
            e.end <= now
        } else {
            e.start < now
        };
    let current = !e.all_day && e.start <= now && now < e.end;
    let status = if current {
        format!("now · ends in {}", span(e.end - now))
    } else if Some(e.start) == next {
        format!("in {}", span(e.start - now))
    } else {
        String::new()
    };
    let loc = e.location.as_deref().unwrap_or("");
    let right_w = 18usize.min(w / 4);
    let title_w = w.saturating_sub(4 + 2 + 10 + right_w + 2 + 19 + 2);
    let marker = if current {
        "▶".green().to_string()
    } else {
        " ".into()
    };
    let time_col = pad(&when, 10);
    let title = pad(&fit(&e.title, title_w), title_w);
    let status_col = pad(&fit(&status, 18), 19);
    let loc_col = fit(loc, right_w);
    if past {
        format!(
            "  {} {} {}  {}{}",
            marker,
            time_col.dimmed(),
            title.dimmed().strikethrough(),
            status_col.dimmed(),
            loc_col.dimmed()
        )
    } else if current {
        format!(
            "  {} {} {}  {}{}",
            marker,
            time_col.green().bold(),
            title.bold(),
            status_col.green(),
            loc_col.dimmed()
        )
    } else {
        let status_c = if status.is_empty() {
            status_col.normal()
        } else {
            status_col.cyan()
        };
        format!(
            "  {} {} {}  {}{}",
            marker,
            time_col.cyan(),
            title,
            status_c,
            loc_col.dimmed()
        )
    }
}

fn task_line(t: &Task, s: &Snapshot, w: usize) -> String {
    let overdue = t.overdue_days(s.date.date);
    let badge = t.sources.join("+");
    let due = match overdue {
        0 => String::new(),
        1 => "yesterday".into(),
        n => format!("{n}d overdue"),
    };
    let title_w = w.saturating_sub(4 + 2 + 3 + 12 + 18 + 2);
    let dot = match t.priority {
        Priority::None => "○".dimmed(),
        p => priority_paint(p, "●"),
    };
    let label = priority_paint(t.priority, t.priority.label());
    let title = pad(&fit(&t.title, title_w), title_w);
    let title = if t.priority >= Priority::High {
        priority_paint(t.priority, &title)
    } else {
        title.normal()
    };
    format!(
        "    {} {} {} {} {}",
        dot,
        label,
        title,
        pad(&due, 11).red(),
        fit(&badge, 18).dimmed()
    )
}

/// The short `--compact` view for shell startup (≤ 3 lines).
pub fn compact(s: &Snapshot, st: &Style) -> String {
    let now_utc = s.now.with_timezone(&Utc);
    let mut first = format!(
        "{} {}",
        s.now.format("%a %b %-d").to_string().bold(),
        clock(s.now, st.time_format).dimmed()
    );
    if let Some(x) = s.weather.as_ref().and_then(|w| w.data.as_ref()) {
        let (_, glyph) = describe(x.code);
        first.push_str(&format!(
            "  {} {:.0}{} {}",
            glyph,
            x.temperature,
            x.unit,
            x.condition.to_lowercase().dimmed()
        ));
    }
    let mut lines = vec![first];

    if !s.calendars.is_empty() {
        let upcoming: Vec<&Event> = s
            .events
            .iter()
            .filter(|e| !e.all_day && e.end > now_utc)
            .collect();
        let tz = s.now.timezone();
        let at = |e: &Event| clock(e.start.with_timezone(&tz), st.time_format);
        let mut parts: Vec<String> = Vec::new();
        let mut rest = upcoming.iter().peekable();
        if let Some(e) = rest.peek().filter(|e| e.start <= now_utc) {
            parts.push(format!("{} {}", "now".green().bold(), fit(&e.title, 32)));
            rest.next();
        }
        if let Some(e) = rest.next() {
            parts.push(
                format!(
                    "{} {} {}",
                    format!("{} (in {})", at(e), span(e.start - now_utc)).cyan(),
                    fit(&e.title, 32),
                    ""
                )
                .trim_end()
                .to_string(),
            );
        }
        let more = rest.count();
        let line = if parts.is_empty() {
            format!("{} {}", "▸".cyan(), "no more events today".dimmed())
        } else {
            let mut l = format!("{} {}", "▸".cyan(), parts.join(" · "));
            if more > 0 {
                l.push_str(&format!(" {}", format!("+{more} more").dimmed()));
            }
            l
        };
        lines.push(line);
    }

    if !s.providers.is_empty() {
        let overdue = s
            .tasks
            .iter()
            .filter(|t| t.overdue_days(s.date.date) > 0)
            .count();
        let mut line = format!("{} {} due", "✓".green(), s.tasks.len());
        if overdue > 0 {
            line.push_str(&format!(" ({})", format!("{overdue} overdue").red()));
        }
        if let Some(top) = s.tasks.first() {
            line.push_str(&format!(
                " · {} {}",
                priority_paint(top.priority, top.priority.label().trim()),
                fit(&top.title, 40)
            ));
        }
        let stale = s
            .providers
            .iter()
            .chain(&s.calendars)
            .filter(|p| !p.freshness.is_live() && !is_auth_missing(&p.freshness))
            .count();
        if stale > 0 {
            line.push_str(&format!(" {}", format!("↻{stale}").yellow()));
        }
        lines.push(line);
    }
    lines.join("\n")
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
    fn spans_and_fit() {
        assert_eq!(span(Duration::minutes(5)), "5m");
        assert_eq!(span(Duration::minutes(185)), "3h 05m");
        assert_eq!(span(Duration::days(2)), "2d");
        assert_eq!(fit("hello world", 8), "hello w…");
        assert_eq!(fit("short", 8), "short");
    }
}

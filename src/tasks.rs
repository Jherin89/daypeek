//! Merging tasks from many providers into one list.

use std::collections::HashMap;

use chrono::NaiveDate;

use crate::model::Task;

/// Lowercase, strip punctuation and markdown, collapse whitespace.
pub fn normalize_title(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut space = false;
    for c in title.chars() {
        if c.is_alphanumeric() {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.extend(c.to_lowercase());
        } else {
            space = true;
        }
    }
    out
}

/// Keep tasks that are due today or earlier (undated tasks pass through: a
/// provider only emits them when configured to).
pub fn relevant(task: &Task, today: NaiveDate) -> bool {
    in_window(task, today, today, today)
}

/// Does `task` belong in a view of `start..=end`? Views that include today
/// also show overdue and undated tasks; a view starting later (tomorrow)
/// shows only what is due inside it.
pub fn in_window(task: &Task, today: NaiveDate, start: NaiveDate, end: NaiveDate) -> bool {
    let from_today = start <= today;
    match task.due {
        None => from_today,
        Some(d) => d <= end && (from_today || d >= start),
    }
}

/// Merge and de-duplicate by (normalized title, due date). Duplicates combine
/// their source badges and keep the highest priority.
pub fn merge(lists: Vec<Vec<Task>>, today: NaiveDate) -> Vec<Task> {
    merge_window(lists, today, today, today)
}

/// [`merge`] for a view covering `start..=end` (see [`in_window`]).
pub fn merge_window(
    lists: Vec<Vec<Task>>,
    today: NaiveDate,
    start: NaiveDate,
    end: NaiveDate,
) -> Vec<Task> {
    let mut index: HashMap<(String, Option<NaiveDate>), usize> = HashMap::new();
    let mut out: Vec<Task> = Vec::new();
    for task in lists.into_iter().flatten() {
        if !in_window(&task, today, start, end) || task.title.trim().is_empty() {
            continue;
        }
        let key = (normalize_title(&task.title), task.due);
        match index.get(&key) {
            Some(&i) => {
                let existing = &mut out[i];
                existing.priority = existing.priority.max(task.priority);
                for s in task.sources {
                    if !existing.sources.contains(&s) {
                        existing.sources.push(s);
                    }
                }
                if existing.context.is_none() {
                    existing.context = task.context;
                }
                for r in task.refs {
                    if !existing.refs.contains(&r) {
                        existing.refs.push(r);
                    }
                }
            }
            None => {
                index.insert(key, out.len());
                out.push(task);
            }
        }
    }
    sort(&mut out);
    out
}

/// Highest priority first, then oldest due date, then title.
pub fn sort(tasks: &mut [Task]) {
    tasks.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| match (a.due, b.due) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Priority;

    fn d(day: u32) -> Option<NaiveDate> {
        NaiveDate::from_ymd_opt(2026, 10, day)
    }

    fn t(title: &str, due: Option<NaiveDate>, p: Priority, src: &str) -> Task {
        let mut t = Task::new(title, due, p);
        t.sources.push(src.into());
        t
    }

    #[test]
    fn windows_for_tomorrow_and_week() {
        let today = d(21).unwrap();
        let tomorrow = d(22).unwrap();
        let week_end = d(27).unwrap();
        let overdue = t("Old", d(19), Priority::None, "x");
        let undated = t("Someday", None, Priority::None, "x");
        let due_tomorrow = t("Dentist forms", d(22), Priority::None, "x");
        let due_sat = t("Pay bill", d(24), Priority::None, "x");
        let next_month = t("Later", d(30), Priority::None, "x");
        // Today: overdue + today + undated, nothing later.
        assert!(in_window(&overdue, today, today, today));
        assert!(in_window(&undated, today, today, today));
        assert!(!in_window(&due_tomorrow, today, today, today));
        // Tomorrow: only what is due tomorrow.
        assert!(in_window(&due_tomorrow, today, tomorrow, tomorrow));
        assert!(!in_window(&overdue, today, tomorrow, tomorrow));
        assert!(!in_window(&undated, today, tomorrow, tomorrow));
        assert!(!in_window(&due_sat, today, tomorrow, tomorrow));
        // Week: overdue, undated, and everything through day 7.
        for task in [&overdue, &undated, &due_tomorrow, &due_sat] {
            assert!(in_window(task, today, today, week_end), "{}", task.title);
        }
        assert!(!in_window(&next_month, today, today, week_end));
    }

    #[test]
    fn normalizes_titles() {
        assert_eq!(normalize_title("  Call **Mom**!  "), "call mom");
        assert_eq!(normalize_title("Pay rent — Oct."), "pay rent oct");
        assert_eq!(normalize_title("Café run"), "café run");
    }

    #[test]
    fn dedups_across_providers() {
        let today = d(21).unwrap();
        let merged = merge(
            vec![
                vec![t("Call mom", d(21), Priority::Medium, "todoist")],
                vec![t("call Mom!", d(21), Priority::Urgent, "obsidian")],
                vec![t("Call mom", d(20), Priority::None, "todotxt")], // different due → kept
            ],
            today,
        );
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].priority, Priority::Urgent);
        assert_eq!(merged[0].sources, vec!["todoist", "obsidian"]);
    }

    #[test]
    fn drops_future_and_sorts() {
        let today = d(21).unwrap();
        let merged = merge(
            vec![vec![
                t("future", d(25), Priority::Urgent, "a"),
                t("undated", None, Priority::None, "a"),
                t("old", d(1), Priority::None, "a"),
                t("hot", d(21), Priority::High, "a"),
            ]],
            today,
        );
        let titles: Vec<_> = merged.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, vec!["hot", "old", "undated"]);
    }
}

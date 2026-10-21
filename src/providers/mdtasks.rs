//! Markdown checkbox parsing shared by the Obsidian and Markdown providers.
//!
//! Understands the common due-date conventions:
//!
//! | Syntax | Origin |
//! | --- | --- |
//! | `📅 2026-10-21` | Obsidian Tasks plugin (due) |
//! | `⏳ 2026-10-21` | Obsidian Tasks plugin (scheduled; used if no due) |
//! | `[due:: 2026-10-21]` / `due:: 2026-10-21` | Dataview inline field |
//! | `due:2026-10-21` | todo.txt style |
//! | `@due(2026-10-21)` | TaskPaper style |
//!
//! Priority signifiers `🔺 ⏫ 🔼 🔽 ⏬` (Tasks plugin) and a leading `!!!`/`!!`/`!`
//! are mapped onto [`Priority`].

use chrono::NaiveDate;

use crate::model::{Priority, Task};

/// Tasks-plugin signifiers that start the metadata tail of a line.
const SIGNIFIERS: &[&str] = &[
    "📅", "⏳", "🛫", "➕", "✅", "❌", "🔁", "🔺", "⏫", "🔼", "🔽", "⏬", "🆔", "⛔", "🏁",
];

/// A parsed open checkbox line.
#[derive(Debug, Clone, PartialEq)]
pub struct MdTask {
    pub title: String,
    pub due: Option<NaiveDate>,
    pub priority: Priority,
}

/// Parse one line. Returns `None` unless it is an *open* checkbox item.
pub fn parse_line(line: &str) -> Option<MdTask> {
    let s = line.trim_start();
    // Bullet: -, *, + or "1." / "1)".
    let rest = if let Some(r) = s.strip_prefix(['-', '*', '+']) {
        r
    } else {
        let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        s[digits..].strip_prefix(['.', ')'])?
    };
    let rest = rest.strip_prefix(' ')?.trim_start();
    let rest = rest.strip_prefix('[')?;
    let mut chars = rest.chars();
    let mark = chars.next()?;
    let rest = chars.as_str().strip_prefix(']')?;
    // ' ' open, '/' in progress, '>' / '<' deferred (still open); x X - done/cancelled.
    if !matches!(mark, ' ' | '/' | '>' | '<' | '!') {
        return None;
    }
    let body = rest.trim();

    let due = emoji_date(body, "📅")
        .or_else(|| inline_field(body, "due::"))
        .or_else(|| inline_field(body, "due:"))
        .or_else(|| paren_field(body, "@due("))
        .or_else(|| emoji_date(body, "⏳"));

    let mut priority = if body.contains('🔺') {
        Priority::Urgent
    } else if body.contains('⏫') {
        Priority::High
    } else if body.contains('🔼') {
        Priority::Medium
    } else if body.contains('🔽') || body.contains('⏬') {
        Priority::Low
    } else {
        Priority::None
    };

    let mut title = strip_metadata(body);
    for (bang, p) in [
        ("!!! ", Priority::Urgent),
        ("!! ", Priority::High),
        ("! ", Priority::Medium),
    ] {
        if let Some(t) = title.strip_prefix(bang) {
            priority = priority.max(p);
            title = t.to_string();
            break;
        }
    }
    let title = title.trim().to_string();
    if title.is_empty() {
        return None;
    }
    Some(MdTask {
        title,
        due,
        priority,
    })
}

fn date_at(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim_start().get(..10)?, "%Y-%m-%d").ok()
}

fn emoji_date(body: &str, emoji: &str) -> Option<NaiveDate> {
    let i = body.find(emoji)?;
    date_at(&body[i + emoji.len()..])
}

fn inline_field(body: &str, key: &str) -> Option<NaiveDate> {
    let lower = body.to_lowercase();
    let mut from = 0;
    while let Some(off) = lower[from..].find(key) {
        let i = from + off;
        let at_boundary = i == 0 || !lower[..i].ends_with(|c: char| c.is_alphanumeric());
        // `due:` must not match the first half of `due::`.
        let after = &body[i + key.len()..];
        if at_boundary && !(key == "due:" && after.starts_with(':')) {
            if let Some(d) = date_at(after) {
                return Some(d);
            }
        }
        from = i + key.len();
    }
    None
}

fn paren_field(body: &str, open: &str) -> Option<NaiveDate> {
    let i = body.find(open)?;
    date_at(&body[i + open.len()..])
}

/// Remove the Tasks-plugin tail, inline fields, and block ids from a title.
fn strip_metadata(body: &str) -> String {
    let mut s = body.to_string();
    if let Some(i) = SIGNIFIERS.iter().filter_map(|e| s.find(e)).min() {
        s.truncate(i);
    }
    // [due:: 2026-10-21] and friends (but not ordinary [links](...)).
    let mut i = 0;
    while let Some(off) = s[i..].find('[') {
        let a = i + off;
        match s[a..].find(']') {
            Some(e) if s[a..a + e].contains("::") => {
                s.replace_range(a..a + e + 1, "");
                i = a;
            }
            Some(e) => i = a + e + 1,
            None => break,
        }
    }
    if let Some(i) = s.to_lowercase().find("due::") {
        if s.is_char_boundary(i) {
            s.truncate(i);
        }
    }
    let words: Vec<&str> = s
        .split_whitespace()
        .filter(|w| {
            let lw = w.to_lowercase();
            !(lw.starts_with("due:") || lw.starts_with("@due(") || w.starts_with('^'))
        })
        .collect();
    words.join(" ").trim().to_string()
}

/// Parse every open task in a document.
pub fn parse_document(text: &str) -> Vec<MdTask> {
    let mut in_code = false;
    let mut out = Vec::new();
    for line in strip_frontmatter(text).lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        if let Some(t) = parse_line(line) {
            out.push(t);
        }
    }
    out
}

/// Drop a leading `---` YAML frontmatter block.
pub fn strip_frontmatter(text: &str) -> &str {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return text;
    };
    match rest.find("\n---") {
        Some(end) => {
            let after = &rest[end + 4..];
            after
                .strip_prefix('\n')
                .or_else(|| after.strip_prefix("\r\n"))
                .unwrap_or(after)
        }
        None => text,
    }
}

/// Convert to [`Task`]s, keeping undated items only if `include_undated`.
/// `implicit_due` dates undated tasks (e.g. items in today's daily note).
pub fn to_tasks(
    items: Vec<MdTask>,
    source: &str,
    context: &str,
    include_undated: bool,
    implicit_due: Option<NaiveDate>,
) -> Vec<Task> {
    items
        .into_iter()
        .filter_map(|m| {
            let due = m.due.or(implicit_due);
            if due.is_none() && !include_undated {
                return None;
            }
            let mut t = Task::new(m.title, due, m.priority).with_context(context);
            t.sources.push(source.to_string());
            Some(t)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Option<NaiveDate> {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
    }

    #[test]
    fn tasks_plugin_syntax() {
        let t = parse_line("- [ ] Ship v0.1 ⏫ 🔁 every week 📅 2026-10-21 ^abc123").unwrap();
        assert_eq!(t.title, "Ship v0.1");
        assert_eq!(t.due, d("2026-10-21"));
        assert_eq!(t.priority, Priority::High);
    }

    #[test]
    fn scheduled_is_fallback() {
        let t = parse_line("- [ ] Review PRs ⏳ 2026-10-20").unwrap();
        assert_eq!(t.due, d("2026-10-20"));
        let t = parse_line("- [ ] Both ⏳ 2026-10-20 📅 2026-10-25").unwrap();
        assert_eq!(t.due, d("2026-10-25"));
    }

    #[test]
    fn dataview_todotxt_taskpaper() {
        let t = parse_line("* [ ] Call bank [due:: 2026-10-21]").unwrap();
        assert_eq!((t.title.as_str(), t.due), ("Call bank", d("2026-10-21")));
        let t = parse_line("  - [ ] Pay rent due:: 2026-10-01").unwrap();
        assert_eq!((t.title.as_str(), t.due), ("Pay rent", d("2026-10-01")));
        let t = parse_line("1. [ ] Renew domain due:2026-10-22").unwrap();
        assert_eq!((t.title.as_str(), t.due), ("Renew domain", d("2026-10-22")));
        let t = parse_line("+ [/] Groceries @due(2026-10-21)").unwrap();
        assert_eq!((t.title.as_str(), t.due), ("Groceries", d("2026-10-21")));
    }

    #[test]
    fn done_and_non_tasks_are_ignored() {
        assert!(parse_line("- [x] Done").is_none());
        assert!(parse_line("- [-] Cancelled").is_none());
        assert!(parse_line("- plain bullet").is_none());
        assert!(parse_line("[ ] not a list item").is_none());
        assert!(parse_line("- [ ] ").is_none());
    }

    #[test]
    fn bang_priorities() {
        assert_eq!(
            parse_line("- [ ] !!! Fire").unwrap().priority,
            Priority::Urgent
        );
        assert_eq!(parse_line("- [ ] !! Smoke").unwrap().title, "Smoke");
        assert_eq!(parse_line("- [ ] Low 🔽").unwrap().priority, Priority::Low);
    }

    #[test]
    fn document_skips_frontmatter_and_code() {
        let doc = "---\ntags: [x]\n---\n- [ ] Real\n```\n- [ ] In code\n```\n- [x] Done\n";
        let items = parse_document(doc);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Real");
    }
}

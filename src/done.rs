//! `daypeek done <n>`: complete a task by the number shown next to it.
//!
//! Opt-in and narrow on purpose. daypeek is read-only unless you set
//! `allow_write = true` (per provider or under `[general]`). Only Todoist and
//! TickTick can be changed, and only by marking one task done. Numbers come
//! from the list you last saw (saved in the cache folder when the dashboard
//! drew them), so `done 3` is the task that was on your screen as 3.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::cache::Cache;
use crate::config::Config;
use crate::error::Error;
use crate::http::Http;
use crate::model::TaskRef;
use crate::providers::{self, Provider};
use crate::render;
use crate::snapshot::{provider_cache_keys, Snapshot};

/// Cache key of the last numbered list shown.
pub const SAVED_KEY: &str = "numbered-tasks";

/// Shown when nothing is allowed to write.
pub const READ_ONLY: &str =
    "daypeek is read-only by default, so `done` can't change anything yet.\n  \
To let it complete tasks in Todoist or TickTick, add this to ~/.config/daypeek/config.toml:\n\n    \
[providers.todoist]\n    allow_write = true\n\n  \
(or `allow_write = true` under [general] for every app that supports it).\n  \
Guide: https://github.com/Jherin89/daypeek/blob/main/docs/USAGE.md#completing-tasks";

/// One numbered task as it was shown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Numbered {
    pub n: usize,
    pub title: String,
    pub refs: Vec<TaskRef>,
    /// Every app that reported it (including read-only ones).
    pub sources: Vec<String>,
}

/// The numbers on screen for `s`, matching what the renderer drew.
pub fn number(s: &Snapshot, max_tasks: usize) -> Vec<Numbered> {
    render::numbered(s, max_tasks)
        .into_iter()
        .enumerate()
        .map(|(i, t)| Numbered {
            n: i + 1,
            title: t.title.clone(),
            refs: t
                .refs
                .iter()
                .filter(|r| s.writable.contains(&r.source))
                .cloned()
                .collect(),
            sources: t.sources.clone(),
        })
        .collect()
}

/// Remember the numbers just drawn, including an explicitly empty dashboard.
pub fn save(cache: &Cache, list: &[Numbered]) {
    cache.store(SAVED_KEY, &list);
}

/// The last numbered list, if it was drawn after `not_before` (today).
pub fn load(cache: &Cache, not_before: DateTime<Utc>) -> Option<Vec<Numbered>> {
    let (at, list) = cache.load::<Vec<Numbered>>(SAVED_KEY)?;
    (at >= not_before).then_some(list)
}

/// Find task `n`, or explain which numbers exist.
pub fn pick(list: &[Numbered], n: usize) -> Result<&Numbered, String> {
    list.iter().find(|t| t.n == n).ok_or_else(|| {
        let nums: Vec<String> = list.iter().map(|t| t.n.to_string()).collect();
        if nums.is_empty() {
            "no numbered tasks right now. Numbers appear next to tasks from apps with \
             `allow_write = true`; run `daypeek` to see them."
                .to_string()
        } else {
            format!(
                "there's no task {n} (numbers on screen: {}). Run `daypeek` to see the current list.",
                nums.join(", ")
            )
        }
    })
}

/// Is any configured provider allowed and able to complete tasks?
pub fn any_writable(cfg: &Config) -> bool {
    providers::build_all(cfg)
        .0
        .iter()
        .any(|p| p.can_complete() && providers::write_allowed(cfg, p.id()))
}

/// The apps `item` would be completed in: `(provider id, display name)`.
pub fn targets(cfg: &Config, item: &Numbered) -> Vec<(String, String)> {
    let (built, _) = providers::build_all(cfg);
    item.refs
        .iter()
        .filter_map(|r| writable_provider(cfg, &built, &r.source))
        .map(|p| (p.id().to_string(), p.name().to_string()))
        .collect()
}

fn writable_provider<'a>(
    cfg: &Config,
    built: &'a [Box<dyn Provider>],
    id: &str,
) -> Option<&'a dyn Provider> {
    built
        .iter()
        .find(|p| p.id() == id && p.can_complete() && providers::write_allowed(cfg, id))
        .map(|p| p.as_ref())
}

/// Complete `item` in every app that allows it. Each app's cached lists are
/// dropped afterwards so the next run shows the change. Returns one result
/// per app.
pub fn complete(
    cfg: &Config,
    http: &Http,
    cache: &Cache,
    item: &Numbered,
) -> Vec<(String, Result<(), Error>)> {
    let (built, _) = providers::build_all(cfg);
    let mut out = Vec::new();
    for r in &item.refs {
        let Some(p) = writable_provider(cfg, &built, &r.source) else {
            out.push((
                r.source.clone(),
                Err(Error::Config(format!(
                    "writes to {} aren't allowed (set allow_write = true in [providers.{}])",
                    r.source, r.source
                ))),
            ));
            continue;
        };
        let res = p.complete(http, r);
        if res.is_ok() {
            for key in provider_cache_keys(p.id()) {
                cache.forget(&key);
            }
        }
        out.push((p.name().to_string(), res));
    }
    out
}

/// After completing `n`, keep the other numbers as they were on screen.
pub fn forget_number(cache: &Cache, list: &[Numbered], n: usize) {
    let rest: Vec<Numbered> = list.iter().filter(|t| t.n != n).cloned().collect();
    save(cache, &rest);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::View;
    use crate::testserver::{serve, Response};
    use chrono::TimeZone;

    fn demo_writable() -> Snapshot {
        let tz = chrono_tz::America::New_York;
        let now = tz.with_ymd_and_hms(2026, 10, 21, 10, 58, 0).unwrap();
        let mut s = crate::demo::snapshot(now, View::Today);
        s.writable = vec!["todoist".into()];
        s
    }

    #[test]
    fn only_tasks_from_writable_apps_are_numbered() {
        let mut s = demo_writable();
        let list = number(&s, 15);
        assert!(!list.is_empty());
        assert!(list.iter().all(|t| t.sources.contains(&"todoist".into())));
        assert!(list
            .iter()
            .all(|t| t.refs.iter().all(|r| r.source == "todoist")));
        assert_eq!(list[0].n, 1);
        // Read-only by default: no numbers at all.
        s.writable.clear();
        assert!(number(&s, 15).is_empty());
    }

    #[test]
    fn numbers_on_screen_match_the_saved_list() {
        let s = demo_writable();
        let style = render::Style {
            width: 90,
            time_format: crate::config::TimeFormat::H12,
            max_tasks: 15,
        };
        colored::control::set_override(false);
        let text = render::full(&s, &style);
        for t in number(&s, 15) {
            let row = text
                .lines()
                .find(|l| l.contains(&t.title))
                .unwrap_or_else(|| panic!("{} not shown", t.title));
            assert!(
                row.trim_start().starts_with(&format!("{} ", t.n)),
                "{row:?} should start with {}",
                t.n
            );
        }
        // A read-only task keeps the plain indent.
        let things = text.lines().find(|l| l.contains("Book flights")).unwrap();
        assert!(things.starts_with("    "), "{things:?}");
    }

    #[test]
    fn pick_explains_missing_numbers() {
        let list = number(&demo_writable(), 15);
        assert_eq!(pick(&list, 1).unwrap().n, 1);
        let err = pick(&list, 99).unwrap_err();
        assert!(err.contains("no task 99") && err.contains("1, 2"), "{err}");
        assert!(pick(&[], 1).unwrap_err().contains("allow_write"));
    }

    #[test]
    fn saved_lists_expire_at_midnight_and_numbers_stay_put() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(Some(dir.path().to_path_buf()), false);
        let list = number(&demo_writable(), 15);
        save(&cache, &list);
        let earlier = Utc::now() - chrono::Duration::hours(1);
        assert_eq!(load(&cache, earlier).unwrap(), list);
        assert!(load(&cache, Utc::now() + chrono::Duration::hours(1)).is_none());
        forget_number(&cache, &list, 1);
        let rest = load(&cache, earlier).unwrap();
        assert_eq!(rest[0].n, 2, "the others keep their numbers");
    }

    #[test]
    fn empty_dashboard_and_last_completion_keep_an_empty_numbered_list() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(Some(dir.path().to_path_buf()), false);
        let earlier = Utc::now() - chrono::Duration::hours(1);
        let list = number(&demo_writable(), 15);
        save(&cache, &list);
        save(&cache, &[]);
        assert_eq!(load(&cache, earlier), Some(vec![]));
        assert!(pick(&load(&cache, earlier).unwrap(), 1).is_err());
        save(&cache, &list[..1]);
        forget_number(&cache, &list[..1], 1);
        assert_eq!(load(&cache, earlier), Some(vec![]));
    }

    #[test]
    fn complete_needs_allow_write_and_clears_the_cache() {
        let server = serve(|_| Response::new(204, ""));
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(Some(dir.path().to_path_buf()), false);
        cache.store("provider-todoist", &1);
        cache.store("provider-todoist@week", &1);
        let item = Numbered {
            n: 1,
            title: "Submit expense report".into(),
            refs: vec![TaskRef {
                source: "todoist".into(),
                id: "42".into(),
                project: None,
            }],
            sources: vec!["todoist".into()],
        };
        let http = Http::new(std::time::Duration::from_secs(5));
        let toml = |allow: bool| {
            format!(
                "[providers.todoist]\ntoken = \"t\"\ntoken_env = \"DAYPEEK_TEST_DONE_UNSET\"\nbase_url = \"{}\"\nallow_write = {allow}\n",
                server.url
            )
        };
        let ro = Config::parse(&toml(false)).unwrap();
        assert!(!any_writable(&ro));
        assert!(targets(&ro, &item).is_empty());
        let res = complete(&ro, &http, &cache, &item);
        assert!(res[0].1.is_err());
        assert!(
            server.requests().is_empty(),
            "nothing is sent when read-only"
        );

        let rw = Config::parse(&toml(true)).unwrap();
        assert!(any_writable(&rw));
        assert_eq!(
            targets(&rw, &item),
            vec![("todoist".into(), "Todoist".into())]
        );
        let res = complete(&rw, &http, &cache, &item);
        assert!(res[0].1.is_ok(), "{:?}", res[0].1);
        assert_eq!(server.requests()[0].path, "/api/v1/tasks/42/close");
        assert!(cache.load::<i32>("provider-todoist").is_none());
        assert!(cache.load::<i32>("provider-todoist@week").is_none());
    }
}

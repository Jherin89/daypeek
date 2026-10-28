//! `~/.config/daypeek/config.toml`: loading, defaults, and the `--init` sample.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Top-level configuration. Every section is optional.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub general: General,
    /// iCal feeds (`[[calendars]]`).
    pub calendars: Vec<CalendarSource>,
    /// `[weather]`; weather is off when this section is absent.
    pub weather: Option<WeatherConfig>,
    /// `[providers.<id>]` tables, parsed by each provider.
    pub providers: BTreeMap<String, toml::Table>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct General {
    /// IANA zone like `America/New_York`; defaults to the system zone.
    pub timezone: Option<String>,
    /// `"12h"` or `"24h"`.
    pub time_format: TimeFormat,
    /// Per-request network timeout.
    pub timeout_secs: u64,
    /// Override for `~/.cache/daypeek`.
    pub cache_dir: Option<String>,
    /// Max tasks shown in the full view (JSON always has everything).
    pub max_tasks: usize,
    /// Reuse cloud results fetched less than this many minutes ago, so a
    /// `daypeek --compact` in every new terminal is instant. 0 = always
    /// fetch. `--refresh` forces a fetch.
    pub cache_minutes: u64,
    /// Let `daypeek done` complete tasks in every app that supports it
    /// (Todoist, TickTick). Off by default: daypeek is read-only unless you
    /// opt in here or per provider with `allow_write = true`.
    pub allow_write: bool,
}

impl Default for General {
    fn default() -> Self {
        General {
            timezone: None,
            time_format: TimeFormat::H12,
            timeout_secs: 4,
            cache_dir: None,
            max_tasks: 15,
            cache_minutes: 5,
            allow_write: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
pub enum TimeFormat {
    #[serde(rename = "12h")]
    H12,
    #[serde(rename = "24h")]
    H24,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CalendarSource {
    pub name: String,
    /// iCal: `https://`, `webcal://`, `file://`, or a plain local path.
    /// CalDAV: the server (e.g. `https://caldav.icloud.com/`) or a calendar's
    /// own address.
    pub url: String,
    /// `"ical"` (default) or `"caldav"`.
    #[serde(default, rename = "type")]
    pub kind: CalendarKind,
    /// CalDAV sign-in name (usually your email address).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// CalDAV password in the config file. Discouraged: use the env var
    /// (`password_env`) or the Keychain item `daypeek-<id>` instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// Env var holding the CalDAV password [default: `DAYPEEK_<ID>_PASSWORD`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_env: Option<String>,
    /// CalDAV: only these calendars (by name); empty = all of them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calendars: Vec<String>,
    /// Short id for the Keychain item and env var [default: from `name`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CalendarKind {
    #[default]
    Ical,
    Caldav,
}

impl CalendarSource {
    /// A plain iCal feed.
    pub fn ical(name: impl Into<String>, url: impl Into<String>) -> Self {
        CalendarSource {
            name: name.into(),
            url: url.into(),
            kind: CalendarKind::Ical,
            username: None,
            password: None,
            password_env: None,
            calendars: Vec::new(),
            id: None,
        }
    }

    pub fn is_caldav(&self) -> bool {
        self.kind == CalendarKind::Caldav
    }

    /// `id`, or the name as a slug: "Work iCloud" → `work-icloud`. Used for
    /// the Keychain item `daypeek-<id>`.
    pub fn secret_id(&self) -> String {
        match self.id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(id) => id.to_string(),
            None => slug(&self.name),
        }
    }

    /// The env var checked first for the CalDAV password.
    pub fn password_env(&self) -> String {
        match &self.password_env {
            Some(e) if !e.trim().is_empty() => e.trim().to_string(),
            _ => default_password_env(&self.secret_id()),
        }
    }
}

/// `work-icloud` → `DAYPEEK_WORK_ICLOUD_PASSWORD`.
pub fn default_password_env(id: &str) -> String {
    format!("DAYPEEK_{}_PASSWORD", id.to_uppercase().replace('-', "_"))
}

/// Lowercase letters and digits, with `-` between words.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "calendar".into()
    } else {
        out
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct WeatherConfig {
    pub enabled: bool,
    /// Free-form place, e.g. `"Evans, Georgia"` — geocoded via Open-Meteo.
    pub city: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub units: Units,
}

impl Default for WeatherConfig {
    fn default() -> Self {
        WeatherConfig {
            enabled: true,
            city: None,
            latitude: None,
            longitude: None,
            units: Units::Imperial,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Units {
    #[default]
    Imperial,
    Metric,
}

impl Config {
    pub fn parse(text: &str) -> Result<Config> {
        toml::from_str(text).map_err(|e| Error::Config(e.to_string()))
    }

    /// Load from `path`; a missing file yields the defaults.
    pub fn load(path: &Path) -> Result<Config> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                Config::parse(&text).map_err(|e| Error::Config(format!("{}: {e}", path.display())))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn cache_dir(&self) -> Option<PathBuf> {
        match &self.general.cache_dir {
            Some(d) => Some(expand_tilde(d)),
            None => default_cache_dir(),
        }
    }

    /// Isolate cached data and task numbers by config path and contents.
    /// Only an opaque digest is used on disk, never config values or secrets.
    pub fn scoped_cache_dir(&self, path: &Path) -> Option<PathBuf> {
        use sha2::{Digest, Sha256};
        let path = path
            .canonicalize()
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join(path));
        let mut digest = Sha256::new();
        digest.update(path.as_os_str().as_encoded_bytes());
        digest.update([0]);
        digest.update(serde_json::to_vec(self).ok()?);
        self.cache_dir()
            .map(|base| base.join(format!("profile-{:x}", digest.finalize())))
    }

    /// Weather settings, if weather is configured and enabled.
    pub fn weather(&self) -> Option<&WeatherConfig> {
        self.weather.as_ref().filter(|w| {
            w.enabled && (w.city.is_some() || (w.latitude.is_some() && w.longitude.is_some()))
        })
    }
}

pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// Expand a leading `~/`.
pub fn expand_tilde(p: &str) -> PathBuf {
    if p == "~" {
        return home().unwrap_or_else(|| PathBuf::from(p));
    }
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(h) = home() {
            return h.join(rest);
        }
    }
    PathBuf::from(p)
}

/// `$DAYPEEK_CONFIG`, else `$XDG_CONFIG_HOME/daypeek/config.toml`, else
/// `~/.config/daypeek/config.toml` (also on macOS, on purpose: dotfile-friendly).
pub fn default_config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("DAYPEEK_CONFIG").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("daypeek").join("config.toml")
}

pub fn default_cache_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".cache")))
        .map(|b| b.join("daypeek"))
}

/// Written by `daypeek --init`. Must always parse (see tests).
pub const SAMPLE_CONFIG: &str = r#"# daypeek config. Everything below is commented out: uncomment what you use.
# Easiest path: run `daypeek setup` and it writes this file for you.
# Setup guide: https://github.com/Jherin89/daypeek/blob/main/docs/SETUP.md
# Keep secrets out of this file: prefer env vars or the macOS Keychain
# (see docs/SETUP.md, "Credentials").

# [general]
# timezone = "America/New_York"   # default: your system time zone
# time_format = "12h"             # or "24h"
# timeout_secs = 4                # per network request
# max_tasks = 15
# cache_minutes = 5               # reuse results this fresh (0 = always fetch)
# allow_write = false             # true lets `daypeek done` complete tasks (Todoist, TickTick)

# ── Calendars ─────────────────────────────────────────────────────────────
# Any iCal (.ics) feed. For Google Calendar use the calendar's
# "Secret address in iCal format" (Settings → your calendar → Integrate).
# [[calendars]]
# name = "Personal"
# url  = "https://calendar.google.com/calendar/ical/.../basic.ics"
#
# [[calendars]]
# name = "Holidays"
# url  = "webcal://example.com/holidays.ics"
#
# CalDAV (iCloud, Fastmail, Nextcloud, ...): sign in with an app password.
# The password comes from $DAYPEEK_ICLOUD_PASSWORD or Keychain item
# "daypeek-icloud" (the id is the name in lowercase), never this file.
# [[calendars]]
# name = "iCloud"
# type = "caldav"
# url  = "https://caldav.icloud.com/"
# username = "you@icloud.com"
# calendars = ["Home", "Work"]   # optional; empty = every calendar

# ── Weather (Open-Meteo, no API key) ──────────────────────────────────────
# [weather]
# city  = "Your City, Region"     # e.g. "Portland, Oregon" or "Leeds, UK"
# units = "imperial"              # or "metric"
# latitude = 45.52                # lat/lon skip the city lookup
# longitude = -122.68

# ── Task providers ────────────────────────────────────────────────────────
# Enable as many as you like; tasks are merged and de-duplicated.

# [providers.todoist]             # token: Todoist → Settings → Integrations → Developer
# # Read from $DAYPEEK_TODOIST_TOKEN or $TODOIST_API_TOKEN, or Keychain item "daypeek-todoist".
# filter = "today | overdue"      # used by the today view
# allow_write = true              # let `daypeek done` complete tasks here (off by default)

# [providers.ticktick]            # $TICKTICK_ACCESS_TOKEN  (see `daypeek auth ticktick`)
# projects = []                   # empty = inbox + all projects

# [providers.mstodo]              # Microsoft To Do via Graph
# client_id = "your-azure-app-client-id"   # refresh token: $MSTODO_REFRESH_TOKEN

# [providers.gtasks]              # Google Tasks
# client_id = "....apps.googleusercontent.com"
# # $GTASKS_CLIENT_SECRET and $GTASKS_REFRESH_TOKEN (see `daypeek auth gtasks`)

# [providers.things]              # Things 3 (macOS, read-only)

# [providers.reminders]           # Apple Reminders (macOS)
# lists = []                      # empty = all lists

# [providers.notion]              # $NOTION_TOKEN
# database_id = "..."
# title_property = "Name"
# date_property = "Due"
# done_property = "Done"
# priority_property = "Priority"  # optional select

# [providers.linear]              # $LINEAR_API_KEY or Keychain "daypeek-linear"
# teams = []                      # team keys like "ENG"; empty = all teams
# include_in_progress = true      # show started issues even without a due date

# [providers.asana]               # $ASANA_TOKEN or Keychain "daypeek-asana"
# workspace = "My Company"        # name or gid; empty = every workspace

# [providers.trello]              # $TRELLO_API_KEY + $TRELLO_TOKEN (or Keychain)
# mine_only = true                # cards assigned to you
# boards = []                     # board names/ids; with mine_only = false, all cards on them

# [providers.obsidian]
# vault = "~/Documents/Obsidian/Main"
# daily_notes_folder = "Daily"
# daily_note_format = "%Y-%m-%d"

# [providers.todotxt]
# path = "~/todo/todo.txt"

# [providers.markdown]
# paths = ["~/notes/tasks.md"]
# include_undated = true
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_config_is_neutral_and_all_commented() {
        // Parses to pure defaults: nothing is switched on by accident.
        let c = Config::parse(SAMPLE_CONFIG).unwrap();
        assert_eq!(c, Config::default());
        assert!(c.weather().is_none() && c.providers.is_empty());
        for line in SAMPLE_CONFIG.lines() {
            let t = line.trim();
            assert!(t.is_empty() || t.starts_with('#'), "uncommented: {line}");
        }
        assert!(!SAMPLE_CONFIG.contains("Evans"), "no personal defaults");
        // Every commented example is valid once uncommented.
        let uncommented: String = SAMPLE_CONFIG
            .lines()
            .filter_map(|l| l.strip_prefix("# "))
            .filter(|l| l.starts_with('[') || l.contains(" = ") || l.contains("  = "))
            .filter(|l| {
                !l.starts_with("[[calendars]]")
                    && !l.starts_with("name")
                    && !l.starts_with("url")
                    && !l.starts_with("type")
                    && !l.starts_with("username")
                    && !l.starts_with("calendars")
            })
            .collect::<Vec<_>>()
            .join("\n");
        let c = Config::parse(&uncommented).unwrap_or_else(|e| panic!("{e}\n{uncommented}"));
        assert_eq!(c.general.cache_minutes, 5);
        assert!(c.providers.contains_key("todoist") && c.weather().is_some());
    }

    #[test]
    fn empty_config_is_all_defaults() {
        let c = Config::parse("").unwrap();
        assert_eq!(c, Config::default());
        assert!(c.weather().is_none(), "weather is off unless configured");
    }

    #[test]
    fn full_config_round_trip() {
        let c = Config::parse(
            r#"
            [general]
            timezone = "America/Chicago"
            time_format = "24h"
            [[calendars]]
            name = "Work"
            url = "webcal://example.com/a.ics"
            [[calendars]]
            name = "Home"
            url = "~/cal.ics"
            [weather]
            latitude = 33.5
            longitude = -82.1
            units = "metric"
            [providers.todoist]
            filter = "today"
            [providers.work-notes]
            type = "markdown"
            paths = ["~/work.md"]
            "#,
        )
        .unwrap();
        assert_eq!(c.general.timezone.as_deref(), Some("America/Chicago"));
        assert_eq!(c.general.time_format, TimeFormat::H24);
        assert_eq!(c.calendars.len(), 2);
        assert_eq!(c.weather().unwrap().units, Units::Metric);
        assert_eq!(c.providers["work-notes"]["type"].as_str(), Some("markdown"));
    }

    #[test]
    fn caldav_calendars_and_secret_names() {
        let c = Config::parse(
            r#"
            [[calendars]]
            name = "Work iCloud"
            type = "caldav"
            url = "https://caldav.icloud.com/"
            username = "me@icloud.com"
            calendars = ["Work"]
            [[calendars]]
            name = "Fastmail"
            type = "caldav"
            url = "https://caldav.fastmail.com/"
            username = "me@fastmail.com"
            id = "fm"
            password_env = "FM_PASS"
            [[calendars]]
            name = "Holidays"
            url = "webcal://example.com/h.ics"
            "#,
        )
        .unwrap();
        let icloud = &c.calendars[0];
        assert!(icloud.is_caldav() && !c.calendars[2].is_caldav());
        assert_eq!(icloud.secret_id(), "work-icloud");
        assert_eq!(icloud.password_env(), "DAYPEEK_WORK_ICLOUD_PASSWORD");
        assert_eq!(c.calendars[1].secret_id(), "fm");
        assert_eq!(c.calendars[1].password_env(), "FM_PASS");
        assert_eq!(slug("  Mom's ⭐ cal "), "mom-s-cal");
        assert_eq!(slug("!!"), "calendar");
        assert!(
            Config::parse("[[calendars]]\nname = \"x\"\nurl = \"y\"\ntype = \"exchange\"").is_err()
        );
    }

    #[test]
    fn weather_needs_location_or_enabled() {
        let c = Config::parse("[weather]\nunits = \"metric\"").unwrap();
        assert!(c.weather().is_none());
        let c = Config::parse("[weather]\ncity = \"Oslo\"\nenabled = false").unwrap();
        assert!(c.weather().is_none());
    }

    #[test]
    fn unknown_keys_are_errors() {
        let err = Config::parse("[general]\ntimeout = 3").unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn tilde_expands() {
        if let Some(h) = home() {
            assert_eq!(expand_tilde("~/x/y"), h.join("x/y"));
        }
        assert_eq!(expand_tilde("/abs"), PathBuf::from("/abs"));
    }

    #[test]
    fn profiles_isolate_data_and_numbers_even_with_a_shared_cache_root() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.general.cache_dir = Some(dir.path().display().to_string());
        let work = dir.path().join("work.toml");
        let personal = dir.path().join("personal.toml");
        let work_cache = crate::cache::Cache::new(cfg.scoped_cache_dir(&work), false);
        let personal_cache = crate::cache::Cache::new(cfg.scoped_cache_dir(&personal), false);
        work_cache.store("calendar-Calendar", &"fake work event");
        work_cache.store(crate::done::SAVED_KEY, &vec![1]);
        assert!(personal_cache.load::<String>("calendar-Calendar").is_none());
        assert!(personal_cache
            .load::<Vec<i32>>(crate::done::SAVED_KEY)
            .is_none());
        assert_eq!(cfg.scoped_cache_dir(&work), cfg.scoped_cache_dir(&work));
        let original = cfg.scoped_cache_dir(&work);
        cfg.weather = Some(WeatherConfig {
            city: Some("Oslo".into()),
            ..WeatherConfig::default()
        });
        assert_ne!(
            original,
            cfg.scoped_cache_dir(&work),
            "config edits invalidate old data"
        );
    }
}

//! `~/.config/today/config.toml`: loading, defaults, and the `--init` sample.

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
    /// Override for `~/.cache/today`.
    pub cache_dir: Option<String>,
    /// Max tasks shown in the full view (JSON always has everything).
    pub max_tasks: usize,
}

impl Default for General {
    fn default() -> Self {
        General {
            timezone: None,
            time_format: TimeFormat::H12,
            timeout_secs: 4,
            cache_dir: None,
            max_tasks: 15,
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
    /// `https://`, `webcal://`, `file://`, or a plain local path.
    pub url: String,
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

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Units {
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

    /// Weather settings, if weather is configured and enabled.
    pub fn weather(&self) -> Option<&WeatherConfig> {
        self.weather.as_ref().filter(|w| {
            w.enabled && (w.city.is_some() || (w.latitude.is_some() && w.longitude.is_some()))
        })
    }
}

fn home() -> Option<PathBuf> {
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

/// `$TODAY_CONFIG`, else `$XDG_CONFIG_HOME/today/config.toml`, else
/// `~/.config/today/config.toml` (also on macOS, on purpose: dotfile-friendly).
pub fn default_config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("TODAY_CONFIG").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("today").join("config.toml")
}

pub fn default_cache_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CACHE_HOME")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".cache")))
        .map(|b| b.join("today"))
}

/// Written by `today --init`. Must always parse (see tests).
pub const SAMPLE_CONFIG: &str = r#"# today — config. Docs: https://github.com/Jherin89/today#configuration
# Secrets never belong in this file if you can avoid it: prefer env vars or
# the macOS Keychain (see README "Credentials").

[general]
# timezone = "America/New_York"   # default: system time zone
time_format = "12h"                # or "24h"
timeout_secs = 4                   # per network request
max_tasks = 15

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

# ── Weather (Open-Meteo, no API key) ──────────────────────────────────────
[weather]
city  = "Evans, Georgia"
units = "imperial"                 # or "metric"
# latitude = 33.5337               # lat/lon skip geocoding
# longitude = -82.1307

# ── Task providers ────────────────────────────────────────────────────────
# Enable as many as you like; tasks are merged and de-duplicated.

[providers.todoist]
# Token from Todoist → Settings → Integrations → Developer.
# Read from $TODOIST_API_TOKEN, or Keychain item "today-todoist".
filter = "today | overdue"

# [providers.ticktick]            # $TICKTICK_ACCESS_TOKEN  (see `today auth ticktick`)
# projects = []                   # empty = inbox + all projects

# [providers.mstodo]              # Microsoft To Do via Graph
# client_id = "your-azure-app-client-id"   # refresh token: $MSTODO_REFRESH_TOKEN

# [providers.gtasks]              # Google Tasks
# client_id = "....apps.googleusercontent.com"
# # $GTASKS_CLIENT_SECRET and $GTASKS_REFRESH_TOKEN (see `today auth gtasks`)

# [providers.things]              # Things 3 (macOS, read-only)

# [providers.reminders]           # Apple Reminders (macOS)
# lists = []                      # empty = all lists

# [providers.notion]              # $NOTION_TOKEN
# database_id = "..."
# title_property = "Name"
# date_property = "Due"
# done_property = "Done"
# priority_property = "Priority"  # optional select

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
    fn sample_config_parses() {
        let c = Config::parse(SAMPLE_CONFIG).unwrap();
        assert_eq!(c.general.timeout_secs, 4);
        assert_eq!(c.general.time_format, TimeFormat::H12);
        let w = c.weather().unwrap();
        assert_eq!(w.city.as_deref(), Some("Evans, Georgia"));
        assert_eq!(w.units, Units::Imperial);
        assert!(c.providers.contains_key("todoist"));
        assert!(c.calendars.is_empty());
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
}

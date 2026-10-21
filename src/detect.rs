//! Zero-setup detection: with no config file at all, daypeek looks for apps
//! that keep their data on this Mac (Things 3, Obsidian vaults) so the very
//! first run shows something real. `daypeek setup` pre-ticks the same apps.
//!
//! Detection only reads well-known local paths; it never touches the network
//! or prompts for permissions. On Linux and other platforms it finds nothing
//! and daypeek falls back to the "run `daypeek setup`" hint.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::config::Config;

/// An Obsidian vault listed in Obsidian's own `obsidian.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct Vault {
    pub path: PathBuf,
    /// Daily-notes core plugin settings, translated to daypeek's.
    pub daily_notes_folder: String,
    pub daily_note_format: String,
}

impl Vault {
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

/// What was found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Detected {
    /// Things 3's database (opened read-only by the provider).
    pub things: Option<PathBuf>,
    /// Vaults, most recently used first.
    pub vaults: Vec<Vault>,
}

impl Detected {
    pub fn is_empty(&self) -> bool {
        self.things.is_none() && self.vaults.is_empty()
    }

    /// "Things 3 and Obsidian (Main)".
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.things.is_some() {
            parts.push("Things 3".to_string());
        }
        if let Some(v) = self.vaults.first() {
            parts.push(format!("Obsidian ({})", v.name()));
        }
        parts.join(" and ")
    }

    /// A config using only what was detected (no network sources).
    pub fn to_config(&self) -> Config {
        let mut cfg = Config::default();
        if self.things.is_some() {
            cfg.providers.insert("things".into(), toml::Table::new());
        }
        if let Some(v) = self.vaults.first() {
            let mut t = toml::Table::new();
            t.insert("vault".into(), v.path.display().to_string().into());
            t.insert(
                "daily_notes_folder".into(),
                v.daily_notes_folder.clone().into(),
            );
            t.insert(
                "daily_note_format".into(),
                v.daily_note_format.clone().into(),
            );
            cfg.providers.insert("obsidian".into(), t);
        }
        cfg
    }
}

/// Look for local apps. Finds nothing outside macOS.
pub fn detect() -> Detected {
    #[cfg(target_os = "macos")]
    {
        let things = crate::providers::things::default_db_path();
        let vaults = crate::config::home()
            .map(|h| h.join("Library/Application Support/obsidian/obsidian.json"))
            .map(|p| obsidian_vaults(&p))
            .unwrap_or_default();
        Detected { things, vaults }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Detected::default()
    }
}

#[derive(Deserialize)]
struct ObsidianJson {
    #[serde(default)]
    vaults: std::collections::BTreeMap<String, VaultEntry>,
}

#[derive(Deserialize)]
struct VaultEntry {
    path: PathBuf,
    #[serde(default)]
    ts: i64,
    #[serde(default)]
    open: bool,
}

/// Vaults from an `obsidian.json` (open vault first, then most recent), keeping
/// only folders that still exist. Unreadable or odd files yield nothing.
pub fn obsidian_vaults(obsidian_json: &Path) -> Vec<Vault> {
    let Ok(raw) = std::fs::read_to_string(obsidian_json) else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<ObsidianJson>(&raw) else {
        return Vec::new();
    };
    let mut entries: Vec<VaultEntry> = parsed
        .vaults
        .into_values()
        .filter(|v| v.path.is_dir())
        .collect();
    entries.sort_by(|a, b| b.open.cmp(&a.open).then(b.ts.cmp(&a.ts)));
    entries
        .into_iter()
        .map(|v| {
            let (folder, format) = daily_notes_settings(&v.path);
            Vault {
                path: v.path,
                daily_notes_folder: folder,
                daily_note_format: format,
            }
        })
        .collect()
}

#[derive(Deserialize, Default)]
struct DailyNotesJson {
    #[serde(default)]
    folder: String,
    #[serde(default)]
    format: String,
}

/// The vault's daily-notes plugin folder and date format (as strftime).
fn daily_notes_settings(vault: &Path) -> (String, String) {
    let cfg: DailyNotesJson = std::fs::read_to_string(vault.join(".obsidian/daily-notes.json"))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    let folder = cfg.folder.trim_matches('/').to_string();
    let folder = if folder.is_empty() && vault.join("Daily").is_dir() {
        "Daily".to_string()
    } else {
        folder
    };
    let format = moment_to_strftime(&cfg.format).unwrap_or_else(|| "%Y-%m-%d".into());
    (folder, format)
}

/// Translate a Moment.js date format (what Obsidian uses) to strftime.
/// Returns `None` for an empty format or tokens we don't understand.
pub fn moment_to_strftime(fmt: &str) -> Option<String> {
    if fmt.trim().is_empty() {
        return None;
    }
    const TOKENS: [(&str, &str); 12] = [
        ("YYYY", "%Y"),
        ("YY", "%y"),
        ("MMMM", "%B"),
        ("MMM", "%b"),
        ("MM", "%m"),
        ("M", "%-m"),
        ("DDDD", "%j"),
        ("DD", "%d"),
        ("D", "%-d"),
        ("dddd", "%A"),
        ("ddd", "%a"),
        ("ww", "%V"),
    ];
    let mut out = String::new();
    let mut rest = fmt;
    'outer: while !rest.is_empty() {
        // [literal text]
        if let Some(stripped) = rest.strip_prefix('[') {
            let end = stripped.find(']')?;
            out.push_str(&stripped[..end].replace('%', "%%"));
            rest = &stripped[end + 1..];
            continue;
        }
        for (m, s) in TOKENS {
            if let Some(r) = rest.strip_prefix(m) {
                out.push_str(s);
                rest = r;
                continue 'outer;
            }
        }
        let c = rest.chars().next()?;
        if c.is_ascii_alphabetic() {
            return None; // an unsupported token (e.g. "Do", "Q")
        }
        if c == '%' {
            out.push_str("%%");
        } else {
            out.push(c);
        }
        rest = &rest[c.len_utf8()..];
    }
    Some(out)
}

/// A weather city guessed from an IANA zone: `America/New_York` → "New York".
/// Only a default for the setup prompt: the user confirms or edits it.
pub fn city_from_timezone(tz: &str) -> Option<String> {
    let (region, city) = tz.rsplit_once('/')?;
    let region = region.split('/').next().unwrap_or(region);
    if !matches!(
        region,
        "America" | "Europe" | "Asia" | "Africa" | "Australia" | "Pacific" | "Atlantic" | "Indian"
    ) {
        return None; // Etc/UTC, US/Eastern, ...
    }
    let city = city.replace('_', " ");
    (!city.is_empty()).then_some(city)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moment_formats_translate() {
        assert_eq!(
            moment_to_strftime("YYYY-MM-DD").as_deref(),
            Some("%Y-%m-%d")
        );
        assert_eq!(
            moment_to_strftime("dddd, MMMM D, YYYY").as_deref(),
            Some("%A, %B %-d, %Y")
        );
        assert_eq!(
            moment_to_strftime("YYYY/MM/[Week] ww").as_deref(),
            Some("%Y/%m/Week %V")
        );
        assert_eq!(moment_to_strftime(""), None);
        assert_eq!(moment_to_strftime("Do MMM"), None, "ordinal unsupported");
    }

    #[test]
    fn reads_obsidian_json_most_recent_open_first() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("Old Vault");
        let main = dir.path().join("Main");
        std::fs::create_dir_all(main.join(".obsidian")).unwrap();
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(
            main.join(".obsidian/daily-notes.json"),
            r#"{"folder":"Journal/","format":"YYYY-MM-DD dddd"}"#,
        )
        .unwrap();
        let json = dir.path().join("obsidian.json");
        std::fs::write(
            &json,
            serde_json::json!({"vaults": {
                "a": {"path": old, "ts": 1_900_000_000_000_i64},
                "b": {"path": main, "ts": 1_700_000_000_000_i64, "open": true},
                "gone": {"path": dir.path().join("deleted"), "ts": 1}
            }})
            .to_string(),
        )
        .unwrap();
        let vaults = obsidian_vaults(&json);
        assert_eq!(vaults.len(), 2, "missing folders are skipped");
        assert_eq!(vaults[0].name(), "Main");
        assert_eq!(vaults[0].daily_notes_folder, "Journal");
        assert_eq!(vaults[0].daily_note_format, "%Y-%m-%d %A");
        assert_eq!(vaults[1].daily_note_format, "%Y-%m-%d", "plugin defaults");

        let d = Detected {
            things: Some(dir.path().join("main.sqlite")),
            vaults,
        };
        assert_eq!(d.summary(), "Things 3 and Obsidian (Main)");
        let cfg = d.to_config();
        assert!(cfg.providers.contains_key("things"));
        assert_eq!(
            cfg.providers["obsidian"]["daily_notes_folder"].as_str(),
            Some("Journal")
        );
        // The generated config builds real providers.
        let (built, broken) = crate::providers::build_all(&cfg);
        assert_eq!((built.len(), broken.len()), (2, 0));
    }

    #[test]
    fn bad_or_missing_obsidian_json_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(obsidian_vaults(&dir.path().join("nope.json")).is_empty());
        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, "{not json").unwrap();
        assert!(obsidian_vaults(&bad).is_empty());
    }

    #[test]
    fn cities_from_time_zones() {
        assert_eq!(
            city_from_timezone("America/New_York").as_deref(),
            Some("New York")
        );
        assert_eq!(
            city_from_timezone("America/Argentina/Buenos_Aires").as_deref(),
            Some("Buenos Aires")
        );
        assert_eq!(
            city_from_timezone("Europe/London").as_deref(),
            Some("London")
        );
        assert_eq!(city_from_timezone("Etc/UTC"), None);
        assert_eq!(city_from_timezone("UTC"), None);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn detects_nothing_off_macos() {
        assert!(detect().is_empty());
    }
}

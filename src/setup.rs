//! The pure half of `daypeek setup`: the app catalog and turning the
//! wizard's answers into a `config.toml`. No prompts and no I/O here, so it can
//! be tested. The interactive half lives in [`crate::wizard`].
//!
//! Secrets are never written to the config. They go to the Keychain or an
//! env var, and the config only names the provider.

use std::fmt::Write as _;

use crate::config::Units;

/// One app the wizard can set up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct App {
    /// The `[providers.<id>]` key, which is also the provider type.
    pub id: &'static str,
    pub name: &'static str,
    /// One line: where to get the credential.
    pub hint: &'static str,
    /// Official docs for that step.
    pub link: &'static str,
    /// Anchor in docs/SETUP.md.
    pub guide: &'static str,
    pub macos_only: bool,
}

const GUIDE: &str = "https://github.com/Jherin89/daypeek/blob/main/docs/SETUP.md";

/// Every v1 provider, in the order the wizard lists them.
pub const APPS: &[App] = &[
    App {
        id: "todoist",
        name: "Todoist",
        hint: "Todoist → Settings → Integrations → Developer → Copy API token",
        link: "https://www.todoist.com/help/articles/find-your-api-token-Jpzx9IIlB",
        guide: "todoist",
        macos_only: false,
    },
    App {
        id: "ticktick",
        name: "TickTick",
        hint: "Register an app (New App → Edit) and set its redirect URL to http://127.0.0.1:8765/callback",
        link: "https://developer.ticktick.com/manage",
        guide: "ticktick",
        macos_only: false,
    },
    App {
        id: "mstodo",
        name: "Microsoft To Do",
        hint: "Register an app (Mobile and desktop, redirect http://localhost/callback, permission Tasks.Read)",
        link: "https://learn.microsoft.com/en-us/entra/identity-platform/quickstart-register-app",
        guide: "microsoft-to-do",
        macos_only: false,
    },
    App {
        id: "gtasks",
        name: "Google Tasks",
        hint: "Enable the Tasks API, then create an OAuth client of type \"Desktop app\"",
        link: "https://developers.google.com/workspace/tasks/quickstart/python",
        guide: "google-tasks",
        macos_only: false,
    },
    App {
        id: "notion",
        name: "Notion",
        hint: "Developer portal → Internal connections → copy the API token, then add the connection to your database",
        link: "https://developers.notion.com/guides/get-started/internal-connections",
        guide: "notion",
        macos_only: false,
    },
    App {
        id: "linear",
        name: "Linear",
        hint: "Linear → Settings → Security & access → Personal API keys → New API key",
        link: "https://linear.app/developers/graphql",
        guide: "linear",
        macos_only: false,
    },
    App {
        id: "asana",
        name: "Asana",
        hint: "Asana developer console (app.asana.com/0/my-apps) → Create new token",
        link: "https://developers.asana.com/docs/personal-access-token",
        guide: "asana",
        macos_only: false,
    },
    App {
        id: "trello",
        name: "Trello",
        hint: "Create a Power-Up at trello.com/power-ups/admin → API key → then the Token link",
        link: "https://developer.atlassian.com/cloud/trello/guides/rest-api/api-introduction/",
        guide: "trello",
        macos_only: false,
    },
    App {
        id: "things",
        name: "Things 3",
        hint: "Nothing to set up: daypeek reads Things' local database (read-only)",
        link: "https://culturedcode.com/things/",
        guide: "things-3",
        macos_only: true,
    },
    App {
        id: "reminders",
        name: "Apple Reminders",
        hint: "Nothing to set up: allow your terminal to control Reminders when macOS asks",
        link: "https://support.apple.com/guide/reminders/welcome/mac",
        guide: "apple-reminders",
        macos_only: true,
    },
    App {
        id: "obsidian",
        name: "Obsidian",
        hint: "Just the path to your vault",
        link: "https://publish.obsidian.md/tasks/Reference/Task+Formats/Tasks+Emoji+Format",
        guide: "obsidian",
        macos_only: false,
    },
    App {
        id: "todotxt",
        name: "todo.txt",
        hint: "Just the path to your todo.txt",
        link: "https://github.com/todotxt/todo.txt",
        guide: "todotxt",
        macos_only: false,
    },
    App {
        id: "markdown",
        name: "Markdown files",
        hint: "Files or folders with - [ ] checkboxes",
        link: "https://github.com/Jherin89/daypeek/blob/main/docs/SETUP.md#markdown",
        guide: "markdown",
        macos_only: false,
    },
];

impl App {
    pub fn by_id(id: &str) -> Option<&'static App> {
        APPS.iter().find(|a| a.id == id)
    }

    /// Link to this app's section of the setup guide.
    pub fn guide_url(&self) -> String {
        format!("{GUIDE}#{}", self.guide)
    }

    /// The secrets this app needs: `(name, env var)`. The Keychain service is
    /// [`crate::secrets::keychain_service`]`(id, name)`.
    pub fn secrets(&self) -> &'static [(&'static str, &'static str)] {
        match self.id {
            "todoist" => &[("token", "DAYPEEK_TODOIST_TOKEN")],
            "notion" => &[("token", "NOTION_TOKEN")],
            "gtasks" => &[("client_secret", "GTASKS_CLIENT_SECRET")],
            "linear" => &[("token", "LINEAR_API_KEY")],
            "asana" => &[("token", "ASANA_TOKEN")],
            "trello" => &[("api_key", "TRELLO_API_KEY"), ("token", "TRELLO_TOKEN")],
            _ => &[],
        }
    }

    /// Whether `daypeek auth <id>` (browser sign-in) is part of setup.
    pub fn needs_sign_in(&self) -> bool {
        matches!(self.id, "ticktick" | "mstodo" | "gtasks")
    }
}

/// Settings for one chosen app (never secrets).
#[derive(Debug, Clone, PartialEq)]
pub enum AppConfig {
    Todoist,
    TickTick {
        client_id: String,
    },
    MsTodo {
        client_id: String,
        tenant: String,
    },
    GTasks {
        client_id: String,
    },
    Notion {
        database_id: String,
        title_property: String,
        date_property: String,
        done_property: String,
    },
    Linear,
    Asana {
        /// Name or gid; empty means "your only workspace".
        workspace: String,
    },
    Trello {
        /// Board names or ids; empty means "cards assigned to me anywhere".
        boards: Vec<String>,
    },
    Things,
    Reminders,
    Obsidian {
        vault: String,
        daily_notes_folder: String,
        /// Only when it differs from the default `%Y-%m-%d`.
        daily_note_format: Option<String>,
    },
    TodoTxt {
        path: String,
    },
    Markdown {
        paths: Vec<String>,
    },
}

impl AppConfig {
    pub fn id(&self) -> &'static str {
        match self {
            AppConfig::Todoist => "todoist",
            AppConfig::TickTick { .. } => "ticktick",
            AppConfig::MsTodo { .. } => "mstodo",
            AppConfig::GTasks { .. } => "gtasks",
            AppConfig::Notion { .. } => "notion",
            AppConfig::Linear => "linear",
            AppConfig::Asana { .. } => "asana",
            AppConfig::Trello { .. } => "trello",
            AppConfig::Things => "things",
            AppConfig::Reminders => "reminders",
            AppConfig::Obsidian { .. } => "obsidian",
            AppConfig::TodoTxt { .. } => "todotxt",
            AppConfig::Markdown { .. } => "markdown",
        }
    }
}

/// A CalDAV account from the wizard (the password went to the Keychain).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CalDavAnswer {
    pub name: String,
    pub url: String,
    pub username: String,
    /// Calendar names to show; empty means all of them.
    pub calendars: Vec<String>,
}

/// Where the popular CalDAV services live.
pub const CALDAV_PRESETS: &[(&str, &str)] = &[
    ("iCloud", "https://caldav.icloud.com/"),
    ("Fastmail", "https://caldav.fastmail.com/"),
    ("Nextcloud", "https://YOUR-SERVER/remote.php/dav"),
];

/// Everything the wizard collected.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Answers {
    pub apps: Vec<AppConfig>,
    /// iCal links: `(name, url)`
    pub calendars: Vec<(String, String)>,
    pub caldav: Vec<CalDavAnswer>,
    pub city: Option<String>,
    pub units: Units,
}

/// A TOML string literal, escaped.
fn q(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

/// Render the answers as a commented `config.toml`.
pub fn render_config(a: &Answers) -> String {
    let mut out = String::new();
    out.push_str(
        "# daypeek config, written by `daypeek setup`.\n\
         # Re-run `daypeek setup` any time, or edit by hand. Guide:\n\
         # https://github.com/Jherin89/daypeek/blob/main/docs/SETUP.md\n\
         # Tokens are NOT stored here: they live in the macOS Keychain or env vars.\n\n\
         [general]\n\
         time_format = \"12h\"   # or \"24h\"\n\
         timeout_secs = 4\n",
    );

    if !a.calendars.is_empty() || !a.caldav.is_empty() {
        out.push_str("\n# ── Calendars ──────────────────────────────────────────────\n");
        for (name, url) in &a.calendars {
            let _ = write!(
                out,
                "[[calendars]]\nname = {}\nurl  = {}\n\n",
                q(name),
                q(url)
            );
        }
        for c in &a.caldav {
            let id = crate::config::slug(&c.name);
            let _ = write!(
                out,
                "[[calendars]]\nname     = {}\ntype     = \"caldav\"\nurl      = {}\nusername = {}\n\
                 # password: ${} or Keychain \"daypeek-{id}\"\n",
                q(&c.name),
                q(&c.url),
                q(&c.username),
                crate::config::default_password_env(&id),
            );
            if !c.calendars.is_empty() {
                let list: Vec<String> = c.calendars.iter().map(|n| q(n)).collect();
                let _ = writeln!(out, "calendars = [{}]", list.join(", "));
            }
            out.push('\n');
        }
        out.pop();
    }

    if let Some(city) = a.city.as_deref().filter(|c| !c.trim().is_empty()) {
        let units = match a.units {
            Units::Imperial => "imperial",
            Units::Metric => "metric",
        };
        let _ = write!(
            out,
            "\n# ── Weather (Open-Meteo, no key) ────────────────────────────\n\
             [weather]\ncity  = {}\nunits = \"{units}\"\n",
            q(city.trim())
        );
    }

    if !a.apps.is_empty() {
        out.push_str("\n# ── Task apps ───────────────────────────────────────────────\n");
    }
    for app in &a.apps {
        let _ = writeln!(out, "[providers.{}]", app.id());
        match app {
            AppConfig::Todoist => out.push_str(
                "filter = \"today | overdue\"   # token: $DAYPEEK_TODOIST_TOKEN or Keychain \"daypeek-todoist\"\n",
            ),
            AppConfig::TickTick { client_id } => {
                let _ = writeln!(out, "client_id = {}   # token: `daypeek auth ticktick`", q(client_id));
            }
            AppConfig::MsTodo { client_id, tenant } => {
                let _ = writeln!(out, "client_id = {}\ntenant    = {}", q(client_id), q(tenant));
            }
            AppConfig::GTasks { client_id } => {
                let _ = writeln!(out, "client_id = {}", q(client_id));
            }
            AppConfig::Notion {
                database_id,
                title_property,
                date_property,
                done_property,
            } => {
                let _ = writeln!(
                    out,
                    "database_id    = {}\ntitle_property = {}\ndate_property  = {}\ndone_property  = {}",
                    q(database_id),
                    q(title_property),
                    q(date_property),
                    q(done_property)
                );
            }
            AppConfig::Linear => out.push_str(
                "# key: $LINEAR_API_KEY or Keychain \"daypeek-linear\"\n",
            ),
            AppConfig::Asana { workspace } => {
                out.push_str("# token: $ASANA_TOKEN or Keychain \"daypeek-asana\"\n");
                if !workspace.trim().is_empty() {
                    let _ = writeln!(out, "workspace = {}", q(workspace.trim()));
                }
            }
            AppConfig::Trello { boards } => {
                out.push_str(
                    "# key + token: $TRELLO_API_KEY / $TRELLO_TOKEN, or Keychain \"daypeek-trello-api-key\" / \"daypeek-trello\"\n",
                );
                if !boards.is_empty() {
                    let list: Vec<String> = boards.iter().map(|b| q(b)).collect();
                    let _ = writeln!(out, "boards = [{}]", list.join(", "));
                }
            }
            AppConfig::Things | AppConfig::Reminders => {}
            AppConfig::Obsidian {
                vault,
                daily_notes_folder,
                daily_note_format,
            } => {
                let _ = writeln!(
                    out,
                    "vault = {}\ndaily_notes_folder = {}",
                    q(vault),
                    q(daily_notes_folder)
                );
                if let Some(f) = daily_note_format {
                    let _ = writeln!(out, "daily_note_format = {}", q(f));
                }
            }
            AppConfig::TodoTxt { path } => {
                let _ = writeln!(out, "path = {}", q(path));
            }
            AppConfig::Markdown { paths } => {
                let list: Vec<String> = paths.iter().map(|p| q(p)).collect();
                let _ = writeln!(out, "paths = [{}]", list.join(", "));
            }
        }
        out.push('\n');
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

/// Split a comma-separated answer into trimmed, non-empty parts.
pub fn split_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

/// A short calendar name from its URL host (`calendar.google.com` → `Google`).
pub fn guess_calendar_name(url: &str, index: usize) -> String {
    let lower = url.to_ascii_lowercase();
    let name = if lower.contains("google.com") {
        "Google"
    } else if lower.contains("icloud.com") {
        "iCloud"
    } else if lower.contains("outlook") || lower.contains("office365") || lower.contains("live.com")
    {
        "Outlook"
    } else if lower.contains("fastmail") {
        "Fastmail"
    } else {
        "Calendar"
    };
    if index == 0 {
        name.to_string()
    } else {
        format!("{name} {}", index + 1)
    }
}

/// `base`, or `base 2`, `base 3`… if that name is taken.
pub fn unique_name(base: &str, taken: &[String]) -> String {
    let free = |n: &str| !taken.iter().any(|t| t.eq_ignore_ascii_case(n));
    if free(base) {
        return base.to_string();
    }
    (2..)
        .map(|i| format!("{base} {i}"))
        .find(|n| free(n))
        .unwrap_or_else(|| base.to_string())
}

/// `https://cloud.example.com/` → `https://cloud.example.com/remote.php/dav`.
pub fn nextcloud_dav_url(host: &str) -> String {
    let h = host.trim().trim_end_matches('/');
    if h.contains("/remote.php/") {
        h.to_string()
    } else {
        format!("{h}/remote.php/dav")
    }
}

/// Basic sanity check for an iCal URL or path the user typed.
pub fn looks_like_calendar(url: &str) -> bool {
    let u = url.trim();
    u.starts_with("https://")
        || u.starts_with("http://")
        || u.starts_with("webcal://")
        || u.starts_with("file://")
        || u.starts_with('/')
        || u.starts_with('~')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn everything() -> Answers {
        Answers {
            apps: vec![
                AppConfig::Todoist,
                AppConfig::TickTick {
                    client_id: "tt-client".into(),
                },
                AppConfig::MsTodo {
                    client_id: "00000000-0000-0000-0000-000000000000".into(),
                    tenant: "common".into(),
                },
                AppConfig::GTasks {
                    client_id: "123-abc.apps.googleusercontent.com".into(),
                },
                AppConfig::Notion {
                    database_id: "a1b2c3".into(),
                    title_property: "Task name".into(),
                    date_property: "Due".into(),
                    done_property: "Done".into(),
                },
                AppConfig::Linear,
                AppConfig::Asana {
                    workspace: "Acme".into(),
                },
                AppConfig::Trello {
                    boards: vec!["Side projects".into()],
                },
                AppConfig::Things,
                AppConfig::Reminders,
                AppConfig::Obsidian {
                    vault: "~/Vault".into(),
                    daily_notes_folder: "Daily".into(),
                    daily_note_format: Some("%Y-%m-%d %A".into()),
                },
                AppConfig::TodoTxt {
                    path: "~/todo.txt".into(),
                },
                AppConfig::Markdown {
                    paths: vec!["~/a.md".into(), "~/notes/".into()],
                },
            ],
            calendars: vec![
                (
                    "Google".into(),
                    "https://calendar.google.com/x/basic.ics".into(),
                ),
                ("iCloud".into(), "webcal://p01-caldav.icloud.com/y".into()),
            ],
            caldav: vec![CalDavAnswer {
                name: "Fastmail".into(),
                url: "https://caldav.fastmail.com/".into(),
                username: "me@example.com".into(),
                calendars: vec!["Personal".into()],
            }],
            city: Some("Evans, Georgia".into()),
            units: Units::Imperial,
        }
    }

    #[test]
    fn every_app_round_trips_through_the_config_parser() {
        let text = render_config(&everything());
        let c = Config::parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert_eq!(c.calendars.len(), 3);
        assert_eq!(c.calendars[1].name, "iCloud");
        assert!(c.calendars[2].is_caldav());
        assert_eq!(c.calendars[2].username.as_deref(), Some("me@example.com"));
        assert_eq!(c.calendars[2].calendars, vec!["Personal".to_string()]);
        assert_eq!(c.calendars[2].password_env(), "DAYPEEK_FASTMAIL_PASSWORD");
        assert!(text.contains("Keychain \"daypeek-fastmail\""));
        assert_eq!(c.weather().unwrap().city.as_deref(), Some("Evans, Georgia"));
        assert_eq!(c.providers.len(), 13);
        assert_eq!(
            c.providers["notion"]["title_property"].as_str(),
            Some("Task name")
        );
        assert_eq!(
            c.providers["markdown"]["paths"].as_array().unwrap().len(),
            2
        );
        // and every provider block is accepted by its provider's settings
        let (built, broken) = crate::providers::build_all(&c);
        assert!(
            broken.is_empty(),
            "{:?}",
            broken.iter().map(|b| &b.id).collect::<Vec<_>>()
        );
        assert_eq!(built.len(), 13);
    }

    #[test]
    fn empty_answers_are_still_a_valid_config() {
        let text = render_config(&Answers::default());
        let c = Config::parse(&text).unwrap();
        assert!(c.providers.is_empty() && c.calendars.is_empty() && c.weather().is_none());
    }

    #[test]
    fn values_are_escaped() {
        let a = Answers {
            calendars: vec![(
                "Mom \"Work\" \\ cal".into(),
                "https://example.com/a.ics?x=\"1\"".into(),
            )],
            apps: vec![AppConfig::TodoTxt {
                path: "C:\\todo\\todo.txt".into(),
            }],
            ..Answers::default()
        };
        let c = Config::parse(&render_config(&a)).unwrap();
        assert_eq!(c.calendars[0].name, "Mom \"Work\" \\ cal");
        assert_eq!(c.calendars[0].url, "https://example.com/a.ics?x=\"1\"");
        assert_eq!(
            c.providers["todotxt"]["path"].as_str(),
            Some("C:\\todo\\todo.txt")
        );
    }

    #[test]
    fn no_secret_keys_are_ever_written() {
        let text = render_config(&everything());
        for key in [
            "token =",
            "token=",
            "client_secret",
            "password =",
            "api_key =",
            "refresh_token =",
            "access_token =",
        ] {
            assert!(
                !text.contains(key),
                "config must not contain {key}:\n{text}"
            );
        }
    }

    #[test]
    fn blank_city_turns_weather_off_and_metric_is_kept() {
        let mut a = Answers {
            city: Some("  ".into()),
            ..Answers::default()
        };
        assert!(!render_config(&a).contains("[weather]"));
        a.city = Some("Oslo".into());
        a.units = Units::Metric;
        let c = Config::parse(&render_config(&a)).unwrap();
        assert_eq!(c.weather().unwrap().units, Units::Metric);
    }

    #[test]
    fn catalog_is_consistent() {
        assert_eq!(APPS.len(), 13);
        for app in APPS {
            assert!(
                crate::providers::REGISTRY.iter().any(|r| r.0 == app.id),
                "{}",
                app.id
            );
            assert!(app.link.starts_with("https://"));
            assert!(app.guide_url().contains("SETUP.md#"));
        }
        assert!(App::by_id("things").unwrap().macos_only);
        assert_eq!(
            App::by_id("todoist").unwrap().secrets()[0].1,
            "DAYPEEK_TODOIST_TOKEN"
        );
        assert_eq!(App::by_id("trello").unwrap().secrets().len(), 2);
        assert!(CALDAV_PRESETS
            .iter()
            .all(|(_, u)| u.starts_with("https://")));
    }

    #[test]
    fn helpers() {
        assert_eq!(split_list(" a.md, ,b/ "), vec!["a.md", "b/"]);
        assert_eq!(
            guess_calendar_name("https://calendar.google.com/x", 0),
            "Google"
        );
        assert_eq!(
            guess_calendar_name("webcal://p01.icloud.com/x", 1),
            "iCloud 2"
        );
        assert!(looks_like_calendar("webcal://x"));
        assert_eq!(unique_name("iCloud", &["Google".into()]), "iCloud");
        assert_eq!(
            unique_name("iCloud", &["icloud".into(), "iCloud 2".into()]),
            "iCloud 3"
        );
        assert_eq!(
            nextcloud_dav_url(" https://cloud.example.com/ "),
            "https://cloud.example.com/remote.php/dav"
        );
        assert_eq!(
            nextcloud_dav_url("https://x.org/nc/remote.php/dav/"),
            "https://x.org/nc/remote.php/dav"
        );
        assert!(!looks_like_calendar("my calendar"));
    }
}

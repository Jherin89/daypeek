//! `daypeek setup`: the interactive first-run wizard.
//!
//! It asks which apps you use, collects what each one needs (tokens go to
//! the macOS Keychain with hidden input; on Linux you get the env var to
//! set), asks for calendars and a city, and writes `config.toml`. The
//! answers → TOML step is pure and tested in [`crate::setup`].

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::Duration;

use colored::Colorize;
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, Input, MultiSelect, Password, Select};

use crate::config::{expand_tilde, Units};
use crate::detect::{self, Detected, Vault};
use crate::http::Http;
use crate::setup::{self, Answers, App, AppConfig, APPS};
use crate::{oauth, secrets};

/// `daypeek setup --demo` pretends to find Things and a vault, to show the
/// pre-ticked list.
fn demo_detected() -> Detected {
    Detected {
        things: Some(PathBuf::from("Things Database.thingsdatabase/main.sqlite")),
        vaults: vec![Vault {
            path: expand_tilde("~/Documents/Obsidian/Main"),
            daily_notes_folder: "Daily".into(),
            daily_note_format: "%Y-%m-%d".into(),
        }],
    }
}

/// Which entries of [`APPS`] to pre-tick.
pub fn preticked(d: &Detected) -> Vec<bool> {
    APPS.iter()
        .map(|a| match a.id {
            "things" => d.things.is_some(),
            "obsidian" => !d.vaults.is_empty(),
            _ => false,
        })
        .collect()
}

/// The setup prompt's answer for the weather city: `None` = skip.
pub fn city_answer(input: &str) -> Option<String> {
    let t = input.trim();
    if t.is_empty()
        || matches!(
            t.to_ascii_lowercase().as_str(),
            "-" | "none" | "skip" | "no"
        )
    {
        None
    } else {
        Some(t.to_string())
    }
}

pub struct Options {
    pub path: PathBuf,
    /// Walk through every prompt but write and store nothing.
    pub demo: bool,
    /// Loopback port for browser sign-in.
    pub port: u16,
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Written,
    /// Demo mode: what would have been written.
    Demo(Answers),
    Cancelled,
}

/// `/Users/me/.config/x` → `~/.config/x`, for friendlier messages.
pub fn tilde(path: &Path) -> String {
    match crate::config::home() {
        Some(h) => match path.strip_prefix(&h) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => path.display().to_string(),
        },
        None => path.display().to_string(),
    }
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// The message shown when there's no terminal to talk to.
pub const NO_TTY: &str = "`daypeek setup` is interactive and needs a terminal.\n\
     Run it in a terminal window, or write a config by hand:\n  \
     daypeek --init   # then edit ~/.config/daypeek/config.toml\n\
     Guide: https://github.com/Jherin89/daypeek/blob/main/docs/SETUP.md";

struct Wizard {
    opts: Options,
    theme: ColorfulTheme,
    /// macOS Keychain available (or pretended, in demo mode).
    keychain: bool,
    /// Lines to add to a shell profile (Linux, or Keychain failures).
    exports: Vec<String>,
    /// Commands the user still has to run.
    later: Vec<String>,
    /// Apps found on this machine (pre-ticked).
    detected: Detected,
}

fn err(e: dialoguer::Error) -> String {
    format!("prompt: {e}")
}

pub fn run(opts: Options) -> Result<Outcome, String> {
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        return Err(NO_TTY.into());
    }
    let keychain = cfg!(target_os = "macos") || opts.demo;
    let detected = if opts.demo {
        demo_detected()
    } else {
        detect::detect()
    };
    let mut w = Wizard {
        opts,
        theme: ColorfulTheme::default(),
        keychain,
        exports: Vec::new(),
        later: Vec::new(),
        detected,
    };
    w.run()
}

impl Wizard {
    fn run(&mut self) -> Result<Outcome, String> {
        eprintln!(
            "\n{}  {}\n{}\n",
            "daypeek setup".bold(),
            if self.opts.demo {
                "(demo: nothing is saved)".magenta().to_string()
            } else {
                String::new()
            },
            "Four quick steps: apps · calendars · weather · check. Ctrl-C quits any time.".dimmed()
        );

        if self.opts.path.exists() && !self.opts.demo {
            let replace = Confirm::with_theme(&self.theme)
                .with_prompt(format!(
                    "{} already exists. Replace it? (the old one is kept as config.toml.bak)",
                    tilde(&self.opts.path)
                ))
                .default(false)
                .interact()
                .map_err(err)?;
            if !replace {
                eprintln!(
                    "Nothing changed. Edit it directly, or run `daypeek providers` to check it."
                );
                return Ok(Outcome::Cancelled);
            }
        }

        let mut answers = Answers::default();
        let ticks = preticked(&self.detected);
        let found = !self.detected.is_empty();
        self.step(
            "1/4",
            "Which apps do you use?",
            if found {
                "Press Space to select, Enter to continue. Apps found on this Mac are pre-ticked."
            } else {
                "Press Space to select, Enter to continue."
            },
        );
        let labels: Vec<String> = APPS
            .iter()
            .zip(&ticks)
            .map(|(a, &found)| {
                let mut l = a.name.to_string();
                if a.macos_only {
                    l.push_str(&format!(" {}", "(macOS)".dimmed()));
                }
                if found {
                    l.push_str(&format!(" {}", "· found".green()));
                }
                l
            })
            .collect();
        let mut picked = Vec::new();
        for attempt in 0..2 {
            picked = MultiSelect::with_theme(&self.theme)
                .with_prompt("Task apps")
                .items(&labels)
                .defaults(&ticks)
                .interact()
                .map_err(err)?;
            if !picked.is_empty() {
                break;
            }
            if attempt == 0 {
                eprintln!(
                    "  {} Nothing selected. Press {} to tick an app, then Enter. (Enter again with nothing ticked to skip task apps.)",
                    "!".yellow(),
                    "Space".bold()
                );
            } else {
                eprintln!(
                    "  {} No task apps for now. Run `daypeek setup` again any time to add one.",
                    "–".dimmed()
                );
            }
        }
        for i in picked {
            let app = &APPS[i];
            if app.macos_only && !cfg!(target_os = "macos") && !self.opts.demo {
                eprintln!("  {} {} needs macOS, skipping.", "–".dimmed(), app.name);
                continue;
            }
            if let Some(c) = self.app(app)? {
                answers.apps.push(c);
            }
        }

        self.step(
            "2/4",
            "Calendars",
            "An iCal link (Google: Settings → your calendar → Integrate calendar →\n    \"Secret address in iCal format\"), or a CalDAV account (iCloud, Fastmail, Nextcloud).",
        );
        loop {
            let n = answers.calendars.len() + answers.caldav.len();
            let kind = Select::with_theme(&self.theme)
                .with_prompt(if n == 0 {
                    "Add a calendar?"
                } else {
                    "Add another calendar?"
                })
                .items(&[
                    "iCal link        (.ics or webcal://)",
                    "CalDAV account   (iCloud, Fastmail, Nextcloud, …)",
                    if n == 0 { "Skip" } else { "Done" },
                ])
                .default(if n == 0 { 0 } else { 2 })
                .interact()
                .map_err(err)?;
            match kind {
                0 => {}
                1 => {
                    let taken: Vec<String> = answers
                        .calendars
                        .iter()
                        .map(|c| c.0.clone())
                        .chain(answers.caldav.iter().map(|c| c.name.clone()))
                        .collect();
                    if let Some(c) = self.caldav(&taken)? {
                        answers.caldav.push(c);
                    }
                    continue;
                }
                _ => break,
            }
            let url: String = Input::with_theme(&self.theme)
                .with_prompt("iCal URL (Enter to go back)")
                .allow_empty(true)
                .validate_with(|s: &String| -> Result<(), &str> {
                    if s.trim().is_empty() || setup::looks_like_calendar(s) {
                        Ok(())
                    } else {
                        Err("that doesn't look like an https://, webcal://, or file path")
                    }
                })
                .interact_text()
                .map_err(err)?;
            let url = url.trim().to_string();
            if url.is_empty() {
                continue;
            }
            let name: String = Input::with_theme(&self.theme)
                .with_prompt("Name for this calendar")
                .default(setup::guess_calendar_name(&url, n))
                .interact_text()
                .map_err(err)?;
            answers.calendars.push((name, url));
        }

        self.step("3/4", "Weather", "Free from Open-Meteo, no key needed.");
        let tz = crate::snapshot::system_tz();
        let guess = detect::city_from_timezone(tz.name());
        let mut city_prompt = Input::<String>::with_theme(&self.theme).allow_empty(true);
        city_prompt = match &guess {
            Some(g) => city_prompt
                .with_prompt(format!(
                    "Your city (Enter = {g}, from your time zone; `none` skips)"
                ))
                .default(g.clone())
                .show_default(false),
            None => city_prompt.with_prompt("Your city (Enter to skip)"),
        };
        let city = city_prompt.interact_text().map_err(err)?;
        if let Some(city) = city_answer(&city) {
            let metric_first = !tz.name().starts_with("America/");
            let u = Select::with_theme(&self.theme)
                .with_prompt("Units")
                .items(&["°F  (imperial)", "°C  (metric)"])
                .default(usize::from(metric_first))
                .interact()
                .map_err(err)?;
            answers.units = if u == 0 {
                Units::Imperial
            } else {
                Units::Metric
            };
            answers.city = Some(city);
        }

        let text = setup::render_config(&answers);
        self.step("4/4", "Save and check", "");
        if self.opts.demo {
            eprintln!(
                "  {} would write {} ({}, {}{})",
                "✓".green(),
                tilde(&self.opts.path),
                plural(answers.apps.len(), "app"),
                plural(answers.calendars.len() + answers.caldav.len(), "calendar"),
                if answers.city.is_some() {
                    ", weather"
                } else {
                    ""
                }
            );
        } else {
            write_config(&self.opts.path, &text)?;
            eprintln!("  {} wrote {}", "✓".green(), tilde(&self.opts.path));
        }

        if !self.exports.is_empty() {
            eprintln!(
                "\n  Add these to your shell profile (~/.zshrc, ~/.bashrc, or fish config), then open a new terminal:"
            );
            for e in &self.exports {
                eprintln!("    {}", e.cyan());
            }
        }
        if !self.later.is_empty() {
            eprintln!("\n  Still to do:");
            for l in &self.later {
                eprintln!("    {}", l.cyan());
            }
        }
        eprintln!();
        Ok(if self.opts.demo {
            Outcome::Demo(answers)
        } else {
            Outcome::Written
        })
    }

    fn step(&self, n: &str, title: &str, note: &str) {
        eprintln!("\n{} {}", format!("[{n}]").cyan().bold(), title.bold());
        if !note.is_empty() {
            eprintln!("    {}", note.dimmed());
        }
    }

    fn app(&mut self, app: &App) -> Result<Option<AppConfig>, String> {
        eprintln!("\n  {} {}", "▸".cyan(), app.name.bold());
        eprintln!("    {}", app.hint);
        eprintln!("    {}", app.link.dimmed().underline());
        let theme = ColorfulTheme::default();
        let t = &theme;
        let cfg = match app.id {
            "todoist" => {
                self.secret(app.id, "token", "DAYPEEK_TODOIST_TOKEN", "API token")?;
                AppConfig::Todoist
            }
            "notion" => {
                self.secret(app.id, "token", "NOTION_TOKEN", "API token")?;
                let raw: String = Input::with_theme(t)
                    .with_prompt("Database link or ID")
                    .validate_with(|s: &String| -> Result<(), &str> {
                        notion_id(s)
                            .map(|_| ())
                            .ok_or("paste the database's URL or its 32-character ID")
                    })
                    .interact_text()
                    .map_err(err)?;
                let database_id = notion_id(&raw).unwrap_or_default();
                let ask = |p: &str, d: &str| -> Result<String, String> {
                    Input::with_theme(t)
                        .with_prompt(p)
                        .default(d.to_string())
                        .interact_text()
                        .map_err(err)
                };
                AppConfig::Notion {
                    database_id,
                    title_property: ask("Title column", "Name")?,
                    date_property: ask("Date column", "Due")?,
                    done_property: ask("Done checkbox column", "Done")?,
                }
            }
            "ticktick" | "mstodo" | "gtasks" => {
                let client_id: String = Input::with_theme(t)
                    .with_prompt("Client ID")
                    .interact_text()
                    .map_err(err)?;
                let client_id = client_id.trim().to_string();
                let tenant = if app.id == "mstodo" {
                    let opts = ["common", "consumers", "organizations"];
                    let i = Select::with_theme(t)
                        .with_prompt("Accounts")
                        .items(&[
                            "common         personal and work/school",
                            "consumers      personal (Outlook.com) only",
                            "organizations  work/school only",
                        ])
                        .default(0)
                        .interact()
                        .map_err(err)?;
                    opts[i].to_string()
                } else {
                    "common".to_string()
                };
                let secret = match app.id {
                    // Google needs the secret on every refresh: keep it.
                    "gtasks" => self.secret(
                        app.id,
                        "client_secret",
                        "GTASKS_CLIENT_SECRET",
                        "Client secret",
                    )?,
                    // TickTick only needs it once, for the sign-in below.
                    "ticktick" => Some(
                        Password::with_theme(t)
                            .with_prompt("Client secret (hidden, used once for sign-in)")
                            .allow_empty_password(true)
                            .interact()
                            .map_err(err)?,
                    )
                    .filter(|s| !s.is_empty()),
                    _ => None,
                };
                self.sign_in(app, &client_id, secret.as_deref(), &tenant)?;
                match app.id {
                    "ticktick" => AppConfig::TickTick { client_id },
                    "mstodo" => AppConfig::MsTodo { client_id, tenant },
                    _ => AppConfig::GTasks { client_id },
                }
            }
            "linear" => {
                self.secret(app.id, "token", "LINEAR_API_KEY", "Personal API key")?;
                AppConfig::Linear
            }
            "asana" => {
                self.secret(app.id, "token", "ASANA_TOKEN", "Personal access token")?;
                let workspace: String = Input::with_theme(t)
                    .with_prompt("Workspace name (Enter = all your workspaces)")
                    .allow_empty(true)
                    .interact_text()
                    .map_err(err)?;
                AppConfig::Asana {
                    workspace: workspace.trim().to_string(),
                }
            }
            "trello" => {
                self.secret(app.id, "api_key", "TRELLO_API_KEY", "API key")?;
                eprintln!(
                    "    Next to the API key, click {} → Allow, and copy the token.",
                    "Token".bold()
                );
                self.secret(app.id, "token", "TRELLO_TOKEN", "Token")?;
                let raw: String = Input::with_theme(t)
                    .with_prompt(
                        "Boards (comma-separated; Enter = cards assigned to you on any board)",
                    )
                    .allow_empty(true)
                    .interact_text()
                    .map_err(err)?;
                AppConfig::Trello {
                    boards: setup::split_list(&raw),
                }
            }
            "things" => AppConfig::Things,
            "reminders" => AppConfig::Reminders,
            "obsidian" => {
                let found = self.detected.vaults.first().cloned();
                let default_vault = found
                    .as_ref()
                    .map(|v| tilde(&v.path))
                    .unwrap_or_else(|| "~/Documents/Obsidian".into());
                let vault = self.path_prompt("Vault folder", &default_vault, true)?;
                let same = found
                    .as_ref()
                    .filter(|v| expand_tilde(&vault) == v.path)
                    .cloned();
                let daily = match &same {
                    Some(v) => v.daily_notes_folder.clone(),
                    None if expand_tilde(&vault).join("Daily").is_dir() => "Daily".into(),
                    None => String::new(),
                };
                let daily_notes_folder: String = Input::with_theme(t)
                    .with_prompt("Daily notes folder inside the vault (Enter = vault root)")
                    .default(daily.to_string())
                    .allow_empty(true)
                    .interact_text()
                    .map_err(err)?;
                AppConfig::Obsidian {
                    vault,
                    daily_notes_folder,
                    daily_note_format: same
                        .map(|v| v.daily_note_format)
                        .filter(|f| f != "%Y-%m-%d"),
                }
            }
            "todotxt" => AppConfig::TodoTxt {
                path: self.path_prompt("todo.txt file", "~/todo.txt", false)?,
            },
            "markdown" => {
                let raw: String = Input::with_theme(t)
                    .with_prompt("Files or folders (comma-separated)")
                    .default("~/notes/tasks.md".into())
                    .interact_text()
                    .map_err(err)?;
                AppConfig::Markdown {
                    paths: setup::split_list(&raw),
                }
            }
            _ => return Ok(None),
        };
        Ok(Some(cfg))
    }

    /// One CalDAV account: server, username, app password (to the Keychain),
    /// then optionally look up its calendars and pick some.
    fn caldav(&mut self, taken: &[String]) -> Result<Option<setup::CalDavAnswer>, String> {
        let theme = ColorfulTheme::default();
        let t = &theme;
        let services = ["iCloud", "Fastmail", "Nextcloud", "Other CalDAV server"];
        let i = Select::with_theme(t)
            .with_prompt("Which service?")
            .items(&services)
            .default(0)
            .interact()
            .map_err(err)?;
        let (hint, user_hint) = match i {
            0 => (
                "Make an app-specific password: account.apple.com → Sign-In and Security →\n    App-Specific Passwords → \"+\". (Your normal Apple password won't work.)",
                "Apple Account email",
            ),
            1 => (
                "Make an app password: Settings → Privacy & Security → Manage app passwords\n    and access → New app password, with access to Calendars (CalDAV).",
                "Fastmail email address",
            ),
            2 => (
                "Make an app password: your avatar → Settings → Security → Devices & sessions\n    → Create new app password.",
                "Nextcloud username",
            ),
            _ => (
                "Use your server's CalDAV address and, if it offers one, an app password.",
                "Username",
            ),
        };
        eprintln!("    {}", hint.dimmed());
        let url = match i {
            0 | 1 => setup::CALDAV_PRESETS[i].1.to_string(),
            2 => {
                let host: String = Input::with_theme(t)
                    .with_prompt("Nextcloud address (e.g. https://cloud.example.com)")
                    .validate_with(|s: &String| -> Result<(), &str> {
                        if s.trim().starts_with("https://") || s.trim().starts_with("http://") {
                            Ok(())
                        } else {
                            Err("start with https://")
                        }
                    })
                    .interact_text()
                    .map_err(err)?;
                setup::nextcloud_dav_url(&host)
            }
            _ => Input::with_theme(t)
                .with_prompt("CalDAV URL")
                .validate_with(|s: &String| -> Result<(), &str> {
                    if s.trim().starts_with("https://") || s.trim().starts_with("http://") {
                        Ok(())
                    } else {
                        Err("start with https://")
                    }
                })
                .interact_text()
                .map_err(err)?
                .trim()
                .to_string(),
        };
        let default_name = match i {
            0..=2 => services[i].to_string(),
            _ => "CalDAV".to_string(),
        };
        let default_name = setup::unique_name(&default_name, taken);
        let name: String = Input::with_theme(t)
            .with_prompt("Name for this calendar")
            .default(default_name)
            .interact_text()
            .map_err(err)?;
        let username: String = Input::with_theme(t)
            .with_prompt(user_hint)
            .interact_text()
            .map_err(err)?;
        let username = username.trim().to_string();
        let id = crate::config::slug(&name);
        let env = crate::config::default_password_env(&id);
        let mut password = self.secret(&id, "token", &env, "App password")?;
        if password.is_none() && !self.keychain {
            // No Keychain (Linux): use it once to list calendars, keep nothing.
            password = Some(
                Password::with_theme(t)
                    .with_prompt(
                        "App password, used once to list your calendars (hidden, Enter to skip)",
                    )
                    .allow_empty_password(true)
                    .interact()
                    .map_err(err)?,
            )
            .filter(|p| !p.trim().is_empty());
        }
        let calendars = match password {
            Some(p) => self.pick_calendars(&url, &username, &p)?,
            None => Vec::new(),
        };
        Ok(Some(setup::CalDavAnswer {
            name,
            url,
            username,
            calendars,
        }))
    }

    /// Look up the account's calendars and let the user untick some.
    /// Returns the picked names, or empty for "all".
    fn pick_calendars(
        &self,
        url: &str,
        username: &str,
        password: &str,
    ) -> Result<Vec<String>, String> {
        eprintln!("    {} looking up your calendars…", "→".cyan());
        let names: Vec<String> = if self.opts.demo {
            vec!["Home".into(), "Work".into(), "Family".into()]
        } else {
            let http = Http::new(Duration::from_secs(15));
            let acct = crate::calendar::caldav::Account {
                url,
                username,
                password,
            };
            match crate::calendar::caldav::discover(&http, &acct) {
                Ok(found) => found.into_iter().map(|c| c.name).collect(),
                Err(e) => {
                    eprintln!(
                        "    {} {e}\n      (saved anyway; fix it later and check with `daypeek providers`)",
                        "!".red()
                    );
                    return Ok(Vec::new());
                }
            }
        };
        if names.is_empty() {
            eprintln!("    {} no calendars with events found", "!".yellow());
            return Ok(Vec::new());
        }
        let picked = MultiSelect::with_theme(&self.theme)
            .with_prompt("Show which calendars? (Space toggles)")
            .items(&names)
            .defaults(&vec![true; names.len()])
            .interact()
            .map_err(err)?;
        if picked.len() == names.len() || picked.is_empty() {
            Ok(Vec::new())
        } else {
            Ok(picked.into_iter().map(|i| names[i].clone()).collect())
        }
    }

    /// Ask for a secret with hidden input and keep it in the Keychain
    /// (macOS), or tell the user which env var to set (Linux). Returns the
    /// value typed, if any.
    fn secret(
        &mut self,
        instance: &str,
        name: &str,
        env: &str,
        label: &str,
    ) -> Result<Option<String>, String> {
        let service = secrets::keychain_service(instance, name);
        if !self.keychain {
            eprintln!("    Linux: keep it in an env var instead of the config file.");
            self.exports
                .push(format!("export {env}='<your {}>'", label.to_lowercase()));
            return Ok(None);
        }
        let value = Password::with_theme(&self.theme)
            .with_prompt(format!("{label} (hidden, Enter to skip)"))
            .allow_empty_password(true)
            .interact()
            .map_err(err)?;
        let value = value.trim().to_string();
        if value.is_empty() {
            self.later.push(format!(
                "security add-generic-password -U -s {service} -a \"$USER\" -w"
            ));
            return Ok(None);
        }
        self.keep(&service, env, &value);
        Ok(Some(value))
    }

    /// Store a secret, or fall back to telling the user what to set.
    fn keep(&mut self, service: &str, env: &str, value: &str) {
        if self.opts.demo {
            eprintln!(
                "    {} would save to Keychain item {}",
                "✓".green(),
                service.bold()
            );
            return;
        }
        if !cfg!(target_os = "macos") {
            eprintln!("    {} store it in ${env}", "→".cyan());
            self.exports
                .push(format!("export {env}='…'   # the value printed above"));
            return;
        }
        match secrets::store_keychain(service, value) {
            Ok(()) => eprintln!(
                "    {} saved to Keychain item {}",
                "✓".green(),
                service.bold()
            ),
            Err(e) => {
                eprintln!("    {} {e}", "!".red());
                self.later.push(format!(
                    "security add-generic-password -U -s {service} -a \"$USER\" -w"
                ));
            }
        }
    }

    fn sign_in(
        &mut self,
        app: &App,
        client_id: &str,
        secret: Option<&str>,
        tenant: &str,
    ) -> Result<(), String> {
        let later = format!("daypeek auth {}", app.id);
        if client_id.is_empty() {
            self.later.push(later);
            return Ok(());
        }
        if self.opts.demo {
            eprintln!("    {} (demo) skipping the browser sign-in", "–".dimmed());
            return Ok(());
        }
        if app.id != "mstodo" && secret.is_none() {
            self.later.push(later);
            return Ok(());
        }
        let go = Confirm::with_theme(&self.theme)
            .with_prompt("Sign in now? (opens your browser)")
            .default(true)
            .interact()
            .map_err(err)?;
        if !go {
            self.later.push(later);
            return Ok(());
        }
        let Some(flow) = oauth::flow_for(app.id, tenant) else {
            return Ok(());
        };
        let http = Http::new(Duration::from_secs(15));
        match oauth::loopback(&http, &flow, client_id, secret, self.opts.port) {
            Ok(tok) => {
                let (name, value, env) = oauth::secret_to_store(app.id, &tok);
                if cfg!(target_os = "macos") {
                    self.keep(&secrets::keychain_service(app.id, name), &env, &value);
                } else {
                    // There's no keychain to put it in, so show it once.
                    eprintln!("    {} signed in. Your {name}:\n      {value}", "✓".green());
                    self.exports
                        .push(format!("export {env}='…'   # the {name} printed above"));
                }
            }
            Err(e) => {
                eprintln!("    {} sign-in failed: {e}", "!".red());
                self.later.push(later);
            }
        }
        Ok(())
    }

    fn path_prompt(&self, prompt: &str, default: &str, dir: bool) -> Result<String, String> {
        let demo = self.opts.demo;
        Input::with_theme(&self.theme)
            .with_prompt(prompt)
            .default(default.to_string())
            .validate_with(move |s: &String| -> Result<(), String> {
                let p = expand_tilde(s.trim());
                let ok = if dir { p.is_dir() } else { p.is_file() };
                if ok || demo {
                    Ok(())
                } else {
                    Err(format!("{} not found", p.display()))
                }
            })
            .interact_text()
            .map(|s| s.trim().to_string())
            .map_err(err)
    }
}

/// Pull a Notion database ID out of a pasted URL or ID.
pub fn notion_id(s: &str) -> Option<String> {
    let s = s.trim();
    let s = s.split(['?', '#']).next().unwrap_or(s);
    let last = s.rsplit('/').next().unwrap_or(s);
    let hex: String = last.chars().filter(|c| *c != '-').collect();
    let tail = hex.get(hex.len().checked_sub(32)?..)?;
    tail.chars()
        .all(|c| c.is_ascii_hexdigit())
        .then(|| tail.to_ascii_lowercase())
}

/// Write the config with owner-only permissions, keeping a backup of any
/// existing file.
pub fn write_config(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    if path.exists() {
        let bak = path.with_extension("toml.bak");
        std::fs::copy(path, &bak).map_err(|e| format!("{}: {e}", bak.display()))?;
    }
    // Created owner-only from the first byte (it may hold a client id or a
    // token someone pasted in), and tightened if it already existed.
    let mut open = std::fs::OpenOptions::new();
    open.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open.mode(0o600);
    }
    let mut file = open
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    std::io::Write::write_all(&mut file, text.as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notion_ids_from_urls_and_ids() {
        let id = "a1b2c3d4e5f60718293a4b5c6d7e8f90";
        assert_eq!(notion_id(id).as_deref(), Some(id));
        assert_eq!(
            notion_id(&format!("https://www.notion.so/acme/Tasks-{id}?v=123abc")).as_deref(),
            Some(id)
        );
        assert_eq!(
            notion_id("a1b2c3d4-e5f6-0718-293a-4b5c6d7e8f90").as_deref(),
            Some(id)
        );
        assert_eq!(notion_id("not an id"), None);
        assert_eq!(notion_id(""), None);
    }

    #[test]
    fn write_config_backs_up_and_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/config.toml");
        write_config(&path, "a = 1\n").unwrap();
        write_config(&path, "a = 2\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a = 2\n");
        assert_eq!(
            std::fs::read_to_string(path.with_extension("toml.bak")).unwrap(),
            "a = 1\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn detected_apps_are_preticked() {
        let none = preticked(&Detected::default());
        assert!(none.iter().all(|t| !t));
        let ticks = preticked(&demo_detected());
        let on: Vec<&str> = APPS
            .iter()
            .zip(&ticks)
            .filter(|(_, t)| **t)
            .map(|(a, _)| a.id)
            .collect();
        assert_eq!(on, vec!["things", "obsidian"]);
    }

    #[test]
    fn city_answers() {
        assert_eq!(city_answer(" New York "), Some("New York".into()));
        assert_eq!(city_answer(""), None);
        assert_eq!(city_answer("none"), None);
        assert_eq!(city_answer("-"), None);
    }

    #[test]
    fn write_config_is_private_even_when_replacing_a_public_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "old").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            write_config(&path, "new").unwrap();
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn refuses_without_a_terminal() {
        // `cargo test` runs without a TTY on stdin.
        if std::io::stdin().is_terminal() {
            return;
        }
        let r = run(Options {
            path: PathBuf::from("/nonexistent/config.toml"),
            demo: true,
            port: 8765,
        });
        assert!(r.unwrap_err().contains("needs a terminal"));
    }
}

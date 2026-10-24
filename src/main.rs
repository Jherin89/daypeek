use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand, ValueEnum};
use colored::Colorize;

use daypeek::cache::Cache;
use daypeek::config::{self, Config};
use daypeek::http::Http;
use daypeek::providers::{self, Ctx, Health};
use daypeek::render::{self, Style};
use daypeek::snapshot::Snapshot;
use daypeek::snapshot::View;
use daypeek::{calendar, demo, detect, done, oauth, secrets, snapshot, watch, weather};

/// A fast one-screen daily dashboard: calendar, tasks from every app, weather.
#[derive(Parser)]
#[command(name = "daypeek", version, about, long_about = None)]
struct Cli {
    /// Short 2–3 line summary (for shell startup)
    #[arg(long, conflicts_with = "json", global = true)]
    compact: bool,

    /// Machine-readable JSON
    #[arg(long, global = true)]
    json: bool,

    /// Show fake demo data (no accounts or network needed)
    #[arg(long, global = true)]
    demo: bool,

    /// Don't touch the network; show cached data
    #[arg(long, global = true)]
    offline: bool,

    /// Fetch everything now instead of reusing results from the last few
    /// minutes (see `cache_minutes`)
    #[arg(long, conflicts_with = "offline", global = true)]
    refresh: bool,

    /// Keep the dashboard open and redraw it in place (q or Ctrl-C quits,
    /// r refreshes)
    #[arg(long, conflicts_with = "json", global = true)]
    watch: bool,

    /// With --watch: seconds between redraws [default: 60]
    #[arg(
        long,
        value_name = "SECS",
        requires = "watch",
        global = true,
        value_parser = clap::value_parser!(u64).range(watch::MIN_EVERY..=86_400)
    )]
    every: Option<u64>,

    /// With --demo: show the offline / stale-cache state
    #[arg(long, requires = "demo", hide = true, global = true)]
    demo_offline: bool,

    /// With --demo: pretend Todoist and TickTick allow writes (task numbers)
    #[arg(long, requires = "demo", hide = true, global = true)]
    demo_write: bool,

    /// Write a commented sample config to ~/.config/daypeek/config.toml
    #[arg(long)]
    init: bool,

    /// With --init: overwrite an existing config (the old one is kept as .bak)
    #[arg(long, requires = "init")]
    force: bool,

    /// Config file [default: ~/.config/daypeek/config.toml, or $DAYPEEK_CONFIG]
    #[arg(long, value_name = "PATH", global = true)]
    config: Option<PathBuf>,

    /// When to use color
    #[arg(long, value_enum, default_value_t = ColorWhen::Auto, global = true)]
    color: ColorWhen,

    /// Pretend it is this instant (RFC 3339) — for screenshots and tests
    #[arg(long, value_name = "RFC3339", hide = true, global = true)]
    at: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Today's dashboard (the default when you run plain `daypeek`)
    Today,
    /// Tomorrow: events, tasks due tomorrow, and the forecast
    Tomorrow,
    /// The next seven days, one block per day
    Week,
    /// Guided first-run setup: pick your apps, paste tokens, write the config
    Setup {
        /// Loopback port for browser sign-in (TickTick, Microsoft, Google)
        #[arg(long, default_value_t = 8765)]
        port: u16,
    },
    /// Complete task <N> (the number shown next to it). Off unless
    /// `allow_write = true` is in your config; asks before changing anything
    Done {
        /// The task's number on the dashboard
        number: usize,
        /// Don't ask for confirmation
        #[arg(long, short)]
        yes: bool,
    },
    /// List configured providers and check that each one works
    Providers,
    /// One-time OAuth helper for ticktick, mstodo, or gtasks (bring your own app)
    Auth {
        /// Provider id from your config, e.g. `gtasks`
        provider: String,
        /// Loopback port. Redirect URI: http://127.0.0.1:<port>/callback
        /// (Microsoft: http://localhost/callback)
        #[arg(long, default_value_t = 8765)]
        port: u16,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ColorWhen {
    Auto,
    Always,
    Never,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.color {
        ColorWhen::Always => colored::control::set_override(true),
        ColorWhen::Never => colored::control::set_override(false),
        ColorWhen::Auto => {
            if !std::io::stdout().is_terminal() || std::env::var_os("NO_COLOR").is_some() {
                colored::control::set_override(false);
            }
        }
    }
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("daypeek: {msg}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    let path = cli
        .config
        .clone()
        .unwrap_or_else(config::default_config_path);

    if cli.init {
        return init(&path, cli.force);
    }
    if let Some(Command::Setup { port }) = cli.command {
        return setup_cmd(path, cli.demo, port);
    }

    let now: DateTime<Utc> = match &cli.at {
        Some(s) => DateTime::parse_from_rfc3339(s)
            .map_err(|e| format!("--at: {e}"))?
            .with_timezone(&Utc),
        None => Utc::now(),
    };

    // A broken config must not break your shell prompt: warn and use defaults.
    let have_config = path.exists();
    let mut cfg = match Config::load(&path) {
        Ok(c) => c,
        Err(_) if cli.demo => Config::default(),
        Err(e) => {
            eprintln!("daypeek: {e} (using defaults)");
            Config::default()
        }
    };
    if !cli.demo {
        if let Some(w) = snapshot::timezone_warning(&cfg) {
            eprintln!("daypeek: {w}");
        }
    }

    // No config at all: use whatever lives on this Mac (Things, Obsidian).
    let mut found = None;
    if !have_config && !cli.demo {
        let d = detect::detect();
        if !d.is_empty() {
            cfg = d.to_config();
            found = Some(d.summary());
        }
    }

    if cli.watch
        && matches!(
            cli.command,
            Some(Command::Providers | Command::Auth { .. } | Command::Done { .. })
        )
    {
        return Err("--watch works with the dashboard: today, tomorrow, or week".into());
    }
    let view = match cli.command {
        Some(Command::Providers) if cli.demo => {
            return print_providers(demo::provider_rows(), cli.json)
        }
        Some(Command::Providers) => return providers_cmd(&cfg, cli.json),
        Some(Command::Auth { provider, port }) => return auth_cmd(&cfg, &provider, port),
        Some(Command::Done { number, yes }) => {
            return done_cmd(&cfg, number, yes, now, cli.demo, cli.offline)
        }
        Some(Command::Tomorrow) => View::Tomorrow,
        Some(Command::Week) => View::Week,
        Some(Command::Today) | Some(Command::Setup { .. }) | None => View::Today,
    };

    let sources = Sources {
        cfg: &cfg,
        demo: cli.demo,
        demo_offline: cli.demo_offline,
        demo_write: cli.demo_write,
        offline: cli.offline,
    };
    if cli.watch {
        let every = Duration::from_secs(cli.every.unwrap_or(watch::DEFAULT_EVERY));
        // With --at (screenshots), the clock starts there and keeps ticking.
        let start = cli.at.as_ref().map(|_| now);
        return watch_cmd(&sources, view, every, start, cli.compact, cli.refresh);
    }
    let reuse = if cli.refresh {
        None
    } else {
        // Shell-startup runs reuse results from the last few minutes.
        Some(chrono::Duration::minutes(
            i64::try_from(cfg.general.cache_minutes.min(7 * 24 * 60)).unwrap_or(0),
        ))
    };
    let snap = sources.snapshot(now, view, reuse);

    if cli.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&snap).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    let width = terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .or_else(|| std::env::var("COLUMNS").ok()?.parse().ok())
        .unwrap_or(80);
    let style = style_for(&cfg, width);
    if cli.compact {
        println!("{}", render::compact(&snap, &style));
    } else {
        println!("{}", render::full(&snap, &style));
        remember_numbers(&cfg, &snap, &style);
        if let Some(found) = found {
            println!(
                "  {}\n",
                format!(
                    "Found {found} on this Mac. Run `daypeek setup` to add calendars and weather."
                )
                .dimmed()
            );
        }
    }
    Ok(())
}

/// `daypeek done <n>`: complete a numbered task after confirming.
/// Where the dashboard's data comes from (real sources or demo data).
struct Sources<'a> {
    cfg: &'a Config,
    demo: bool,
    demo_offline: bool,
    demo_write: bool,
    offline: bool,
}

impl Sources<'_> {
    /// Gather a snapshot. `reuse`: cloud results this fresh are taken from
    /// the cache (never ones from before local midnight); `None` fetches all.
    fn snapshot(
        &self,
        now: DateTime<Utc>,
        view: View,
        reuse: Option<chrono::Duration>,
    ) -> Snapshot {
        let cfg = self.cfg;
        let tz = snapshot::resolve_tz(cfg);
        if self.demo {
            let mut s = if self.demo_offline {
                demo::offline_snapshot(now.with_timezone(&tz), view)
            } else {
                demo::snapshot(now.with_timezone(&tz), view)
            };
            if self.demo_write {
                s.writable = vec!["todoist".into(), "ticktick".into()];
            }
            return s;
        }
        let today = now.with_timezone(&tz).date_naive();
        let mut cache = Cache::new(cfg.cache_dir(), self.offline);
        if let Some(window) = reuse {
            cache = cache.reuse_recent(window, Some(calendar::day_window(today, tz).0));
        }
        snapshot::gather(cfg, &cache, now, view)
    }
}

fn style_for(cfg: &Config, width: usize) -> Style {
    Style {
        width: width.min(96),
        time_format: cfg.general.time_format,
        max_tasks: cfg.general.max_tasks,
    }
}

/// Remember the numbers just drawn, for `daypeek done <n>`.
fn remember_numbers(cfg: &Config, snap: &Snapshot, style: &Style) {
    if !snap.demo && !snap.writable.is_empty() {
        let list = done::number(snap, style.max_tasks);
        done::save(&Cache::new(cfg.cache_dir(), false), &list);
    }
}

/// `--watch`: redraw every `every`, reusing cloud data per `cache_minutes`
/// (at least a minute); `r` fetches everything.
fn watch_cmd(
    src: &Sources,
    view: View,
    every: Duration,
    start_at: Option<DateTime<Utc>>,
    compact: bool,
    refresh_first: bool,
) -> Result<(), String> {
    let cfg = src.cfg;
    let started = std::time::Instant::now();
    let clock_now = || match start_at {
        Some(at) => at + chrono::Duration::from_std(started.elapsed()).unwrap_or_default(),
        None => Utc::now(),
    };
    let tz = snapshot::resolve_tz(cfg);
    let window = watch::min_refetch(cfg.general.cache_minutes);
    let mut first = true;
    let fetch = |force: bool| {
        let now = clock_now();
        let all = force || (first && refresh_first);
        first = false;
        src.snapshot(now, view, if all { None } else { Some(window) })
    };
    let draw = |snap: &Snapshot, width: usize| {
        let style = style_for(cfg, width);
        if compact {
            format!("\n{}", render::compact(snap, &style))
        } else {
            remember_numbers(cfg, snap, &style);
            render::full(snap, &style)
        }
    };
    let clock = match cfg.general.time_format {
        config::TimeFormat::H12 => "%-I:%M %p",
        config::TimeFormat::H24 => "%H:%M",
    };
    let secs = every.as_secs();
    let footer = |_width: usize| {
        let now = clock_now().with_timezone(&tz);
        format!(
            "  {}",
            format!(
                "updated {} · every {} · r refresh · q quit",
                now.format(clock),
                if secs % 60 == 0 {
                    format!("{}m", secs / 60)
                } else {
                    format!("{secs}s")
                }
            )
            .dimmed()
        )
    };
    watch::run(every, fetch, draw, footer)
}

fn done_cmd(
    cfg: &Config,
    n: usize,
    yes: bool,
    now: DateTime<Utc>,
    demo_mode: bool,
    offline: bool,
) -> Result<(), String> {
    let tz = snapshot::resolve_tz(cfg);
    if demo_mode {
        let mut s = demo::snapshot(now.with_timezone(&tz), View::Today);
        s.writable = vec!["todoist".into(), "ticktick".into()];
        let list = done::number(&s, cfg.general.max_tasks);
        let item = done::pick(&list, n)?;
        let apps: Vec<&str> = item
            .refs
            .iter()
            .map(|r| {
                providers::REGISTRY
                    .iter()
                    .find(|p| p.0 == r.source)
                    .map_or(r.source.as_str(), |p| p.1)
            })
            .collect();
        println!(
            "{} Would complete \"{}\" in {}. Nothing was changed (demo data).",
            "(demo)".magenta(),
            item.title,
            apps.join(" and ")
        );
        return Ok(());
    }
    if offline {
        return Err("`done` needs the network (drop --offline)".into());
    }
    if !done::any_writable(cfg) {
        return Err(done::READ_ONLY.into());
    }
    let today = now.with_timezone(&tz).date_naive();
    let midnight = calendar::day_window(today, tz).0;
    let minutes = i64::try_from(cfg.general.cache_minutes).unwrap_or(0);
    let cache = Cache::new(cfg.cache_dir(), false).reuse_recent(
        chrono::Duration::minutes(minutes.min(7 * 24 * 60)),
        Some(midnight),
    );
    // The numbers you last saw; if there are none from today, number the
    // current list the same way the dashboard would.
    let list = match done::load(&cache, midnight) {
        Some(list) => list,
        None => {
            let snap = snapshot::gather(cfg, &cache, now, View::Today);
            done::number(&snap, cfg.general.max_tasks)
        }
    };
    let item = done::pick(&list, n)?.clone();
    let targets = done::targets(cfg, &item);
    if targets.is_empty() {
        return Err(format!(
            "\"{}\" comes from an app daypeek can't change. {}",
            item.title,
            done::READ_ONLY.lines().next().unwrap_or_default()
        ));
    }
    let names: Vec<&str> = targets.iter().map(|(_, name)| name.as_str()).collect();
    if !yes {
        if !std::io::stdin().is_terminal() {
            return Err(format!(
                "`done` asks before changing anything. To complete \"{}\" in {} without asking, add --yes.",
                item.title,
                names.join(" and ")
            ));
        }
        let ok = dialoguer::Confirm::new()
            .with_prompt(format!(
                "Complete \"{}\" in {}?",
                item.title,
                names.join(" and ")
            ))
            .default(false)
            .interact()
            .map_err(|e| format!("prompt: {e}"))?;
        if !ok {
            println!("Nothing changed.");
            return Ok(());
        }
    }
    let http = Http::new(Duration::from_secs(cfg.general.timeout_secs.max(1) + 4));
    let mut failed = Vec::new();
    for (name, res) in done::complete(cfg, &http, &cache, &item) {
        match res {
            Ok(()) => println!("{} Completed \"{}\" in {name}.", "✓".green(), item.title),
            Err(e) => failed.push(format!("{name}: {e}")),
        }
    }
    let read_only: Vec<&String> = item
        .sources
        .iter()
        .filter(|s| !item.refs.iter().any(|r| &r.source == *s))
        .collect();
    if !read_only.is_empty() {
        println!(
            "  {}",
            format!(
                "Also listed in {} (read-only): mark it done there too.",
                read_only
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .dimmed()
        );
    }
    if failed.is_empty() {
        done::forget_number(&cache, &list, n);
        Ok(())
    } else {
        let mut msg = failed.join("\n  ");
        if msg.contains("TickTick") && msg.contains("auth missing") {
            msg.push_str(
                "\n  TickTick needs a token with write access: run `daypeek auth ticktick` again.",
            );
        }
        Err(msg)
    }
}

fn init(path: &std::path::Path, force: bool) -> Result<(), String> {
    if path.exists() && !force {
        return Err(format!(
            "{} already exists (use --init --force to overwrite)",
            path.display()
        ));
    }
    // Owner-only (0600) like `daypeek setup`, with a .bak of any old file.
    daypeek::wizard::write_config(path, config::SAMPLE_CONFIG)?;
    println!("Wrote {}", path.display());
    println!(
        "Next: run `daypeek setup` for guided setup, or uncomment what you use and run `daypeek providers`."
    );
    Ok(())
}

fn setup_cmd(path: PathBuf, demo: bool, port: u16) -> Result<(), String> {
    use daypeek::wizard::{self, Options, Outcome};
    let outcome = wizard::run(Options {
        path: path.clone(),
        demo,
        port,
    })?;
    let rows = match outcome {
        Outcome::Cancelled => return Ok(()),
        Outcome::Demo(answers) => {
            if answers.apps.is_empty() && answers.calendars.is_empty() && answers.city.is_none() {
                nothing_configured();
                return Ok(());
            }
            demo::provider_rows_for(&answers)
        }
        Outcome::Written => {
            let cfg = Config::load(&path).map_err(|e| format!("config: {e}"))?;
            if cfg.calendars.is_empty() && cfg.providers.is_empty() && cfg.weather().is_none() {
                nothing_configured();
                return Ok(());
            }
            println!("Checking every source…");
            return providers_cmd(&cfg, false).map(|()| next_steps());
        }
    };
    println!("Checking every source…");
    print_providers(rows, false)?;
    next_steps();
    Ok(())
}

fn next_steps() {
    println!(
        "\n{}\n  {}             your day\n  {}        the next seven days\n  {}   see what's still missing\n  {}\n",
        "You're set. Next:".bold(),
        "daypeek".cyan(),
        "daypeek week".cyan(),
        "daypeek providers".cyan(),
        "echo 'daypeek --compact' >> ~/.zshrc   # show it in every new terminal".dimmed()
    );
}

/// Setup finished with nothing picked: say so plainly (no "You're set").
fn nothing_configured() {
    println!(
        "\n{}\n  {}   run it again when you're ready\n  {}  see what it looks like with sample data\n",
        "Nothing to show yet: no apps, calendars, or weather were added.".bold(),
        "daypeek setup".cyan(),
        "daypeek --demo".cyan(),
    );
}

fn providers_cmd(cfg: &Config, json: bool) -> Result<(), String> {
    let tz = snapshot::resolve_tz(cfg);
    let http = Http::new(Duration::from_secs(cfg.general.timeout_secs.max(1)));
    let today = Utc::now().with_timezone(&tz).date_naive();
    let ctx = Ctx {
        http: &http,
        today,
        until: today,
        tz,
    };
    let (built, broken) = providers::build_all(cfg);

    // Health checks run in parallel, like the dashboard.
    let mut rows: Vec<Row> = std::thread::scope(|s| {
        let handles: Vec<_> = built
            .iter()
            .map(|p| {
                let ctx = &ctx;
                s.spawn(move || {
                    (
                        p.id().to_string(),
                        p.name().to_string(),
                        format!("{:?}", p.kind()).to_lowercase(),
                        p.health(ctx),
                    )
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    for b in broken {
        rows.push((
            b.id,
            "?".into(),
            "-".into(),
            Health::Error(b.error.to_string()),
        ));
    }
    // Calendars are checked in parallel too (CalDAV takes a few requests).
    let cal_rows: Vec<Row> = std::thread::scope(|s| {
        let handles: Vec<_> = cfg
            .calendars
            .iter()
            .map(|c| {
                let http = &http;
                s.spawn(move || {
                    let h = match calendar::fetch_source(http, c, today, tz) {
                        Ok(text) => {
                            let n = calendar::events_on(&text, &c.name, today, tz).len();
                            Health::Ok(format!("{n} event{} today", if n == 1 { "" } else { "s" }))
                        }
                        Err(e) => providers::health_from_error(e),
                    };
                    let kind = if calendar::is_remote_source(c) {
                        "cloud"
                    } else {
                        "local"
                    };
                    let name = format!("{} calendar", calendar::source_label(c));
                    (c.name.clone(), name, kind.to_string(), h)
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    rows.extend(cal_rows);
    if let Some(w) = cfg.weather() {
        let cache = Cache::new(cfg.cache_dir(), false);
        let h = match weather::fetch(&http, &cache, w) {
            Ok(x) => Health::Ok(format!("{:.0}{} {}", x.temperature, x.unit, x.condition)),
            Err(e) => providers::health_from_error(e),
        };
        rows.push(("weather".into(), "Open-Meteo".into(), "cloud".into(), h));
    }

    print_providers(rows, json)
}

type Row = (String, String, String, Health);

fn print_providers(rows: Vec<Row>, json: bool) -> Result<(), String> {
    if json {
        let v: Vec<_> = rows
            .iter()
            .map(|(id, name, kind, h)| serde_json::json!({"id": id, "name": name, "kind": kind, "health": h}))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?
        );
        return Ok(());
    }
    if rows.is_empty() {
        println!("No sources configured yet. Run `daypeek setup` to add your apps, calendars and weather.");
        return Ok(());
    }
    println!(
        "  {:<14} {:<18} {:<7} {}",
        "ID".bold(),
        "SOURCE".bold(),
        "KIND".bold(),
        "STATUS".bold()
    );
    for (id, name, kind, h) in &rows {
        let status = match h {
            Health::Ok(m) => format!("{} {}", "✓ ok".green(), m.dimmed()),
            Health::AuthMissing(m) => format!("{} {}", "○ auth missing".yellow(), m.dimmed()),
            Health::Unsupported(m) => format!("{} {}", "– unsupported".dimmed(), m.dimmed()),
            Health::Error(m) => format!("{} {}", "✗ error".red(), m.dimmed()),
        };
        println!("  {:<14} {:<18} {:<7} {}", id, name, kind, status);
    }
    Ok(())
}

/// Why `daypeek auth <kind>` doesn't apply, for providers without OAuth.
fn auth_not_needed(id: &str, kind: &str) -> Option<String> {
    let token = |name: &str, env: &str, page: &str| {
        format!(
            "{name} doesn't use a browser sign-in: it uses a personal API token.\n  \
             Get one: {page}\n  \
             Then {}, or run `daypeek setup` and paste it when asked.",
            secrets::hint(id, "token", env)
        )
    };
    match kind {
        "todoist" => Some(token(
            "Todoist",
            providers::todoist::TOKEN_ENV,
            "Todoist → Settings → Integrations → Developer → API token",
        )),
        "notion" => Some(token(
            "Notion",
            providers::notion::TOKEN_ENV,
            "https://www.notion.so/profile/integrations (create an internal integration)",
        )),
        "linear" => Some(token(
            "Linear",
            providers::linear::TOKEN_ENV,
            "Linear → Settings → Security & access → Personal API keys",
        )),
        "asana" => Some(token(
            "Asana",
            providers::asana::TOKEN_ENV,
            "https://app.asana.com/0/my-apps → Create new token",
        )),
        "trello" => Some(format!(
            "Trello doesn't use a browser sign-in: it uses an API key and a token.\n  \
             Get them: create a Power-Up at https://trello.com/power-ups/admin → API key → Token.\n  \
             Then {} and {}, or run `daypeek setup`.",
            secrets::hint(id, "api_key", providers::trello::KEY_ENV),
            secrets::hint(id, "token", providers::trello::TOKEN_ENV)
        )),
        "things" | "reminders" | "obsidian" | "todotxt" | "markdown" => Some(format!(
            "{kind} reads data on this computer and needs no sign-in. Add it with `daypeek setup`, then check it with `daypeek providers`."
        )),
        _ => None,
    }
}

fn auth_cmd(cfg: &Config, id: &str, port: u16) -> Result<(), String> {
    let table = cfg.providers.get(id);
    let kind = table
        .and_then(|t| t.get("type"))
        .and_then(|v| v.as_str())
        .unwrap_or(id);
    if let Some(msg) = auth_not_needed(id, kind) {
        return Err(msg);
    }
    if !matches!(kind, "ticktick" | "mstodo" | "gtasks") {
        return Err(format!(
            "`daypeek auth` signs in to ticktick, mstodo, or gtasks; {kind:?} isn't one of them. See `daypeek providers`."
        ));
    }
    let table = table.ok_or_else(|| {
        format!(
            "no [providers.{id}] in your config yet. Run `daypeek setup` (it signs you in too), \
             or add the block from https://github.com/Jherin89/daypeek/blob/main/docs/SETUP.md#{kind}"
        )
    })?;
    let client_id_cfg = table.get("client_id").and_then(|v| v.as_str());
    let env_prefix = kind.to_uppercase();
    let client_id = std::env::var(format!("{env_prefix}_CLIENT_ID"))
        .ok()
        .or(client_id_cfg.map(str::to_string))
        .ok_or_else(|| {
            format!("set client_id in [providers.{id}] (from your own {kind} app registration)")
        })?;
    let secret = secrets::resolve(
        id,
        "client_secret",
        &format!("{env_prefix}_CLIENT_SECRET"),
        None,
    );
    let tenant = table
        .get("tenant")
        .and_then(|v| v.as_str())
        .unwrap_or("common");

    let mut flow = oauth::flow_for(kind, tenant)
        .ok_or_else(|| format!("`daypeek auth` supports ticktick, mstodo, gtasks (not {kind})"))?;
    if matches!(kind, "ticktick" | "gtasks") && secret.is_none() {
        return Err(format!(
            "{kind} needs your app's client secret: {}",
            secrets::hint(id, "client_secret", &format!("{env_prefix}_CLIENT_SECRET"))
        ));
    }
    if kind == "ticktick" && providers::write_allowed(cfg, id) {
        // `daypeek done` needs to change tasks, not just read them.
        flow.scope = "tasks:read tasks:write";
        println!("allow_write is on, so this asks TickTick for read and write access.");
    }
    let http = Http::new(Duration::from_secs(15));
    let tok = oauth::loopback(&http, &flow, &client_id, secret.as_deref(), port)
        .map_err(|e| e.to_string())?;

    let (name, value, env) = oauth::secret_to_store(kind, &tok);
    println!("\nSuccess. Store this {name} somewhere safe (it is not saved anywhere):\n");
    println!("  {value}\n");
    println!("Either export it from your shell profile:\n  export {env}='…'\n");
    println!(
        "or, on macOS, keep it in the Keychain (it prompts for the value):\n  security add-generic-password -s {} -a \"$USER\" -w",
        secrets::keychain_service(id, name)
    );
    Ok(())
}

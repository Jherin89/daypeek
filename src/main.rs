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
use daypeek::snapshot::View;
use daypeek::{calendar, demo, detect, oauth, secrets, snapshot, weather};

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

    /// With --demo: show the offline / stale-cache state
    #[arg(long, requires = "demo", hide = true, global = true)]
    demo_offline: bool,

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

    let view = match cli.command {
        Some(Command::Providers) if cli.demo => {
            return print_providers(demo::provider_rows(), cli.json)
        }
        Some(Command::Providers) => return providers_cmd(&cfg, cli.json),
        Some(Command::Auth { provider, port }) => return auth_cmd(&cfg, &provider, port),
        Some(Command::Tomorrow) => View::Tomorrow,
        Some(Command::Week) => View::Week,
        Some(Command::Today) | Some(Command::Setup { .. }) | None => View::Today,
    };

    let snap = if cli.demo {
        let tz = snapshot::resolve_tz(&cfg);
        if cli.demo_offline {
            demo::offline_snapshot(now.with_timezone(&tz), view)
        } else {
            demo::snapshot(now.with_timezone(&tz), view)
        }
    } else {
        let tz = snapshot::resolve_tz(&cfg);
        let today = now.with_timezone(&tz).date_naive();
        let mut cache = Cache::new(cfg.cache_dir(), cli.offline);
        if !cli.refresh {
            // Shell-startup runs reuse results from the last few minutes, but
            // never ones fetched before local midnight.
            let minutes = i64::try_from(cfg.general.cache_minutes).unwrap_or(i64::MAX / 60_000);
            cache = cache.reuse_recent(
                chrono::Duration::minutes(minutes.min(7 * 24 * 60)),
                Some(calendar::day_window(today, tz).0),
            );
        }
        snapshot::gather(&cfg, &cache, now, view)
    };

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
    let style = Style {
        width: width.min(96),
        time_format: cfg.general.time_format,
        max_tasks: cfg.general.max_tasks,
    };
    if cli.compact {
        println!("{}", render::compact(&snap, &style));
    } else {
        println!("{}", render::full(&snap, &style));
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
    for c in &cfg.calendars {
        let h = match calendar::fetch_feed(&http, &c.url) {
            Ok(text) => {
                let n = calendar::events_on(&text, &c.name, today, tz).len();
                Health::Ok(format!("{n} event{} today", if n == 1 { "" } else { "s" }))
            }
            Err(e) => providers::health_from_error(e),
        };
        let kind = if calendar::is_remote(&c.url) {
            "cloud"
        } else {
            "local"
        };
        rows.push((c.name.clone(), "iCal calendar".into(), kind.into(), h));
    }
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

    let flow = oauth::flow_for(kind, tenant)
        .ok_or_else(|| format!("`daypeek auth` supports ticktick, mstodo, gtasks (not {kind})"))?;
    if matches!(kind, "ticktick" | "gtasks") && secret.is_none() {
        return Err(format!(
            "{kind} needs your app's client secret: {}",
            secrets::hint(id, "client_secret", &format!("{env_prefix}_CLIENT_SECRET"))
        ));
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

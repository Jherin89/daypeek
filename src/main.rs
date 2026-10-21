use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand, ValueEnum};
use colored::Colorize;

use today::cache::Cache;
use today::config::{self, Config};
use today::http::Http;
use today::providers::{self, Ctx, Health};
use today::render::{self, Style};
use today::{calendar, demo, oauth, secrets, snapshot, weather};

/// A fast one-screen daily dashboard: calendar, tasks from every app, weather.
#[derive(Parser)]
#[command(name = "today", version, about, long_about = None)]
struct Cli {
    /// Short 2–3 line summary (for shell startup)
    #[arg(long, conflicts_with = "json")]
    compact: bool,

    /// Machine-readable JSON
    #[arg(long)]
    json: bool,

    /// Show fake demo data (no accounts or network needed)
    #[arg(long)]
    demo: bool,

    /// Don't touch the network; show cached data
    #[arg(long)]
    offline: bool,

    /// Write a sample config to ~/.config/today/config.toml
    #[arg(long)]
    init: bool,

    /// With --init: overwrite an existing config
    #[arg(long, requires = "init")]
    force: bool,

    /// Config file [default: ~/.config/today/config.toml, or $TODAY_CONFIG]
    #[arg(long, value_name = "PATH", global = true)]
    config: Option<PathBuf>,

    /// When to use color
    #[arg(long, value_enum, default_value_t = ColorWhen::Auto, global = true)]
    color: ColorWhen,

    /// Pretend it is this instant (RFC 3339) — for screenshots and tests
    #[arg(long, value_name = "RFC3339", hide = true)]
    at: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// List configured providers and check that each one works
    Providers {
        /// Machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// One-time OAuth helper for ticktick, mstodo, or gtasks (bring your own app)
    Auth {
        /// Provider id from your config, e.g. `gtasks`
        provider: String,
        /// Loopback port; register http://127.0.0.1:<port>/callback as the redirect URI
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
            eprintln!("today: {msg}");
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

    let now: DateTime<Utc> = match &cli.at {
        Some(s) => DateTime::parse_from_rfc3339(s)
            .map_err(|e| format!("--at: {e}"))?
            .with_timezone(&Utc),
        None => Utc::now(),
    };

    // A broken config must not break your shell prompt: warn and use defaults.
    let cfg = match Config::load(&path) {
        Ok(c) => c,
        Err(_) if cli.demo => Config::default(),
        Err(e) => {
            eprintln!("today: {e} (using defaults)");
            Config::default()
        }
    };

    match cli.command {
        Some(Command::Providers { json }) => return providers_cmd(&cfg, json),
        Some(Command::Auth { provider, port }) => return auth_cmd(&cfg, &provider, port),
        None => {}
    }

    let snap = if cli.demo {
        let tz = snapshot::resolve_tz(&cfg);
        demo::snapshot(now.with_timezone(&tz))
    } else {
        let cache = Cache::new(cfg.cache_dir(), cli.offline);
        snapshot::gather(&cfg, &cache, now)
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
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, config::SAMPLE_CONFIG).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("Wrote {}", path.display());
    println!("Next: set $TODOIST_API_TOKEN (or edit the file), then run `today providers`.");
    Ok(())
}

fn providers_cmd(cfg: &Config, json: bool) -> Result<(), String> {
    let tz = snapshot::resolve_tz(cfg);
    let http = Http::new(Duration::from_secs(cfg.general.timeout_secs.max(1)));
    let today = Utc::now().with_timezone(&tz).date_naive();
    let ctx = Ctx {
        http: &http,
        today,
        tz,
    };
    let (built, broken) = providers::build_all(cfg);

    // Health checks run in parallel, like the dashboard.
    let mut rows: Vec<(String, String, String, Health)> = std::thread::scope(|s| {
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
        println!("No providers configured. Run `today --init` to write a sample config.");
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

fn auth_cmd(cfg: &Config, id: &str, port: u16) -> Result<(), String> {
    let table = cfg
        .providers
        .get(id)
        .ok_or_else(|| format!("no [providers.{id}] in your config"))?;
    let kind = table.get("type").and_then(|v| v.as_str()).unwrap_or(id);
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

    let flow = match kind {
        "ticktick" => oauth::Flow {
            provider: "TickTick",
            authorize_url: "https://ticktick.com/oauth/authorize".into(),
            token_url: "https://ticktick.com/oauth/token".into(),
            scope: "tasks:read",
            extra: &[],
            basic_auth: true,
            pkce: false,
        },
        "mstodo" => oauth::Flow {
            provider: "Microsoft To Do",
            authorize_url: providers::mstodo::authorize_url(tenant),
            token_url: providers::mstodo::token_url(tenant),
            scope: providers::mstodo::SCOPE,
            extra: &[("response_mode", "query")],
            basic_auth: false,
            pkce: true,
        },
        "gtasks" => oauth::Flow {
            provider: "Google Tasks",
            authorize_url: providers::gtasks::AUTHORIZE_URL.into(),
            token_url: providers::gtasks::TOKEN_URL.into(),
            scope: providers::gtasks::SCOPE,
            extra: &[("access_type", "offline"), ("prompt", "consent")],
            basic_auth: false,
            pkce: true,
        },
        other => {
            return Err(format!(
                "`today auth` supports ticktick, mstodo, gtasks (not {other})"
            ))
        }
    };
    if matches!(kind, "ticktick" | "gtasks") && secret.is_none() {
        return Err(format!(
            "{kind} needs your app's client secret: {}",
            secrets::hint(id, "client_secret", &format!("{env_prefix}_CLIENT_SECRET"))
        ));
    }
    let http = Http::new(Duration::from_secs(15));
    let tok = oauth::loopback(&http, &flow, &client_id, secret.as_deref(), port)
        .map_err(|e| e.to_string())?;

    let (name, value, env) = match (kind, &tok.refresh_token) {
        ("ticktick", _) => (
            "token",
            tok.access_token.clone(),
            format!("{env_prefix}_ACCESS_TOKEN"),
        ),
        (_, None) => (
            "access_token",
            tok.access_token.clone(),
            format!("{env_prefix}_ACCESS_TOKEN"),
        ),
        (_, Some(rt)) => (
            "refresh_token",
            rt.clone(),
            format!("{env_prefix}_REFRESH_TOKEN"),
        ),
    };
    println!("\nSuccess. Store this {name} somewhere safe (it is not saved anywhere):\n");
    println!("  {value}\n");
    println!("Either export it from your shell profile:\n  export {env}='…'\n");
    println!(
        "or, on macOS, keep it in the Keychain:\n  security add-generic-password -s {} -a \"$USER\" -w '…'",
        secrets::keychain_service(id, name)
    );
    Ok(())
}

//! End-to-end tests of the `daypeek` binary.

use std::process::Command;

const AT: &str = "2026-10-21T10:58:00-04:00";

fn daypeek() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_daypeek"));
    // Isolate from the developer's real config, cache, and tokens.
    let dir = std::env::temp_dir().join(format!("daypeek-cli-{}", std::process::id()));
    c.env("HOME", &dir)
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env_remove("DAYPEEK_CONFIG")
        .env_remove("TODOIST_API_TOKEN")
        .env_remove("DAYPEEK_TODOIST_TOKEN")
        .env("TZ", "America/New_York");
    c
}

fn run(args: &[&str]) -> (bool, String, String) {
    let out = daypeek().args(args).output().expect("run daypeek");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn fixture_config(extra: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let fx = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    let cfg = format!(
        r#"
[general]
timezone = "America/New_York"
cache_dir = "{cache}"

[[calendars]]
name = "Fixture"
url = "{fx}/calendar.ics"

[providers.notes]
type = "markdown"
paths = ["{fx}/tasks.md"]

[providers.vault]
type = "obsidian"
vault = "{fx}/vault"
daily_notes_folder = "Daily"

[providers.todo]
type = "todotxt"
path = "{fx}/todo.txt"
{extra}
"#,
        cache = dir.path().join("cache").display()
    );
    std::fs::write(dir.path().join("config.toml"), cfg).unwrap();
    dir
}

#[test]
fn demo_renders_every_section() {
    let (ok, out, _) = run(&["--demo", "--at", AT, "--color", "never"]);
    assert!(ok);
    for needle in [
        "Wednesday, October 21",
        "week 43",
        "CALENDAR",
        "TASKS",
        "WEATHER",
        "NOTE",
        "Evans, Georgia",
        "todoist+reminders",
    ] {
        assert!(out.contains(needle), "missing {needle:?} in\n{out}");
    }
    assert!(!out.contains('\x1b'), "--color never must not emit escapes");
}

#[test]
fn compact_is_short() {
    let (ok, out, _) = run(&["--demo", "--compact", "--at", AT]);
    assert!(ok);
    assert!(out.trim().lines().count() <= 3, "{out}");
    assert!(out.starts_with("Wed Oct 21"));
}

#[test]
fn json_is_valid() {
    let (ok, out, _) = run(&["--demo", "--json", "--at", AT]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["date"]["iso_week"], 43);
    assert_eq!(v["demo"], true);
    assert!(v["tasks"].as_array().unwrap().len() >= 5);
    assert_eq!(v["weather"]["data"]["unit"], "°F");
}

#[test]
fn local_fixture_sources_end_to_end() {
    let dir = fixture_config("");
    let cfg = dir.path().join("config.toml");
    let (ok, out, err) = run(&[
        "--config",
        cfg.to_str().unwrap(),
        "--offline",
        "--at",
        AT,
        "--json",
    ]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let events: Vec<_> = v["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["title"].as_str().unwrap())
        .collect();
    assert!(events.contains(&"Design review (moved)"));
    let tasks: Vec<_> = v["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["title"].as_str().unwrap())
        .collect();
    assert_eq!(
        tasks[0], "Call Mom +family @phone",
        "todo.txt (A) sorts first"
    );
    assert!(tasks.contains(&"Outline talk for Augusta.rs"));
    assert!(!tasks.contains(&"Future task"));
    assert_eq!(v["notes"][0]["source"], "vault");
}

#[test]
fn unreachable_network_never_crashes() {
    let dir = fixture_config(
        r#"
[[calendars]]
name = "Dead"
url = "http://127.0.0.1:9/nothing.ics"
[weather]
latitude = 33.5
longitude = -82.1
[providers.todoist]
token = "not-a-real-token"
base_url = "http://127.0.0.1:9"
"#,
    );
    let cfg = dir.path().join("config.toml");
    let (ok, out, _) = run(&[
        "--config",
        cfg.to_str().unwrap(),
        "--at",
        AT,
        "--color",
        "never",
    ]);
    assert!(ok);
    assert!(out.contains("Dead"), "failed calendar is reported:\n{out}");
    assert!(
        out.contains("todoist"),
        "failed provider is reported:\n{out}"
    );
    assert!(
        out.contains("Fix bike tire"),
        "local sources still shown:\n{out}"
    );
}

#[test]
fn init_writes_sample_and_refuses_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("sub/config.toml");
    let (ok, _, _) = run(&["--init", "--config", cfg.to_str().unwrap()]);
    assert!(ok);
    let text = std::fs::read_to_string(&cfg).unwrap();
    assert!(
        text.contains("# [weather]") && !text.contains("Evans"),
        "neutral"
    );
    assert!(
        text.lines()
            .all(|l| l.trim().is_empty() || l.starts_with('#')),
        "all commented"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&cfg).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "owner-only like `daypeek setup`");
    }
    let (ok, _, err) = run(&["--init", "--config", cfg.to_str().unwrap()]);
    assert!(!ok);
    assert!(err.contains("already exists"));
    let (ok, _, _) = run(&["--init", "--force", "--config", cfg.to_str().unwrap()]);
    assert!(ok);
}

#[test]
fn broken_config_warns_but_renders() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("config.toml");
    std::fs::write(&cfg, "this is = = not toml").unwrap();
    let (ok, out, err) = run(&[
        "--config",
        cfg.to_str().unwrap(),
        "--at",
        AT,
        "--color",
        "never",
    ]);
    assert!(ok);
    assert!(err.contains("using defaults"));
    assert!(out.contains("Wednesday, October 21"));
}

#[test]
fn providers_subcommand_reports_status() {
    let dir = fixture_config("[providers.todoist]\ntoken_env = \"DAYPEEK_CLI_TEST_UNSET\"\n");
    let cfg = dir.path().join("config.toml");
    let (ok, out, _) = run(&["--config", cfg.to_str().unwrap(), "providers", "--json"]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let status = |id: &str| {
        v.as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap()["health"]["status"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(status("notes"), "ok");
    assert_eq!(status("vault"), "ok");
    assert_eq!(status("todo"), "ok");
    assert_eq!(status("todoist"), "auth_missing");
    assert_eq!(status("Fixture"), "ok");
}

#[test]
fn demo_providers_and_offline_variants() {
    let (ok, out, _) = run(&["--demo", "--color", "never", "providers"]);
    assert!(ok);
    assert!(
        out.contains("todoist") && out.contains("auth missing"),
        "{out}"
    );

    let (ok, out, _) = run(&["--demo", "--color", "never", "providers", "--json"]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v.as_array().unwrap().len() >= 5);

    let (ok, out, _) = run(&["--demo", "--demo-offline", "--color", "never", "--at", AT]);
    assert!(ok);
    assert!(out.contains("cached 3h 04m ago"), "{out}");
    assert!(out.contains("Submit expense report"));

    let (ok, _, err) = run(&["--demo-offline"]);
    assert!(!ok, "--demo-offline requires --demo: {err}");
}

#[test]
fn setup_without_a_terminal_exits_with_a_hint() {
    let out = daypeek()
        .args(["setup"])
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("needs a terminal") && err.contains("daypeek --init"),
        "{err}"
    );
}

#[test]
fn view_flags_work_before_or_after_the_subcommand() {
    for args in [
        vec!["providers", "--demo", "--color", "never"],
        vec!["--demo", "providers", "--color", "never"],
    ] {
        let (ok, out, err) = run(&args);
        assert!(ok && out.contains("auth missing"), "{args:?}: {err}");
    }
    let (ok, out, err) = run(&["week", "--demo", "--json", "--at", AT]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["view"], "week");
    assert_eq!(
        (v["start"].as_str(), v["end"].as_str()),
        (Some("2026-10-21"), Some("2026-10-27"))
    );
    assert_eq!(v["weather"]["data"]["days"].as_array().unwrap().len(), 7);

    let (ok, out, _) = run(&["tomorrow", "--compact", "--demo", "--at", AT]);
    assert!(ok);
    assert!(out.starts_with("Tomorrow · Thu Oct 22"), "{out}");
    assert!(
        out.contains("Dentist") && out.trim().lines().count() <= 3,
        "{out}"
    );

    let (ok, out, _) = run(&["tomorrow", "--demo", "--json", "--at", AT]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["view"], "tomorrow");
    assert!(v["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["due"] == "2026-10-22"));

    let (ok, out, _) = run(&[
        "week",
        "--demo",
        "--compact",
        "--at",
        AT,
        "--color",
        "never",
    ]);
    assert!(ok);
    assert_eq!(
        out.trim().lines().count(),
        8,
        "header + one line per day:\n{out}"
    );
    assert!(
        out.contains("Sat 24") && out.contains("Weekend in Savannah"),
        "{out}"
    );

    let (ok, out, _) = run(&["week", "--demo", "--at", AT, "--color", "never"]);
    assert!(ok);
    for needle in [
        "This week · Oct 21 – Oct 27",
        "THU OCT 22  tomorrow",
        "Brunch with the Hendersons",
        "Send invoice to Acme",
    ] {
        assert!(out.contains(needle), "missing {needle:?} in\n{out}");
    }
}

#[test]
fn narrow_terminals_truncate_instead_of_blanking_titles() {
    use unicode_width::UnicodeWidthStr;
    for view in ["today", "tomorrow", "week"] {
        let out = daypeek()
            .args([view, "--demo", "--at", AT, "--color", "never"])
            .env("COLUMNS", "50")
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&out.stdout);
        for line in out.lines() {
            assert!(
                line.width() <= 50,
                "{view}: too wide ({}): {line:?}",
                line.width()
            );
            assert!(
                !line.trim_end().ends_with("  …"),
                "{view}: blank title: {line:?}"
            );
        }
        assert!(out.contains("Team standup"), "{view}:\n{out}");
    }
}

#[test]
fn compact_shows_sources_that_need_setup() {
    let dir = fixture_config("[providers.todoist]\ntoken_env = \"DAYPEEK_CLI_TEST_UNSET\"\n");
    let cfg = dir.path().join("config.toml");
    let (ok, out, _) = run(&[
        "--config",
        cfg.to_str().unwrap(),
        "--offline",
        "--compact",
        "--at",
        AT,
        "--color",
        "never",
    ]);
    assert!(ok);
    assert!(out.contains("○ todoist needs setup"), "{out}");
}

#[test]
fn empty_state_points_to_setup() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("none.toml");
    let (ok, out, _) = run(&[
        "--config",
        cfg.to_str().unwrap(),
        "--at",
        AT,
        "--color",
        "never",
    ]);
    assert!(ok);
    assert!(
        out.contains("Run `daypeek setup`") && !out.contains("--init"),
        "{out}"
    );
    let (ok, out, _) = run(&[
        "--config",
        cfg.to_str().unwrap(),
        "--compact",
        "--at",
        AT,
        "--color",
        "never",
    ]);
    assert!(ok && out.contains("daypeek setup"), "{out}");
    let (ok, out, _) = run(&["--config", cfg.to_str().unwrap(), "providers"]);
    assert!(
        ok && out.contains("daypeek setup") && !out.contains("--init"),
        "{out}"
    );
}

#[test]
fn auth_explains_token_providers() {
    // With or without a [providers.todoist] block.
    let dir = fixture_config("[providers.todoist]\n");
    let cfg = dir.path().join("config.toml");
    for args in [
        vec!["auth", "todoist"],
        vec!["--config", cfg.to_str().unwrap(), "auth", "todoist"],
    ] {
        let (ok, _, err) = run(&args);
        assert!(!ok);
        assert!(
            err.contains("personal API token") && err.contains("DAYPEEK_TODOIST_TOKEN"),
            "{err}"
        );
        assert!(!err.contains("client_id"), "{err}");
    }
    let (_, _, err) = run(&["auth", "things"]);
    assert!(err.contains("needs no sign-in"), "{err}");
    let (_, _, err) = run(&["auth", "gtasks"]);
    assert!(err.contains("daypeek setup"), "{err}");
}

#[test]
fn invalid_timezone_warns() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("config.toml");
    std::fs::write(&cfg, "[general]\ntimezone = \"Mars/Olympus\"\n").unwrap();
    let (ok, out, err) = run(&[
        "--config",
        cfg.to_str().unwrap(),
        "--at",
        AT,
        "--color",
        "never",
    ]);
    assert!(ok);
    assert!(err.contains("unknown timezone \"Mars/Olympus\""), "{err}");
    assert!(out.contains("October 21"));
}

#[test]
fn refresh_is_accepted_and_conflicts_with_offline() {
    let dir = fixture_config("");
    let cfg = dir.path().join("config.toml");
    let (ok, out, err) = run(&[
        "--config",
        cfg.to_str().unwrap(),
        "--refresh",
        "--json",
        "--at",
        AT,
    ]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v["providers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["state"] == "live"));
    let (ok, _, _) = run(&["--refresh", "--offline", "--demo"]);
    assert!(!ok);
}

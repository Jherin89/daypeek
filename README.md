<div align="center">

# daypeek

**Your whole day in one glance, right in the terminal.**<br>
Calendar, tasks from 10 apps, and the weather. Printed in milliseconds, then it gets out of your way.

[![CI](https://github.com/Jherin89/daypeek/actions/workflows/ci.yml/badge.svg)](https://github.com/Jherin89/daypeek/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust 1.85+](https://img.shields.io/badge/rust-1.85%2B-orange.svg?logo=rust)](https://www.rust-lang.org)
[![Platform: macOS | Linux](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey.svg)](#install)

<img src="docs/screenshot.png" alt="daypeek showing today's calendar, merged tasks from five apps, weather, and a daily-note excerpt" width="760">

<sub>Every screenshot here is <code>daypeek --demo</code>: fake data, no accounts.</sub>

</div>

## Why

Your day is spread across a calendar, a to-do app or three, a notes vault,
and a weather tab. Opening each of them every morning adds up. `daypeek` is
one command that reads them all, **read-only**, and prints a single screen:
what's on the calendar, what's due or overdue, and whether to grab a jacket.
Put it in your shell startup and you see your day every time you open a
terminal.

## Features

- **One screen, one command.** Calendar, tasks, weather, and today's daily note.
  `--compact` gives you three lines for shell startup; `--json` is there for scripts.
- **Tasks from 10 apps, merged.** Todoist, TickTick, Microsoft To Do, Google
  Tasks, Things 3, Apple Reminders, Notion, Obsidian, todo.txt, and Markdown.
  Duplicates are folded together, priorities are normalized to p1–p4, and every
  task gets a badge showing where it came from.
- **Any calendar with an iCal link.** Google, iCloud, Outlook, Fastmail, sports
  schedules, and so on. Recurring events, all-day events, and time zones are
  handled.
- **Fast and offline-friendly.** Sources are fetched in parallel. Each one is
  cached, so when the network is down you see the last good data marked
  `↻ cached 3h ago` instead of an error.
- **Private by design.** Read-only. No account, server, or telemetry. Tokens
  come from env vars or the macOS Keychain and are only sent to that app's own API.
- **Never crashes your shell.** A broken source shows up as one dim line, and
  the rest of the dashboard still prints.
- **Small and dependency-light.** A single static binary, no async runtime, and
  no client secrets baked in.

## Gallery

| | |
| :---: | :---: |
| <img src="docs/screenshot-compact.png" alt="daypeek --compact: a three-line summary" width="420"><br>`--compact` for your shell startup | <img src="docs/screenshot-providers.png" alt="daypeek providers: a health check per source" width="420"><br>`daypeek providers` health check |
| <img src="docs/screenshot-json.png" alt="daypeek --json piped to jq" width="420"><br>`--json` for scripts, status bars, and `jq` | <img src="docs/screenshot-offline.png" alt="offline: every cloud source shows its cached data, marked stale" width="420"><br>Offline: cached data, clearly marked |
| <img src="docs/screenshot-light.png" alt="daypeek in a light terminal theme" width="420"><br>Light terminal theme | <img src="docs/screenshot.png" alt="daypeek in a dark terminal theme" width="420"><br>Dark terminal theme |

## Quick start

```sh
# 1. Install (needs Rust 1.85+, or grab a macOS binary from Releases; see docs/SETUP.md)
cargo install --git https://github.com/Jherin89/daypeek

# 2. See it with fake data, no setup needed
daypeek --demo

# 3. Write a commented starter config to ~/.config/daypeek/config.toml
daypeek --init

# 4. Add a token for the app you use (Todoist shown here), then check every source
export DAYPEEK_TODOIST_TOKEN='…'       # or keep it in the macOS Keychain
daypeek providers

# 5. Your day
daypeek
```

Then [run it every time a terminal opens](docs/SETUP.md#5-run-it-when-a-terminal-opens):

```sh
echo 'command -v daypeek >/dev/null && daypeek --compact' >> ~/.zshrc
```

**Docs:** 📦 [Setup guide](docs/SETUP.md) (install, every provider, calendars, weather, shell startup) ·
📖 [Usage reference](docs/USAGE.md) (flags, subcommands, JSON, troubleshooting, privacy) ·
🧩 [Add a provider](CONTRIBUTING.md) · 📝 [Changelog](CHANGELOG.md)

## Install

| How | Command |
| --- | --- |
| Prebuilt macOS (Apple Silicon) or Linux x86_64 binary | Download from [Releases](https://github.com/Jherin89/daypeek/releases) ([steps](docs/SETUP.md#1-install)) |
| From GitHub with Cargo | `cargo install --git https://github.com/Jherin89/daypeek` |
| From a clone | `git clone https://github.com/Jherin89/daypeek && cd daypeek && cargo install --path .` |
| From crates.io (once it's published) | `cargo install daypeek` |

## Supported apps

| App | Status | How it connects | Setup |
| --- | --- | --- | --- |
| **Todoist** | ✅ v1 | Official REST API, your personal API token | [guide](docs/SETUP.md#todoist) |
| **TickTick** | ✅ v1 | Official Open API, your own registered app | [guide](docs/SETUP.md#ticktick) |
| **Microsoft To Do** | ✅ v1 | Microsoft Graph, your own Entra app (no secret) | [guide](docs/SETUP.md#microsoft-to-do) |
| **Google Tasks** | ✅ v1 | Google Tasks API, your own OAuth client | [guide](docs/SETUP.md#google-tasks) |
| **Things 3** | ✅ v1 · macOS | Reads Things' local database **read-only** | [guide](docs/SETUP.md#things-3) |
| **Apple Reminders** | ✅ v1 · macOS | Small bundled `osascript` (JXA) bridge | [guide](docs/SETUP.md#apple-reminders) |
| **Notion** | ✅ v1 | Database query through an internal connection | [guide](docs/SETUP.md#notion) |
| **Obsidian** | ✅ v1 | Vault scan (Tasks and Dataview syntax) plus the daily note | [guide](docs/SETUP.md#obsidian) |
| **todo.txt** | ✅ v1 | Local file with `due:` dates | [guide](docs/SETUP.md#todotxt) |
| **Markdown** | ✅ v1 | Any `.md` files with `- [ ]` checkboxes | [guide](docs/SETUP.md#markdown) |
| **Calendars (iCal)** | ✅ v1 | Any `.ics` / `webcal://` link or local file | [guide](docs/SETUP.md#calendars) |
| **Weather** | ✅ v1 | [Open-Meteo](https://open-meteo.com), no API key | [guide](docs/SETUP.md#weather) |
| Asana | 🗺 planned | | |
| Trello | 🗺 planned | | |
| Linear | 🗺 planned | | |
| ClickUp | 🗺 planned | | |
| Logseq | 🗺 planned | | |
| Bear | 🗺 planned | | |

Want another app? [Request it](https://github.com/Jherin89/daypeek/issues/new?template=new_provider.yml)
or [add it yourself](CONTRIBUTING.md): a provider is a single file.

## Privacy

- **Read-only.** `daypeek` never creates, edits, completes, or deletes anything.
- **Your tokens stay yours.** They're read from env vars, the macOS Keychain, or
  (if you insist) your config file. Each one is only sent to its own app's API
  over HTTPS. They're never logged, cached, or sent anywhere else.
- **No telemetry, no server.** The only other network calls are your iCal URLs
  and Open-Meteo (your city or coordinates).
- **Local cache** of the last good data lives in `~/.cache/daypeek/`, one file
  per source, readable only by you (`0600`). Delete it any time.

## How it works

```
config.toml ─┬─ calendars ─► fetch .ics ─► parse VEVENTs ─► expand RRULE for today ─┐
             ├─ providers ─► Provider::fetch() (one thread each) ─► merge + dedup ───┼─► Snapshot ─► full / compact / JSON
             └─ weather ───► geocode (cached) ─► Open-Meteo forecast ────────────────┘
                       every source goes through the cache: live ▸ stale ▸ failed
```

- Each source runs on its own thread (`std::thread::scope`) with blocking HTTP
  and a hard timeout. There's no async runtime.
- Providers implement one small trait and live one per file in
  [`src/providers/`](src/providers).
- iCal is parsed by a small, forgiving reader. Recurrence (`RRULE`, `EXDATE`,
  overrides) is worked out for the current day only, rather than expanding the
  whole series.
- On a Linux x86_64 test machine with two calendars, five providers, and
  weather configured, a run takes about **0.1–0.5 s** live, **5 ms** with
  `--offline`, and **3 ms** with `--demo`.

## Development

```sh
cargo test                                   # unit, fixture, and CLI tests
cargo clippy --all-targets -- -D warnings
cargo run -- --demo
python3 scripts/screenshot.py                # re-render docs/*.png from --demo (needs Chrome + jq)
vhs docs/demo.tape                           # optional animated GIF (brew install vhs)
```

Cloud providers are tested against recorded, anonymized JSON fixtures in
[`tests/fixtures/`](tests/fixtures). Things is tested against a generated SQLite
database, and Reminders against sample bridge output. CI runs fmt, clippy, and
tests on Ubuntu and on macOS (Apple Silicon). A `v*` tag builds release
binaries for `aarch64-apple-darwin` and `x86_64-unknown-linux-gnu`.

## Roadmap

- [ ] Asana, Trello, Linear, and ClickUp providers
- [ ] Logseq journals and Bear notes
- [ ] CalDAV (iCloud, Fastmail) without a public `.ics` link
- [ ] `daypeek week`: the next 7 days
- [ ] Homebrew tap
- [ ] Optional live mode that refreshes on a timer

## License

[MIT](LICENSE) © Jordan Herin

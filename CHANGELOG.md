# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-10-21

### Added

- First release.
- One-screen dashboard: date header, today's calendar, merged tasks, weather,
  and a daily-note excerpt. `--compact` for shell startup, `--json` for scripts.
- Calendars from any iCal feed (`https://`, `webcal://`, local files), with
  recurrence (`RRULE`, `EXDATE`, `RECURRENCE-ID`), all-day events, and IANA,
  vendor-prefixed, and Windows time-zone names.
- Task providers: Todoist, TickTick, Microsoft To Do, Google Tasks, Things 3
  (macOS), Apple Reminders (macOS), Notion, Obsidian, todo.txt, and Markdown.
  Tasks are merged and de-duplicated, with p1–p4 priorities and source badges.
- Weather from Open-Meteo, with city geocoding (no API key).
- A cache for every source, with stale markers, and `--offline`.
- `daypeek providers` health check (with `--json`).
- `daypeek auth` loopback OAuth helper (PKCE where supported) for TickTick,
  Microsoft To Do, and Google Tasks, using your own app registration.
- Credentials from env vars, the macOS Keychain (`daypeek-<id>`), or the config file.
- `daypeek setup`: a guided wizard. You pick your apps, tokens are typed
  hidden and saved to the macOS Keychain (on Linux you're shown the env var),
  it can run the browser sign-in for TickTick, Microsoft To Do, and Google
  Tasks, asks for calendars and a city, writes the config (`0600`, with a
  backup of the old one), and runs the health check. `--demo setup` saves nothing.
- `install.sh` one-line installer: latest release for macOS arm64 or Linux
  x86_64, SHA-256 verified, with a fallback to `cargo install`.
- A Homebrew formula template in `packaging/homebrew/`.
- `--demo` fake data, `--init` starter config, an animated demo GIF.
- CI (Ubuntu and macOS) and a release workflow for `aarch64-apple-darwin` and
  `x86_64-unknown-linux-gnu`.

[Unreleased]: https://github.com/Jherin89/daypeek/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Jherin89/daypeek/releases/tag/v0.1.0

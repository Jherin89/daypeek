# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2026-10-23

### Added

- **Linear, Asana and Trello.** Linear shows open issues assigned to you that
  are due, plus ones in progress (GraphQL, personal API key in
  `$LINEAR_API_KEY`). Asana shows your incomplete tasks across your workspaces
  (personal access token in `$ASANA_TOKEN`). Trello shows due cards assigned to
  you, or every due card on chosen `boards` (API key + token in
  `$TRELLO_API_KEY` / `$TRELLO_TOKEN`, sent in a header, never in a URL). Each
  also reads the Keychain or the config, is in `daypeek setup`, and has a
  step-by-step guide.
- **CalDAV calendars** (`type = "caldav"`): sign in to iCloud, Fastmail,
  Nextcloud or any CalDAV server with an app password and see private
  calendars without publishing a link. daypeek discovers your calendars
  (`current-user-principal` → `calendar-home-set`, or `/.well-known/caldav`),
  fetches only the needed date range with a `calendar-query` REPORT, and uses
  the same recurrence and time-zone handling as iCal links. The password comes
  from `$DAYPEEK_<NAME>_PASSWORD`, the Keychain item `daypeek-<name>`, or the
  config; it's never sent to another host after a redirect. `daypeek setup`
  offers iCloud, Fastmail and Nextcloud presets and lets you pick calendars.
- **`--watch`** keeps the dashboard open and redraws it in place every
  `--every <secs>` (default 60) and on resize, on the alternate screen. `q`,
  `Esc` or `Ctrl-C` quits and restores the terminal; `r` fetches everything
  now. Cloud results are reused for `cache_minutes` (at least a minute) between
  redraws. Works with `today`, `tomorrow`, `week` and `--compact`.
- **`daypeek done <n>`** (opt-in): with `allow_write = true` for an app (or
  under `[general]`), tasks from Todoist and TickTick get numbers and
  `daypeek done 2` completes that task after asking (`--yes` skips the
  question). The numbers match what was last on screen. Without
  `allow_write`, daypeek stays read-only and `done` explains how to opt in.
  TickTick needs `daypeek auth ticktick` again to grant `tasks:write`.
- Dependabot keeps Cargo dependencies and GitHub Actions up to date weekly, and
  CI runs `cargo audit` on every push and every Monday.

### Changed

- Dependency updates: `tinystr` 0.8.5, `yoke` 0.8.4, `zerovec` 0.11.9,
  `zerovec-derive` 0.11.7.

## [0.2.0] - 2026-10-21

### Added

- `daypeek tomorrow` and `daypeek week`: tomorrow's events, tasks due tomorrow
  and the forecast, or seven day blocks with each day's events, tasks and
  high/low. Both work with `--compact`, `--json` and `--demo`, and every task
  app fetches the right date range (Todoist filter, Things scheduled/deadline
  dates, Google Tasks `dueMax`, Notion date filter, Reminders bridge `--days`).
  The weather now fetches a 7-day forecast (`weather.data.days` in JSON).
- Instant repeat runs: cloud results fetched in the last `general.cache_minutes`
  (default 5, `0` = off) are reused, never across midnight. They show as
  `"state": "cached"` in JSON and with no marker on screen. `--refresh` fetches
  everything now; `daypeek providers` always checks live.
- Zero setup on a Mac: with no config file, daypeek finds Things 3 and your most
  recent Obsidian vault (including its daily-notes folder and date format) and
  shows their tasks. `daypeek setup` pre-ticks the apps it finds and suggests a
  weather city from your time zone. Linux detects nothing and points you to
  `daypeek setup`.
- `daypeek today`, the same as plain `daypeek`.
- Homebrew: `brew install jherin89/tap/daypeek` from the new
  [Jherin89/homebrew-tap](https://github.com/Jherin89/homebrew-tap).
- Release builds for Intel Macs (`x86_64-apple-darwin`) and a static Linux
  build (`x86_64-unknown-linux-musl`) that runs on any distribution. CI builds
  both on every push.

### Changed

- `--demo`, `--offline`, `--json`, `--compact` and the new `--refresh` are global:
  they work before or after a subcommand (`daypeek providers --demo`).
- `daypeek --init` writes a neutral sample config with everything commented out
  (it used to switch on weather for one city and Todoist), with mode `0600`.
- install.sh installs into a folder that's already on your `PATH` when it can,
  honours `DAYPEEK_INSTALL_DIR`, prints the exact line to fix `PATH` otherwise,
  and uses full paths in its next steps until you do.
- The release workflow uses `softprops/action-gh-release` v3 (Node 24).

### Fixed

- The Linux release binary needed glibc 2.39; it's now a static musl build.
- `daypeek setup` with no app ticked no longer says "You're set". It asks again
  ("press Space to select"), then says plainly that nothing was configured.
- Narrow terminals (for example 50 columns) no longer blank event titles: the
  location column goes first, then the status shortens, and titles keep room.
  Stale/failed lines and weather fit the width too.
- `--compact` now shows sources that still need setup (`○ todoist needs setup`)
  and failed ones (`!N`) instead of hiding them, and no longer claims `0 due`
  when no task app answered. Missing credentials are reported even with
  `--offline`.
- Empty states point to `daypeek setup` instead of `daypeek --init`.
- `daypeek auth todoist` (and `notion`) explains that these use a personal API
  token and where to put it, instead of asking for a `client_id`; local apps say
  they need no sign-in.
- An invalid `general.timezone` prints a clear warning instead of being ignored.
- README: no more "before the first release is out".

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

[Unreleased]: https://github.com/Jherin89/daypeek/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/Jherin89/daypeek/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/Jherin89/daypeek/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/Jherin89/daypeek/releases/tag/v0.1.0

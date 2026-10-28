# Usage reference

- [Commands at a glance](#commands-at-a-glance)
- [Flags](#flags)
- [Subcommands](#subcommands): [`tomorrow` and `week`](#daypeek-tomorrow-and-daypeek-week) · [`--watch`](#--watch-a-live-dashboard) · [`done`](#daypeek-done) · [`setup`](#daypeek-setup) · [`providers`](#daypeek-providers) · [`auth`](#daypeek-auth)
- [Completing tasks](#completing-tasks) (opt-in; read-only is the default)
- [Caching: why it's instant](#caching-why-its-instant)
- [Reading the dashboard](#reading-the-dashboard)
- [JSON output and `jq`](#json-output-and-jq)
- [Environment variables](#environment-variables)
- [Files](#files)
- [Troubleshooting and FAQ](#troubleshooting-and-faq)
- [Privacy](#privacy)

New here? Run `daypeek setup`, or see the [setup guide](SETUP.md).

## Commands at a glance

```sh
daypeek                       # full dashboard for today
daypeek tomorrow              # tomorrow: events, tasks due, forecast
daypeek week                  # the next seven days
daypeek --compact             # 2–3 line summary, for shell startup
daypeek --json                # everything as JSON
daypeek --refresh             # fetch everything now (skip the 5-minute reuse)
daypeek --offline             # cached data only, no network
daypeek --demo                # fake data (works without a config)
daypeek --watch               # stay open and redraw in place (q quits)
daypeek done 3                # complete task 3 (only with allow_write = true)
daypeek setup                 # guided first-run setup (wizard)
daypeek providers             # health check for every configured source
daypeek auth <provider>       # one-time OAuth sign-in: ticktick, mstodo, gtasks
daypeek --init                # write a starter config
```

## Flags

| Flag | What it does |
| --- | --- |
| `--compact` | A short summary: date, time, and weather; the current or next event; how many tasks are due and the most urgent one. Problems are flagged: `↻N` stale, `!N` failed, `○ todoist needs setup` for a source with no credential yet. With `week` it prints one line per day. |
| `--json` | Prints the full snapshot as JSON on stdout (see [below](#json-output-and-jq)). There's no color and no task limit. |
| `--demo` | Uses built-in fake data and makes no network calls. Dashboard formatting (such as time zone and task limit) still follows your config; configured sources are never fetched. Combine it with `--compact`, `--json`, `tomorrow`, `week`, `providers`, or `setup`. |
| `--refresh` | Fetches every source now instead of reusing results from the last `cache_minutes` ([caching](#caching-why-its-instant)). |
| `--watch` | Keeps the dashboard open and redraws it in place ([details](#--watch-a-live-dashboard)). Works with `today`, `tomorrow`, `week` and `--compact`; not with `--json`. |
| `--every <SECS>` | With `--watch`: seconds between redraws (default 60, minimum 2). |
| `--offline` | Skips every network source and shows the last cached data, marked stale. Local sources (Things, Obsidian, files) are still read live. |
| `--init` | Writes a sample config to `~/.config/daypeek/config.toml` (or the `--config` path). Everything in it is commented out, and the file is readable only by you (`0600`). |
| `--force` | With `--init`, overwrites an existing config (keeping a `.bak`). |
| `--config <PATH>` | Uses a different config file. Defaults to `$DAYPEEK_CONFIG`, then `$XDG_CONFIG_HOME/daypeek/config.toml`, then `~/.config/daypeek/config.toml`. |
| `--color auto\|always\|never` | `auto` (the default) uses color only when stdout is a terminal and `NO_COLOR` isn't set. |
| `-V`, `--version` | Prints the version. |
| `-h`, `--help` | Prints help. Works on subcommands too, e.g. `daypeek auth --help`. |

Flags work before or after a subcommand: `daypeek week --compact` and
`daypeek --compact week` are the same.

Examples:

```sh
daypeek --compact --offline              # instant: cache only
daypeek --color never > ~/today.txt      # plain text
daypeek --config ~/work/daypeek.toml     # a separate work profile
alias dpw='daypeek --config ~/work/daypeek.toml'
```

## Subcommands

### `daypeek tomorrow` and `daypeek week`

```sh
daypeek tomorrow              # tomorrow's events, tasks due tomorrow, the forecast
daypeek week                  # today plus the next six days, one block per day
daypeek week --compact        # one line per day
daypeek tomorrow --json       # same JSON shape, with "view": "tomorrow"
daypeek --demo week           # try it with fake data
```

- **Tomorrow** lists tomorrow's events (no "now / next" markers, since it hasn't
  started), the tasks due exactly tomorrow, and tomorrow's high, low and chance
  of rain.
- **Week** has one block per day with that day's events, tasks and forecast.
  Overdue and undated tasks sit under today. A multi-day event appears on
  every day it covers.

Every task app takes part. Todoist uses its own filter for these views
(`overdue | 7 days` for the week); your `filter` setting still drives the
today view. Things includes to-dos scheduled or due in the range. Google Tasks,
Notion and Apple Reminders ask for tasks due through the last day, and the
file-based apps are filtered locally.

### `--watch`: a live dashboard

```sh
daypeek --watch               # today, redrawn every minute
daypeek week --watch          # the week view
daypeek --watch --every 10    # redraw every 10 seconds
daypeek --watch --compact     # the short summary, big and live
```

`--watch` takes over the terminal window like `top` or `less` does: it
switches to the alternate screen, redraws the dashboard in place every
`--every` seconds (and whenever you resize the window), and puts your terminal
back exactly as it was when you leave.

| Key | Does |
| --- | --- |
| `q`, `Esc`, `Ctrl-C` | Quit |
| `r` | Fetch everything now, ignoring the cache |

The footer shows when it last updated. Every redraw uses the current time, so
the *now* and *next* markers move along. Cloud sources are **not** fetched on
every redraw: results are reused for your `cache_minutes` (at least one minute,
even if `cache_minutes = 0`), so `--every 5` redraws the clock without calling
every API every five seconds. Local sources (Things, Obsidian, files) are read
fresh each time. Just after midnight it switches to the new day by itself.

`--watch` needs a real terminal: piped or scripted, it exits with an error
(and it can't be combined with `--json`). `--every` without `--watch` is an
error.

### `daypeek done`

```sh
daypeek done 3                # complete task 3 (asks first)
daypeek done 3 --yes          # don't ask
```

Completes a task in the app it came from. **Off by default**: see
[Completing tasks](#completing-tasks).

### `daypeek setup`

```sh
daypeek setup [--port 8765]
daypeek --demo setup          # practice run: same prompts, nothing saved
```

An interactive wizard that asks for:

1. Your apps (multi-select: Space ticks, Enter continues). On macOS, Things 3 and your most recent Obsidian vault are pre-ticked when they're found.
2. Each app's credential, typed hidden. On macOS it goes into the Keychain; on Linux you're shown the env var to set.
3. Your calendars: iCal links, or a CalDAV account (iCloud, Fastmail, Nextcloud, or another server). For CalDAV it asks for your username and app password (hidden, into the Keychain), looks up your calendars, and lets you untick the ones you don't want.
4. A city for weather, suggested from your time zone (type `none` to skip).

It then writes the config (mode `0600`, keeping a `.bak` of any old one) and
runs `daypeek providers`. For TickTick, Microsoft To Do, and Google Tasks it
can run the browser sign-in from [`daypeek auth`](#daypeek-auth) for you.
`--port` is the loopback port that sign-in uses. With no terminal attached, it
prints a hint and exits with status 1. Details:
[setup guide, step 3](SETUP.md#3-run-the-setup-wizard).

### `daypeek providers`

Checks every configured calendar, task provider, and the weather, and prints one
line each:

```
  ID             SOURCE             KIND    STATUS
  todoist        Todoist            cloud   ✓ ok 4 tasks due
  ticktick       TickTick           cloud   ✗ error network: timed out
  mstodo         Microsoft To Do    cloud   ○ auth missing run `daypeek auth mstodo`
  things         Things 3           macos   ✓ ok 1 task due
```

| Status | Meaning |
| --- | --- |
| `✓ ok` | Fetched successfully, with a count. |
| `○ auth missing` | No credential found, or the app rejected it. The hint says which env var or Keychain item to set. |
| `✗ error` | Something else failed (network, bad ID, permissions). The message says what. |
| `– unsupported` | Not available on this OS (Things and Reminders need macOS). |

Options: `--json` (an array of `{id, name, kind, health: {status, detail}}` objects) and `--config`.
`daypeek --demo providers` shows a sample. The health check always fetches
live; it never reuses cached results.

### `daypeek auth`

```sh
daypeek auth ticktick | mstodo | gtasks   [--port 8765]
```

A one-time sign-in helper for the three apps whose APIs only accept OAuth. It
uses **your own** app registration (see the [setup guide](SETUP.md)). Todoist
and Notion don't need it: they use a personal API token, and
`daypeek auth todoist` tells you where to get one and where to put it.

1. It reads `client_id` from `[providers.<id>]` (or `<APP>_CLIENT_ID`). For TickTick and
   Google it also reads your client secret from `TICKTICK_CLIENT_SECRET` /
   `GTASKS_CLIENT_SECRET` or the Keychain.
2. It opens your browser to the app's sign-in page. It uses PKCE where supported,
   plus a random `state` value.
3. It listens on `127.0.0.1:<port>` for the redirect to `/callback`. The
   redirect URI to register is `http://127.0.0.1:8765/callback`, or
   `http://localhost/callback` for Microsoft.
4. It swaps the code for a token and **prints** it, along with the exact
   `export` and `security add-generic-password` commands. Nothing is saved.

Use `--port` if 8765 is taken, and update the redirect URL in the app
registration to match. Microsoft ignores the port on `localhost`, so it needs no change.

## Completing tasks

daypeek is **read-only by default**: out of the box it never creates, changes
or completes anything in any app, and `daypeek done` just explains how to turn
it on.

To let it complete tasks, opt in per app (or for every app that supports it):

```toml
[providers.todoist]
allow_write = true

[providers.ticktick]
client_id = "…"
allow_write = true

# or, for every app that supports it:
# [general]
# allow_write = true
```

An app's own `allow_write` wins over the `[general]` one, so
`allow_write = false` under an app keeps it read-only even when the global one
is on. Today two apps support it:

| App | What `done` does |
| --- | --- |
| Todoist | Closes the task, the same as ticking it in Todoist (a recurring task moves to its next date). Your existing API token already allows this. |
| TickTick | Marks the task complete. **Run `daypeek auth ticktick` once more** after turning on `allow_write`: it asks TickTick for write permission (`tasks:write`), which the read-only sign-in didn't include. |

Other apps (Linear, Asana, Trello, Notion, files…) stay read-only.

**How it works:**

1. With `allow_write` on, the dashboard shows a number next to each task from
   an app that allows writes:

   ```
   Tasks
       1 ● Submit expense report         p1  todoist
       2 ● Call the dentist                  ticktick
         ● Review PR #142                p2  linear
   ```

2. `daypeek done 2` completes the task numbered **2 the last time the dashboard
   was drawn**, so the number matches what you saw (even if a task was
   added since). An empty dashboard clears the old numbers, so they cannot
   complete a task from an earlier screen. The numbers last until midnight;
   otherwise, or if you haven't run `daypeek` yet, it fetches fresh and uses
   those numbers.
3. It shows the task and asks `Complete "Call the dentist" in TickTick? [y/N]`.
   `--yes` (`-y`) skips the question; without a terminal you must pass `--yes`.
4. If the same task appears in two writable apps (duplicates are merged on the
   dashboard), it's completed in both. A read-only copy is only mentioned.
5. The app's cached results are cleared, so the next `daypeek` shows the task
   gone. The other numbers stay the same until the dashboard is redrawn.

`daypeek done` needs the network, so it refuses `--offline`.
`daypeek --demo done 2` shows what would happen with the demo data.

## Caching: why it's instant

Each config path and its contents has its own opaque `profile-<digest>/` folder
under the cache directory, including task numbers. Work and personal configs
do not share data, even with the same provider IDs or explicit `cache_dir`.
Changing a config starts a new cache; old unscoped cache files are ignored.
The first run after upgrading fetches fresh data (offline mode needs a new cache).

Opening five terminals in a minute shouldn't call five APIs five times. Each
cloud source (calendar feeds, Todoist and the other web apps, weather) is
reused from `~/.cache/daypeek/` if it was fetched successfully in the last
`cache_minutes` (default 5):

```toml
[general]
cache_minutes = 5      # 0 = always fetch
```

- Reused results look exactly like fresh ones: no marker, because they are fresh.
- Results are never reused across local midnight, so you won't see yesterday's list.
- Local sources (Things, Reminders, Obsidian, files) are always read fresh; they're fast.
- `daypeek --refresh` fetches everything now. `daypeek providers` always does.
- `↻ stale` markers only appear when a fetch actually failed, exactly as before.
- In JSON, a reused source has `"state": "cached"` with its `fetched_at`.

## Reading the dashboard

```
  CALENDAR  7 events
  ▶ 10:30 AM   1:1 with Priya     now · ends in 2m        ← happening now
    11:00 AM   Focus block        in 2m                   ← next up
    9:30 AM    T̶e̶a̶m̶ ̶s̶t̶a̶n̶d̶u̶p̶                              ← already over
  TASKS  8 due · 2 overdue
    ● p1 Submit expense report     2d overdue  todoist
    ● p1 Call the pharmacy                     todoist+reminders   ← same task in two apps, merged
    ↻ ticktick offline, showing cache from 1h 35m ago          ← stale source
```

- **Priorities** are normalized across apps: **p1** red (urgent), **p2**
  orange (high), **p3** blue (medium), **p4** grey (low), and `○` for none.
  Tasks are sorted by priority, then due date (oldest first), then title.
- **De-duplication.** Tasks with the same title (ignoring case and punctuation)
  and the same due date are merged into one row. The merged row keeps the
  highest priority and lists every source.
- **Badges** show each task's `[providers.<id>]` name, so you know where to
  go to complete it.
- **`↻` stale** means that source failed this run and you're seeing its
  last good data, with its age. A source with no cache yet shows a dim `!`
  line instead.
- **NOTE** is the start of today's Obsidian daily note, if you've configured one.

## JSON output and `jq`

`daypeek --json` prints a single object:

| Key | Contents |
| --- | --- |
| `now` | RFC 3339 timestamp in your time zone |
| `date` | `{date, weekday, iso_week, day_of_year, days_in_year, timezone}` (today) |
| `view`, `start`, `end` | `"today"`, `"tomorrow"` or `"week"`, and the first and last day covered |
| `events` | `[{title, all_day, start, end, calendar, location?}]`, sorted. Times are UTC. |
| `calendars` | Per calendar: `{id, name, state, fetched_at?, error?}` |
| `tasks` | `[{title, due, priority, sources, context?}]`, merged and sorted. `priority` is `urgent`, `high`, `medium`, `low`, or `none`. |
| `providers` | Per provider: `{id, name, state, fetched_at?, error?}` |
| `notes` | Daily-note excerpts: `[{title, lines, source}]` |
| `weather` | `{source, freshness, data: {place, temperature, high, low, code, condition, precip_chance, unit, days}}`, or absent. `days` is the 7-day forecast: `[{date, high, low, code, condition, precip_chance}]` |
| `demo` | `true` for `--demo` output |

`state` is `live`, `cached` (with `fetched_at`: reused from the last few
minutes, see [caching](#caching-why-its-instant)), `stale` (with `fetched_at` and
`error`: the fetch failed and cached data is shown), or `failed` (with `error`:
no cache to fall back on).
`weather.freshness` uses the same shape.

Recipes:

```sh
# just the task titles
daypeek --json | jq -r '.tasks[].title'

# urgent tasks with their sources
daypeek --json | jq -r '.tasks[] | select(.priority == "urgent") | "\(.title)  [\(.sources | join(","))]"'

# count of overdue tasks (for a status bar)
daypeek --json --offline | jq --arg d "$(date +%F)" '[.tasks[] | select(.due != null and .due < $d)] | length'

# next event that hasn't started yet, in local time
daypeek --json | jq -r '[.events[] | select(.all_day | not) | select(.start > (now | todate))][0]
                        | "\(.start | fromdate | strflocaltime("%H:%M"))  \(.title)"'

# "68°F Partly cloudy"
daypeek --json --offline | jq -r '.weather.data | "\(.temperature | round)\(.unit) \(.condition)"'

# anything stale or failing?
daypeek --json | jq -r '.calendars[], .providers[] | select(.state == "stale" or .state == "failed") | "\(.id): \(.state) \(.error)"'

# how many tasks are due this week
daypeek week --json | jq '.tasks | length'
```

## Environment variables

| Variable | Used for |
| --- | --- |
| `DAYPEEK_CONFIG` | Config file path |
| `DAYPEEK_TODOIST_TOKEN` (or `TODOIST_API_TOKEN`) | Todoist API token |
| `TICKTICK_ACCESS_TOKEN` | TickTick access token (`TICKTICK_CLIENT_ID` / `TICKTICK_CLIENT_SECRET` for `daypeek auth`) |
| `MSTODO_REFRESH_TOKEN` or `MSTODO_ACCESS_TOKEN` | Microsoft To Do |
| `GTASKS_CLIENT_SECRET` + `GTASKS_REFRESH_TOKEN`, or `GTASKS_ACCESS_TOKEN` | Google Tasks |
| `NOTION_TOKEN` | Notion API token |
| `LINEAR_API_KEY` | Linear personal API key |
| `ASANA_TOKEN` | Asana personal access token |
| `TRELLO_API_KEY` + `TRELLO_TOKEN` | Trello API key and token |
| `DAYPEEK_<NAME>_PASSWORD` | A CalDAV calendar's app password (`iCloud` → `DAYPEEK_ICLOUD_PASSWORD`; change with `password_env`) |
| `NO_COLOR` | Disables color (`--color auto`) |
| `XDG_CONFIG_HOME`, `XDG_CACHE_HOME` | Base directories, if set |

Any provider's env var name can be changed with `token_env` (or
`access_token_env`, `refresh_token_env`, `client_secret_env`, `key_env`) in its config block.
Each one can also come from the macOS Keychain item `daypeek-<id>` or
`daypeek-<id>-<name>` (for example `daypeek-gtasks-refresh-token`).

## Files

| Path | What |
| --- | --- |
| `~/.config/daypeek/config.toml` | Your config |
| `~/.cache/daypeek/` | Last good data per source and view (JSON, mode `0600`). Safe to delete at any time. Change it with `general.cache_dir`. |

## Troubleshooting and FAQ

<a id="auth-missing"></a>
**`○ auth missing` for a provider I set up.**
- Env vars only reach programs started *after* you set them. Open a new
  terminal or `source ~/.zshrc`.
- Check the exact name: `daypeek providers` prints the variable and Keychain
  item it looked for.
- Check the Keychain item exists: `security find-generic-password -s daypeek-todoist -w`.
- A token the app *rejected* (HTTP 401/403) also shows as `auth missing`.
  Create a new one. For Google Tasks in *Testing* mode, refresh tokens expire after 7
  days ([details](SETUP.md#google-tasks)).

**Everything says `↻ cached … ago`.**
That means the network calls failed and you're seeing cached data. Run
`daypeek providers` to see each error. Common causes: no network, a VPN or
proxy blocking the request, or `timeout_secs` too low for a slow connection.
Try `timeout_secs = 8`. Delete `~/.cache/daypeek/` if you want to start fresh.

**A calendar shows no events, or the wrong times.**
- Check that the URL is the **iCal/ICS** link (often ends in `.ics`), not the
  calendar's web page. Paste it into `curl -s "<url>" | head` and you should see
  `BEGIN:VCALENDAR`.
- Events are shown in `general.timezone`, or your system zone if that's not set.
- Google's secret address stops working if it was reset. Copy it again.

**Reminders: "allow your terminal to control Reminders".**
macOS needs Automation permission for each terminal app (Terminal, iTerm2,
Ghostty, and so on). Go to **System Settings → Privacy & Security → Automation**,
find your terminal, and turn on **Reminders**. If it isn't listed, run
`daypeek providers` from that terminal to trigger the prompt. To reset a past
"Don't Allow": `tccutil reset AppleEvents`, then run it again.

**Reminders is slow.**
Big Reminders libraries can take a few seconds to answer. Limit `lists = [...]`
or raise `timeout_secs` in `[providers.reminders]`.

<a id="things-shows-an-error"></a>
**Things shows an error.**
- *Database not found:* is Things 3 installed and opened at least once on this Mac?
  Otherwise, set `db_path` to the `main.sqlite` file (see [setup](SETUP.md#things-3)).
- *Permission error:* allow your terminal to access other apps' data, or give it
  Full Disk Access (**System Settings → Privacy & Security**).
- *SQL / column errors after a Things update:* the database format is
  private and Cultured Code can change it. Please
  [open a bug](https://github.com/Jherin89/daypeek/issues/new?template=bug_report.yml)
  with your Things version (**Things → About Things**).

**macOS says the binary "cannot be opened because the developer cannot be verified".**
Release binaries aren't notarized. Clear the quarantine flag once:
`xattr -d com.apple.quarantine "$(command -v daypeek)"`. You can also build from
source with Cargo.

**`daypeek: unknown timezone "…" in [general]`.**
`general.timezone` must be an IANA name such as `America/New_York` or
`Europe/London` (not `EST` or `New York`). daypeek uses your system time zone
until you fix it, and says so on every run.

**`daypeek: command not found` after the install script.**
The folder it installed into isn't on your `PATH`. The installer printed the
exact line to paste (for zsh: `echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc`);
open a new terminal afterwards. Or install with Homebrew instead.

**Linux: `GLIBC_2.39 not found`.**
The v0.1.0 Linux binary needed a recent glibc (Ubuntu 24.04 or newer). From
v0.2.0 on, Linux gets a static musl build that runs on any distribution:
reinstall with the install script or `brew upgrade daypeek`.

**`daypeek: config: … unknown field`.**
There's a typo in your config. The message names the field and lists the valid
ones. `daypeek` still runs with defaults so your shell never breaks.

**My shell starts slower.**
Only the first terminal in a few minutes fetches anything; the rest reuse those
results ([caching](#caching-why-its-instant)). For zero waiting even then, use
`daypeek --compact --offline` at startup plus a background refresh (see
[setup step 5](SETUP.md#5-run-it-when-a-terminal-opens)), or lower `timeout_secs`.

**Can it mark tasks done?**
Yes, for Todoist and TickTick after you enable `allow_write = true`. Run `daypeek`, then `daypeek done <n>` using the displayed number; it asks before completing the task. TickTick needs a new sign-in for write permission. See [Completing tasks](#completing-tasks).

**Does it work on Windows?**
It isn't tested or released for Windows. The cross-platform providers may build
there, but there are no promises yet.

## Privacy

- **Read-only by default.** Out of the box every provider only reads, and nothing
  in your apps is ever created, changed, or deleted. The one exception is
  `daypeek done`, and only after you set `allow_write = true`: it completes the
  single task you picked (after asking), in Todoist or TickTick
  ([details](#completing-tasks)).
- **Tokens** come from env vars, the macOS Keychain, or your config file. Each
  one is sent only to that app's own API over HTTPS (Todoist → `api.todoist.com`,
  TickTick → `api.ticktick.com`, Microsoft → `graph.microsoft.com` /
  `login.microsoftonline.com`, Google → `tasks.googleapis.com` /
  `oauth2.googleapis.com`, Notion → `api.notion.com`, Linear → `api.linear.app`,
  Asana → `app.asana.com`, Trello → `api.trello.com`, CalDAV passwords → only
  the server you configured, never to another site after a redirect). They're never logged,
  cached, or printed, except by `daypeek auth`, which prints the token once so
  you can store it.
- **Other network calls:** your calendar URLs, and Open-Meteo
  (`geocoding-api.open-meteo.com` and `api.open-meteo.com`) with your city name or
  coordinates. That's all. There's no telemetry, analytics, or update check.
- **Local data** (Things, Reminders, Obsidian, files) never leaves your machine.
- **Cache:** `~/.cache/daypeek/` holds the last good result per source and config profile, readable
  only by your user. Delete it any time.

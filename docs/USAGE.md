# Usage reference

- [Commands at a glance](#commands-at-a-glance)
- [Flags](#flags)
- [Subcommands](#subcommands): [`setup`](#daypeek-setup) · [`providers`](#daypeek-providers) · [`auth`](#daypeek-auth)
- [Reading the dashboard](#reading-the-dashboard)
- [JSON output and `jq`](#json-output-and-jq)
- [Environment variables](#environment-variables)
- [Files](#files)
- [Troubleshooting and FAQ](#troubleshooting-and-faq)
- [Privacy](#privacy)

New here? Run `daypeek setup`, or see the [setup guide](SETUP.md).

## Commands at a glance

```sh
daypeek                       # full dashboard
daypeek --compact             # 2–3 line summary, for shell startup
daypeek --json                # everything as JSON
daypeek --offline             # cached data only, no network
daypeek --demo                # fake data (works without a config)
daypeek setup                 # guided first-run setup (wizard)
daypeek providers             # health check for every configured source
daypeek auth <provider>       # one-time OAuth sign-in: ticktick, mstodo, gtasks
daypeek --init                # write a starter config
```

## Flags

| Flag | What it does |
| --- | --- |
| `--compact` | A short summary: date, time, and weather; the current or next event; how many tasks are due and the most urgent one. Stale sources are flagged with `↻N`. |
| `--json` | Prints the full snapshot as JSON on stdout (see [below](#json-output-and-jq)). There's no color and no task limit. |
| `--demo` | Uses built-in fake data. Reads no config and makes no network calls. Combine it with `--compact`, `--json`, or `providers`. |
| `--offline` | Skips every network source and shows the last cached data, marked stale. Local sources (Things, Obsidian, files) are still read live. |
| `--init` | Writes the commented sample config to `~/.config/daypeek/config.toml` (or the `--config` path). |
| `--force` | With `--init`, overwrites an existing config. |
| `--config <PATH>` | Uses a different config file. Defaults to `$DAYPEEK_CONFIG`, then `$XDG_CONFIG_HOME/daypeek/config.toml`, then `~/.config/daypeek/config.toml`. |
| `--color auto\|always\|never` | `auto` (the default) uses color only when stdout is a terminal and `NO_COLOR` isn't set. |
| `-V`, `--version` | Prints the version. |
| `-h`, `--help` | Prints help. Works on subcommands too, e.g. `daypeek auth --help`. |

Examples:

```sh
daypeek --compact --offline              # instant: cache only
daypeek --color never > ~/today.txt      # plain text
daypeek --config ~/work/daypeek.toml     # a separate work profile
alias dpw='daypeek --config ~/work/daypeek.toml'
```

## Subcommands

### `daypeek setup`

```sh
daypeek setup [--port 8765]
daypeek --demo setup          # practice run: same prompts, nothing saved
```

An interactive wizard that asks for:

1. Your apps (multi-select).
2. Each app's credential, typed hidden. On macOS it goes into the Keychain; on Linux you're shown the env var to set.
3. Your calendars' iCal URLs.
4. A city for weather.

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
`daypeek --demo providers` shows a sample.

### `daypeek auth`

```sh
daypeek auth ticktick | mstodo | gtasks   [--port 8765]
```

A one-time sign-in helper for the three apps whose APIs only accept OAuth. It
uses **your own** app registration (see the [setup guide](SETUP.md)):

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
| `date` | `{date, weekday, iso_week, day_of_year, days_in_year, timezone}` |
| `events` | `[{title, all_day, start, end, calendar, location?}]`, sorted. Times are UTC. |
| `calendars` | Per calendar: `{id, name, state, fetched_at?, error?}` |
| `tasks` | `[{title, due, priority, sources, context?}]`, merged and sorted. `priority` is `urgent`, `high`, `medium`, `low`, or `none`. |
| `providers` | Per provider: `{id, name, state, fetched_at?, error?}` |
| `notes` | Daily-note excerpts: `[{title, lines, source}]` |
| `weather` | `{source, freshness, data: {place, temperature, high, low, code, condition, precip_chance, unit}}`, or `null` |
| `demo` | `true` for `--demo` output |

`state` is `live`, `stale` (with `fetched_at` and `error`: the fetch failed and
cached data is shown), or `failed` (with `error`: no cache to fall back on).
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
daypeek --json | jq -r '.calendars[], .providers[] | select(.state != "live") | "\(.id): \(.state) \(.error)"'
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
| `NO_COLOR` | Disables color (`--color auto`) |
| `XDG_CONFIG_HOME`, `XDG_CACHE_HOME` | Base directories, if set |

Any provider's env var name can be changed with `token_env` (or
`access_token_env`, `refresh_token_env`, `client_secret_env`) in its config block.
Each one can also come from the macOS Keychain item `daypeek-<id>` or
`daypeek-<id>-<name>` (for example `daypeek-gtasks-refresh-token`).

## Files

| Path | What |
| --- | --- |
| `~/.config/daypeek/config.toml` | Your config |
| `~/.cache/daypeek/` | Last good data per source (JSON, mode `0600`). Safe to delete at any time. Change it with `general.cache_dir`. |

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

**`daypeek: config: … unknown field`.**
There's a typo in your config. The message names the field and lists the valid
ones. `daypeek` still runs with defaults so your shell never breaks.

**My shell starts slower.**
Use `daypeek --compact --offline` at startup, plus a background refresh (see
[setup step 5](SETUP.md#5-run-it-when-a-terminal-opens)), or lower `timeout_secs`.

**Can it mark tasks done?**
No. `daypeek` is read-only on purpose. Badges tell you which app to open.

**Does it work on Windows?**
It isn't tested or released for Windows. The cross-platform providers may build
there, but there are no promises yet.

## Privacy

- **Read-only.** Every provider only reads. Nothing in your apps is ever created,
  changed, or deleted.
- **Tokens** come from env vars, the macOS Keychain, or your config file. Each
  one is sent only to that app's own API over HTTPS (Todoist → `api.todoist.com`,
  TickTick → `api.ticktick.com`, Microsoft → `graph.microsoft.com` /
  `login.microsoftonline.com`, Google → `tasks.googleapis.com` /
  `oauth2.googleapis.com`, Notion → `api.notion.com`). They're never logged,
  cached, or printed, except by `daypeek auth`, which prints the token once so
  you can store it.
- **Other network calls:** your calendar URLs, and Open-Meteo
  (`geocoding-api.open-meteo.com` and `api.open-meteo.com`) with your city name or
  coordinates. That's all. There's no telemetry, analytics, or update check.
- **Local data** (Things, Reminders, Obsidian, files) never leaves your machine.
- **Cache:** `~/.cache/daypeek/` holds the last good result per source, readable
  only by your user. Delete it any time.

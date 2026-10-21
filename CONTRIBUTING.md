# Contributing to today

Thanks for helping! The most useful contribution is usually a **new task
provider**. Here's how to add one.

## Adding a provider

Each provider is one file in `src/providers/` that implements the `Provider`
trait:

```rust
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;              // the [providers.<id>] key, used as the badge
    fn kind_id(&self) -> &'static str; // "asana"
    fn name(&self) -> &'static str;    // "Asana"
    fn kind(&self) -> Kind;            // Cloud, Local, or Macos
    fn check(&self) -> Result<()>;     // cheap, no network: credentials/paths present?
    fn fetch(&self, ctx: &Ctx) -> Result<Output>;  // open tasks due today or overdue
    // health() has a default built on check() + fetch()
}
```

1. **Create `src/providers/<name>.rs`.** Copy `todoist.rs` for a cloud API or
   `todotxt.rs` for a local file. You'll need:
   - a `Settings` struct with `#[serde(default, deny_unknown_fields)]` and a
     `Default` impl, so typos in config are caught;
   - `pub fn build(id: &str, table: toml::Table) -> Result<Box<dyn Provider>>`;
   - a **pure** `to_tasks(...)` / `parse(...)` function that turns the API
     response into `Vec<Task>`. Keep the HTTP call in `fetch()` and the logic
     in the pure function, so the logic can be tested without a network.
2. **Register it** in `REGISTRY` in `src/providers/mod.rs` and add `pub mod`.
3. **Map priorities** onto `Priority::{Urgent, High, Medium, Low, None}`.
   Urgent is the service's top level.
4. **Dates.** Return the due date as a `NaiveDate` in the user's zone
   (`ctx.tz`). Cloud APIs often send midnight UTC. Look at
   `ticktick::due_date` and `mstodo::due_date` for how to handle that.
5. **Secrets** go through `secrets::resolve(id, "token", "<APP>_TOKEN", inline)`
   (env, then Keychain, then config). Never log a token, and never embed a
   client ID or secret in the code.
6. **Return errors, never panic.** `Error::AuthMissing` for missing or rejected
   credentials, `Error::Unsupported` for the wrong OS. Everything else is
   shown as a red `!` line, and the dashboard keeps going. 401/403/429 are
   mapped for you by `From<ureq::Error>`.
7. **macOS-only code** sits behind `#[cfg(target_os = "macos")]` with a
   non-macOS stub, so the crate builds and tests everywhere.
8. **Tests.** Save a realistic, anonymized response in
   `tests/fixtures/<name>_*.json` and test your pure parser against it. Cover
   priorities, done items being skipped, undated items, and dates near midnight.
9. **Docs.** Add a row to the "Supported apps" and "Provider reference" tables
   in the README, and a commented block to `SAMPLE_CONFIG` in `src/config.rs`.

## Ground rules

- `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`
  must pass (CI checks all three).
- Read-only access only. `today` never modifies your tasks, notes, or
  calendars.
- No scraping, no private APIs, no bundled credentials.
- Fixtures must be fake data.
- Keep startup fast. Avoid new heavy dependencies (no async runtime).

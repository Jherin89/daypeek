## What does this change?

<!-- One or two sentences. Link an issue if there is one: Closes #123 -->

## Checklist

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test`
- [ ] Still read-only. No scraping, private APIs, or bundled client IDs/secrets
- [ ] No real tokens, calendar URLs, or personal data in code, fixtures, or screenshots
- [ ] Docs updated (README table, `docs/SETUP.md`, `SAMPLE_CONFIG`) if behavior or config changed
- [ ] `CHANGELOG.md` updated under **Unreleased**

### New provider? Also:

- [ ] One file in `src/providers/`, registered in `REGISTRY`
- [ ] Pure parse function tested against an anonymized fixture in `tests/fixtures/`
- [ ] Errors are returned (`AuthMissing`, `Unsupported`, …), never panics
- [ ] macOS-only code is behind `#[cfg(target_os = "macos")]` with a stub
- [ ] Setup steps link the app's official docs

## Screenshot (if output changed)

<!-- `cargo run -- --demo` output or a re-rendered docs/screenshot.png -->

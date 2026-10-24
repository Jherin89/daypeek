//! `--watch`: keep the dashboard open and redraw it in place.
//!
//! Uses the terminal's alternate screen (like `top` or `less`), so your
//! scrollback is untouched and everything is restored on exit: `q`, `Esc`,
//! `Ctrl-C`, or a panic. Each tick redraws with the current time; cloud data
//! is reused from the cache between fetches (see [`min_refetch`]), and `r`
//! fetches everything now.

use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::{cursor, execute, queue, terminal};

/// Shortest allowed `--every`.
pub const MIN_EVERY: u64 = 2;
/// Default `--every`.
pub const DEFAULT_EVERY: u64 = 60;

/// How long cloud results are reused while watching: your `cache_minutes`,
/// but at least a minute, so a short `--every` redraws the clock without
/// hitting every API each time. `r` always fetches.
pub fn min_refetch(cache_minutes: u64) -> chrono::Duration {
    let minutes = i64::try_from(cache_minutes.min(7 * 24 * 60)).unwrap_or(1);
    chrono::Duration::minutes(minutes.max(1))
}

/// What a key press means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    Refresh,
    Ignore,
}

pub fn action(code: KeyCode, mods: KeyModifiers) -> Action {
    match code {
        KeyCode::Char('c') | KeyCode::Char('d') if mods.contains(KeyModifiers::CONTROL) => {
            Action::Quit
        }
        KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => Action::Quit,
        KeyCode::Char('r') | KeyCode::Char('R') => Action::Refresh,
        _ => Action::Ignore,
    }
}

/// One screenful: `text` cut to fit `rows` (the footer always gets the last
/// line), each line followed by "clear to end of line", joined with `\r\n`
/// because raw mode doesn't turn `\n` into a carriage return.
pub fn frame(text: &str, footer: &str, rows: u16) -> String {
    const CLEAR_EOL: &str = "\x1b[K";
    let body_rows = usize::from(rows.max(2)) - 1;
    let mut lines: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
    if lines.len() > body_rows {
        lines.truncate(body_rows);
    }
    let mut out = String::new();
    for l in &lines {
        out.push_str(l.trim_end_matches('\r'));
        out.push_str(CLEAR_EOL);
        out.push_str("\r\n");
    }
    // Blank rows between the dashboard and the footer, so the footer sits at
    // the bottom and stale text from a taller frame is cleared.
    for _ in lines.len()..body_rows {
        out.push_str(CLEAR_EOL);
        out.push_str("\r\n");
    }
    out.push_str(footer);
    out.push_str(CLEAR_EOL);
    out
}

/// Puts the terminal back however we leave: return, `?`, or panic.
struct Guard;

impl Guard {
    fn enter() -> io::Result<Guard> {
        terminal::enable_raw_mode()?;
        let guard = Guard;
        execute!(io::stdout(), terminal::EnterAlternateScreen, cursor::Hide)?;
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore();
            previous(info);
        }));
        Ok(guard)
    }
}

fn restore() {
    let _ = execute!(io::stdout(), cursor::Show, terminal::LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
}

impl Drop for Guard {
    fn drop(&mut self) {
        restore();
    }
}

/// Run the loop. `fetch(force)` gathers fresh data (`force` = the user
/// pressed `r`); `draw(&data, width)` renders it; `footer(width)` is the
/// status line. A resize redraws without fetching.
pub fn run<T>(
    every: Duration,
    mut fetch: impl FnMut(bool) -> T,
    mut draw: impl FnMut(&T, usize) -> String,
    mut footer: impl FnMut(usize) -> String,
) -> Result<(), String> {
    if !io::stdout().is_terminal() || !io::stdin().is_terminal() {
        return Err("--watch needs a terminal (drop it when piping or scripting)".into());
    }
    let io_err = |e: io::Error| format!("terminal: {e}");
    let _guard = Guard::enter().map_err(io_err)?;
    let mut data = fetch(false);
    let mut next = Instant::now() + every;
    loop {
        let (cols, rows) = terminal::size().unwrap_or((80, 24));
        let width = usize::from(cols);
        let text = frame(&draw(&data, width), &footer(width), rows);
        let mut out = io::stdout().lock();
        queue!(out, cursor::MoveTo(0, 0)).map_err(io_err)?;
        out.write_all(text.as_bytes()).map_err(io_err)?;
        queue!(out, terminal::Clear(terminal::ClearType::FromCursorDown)).map_err(io_err)?;
        out.flush().map_err(io_err)?;
        drop(out);

        // Wait for the next tick, a key, or a resize.
        let mut redraw_only = false;
        let mut force = false;
        loop {
            let left = next.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            if !event::poll(left).map_err(io_err)? {
                break;
            }
            match event::read().map_err(io_err)? {
                Event::Key(k) if k.kind != KeyEventKind::Release => {
                    match action(k.code, k.modifiers) {
                        Action::Quit => return Ok(()),
                        Action::Refresh => {
                            force = true;
                            break;
                        }
                        Action::Ignore => {}
                    }
                }
                Event::Resize(..) => {
                    redraw_only = true;
                    break;
                }
                _ => {}
            }
        }
        if !redraw_only {
            data = fetch(force);
            next = Instant::now() + every;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_fit_the_screen_with_the_footer_last() {
        let text = "a\nb\nc\nd\ne\n";
        let f = frame(text, "FOOT", 4);
        let rows: Vec<&str> = f.split("\r\n").collect();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0], "a\x1b[K");
        assert_eq!(rows[2], "c\x1b[K");
        assert_eq!(rows[3], "FOOT\x1b[K");
        assert!(!f.contains("d\x1b"), "cut to fit");
        // A short dashboard is padded so the footer stays at the bottom.
        let f = frame("x\n", "F", 5);
        assert_eq!(f.split("\r\n").count(), 5);
        assert!(f.ends_with("F\x1b[K"));
        assert!(!f.contains("\n\n"), "every newline gets a carriage return");
    }

    #[test]
    fn keys() {
        let none = KeyModifiers::NONE;
        assert_eq!(action(KeyCode::Char('q'), none), Action::Quit);
        assert_eq!(action(KeyCode::Esc, none), Action::Quit);
        assert_eq!(
            action(KeyCode::Char('c'), KeyModifiers::CONTROL),
            Action::Quit
        );
        assert_eq!(action(KeyCode::Char('c'), none), Action::Ignore);
        assert_eq!(action(KeyCode::Char('r'), none), Action::Refresh);
    }

    #[test]
    fn refetch_window_is_at_least_a_minute() {
        assert_eq!(min_refetch(0), chrono::Duration::minutes(1));
        assert_eq!(min_refetch(5), chrono::Duration::minutes(5));
    }
}

//! A small, forgiving iCalendar (RFC 5545) reader: just enough of VEVENT for a
//! daily agenda. Unknown properties and components are ignored.

use chrono::{NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

/// A DTSTART/DTEND/EXDATE-style value, resolved to a wall-clock time in a zone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IcsTime {
    pub naive: NaiveDateTime,
    pub zone: Tz,
    /// `VALUE=DATE` (all-day).
    pub date_only: bool,
}

impl IcsTime {
    pub fn date(&self) -> NaiveDate {
        self.naive.date()
    }

    /// The UTC instant. Times inside a DST gap are shifted forward an hour.
    pub fn to_utc(&self) -> chrono::DateTime<Utc> {
        to_utc(self.zone, self.naive)
    }
}

/// Wall-clock `naive` in `zone` → UTC, tolerating DST gaps and folds.
pub fn to_utc(zone: Tz, naive: NaiveDateTime) -> chrono::DateTime<Utc> {
    match zone.from_local_datetime(&naive) {
        chrono::LocalResult::Single(t) => t.with_timezone(&Utc),
        chrono::LocalResult::Ambiguous(a, _) => a.with_timezone(&Utc),
        chrono::LocalResult::None => {
            let shifted = naive + chrono::Duration::hours(1);
            zone.from_local_datetime(&shifted)
                .earliest()
                .map(|t| t.with_timezone(&Utc))
                .unwrap_or_else(|| Utc.from_utc_datetime(&naive))
        }
    }
}

/// One VEVENT, uninterpreted beyond value parsing.
#[derive(Debug, Clone, Default)]
pub struct RawEvent {
    pub uid: Option<String>,
    pub summary: Option<String>,
    pub location: Option<String>,
    pub status: Option<String>,
    pub dtstart: Option<IcsTime>,
    pub dtend: Option<IcsTime>,
    pub duration: Option<chrono::Duration>,
    pub rrule: Option<String>,
    pub rdates: Vec<IcsTime>,
    pub exdates: Vec<IcsTime>,
    pub recurrence_id: Option<IcsTime>,
}

/// Parse every VEVENT in `text`. `local` is used for floating and date values.
pub fn parse(text: &str, local: Tz) -> Vec<RawEvent> {
    let mut events = Vec::new();
    let mut current: Option<RawEvent> = None;
    // Depth of nested components inside the current VEVENT (VALARM, ...).
    let mut nested = 0usize;

    for line in unfold(text) {
        let Some((name, params, value)) = split_line(&line) else {
            continue;
        };
        match (name.as_str(), value.as_str()) {
            ("BEGIN", "VEVENT") if current.is_none() => {
                current = Some(RawEvent::default());
                nested = 0;
                continue;
            }
            ("END", "VEVENT") if nested == 0 => {
                if let Some(ev) = current.take() {
                    if ev.dtstart.is_some() {
                        events.push(ev);
                    }
                }
                continue;
            }
            ("BEGIN", _) if current.is_some() => {
                nested += 1;
                continue;
            }
            ("END", _) if current.is_some() => {
                nested = nested.saturating_sub(1);
                continue;
            }
            _ => {}
        }
        let Some(ev) = current.as_mut() else { continue };
        if nested > 0 {
            continue;
        }
        match name.as_str() {
            "UID" => ev.uid = Some(value),
            "SUMMARY" => ev.summary = Some(unescape(&value)),
            "LOCATION" => {
                let loc = unescape(&value);
                if !loc.trim().is_empty() {
                    ev.location = Some(loc);
                }
            }
            "STATUS" => ev.status = Some(value.to_ascii_uppercase()),
            "DTSTART" => ev.dtstart = parse_time(&value, &params, local),
            "DTEND" => ev.dtend = parse_time(&value, &params, local),
            "DURATION" => ev.duration = parse_duration(&value),
            "RRULE" => ev.rrule = Some(value),
            "RECURRENCE-ID" => ev.recurrence_id = parse_time(&value, &params, local),
            "EXDATE" => ev.exdates.extend(parse_list(&value, &params, local)),
            "RDATE" => ev.rdates.extend(parse_list(&value, &params, local)),
            _ => {}
        }
    }
    events
}

/// Join folded lines (RFC 5545 §3.1).
fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix([' ', '\t']) {
            if let Some(last) = out.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        if !line.is_empty() {
            out.push(line.to_string());
        }
    }
    out
}

type Params = Vec<(String, String)>;

/// `NAME;P1=a;P2="b:c":value` → (NAME, params, value).
fn split_line(line: &str) -> Option<(String, Params, String)> {
    let mut in_quotes = false;
    let mut colon = None;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let colon = colon?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.trim().to_ascii_uppercase();
    let params = parts
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((
                k.trim().to_ascii_uppercase(),
                v.trim_matches('"').to_string(),
            ))
        })
        .collect();
    Some((name, params, value.trim().to_string()))
}

fn param<'a>(params: &'a Params, key: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push(' '),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out.trim().to_string()
}

fn parse_list(value: &str, params: &Params, local: Tz) -> Vec<IcsTime> {
    value
        .split(',')
        .filter_map(|v| parse_time(v.trim(), params, local))
        .collect()
}

/// Parse a DATE or DATE-TIME with optional `TZID`.
pub fn parse_time(value: &str, params: &Params, local: Tz) -> Option<IcsTime> {
    let is_date = param(params, "VALUE") == Some("DATE") || value.len() == 8;
    if is_date {
        let d = NaiveDate::parse_from_str(value.get(..8)?, "%Y%m%d").ok()?;
        return Some(IcsTime {
            naive: d.and_time(NaiveTime::MIN),
            zone: local,
            date_only: true,
        });
    }
    let (body, utc) = match value.strip_suffix(['Z', 'z']) {
        Some(b) => (b, true),
        None => (value, false),
    };
    let naive = NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S")
        .or_else(|_| NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M"))
        .ok()?;
    let zone = if utc {
        Tz::UTC
    } else {
        param(params, "TZID")
            .and_then(resolve_tzid)
            .unwrap_or(local)
    };
    Some(IcsTime {
        naive,
        zone,
        date_only: false,
    })
}

/// Map a TZID to an IANA zone: IANA names, vendor-prefixed paths like
/// `/mozilla.org/20050126_1/America/New_York`, and common Windows names.
pub fn resolve_tzid(tzid: &str) -> Option<Tz> {
    let tzid = tzid.trim();
    if let Ok(tz) = tzid.parse::<Tz>() {
        return Some(tz);
    }
    let segs: Vec<&str> = tzid.split('/').filter(|s| !s.is_empty()).collect();
    for take in [3, 2] {
        if segs.len() >= take {
            if let Ok(tz) = segs[segs.len() - take..].join("/").parse::<Tz>() {
                return Some(tz);
            }
        }
    }
    let iana = match tzid {
        "Eastern Standard Time" | "US Eastern Standard Time" => "America/New_York",
        "Central Standard Time" => "America/Chicago",
        "Mountain Standard Time" => "America/Denver",
        "US Mountain Standard Time" => "America/Phoenix",
        "Pacific Standard Time" => "America/Los_Angeles",
        "Alaskan Standard Time" => "America/Anchorage",
        "Hawaiian Standard Time" => "Pacific/Honolulu",
        "Atlantic Standard Time" => "America/Halifax",
        "GMT Standard Time" => "Europe/London",
        "W. Europe Standard Time" => "Europe/Berlin",
        "Romance Standard Time" => "Europe/Paris",
        "Central Europe Standard Time" => "Europe/Budapest",
        "E. Europe Standard Time" => "Europe/Bucharest",
        "India Standard Time" => "Asia/Kolkata",
        "China Standard Time" => "Asia/Shanghai",
        "Tokyo Standard Time" => "Asia/Tokyo",
        "AUS Eastern Standard Time" => "Australia/Sydney",
        "UTC" | "Coordinated Universal Time" | "GMT" => "UTC",
        _ => return None,
    };
    iana.parse().ok()
}

/// ISO 8601 duration as used by iCal: `P1D`, `PT1H30M`, `P2W`, `-PT15M`.
pub fn parse_duration(s: &str) -> Option<chrono::Duration> {
    let (neg, s) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let s = s.strip_prefix('P')?;
    let mut secs: i64 = 0;
    let mut num = String::new();
    let mut in_time = false;
    for c in s.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => num.push(c),
            unit => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                secs += n * match (unit, in_time) {
                    ('W', false) => 7 * 86_400,
                    ('D', false) => 86_400,
                    ('H', true) => 3_600,
                    ('M', true) => 60,
                    ('S', true) => 1,
                    _ => return None,
                };
            }
        }
    }
    let d = chrono::Duration::seconds(secs);
    Some(if neg { -d } else { d })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfolds_and_unescapes() {
        let text = "BEGIN:VEVENT\r\nSUMMARY:Long\r\n  title\\, with comma\r\nDTSTART:20261021T130000Z\r\nEND:VEVENT\r\n";
        let evs = parse(text, Tz::UTC);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].summary.as_deref(), Some("Long title, with comma"));
    }

    #[test]
    fn skips_valarm_properties() {
        let text = "BEGIN:VEVENT\nSUMMARY:Real\nDTSTART:20261021T130000Z\nBEGIN:VALARM\nSUMMARY:Alarm\nEND:VALARM\nEND:VEVENT\n";
        let evs = parse(text, Tz::UTC);
        assert_eq!(evs[0].summary.as_deref(), Some("Real"));
    }

    #[test]
    fn tzid_variants() {
        assert_eq!(
            resolve_tzid("America/New_York"),
            Some(chrono_tz::America::New_York)
        );
        assert_eq!(
            resolve_tzid("/mozilla.org/20050126_1/America/New_York"),
            Some(chrono_tz::America::New_York)
        );
        assert_eq!(
            resolve_tzid("Pacific Standard Time"),
            Some(chrono_tz::America::Los_Angeles)
        );
        assert_eq!(resolve_tzid("Mars/Olympus"), None);
    }

    #[test]
    fn durations() {
        assert_eq!(
            parse_duration("PT1H30M"),
            Some(chrono::Duration::minutes(90))
        );
        assert_eq!(parse_duration("P1D"), Some(chrono::Duration::days(1)));
        assert_eq!(parse_duration("P2W"), Some(chrono::Duration::days(14)));
        assert_eq!(
            parse_duration("-PT15M"),
            Some(chrono::Duration::minutes(-15))
        );
        assert_eq!(parse_duration("bogus"), None);
    }

    #[test]
    fn quoted_param_with_colon() {
        let (n, p, v) = split_line("DTSTART;TZID=\"Weird:Zone\":20261021T090000").unwrap();
        assert_eq!(n, "DTSTART");
        assert_eq!(param(&p, "TZID"), Some("Weird:Zone"));
        assert_eq!(v, "20261021T090000");
    }
}

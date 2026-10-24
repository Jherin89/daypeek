//! CalDAV (RFC 4791): find your calendars and download the events in a date
//! range. Works with iCloud, Fastmail, Nextcloud and other standard servers.
//!
//! 1. `PROPFIND` the configured URL for `current-user-principal`,
//!    `calendar-home-set` and `resourcetype` (a calendar's own URL also works).
//! 2. `PROPFIND` the principal for its `calendar-home-set` if needed.
//! 3. `PROPFIND` the home (Depth 1) to list calendars that hold events.
//! 4. `REPORT calendar-query` with a `time-range` per calendar. The VEVENTs
//!    come back as iCalendar text and go through the same parser and
//!    recurrence rules as `.ics` feeds.
//!
//! Sign-in is HTTP Basic over HTTPS with an app-specific password. Redirects
//! are followed by hand so the method and body survive, and the password is
//! only sent on to the same site (e.g. `caldav.icloud.com` →
//! `p42-caldav.icloud.com`).

use chrono::{DateTime, Utc};

use crate::config::CalendarSource;
use crate::error::{Error, Result};
use crate::http::Http;

/// One calendar collection on the server.
#[derive(Debug, Clone, PartialEq)]
pub struct CalendarInfo {
    pub url: String,
    pub name: String,
}

/// Credentials for one account.
pub struct Account<'a> {
    pub url: &'a str,
    pub username: &'a str,
    pub password: &'a str,
}

impl Account<'_> {
    fn basic(&self) -> String {
        use base64::Engine;
        let raw = format!("{}:{}", self.username, self.password);
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(raw)
        )
    }
}

const PROPFIND_START: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <d:resourcetype/>
    <d:displayname/>
    <d:current-user-principal/>
    <c:calendar-home-set/>
  </d:prop>
</d:propfind>"#;

const PROPFIND_HOME_SET: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop><c:calendar-home-set/></d:prop>
</d:propfind>"#;

const PROPFIND_CALENDARS: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <d:resourcetype/>
    <d:displayname/>
    <c:supported-calendar-component-set/>
  </d:prop>
</d:propfind>"#;

/// The `calendar-query` REPORT body for events overlapping `start..end`.
pub fn report_body(start: DateTime<Utc>, end: DateTime<Utc>) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop><c:calendar-data/></d:prop>
  <c:filter>
    <c:comp-filter name="VCALENDAR">
      <c:comp-filter name="VEVENT">
        <c:time-range start="{}" end="{}"/>
      </c:comp-filter>
    </c:comp-filter>
  </c:filter>
</c:calendar-query>"#,
        start.format("%Y%m%dT%H%M%SZ"),
        end.format("%Y%m%dT%H%M%SZ")
    )
}

// ── A tiny XML tree (namespaces dropped; DAV/CalDAV local names are unique
//    enough for what we read) ──────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct Node {
    pub name: String,
    pub text: String,
    pub children: Vec<Node>,
}

impl Node {
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// First descendant called `name` (depth-first).
    pub fn find(&self, name: &str) -> Option<&Node> {
        for c in &self.children {
            if c.name == name {
                return Some(c);
            }
            if let Some(n) = c.find(name) {
                return Some(n);
            }
        }
        None
    }
}

/// Parse XML into a [`Node`] tree rooted at the document element.
pub fn parse_xml(xml: &str) -> Result<Node> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut stack: Vec<Node> = vec![Node::default()];
    let local = |raw: &[u8]| String::from_utf8_lossy(raw).into_owned();
    let bad = |e: &dyn std::fmt::Display| Error::Parse(format!("CalDAV XML: {e}"));
    // Attributes become `@name` children (e.g. `<comp name="VEVENT"/>`).
    let element = |e: &quick_xml::events::BytesStart| Node {
        name: local(e.local_name().as_ref()),
        text: String::new(),
        children: e
            .attributes()
            .flatten()
            .map(|a| Node {
                name: format!("@{}", local(a.key.local_name().as_ref())),
                text: a
                    .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .map(|v| v.into_owned())
                    .unwrap_or_default(),
                children: Vec::new(),
            })
            .collect(),
    };
    loop {
        match reader.read_event().map_err(|e| bad(&e))? {
            Event::Start(e) => stack.push(element(&e)),
            Event::Empty(e) => {
                let node = element(&e);
                if let Some(top) = stack.last_mut() {
                    top.children.push(node);
                }
            }
            Event::End(_) => {
                let done = stack.pop().unwrap_or_default();
                match stack.last_mut() {
                    Some(parent) => parent.children.push(done),
                    None => return Err(Error::Parse("CalDAV XML: unbalanced tags".into())),
                }
            }
            Event::Text(t) => {
                let s = t.xml10_content().map_err(|e| bad(&e))?;
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&s);
                }
            }
            Event::CData(t) => {
                let s = t.decode().map_err(|e| bad(&e))?;
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&s);
                }
            }
            Event::GeneralRef(r) => {
                let resolved = match r.resolve_char_ref().map_err(|e| bad(&e))? {
                    Some(c) => c.to_string(),
                    None => match r.decode().map_err(|e| bad(&e))?.as_ref() {
                        "lt" => "<".into(),
                        "gt" => ">".into(),
                        "amp" => "&".into(),
                        "quot" => "\"".into(),
                        "apos" => "'".into(),
                        _ => String::new(),
                    },
                };
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&resolved);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if stack.len() != 1 {
        return Err(Error::Parse("CalDAV XML: unclosed tags".into()));
    }
    stack
        .pop()
        .and_then(|mut root| root.children.pop())
        .ok_or_else(|| Error::Parse("CalDAV XML: empty response".into()))
}

/// One `<response>` from a 207 Multi-Status: its href and the properties the
/// server answered with 200 OK.
#[derive(Debug, Clone)]
pub struct DavResponse {
    pub href: String,
    pub props: Vec<Node>,
}

impl DavResponse {
    pub fn prop(&self, name: &str) -> Option<&Node> {
        self.props.iter().find(|p| p.name == name)
    }

    fn href_in(&self, prop: &str) -> Option<String> {
        self.prop(prop)
            .and_then(|p| p.find("href"))
            .map(|h| h.text.trim().to_string())
            .filter(|h| !h.is_empty())
    }

    pub fn is_calendar(&self) -> bool {
        self.prop("resourcetype")
            .is_some_and(|r| r.child("calendar").is_some())
    }

    /// Holds events? (Servers that don't say are assumed to.)
    pub fn has_events(&self) -> bool {
        match self.prop("supported-calendar-component-set") {
            None => true,
            Some(set) => set.all("comp").any(|c| {
                c.child("@name")
                    .is_some_and(|n| n.text.eq_ignore_ascii_case("VEVENT"))
            }),
        }
    }

    pub fn name(&self) -> String {
        self.prop("displayname")
            .map(|n| n.text.trim().to_string())
            .unwrap_or_default()
    }
}

/// Parse a `multistatus` body.
pub fn multistatus(xml: &str) -> Result<Vec<DavResponse>> {
    let root = parse_xml(xml)?;
    if root.name != "multistatus" {
        return Err(Error::Parse(format!(
            "CalDAV: expected a multistatus reply, got <{}>",
            root.name
        )));
    }
    Ok(root
        .all("response")
        .map(|r| {
            let href = r
                .child("href")
                .map(|h| h.text.trim().to_string())
                .unwrap_or_default();
            let props = r
                .all("propstat")
                .filter(|ps| {
                    ps.child("status")
                        .is_none_or(|s| s.text.split_whitespace().nth(1) == Some("200"))
                })
                .filter_map(|ps| ps.child("prop"))
                .flat_map(|p| p.children.iter().cloned())
                .collect();
            DavResponse { href, props }
        })
        .collect())
}

// ── HTTP ──────────────────────────────────────────────────────────────────

/// `a` and `b` are the same site: equal hosts, or the same last two labels
/// (`caldav.icloud.com` and `p42-caldav.icloud.com`).
fn same_site(a: &url::Url, b: &url::Url) -> bool {
    let (Some(ha), Some(hb)) = (a.host_str(), b.host_str()) else {
        return false;
    };
    if ha == hb {
        return true;
    }
    let tail = |h: &str| {
        let parts: Vec<&str> = h.rsplit('.').take(2).collect();
        parts.join(".")
    };
    ha.contains('.') && tail(ha) == tail(hb)
}

/// Send one WebDAV request, following redirects by hand. Returns the final
/// URL and the body.
fn dav(
    http: &Http,
    acct: &Account,
    method: &str,
    url: &str,
    depth: &str,
    body: &str,
) -> Result<(url::Url, String)> {
    let first =
        url::Url::parse(url).map_err(|e| Error::Config(format!("CalDAV url {url:?}: {e}")))?;
    let mut current = first.clone();
    for _ in 0..6 {
        let mut req = http
            .request(method, current.as_str())
            .set("Depth", depth)
            .set("Content-Type", "application/xml; charset=utf-8")
            .set("Accept", "application/xml, text/xml");
        // The password only goes to the address you configured (and its
        // own site), and never over plain http to somewhere else.
        // Never send the password to another site, or over plain http
        // to a host it wasn't configured for.
        let secure = current.scheme() == "https" || current.host_str() == first.host_str();
        if secure && same_site(&first, &current) {
            req = req.set("Authorization", &acct.basic());
        }
        match req.send_string(body) {
            Ok(resp) if (300..400).contains(&resp.status()) => {
                let loc = resp.header("Location").ok_or_else(|| {
                    Error::Http(format!(
                        "CalDAV: redirect from {current} without a Location"
                    ))
                })?;
                current = current
                    .join(loc)
                    .map_err(|e| Error::Http(format!("CalDAV redirect {loc:?}: {e}")))?;
            }
            Ok(resp) => {
                let text = resp
                    .into_string()
                    .map_err(|e| Error::Http(format!("CalDAV: {e}")))?;
                return Ok((current, text));
            }
            Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
                return Err(Error::AuthMissing(format!(
                    "{} rejected the username or password (iCloud and Fastmail need an app-specific password)",
                    current.host_str().unwrap_or("the CalDAV server")
                )));
            }
            Err(ureq::Error::Status(code, _)) => {
                return Err(Error::Http(format!(
                    "CalDAV {method} {}: HTTP {code}",
                    current.path()
                )));
            }
            Err(e) => return Err(Error::Http(format!("CalDAV: {e}"))),
        }
    }
    Err(Error::Http("CalDAV: too many redirects".into()))
}

fn resolve(base: &url::Url, href: &str) -> Result<String> {
    base.join(href)
        .map(|u| u.to_string())
        .map_err(|e| Error::Parse(format!("CalDAV href {href:?}: {e}")))
}

/// Find every calendar that holds events for this account.
pub fn discover(http: &Http, acct: &Account) -> Result<Vec<CalendarInfo>> {
    let mut tried_well_known = false;
    let mut url = acct.url.to_string();
    loop {
        let (base, body) = dav(http, acct, "PROPFIND", &url, "0", PROPFIND_START)?;
        let responses = multistatus(&body)?;
        let Some(r) = responses.first() else {
            return Err(Error::Parse("CalDAV: empty PROPFIND reply".into()));
        };
        // The URL is a calendar itself.
        if r.is_calendar() {
            let name = r.name();
            return Ok(vec![CalendarInfo {
                url: base.to_string(),
                name: if name.is_empty() {
                    "Calendar".into()
                } else {
                    name
                },
            }]);
        }
        let home = match r.href_in("calendar-home-set") {
            Some(h) => Some(resolve(&base, &h)?),
            None => match r.href_in("current-user-principal") {
                Some(p) => {
                    let principal = resolve(&base, &p)?;
                    let (pbase, pbody) =
                        dav(http, acct, "PROPFIND", &principal, "0", PROPFIND_HOME_SET)?;
                    let pr = multistatus(&pbody)?;
                    match pr.first().and_then(|r| r.href_in("calendar-home-set")) {
                        Some(h) => Some(resolve(&pbase, &h)?),
                        None => None,
                    }
                }
                None => None,
            },
        };
        if let Some(home) = home {
            return list_calendars(http, acct, &home);
        }
        // Server root with nothing useful: try the well-known address once.
        if !tried_well_known {
            tried_well_known = true;
            url = resolve(&base, "/.well-known/caldav")?;
            continue;
        }
        return Err(Error::Config(format!(
            "couldn't find any calendars at {} (check the server address)",
            acct.url
        )));
    }
}

fn list_calendars(http: &Http, acct: &Account, home: &str) -> Result<Vec<CalendarInfo>> {
    let (base, body) = dav(http, acct, "PROPFIND", home, "1", PROPFIND_CALENDARS)?;
    let mut out = Vec::new();
    for r in multistatus(&body)? {
        if r.is_calendar() && r.has_events() {
            let url = resolve(&base, &r.href)?;
            let name = r.name();
            out.push(CalendarInfo {
                name: if name.is_empty() {
                    url.trim_end_matches('/')
                        .rsplit('/')
                        .next()
                        .unwrap_or("Calendar")
                        .to_string()
                } else {
                    name
                },
                url,
            });
        }
    }
    Ok(out)
}

/// Events in `start..end` from one calendar, as iCalendar text blocks.
pub fn events(
    http: &Http,
    acct: &Account,
    calendar: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<String>> {
    let (_, body) = dav(
        http,
        acct,
        "REPORT",
        calendar,
        "1",
        &report_body(start, end),
    )?;
    Ok(multistatus(&body)?
        .iter()
        .filter_map(|r| r.prop("calendar-data"))
        .map(|d| d.text.trim().to_string())
        .filter(|t| t.contains("BEGIN:VCALENDAR"))
        .collect())
}

/// Pick the calendars named in the config (case-insensitive), or all.
pub fn select(found: Vec<CalendarInfo>, wanted: &[String]) -> Result<Vec<CalendarInfo>> {
    if wanted.is_empty() {
        return Ok(found);
    }
    let mut out = Vec::new();
    for w in wanted {
        match found.iter().find(|c| c.name.eq_ignore_ascii_case(w.trim())) {
            Some(c) => out.push(c.clone()),
            None => {
                let names: Vec<&str> = found.iter().map(|c| c.name.as_str()).collect();
                return Err(Error::Config(format!(
                    "no calendar named {w:?} (found: {})",
                    names.join(", ")
                )));
            }
        }
    }
    Ok(out)
}

/// Everything daypeek needs from one `[[calendars]]` CalDAV entry: the
/// events in `start..end` from each selected calendar, joined into one
/// iCalendar text (cached and parsed like an `.ics` feed).
pub fn fetch(
    http: &Http,
    src: &CalendarSource,
    password: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<String> {
    let username = src.username.as_deref().unwrap_or_default();
    let acct = Account {
        url: src.url.trim(),
        username,
        password,
    };
    let calendars = select(discover(http, &acct)?, &src.calendars)?;
    let results: Vec<Result<Vec<String>>> = std::thread::scope(|s| {
        let handles: Vec<_> = calendars
            .iter()
            .map(|c| {
                let acct = &acct;
                s.spawn(move || events(http, acct, &c.url, start, end))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err(Error::Http("worker panicked".into())))
            })
            .collect()
    });
    let mut blocks = Vec::new();
    for r in results {
        blocks.extend(r?);
    }
    Ok(blocks.join("\r\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testserver::{serve, Request, Response};
    use chrono::TimeZone;
    use std::time::Duration;

    const ROOT: &str = include_str!("../../tests/fixtures/caldav/root.xml");
    const PRINCIPAL: &str = include_str!("../../tests/fixtures/caldav/principal.xml");
    const HOME: &str = include_str!("../../tests/fixtures/caldav/home.xml");
    const REPORT_HOME: &str = include_str!("../../tests/fixtures/caldav/report_home.xml");
    const REPORT_WORK: &str = include_str!("../../tests/fixtures/caldav/report_work.xml");

    /// A fake iCloud-ish server: root → principal → home → two calendars.
    fn handler(req: &Request) -> Response {
        if req.header("authorization") != Some("Basic bWVAZXhhbXBsZS5jb206YXBwLXBhc3M=") {
            return Response::new(401, "");
        }
        match (req.method.as_str(), req.path.as_str()) {
            ("PROPFIND", "/") => Response::new(301, "").header("Location", "/dav/"),
            ("PROPFIND", "/dav/") => Response::new(207, ROOT),
            ("PROPFIND", "/123/principal/") => Response::new(207, PRINCIPAL),
            ("PROPFIND", "/123/calendars/") => Response::new(207, HOME),
            ("REPORT", "/123/calendars/home/") => Response::new(207, REPORT_HOME),
            ("REPORT", "/123/calendars/work/") => Response::new(207, REPORT_WORK),
            _ => Response::new(404, ""),
        }
    }

    fn src(url: &str, calendars: &[&str]) -> CalendarSource {
        let mut s = CalendarSource::ical("Test", url);
        s.kind = crate::config::CalendarKind::Caldav;
        s.username = Some("me@example.com".into());
        s.calendars = calendars.iter().map(|c| c.to_string()).collect();
        s
    }

    fn range() -> (DateTime<Utc>, DateTime<Utc>) {
        (
            Utc.with_ymd_and_hms(2026, 10, 20, 4, 0, 0).unwrap(),
            Utc.with_ymd_and_hms(2026, 10, 29, 4, 0, 0).unwrap(),
        )
    }

    #[test]
    fn discovers_calendars_through_principal_and_home() {
        let server = serve(handler);
        let http = Http::new(Duration::from_secs(5));
        let acct = Account {
            url: &format!("{}/", server.url),
            username: "me@example.com",
            password: "app-pass",
        };
        let found = discover(&http, &acct).unwrap();
        let names: Vec<_> = found.iter().map(|c| c.name.as_str()).collect();
        // The tasks-only (VTODO) collection and the inbox are skipped.
        assert_eq!(names, vec!["Home", "Work & Projects"]);
        assert!(found[0].url.ends_with("/123/calendars/home/"));
        let reqs = server.requests();
        assert_eq!(reqs[0].method, "PROPFIND");
        assert_eq!(reqs[0].header("depth"), Some("0"));
        assert_eq!(reqs[1].path, "/dav/", "followed the redirect with PROPFIND");
        assert_eq!(reqs[1].method, "PROPFIND");
        assert_eq!(reqs.last().unwrap().header("depth"), Some("1"));
    }

    #[test]
    fn fetches_events_and_they_parse_like_ics() {
        let server = serve(handler);
        let http = Http::new(Duration::from_secs(5));
        let (start, end) = range();
        let text = fetch(&http, &src(&server.url, &[]), "app-pass", start, end).unwrap();
        let tz = chrono_tz::America::New_York;
        let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 21).unwrap();
        let titles: Vec<String> = crate::calendar::events_on(&text, "iCloud", day, tz)
            .into_iter()
            .map(|e| e.title)
            .collect();
        assert_eq!(titles, vec!["Standup", "Dentist", "Design review"]);
        // The weekly standup recurs from its master event.
        let next_week = chrono::NaiveDate::from_ymd_opt(2026, 10, 28).unwrap();
        let titles: Vec<String> = crate::calendar::events_on(&text, "iCloud", next_week, tz)
            .into_iter()
            .map(|e| e.title)
            .collect();
        assert_eq!(titles, vec!["Standup"]);
        let report = server
            .requests()
            .into_iter()
            .find(|r| r.method == "REPORT")
            .unwrap();
        assert!(
            report.body.contains(r#"start="20261020T040000Z""#),
            "{}",
            report.body
        );
        assert!(report.body.contains(r#"end="20261029T040000Z""#));
    }

    #[test]
    fn calendar_filter_and_wrong_password() {
        let server = serve(handler);
        let http = Http::new(Duration::from_secs(5));
        let (start, end) = range();
        let text = fetch(
            &http,
            &src(&server.url, &["work & projects"]),
            "app-pass",
            start,
            end,
        )
        .unwrap();
        assert!(text.contains("Design review") && !text.contains("Dentist"));
        let err = fetch(&http, &src(&server.url, &["Nope"]), "app-pass", start, end).unwrap_err();
        assert!(
            err.to_string().contains("found: Home, Work & Projects"),
            "{err}"
        );
        let err = fetch(&http, &src(&server.url, &[]), "wrong", start, end).unwrap_err();
        assert!(matches!(err, Error::AuthMissing(_)), "{err}");
        assert!(err.to_string().contains("app-specific password"));
    }

    #[test]
    fn a_calendar_url_works_directly() {
        let server = serve(|req| match req.method.as_str() {
            "PROPFIND" => Response::new(
                207,
                r#"<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav"><d:response><d:href>/cal/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><cal:calendar/></d:resourcetype><d:displayname>Family</d:displayname></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat><d:propstat><d:prop><d:current-user-principal/></d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat></d:response></d:multistatus>"#,
            ),
            _ => Response::new(207, REPORT_HOME),
        });
        let http = Http::new(Duration::from_secs(5));
        let acct = Account {
            url: &format!("{}/cal/", server.url),
            username: "u",
            password: "p",
        };
        let found = discover(&http, &acct).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Family");
    }

    #[test]
    fn dashboard_shows_caldav_events_and_missing_passwords() {
        let server = serve(handler);
        let cfg = crate::config::Config::parse(&format!(
            r#"
            [general]
            timezone = "America/New_York"
            [[calendars]]
            name = "iCloud"
            type = "caldav"
            url = "{}/"
            username = "me@example.com"
            password = "app-pass"
            password_env = "DAYPEEK_TEST_CALDAV_UNSET"
            [[calendars]]
            name = "No password yet"
            type = "caldav"
            url = "https://caldav.example.com/"
            username = "me@example.com"
            password_env = "DAYPEEK_TEST_CALDAV_UNSET_2"
            "#,
            server.url
        ))
        .unwrap();
        let now = Utc.with_ymd_and_hms(2026, 10, 21, 12, 0, 0).unwrap();
        let snap = crate::snapshot::gather(
            &cfg,
            &crate::cache::Cache::disabled(),
            now,
            crate::snapshot::View::Today,
        );
        let titles: Vec<&str> = snap.events.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, vec!["Standup", "Dentist", "Design review"]);
        assert_eq!(snap.calendars[0].name, "CalDAV");
        assert!(snap.calendars[0].freshness.is_live());
        assert!(crate::snapshot::is_auth_missing(
            &snap.calendars[1].freshness
        ));
        let week = crate::snapshot::gather(
            &cfg,
            &crate::cache::Cache::disabled(),
            now,
            crate::snapshot::View::Week,
        );
        assert_eq!(
            week.events.iter().filter(|e| e.title == "Standup").count(),
            1,
            "one Wednesday in Oct 21-27, and no duplicates from the cached range"
        );
    }

    #[test]
    fn same_site_rules() {
        let u = |s: &str| url::Url::parse(s).unwrap();
        assert!(same_site(
            &u("https://caldav.icloud.com/"),
            &u("https://p42-caldav.icloud.com:443/1/")
        ));
        assert!(!same_site(
            &u("https://caldav.icloud.com/"),
            &u("https://evil.example/")
        ));
        assert!(same_site(
            &u("http://127.0.0.1:9/"),
            &u("http://127.0.0.1:9/x")
        ));
    }

    #[test]
    fn xml_handles_prefixes_entities_and_cdata() {
        let n = parse_xml(
            r#"<?xml version="1.0"?><multistatus xmlns="DAV:"><response><href>/a%20b/</href><propstat><prop><displayname>Tom &amp; Jerry &#x2764;</displayname><calendar-data xmlns="urn:ietf:params:xml:ns:caldav"><![CDATA[BEGIN:VCALENDAR]]></calendar-data></prop><status>HTTP/1.1 200 OK</status></propstat></response></multistatus>"#,
        )
        .unwrap();
        assert_eq!(n.name, "multistatus");
        assert_eq!(n.find("displayname").unwrap().text, "Tom & Jerry ❤");
        assert_eq!(n.find("calendar-data").unwrap().text, "BEGIN:VCALENDAR");
        assert!(multistatus("<html>login</html>").is_err());
    }
}

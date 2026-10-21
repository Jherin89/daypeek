//! RRULE evaluation for "does this rule produce an occurrence on date D?".
//!
//! Supports FREQ=DAILY|WEEKLY|MONTHLY|YEARLY with INTERVAL, COUNT, UNTIL,
//! BYDAY (incl. ordinals like `2TU`, `-1FR`), BYMONTHDAY, BYMONTH, BYSETPOS
//! and WKST — which covers what Google, Apple, and Outlook calendars emit for
//! everyday events. Rules using anything else (BYWEEKNO, BYYEARDAY, sub-daily
//! frequencies) are reported as unsupported and only their first occurrence
//! is shown.

use chrono::{Datelike, Duration, NaiveDate, Weekday};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// UNTIL, kept as a date or an exact UTC instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Until {
    Date(NaiveDate),
    Instant(chrono::DateTime<chrono::Utc>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub freq: Freq,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<Until>,
    pub by_day: Vec<(Option<i32>, Weekday)>,
    pub by_month_day: Vec<i32>,
    pub by_month: Vec<u32>,
    pub by_set_pos: Vec<i32>,
    pub wkst: Weekday,
}

fn weekday(s: &str) -> Option<Weekday> {
    Some(match s {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        _ => return None,
    })
}

impl Rule {
    /// Parse an RRULE value. Returns `Err` for rules we can't evaluate faithfully.
    pub fn parse(s: &str) -> Result<Rule, String> {
        let mut rule = Rule {
            freq: Freq::Daily,
            interval: 1,
            count: None,
            until: None,
            by_day: Vec::new(),
            by_month_day: Vec::new(),
            by_month: Vec::new(),
            by_set_pos: Vec::new(),
            wkst: Weekday::Mon,
        };
        let mut freq = None;
        for part in s.split(';').filter(|p| !p.is_empty()) {
            let (k, v) = part
                .split_once('=')
                .ok_or_else(|| format!("bad RRULE part {part:?}"))?;
            let k = k.to_ascii_uppercase();
            let v = v.to_ascii_uppercase();
            match k.as_str() {
                "FREQ" => {
                    freq = Some(match v.as_str() {
                        "DAILY" => Freq::Daily,
                        "WEEKLY" => Freq::Weekly,
                        "MONTHLY" => Freq::Monthly,
                        "YEARLY" => Freq::Yearly,
                        other => return Err(format!("unsupported FREQ={other}")),
                    })
                }
                "INTERVAL" => rule.interval = v.parse().map_err(|_| "bad INTERVAL")?,
                "COUNT" => rule.count = Some(v.parse().map_err(|_| "bad COUNT")?),
                "UNTIL" => {
                    let t = super::ics::parse_time(&v, &Vec::new(), chrono_tz::UTC)
                        .ok_or("bad UNTIL")?;
                    rule.until = Some(if t.date_only {
                        Until::Date(t.date())
                    } else {
                        Until::Instant(t.to_utc())
                    });
                }
                "BYDAY" => {
                    for d in v.split(',') {
                        let (num, wd) = d.split_at(d.len().saturating_sub(2));
                        let wd = weekday(wd).ok_or_else(|| format!("bad BYDAY {d}"))?;
                        let ord = if num.is_empty() {
                            None
                        } else {
                            Some(num.parse::<i32>().map_err(|_| format!("bad BYDAY {d}"))?)
                        };
                        rule.by_day.push((ord, wd));
                    }
                }
                "BYMONTHDAY" => {
                    for d in v.split(',') {
                        rule.by_month_day
                            .push(d.parse().map_err(|_| "bad BYMONTHDAY")?);
                    }
                }
                "BYMONTH" => {
                    for m in v.split(',') {
                        rule.by_month.push(m.parse().map_err(|_| "bad BYMONTH")?);
                    }
                }
                "BYSETPOS" => {
                    for p in v.split(',') {
                        rule.by_set_pos.push(p.parse().map_err(|_| "bad BYSETPOS")?);
                    }
                }
                "WKST" => rule.wkst = weekday(&v).ok_or("bad WKST")?,
                other => return Err(format!("unsupported RRULE part {other}")),
            }
        }
        rule.freq = freq.ok_or("RRULE without FREQ")?;
        if rule.interval == 0 {
            rule.interval = 1;
        }
        Ok(rule)
    }

    /// Does the rule (anchored at `start`) generate `d`? Ignores COUNT/UNTIL.
    pub fn matches(&self, start: NaiveDate, d: NaiveDate) -> bool {
        if d < start {
            return false;
        }
        if !self.by_month.is_empty() && !self.by_month.contains(&d.month()) {
            return false;
        }
        let interval = i64::from(self.interval);
        match self.freq {
            Freq::Daily => {
                (d - start).num_days() % interval == 0
                    && (self.by_day.is_empty()
                        || self.by_day.iter().any(|(_, w)| *w == d.weekday()))
                    && (self.by_month_day.is_empty() || month_day_matches(&self.by_month_day, d))
            }
            Freq::Weekly => {
                let weeks =
                    (week_start(d, self.wkst) - week_start(start, self.wkst)).num_days() / 7;
                if weeks % interval != 0 {
                    return false;
                }
                if self.by_day.is_empty() {
                    d.weekday() == start.weekday()
                } else {
                    self.by_day.iter().any(|(_, w)| *w == d.weekday())
                }
            }
            Freq::Monthly => {
                let months =
                    (d.year() * 12 + d.month() as i32) - (start.year() * 12 + start.month() as i32);
                months % interval as i32 == 0
                    && self.month_set(start, d.year(), d.month()).contains(&d)
            }
            Freq::Yearly => {
                let years = d.year() - start.year();
                if years % interval as i32 != 0 {
                    return false;
                }
                if self.by_month.is_empty() && d.month() != start.month() {
                    return false;
                }
                self.month_set(start, d.year(), d.month()).contains(&d)
            }
        }
    }

    /// Candidate days in one month for MONTHLY/YEARLY rules, after BYSETPOS.
    fn month_set(&self, start: NaiveDate, year: i32, month: u32) -> Vec<NaiveDate> {
        let days: Vec<NaiveDate> = (1..=days_in_month(year, month))
            .filter_map(|day| NaiveDate::from_ymd_opt(year, month, day))
            .collect();
        let mut set: Vec<NaiveDate> = if !self.by_day.is_empty() {
            days.iter()
                .copied()
                .filter(|d| {
                    self.by_day.iter().any(|(ord, wd)| {
                        d.weekday() == *wd
                            && match ord {
                                None => true,
                                Some(n) => nth_weekday_matches(*d, *n),
                            }
                    })
                })
                .filter(|d| {
                    self.by_month_day.is_empty() || month_day_matches(&self.by_month_day, *d)
                })
                .collect()
        } else if !self.by_month_day.is_empty() {
            days.iter()
                .copied()
                .filter(|d| month_day_matches(&self.by_month_day, *d))
                .collect()
        } else {
            days.iter()
                .copied()
                .filter(|d| d.day() == start.day())
                .collect()
        };
        if !self.by_set_pos.is_empty() {
            let n = set.len() as i32;
            let picked: Vec<NaiveDate> = self
                .by_set_pos
                .iter()
                .filter_map(|&p| {
                    let idx = if p > 0 { p - 1 } else { n + p };
                    (0..n).contains(&idx).then(|| set[idx as usize])
                })
                .collect();
            set = picked;
        }
        set
    }

    /// Occurrence dates (in the event's zone) within `[from, to]`, honoring
    /// COUNT and a date-level UNTIL. Exact-instant UNTIL is checked by the caller.
    pub fn dates_between(
        &self,
        start: NaiveDate,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Vec<NaiveDate> {
        let until_date = match self.until {
            Some(Until::Date(d)) => Some(d),
            Some(Until::Instant(t)) => Some(t.date_naive() + Duration::days(1)),
            None => None,
        };
        let mut out = Vec::new();
        if let Some(count) = self.count {
            // Walk from the start so COUNT is exact. ~50 years max.
            let mut seen = 0u32;
            let mut d = start;
            let limit = start + Duration::days(366 * 50);
            while d <= to && d <= limit && seen < count {
                if d == start || self.matches(start, d) {
                    seen += 1;
                    if d >= from {
                        out.push(d);
                    }
                }
                d += Duration::days(1);
            }
        } else {
            let mut d = from.max(start);
            while d <= to {
                if d == start || self.matches(start, d) {
                    out.push(d);
                }
                d += Duration::days(1);
            }
        }
        if let Some(u) = until_date {
            out.retain(|d| *d <= u);
        }
        out
    }
}

fn week_start(d: NaiveDate, wkst: Weekday) -> NaiveDate {
    let offset = (7 + d.weekday().num_days_from_monday() - wkst.num_days_from_monday()) % 7;
    d - Duration::days(i64::from(offset))
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(ny, nm, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| d.day())
        .unwrap_or(28)
}

fn month_day_matches(list: &[i32], d: NaiveDate) -> bool {
    let dim = days_in_month(d.year(), d.month()) as i32;
    let day = d.day() as i32;
    list.iter()
        .any(|&m| if m > 0 { m == day } else { dim + m + 1 == day })
}

fn nth_weekday_matches(d: NaiveDate, n: i32) -> bool {
    let dim = days_in_month(d.year(), d.month()) as i32;
    let day = d.day() as i32;
    if n > 0 {
        (day - 1) / 7 + 1 == n
    } else {
        (dim - day) / 7 + 1 == -n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn occurs(rule: &str, start: NaiveDate, d: NaiveDate) -> bool {
        Rule::parse(rule).unwrap().dates_between(start, d, d) == vec![d]
    }

    #[test]
    fn weekly_byday() {
        let s = ymd(2026, 1, 5); // Monday
        let r = "FREQ=WEEKLY;BYDAY=MO,WE,FR";
        assert!(occurs(r, s, ymd(2026, 10, 21))); // Wednesday
        assert!(!occurs(r, s, ymd(2026, 10, 20))); // Tuesday
    }

    #[test]
    fn biweekly_interval() {
        let s = ymd(2026, 10, 7); // Wednesday
        let r = "FREQ=WEEKLY;INTERVAL=2";
        assert!(occurs(r, s, ymd(2026, 10, 21)));
        assert!(!occurs(r, s, ymd(2026, 10, 14)));
    }

    #[test]
    fn monthly_nth_and_last_weekday() {
        let s = ymd(2026, 1, 1);
        assert!(occurs("FREQ=MONTHLY;BYDAY=3WE", s, ymd(2026, 10, 21)));
        assert!(!occurs("FREQ=MONTHLY;BYDAY=3WE", s, ymd(2026, 10, 14)));
        assert!(occurs("FREQ=MONTHLY;BYDAY=-1FR", s, ymd(2026, 10, 30)));
        assert!(occurs(
            "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1",
            s,
            ymd(2026, 10, 30)
        ));
    }

    #[test]
    fn monthly_by_month_day_negative() {
        let s = ymd(2026, 1, 31);
        assert!(occurs("FREQ=MONTHLY;BYMONTHDAY=-1", s, ymd(2026, 2, 28)));
        // Plain monthly on the 31st skips short months (RFC 5545).
        assert!(!occurs("FREQ=MONTHLY", s, ymd(2026, 4, 30)));
    }

    #[test]
    fn yearly_birthday() {
        let s = ymd(1990, 10, 21);
        assert!(occurs("FREQ=YEARLY", s, ymd(2026, 10, 21)));
        assert!(!occurs("FREQ=YEARLY", s, ymd(2026, 10, 22)));
    }

    #[test]
    fn count_and_until() {
        let s = ymd(2026, 10, 19);
        assert!(occurs("FREQ=DAILY;COUNT=3", s, ymd(2026, 10, 21)));
        assert!(!occurs("FREQ=DAILY;COUNT=2", s, ymd(2026, 10, 21)));
        assert!(occurs("FREQ=DAILY;UNTIL=20261021", s, ymd(2026, 10, 21)));
        assert!(!occurs(
            "FREQ=DAILY;UNTIL=20261020T235959Z",
            s,
            ymd(2026, 10, 22)
        ));
    }

    #[test]
    fn unsupported_rules_are_errors() {
        assert!(Rule::parse("FREQ=HOURLY").is_err());
        assert!(Rule::parse("FREQ=YEARLY;BYWEEKNO=20").is_err());
        assert!(Rule::parse("INTERVAL=2").is_err());
    }
}

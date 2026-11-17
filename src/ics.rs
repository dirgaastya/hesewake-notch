//! iCalendar (.ics, RFC 5545) reader for the calendar feed: the events that happen in a time window, with recurring
//! events expanded. Covers what Google Calendar and Outlook feeds use: folded lines, UTC / TZID (with the feed's own
//! VTIMEZONE, DST included) / floating / all-day times, RRULE (DAILY, WEEKLY, MONTHLY, YEARLY with INTERVAL, COUNT,
//! UNTIL, BYDAY, BYMONTHDAY, BYMONTH), EXDATE, moved instances (RECURRENCE-ID), cancelled events, and Meet / Zoom /
//! Teams / Webex links.

use std::collections::{HashMap, HashSet};

/// One occurrence of an event. Times are unix seconds (UTC).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Event {
    pub(crate) summary: String,
    pub(crate) start: i64,
    pub(crate) end: i64,
    pub(crate) all_day: bool,
    /// Meet / Zoom / Teams / Webex link, if the event has one.
    pub(crate) link: Option<String>,
}

const D: i64 = 86400;
/// Hosts whose links count as "join the meeting".
const MEETING_HOSTS: [&str; 5] = ["meet.google.com", "zoom.us", "teams.microsoft.com", "teams.live.com", "webex.com"];
/// Most rule periods looked at for one event (a daily rule over ~50 years), and for the whole feed, so a broken or
/// hostile feed (anyone can send an invite) can't keep the fetch thread busy.
const MAX_PERIODS: i64 = 20_000;
const FEED_BUDGET: i64 = 500_000;
/// Each recurring event gets an equal share of the budget whatever its place in the feed (thousands of periods in a
/// normal feed), so costly events (an invite anyone can send) can't use up the share of the user's own. The floor
/// is what a rule needs to jump to the window and read it; it only applies past ~30 000 rules, so the total stays
/// within max(budget, 16 x rules), and the 10 MB download limit keeps that around 2 million cheap periods.
const MIN_SHARE: i64 = 16;

// ---------------------------------------------------------------- civil time

pub(crate) fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub(crate) fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// 0 = Sunday.
fn weekday(day: i64) -> u32 {
    (day + 4).rem_euclid(7) as u32
}

pub(crate) fn days_in_month(y: i64, m: u32) -> u32 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    (days_from_civil(ny, nm, 1) - days_from_civil(y, m, 1)) as u32
}

/// Day of the `nth` weekday `wd` in a month (`nth` < 0 counts from the end), if it exists.
fn nth_weekday(y: i64, m: u32, nth: i32, wd: u32) -> Option<i64> {
    let first = days_from_civil(y, m, 1);
    let last = first + days_in_month(y, m) as i64 - 1;
    let day = if nth > 0 {
        first + ((wd + 7 - weekday(first)) % 7) as i64 + (nth as i64 - 1) * 7
    } else {
        last - ((weekday(last) + 7 - wd) % 7) as i64 + (nth as i64 + 1) * 7
    };
    (first..=last).contains(&day).then_some(day)
}

/// Unix seconds for a UTC date and time (tests build their expected times with it).
#[cfg(test)]
pub(crate) fn unix_utc(y: i32, m: u32, d: u32, h: u32, mi: u32, s: u32) -> i64 {
    days_from_civil(y as i64, m, d) * D + (h * 3600 + mi * 60 + s) as i64
}

// ---------------------------------------------------------------- lines and properties

/// Lines with folding undone (a line starting with a space or tab continues the previous one).
fn unfold(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in s.split('\n') {
        let l = l.strip_suffix('\r').unwrap_or(l);
        match (l.strip_prefix(' ').or_else(|| l.strip_prefix('\t')), out.last_mut()) {
            (Some(rest), Some(prev)) => prev.push_str(rest),
            _ => out.push(l.to_string()),
        }
    }
    out
}

struct Prop {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Prop {
    fn param(&self, k: &str) -> Option<&str> {
        self.params.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
}

/// `NAME;PARAM=V;P2="a:b":value`.
fn parse_prop(line: &str) -> Option<Prop> {
    let mut quoted = false;
    let colon = line.char_indices().find(|&(_, c)| {
        if c == '"' {
            quoted = !quoted;
        }
        c == ':' && !quoted
    })?.0;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.trim().to_ascii_uppercase();
    let params = parts
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.trim().to_ascii_uppercase(), v.trim_matches('"').to_string()))
        .collect();
    Some(Prop { name, params, value: value.to_string() })
}

/// TEXT value escapes: `\,` `\;` `\\` `\n`.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') | Some('N') => out.push('\n'),
            Some(c) => out.push(c),
            None => {}
        }
    }
    out
}

// ---------------------------------------------------------------- date-times

/// A DATE or DATE-TIME as written: `wall` is the clock time as if it were UTC.
#[derive(Clone)]
struct Dt {
    wall: i64,
    date_only: bool,
    utc: bool,
    tz: Option<String>,
}

fn parse_wall(v: &str) -> Option<(i64, bool, bool)> {
    let v = v.trim();
    let num = |a: usize, b: usize| v.get(a..b).filter(|s| s.bytes().all(|c| c.is_ascii_digit())).and_then(|s| s.parse::<u32>().ok());
    let (y, m, d) = (num(0, 4)?, num(4, 6)?, num(6, 8)?);
    if !(1..=12).contains(&m) || d == 0 || d > days_in_month(y as i64, m) {
        return None;
    }
    let day = days_from_civil(y as i64, m, d);
    if v.len() == 8 {
        return Some((day * D, true, false));
    }
    if v.as_bytes().get(8) != Some(&b'T') {
        return None;
    }
    let (h, mi, s) = (num(9, 11)?, num(11, 13)?, num(13, 15)?);
    Some((day * D + (h * 3600 + mi * 60 + s.min(59)) as i64, false, v.ends_with('Z')))
}

fn parse_dt(p: &Prop) -> Option<Dt> {
    let (wall, date_only, utc) = parse_wall(&p.value)?;
    let date_only = date_only || p.param("VALUE") == Some("DATE");
    Some(Dt { wall, date_only, utc, tz: p.param("TZID").map(str::to_string) })
}

/// `PT1H30M`, `P1D`, `P1W` -> seconds.
fn parse_duration(v: &str) -> Option<i64> {
    let v = v.trim().trim_start_matches(['+', '-']).strip_prefix('P')?;
    let (mut total, mut n) = (0i64, 0i64);
    for c in v.chars() {
        match c {
            '0'..='9' => n = n.checked_mul(10)?.checked_add(c.to_digit(10)? as i64)?,
            'T' => {}
            'W' => {
                (total, n) = (total + n * 7 * D, 0);
            }
            'D' => {
                (total, n) = (total + n * D, 0);
            }
            'H' => {
                (total, n) = (total + n * 3600, 0);
            }
            'M' => {
                (total, n) = (total + n * 60, 0);
            }
            'S' => {
                (total, n) = (total + n, 0);
            }
            _ => return None,
        }
    }
    // longer than a year is not a meeting length worth trusting
    Some(total.clamp(0, 366 * D))
}

/// A STANDARD / DAYLIGHT part being read: TZOFFSETTO, DTSTART, yearly rule.
type ObservanceDraft = (Option<i32>, Option<i64>, Option<(u32, i32, u32)>);

/// A VTIMEZONE STANDARD / DAYLIGHT part.
struct Observance {
    offset: i32,
    start: i64,
    /// Yearly onset: (month, nth, weekday), from `RRULE:FREQ=YEARLY;BYMONTH=m;BYDAY=nXX`.
    rule: Option<(u32, i32, u32)>,
}

fn parse_offset(v: &str) -> Option<i32> {
    let v = v.trim();
    let sign = match v.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let h: i32 = v.get(1..3)?.parse().ok()?;
    let m: i32 = v.get(3..5)?.parse().ok()?;
    Some(sign * (h * 3600 + m * 60))
}

struct Zones {
    tz: HashMap<String, Vec<Observance>>,
    local: i32,
}

impl Zones {
    /// Offset (seconds east of UTC) in zone `tz` at wall time `wall`; unknown zones use the PC's zone.
    fn offset(&self, tz: &str, wall: i64) -> i32 {
        let Some(obs) = self.tz.get(tz).filter(|o| !o.is_empty()) else { return self.local };
        let tod = |t: i64| t.rem_euclid(D);
        let (y, _, _) = civil_from_days(wall.div_euclid(D));
        let onset = |o: &Observance| -> Option<i64> {
            match o.rule {
                None => (o.start <= wall).then_some(o.start),
                Some((m, nth, wd)) => [y, y - 1]
                    .iter()
                    .filter_map(|&yy| nth_weekday(yy, m, nth, wd).map(|day| day * D + tod(o.start)))
                    .find(|&t| t <= wall && t >= o.start),
            }
        };
        obs.iter().filter_map(|o| onset(o).map(|t| (t, o.offset))).max_by_key(|&(t, _)| t).map_or(obs[0].offset, |(_, off)| off)
    }

    fn to_utc(&self, dt: &Dt) -> i64 {
        match (&dt.tz, dt.utc, dt.date_only) {
            (_, true, _) => dt.wall,
            (Some(tz), false, false) => dt.wall - self.offset(tz, dt.wall) as i64,
            _ => dt.wall - self.local as i64,
        }
    }
}

// ---------------------------------------------------------------- events

#[derive(Default)]
struct Raw {
    uid: String,
    summary: String,
    start: Option<Dt>,
    end: Option<Dt>,
    duration: Option<i64>,
    rrule: Option<String>,
    exdates: Vec<Dt>,
    recurrence_id: Option<Dt>,
    cancelled: bool,
    /// Where to look for a meeting link, best first.
    link_texts: Vec<(u8, String)>,
}

fn weekday_code(s: &str) -> Option<u32> {
    ["SU", "MO", "TU", "WE", "TH", "FR", "SA"].iter().position(|&c| c == s).map(|i| i as u32)
}

struct Rule {
    freq: String,
    interval: i64,
    count: Option<u32>,
    until: Option<Dt>,
    by_day: Vec<(i32, u32)>,
    by_month_day: Vec<i32>,
    by_month: Vec<u32>,
}

fn parse_rule(s: &str) -> Option<Rule> {
    let mut r = Rule { freq: String::new(), interval: 1, count: None, until: None, by_day: vec![], by_month_day: vec![], by_month: vec![] };
    // real rules list a handful of days or months; very long lists are not worth reading
    if s.matches(',').count() > 100 {
        return None;
    }
    for part in s.split(';') {
        let (k, v) = part.split_once('=')?;
        match k.to_ascii_uppercase().as_str() {
            "FREQ" => r.freq = v.to_ascii_uppercase(),
            "INTERVAL" => r.interval = v.parse::<i64>().ok()?.clamp(1, 10_000),
            "COUNT" => r.count = Some(v.parse::<u64>().ok()?.min(u32::MAX as u64) as u32),
            "UNTIL" => {
                let (wall, date_only, utc) = parse_wall(v)?;
                r.until = Some(Dt { wall, date_only, utc, tz: None });
            }
            "BYDAY" => {
                for d in v.split(',') {
                    let d = d.trim();
                    if !d.is_ascii() || d.len() < 2 {
                        return None;
                    }
                    let (n, code) = d.split_at(d.len() - 2);
                    let nth = if n.is_empty() { 0 } else { n.parse::<i32>().ok().filter(|n| (-5..=5).contains(n) && *n != 0)? };
                    r.by_day.push((nth, weekday_code(&code.to_ascii_uppercase())?));
                }
                r.by_day.sort_unstable();
                r.by_day.dedup();
                r.by_day.truncate(7 * 11);
            }
            "BYMONTHDAY" => {
                r.by_month_day = v.split(',').filter_map(|x| x.trim().parse().ok()).filter(|d: &i32| (-31..=31).contains(d) && *d != 0).collect();
                r.by_month_day.sort_unstable();
                r.by_month_day.dedup();
            }
            "BYMONTH" => {
                r.by_month = v.split(',').filter_map(|x| x.trim().parse().ok()).filter(|m| (1..=12).contains(m)).collect();
                r.by_month.sort_unstable();
                r.by_month.dedup();
            }
            _ => {}
        }
    }
    matches!(r.freq.as_str(), "DAILY" | "WEEKLY" | "MONTHLY" | "YEARLY").then_some(r)
}

/// Days in month (y, m) picked by the rule's BYDAY / BYMONTHDAY, or the start's day of month.
fn month_days(r: &Rule, y: i64, m: u32, d0: u32) -> Vec<i64> {
    let dim = days_in_month(y, m) as i32;
    let first = days_from_civil(y, m, 1);
    let mut v: Vec<i64> = if !r.by_day.is_empty() {
        r.by_day
            .iter()
            .flat_map(|&(nth, wd)| {
                if nth != 0 {
                    nth_weekday(y, m, nth, wd).into_iter().collect::<Vec<_>>()
                } else {
                    (0..dim as i64).map(|i| first + i).filter(|&d| weekday(d) == wd).collect()
                }
            })
            .collect()
    } else if !r.by_month_day.is_empty() {
        r.by_month_day
            .iter()
            .filter_map(|&md| {
                let d = if md > 0 { md } else { dim + md + 1 };
                (1..=dim).contains(&d).then(|| first + d as i64 - 1)
            })
            .collect()
    } else if d0 as i32 <= dim {
        vec![first + d0 as i64 - 1]
    } else {
        vec![]
    };
    v.sort_unstable();
    v.dedup();
    v
}

/// Start wall times (as in `start`) produced by `rule`, up to wall time `to_wall`; earlier periods than `from_wall`
/// are skipped when that doesn't change COUNT.
fn expand(r: &Rule, start: i64, from_wall: i64, to_wall: i64, until_ok: &dyn Fn(i64) -> bool, budget: &mut i64) -> Vec<i64> {
    let (day0, tod) = (start.div_euclid(D), start.rem_euclid(D));
    let (y0, m0, d0) = civil_from_days(day0);
    let months0 = y0 * 12 + m0 as i64 - 1;
    let week0 = day0 - ((weekday(day0) + 6) % 7) as i64;
    // jump ahead to near the window when nothing has to be counted from the start
    // weekday offsets from Monday that a WEEKLY rule uses
    let week_days: Vec<i64> = {
        let mut v: Vec<i64> = if r.by_day.is_empty() { vec![day0 - week0] } else { r.by_day.iter().map(|&(_, wd)| ((wd + 6) % 7) as i64).collect() };
        v.sort_unstable();
        v.dedup();
        v
    };
    // jump to just before the window; with COUNT only where each period makes a known number of occurrences
    let span = {
        let from_day = from_wall.div_euclid(D) - 1;
        let (fy, fm, _) = civil_from_days(from_day);
        let span = match r.freq.as_str() {
            "DAILY" => (from_day - day0) / r.interval,
            "WEEKLY" => (from_day - week0) / (7 * r.interval),
            "MONTHLY" => (fy * 12 + fm as i64 - 1 - months0) / r.interval,
            _ => (fy - y0) / r.interval,
        };
        (span - 1).max(0)
    };
    let mut count = 0u64;
    let first_period = match (r.count, r.freq.as_str()) {
        (None, _) => span,
        (Some(_), "DAILY") => {
            count = span as u64;
            span
        }
        (Some(_), "WEEKLY") if span > 0 => {
            // the first week counts only the days from DTSTART on
            let first = week_days.iter().filter(|&&o| o >= day0 - week0).count() as u64;
            count = first + (span as u64 - 1) * week_days.len() as u64;
            span
        }
        _ => 0,
    };
    if r.count.is_some_and(|c| count >= c as u64) {
        return Vec::new(); // all occurrences were before the window
    }
    let mut out = Vec::new();
    for p in first_period..first_period + MAX_PERIODS {
        *budget -= 1;
        if *budget < 0 {
            break;
        }
        let k = p * r.interval;
        let period_day = match r.freq.as_str() {
            "DAILY" => day0 + k,
            "WEEKLY" => week0 + 7 * k,
            "MONTHLY" => {
                let mi = months0 + k;
                days_from_civil(mi.div_euclid(12), (mi.rem_euclid(12) + 1) as u32, 1)
            }
            _ => days_from_civil(y0 + k, 1, 1),
        };
        if period_day * D > to_wall + D {
            break;
        }
        let days: Vec<i64> = match r.freq.as_str() {
            "DAILY" => vec![day0 + k],
            "WEEKLY" => {
                let ws = week0 + 7 * k;
                week_days.iter().map(|o| ws + o).collect()
            }
            "MONTHLY" => {
                let mi = months0 + k;
                month_days(r, mi.div_euclid(12), (mi.rem_euclid(12) + 1) as u32, d0)
            }
            _ => {
                let y = y0 + k;
                let months = if r.by_month.is_empty() { vec![m0] } else { r.by_month.clone() };
                let yearly = Rule { by_month_day: if r.by_month_day.is_empty() && r.by_day.is_empty() { vec![d0 as i32] } else { r.by_month_day.clone() }, ..clone_rule(r) };
                months.into_iter().filter(|m| (1..=12).contains(m)).flat_map(|m| month_days(&yearly, y, m, d0)).collect()
            }
        };
        for d in days {
            if d < day0 {
                continue;
            }
            let wall = d * D + tod;
            if !until_ok(wall) || r.count.is_some_and(|c| count >= c as u64) {
                return out;
            }
            count += 1;
            if wall <= to_wall {
                out.push(wall);
            }
        }
    }
    out
}

fn clone_rule(r: &Rule) -> Rule {
    Rule { freq: r.freq.clone(), interval: r.interval, count: r.count, until: r.until.clone(), by_day: r.by_day.clone(), by_month_day: r.by_month_day.clone(), by_month: r.by_month.clone() }
}

/// First Meet / Zoom / Teams / Webex link in `text`.
fn meeting_link(text: &str) -> Option<String> {
    let mut rest = text;
    while let Some(i) = rest.find("https://") {
        let url: String = rest[i..].chars().take_while(|c| !c.is_whitespace() && !matches!(c, '<' | '>' | '"' | '\'' | ')' | ',' | ';')).collect();
        let url = url.trim_end_matches(['.', ']']).to_string();
        // only a plain host: no user@, port, backslash or %-escapes that a browser could read as another host
        let after = &url["https://".len()..];
        let host = &after[..after.find(['/', '?', '#']).unwrap_or(after.len())];
        let plain = !host.is_empty() && host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.') && !url.contains('\\');
        let host = host.to_ascii_lowercase();
        if plain && MEETING_HOSTS.iter().any(|h| host == *h || host.ends_with(&format!(".{h}"))) {
            return Some(url);
        }
        rest = &rest[i + "https://".len()..];
    }
    None
}

/// The occurrences that overlap `[from, to)`, sorted by start. `local_offset` (seconds east of UTC) is used for times
/// without a zone and for all-day events.
pub(crate) fn events_between(ics: &str, from: i64, to: i64, local_offset: i32) -> Vec<Event> {
    events_with_budget(ics, from, to, local_offset, FEED_BUDGET).0
}

/// Work done expanding recurring events: rule periods looked at, and whether some event was cut short.
#[derive(Debug)]
struct Work {
    used: i64,
    cut: bool,
}

/// `events_between` with the work budget (rule periods for the whole feed, shared out per event) passed in, and the
/// work done reported back.
fn events_with_budget(ics: &str, from: i64, to: i64, local_offset: i32, budget: i64) -> (Vec<Event>, Work) {
    let mut zones = Zones { tz: HashMap::new(), local: local_offset };
    let mut raws: Vec<Raw> = Vec::new();
    let mut cur: Option<Raw> = None;
    let (mut tz_id, mut tz_obs, mut obs): (Option<String>, Vec<Observance>, Option<ObservanceDraft>) = (None, vec![], None);
    let mut depth_skip = 0u32; // inside VALARM etc. within an event
    for line in unfold(ics) {
        let Some(p) = parse_prop(&line) else { continue };
        let v = p.value.trim();
        match (p.name.as_str(), v) {
            ("BEGIN", "VEVENT") => cur = Some(Raw::default()),
            ("END", "VEVENT") => raws.extend(cur.take()),
            ("BEGIN", "VTIMEZONE") => {
                (tz_id, tz_obs) = (None, vec![]);
            }
            ("END", "VTIMEZONE") => {
                if let Some(id) = tz_id.take() {
                    zones.tz.insert(id, std::mem::take(&mut tz_obs));
                }
            }
            ("BEGIN", "STANDARD" | "DAYLIGHT") => obs = Some((None, None, None)),
            ("END", "STANDARD" | "DAYLIGHT") => {
                if let Some((Some(offset), Some(start), rule)) = obs.take() {
                    tz_obs.push(Observance { offset, start, rule });
                }
            }
            ("BEGIN", _) if cur.is_some() => depth_skip += 1,
            ("END", _) if cur.is_some() && depth_skip > 0 => depth_skip -= 1,
            _ if depth_skip > 0 => {}
            _ => {
                if let Some(o) = obs.as_mut() {
                    match p.name.as_str() {
                        "TZOFFSETTO" => o.0 = parse_offset(v),
                        "DTSTART" => o.1 = parse_wall(v).map(|w| w.0),
                        "RRULE" => {
                            o.2 = parse_rule(v).and_then(|r| Some((*r.by_month.first()?, r.by_day.first()?.0, r.by_day.first()?.1)));
                        }
                        _ => {}
                    }
                } else if let Some(e) = cur.as_mut() {
                    match p.name.as_str() {
                        "UID" => e.uid = v.to_string(),
                        "SUMMARY" => e.summary = unescape(v).replace('\n', " ").trim().to_string(),
                        "DTSTART" => e.start = parse_dt(&p),
                        "DTEND" => e.end = parse_dt(&p),
                        "DURATION" => e.duration = parse_duration(v),
                        "RRULE" => e.rrule = Some(v.to_string()),
                        "EXDATE" => {
                            for x in v.split(',') {
                                let q = Prop { name: String::new(), params: p.params.clone(), value: x.to_string() };
                                e.exdates.extend(parse_dt(&q));
                            }
                        }
                        "RECURRENCE-ID" => e.recurrence_id = parse_dt(&p),
                        "STATUS" => e.cancelled = v.eq_ignore_ascii_case("CANCELLED"),
                        "X-GOOGLE-CONFERENCE" => e.link_texts.push((0, unescape(v))),
                        "URL" => e.link_texts.push((1, unescape(v))),
                        "LOCATION" => e.link_texts.push((2, unescape(v))),
                        "DESCRIPTION" => e.link_texts.push((3, unescape(v))),
                        _ => {}
                    }
                } else if p.name == "TZID" {
                    tz_id = Some(v.to_string());
                }
            }
        }
    }

    // instances replaced by a moved or cancelled copy: (uid, original start)
    let replaced: HashSet<(String, i64)> =
        raws.iter().filter_map(|e| e.recurrence_id.as_ref().map(|r| (e.uid.clone(), zones.to_utc(r)))).collect();
    let mut out = Vec::new();
    let rules = raws.iter().filter(|e| e.rrule.is_some() && e.recurrence_id.is_none()).count().max(1) as i64;
    let share = (budget / rules).clamp(MIN_SHARE, MAX_PERIODS);
    let mut work = Work { used: 0, cut: false };
    for e in &raws {
        let Some(start) = &e.start else { continue };
        if e.cancelled {
            continue;
        }
        let start_utc = zones.to_utc(start);
        let len = match (&e.end, e.duration) {
            (Some(end), _) => (zones.to_utc(end) - start_utc).max(0),
            (None, Some(d)) => d,
            (None, None) if start.date_only => D,
            _ => 0,
        };
        let link = {
            let mut texts = e.link_texts.clone();
            texts.sort_by_key(|(rank, _)| *rank);
            texts.iter().find_map(|(_, t)| meeting_link(t))
        };
        let at = |wall: i64| zones.to_utc(&Dt { wall, ..start.clone() });
        let walls = match e.rrule.as_deref().and_then(parse_rule).filter(|_| e.recurrence_id.is_none()) {
            Some(rule) => {
                let until_ok = |wall: i64| match &rule.until {
                    None => true,
                    Some(u) if u.date_only => wall.div_euclid(D) <= u.wall.div_euclid(D),
                    Some(u) if u.utc => at(wall) <= u.wall,
                    Some(u) => wall <= u.wall,
                };
                // the window in this event's wall time, with a day of slack for the zone offset
                let shift = start.wall - start_utc;
                let mut mine = share;
                let walls = expand(&rule, start.wall, from + shift - D, to + shift + D, &until_ok, &mut mine);
                work.used += share - mine.max(0);
                work.cut |= mine < 0;
                walls
            }
            None => vec![start.wall],
        };
        let excluded: HashSet<i64> = e.exdates.iter().map(|x| zones.to_utc(x)).collect();
        for wall in walls {
            let s = at(wall);
            if e.recurrence_id.is_none() && (excluded.contains(&s) || replaced.contains(&(e.uid.clone(), s))) {
                continue;
            }
            let end = s + len;
            if s < to && (end > from || (end == s && s >= from)) {
                out.push(Event { summary: e.summary.clone(), start: s, end, all_day: start.date_only, link: link.clone() });
            }
        }
    }
    if work.cut && !cfg!(test) {
        crate::perf::note("calendar: some recurring events needed more work than allowed and were cut short");
    }
    out.sort_by(|a, b| (a.start, &a.summary).cmp(&(b.start, &b.summary)));
    (out, work)
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = 3600;
    const D: i64 = 86400;
    /// Jakarta is UTC+7 (no DST).
    const WIB: i32 = 7 * 3600;

    fn cal(events: &str) -> String {
        format!(
            "BEGIN:VCALENDAR\r\nPRODID:-//Google Inc//Google Calendar 70.9054//EN\r\nVERSION:2.0\r\nX-WR-TIMEZONE:Asia/Jakarta\r\n\
             BEGIN:VTIMEZONE\r\nTZID:Asia/Jakarta\r\nX-LIC-LOCATION:Asia/Jakarta\r\nBEGIN:STANDARD\r\nTZOFFSETFROM:+0700\r\n\
             TZOFFSETTO:+0700\r\nTZNAME:WIB\r\nDTSTART:19700101T000000\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\n\
             BEGIN:VTIMEZONE\r\nTZID:America/New_York\r\nBEGIN:DAYLIGHT\r\nTZOFFSETFROM:-0500\r\nTZOFFSETTO:-0400\r\n\
             TZNAME:EDT\r\nDTSTART:19700308T020000\r\nRRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=2SU\r\nEND:DAYLIGHT\r\n\
             BEGIN:STANDARD\r\nTZOFFSETFROM:-0400\r\nTZOFFSETTO:-0500\r\nTZNAME:EST\r\nDTSTART:19701101T020000\r\n\
             RRULE:FREQ=YEARLY;BYMONTH=11;BYDAY=1SU\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\n{events}END:VCALENDAR\r\n"
        )
    }

    fn ev(lines: &str) -> String {
        format!("BEGIN:VEVENT\r\nDTSTAMP:20261001T000000Z\r\n{lines}END:VEVENT\r\n")
    }

    /// Monday 26 Oct 2026 00:00 Jakarta time .. one week later.
    fn week() -> (i64, i64) {
        let from = unix_utc(2026, 10, 25, 17, 0, 0);
        (from, from + 7 * D)
    }

    fn starts(v: &[Event]) -> Vec<i64> {
        v.iter().map(|e| e.start).collect()
    }

    #[test]
    fn unix_time_from_a_date() {
        assert_eq!(unix_utc(1970, 1, 1, 0, 0, 0), 0);
        assert_eq!(unix_utc(2026, 10, 28, 2, 0, 0), 1_793_152_800);
        assert_eq!(unix_utc(2024, 2, 29, 12, 0, 0), 1_709_208_000, "leap day");
    }

    #[test]
    fn utc_time_and_unfolded_escaped_summary() {
        let ics = cal(&ev("UID:a\r\nDTSTART:20261028T020000Z\r\nDTEND:20261028T030000Z\r\nSUMMARY:Sprint Review\\, Q4\r\n  \\; demo\r\n"));
        let (from, to) = week();
        let v = events_between(&ics, from, to, WIB);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].summary, "Sprint Review, Q4 ; demo", "folded line joined, \\, and \\; unescaped");
        assert_eq!((v[0].start, v[0].end), (unix_utc(2026, 10, 28, 2, 0, 0), unix_utc(2026, 10, 28, 3, 0, 0)));
        assert!(!v[0].all_day);
    }

    #[test]
    fn tzid_uses_the_feeds_timezone() {
        let ics = cal(&ev("UID:a\r\nDTSTART;TZID=Asia/Jakarta:20261028T090000\r\nDTEND;TZID=Asia/Jakarta:20261028T093000\r\nSUMMARY:Standup\r\n"));
        let (from, to) = week();
        let v = events_between(&ics, from, to, 0);
        assert_eq!(v[0].start, unix_utc(2026, 10, 28, 2, 0, 0), "09:00 WIB = 02:00 UTC, whatever the PC's zone");
        assert_eq!(v[0].end - v[0].start, 30 * 60);
    }

    #[test]
    fn daylight_saving_from_vtimezone() {
        // 09:00 New York every Wednesday: EDT (UTC-4) until 1 Nov 2026, then EST (UTC-5)
        let ics = cal(&ev("UID:a\r\nDTSTART;TZID=America/New_York:20261021T090000\r\nDURATION:PT1H\r\nRRULE:FREQ=WEEKLY\r\nSUMMARY:Sync\r\n"));
        let from = unix_utc(2026, 10, 26, 0, 0, 0);
        let v = events_between(&ics, from, from + 14 * D, 0);
        assert_eq!(starts(&v), [unix_utc(2026, 10, 28, 13, 0, 0), unix_utc(2026, 11, 4, 14, 0, 0)]);
        assert_eq!(v[0].end - v[0].start, H, "DURATION:PT1H");
    }

    #[test]
    fn floating_time_and_all_day_use_the_local_zone() {
        let ics = cal(&(ev("UID:a\r\nDTSTART:20261028T090000\r\nDTEND:20261028T100000\r\nSUMMARY:Local\r\n")
            + &ev("UID:b\r\nDTSTART;VALUE=DATE:20261029\r\nDTEND;VALUE=DATE:20261030\r\nSUMMARY:Holiday\r\n")));
        let (from, to) = week();
        let v = events_between(&ics, from, to, WIB);
        assert_eq!(v[0].start, unix_utc(2026, 10, 28, 2, 0, 0));
        assert_eq!(v[1].summary, "Holiday");
        assert!(v[1].all_day);
        assert_eq!((v[1].start, v[1].end), (unix_utc(2026, 10, 28, 17, 0, 0), unix_utc(2026, 10, 29, 17, 0, 0)), "local midnight to midnight");
    }

    #[test]
    fn weekly_rule_with_days_and_an_excluded_date() {
        let ics = cal(&ev("UID:s\r\nDTSTART;TZID=Asia/Jakarta:20261005T090000\r\nDTEND;TZID=Asia/Jakarta:20261005T091500\r\n\
             RRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR\r\nEXDATE;TZID=Asia/Jakarta:20261030T090000\r\nSUMMARY:Daily Standup\r\n"));
        let (from, to) = week();
        let v = events_between(&ics, from, to, WIB);
        assert_eq!(starts(&v), [unix_utc(2026, 10, 26, 2, 0, 0), unix_utc(2026, 10, 28, 2, 0, 0)], "Mon, Wed; Fri excluded");
        assert!(v.iter().all(|e| e.summary == "Daily Standup" && e.end - e.start == 15 * 60));
    }

    #[test]
    fn count_until_and_interval() {
        let daily3 = ev("UID:c\r\nDTSTART:20261026T010000Z\r\nDTEND:20261026T013000Z\r\nRRULE:FREQ=DAILY;COUNT=3\r\nSUMMARY:Three\r\n");
        let until = ev("UID:u\r\nDTSTART:20261026T050000Z\r\nDTEND:20261026T053000Z\r\nRRULE:FREQ=DAILY;UNTIL=20261027T235959Z\r\nSUMMARY:Two\r\n");
        let every2 = ev("UID:i\r\nDTSTART:20261012T060000Z\r\nDTEND:20261012T063000Z\r\nRRULE:FREQ=WEEKLY;INTERVAL=2\r\nSUMMARY:Biweekly\r\n");
        let (from, to) = week();
        let v = events_between(&cal(&(daily3 + &until + &every2)), from, to, WIB);
        let n = |s: &str| v.iter().filter(|e| e.summary == s).count();
        assert_eq!((n("Three"), n("Two")), (3, 2));
        assert_eq!(n("Biweekly"), 1, "12 Oct, 26 Oct (in the window), 9 Nov");
        let v = events_between(&cal(&ev("UID:i\r\nDTSTART:20261019T060000Z\r\nDTEND:20261019T063000Z\r\nRRULE:FREQ=WEEKLY;INTERVAL=2\r\nSUMMARY:Off\r\n")), from, to, WIB);
        assert!(v.is_empty(), "19 Oct, 2 Nov: none in this week");
    }

    #[test]
    fn monthly_and_yearly_rules() {
        let by_day = ev("UID:m\r\nDTSTART:20260105T020000Z\r\nDTEND:20260105T030000Z\r\nRRULE:FREQ=MONTHLY;BYDAY=-1WE\r\nSUMMARY:Last Wednesday\r\n");
        let by_date = ev("UID:d\r\nDTSTART:20260127T020000Z\r\nDTEND:20260127T030000Z\r\nRRULE:FREQ=MONTHLY\r\nSUMMARY:Payday\r\n");
        let yearly = ev("UID:y\r\nDTSTART;VALUE=DATE:19900101\r\nRRULE:FREQ=YEARLY;BYMONTH=10;BYMONTHDAY=29\r\nSUMMARY:Birthday\r\n");
        let (from, to) = week();
        let v = events_between(&cal(&(by_day + &by_date + &yearly)), from, to, WIB);
        let at = |s: &str| v.iter().find(|e| e.summary == s).map(|e| e.start);
        assert_eq!(at("Last Wednesday"), Some(unix_utc(2026, 10, 28, 2, 0, 0)));
        assert_eq!(at("Payday"), Some(unix_utc(2026, 10, 27, 2, 0, 0)));
        assert_eq!(at("Birthday"), Some(unix_utc(2026, 10, 28, 17, 0, 0)));
    }

    #[test]
    fn moved_and_cancelled_instances() {
        let master = ev("UID:s\r\nDTSTART;TZID=Asia/Jakarta:20261026T090000\r\nDTEND;TZID=Asia/Jakarta:20261026T091500\r\nRRULE:FREQ=DAILY;COUNT=4\r\nSUMMARY:Standup\r\n");
        let moved = ev("UID:s\r\nRECURRENCE-ID;TZID=Asia/Jakarta:20261027T090000\r\nDTSTART;TZID=Asia/Jakarta:20261027T130000\r\nDTEND;TZID=Asia/Jakarta:20261027T131500\r\nSUMMARY:Standup (moved)\r\n");
        let cancelled = ev("UID:s\r\nRECURRENCE-ID;TZID=Asia/Jakarta:20261028T090000\r\nDTSTART;TZID=Asia/Jakarta:20261028T090000\r\nSTATUS:CANCELLED\r\nSUMMARY:Standup\r\n");
        let gone = ev("UID:x\r\nDTSTART:20261029T020000Z\r\nDTEND:20261029T030000Z\r\nSTATUS:CANCELLED\r\nSUMMARY:Called off\r\n");
        let (from, to) = week();
        let v = events_between(&cal(&(master + &moved + &cancelled + &gone)), from, to, WIB);
        let got: Vec<_> = v.iter().map(|e| (e.summary.as_str(), e.start)).collect();
        assert_eq!(
            got,
            [
                ("Standup", unix_utc(2026, 10, 26, 2, 0, 0)),
                ("Standup (moved)", unix_utc(2026, 10, 27, 6, 0, 0)),
                ("Standup", unix_utc(2026, 10, 29, 2, 0, 0)),
            ]
        );
    }

    #[test]
    fn window_keeps_events_that_overlap_it() {
        let ics = cal(&ev("UID:a\r\nDTSTART:20261028T020000Z\r\nDTEND:20261028T040000Z\r\nSUMMARY:Long\r\n"));
        let mid = unix_utc(2026, 10, 28, 3, 0, 0);
        assert_eq!(events_between(&ics, mid, mid + H, 0).len(), 1, "started before the window, still running");
        assert!(events_between(&ics, mid + 2 * H, mid + 3 * H, 0).is_empty());
    }

    #[test]
    fn meeting_links() {
        let meet = ev("UID:1\r\nDTSTART:20261028T020000Z\r\nDTEND:20261028T030000Z\r\nSUMMARY:Meet\r\n\
             DESCRIPTION:Join with Google Meet: https://meet.google.com/abc-defg-hij\\n\\nLearn more\r\n\
             X-GOOGLE-CONFERENCE:https://meet.google.com/abc-defg-hij\r\n");
        let zoom = ev("UID:2\r\nDTSTART:20261028T030000Z\r\nDTEND:20261028T040000Z\r\nSUMMARY:Zoom\r\nLOCATION:https://us02web.zoom.us/j/8123456789?pwd=Ab\r\n");
        let teams = ev("UID:3\r\nDTSTART:20261028T040000Z\r\nDTEND:20261028T050000Z\r\nSUMMARY:Teams\r\n\
             DESCRIPTION:Microsoft Teams meeting\\nJoin: <https://teams.microsoft.com/l/meetup-join/19%3ameeting_x%40thread.v2/0?context=y>\\nMeeting ID\r\n");
        let none = ev("UID:4\r\nDTSTART:20261028T050000Z\r\nDTEND:20261028T060000Z\r\nSUMMARY:Lunch\r\nLOCATION:Warung\\, https://maps.google.com/x\r\n");
        let (from, to) = week();
        let v = events_between(&cal(&(meet + &zoom + &teams + &none)), from, to, WIB);
        let link = |s: &str| v.iter().find(|e| e.summary == s).unwrap().link.clone();
        assert_eq!(link("Meet").as_deref(), Some("https://meet.google.com/abc-defg-hij"));
        assert_eq!(link("Zoom").as_deref(), Some("https://us02web.zoom.us/j/8123456789?pwd=Ab"));
        assert_eq!(link("Teams").as_deref(), Some("https://teams.microsoft.com/l/meetup-join/19%3ameeting_x%40thread.v2/0?context=y"));
        assert_eq!(link("Lunch"), None, "a map link is not a meeting");
    }

    /// Anyone can send a calendar invite, so links in a feed are untrusted: only a plain https URL whose host really
    /// is a meeting service counts.
    #[test]
    fn disguised_links_are_not_meeting_links() {
        for bad in [
            "https://evil.com\\@x.zoom.us/j/1",
            "https://meet.google.com@evil.com/abc",
            "https://evil.com%2F@meet.google.com/abc",
            "https://evil.com\\meet.google.com/abc",
            "https://meet.google.com.evil.com/abc",
            "https://xzoom.us/j/1",
            "https://meet.google.com:443@evil.com/",
        ] {
            assert_eq!(meeting_link(bad), None, "{bad:?}");
        }
        assert_eq!(meeting_link("Join: https://us02web.zoom.us/j/81?pwd=Ab").as_deref(), Some("https://us02web.zoom.us/j/81?pwd=Ab"));
        assert_eq!(meeting_link("https://meet.google.com/abc-defg-hij.").as_deref(), Some("https://meet.google.com/abc-defg-hij"));
    }

    /// The release build aborts on panic, so a hostile feed must never panic the fetch thread.
    #[test]
    fn hostile_values_do_not_panic() {
        let (from, to) = week();
        for rule in ["FREQ=WEEKLY;BYDAY=\u{e9}\u{e9}", "FREQ=WEEKLY;BYDAY=1\u{e9}", "FREQ=DAILY;INTERVAL=999999999999999999", "FREQ=MONTHLY;BYDAY=99999999999MO", "FREQ=DAILY;COUNT=99999999999"] {
            let ics = cal(&ev(&format!("UID:h\r\nDTSTART:20261026T010000Z\r\nDURATION:PT99999999999999999999H\r\nRRULE:{rule}\r\nSUMMARY:x\r\n")));
            let _ = events_between(&ics, from, to, WIB);
        }
        let ics = cal(&ev("UID:h\r\nDTSTART:99991231T235959Z\r\nDTEND:00010101T000000Z\r\nRRULE:FREQ=YEARLY;INTERVAL=9999\r\nSUMMARY:x\r\n"));
        let _ = events_between(&ics, i64::MIN / 4, i64::MAX / 4, WIB);
    }

    /// Thousands of recurring events with long BYDAY lists still finish quickly.
    #[test]
    fn work_stays_bounded_for_huge_feeds() {
        // ~6 MB, under the 10 MB download limit: long lists, and many rules counted from 1900
        let by_day = vec!["MO,TU,WE,TH,FR,SA,SU"; 300].join(",");
        let long = ev(&format!("UID:b\r\nDTSTART:19000101T010000Z\r\nDTEND:19000101T011500Z\r\nRRULE:FREQ=MONTHLY;COUNT=999999;BYDAY={by_day}\r\nSUMMARY:x\r\n"));
        let busy = ev("UID:c\r\nDTSTART:19000101T010000Z\r\nDTEND:19000101T011500Z\r\nRRULE:FREQ=MONTHLY;COUNT=999999;BYDAY=MO,TU,WE,TH,FR,SA,SU\r\nSUMMARY:x\r\n");
        let ics = cal(&(long.repeat(900) + &busy.repeat(2000)));
        assert!(ics.len() < 10_000_000);
        let (from, to) = week();
        let t = std::time::Instant::now();
        let (_, work) = events_with_budget(&ics, from, to, WIB, FEED_BUDGET);
        assert!(work.cut, "the budget stopped the work");
        // without the budget this took hours; the limit is loose for slow debug builds on CI
        assert!(t.elapsed().as_secs_f32() < 30.0, "took {:?}", t.elapsed());
        let (normal, work) = events_with_budget(&cal(&ev("UID:n\r\nDTSTART:20261026T010000Z\r\nDTEND:20261026T011500Z\r\nRRULE:FREQ=DAILY\r\nSUMMARY:n\r\n")), from, to, WIB, FEED_BUDGET);
        assert_eq!(normal.len(), 7);
        assert!(work.used <= 20 && !work.cut, "an ordinary rule costs a few periods: {work:?}");
    }

    /// Costly invites placed first in the feed must not use up the work budget and hide the user's own meetings.
    #[test]
    fn a_costly_invite_cannot_hide_real_meetings() {
        // monthly from year 1 with COUNT: can't skip ahead, so each one costs the per-event maximum
        let costly = ev("UID:x\r\nDTSTART:00010101T010000Z\r\nDTEND:00010101T011500Z\r\nRRULE:FREQ=MONTHLY;COUNT=999999;BYDAY=MO,TU,WE,TH,FR\r\nSUMMARY:spam\r\n");
        let standup = ev("UID:s\r\nDTSTART:20000103T020000Z\r\nDTEND:20000103T021500Z\r\nRRULE:FREQ=WEEKLY;COUNT=5000;BYDAY=MO,WE\r\nSUMMARY:Standup\r\n");
        let review = ev("UID:r\r\nDTSTART:20261028T070000Z\r\nDTEND:20261028T080000Z\r\nSUMMARY:Review\r\n");
        let ics = cal(&(costly.repeat(40) + &standup + &review));
        let (from, to) = week();
        let v = events_between(&ics, from, to, WIB);
        let mine: Vec<_> = v.iter().filter(|e| e.summary != "spam").map(|e| (e.summary.as_str(), e.start)).collect();
        assert_eq!(
            mine,
            [("Standup", unix_utc(2026, 10, 26, 2, 0, 0)), ("Standup", unix_utc(2026, 10, 28, 2, 0, 0)), ("Review", unix_utc(2026, 10, 28, 7, 0, 0))]
        );
    }

    /// COUNT on a daily / weekly rule is worked out without walking from the start, and still ends at the right time.
    #[test]
    fn count_skips_ahead_and_still_ends() {
        let (from, to) = week();
        // 2 per week since Mon 3 Jan 2000: Mon 26 Oct 2026 is the 2 799th, so COUNT=2798 ended last week
        let ended = ev("UID:e\r\nDTSTART:20000103T020000Z\r\nDTEND:20000103T021500Z\r\nRRULE:FREQ=WEEKLY;COUNT=2798;BYDAY=MO,WE\r\nSUMMARY:ended\r\n");
        let running = ev("UID:r\r\nDTSTART:20000103T020000Z\r\nDTEND:20000103T021500Z\r\nRRULE:FREQ=WEEKLY;COUNT=2900;BYDAY=MO,WE\r\nSUMMARY:running\r\n");
        // the first week only counts days from DTSTART (Wednesday) on
        let mid_week = ev("UID:m\r\nDTSTART:20261021T020000Z\r\nDTEND:20261021T021500Z\r\nRRULE:FREQ=WEEKLY;COUNT=2;BYDAY=MO,WE\r\nSUMMARY:two\r\n");
        let daily = ev("UID:d\r\nDTSTART:20000101T030000Z\r\nDTEND:20000101T031500Z\r\nRRULE:FREQ=DAILY;COUNT=9796\r\nSUMMARY:daily\r\n");
        let (v, work) = events_with_budget(&cal(&(ended + &running + &mid_week + &daily)), from, to, WIB, FEED_BUDGET);
        let n = |s: &str| v.iter().filter(|e| e.summary == s).count();
        assert_eq!((n("ended"), n("running")), (0, 2));
        assert_eq!(n("two"), 1, "Wed 21, then Mon 26 in the window; none after");
        // 1 Jan 2000 + 9795 days = 26 Oct 2026: the last one
        assert_eq!(n("daily"), 1);
        assert!(work.used < 100, "skipped ahead instead of walking 26 years: {work:?}");
    }

    /// Rules that never produce a date (30 February) must stop at the window too, not run every period they are
    /// allowed and starve the user's own events.
    #[test]
    fn rules_that_never_happen_cost_almost_nothing() {
        let never = ev("UID:n\r\nDTSTART:20000101T010000Z\r\nDTEND:20000101T011500Z\r\nRRULE:FREQ=YEARLY;BYMONTH=2;BYMONTHDAY=30\r\nSUMMARY:never\r\n");
        let monthly = ev("UID:m\r\nDTSTART:20000126T020000Z\r\nDTEND:20000126T021500Z\r\nRRULE:FREQ=MONTHLY;COUNT=500;BYMONTHDAY=26\r\nSUMMARY:Monthly review\r\n");
        let ics = cal(&(never.repeat(40) + &monthly));
        let (from, to) = week();
        let v = events_between(&ics, from, to, WIB);
        let mine: Vec<_> = v.iter().map(|e| (e.summary.as_str(), e.start)).collect();
        assert_eq!(mine, [("Monthly review", unix_utc(2026, 10, 26, 2, 0, 0))]);
    }

    /// With thousands of recurring events the shares shrink so the total stays within the budget (the floor only
    /// takes over past budget / 16 rules).
    #[test]
    fn total_work_never_exceeds_the_budget() {
        let busy = ev("UID:c\r\nDTSTART:19000101T010000Z\r\nDTEND:19000101T011500Z\r\nRRULE:FREQ=MONTHLY;COUNT=999999;BYMONTHDAY=1\r\nSUMMARY:x\r\n");
        let ics = cal(&busy.repeat(8000));
        let (from, to) = week();
        let budget = 300_000;
        let (_, work) = events_with_budget(&ics, from, to, WIB, budget);
        assert!(work.cut);
        assert!(work.used <= budget, "{work:?}");
    }

    /// Feed order can't be used to starve an event: thousands of costly rules placed first still leave the user's own
    /// weekly meeting (which needs only a few periods) its share.
    #[test]
    fn feed_order_cannot_starve_an_event() {
        let busy = ev("UID:c\r\nDTSTART:19000101T010000Z\r\nDTEND:19000101T011500Z\r\nRRULE:FREQ=MONTHLY;COUNT=999999;BYMONTHDAY=1\r\nSUMMARY:x\r\n");
        let standup = ev("UID:s\r\nDTSTART:20200106T020000Z\r\nDTEND:20200106T021500Z\r\nRRULE:FREQ=WEEKLY;BYDAY=MO\r\nSUMMARY:Standup\r\n");
        let ics = cal(&(busy.repeat(8000) + &standup));
        let (from, to) = week();
        let (v, work) = events_with_budget(&ics, from, to, WIB, 300_000);
        assert!(v.iter().any(|e| e.summary == "Standup" && e.start == unix_utc(2026, 10, 26, 2, 0, 0)), "{work:?}");
    }

    #[test]
    fn bad_input_gives_no_events_and_does_not_panic() {
        let (from, to) = week();
        for s in ["", "garbage", "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART:2026\r\nEND:VEVENT\r\nEND:VCALENDAR", "BEGIN:VEVENT\r\nSUMMARY:x"] {
            assert!(events_between(s, from, to, WIB).is_empty(), "{s:?}");
        }
        let unbounded = ev("UID:z\r\nDTSTART:20000101T010000Z\r\nDTEND:20000101T011500Z\r\nRRULE:FREQ=DAILY\r\nSUMMARY:Every day since 2000\r\n");
        let t = std::time::Instant::now();
        assert_eq!(events_between(&cal(&unbounded), from, to, WIB).len(), 7);
        assert!(t.elapsed().as_millis() < 200, "long-running rules stay cheap");
    }
}

//! Due dates for To-do items, typed at the end of the task: "call mom tomorrow 14:00", "report fri", "pay rent 25/11".
//! Pure functions (no window, no clock): the caller passes the current time, so every rule is tested directly.
//! Times are unix seconds (UTC); `offset` is the PC's offset from UTC in seconds.

const DAY: i64 = 86_400;
/// A date without a time reminds at 09:00 ...
const DEFAULT_MIN: i64 = 9 * 60;
/// ... or at 23:59 when 09:00 of that day has already passed.
const LAST_MIN: i64 = 23 * 60 + 59;

/// 0 = Sunday.
fn weekday(day: i64) -> i64 {
    (day + 4).rem_euclid(7)
}

/// A date the user typed.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Date {
    /// Days from today (0 = today, 1 = tomorrow ...).
    Relative(i64),
    /// The next such weekday (0 = Sunday).
    Weekday(i64),
    /// Day and month, this year (or next year if it has passed).
    DayMonth(u32, u32),
    /// A full date, kept exactly.
    Full(i64, u32, u32),
}

/// Words that name a day, in English and Indonesian (lowercase).
fn word_date(w: &str) -> Option<Date> {
    Some(match w {
        "today" | "tod" => Date::Relative(0),
        "tomorrow" | "tmrw" | "tmr" | "besok" | "bsk" => Date::Relative(1),
        "lusa" => Date::Relative(2),
        "sunday" | "sun" | "minggu" | "ahad" => Date::Weekday(0),
        "monday" | "mon" | "senin" | "sen" => Date::Weekday(1),
        "tuesday" | "tue" | "tues" | "selasa" | "sel" => Date::Weekday(2),
        "wednesday" | "wed" | "rabu" | "rab" => Date::Weekday(3),
        "thursday" | "thu" | "thur" | "thurs" | "kamis" | "kam" => Date::Weekday(4),
        "friday" | "fri" | "jumat" | "jum'at" | "jum" => Date::Weekday(5),
        "saturday" | "sat" | "sabtu" | "sab" => Date::Weekday(6),
        _ => return None,
    })
}

fn valid_day(y: i64, m: u32, d: u32) -> bool {
    (1..=12).contains(&m) && d >= 1 && d <= crate::ics::days_in_month(y, m)
}

/// One or two words naming a date: "tomorrow", "hari ini", "25/11", "25-11-2026", "2026-11-25".
fn parse_date(words: &[&str]) -> Option<Date> {
    let lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
    match lower.as_slice() {
        [a, b] if a == "hari" && b == "ini" => return Some(Date::Relative(0)),
        [one] => {
            if let Some(d) = word_date(one) {
                return Some(d);
            }
            let parts: Vec<&str> = one.split(['/', '-', '.']).collect();
            if parts.iter().any(|p| p.is_empty() || p.len() > 4 || !p.bytes().all(|b| b.is_ascii_digit())) {
                return None;
            }
            let n: Vec<i64> = parts.iter().filter_map(|p| p.parse().ok()).collect();
            match (parts.len(), parts.first().map_or(0, |p| p.len())) {
                // 2026-11-25
                (3, 4) => {
                    let (y, m, d) = (n[0], n[1] as u32, n[2] as u32);
                    return valid_day(y, m, d).then_some(Date::Full(y, m, d));
                }
                // 25/11/2026 or 25/11/26
                (3, 1..=2) if parts[2].len() == 4 || parts[2].len() == 2 => {
                    let y = if parts[2].len() == 2 { 2000 + n[2] } else { n[2] };
                    let (m, d) = (n[1] as u32, n[0] as u32);
                    return valid_day(y, m, d).then_some(Date::Full(y, m, d));
                }
                // 25/11 (a dot would clash with times like 14.30)
                (2, 1..=2) if !one.contains('.') && parts[1].len() <= 2 => {
                    let (m, d) = (n[1] as u32, n[0] as u32);
                    // 29 February is checked against a leap year: it is resolved to a real year later
                    return (valid_day(2024, m, d)).then_some(Date::DayMonth(d, m));
                }
                _ => {}
            }
            None
        }
        _ => None,
    }
}

/// Words that may stand right before a time: "jam 14:00", "at 9am", "pukul 9".
fn is_at_word(w: &str) -> bool {
    matches!(w.to_lowercase().as_str(), "at" | "jam" | "pukul" | "@" | "by" | "pada" | "on" | "sebelum")
}

/// Minutes since midnight from a typed time: "14:00", "9:30", "14.30" (two-digit hour), "9am", "9:30pm".
fn parse_time(w: &str) -> Option<i64> {
    let s = w.to_lowercase();
    let (body, ampm) = if let Some(b) = s.strip_suffix("am") {
        (b, Some(false))
    } else if let Some(b) = s.strip_suffix("pm") {
        (b, Some(true))
    } else {
        (s.as_str(), None)
    };
    let (h, m): (&str, &str) = match body.split_once(':') {
        Some(p) => p,
        None => match body.split_once('.') {
            // "14.30" yes; "3.14" no (a version or a decimal number)
            Some((h, m)) if h.len() == 2 && m.len() == 2 && ampm.is_none() => (h, m),
            Some(_) => return None,
            // "9am" has no minutes; a bare number is not a time
            None if ampm.is_some() => (body, "0"),
            None => return None,
        },
    };
    if h.is_empty() || h.len() > 2 || m.is_empty() || m.len() > 2 || !h.bytes().chain(m.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (mut h, m): (i64, i64) = (h.parse().ok()?, m.parse().ok()?);
    if m > 59 {
        return None;
    }
    match ampm {
        Some(pm) => {
            if !(1..=12).contains(&h) {
                return None;
            }
            h %= 12;
            if pm {
                h += 12;
            }
        }
        None if h > 23 => return None,
        None => {}
    }
    Some(h * 60 + m)
}

/// Split the due phrase off the end of `input`. Returns the task text (what is left, never empty) and the due time as
/// unix seconds. Without a recognised phrase the text is returned unchanged (whitespace tidied) and the due is `None`.
pub(crate) fn split_due(input: &str, now: i64, offset: i32) -> (String, Option<i64>) {
    let words: Vec<&str> = input.split_whitespace().collect();
    let whole = words.join(" ");
    let mut end = words.len();
    let (mut date, mut time): (Option<Date>, Option<i64>) = (None, None);
    // a phrase is a time, a date, or both in either order; at least one word is left as the task
    for _ in 0..2 {
        if end < 2 {
            break;
        }
        if time.is_none() {
            if let Some(t) = parse_time(words[end - 1]) {
                time = Some(t);
                end -= 1;
                if end >= 2 && is_at_word(words[end - 1]) {
                    end -= 1;
                }
                continue;
            }
            // "at 9": a bare hour is a time only after "at" / "jam"
            if end >= 3 && is_at_word(words[end - 2]) {
                if let Ok(h @ 0..=23) = words[end - 1].parse::<i64>() {
                    time = Some(h * 60);
                    end -= 2;
                    continue;
                }
            }
        }
        if date.is_none() {
            if end >= 3 {
                if let Some(d) = parse_date(&words[end - 2..end]) {
                    date = Some(d);
                    end -= 2;
                    if end >= 2 && is_at_word(words[end - 1]) {
                        end -= 1;
                    }
                    continue;
                }
            }
            if let Some(d) = parse_date(&words[end - 1..end]) {
                date = Some(d);
                end -= 1;
                if end >= 2 && is_at_word(words[end - 1]) {
                    end -= 1;
                }
                continue;
            }
        }
        break;
    }
    if date.is_none() && time.is_none() {
        return (whole, None);
    }
    (words[..end].join(" "), Some(resolve(date, time, now, offset)))
}

/// The moment a date and/or time (as typed) means, seen from `now`.
fn resolve(date: Option<Date>, time: Option<i64>, now: i64, offset: i32) -> i64 {
    let local = now + offset as i64;
    let today = local.div_euclid(DAY);
    let minute = local.rem_euclid(DAY) / 60;
    let day = match date {
        None => {
            // only a time: today, or tomorrow when it has passed
            if time.is_some_and(|t| t <= minute) {
                today + 1
            } else {
                today
            }
        }
        Some(Date::Relative(n)) => today + n,
        Some(Date::Weekday(wd)) => {
            let ahead = (wd - weekday(today)).rem_euclid(7);
            // the same weekday means today if the time is still to come, otherwise next week
            if ahead == 0 && !time.is_some_and(|t| t > minute) {
                today + 7
            } else {
                today + ahead
            }
        }
        Some(Date::DayMonth(d, m)) => {
            let (y, _, _) = crate::ics::civil_from_days(today);
            let pick = |y: i64| -> Option<i64> { valid_day(y, m, d).then(|| crate::ics::days_from_civil(y, m, d)) };
            // 29 February: the next leap year that has it
            let this = (y..y + 9).find_map(pick).unwrap_or(today);
            if this < today {
                (y + 1..y + 10).find_map(pick).unwrap_or(this)
            } else {
                this
            }
        }
        Some(Date::Full(y, m, d)) => crate::ics::days_from_civil(y, m, d),
    };
    let min = match time {
        Some(t) => t,
        // a date alone: 09:00, or the end of the day when 09:00 of today has gone
        None if day == today && DEFAULT_MIN <= minute => LAST_MIN,
        None => DEFAULT_MIN,
    };
    day * DAY + min * 60 - offset as i64
}

/// The phrase that gives back the same due when it is typed again ("25/11/2026 14:00"): shown when editing a task.
pub(crate) fn format_due(due: i64, offset: i32) -> String {
    let local = due + offset as i64;
    let (y, m, d) = crate::ics::civil_from_days(local.div_euclid(DAY));
    let min = local.rem_euclid(DAY) / 60;
    format!("{d:02}/{m:02}/{y:04} {:02}:{:02}", min / 60, min % 60)
}

/// Where a due time falls, for the badge on a task.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum When {
    Overdue,
    /// Later today, minutes since midnight.
    Today(i64),
    Tomorrow,
    /// Within the week: weekday, 0 = Sunday.
    Weekday(usize),
    /// Further away: day of month and month (0 = January).
    Date(u32, usize),
}

pub(crate) fn when(due: i64, now: i64, offset: i32) -> When {
    if due <= now {
        return When::Overdue;
    }
    let today = (now + offset as i64).div_euclid(DAY);
    let local = due + offset as i64;
    let day = local.div_euclid(DAY);
    match day - today {
        0 => When::Today(local.rem_euclid(DAY) / 60),
        1 => When::Tomorrow,
        2..=6 => When::Weekday(weekday(day) as usize),
        _ => {
            let (_, m, d) = crate::ics::civil_from_days(day);
            When::Date(d, m as usize - 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIB: i32 = 7 * 3600;

    /// Unix seconds for a local (WIB) date and time.
    fn at(y: i64, m: u32, d: u32, h: i64, mi: i64) -> i64 {
        crate::ics::days_from_civil(y, m, d) * DAY + (h * 60 + mi) * 60 - WIB as i64
    }

    /// Wednesday 25 November 2026, 10:30 WIB.
    fn now() -> i64 {
        at(2026, 11, 25, 10, 30)
    }

    fn due(s: &str) -> (String, Option<i64>) {
        split_due(s, now(), WIB)
    }

    #[test]
    fn a_task_without_a_phrase_is_left_alone() {
        assert_eq!(due("buy milk"), ("buy milk".to_string(), None));
        assert_eq!(due("  buy   milk  "), ("buy milk".to_string(), None), "whitespace is tidied");
        assert_eq!(due("version 3.14"), ("version 3.14".to_string(), None), "a decimal number is not a time");
        assert_eq!(due("wait 5 min"), ("wait 5 min".to_string(), None), "min is not a weekday");
        assert_eq!(due("call at 99:99"), ("call at 99:99".to_string(), None));
    }

    #[test]
    fn the_phrase_alone_stays_as_the_task() {
        assert_eq!(due("tomorrow"), ("tomorrow".to_string(), None), "nothing would be left of the task");
        assert_eq!(due("14:00"), ("14:00".to_string(), None));
    }

    #[test]
    fn relative_days_and_times() {
        assert_eq!(due("call mom tomorrow 14:00"), ("call mom".to_string(), Some(at(2026, 11, 26, 14, 0))));
        assert_eq!(due("call mom besok 14:00"), ("call mom".to_string(), Some(at(2026, 11, 26, 14, 0))));
        assert_eq!(due("call mom 14:00 besok"), ("call mom".to_string(), Some(at(2026, 11, 26, 14, 0))), "either order");
        assert_eq!(due("rapat lusa jam 9"), ("rapat".to_string(), Some(at(2026, 11, 27, 9, 0))));
        assert_eq!(due("rapat hari ini 16:45"), ("rapat".to_string(), Some(at(2026, 11, 25, 16, 45))));
        assert_eq!(due("pay rent today at 5pm"), ("pay rent".to_string(), Some(at(2026, 11, 25, 17, 0))));
        assert_eq!(due("pay rent today at 12am"), ("pay rent".to_string(), Some(at(2026, 11, 25, 0, 0))));
        assert_eq!(due("lunch 12pm"), ("lunch".to_string(), Some(at(2026, 11, 25, 12, 0))), "12pm is noon, still ahead");
        assert_eq!(due("breakfast 8am"), ("breakfast".to_string(), Some(at(2026, 11, 26, 8, 0))), "8am has passed: tomorrow");
        assert_eq!(due("pulang 17.30"), ("pulang".to_string(), Some(at(2026, 11, 25, 17, 30))));
    }

    #[test]
    fn a_time_alone_means_today_or_tomorrow() {
        assert_eq!(due("stretch 11:00"), ("stretch".to_string(), Some(at(2026, 11, 25, 11, 0))), "later today");
        assert_eq!(due("stretch 10:30"), ("stretch".to_string(), Some(at(2026, 11, 26, 10, 30))), "this minute has gone");
        assert_eq!(due("stretch 09:00"), ("stretch".to_string(), Some(at(2026, 11, 26, 9, 0))));
    }

    #[test]
    fn a_date_alone_reminds_at_nine_or_at_the_end_of_today() {
        assert_eq!(due("report besok"), ("report".to_string(), Some(at(2026, 11, 26, 9, 0))));
        assert_eq!(due("report today"), ("report".to_string(), Some(at(2026, 11, 25, 23, 59))), "09:00 has passed");
        let early = at(2026, 11, 25, 7, 0);
        assert_eq!(split_due("report today", early, WIB).1, Some(at(2026, 11, 25, 9, 0)));
    }

    #[test]
    fn weekdays_in_both_languages() {
        // today is a Wednesday
        assert_eq!(due("report fri"), ("report".to_string(), Some(at(2026, 11, 27, 9, 0))));
        assert_eq!(due("laporan jumat 13:00"), ("laporan".to_string(), Some(at(2026, 11, 27, 13, 0))));
        assert_eq!(due("review monday"), ("review".to_string(), Some(at(2026, 11, 30, 9, 0))));
        assert_eq!(due("review senin"), ("review".to_string(), Some(at(2026, 11, 30, 9, 0))));
        assert_eq!(due("review minggu"), ("review".to_string(), Some(at(2026, 11, 29, 9, 0))));
        // the same weekday: today if the time is still ahead, otherwise next week
        assert_eq!(due("sync wed 15:00"), ("sync".to_string(), Some(at(2026, 11, 25, 15, 0))));
        assert_eq!(due("sync wed 09:00"), ("sync".to_string(), Some(at(2026, 12, 2, 9, 0))));
        assert_eq!(due("sync rabu"), ("sync".to_string(), Some(at(2026, 12, 2, 9, 0))));
    }

    #[test]
    fn day_and_month_roll_to_next_year_when_they_have_passed() {
        assert_eq!(due("pay rent 28/11"), ("pay rent".to_string(), Some(at(2026, 11, 28, 9, 0))));
        assert_eq!(due("pay rent 25/11 18:00"), ("pay rent".to_string(), Some(at(2026, 11, 25, 18, 0))));
        assert_eq!(due("holiday 5/1"), ("holiday".to_string(), Some(at(2027, 1, 5, 9, 0))), "next January");
        assert_eq!(due("visa 24-12-2026 10:00"), ("visa".to_string(), Some(at(2026, 12, 24, 10, 0))));
        assert_eq!(due("visa 2026-12-24"), ("visa".to_string(), Some(at(2026, 12, 24, 9, 0))));
        assert_eq!(due("old 20/11/2026 08:00"), ("old".to_string(), Some(at(2026, 11, 20, 8, 0))), "a full date is never moved");
        assert_eq!(due("birthday 29/2"), ("birthday".to_string(), Some(at(2028, 2, 29, 9, 0))), "the next leap year");
        assert_eq!(due("nope 31/2"), ("nope 31/2".to_string(), None));
        assert_eq!(due("nope 13/13"), ("nope 13/13".to_string(), None));
    }

    #[test]
    fn the_word_before_a_time_goes_with_it() {
        assert_eq!(due("meet at 14:00"), ("meet".to_string(), Some(at(2026, 11, 25, 14, 0))));
        assert_eq!(due("meet besok pukul 08:15"), ("meet".to_string(), Some(at(2026, 11, 26, 8, 15))));
        assert_eq!(due("meet at 9"), ("meet".to_string(), Some(at(2026, 11, 26, 9, 0))), "a bare hour after \"at\"");
        assert_eq!(due("buy 5"), ("buy 5".to_string(), None), "a bare number is not a time");
    }

    #[test]
    fn a_title_keeps_words_that_only_look_like_a_phrase() {
        assert_eq!(due("sun cream"), ("sun cream".to_string(), None));
        assert_eq!(due("meeting notes monday plan"), ("meeting notes monday plan".to_string(), None));
        assert_eq!(due("a b"), ("a b".to_string(), None));
        assert_eq!(due("note tmrw"), ("note".to_string(), Some(at(2026, 11, 26, 9, 0))));
    }

    #[test]
    fn what_editing_shows_is_read_back_the_same() {
        for d in [at(2026, 11, 20, 8, 5), at(2026, 11, 25, 23, 59), at(2027, 1, 5, 9, 0), at(2026, 12, 31, 0, 0)] {
            let phrase = format_due(d, WIB);
            let (title, back) = split_due(&format!("task {phrase}"), now(), WIB);
            assert_eq!((title.as_str(), back), ("task", Some(d)), "{phrase}");
        }
        assert_eq!(format_due(at(2026, 11, 5, 9, 7), WIB), "05/11/2026 09:07");
    }

    #[test]
    fn the_badge_says_how_far_away_it_is() {
        let n = now();
        assert_eq!(when(n - 60, n, WIB), When::Overdue);
        assert_eq!(when(n, n, WIB), When::Overdue, "due now counts as late");
        assert_eq!(when(at(2026, 11, 25, 14, 0), n, WIB), When::Today(14 * 60));
        assert_eq!(when(at(2026, 11, 26, 9, 0), n, WIB), When::Tomorrow);
        assert_eq!(when(at(2026, 11, 27, 9, 0), n, WIB), When::Weekday(5));
        assert_eq!(when(at(2026, 12, 1, 9, 0), n, WIB), When::Weekday(2), "six days ahead is still a weekday");
        assert_eq!(when(at(2026, 12, 2, 9, 0), n, WIB), When::Date(2, 11), "a week ahead is a date");
        assert_eq!(when(at(2027, 1, 5, 9, 0), n, WIB), When::Date(5, 0));
    }

    #[test]
    fn the_day_follows_the_pcs_offset_not_utc() {
        // 20:00 UTC on the 25th is 03:00 on the 26th in WIB
        let base = crate::ics::days_from_civil(2026, 11, 25) * DAY;
        let (moment, now) = (base + 20 * 3600, base + 10 * 3600);
        assert_eq!(when(moment, now, WIB), When::Tomorrow);
        assert_eq!(when(moment, now, 0), When::Today(20 * 60), "the same moment, seen from UTC, is still today");
    }
}

//! Timestamps as React's `lib/utils.ts` formats them: relative ("3 hours
//! ago"), by day ("Today", "Oct 17, 2026") and in full (tooltips), in the
//! local time zone.

use chrono::{DateTime, Datelike, FixedOffset, Local, NaiveDate, Offset, TimeZone};

/// `parseTimestamp`: RFC 3339, or jj's `2026-10-19 12:00:00.000 +02:00`.
pub fn parse(timestamp: &str) -> Option<DateTime<FixedOffset>> {
  DateTime::parse_from_rfc3339(timestamp)
    .or_else(|_| DateTime::parse_from_str(timestamp, "%Y-%m-%d %H:%M:%S%.f %:z"))
    .ok()
}

/// `formatRelativeTime`, as of `now`; unparseable timestamps show as is.
pub fn relative_at(timestamp: &str, now: DateTime<FixedOffset>) -> String {
  let Some(date) = parse(timestamp) else {
    return timestamp.to_string();
  };
  let ms = (now - date).num_milliseconds();
  if ms < 0 {
    let seconds = -ms / 1000;
    let (minutes, hours) = (seconds / 60, seconds / 3600);
    let days = hours / 24;
    return match () {
      _ if seconds < 60 => "in a few seconds".into(),
      _ if minutes < 60 => format!("in {minutes} minutes"),
      _ if hours < 24 => format!("in {hours} hours"),
      _ if days == 1 => "in 1 day".into(),
      _ => format!("in {days} days"),
    };
  }
  let seconds = ms / 1000;
  let minutes = seconds / 60;
  let hours = minutes / 60;
  let days = hours / 24;
  let (weeks, months, years) = (days / 7, days / 30, days / 365);
  let unit = |n: i64, one: &str| {
    if n == 1 {
      format!("1 {one} ago")
    } else {
      format!("{n} {one}s ago")
    }
  };
  match () {
    _ if seconds < 10 => "just now".into(),
    _ if seconds < 60 => format!("{seconds} seconds ago"),
    _ if minutes < 60 => unit(minutes, "minute"),
    _ if hours < 24 => unit(hours, "hour"),
    _ if days < 7 => unit(days, "day"),
    _ if weeks < 4 => unit(weeks, "week"),
    _ if months < 12 => unit(months, "month"),
    _ => unit(years, "year"),
  }
}

pub fn relative(timestamp: &str) -> String {
  relative_at(timestamp, Local::now().fixed_offset())
}

/// The local calendar day of `timestamp` (`getDayKey`).
pub fn day(timestamp: &str) -> Option<NaiveDate> {
  parse(timestamp).map(|date| date.with_timezone(&Local).date_naive())
}

/// `formatDayLabel`, with `today` the local date.
pub fn day_label_at(timestamp: &str, today: NaiveDate) -> String {
  let Some(date) = day(timestamp) else {
    return timestamp.to_string();
  };
  match (today - date).num_days() {
    0 => "Today".into(),
    1 => "Yesterday".into(),
    _ => format!(
      "{} {}, {}",
      short_month(date.month()),
      date.day(),
      date.year()
    ),
  }
}

pub fn day_label(timestamp: &str) -> String {
  day_label_at(timestamp, Local::now().date_naive())
}

fn short_month(month: u32) -> &'static str {
  [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
  ][(month as usize).saturating_sub(1) % 12]
}

/// `formatFullTimestamp` as en-US `toLocaleString` writes it:
/// `October 19, 2026 at 01:05:09 PM GMT+2`.
pub fn full_in<Tz: TimeZone>(timestamp: &str, zone: &Tz) -> String
where
  Tz::Offset: std::fmt::Display,
{
  let Some(date) = parse(timestamp) else {
    return timestamp.to_string();
  };
  let local = date.with_timezone(zone);
  let offset = local.offset().fix().local_minus_utc();
  let zone = match (offset / 3600, (offset % 3600).abs() / 60) {
    (0, 0) => "UTC".to_string(),
    (h, 0) => format!("GMT{h:+}"),
    (h, m) => format!("GMT{h:+}:{m:02}"),
  };
  format!("{} {zone}", local.format("%B %-d, %Y at %I:%M:%S %p"))
}

pub fn full(timestamp: &str) -> String {
  full_in(timestamp, &Local)
}

/// `new Date(timestamp).toLocaleString()` in en-US: `10/19/2026, 1:05:09 PM`.
pub fn locale_in<Tz: TimeZone>(timestamp: &str, zone: &Tz) -> String
where
  Tz::Offset: std::fmt::Display,
{
  match parse(timestamp) {
    Some(date) => date
      .with_timezone(zone)
      .format("%-m/%-d/%Y, %-I:%M:%S %p")
      .to_string(),
    None => timestamp.to_string(),
  }
}

pub fn locale(timestamp: &str) -> String {
  locale_in(timestamp, &Local)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn at(ts: &str) -> DateTime<FixedOffset> {
    parse(ts).expect("timestamp")
  }

  #[test]
  fn parses_rfc3339_and_jj_timestamps() {
    assert_eq!(
      parse("2026-10-19 12:00:00.000 +02:00"),
      parse("2026-10-19T10:00:00Z")
    );
    assert_eq!(parse("not a date"), None);
  }

  #[test]
  fn relative_time_matches_react() {
    let now = at("2026-10-19T12:00:00Z");
    let cases = [
      ("2026-10-19T11:59:55Z", "just now"),
      ("2026-10-19T11:59:30Z", "30 seconds ago"),
      ("2026-10-19T11:59:00Z", "1 minute ago"),
      ("2026-10-19T09:00:00Z", "3 hours ago"),
      ("2026-10-18T12:00:00Z", "1 day ago"),
      ("2026-10-05T12:00:00Z", "2 weeks ago"),
      ("2026-08-01T12:00:00Z", "2 months ago"),
      ("2024-10-01T12:00:00Z", "2 years ago"),
      ("2026-10-19T12:00:30Z", "in a few seconds"),
      ("2026-10-21T13:00:00Z", "in 2 days"),
      ("bogus", "bogus"),
    ];
    for (ts, want) in cases {
      assert_eq!(relative_at(ts, now), want, "{ts}");
    }
  }

  #[test]
  fn day_labels_name_today_and_yesterday() {
    let today = day("2026-10-19T12:00:00Z").expect("day");
    assert_eq!(day_label_at("2026-10-19T12:00:00Z", today), "Today");
    assert_eq!(day_label_at("2026-10-18T12:00:00Z", today), "Yesterday");
    let label = day_label_at("2026-01-03T12:00:00Z", today);
    assert!(
      label.starts_with("Jan ") && label.ends_with(", 2026"),
      "{label}"
    );
  }

  #[test]
  fn full_timestamps_read_like_en_us() {
    let utc = chrono::Utc;
    assert_eq!(
      full_in("2026-10-19T13:05:09Z", &utc),
      "October 19, 2026 at 01:05:09 PM UTC"
    );
    let plus2 = FixedOffset::east_opt(2 * 3600).expect("offset");
    assert_eq!(
      full_in("2026-10-19T13:05:09Z", &plus2),
      "October 19, 2026 at 03:05:09 PM GMT+2"
    );
  }

  #[test]
  fn locale_timestamps_read_like_to_locale_string() {
    let utc = chrono::Utc;
    assert_eq!(
      locale_in("2026-10-19T13:05:09Z", &utc),
      "10/19/2026, 1:05:09 PM"
    );
    assert_eq!(
      locale_in("2026-01-03T00:00:00Z", &utc),
      "1/3/2026, 12:00:00 AM"
    );
    assert_eq!(locale_in("bogus", &utc), "bogus");
  }
}

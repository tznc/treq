//! Plan author-timestamp rewrites so a mutable commit can move in time
//! without creating a parent-after-child timestamp history.

use chrono::{NaiveDate, TimeZone};

/// Minimum gap applied when two original timestamps are equal or inverted.
pub const MIN_TIMESTAMP_GAP_MS: i64 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimestampShift {
  Offset { days: i64, hours: i64, minutes: i64 },
  ToDay { year: i32, month: u32, day: u32 },
}

/// Compute new timestamps for `[target, descendant...]` (oldest → newest).
///
/// `new_target_millis` is the requested author time for the target. Each later
/// commit keeps its original time when it is still strictly after its
/// predecessor; otherwise it is shifted by the original interval from that
/// predecessor (at least [`MIN_TIMESTAMP_GAP_MS`]). The caller must reject a
/// target that would land at or before `parent_millis`.
pub fn plan_lineage_timestamps(
  original_millis: &[i64],
  new_target_millis: i64,
  parent_millis: Option<i64>,
) -> Vec<i64> {
  let _ = parent_millis;

  let mut planned = Vec::with_capacity(original_millis.len());
  planned.push(new_target_millis);

  for index in 1..original_millis.len() {
    let predecessor_new = planned[index - 1];
    let original = original_millis[index];
    if original > predecessor_new {
      planned.push(original);
      continue;
    }

    let original_gap = original_millis[index].saturating_sub(original_millis[index - 1]);
    let gap = original_gap.max(MIN_TIMESTAMP_GAP_MS);
    planned.push(predecessor_new.saturating_add(gap));
  }

  planned
}

pub fn offset_to_millis(days: i64, hours: i64, minutes: i64) -> Option<i64> {
  days
    .checked_mul(86_400_000)?
    .checked_add(hours.checked_mul(3_600_000)?)?
    .checked_add(minutes.checked_mul(60_000)?)
}

pub fn apply_shift_to_millis(
  original_millis: i64,
  tz_offset_minutes: i32,
  shift: &TimestampShift,
) -> Result<i64, String> {
  match shift {
    TimestampShift::Offset {
      days,
      hours,
      minutes,
    } => {
      let offset = offset_to_millis(*days, *hours, *minutes)
        .ok_or_else(|| "Timestamp offset is too large".to_string())?;
      original_millis
        .checked_add(offset)
        .ok_or_else(|| "Timestamp offset overflowed".to_string())
    }
    TimestampShift::ToDay { year, month, day } => {
      let offset = chrono::FixedOffset::east_opt(tz_offset_minutes.saturating_mul(60))
        .ok_or_else(|| "Invalid timezone offset".to_string())?;
      let utc = chrono::Utc
        .timestamp_millis_opt(original_millis)
        .single()
        .ok_or_else(|| "Out-of-range timestamp".to_string())?;
      let local = utc.with_timezone(&offset);
      let date = NaiveDate::from_ymd_opt(*year, *month, *day)
        .ok_or_else(|| format!("Invalid date {year:04}-{month:02}-{day:02}"))?;
      let naive = date.and_time(local.time());
      offset
        .from_local_datetime(&naive)
        .single()
        .map(|dt| dt.timestamp_millis())
        .ok_or_else(|| "Could not place that date in the commit timezone".to_string())
    }
  }
}

/// Add the same delta to every timestamp on a lineage (oldest → newest).
pub fn plan_uniform_lineage_shift(original_millis: &[i64], delta_millis: i64) -> Vec<i64> {
  original_millis
    .iter()
    .map(|millis| millis.saturating_add(delta_millis))
    .collect()
}

/// Newest non-empty commit in oldest → newest order. Empty working copies are
/// skipped so "shift to now" lands on the last real commit.
pub fn newest_real_commit_index(empty_flags: &[bool]) -> Option<usize> {
  empty_flags
    .iter()
    .rposition(|is_empty| !*is_empty)
    .or_else(|| empty_flags.last().map(|_| empty_flags.len() - 1))
}

pub fn parse_day(value: &str) -> Result<(i32, u32, u32), String> {
  let trimmed = value.trim();
  let parts: Vec<&str> = trimmed.split('-').collect();
  if parts.len() != 3 {
    return Err("Date must be YYYY-MM-DD".to_string());
  }
  let year: i32 = parts[0]
    .parse()
    .map_err(|_| "Date must be YYYY-MM-DD".to_string())?;
  let month: u32 = parts[1]
    .parse()
    .map_err(|_| "Date must be YYYY-MM-DD".to_string())?;
  let day: u32 = parts[2]
    .parse()
    .map_err(|_| "Date must be YYYY-MM-DD".to_string())?;
  NaiveDate::from_ymd_opt(year, month, day).ok_or_else(|| format!("Invalid date {trimmed}"))?;
  Ok((year, month, day))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn shifts_descendants_that_would_be_older_preserving_gaps() {
    let planned = plan_lineage_timestamps(&[1_000, 2_000, 4_000], 10_000, None);
    assert_eq!(planned, vec![10_000, 11_000, 13_000]);
  }

  #[test]
  fn leaves_descendants_that_stay_newer_unchanged() {
    let planned = plan_lineage_timestamps(&[1_000, 5_000, 8_000], 2_000, None);
    assert_eq!(planned, vec![2_000, 5_000, 8_000]);
  }

  #[test]
  fn clamps_are_left_to_the_caller() {
    let planned = plan_lineage_timestamps(&[1_000, 2_000], 100, Some(500));
    assert_eq!(planned, vec![100, 2_000]);
  }

  #[test]
  fn equal_original_timestamps_gain_a_minimum_gap() {
    let planned = plan_lineage_timestamps(&[1_000, 1_000], 5_000, None);
    assert_eq!(planned, vec![5_000, 6_000]);
  }

  #[test]
  fn offset_to_millis_combines_units() {
    assert_eq!(
      offset_to_millis(1, 2, 3),
      Some(86_400_000 + 7_200_000 + 180_000)
    );
    assert_eq!(offset_to_millis(-1, 0, 0), Some(-86_400_000));
  }

  #[test]
  fn uniform_shift_preserves_gaps() {
    assert_eq!(
      plan_uniform_lineage_shift(&[1_000, 4_000, 7_000], 10_000),
      vec![11_000, 14_000, 17_000]
    );
  }

  #[test]
  fn newest_real_commit_skips_trailing_empty() {
    assert_eq!(newest_real_commit_index(&[false, false, true]), Some(1));
    assert_eq!(newest_real_commit_index(&[true]), Some(0));
    assert_eq!(newest_real_commit_index(&[]), None);
  }

  #[test]
  fn apply_shift_moves_to_requested_day_keeping_clock_time() {
    // 2024-01-15 15:30:00 +00:00
    let original = 1_705_332_600_000;
    let shifted = apply_shift_to_millis(
      original,
      0,
      &TimestampShift::ToDay {
        year: 2024,
        month: 1,
        day: 20,
      },
    )
    .expect("shift");
    assert_eq!(shifted, 1_705_764_600_000);
  }
}

#[cfg(test)]
mod proptests {
  use super::*;
  use crate::test_repo::proptest_config;
  use chrono::{Datelike, Timelike};
  use proptest::prelude::*;

  /// Millis between 1970 and ~2100, far from i64 saturation.
  const MAX_MS: i64 = 4_102_444_800_000;

  proptest! {
    #![proptest_config(proptest_config(256))]

    #[test]
    fn lineage_plan_is_strictly_increasing_and_starts_at_target(
      original in prop::collection::vec(0..MAX_MS, 1..8),
      target in 0..MAX_MS,
    ) {
      let planned = plan_lineage_timestamps(&original, target, None);
      prop_assert_eq!(planned.len(), original.len());
      prop_assert_eq!(planned[0], target);
      for i in 1..planned.len() {
        prop_assert!(planned[i] > planned[i - 1], "{:?}", planned);
        if original[i] > planned[i - 1] {
          prop_assert_eq!(planned[i], original[i]);
        }
      }
    }

    #[test]
    fn uniform_shift_preserves_every_gap(
      original in prop::collection::vec(0..MAX_MS, 0..8),
      delta in -MAX_MS..MAX_MS,
    ) {
      let planned = plan_uniform_lineage_shift(&original, delta);
      prop_assert_eq!(planned.len(), original.len());
      for (p, o) in planned.iter().zip(&original) {
        prop_assert_eq!(p - o, delta);
      }
    }

    #[test]
    fn offset_to_millis_matches_wide_arithmetic(days: i64, hours: i64, minutes: i64) {
      let wide = i128::from(days) * 86_400_000 + i128::from(hours) * 3_600_000 + i128::from(minutes) * 60_000;
      match offset_to_millis(days, hours, minutes) {
        Some(v) => prop_assert_eq!(i128::from(v), wide),
        None => prop_assert!(
          i64::try_from(wide).is_err()
            || days.checked_mul(86_400_000).is_none()
            || hours.checked_mul(3_600_000).is_none()
            || minutes.checked_mul(60_000).is_none()
            || days.checked_mul(86_400_000).and_then(|d| d.checked_add(hours * 3_600_000)).is_none()
        ),
      }
    }

    #[test]
    fn to_day_lands_on_the_day_and_keeps_clock_time(
      original in 0..MAX_MS,
      tz in -14 * 60..=14 * 60i32,
      date in (1971i32..2099, 1u32..=12, 1u32..=28),
    ) {
      let (year, month, day) = date;
      let shifted = apply_shift_to_millis(original, tz, &TimestampShift::ToDay { year, month, day });
      let offset = chrono::FixedOffset::east_opt(tz * 60).expect("in range");
      let before = chrono::Utc.timestamp_millis_opt(original).single().expect("in range").with_timezone(&offset);
      let shifted = shifted.map_err(TestCaseError::fail)?;
      let after = chrono::Utc.timestamp_millis_opt(shifted).single().expect("in range").with_timezone(&offset);
      prop_assert_eq!((after.year(), after.month(), after.day()), (year, month, day));
      prop_assert_eq!(after.time().num_seconds_from_midnight(), before.time().num_seconds_from_midnight());
      prop_assert_eq!(after.timestamp_subsec_millis(), before.timestamp_subsec_millis());
    }

    #[test]
    fn newest_real_commit_is_last_non_empty_or_last(flags in prop::collection::vec(any::<bool>(), 0..8)) {
      match newest_real_commit_index(&flags) {
        None => prop_assert!(flags.is_empty()),
        Some(i) => {
          prop_assert!(i < flags.len());
          if flags.iter().any(|e| !*e) {
            prop_assert!(!flags[i]);
            prop_assert!(flags[i + 1..].iter().all(|e| *e));
          } else {
            prop_assert_eq!(i, flags.len() - 1);
          }
        }
      }
    }

    #[test]
    fn parse_day_round_trips_valid_dates(date in (1i32..=9999, 1u32..=12, 1u32..=31)) {
      let (y, m, d) = date;
      let text = format!(" {y:04}-{m:02}-{d:02}\n");
      let valid = NaiveDate::from_ymd_opt(y, m, d).is_some();
      prop_assert_eq!(parse_day(&text).ok(), valid.then_some((y, m, d)));
    }

    #[test]
    fn parse_day_only_accepts_real_dates(text in "\\PC{0,16}|[-+0-9 ]{0,14}") {
      if let Ok((y, m, d)) = parse_day(&text) {
        prop_assert!(NaiveDate::from_ymd_opt(y, m, d).is_some());
      }
    }
  }
}

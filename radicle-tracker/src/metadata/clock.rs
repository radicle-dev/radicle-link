//! _So much for time flowing past, he thought glumly. It might do that
//! everywhere else, but not here. Here it just piles up, like snow._ -
//! **Pyramids, Terry Pratchett**.
//!
//! This module captures the minimal amount of functionality to keep track of
//! time in code collaboration. All we can do is ask for a current point in time
//! and get the elapsed time compared to another point in time.
//!
//! # Examples
//! ```
//! # use std::error::Error;
//! #
//! # fn main() -> Result<(), Box<dyn Error>> {
//! use radicle_tracker::clock::{Clock, Elapsed, RadicleClock};
//! use std::thread::sleep;
//! use std::time::Duration;
//!
//! let now = RadicleClock::current_time();
//! sleep(Duration::new(1, 0));
//! let then = RadicleClock::current_time();
//!
//! let elapsed = now.elapsed(&then)?;
//!
//! assert_eq!(elapsed, Elapsed::Minutes(0));
//! #
//! #     Ok(())
//! # }
//! ```

// Rough calculations for the number of seconds in some larger unit
const SECONDS_IN_MINUTE: u64 = 60;
const SECONDS_IN_HOUR: u64 = SECONDS_IN_MINUTE * 60;
const SECONDS_IN_DAY: u64 = SECONDS_IN_HOUR * 24;
const SECONDS_IN_WEEK: u64 = SECONDS_IN_DAY * 7;
const SECONDS_IN_MONTH: u64 = SECONDS_IN_WEEK * 4;
const SECONDS_IN_YEAR: u64 = SECONDS_IN_MONTH * 12;

use std::time::{Duration, SystemTime, SystemTimeError};

/// The elapsed time from some previous moment in the past. This is to capture
/// concepts like, "this comment was posted 4 minutes ago", or "5 days ago", or
/// "1 year ago".
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Elapsed {
    /// Elapsed "x minutes ago".
    Minutes(u64),
    /// Elapsed "x hours ago".
    Hours(u64),
    /// Elapsed "x days ago".
    Days(u64),
    /// Elapsed "x weeks ago".
    Weeks(u64),
    /// Elapsed "x months ago".
    Months(u64),
    /// Elapsed "x years ago".
    Years(u64),
}

impl std::fmt::Display for Elapsed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Elapsed::Minutes(m) => write!(f, "{} minutes ago", m),
            Elapsed::Hours(m) => write!(f, "{} hours ago", m),
            Elapsed::Days(m) => write!(f, "{} days ago", m),
            Elapsed::Weeks(m) => write!(f, "{} weeks ago", m),
            Elapsed::Months(m) => write!(f, "{} months ago", m),
            Elapsed::Years(m) => write!(f, "{} years ago", m),
        }
    }
}

/// `Clock` captures the minimal functionality for grabbing a point in time and
/// calculating the duration since another point in time.
pub trait Clock {
    /// Time calculations tend to return errors.
    type Error;

    /// Get the current timestamp.
    fn current_time() -> Self;

    /// Get the duration for a timestamp in the past, `self`, up to the point of
    /// `other`. It should return [`std::time::Duration`], which can be used
    /// for further calculation.
    fn duration_since(&self, other: &Self) -> Result<Duration, Self::Error>;
}

/// A minimal clock for getting a moment in time and calculating the duration up
/// to another moment in time.
///
/// It does this through its instance of [`Clock`] trait.
///
/// For usability, it also provides a function [`RadicleClock::elapsed`] which
/// returns an [`Elapsed`] value for presentation purposes.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct RadicleClock(SystemTime);

impl Clock for RadicleClock {
    type Error = SystemTimeError;

    fn current_time() -> Self {
        RadicleClock(SystemTime::now())
    }

    fn duration_since(&self, other: &Self) -> Result<Duration, Self::Error> {
        let timestamp = self.0.duration_since(SystemTime::UNIX_EPOCH)?;
        let other_timestamp = other.0.duration_since(SystemTime::UNIX_EPOCH)?;

        Ok(other_timestamp - timestamp)
    }
}

impl RadicleClock {
    #[cfg(test)]
    fn add(&self, duration: &Duration) -> RadicleClock {
        RadicleClock(self.0 + *duration)
    }

    /// Calculate the [`Elapsed`] time for two `RadicleClock`s.
    pub fn elapsed(&self, other: &Self) -> Result<Elapsed, <RadicleClock as Clock>::Error> {
        elapsed(self, other)
    }
}

fn elapsed<C>(clock: &C, other: &C) -> Result<Elapsed, C::Error>
where
    C: Clock,
{
    let seconds = clock.duration_since(other)?.as_secs();

    if seconds < SECONDS_IN_MINUTE {
        Ok(Elapsed::Minutes(0))
    } else if seconds >= SECONDS_IN_MINUTE && seconds < SECONDS_IN_HOUR {
        let minutes = seconds / SECONDS_IN_MINUTE;
        Ok(Elapsed::Minutes(minutes))
    } else if seconds >= SECONDS_IN_HOUR && seconds < SECONDS_IN_DAY {
        let hours = seconds / SECONDS_IN_HOUR;
        Ok(Elapsed::Hours(hours))
    } else if seconds >= SECONDS_IN_DAY && seconds < SECONDS_IN_WEEK {
        let days = seconds / SECONDS_IN_DAY;
        Ok(Elapsed::Days(days))
    } else if seconds >= SECONDS_IN_WEEK && seconds < SECONDS_IN_MONTH {
        let weeks = seconds / SECONDS_IN_WEEK;
        Ok(Elapsed::Weeks(weeks))
    } else if seconds >= SECONDS_IN_MONTH && seconds < SECONDS_IN_YEAR {
        let months = seconds / SECONDS_IN_MONTH;
        Ok(Elapsed::Months(months))
    } else {
        let years = seconds / SECONDS_IN_YEAR;
        Ok(Elapsed::Years(years))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::time::Duration;

    #[test]
    fn test_elapsed_seconds() {
        let now = RadicleClock::current_time();
        let one_sec = Duration::new(1, 0);
        let then = RadicleClock::current_time().add(&one_sec);
        let elapsed = now.elapsed(&then).expect("Failed to get duration");

        assert_eq!(elapsed, Elapsed::Minutes(0));
    }

    #[test]
    fn test_elapsed_minutes() {
        let now = RadicleClock::current_time();
        let one_minute = Duration::new(60, 0);
        let then = RadicleClock::current_time().add(&one_minute);
        let elapsed = now.elapsed(&then).expect("Failed to get duration");

        assert_eq!(elapsed, Elapsed::Minutes(1));

        let five_minutes = Duration::new(350, 0);
        let then = RadicleClock::current_time().add(&five_minutes);
        let elapsed = now.elapsed(&then).expect("Failed to get duration");

        assert_eq!(elapsed, Elapsed::Minutes(5));
    }

    #[test]
    fn test_elapsed_hours() {
        let now = RadicleClock::current_time();
        let one_hour = Duration::new(3600, 0);
        let then = RadicleClock::current_time().add(&one_hour);
        let elapsed = now.elapsed(&then).expect("Failed to get duration");

        assert_eq!(elapsed, Elapsed::Hours(1));

        let five_hours = Duration::new(3650 * 5, 0);
        let then = RadicleClock::current_time().add(&five_hours);
        let elapsed = now.elapsed(&then).expect("Failed to get duration");

        assert_eq!(elapsed, Elapsed::Hours(5));
    }

    #[test]
    fn test_elapsed_days() {
        let now = RadicleClock::current_time();
        let one_day = Duration::new(3600 * 24, 0);
        let then = RadicleClock::current_time().add(&one_day);
        let elapsed = now.elapsed(&then).expect("Failed to get duration");

        assert_eq!(elapsed, Elapsed::Days(1));

        let five_days = Duration::new(3600 * 24 * 5 + 500, 0);
        let then = RadicleClock::current_time().add(&five_days);
        let elapsed = now.elapsed(&then).expect("Failed to get duration");

        assert_eq!(elapsed, Elapsed::Days(5));
    }

    fn caclulate_elapsed(value: u64, bound: u64) -> (Elapsed, u64) {
        let now = RadicleClock::current_time();
        let then = RadicleClock::current_time().add(&Duration::new(value, 0));
        (
            now.elapsed(&then).expect("Failed to get duration"),
            value / bound,
        )
    }

    proptest! {
        #[test]
        fn elapsed_minutes_bucket(minutes in 0u64..SECONDS_IN_HOUR) {
            let (result, n) = caclulate_elapsed(minutes, SECONDS_IN_MINUTE);
            prop_assert_eq!(result, Elapsed::Minutes(n))
        }

        #[test]
        fn elapsed_hours_bucket(hours in SECONDS_IN_HOUR + 1..SECONDS_IN_DAY) {
            let (result, n) = caclulate_elapsed(hours, SECONDS_IN_HOUR);
            prop_assert_eq!(result, Elapsed::Hours(n));
        }

        #[test]
        fn elapsed_days_bucket(days in SECONDS_IN_DAY + 1..SECONDS_IN_WEEK) {
            let (result, n) = caclulate_elapsed(days, SECONDS_IN_DAY);
            prop_assert_eq!(result, Elapsed::Days(n));
        }

        #[test]
        fn elapsed_week_bucket(days in SECONDS_IN_WEEK + 1..SECONDS_IN_MONTH) {
            let (result, n) = caclulate_elapsed(days, SECONDS_IN_WEEK);
            prop_assert_eq!(result, Elapsed::Weeks(n));
        }

        #[test]
        fn elapsed_month_bucket(days in SECONDS_IN_MONTH + 1..SECONDS_IN_YEAR) {
            let (result, n) = caclulate_elapsed(days, SECONDS_IN_MONTH);
            prop_assert_eq!(result, Elapsed::Months(n));
        }

        #[test]
        fn elapsed_year_bucket(days in SECONDS_IN_YEAR + 1..SECONDS_IN_YEAR * SECONDS_IN_YEAR) {
            let (result, n) = caclulate_elapsed(days, SECONDS_IN_YEAR);
            prop_assert_eq!(result, Elapsed::Years(n));
        }
    }
}

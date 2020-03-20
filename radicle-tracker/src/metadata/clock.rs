// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

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
//! use radicle_tracker::clock::{Clock, Elapsed, RadClock, TimeDiff};
//! use std::thread::sleep;
//! use std::time::Duration;
//!
//! let now = RadClock::current_time();
//! sleep(Duration::new(1, 0));
//! let then = RadClock::current_time();
//!
//! let elapsed = now.elapsed(&then);
//! assert_eq!(elapsed, Elapsed::Minutes(TimeDiff::from(0)));
//! #
//! #     Ok(())
//! # }
//! ```
use std::{
    fmt,
    ops::{Div, Mul, Neg, Sub},
    time::{SystemTime, UNIX_EPOCH},
};

use num_bigint::BigInt;
pub use num_bigint::Sign;

// Rough calculations for the number of seconds in some larger unit
const SECONDS_IN_MINUTE: u64 = 60;
const SECONDS_IN_HOUR: u64 = SECONDS_IN_MINUTE * 60;
const SECONDS_IN_DAY: u64 = SECONDS_IN_HOUR * 24;
const SECONDS_IN_WEEK: u64 = SECONDS_IN_DAY * 7;
const SECONDS_IN_MONTH: u64 = SECONDS_IN_WEEK * 4;
const SECONDS_IN_YEAR: u64 = SECONDS_IN_MONTH * 12;

/// `TimeDiff` is the difference between two points in time. It uses a
/// [`num_bigint::BigInt`] under the hood. The functionality of this is limited
/// to `Display` and [`TimeDiff::sign`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimeDiff(BigInt);

impl TimeDiff {
    /// Check the [`Sign`] to see what moment in time it occurred:
    ///     * `Sign::Minus` LHS < RHS
    ///     * `Sign::Plus` LHS > RHS

    pub fn sign(&self) -> Sign {
        self.0.sign()
    }

    fn abs(self) -> Self {
        match self.sign() {
            Sign::Minus => self.neg(),
            _ => self,
        }
    }
}

impl fmt::Display for TimeDiff {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Sub for TimeDiff {
    type Output = TimeDiff;

    fn sub(self, rhs: TimeDiff) -> Self::Output {
        TimeDiff(self.0 - rhs.0)
    }
}

impl Mul<TimeDiff> for TimeDiff {
    type Output = TimeDiff;

    fn mul(self, other: TimeDiff) -> TimeDiff {
        TimeDiff(self.0 * other.0)
    }
}

impl Div<TimeDiff> for TimeDiff {
    type Output = TimeDiff;

    fn div(self, other: TimeDiff) -> TimeDiff {
        TimeDiff(self.0 / other.0)
    }
}

impl Neg for TimeDiff {
    type Output = Self;

    fn neg(self) -> Self::Output {
        TimeDiff(self.0.neg())
    }
}

impl From<u64> for TimeDiff {
    fn from(u: u64) -> Self {
        TimeDiff(BigInt::from(u))
    }
}

impl From<SystemTime> for TimeDiff {
    fn from(t: SystemTime) -> Self {
        Self(
            t.duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs().into())
                .unwrap_or_else(|e| BigInt::from(e.duration().as_secs()).neg()),
        )
    }
}

/// The elapsed time for two different points in time.
///
/// This is to capture concepts like, "this comment was posted 4 minutes ago",
/// or "5 days ago", or "1 year ago". If the `Elapsed` is calculated like:
/// `then.elapsed(now)`, then the reference point switches from "ago" to
/// "since".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Elapsed {
    /// Elapsed "x minutes ago/since".
    Minutes(TimeDiff),
    /// Elapsed "x hours ago/since".
    Hours(TimeDiff),
    /// Elapsed "x days ago/since".
    Days(TimeDiff),
    /// Elapsed "x weeks ago/since".
    Weeks(TimeDiff),
    /// Elapsed "x months ago/since".
    Months(TimeDiff),
    /// Elapsed "x years ago/since".
    Years(TimeDiff),
}

impl std::fmt::Display for Elapsed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let reference_point = |diff: &TimeDiff| match diff.sign() {
            Sign::Plus => "since",
            Sign::NoSign => "ago",
            Sign::Minus => "ago",
        };
        match self {
            Elapsed::Minutes(m) => {
                write!(f, "{} minute(s) {}", m.clone().abs(), reference_point(m))
            },
            Elapsed::Hours(m) => write!(f, "{} hour(s) {}", m.clone().abs(), reference_point(m)),
            Elapsed::Days(m) => write!(f, "{} day(s) {}", m.clone().abs(), reference_point(m)),
            Elapsed::Weeks(m) => write!(f, "{} week(s) {}", m.clone().abs(), reference_point(m)),
            Elapsed::Months(m) => write!(f, "{} month(s) {}", m.clone().abs(), reference_point(m)),
            Elapsed::Years(m) => write!(f, "{} year(s) {}", m.clone().abs(), reference_point(m)),
        }
    }
}

impl Neg for Elapsed {
    type Output = Self;

    fn neg(self) -> Self::Output {
        match self {
            Elapsed::Minutes(t) => Elapsed::Minutes(t.neg()),
            Elapsed::Hours(t) => Elapsed::Hours(t.neg()),
            Elapsed::Days(t) => Elapsed::Days(t.neg()),
            Elapsed::Weeks(t) => Elapsed::Weeks(t.neg()),
            Elapsed::Months(t) => Elapsed::Months(t.neg()),
            Elapsed::Years(t) => Elapsed::Years(t.neg()),
        }
    }
}

/// `Clock` captures the minimal functionality for grabbing a point in time and
/// calculating the duration since another point in time.
pub trait Clock {
    /// Get the current timestamp.
    fn current_time() -> Self;

    /// Get the duration for a timestamp in the past, `self`, up to the point of
    /// `other`. It should return [`std::time::Duration`], which can be used
    /// for further calculation.
    fn diff_since(&self, other: &Self) -> TimeDiff;
}

/// A minimal clock for getting a moment in time and calculating the duration up
/// to another moment in time.
///
/// It does this through its instance of [`Clock`] trait.
///
/// For usability, it also provides a function [`RadClock::elapsed`] which
/// returns an [`Elapsed`] value for presentation purposes.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct RadClock(SystemTime);

impl Clock for RadClock {
    fn current_time() -> Self {
        RadClock(SystemTime::now())
    }

    fn diff_since(&self, other: &Self) -> TimeDiff {
        TimeDiff::from(self.0) - TimeDiff::from(other.0)
    }
}

impl RadClock {
    /// Calculate the [`Elapsed`] time for two `RadClock`s.
    pub fn elapsed(&self, other: &Self) -> Elapsed {
        elapsed(self, other)
    }
}

fn elapsed<C>(clock: &C, other: &C) -> Elapsed
where
    C: Clock,
{
    let seconds = clock.diff_since(other);

    let elapsed_since = |seconds: TimeDiff| {
        if seconds < TimeDiff::from(SECONDS_IN_MINUTE) {
            Elapsed::Minutes(TimeDiff::from(0))
        } else if seconds >= TimeDiff::from(SECONDS_IN_MINUTE)
            && seconds < TimeDiff::from(SECONDS_IN_HOUR)
        {
            let minutes = seconds / TimeDiff::from(SECONDS_IN_MINUTE);
            Elapsed::Minutes(minutes)
        } else if seconds >= TimeDiff::from(SECONDS_IN_HOUR)
            && seconds < TimeDiff::from(SECONDS_IN_DAY)
        {
            let hours = seconds / TimeDiff::from(SECONDS_IN_HOUR);
            Elapsed::Hours(hours)
        } else if seconds >= TimeDiff::from(SECONDS_IN_DAY)
            && seconds < TimeDiff::from(SECONDS_IN_WEEK)
        {
            let days = seconds / TimeDiff::from(SECONDS_IN_DAY);
            Elapsed::Days(days)
        } else if seconds >= TimeDiff::from(SECONDS_IN_WEEK)
            && seconds < TimeDiff::from(SECONDS_IN_MONTH)
        {
            let weeks = seconds / TimeDiff::from(SECONDS_IN_WEEK);
            Elapsed::Weeks(weeks)
        } else if seconds >= TimeDiff::from(SECONDS_IN_MONTH)
            && seconds < TimeDiff::from(SECONDS_IN_YEAR)
        {
            let months = seconds / TimeDiff::from(SECONDS_IN_MONTH);
            Elapsed::Months(months)
        } else {
            let years = seconds / TimeDiff::from(SECONDS_IN_YEAR);
            Elapsed::Years(years)
        }
    };

    match seconds.sign() {
        Sign::Plus => elapsed_since(seconds),
        Sign::Minus => elapsed_since(seconds.neg()).neg(),
        Sign::NoSign => elapsed_since(seconds),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::time::Duration;

    // Calculate the elapsed value and the expected division value.
    fn caclulate_elapsed(value: u64, bound: u64) -> (Elapsed, TimeDiff) {
        let now = RadClock::current_time();
        let then = add(&RadClock::current_time(), &TimeDiff::from(value));
        (now.elapsed(&then), TimeDiff::from(value / bound))
    }

    // Helper to add a TimeDiff to the inner SystemTime of a RadClock.
    // It only aims to calculate up to u64 because Duration takes a u64 for seconds.
    fn to_duration(diff: &TimeDiff) -> Duration {
        let (sign, digits) = diff.0.to_u32_digits();
        let seconds = if digits.is_empty() {
            0
        } else if digits.len() == 1 {
            digits[0] as u64
        } else if digits.len() == 2 {
            2_u64.pow(32) * digits[1] as u64 + digits[0] as u64
        } else {
            panic!(
                "to_duration is written to calculate u64 max - sign: {:?}, digits: {:?}",
                sign, digits
            )
        };

        Duration::from_secs(seconds)
    }

    // Add a TimeDiff to a RadClock to time travel.
    fn add(clock: &RadClock, diff: &TimeDiff) -> RadClock {
        RadClock(clock.0 + to_duration(diff))
    }

    // `-n` is passed in because we are always getting the TimeDiff between a past
    // value and a later value.
    //
    // The properties ensure that we always fall into the correct buckets for
    // elapsed calculations.
    proptest! {
        #[test]
        fn elapsed_minutes_bucket(minutes in 0u64..SECONDS_IN_HOUR) {
            let (result, n) = caclulate_elapsed(minutes, SECONDS_IN_MINUTE);
            prop_assert_eq!(result, Elapsed::Minutes(-n))
        }

        #[test]
        fn elapsed_hours_bucket(hours in SECONDS_IN_HOUR..SECONDS_IN_DAY) {
            let (result, n) = caclulate_elapsed(hours, SECONDS_IN_HOUR);
            prop_assert_eq!(result, Elapsed::Hours(-n));
        }

        #[test]
        fn elapsed_days_bucket(days in SECONDS_IN_DAY + 1..SECONDS_IN_WEEK) {
            let (result, n) = caclulate_elapsed(days, SECONDS_IN_DAY);
            prop_assert_eq!(result, Elapsed::Days(-n));
        }

        #[test]
        fn elapsed_week_bucket(days in SECONDS_IN_WEEK + 1..SECONDS_IN_MONTH) {
            let (result, n) = caclulate_elapsed(days, SECONDS_IN_WEEK);
            prop_assert_eq!(result, Elapsed::Weeks(-n));
        }

        #[test]
        fn elapsed_month_bucket(days in SECONDS_IN_MONTH + 1..SECONDS_IN_YEAR) {
            let (result, n) = caclulate_elapsed(days, SECONDS_IN_MONTH);
            prop_assert_eq!(result, Elapsed::Months(-n));
        }

        #[test]
        fn elapsed_year_bucket(days in SECONDS_IN_YEAR + 1..SECONDS_IN_YEAR * SECONDS_IN_YEAR) {
            let (result, n) = caclulate_elapsed(days, SECONDS_IN_YEAR);
            prop_assert_eq!(result, Elapsed::Years(-n));
        }
    }
}

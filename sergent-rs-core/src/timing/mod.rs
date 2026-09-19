//! Wall-clock facts carried by run evidence. @sergent/docs/execution-model.md
//!
//! These are inert data types. Core never reads a clock; the runtime mints
//! timestamps and durations and stores them here.

use serde::{Serialize, Serializer};

const MICROS_PER_SECOND: u64 = 1_000_000;
const SECONDS_PER_DAY: u64 = 86_400;

/// A microsecond-capable Unix wall-clock instant with one exact UTC projection.
/// @sergent/docs/observability.md
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp {
    microseconds_since_epoch: u64,
}

impl Timestamp {
    /// Construct inert wall-clock evidence from microseconds since Unix epoch.
    pub const fn from_unix_micros(microseconds_since_epoch: u64) -> Self {
        Self {
            microseconds_since_epoch,
        }
    }

    /// Return the stored Unix instant for clock adapters and exact tests.
    pub const fn as_unix_micros(self) -> u64 {
        self.microseconds_since_epoch
    }

    /// Format the compact UTC timestamp used by Run Record filenames.
    pub fn compact_utc(self) -> String {
        let parts = self.utc_parts();
        format!(
            "{:04}{:02}{:02}T{:02}{:02}{:02}{:06}Z",
            parts.year,
            parts.month,
            parts.day,
            parts.hour,
            parts.minute,
            parts.second,
            parts.microsecond,
        )
    }

    /// Format the exact Run Record UTC representation.
    fn iso_utc(self) -> String {
        let parts = self.utc_parts();
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z",
            parts.year,
            parts.month,
            parts.day,
            parts.hour,
            parts.minute,
            parts.second,
            parts.microsecond,
        )
    }

    /// Split the stored Unix instant into Gregorian UTC fields.
    fn utc_parts(self) -> UtcParts {
        let seconds = self.microseconds_since_epoch / MICROS_PER_SECOND;
        let days = seconds / SECONDS_PER_DAY;
        let seconds_of_day = seconds % SECONDS_PER_DAY;
        let (year, month, day) = civil_from_days(days);
        UtcParts {
            year,
            month,
            day,
            hour: seconds_of_day / 3_600,
            minute: seconds_of_day % 3_600 / 60,
            second: seconds_of_day % 60,
            microsecond: self.microseconds_since_epoch % MICROS_PER_SECOND,
        }
    }
}

impl Serialize for Timestamp {
    /// Serialize with exactly six fractional digits and a literal `Z` suffix.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.iso_utc())
    }
}

/// Calendar fields derived from one non-negative Unix instant.
struct UtcParts {
    year: u64,
    month: u64,
    day: u64,
    hour: u64,
    minute: u64,
    second: u64,
    microsecond: u64,
}

/// Convert days since Unix epoch to a proleptic Gregorian date.
fn civil_from_days(days_since_epoch: u64) -> (u64, u64, u64) {
    let shifted = u128::from(days_since_epoch) + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    year += u128::from(month <= 2);
    (
        u64::try_from(year).expect("timestamp year exceeds u64"),
        u64::try_from(month).expect("timestamp month exceeds u64"),
        u64::try_from(day).expect("timestamp day exceeds u64"),
    )
}

/// A wall-clock lifecycle with a caller-measured duration. A closed span
/// carries both `finished_at` and `duration_ms`. @sergent/docs/execution-model.md
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct TimeSpan {
    /// When the span opened.
    started_at: Timestamp,
    /// When the span closed.
    finished_at: Timestamp,
    /// The measured duration in milliseconds.
    duration_ms: u64,
}

impl TimeSpan {
    /// Construct one closed span from caller-measured wall-clock facts.
    pub fn closed(started_at: Timestamp, finished_at: Timestamp, duration_ms: u64) -> Self {
        Self {
            started_at,
            finished_at,
            duration_ms,
        }
    }

    /// When the span opened.
    pub fn started_at(&self) -> Timestamp {
        self.started_at
    }

    /// When the span closed.
    pub fn finished_at(&self) -> Timestamp {
        self.finished_at
    }

    /// The caller-measured duration in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        self.duration_ms
    }
}

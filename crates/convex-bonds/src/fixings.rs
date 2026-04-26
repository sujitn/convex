//! Overnight-rate fixing history. ARRC compounding reads this for past
//! business days; the projection curve covers anything past `as_of`.

use std::collections::BTreeMap;
use std::path::Path;

use rust_decimal::Decimal;

use convex_core::types::Date;

#[derive(Debug, Clone, Default)]
pub struct OvernightFixings {
    fixings: BTreeMap<Date, Decimal>,
    /// When set, lookups for dates strictly after `as_of` return `None`
    /// (and the caller falls through to a projection curve). This avoids
    /// look-ahead bias when the registry contains rates published after
    /// the pricing valuation date.
    as_of: Option<Date>,
}

impl OvernightFixings {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a clone with the given `as_of` cutoff. Cheap — fixings is
    /// a `BTreeMap`, but only one call per pricing run is expected.
    #[must_use]
    pub fn with_as_of(mut self, as_of: Date) -> Self {
        self.as_of = Some(as_of);
        self
    }

    /// Insert one fixing as a decimal rate (e.g. `dec!(0.0387)` for 3.87%).
    pub fn insert(&mut self, date: Date, rate: Decimal) {
        self.fixings.insert(date, rate);
    }

    /// Insert one fixing, percent-quoted (e.g. 3.87 for 3.87%). Used by the
    /// CSV loader.
    pub fn insert_percent(&mut self, date: Date, rate_pct: f64) {
        let rate = Decimal::try_from(rate_pct / 100.0).unwrap_or(Decimal::ZERO);
        self.fixings.insert(date, rate);
    }

    /// Returns the fixing for `date`, or `None` if missing or strictly
    /// after the registry's `as_of` cutoff.
    #[must_use]
    pub fn lookup(&self, date: Date) -> Option<Decimal> {
        if self.as_of.map_or(false, |a| date > a) {
            return None;
        }
        self.fixings.get(&date).copied()
    }

    /// Load fixings from a CSV with header `effective_date,rate_pct,...` (or
    /// `date,rate`). Extra columns are ignored.
    pub fn from_csv(path: impl AsRef<Path>) -> Result<Self, FixingsError> {
        let mut rdr = csv::Reader::from_path(path.as_ref())?;
        let headers = rdr.headers()?.clone();
        let date_col = pick(&headers, &["effective_date", "date"])?;
        let rate_col = pick(&headers, &["rate_pct", "rate"])?;

        let mut out = Self::new();
        for record in rdr.records() {
            let row = record?;
            let date_str = row.get(date_col).unwrap_or("");
            let rate_str = row.get(rate_col).unwrap_or("");
            let date = Date::parse(date_str)
                .map_err(|e| FixingsError::Parse(format!("date {date_str:?}: {e}")))?;
            let rate_pct: f64 = rate_str
                .parse()
                .map_err(|e| FixingsError::Parse(format!("rate {rate_str:?}: {e}")))?;
            out.insert_percent(date, rate_pct);
        }
        Ok(out)
    }
}

fn pick(headers: &csv::StringRecord, names: &[&'static str]) -> Result<usize, FixingsError> {
    for n in names {
        if let Some(i) = headers.iter().position(|h| h == *n) {
            return Ok(i);
        }
    }
    Err(FixingsError::MissingColumn(names[0]))
}

#[derive(Debug, thiserror::Error)]
pub enum FixingsError {
    #[error(transparent)]
    Csv(#[from] csv::Error),
    #[error("fixings CSV missing column: {0}")]
    MissingColumn(&'static str),
    #[error("fixings CSV: {0}")]
    Parse(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn parses_three_row_csv() {
        let path = std::env::temp_dir().join("convex_fixings_test.csv");
        std::fs::write(
            &path,
            "effective_date,rate_pct,volume_usd_bn\n\
             2025-12-31,3.87,3485\n\
             2025-12-30,3.71,3321\n\
             2025-12-29,3.77,3346\n",
        )
        .unwrap();
        let f = OvernightFixings::from_csv(&path).unwrap();
        assert_eq!(
            f.lookup(Date::from_ymd(2025, 12, 31).unwrap()),
            Some(dec!(0.0387))
        );
    }

    #[test]
    fn as_of_cutoff_filters_future_fixings() {
        let mut f = OvernightFixings::new();
        f.insert_percent(Date::from_ymd(2025, 12, 30).unwrap(), 3.71);
        f.insert_percent(Date::from_ymd(2025, 12, 31).unwrap(), 3.87);
        let f = f.with_as_of(Date::from_ymd(2025, 12, 30).unwrap());
        assert_eq!(
            f.lookup(Date::from_ymd(2025, 12, 30).unwrap()),
            Some(dec!(0.0371))
        );
        assert_eq!(f.lookup(Date::from_ymd(2025, 12, 31).unwrap()), None);
    }
}

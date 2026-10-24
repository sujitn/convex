//! Exact predeclared calendar controls for raw and checked 30E/360 ISDA.
use convex_core::{
    daycounts::{DayCount, Thirty360EIsda},
    types::Date,
};
use rust_decimal::Decimal;
use serde::Deserialize;

#[derive(Deserialize)]
struct Grid {
    requests: usize,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    start: Date,
    end: Date,
    termination_date: Option<Date>,
    independent_days: i64,
    independent_fraction_numerator: i64,
    independent_fraction_denominator: i64,
}

#[test]
fn raw_and_checked_isda_follow_the_independent_calendar_grid() {
    let grid: Grid = serde_json::from_str(include_str!("fixtures/thirty360-isda.json")).unwrap();
    assert_eq!(grid.requests, 1752);
    assert_eq!(grid.cases.len(), grid.requests);
    for row in grid.cases {
        let convention = Thirty360EIsda::new(row.termination_date);
        let erased: &dyn DayCount = &convention;
        assert_eq!(
            erased.day_count(row.start, row.end),
            row.independent_days,
            "{}",
            row.id
        );
        let checked = convention
            .checked_year_fraction(row.start, row.end)
            .unwrap();
        assert_eq!(
            erased.year_fraction(row.start, row.end),
            checked,
            "{}",
            row.id
        );
        let reference = Decimal::from(row.independent_fraction_numerator)
            / Decimal::from(row.independent_fraction_denominator);
        // The exact rational reference is independently generated. Its Decimal
        // conversion reserve stays strictly inside the original 1e-14 bound.
        assert!(
            (checked - reference).abs() + Decimal::new(1, 24) <= Decimal::new(1, 14),
            "{}",
            row.id
        );
    }
}

#[test]
fn checked_isda_keeps_its_original_forward_calendar_validation() {
    let start = Date::from_ymd(2025, 1, 31).unwrap();
    let end = Date::from_ymd(2025, 3, 31).unwrap();
    let convention = Thirty360EIsda::new(Some(end));
    assert!(convention.checked_year_fraction(end, start).is_err());
    for year in [0, 10000] {
        let invalid = Date::from_ymd(year, 1, 1).unwrap();
        assert!(convention.checked_year_fraction(invalid, invalid).is_err());
        assert!(Thirty360EIsda::new(Some(invalid))
            .checked_year_fraction(start, end)
            .is_err());
    }
}

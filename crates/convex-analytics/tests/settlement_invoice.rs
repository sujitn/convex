//! Exact independent supplied-value invoice and bounded calendar controls.
use chrono::NaiveDate;
use convex_analytics::yas::{
    calculate_accrued_amount, calculate_proceeds, calculate_settlement_date, SettlementInvoice,
    SettlementInvoiceBuilder, SettlementInvoiceError,
};
use convex_core::{
    calendars::{checked, Calendar, DynamicCalendar, UKCalendar, WeekendCalendar, WeekendType},
    Date,
};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::{
    str::FromStr,
    sync::atomic::{AtomicUsize, Ordering},
};

fn d(text: &str) -> Decimal {
    Decimal::from_str(text).unwrap()
}
fn date(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}
fn reference() -> Value {
    serde_json::from_str(include_str!("fixtures/settlement-invoice.json")).unwrap()
}
fn complete() -> SettlementInvoiceBuilder {
    SettlementInvoice::builder()
        .settlement_date(date("2020-04-29"))
        .accrued_days(134)
        .face_value(d("1000000"))
        .clean_price(d("100"))
        .accrued_interest(d("2.5"))
}

#[test]
fn every_independent_invoice_preserves_all_original_components() {
    let pack = reference();
    let mut actual = vec![];
    for row in pack["cases"].as_array().unwrap() {
        let field = |key: &str| d(row[key].as_str().unwrap());
        let invoice = complete()
            .face_value(field("face_value"))
            .clean_price(field("clean_price"))
            .accrued_interest(field("accrued_interest"))
            .build()
            .unwrap();
        assert_eq!(invoice.settlement_date, date("2020-04-29"));
        assert_eq!(invoice.accrued_days, 134);
        assert_eq!(invoice.face_value, field("face_value"));
        assert_eq!(invoice.clean_price, field("clean_price"));
        assert_eq!(invoice.accrued_interest, field("accrued_interest"));
        assert_eq!(invoice.dirty_price, field("dirty_price"));
        assert_eq!(invoice.principal_amount, field("principal_amount"));
        assert_eq!(invoice.accrued_amount, field("accrued_amount"));
        assert_eq!(invoice.settlement_amount, field("settlement_amount"));
        let proceeds = calculate_proceeds(
            invoice.face_value,
            invoice.clean_price,
            invoice.accrued_interest,
        )
        .unwrap();
        assert_eq!(proceeds, field("settlement_amount"));
        actual.push(
            json!({"id":row["id"],"dirty_price":invoice.dirty_price.to_string(),
            "principal_amount":invoice.principal_amount.to_string(),
            "accrued_amount":invoice.accrued_amount.to_string(),
            "settlement_amount":invoice.settlement_amount.to_string(),
            "proceeds":proceeds.to_string()}),
        );
    }
    assert_eq!(actual.len(), 40);
    println!(
        "{}",
        json!({"kind":"complete_invoice_reference_results","cases":actual})
    );
}

#[test]
fn original_and_extreme_accrual_products_match_independent_exact_values() {
    let pack = reference();
    let mut actual = vec![];
    for row in pack["accrual_cases"].as_array().unwrap() {
        let field = |key: &str| d(row[key].as_str().unwrap());
        let amount = calculate_accrued_amount(
            field("face_value"),
            field("coupon_rate"),
            field("accrued_fraction"),
        )
        .unwrap();
        assert_eq!(amount, field("amount"));
        actual.push(amount.to_string());
    }
    assert_eq!(actual.len(), 7);
    println!(
        "{}",
        json!({"kind":"complete_accrual_reference_results","amounts":actual})
    );
}

#[test]
fn every_invoice_field_is_required_without_manufactured_metadata() {
    let names = [
        "clean_price",
        "accrued_interest",
        "face_value",
        "settlement_date",
        "accrued_days",
    ];
    for (missing, expected) in names.iter().enumerate() {
        let mut builder = SettlementInvoice::builder();
        if missing != 0 {
            builder = builder.clean_price(d("100"));
        }
        if missing != 1 {
            builder = builder.accrued_interest(d("2.5"));
        }
        if missing != 2 {
            builder = builder.face_value(d("1000000"));
        }
        if missing != 3 {
            builder = builder.settlement_date(date("2020-04-29"));
        }
        if missing != 4 {
            builder = builder.accrued_days(134);
        }
        assert!(
            matches!(builder.build(), Err(SettlementInvoiceError::Missing(field)) if field==*expected)
        );
    }
}

#[test]
fn overflow_and_nonzero_underflow_return_errors_without_panics() {
    for builder in [
        complete()
            .face_value(Decimal::MAX)
            .clean_price(Decimal::MAX)
            .accrued_interest(Decimal::ZERO),
        complete()
            .clean_price(Decimal::MAX)
            .accrued_interest(Decimal::MAX),
        complete()
            .face_value(Decimal::MAX)
            .clean_price(d("100"))
            .accrued_interest(d("1")),
        complete()
            .face_value(d("0.0000000000000000000000000001"))
            .clean_price(d("0.1")),
    ] {
        let actual = std::panic::catch_unwind(|| builder.build());
        assert!(matches!(
            actual,
            Ok(Err(SettlementInvoiceError::Arithmetic(_)))
        ));
    }
    assert!(matches!(
        calculate_proceeds(Decimal::MAX, Decimal::MAX, Decimal::ZERO),
        Err(SettlementInvoiceError::Arithmetic(_))
    ));
    assert!(matches!(
        calculate_accrued_amount(Decimal::MAX, Decimal::MAX, Decimal::ONE),
        Err(SettlementInvoiceError::Arithmetic("accrued_amount"))
    ));
    assert!(matches!(
        calculate_accrued_amount(d("0.0000000000000000000000000001"), d("0.01"), d("0.01")),
        Err(SettlementInvoiceError::Arithmetic("accrued_amount"))
    ));
}

#[test]
fn signed_accrued_metadata_and_currency_neutral_display_are_retained() {
    let invoice = complete()
        .face_value(d("-1000000"))
        .accrued_interest(d("-2.5"))
        .accrued_days(-134)
        .build()
        .unwrap();
    assert_eq!(invoice.accrued_days, -134);
    assert_eq!(invoice.accrued_amount, d("25000"));
    assert_eq!(invoice.settlement_amount, d("-975000"));
    let display = invoice.to_string();
    assert!(!display.contains('$'));
    assert!(display.contains("Settlement:") && display.contains("-975000.00"));
    let wire = serde_json::to_value(&invoice).unwrap();
    let restored: SettlementInvoice = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(restored).unwrap(), wire);
    assert_eq!(wire.as_object().unwrap().len(), 9);
}

#[test]
fn settlement_uses_the_actual_supplied_weekend_and_holiday_calendar() {
    assert_eq!(
        calculate_settlement_date(date("2020-04-27"), 2, &WeekendCalendar).unwrap(),
        date("2020-04-29")
    );
    let supplied = DynamicCalendar::from_dates(
        "Supplied",
        WeekendType::SaturdaySunday,
        [Date::from(date("2020-04-28"))],
    );
    assert_eq!(
        calculate_settlement_date(date("2020-04-27"), 2, &supplied).unwrap(),
        date("2020-04-30")
    );
    assert_eq!(
        calculate_settlement_date(date("2025-12-31"), 1, &UKCalendar::new()).unwrap(),
        date("2026-01-02")
    );
}

struct CountedCalendar {
    open: bool,
    queries: AtomicUsize,
}
impl Calendar for CountedCalendar {
    fn name(&self) -> &'static str {
        "Counted"
    }
    fn is_business_day(&self, _: Date) -> bool {
        self.queries.fetch_add(1, Ordering::Relaxed);
        self.open
    }
}

#[test]
fn settlement_search_and_offsets_obey_the_existing_native_bounds() {
    let closed = CountedCalendar {
        open: false,
        queries: AtomicUsize::new(0),
    };
    assert!(calculate_settlement_date(date("2025-01-02"), 1, &closed).is_err());
    assert_eq!(
        closed.queries.load(Ordering::Relaxed),
        checked::MAX_CALENDAR_SEARCH_DAYS
    );
    let open = CountedCalendar {
        open: true,
        queries: AtomicUsize::new(0),
    };
    for days in [367, u32::MAX] {
        assert!(calculate_settlement_date(date("2025-01-02"), days, &open).is_err());
    }
    assert_eq!(open.queries.load(Ordering::Relaxed), 0);
    assert_eq!(
        calculate_settlement_date(date("2025-01-02"), 366, &open).unwrap(),
        date("2026-01-03")
    );
    assert_eq!(open.queries.load(Ordering::Relaxed), 366);
}

#[test]
fn invalid_financial_dates_and_unavailable_calendar_coverage_fail() {
    let calendar = UKCalendar::new();
    for input in [
        NaiveDate::MIN,
        NaiveDate::MAX,
        NaiveDate::from_ymd_opt(0, 1, 1).unwrap(),
        NaiveDate::from_ymd_opt(10000, 1, 1).unwrap(),
    ] {
        assert!(matches!(
            complete().settlement_date(input).build(),
            Err(SettlementInvoiceError::InvalidDate("settlement_date"))
        ));
        assert!(matches!(
            calculate_settlement_date(input, 1, &calendar),
            Err(SettlementInvoiceError::InvalidDate("trade_date"))
        ));
    }
    assert!(matches!(
        calculate_settlement_date(date("2100-12-31"), 1, &calendar),
        Err(SettlementInvoiceError::Calendar(_))
    ));
    assert!(matches!(
        calculate_settlement_date(date("9999-12-31"), 1, &WeekendCalendar),
        Err(SettlementInvoiceError::InvalidDate("settlement_date"))
    ));
}

#[test]
fn zero_offset_is_date_only_and_does_not_query_unsupported_holiday_data() {
    let input = date("2101-01-03");
    assert_eq!(
        calculate_settlement_date(input, 0, &UKCalendar::new()).unwrap(),
        input
    );
    let closed = CountedCalendar {
        open: false,
        queries: AtomicUsize::new(0),
    };
    assert_eq!(calculate_settlement_date(input, 0, &closed).unwrap(), input);
    assert_eq!(closed.queries.load(Ordering::Relaxed), 0);
}

//! Standalone supplied fixed-bond quotes and exact curve shifts, without a server.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]
use convex::{
    bond_quote::{quote_fixed, BondCurves},
    *,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let issue = Date::parse("2024-01-15")?;
    let settle = Date::parse("2025-02-17")?;
    let bond = FixedRateBond::builder()
        .cusip_unchecked("SUPPLIED-EXAMPLE")
        .coupon_rate("0.05".parse()?)
        .frequency(Frequency::SemiAnnual)
        .issue_date(issue)
        .dated_date(issue)
        .maturity(Date::parse("2029-01-15")?)
        .day_count(DayCountConvention::Thirty360US)
        .currency(Currency::USD)
        .face_value("100".parse()?)
        .redemption_value("100".parse()?)
        .calendar(CalendarId::weekend_only())
        .business_day_convention(BusinessDayConvention::Unadjusted)
        .settlement_days(0)
        .end_of_month(false)
        .build()?;
    let curve = RateCurve::new(DiscreteCurve::with_extrapolation(
        issue,
        vec![0.01, 1.0, 5.0, 10.0],
        vec![0.035; 4],
        ValueType::continuous_zero(DayCountConvention::Act365Fixed),
        InterpolationMethod::Linear,
        ExtrapolationMethod::Flat,
    )?);
    let mark = Mark::Price {
        value: "100".parse()?,
        kind: PriceKind::Clean,
    };
    let quote = quote_fixed(
        &bond,
        settle,
        &mark,
        Frequency::SemiAnnual,
        BondCurves {
            discount: Some(&curve),
            swap: Some(&curve),
            government: None,
        },
        1.0,
        &[0.25, 0.5, 1.0, 2.0, 5.0, 10.0],
    )?;
    let risk = quote.curve_risk.ok_or("missing curve risk")?;
    if quote.g_spread_bps.is_some()
        || quote.errors.len() != 1
        || risk.zero_pv01_signed >= 0.0
        || risk.key_rates.len() != 6
    {
        return Err("unexpected supplied quote / missing benchmark contract".into());
    }
    println!(
        "clean={} dirty={} YTM={} zero_PV01={} Z_PV01={} missing={:?}",
        quote.pricing.clean_price_per_100,
        quote.pricing.dirty_price_per_100,
        quote.pricing.ytm_decimal,
        risk.zero_pv01_signed,
        risk.z_pv01_signed,
        quote.errors[0].code()
    );
    Ok(())
}

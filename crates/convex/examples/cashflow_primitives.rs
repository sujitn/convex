//! Standalone CVX-054 consumer. Every economic input is supplied locally.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use convex::{
    cashflows::*,
    contracts::{IndexId, SourceId, UtcTimestamp, Version},
    fixings::*,
    Date, DayCountConvention, Frequency,
};
use convex_core::calendars::WeekendCalendar;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn date(s: &str) -> Result<Date> {
    Ok(Date::parse(s)?)
}
fn profile(kind: ProfileKind, values: &[(&str, &str, &str)]) -> Result<DatedProfile> {
    let entries = values
        .iter()
        .map(|&(start, end, value)| {
            Ok(DatedValue {
                start: date(start)?,
                end: date(end)?,
                value: value.parse()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(DatedProfile::new(kind, entries)?)
}
fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
fn publication(d: Date) -> std::result::Result<UtcTimestamp, CashflowError> {
    let date = DateOffset::BusinessDays(1).apply(d, &WeekendCalendar)?;
    UtcTimestamp::parse(&format!("{date}T08:00:00Z"))
        .map_err(|error| CashflowError::Date(error.to_string()))
}

fn main() -> Result<()> {
    let spec = ScheduleSpec {
        start_date: date("2024-01-01")?,
        end_date: date("2025-01-01")?,
        frequency: Frequency::SemiAnnual,
        stub_type: StubType::None,
        end_of_month: true,
        first_regular_date: None,
        penultimate_date: None,
    };
    let rules = DateRules::default();
    let schedule = CouponSchedule::generate(&spec, &rules, &WeekendCalendar)?;
    let profiles = CouponProfiles::new(
        profile(ProfileKind::Coupon, &[("2024-01-01", "2025-01-01", "0.06")])?,
        profile(ProfileKind::Spread, &[("2024-01-01", "2025-01-01", "0")])?,
        profile(ProfileKind::Gearing, &[("2024-01-01", "2025-01-01", "0")])?,
        profile(
            ProfileKind::Notional,
            &[
                ("2024-01-01", "2024-07-01", "100"),
                ("2024-07-01", "2025-01-01", "60"),
            ],
        )?,
    )?;
    let calculation = CouponCalculation {
        index_rate: "0".parse()?,
        day_count: DayCountConvention::ActActIcma,
        principal: CouponPrincipal::AccrualProfile,
        rounding: CashRounding::NearestEven(2),
        negative_coupons: NegativeCouponPolicy::Reject,
    };
    let coupons = schedule
        .periods()
        .iter()
        .map(|p| profiles.coupon_amount(p, calculation))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    require(
        coupons.len() == 2 && coupons[0] == "3".parse()? && coupons[1] == "1.8".parse()?,
        "independent notional coupons",
    )?;
    let principal = principal_cashflows(
        profiles.notional(),
        &rules,
        &WeekendCalendar,
        "1".parse()?,
        CashRounding::NearestEven(2),
    )?;
    require(
        principal.len() == 2
            && principal[0].amount == "40".parse()?
            && principal[1].amount == "60".parse()?,
        "principal conservation",
    )?;

    let overnight = OvernightSchedule::generate(
        date("2025-01-02")?,
        date("2025-01-08")?,
        DayCountConvention::Act360,
        &WeekendCalendar,
        OvernightConvention {
            lookback_business_days: 0,
            observation_shift: false,
            lockout_business_days: 0,
        },
    )?;
    let index = IndexId::new("USD.SOFR")?;
    let context = FixingContext {
        valuation_date: date("2025-01-06")?,
        known_at: UtcTimestamp::parse("2025-01-06T09:00:00Z")?,
        unpublished: UnpublishedFixingPolicy::Forecast,
        final_only: true,
    };
    let mut forecast_calls = 0;
    let missing = overnight.compound(
        &index,
        &FixingSeries::default(),
        context,
        publication,
        |_, _| {
            forecast_calls += 1;
            "0.05"
                .parse()
                .map_err(|_| CashflowError::Arithmetic("example rate"))
        },
    );
    let missing_count = match missing {
        Err(CashflowError::MissingFixings(list)) => list.len(),
        _ => return Err("missing history was not rejected".into()),
    };
    require(
        missing_count == 2 && forecast_calls == 0,
        "history must fail before projection",
    )?;
    let mut observations = Vec::new();
    for (d, rate) in [("2025-01-02", "0.04"), ("2025-01-03", "0.041")] {
        let observation_date = date(d)?;
        let instant = publication(observation_date)?;
        observations.push(FixingObservation {
            index_id: index.clone(),
            observation_date,
            rate: rate.parse()?,
            published_at: instant,
            known_at: instant,
            revision: Version::new(1)?,
            status: FixingStatus::Final,
            source_id: SourceId::new("supplied-fixture")?,
        });
    }
    let history = FixingSeries::new(observations)?;
    let compounded = overnight.compound(&index, &history, context, publication, |_, _| {
        "0.05"
            .parse()
            .map_err(|_| CashflowError::Arithmetic("example rate"))
    })?;
    let known = compounded
        .observations
        .iter()
        .filter(|o| matches!(o.origin, RateOrigin::Published(_)))
        .count();
    require(
        known == 2 && compounded.observations.len() == 4,
        "known/projected provenance",
    )?;
    println!("{{\"task\":\"CVX-054\",\"coupon_periods\":{},\"coupons\":[\"{}\",\"{}\"],\"principal\":[\"{}\",\"{}\"],\"missing_published_fixings\":{},\"known_observations\":{},\"projected_observations\":{},\"period_factor\":\"{}\",\"certified\":false}}",
        schedule.periods().len(),coupons[0],coupons[1],principal[0].amount,principal[1].amount,missing_count,known,compounded.observations.len()-known,compounded.period_factor);
    Ok(())
}

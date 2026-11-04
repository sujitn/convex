//! Standalone supplied OIS contract, eligible history and explicit forecasts.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]
use convex::{
    cashflows::{
        CashRounding, DateOffset, DateRules, DatedValue, NegativeCouponPolicy, OvernightConvention,
        PaymentDateEntitlement, ScheduleSpec, StubType,
    },
    contracts::{IndexId, ReferenceDataId, SourceId, TradeId, UtcTimestamp, Version},
    fixings::{
        FixingContext, FixingObservation, FixingSeries, FixingStatus, RateOrigin,
        UnpublishedFixingPolicy,
    },
    rates::*,
    BusinessDayConvention, Currency, Date, DayCountConvention, Frequency,
};
use convex_core::calendars::Calendar;

struct SuppliedCalendar {
    holiday: Date,
}
impl Calendar for SuppliedCalendar {
    fn name(&self) -> &'static str {
        "supplied synthetic calendar v1"
    }
    fn is_business_day(&self, at: Date) -> bool {
        !at.is_weekend() && at != self.holiday
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let start = Date::parse("2024-04-04")?;
    let end = Date::parse("2024-04-11")?;
    let cal = SuppliedCalendar {
        holiday: Date::parse("2024-04-08")?,
    };
    let pin = ReferencePin {
        id: ReferenceDataId::new("SUPPLIED.CALENDAR")?,
        version: Version::new(1)?,
    };
    let profile = |value| vec![DatedValue { start, end, value }];
    let leg = LegTerms {
        schedule: ScheduleSpec {
            start_date: start,
            end_date: end,
            frequency: Frequency::Monthly,
            stub_type: StubType::ShortLast,
            end_of_month: false,
            first_regular_date: None,
            penultimate_date: None,
        },
        dates: DateRules {
            payment_adjustment: BusinessDayConvention::Following,
            payment_delay: DateOffset::BusinessDays(2),
            ..DateRules::default()
        },
        calendars: LegCalendarPins {
            accrual: pin.clone(),
            payment: pin.clone(),
        },
        day_count: DayCountConvention::Act360,
        notionals: profile(1_000_000_i64.into()),
        rounding: CashRounding::NearestEven(2),
        negative_coupons: NegativeCouponPolicy::Allow,
    };
    let index = IndexId::new("SUPPLIED.ON")?;
    let projection = convex::curve_contracts::CurveKey {
        id: "SUPPLIED.PROJECTION".into(),
        version: "v1".into(),
    };
    let terms = SwapTerms {
        schema_version: 1,
        trade_id: TradeId::new("standalone-ois")?,
        version: Version::new(1)?,
        currency: Currency::USD,
        side: SwapSide::PayFixed,
        effective_date: start,
        maturity_date: end,
        fixed: FixedLegTerms {
            leg: leg.clone(),
            rates: profile("0.04".parse()?),
        },
        floating: FloatingLegTerms::Overnight(OvernightLegTerms {
            leg,
            index_id: index.clone(),
            observation_calendar: pin.clone(),
            convention: OvernightConvention {
                lookback_business_days: 0,
                observation_shift: false,
                lockout_business_days: 0,
            },
            profiles: FloatingProfiles {
                spreads: profile("0.001".parse()?),
                gearings: profile(1_i32.into()),
            },
            projection_curve: projection.clone(),
            fixing_policy: FixingPolicy {
                unpublished: UnpublishedFixingPolicy::Forecast,
                final_only: true,
            },
        }),
        collateral: CollateralTerms {
            agreement: ReferencePin {
                id: ReferenceDataId::new("SUPPLIED.CSA")?,
                version: Version::new(1)?,
            },
            currency: Currency::USD,
            discount_curve: convex::curve_contracts::CurveKey {
                id: "SUPPLIED.DISCOUNT".into(),
                version: "v1".into(),
            },
        },
    };
    let binding = CalendarBinding {
        pin: &pin,
        calendar: &cal,
    };
    let leg_calendars = LegCalendars {
        accrual: binding,
        payment: binding,
    };
    let swap = terms.build(&SwapCalendars {
        fixed: leg_calendars,
        floating: leg_calendars,
        reset: None,
        fixing: None,
        observation: Some(binding),
    })?;
    let publication =
        |_: &IndexId, at: Date| -> Result<UtcTimestamp, convex::cashflows::CashflowError> {
            UtcTimestamp::parse(&format!("{at}T08:00:00Z"))
                .map_err(|e| convex::cashflows::CashflowError::Date(e.to_string()))
        };
    let valuation = Date::parse("2024-04-09")?;
    let context = CashflowContext {
        fixings: FixingContext {
            valuation_date: valuation,
            known_at: UtcTimestamp::parse("2024-04-09T12:00:00Z")?,
            unpublished: UnpublishedFixingPolicy::Forecast,
            final_only: true,
        },
        on_payment: PaymentDateEntitlement::Include,
    };
    let mut history = Vec::new();
    for r in swap.fixing_requirements(context, publication)? {
        if r.observation_date <= valuation {
            history.push(FixingObservation {
                index_id: index.clone(),
                observation_date: r.observation_date,
                rate: "0.03".parse()?,
                published_at: r.expected_publication,
                known_at: r.expected_publication,
                revision: Version::new(1)?,
                status: FixingStatus::Final,
                source_id: SourceId::new("synthetic.fixture")?,
            });
        }
    }
    let cash = swap.cashflows(context, &FixingSeries::new(history)?, publication, |r| {
        Ok(ForecastRate {
            rate: "0.045"
                .parse()
                .map_err(|_| convex::cashflows::CashflowError::Arithmetic("fixture rate"))?,
            projection_curve: r.projection_curve.clone(),
        })
    })?;
    let known = cash
        .rates
        .iter()
        .filter(|r| matches!(r.resolved.origin, RateOrigin::Published(_)))
        .count();
    let fixed = cash
        .fixed
        .first()
        .ok_or_else(|| std::io::Error::other("missing expected fixed coupon"))?;
    let floating = cash
        .floating
        .first()
        .ok_or_else(|| std::io::Error::other("missing expected floating coupon"))?;
    println!(
        "fixed_cash={} floating_cash={} payment={} known={} projected={} certified=false",
        fixed.signed_amount,
        floating.signed_amount,
        floating.period.payment_date(),
        known,
        cash.rates.len() - known
    );
    Ok(())
}

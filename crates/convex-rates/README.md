# convex-rates

Interest rate swap contracts for Rust: fixed-versus-term-rate swaps and
fixed-versus-overnight (OIS) swaps in a single currency. You describe the
swap as plain data, build it into a contract, and ask for its expected
cashflows or its present value against a set of curves.

It is for people writing pricing or risk code who want swap schedules,
fixings and accruals handled explicitly, with no hidden market conventions.

## Where it sits

`convex-rates` builds on `convex-core` (dates, calendars, schedules, fixings)
and `convex-curves` (the curves used to project rates and discount cash).
`convex-analytics` builds on it for swap analytics.

## Install

```toml
[dependencies]
convex-rates = "0.14"
convex-core = "0.14"
convex-curves = "0.14"  # for the CurveSet that price_swap takes
serde_json = "1"   # only for the example below, which reads the terms from JSON
```

## What's in it

- `SwapTerms`: the swap described as data (legs, schedules, notionals, rates,
  index, curves). It can be read from and written to JSON.
- `SwapContract`: the result of `SwapTerms::build`. Schedules, reset dates and
  coupon periods are generated and checked once, up front.
- `SwapContract::cashflows`: expected fixed and floating coupons, with the
  rates used for each one.
- `pricing`: `price_swap` values a contract against a `CurveSet` from
  `convex-curves` and returns present values, accrued interest and par rates.
- `basis`: combines two floating legs into a basis swap.
- `results`: typed price results and a description of what the pricer supports.

Nothing is inferred. Calendars, fixing history, forecast rates and the
publication time of each fixing are passed in by the caller, and the contract
records which calendar and curve versions it was built against. Constructors
check their input and return `Result`.

## Example

Build a one-year, quarterly swap that pays 4% fixed and receives a 3-month
term rate, then list the fixed coupons. Floating rates come from a closure; here
it returns a flat 4.25%.

```rust
use convex_core::calendars::WeekendCalendar;
use convex_core::cashflows::{CashflowError, PaymentDateEntitlement};
use convex_core::contracts::{IndexId, UtcTimestamp};
use convex_core::fixings::{FixingContext, FixingSeries, UnpublishedFixingPolicy};
use convex_core::Date;
use convex_rates::{
    CalendarBinding, CashflowContext, ForecastRate, LegCalendars, SwapCalendars, SwapTerms,
};

fn span(value: &str) -> serde_json::Value {
    serde_json::json!([{ "start": "2024-01-02", "end": "2025-01-02", "value": value }])
}

fn leg() -> serde_json::Value {
    serde_json::json!({
        "schedule": { "start_date": "2024-01-02", "end_date": "2025-01-02",
            "frequency": "Quarterly", "stub_type": "None", "end_of_month": false,
            "first_regular_date": null, "penultimate_date": null },
        "dates": { "accrual_adjustment": "Unadjusted", "payment_adjustment": "Following",
            "payment_delay": { "BusinessDays": 0 }, "fixing_offset": null,
            "observation_offset": null, "ex_coupon": null },
        "calendars": { "accrual": { "id": "CAL", "version": 1 },
                       "payment": { "id": "CAL", "version": 1 } },
        "day_count": "Act360", "notionals": span("1000000"),
        "rounding": "Unrounded", "negative_coupons": "Allow"
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let terms: SwapTerms = serde_json::from_value(serde_json::json!({
        "schema_version": 1, "trade_id": "demo", "version": 1, "currency": "USD",
        "side": "PayFixed", "effective_date": "2024-01-02", "maturity_date": "2025-01-02",
        "fixed": { "leg": leg(), "rates": span("0.04") },
        "floating": {
            "kind": "term", "leg": leg(),
            "resets": {
                "index_id": "USD.TERM3M", "tenor_months": 3, "schedule": leg()["schedule"],
                "reset_calendar": { "id": "CAL", "version": 1 }, "reset_adjustment": "Unadjusted",
                "fixing_calendar": { "id": "CAL", "version": 1 },
                "fixing_offset": { "BusinessDays": -2 }, "fixing_adjustment": "Preceding",
                "rate_period": "ResetInterval", "index_end_of_month": false,
                "index_end_adjustment": "Unadjusted", "projection_day_count": "Act360"
            },
            "profiles": { "spreads": span("0"), "gearings": span("1") },
            "projection_curve": { "id": "USD.TERM3M.PROJ", "version": "v1" },
            "fixing_policy": { "unpublished": "Forecast", "final_only": true }
        },
        "collateral": {
            "agreement": { "id": "CSA.USD", "version": 1 }, "currency": "USD",
            "discount_curve": { "id": "USD.OIS", "version": "v1" }
        }
    }))?;

    // The caller supplies the calendars the terms refer to.
    let cal = WeekendCalendar;
    let pin = &terms.fixed.leg.calendars.accrual;
    let bind = CalendarBinding { pin, calendar: &cal };
    let leg = LegCalendars { accrual: bind, payment: bind };
    let calendars = SwapCalendars {
        fixed: leg,
        floating: leg,
        reset: Some(bind),
        fixing: Some(bind),
        observation: None,
    };
    let swap = terms.clone().build(&calendars)?;

    // Valuation date, and the time up to which fixings are known. The first
    // fixing is on 2023-12-29, so we value on that day and forecast it.
    let known_at: UtcTimestamp = serde_json::from_str("\"2023-12-28T23:59:59Z\"")?;
    let context = CashflowContext {
        fixings: FixingContext {
            valuation_date: Date::from_ymd(2023, 12, 29)?,
            known_at,
            unpublished: UnpublishedFixingPolicy::Forecast,
            final_only: true,
        },
        on_payment: PaymentDateEntitlement::Include,
    };

    // When each fixing is published, and what rate to forecast for it.
    let publication = |_: &IndexId, day: Date| -> Result<UtcTimestamp, CashflowError> {
        Ok(serde_json::from_str(&format!("\"{day}T08:00:00Z\"")).unwrap())
    };
    let forecast = |request: &convex_rates::ProjectionRequest| {
        Ok(ForecastRate {
            rate: "0.0425".parse().unwrap(),
            projection_curve: request.projection_curve.clone(),
        })
    };

    let cash = swap.cashflows(context, &FixingSeries::default(), publication, forecast)?;
    for coupon in &cash.fixed {
        println!("{}  {}", coupon.period.payment_date(), coupon.signed_amount);
    }
    assert_eq!(cash.floating.len(), 4);
    Ok(())
}
```

## Links

- API docs: <https://docs.rs/convex-rates>
- Contract rules: <https://github.com/sujitn/convex/blob/main/docs/RATES-CONTRACTS.md>
- Swap valuation: <https://github.com/sujitn/convex/blob/main/docs/SWAP-VALUATION.md>

## License

MIT. See LICENSE.

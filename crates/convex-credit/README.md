# convex-credit

Single-name credit default swap (CDS) valuation and quote conversion. You give
it the contract terms, a discount curve and a date for the valuation, and it
returns the protection and premium leg values, accrued premium, and the upfront
amount. It also converts between running spread and upfront quotes by solving for
a flat hazard rate.

The calculation follows the ISDA CDS Standard Model 1.8.3 and is written
in Rust. Use it directly if you need CDS maths without a larger pricing engine.
There is no clock, network or database access: dates, calendars and curves are
all passed in.

## Where it fits

`convex-credit` builds on `convex-core` (dates, calendars, schedules),
`convex-curves` (curves) and `convex-math` (root finding). `convex-analytics`
builds on it.

## Install

```toml
[dependencies]
convex-credit = "0.14"
convex-core = "0.14"
convex-curves = "0.14"
serde_json = "1"
```

`serde_json` is only needed because the example below reads the terms and
curve from JSON. `CdsTerms` and `CurveRecipe` can also be built directly in Rust.

## What's in it

- `CdsTerms`: the full description of a trade (side, notional, coupon,
  schedule, calendars, curve keys). `CdsTerms::build` checks the terms against
  the calendars you supply and returns a `CdsContract`.
- `CdsPricingContext`: valuation date, step-in date and cash settlement date.
- `price_cds`: value a contract against a supplied survival curve.
- `price_flat_hazard`: value it under a single constant hazard rate.
- `convert_cds` with `CdsQuote`: turn a running spread, clean upfront or dirty
  upfront into a fitted hazard rate and a full `CdsValuation`.
- `cds_roll_on_or_before` and `cds_roll_after`: find the nearest 20 March,
  June, September or December date on or before, or after, a given date.
- `CdsError`: every failure, including terms that do not validate.

Constructors and builders here are "checked": they return a `Result` and refuse
inconsistent input instead of guessing. For example, a quote that does not fit
in the allowed range, or a settlement date that disagrees with the calendar, is an
error.

## Example

This converts a 50 bp running spread on a five-year USD contract (1% coupon,
10 million notional) into an upfront amount. The discount curve is a simple
10-year curve at 3%; calendars have no holidays.

```rust
use convex_core::calendars::{DynamicCalendar, WeekendType};
use convex_credit::{
    convert_cds, CalendarBinding, CdsCalendars, CdsQuote, CdsTerms,
    ConversionSettings, InitialPaymentState,
};
use convex_curves::contracts::{build_curve_set, CurveRecipe, CurveSet};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let terms: CdsTerms = serde_json::from_str(r#"{
        "trade_id": "EXAMPLE-1", "version": 1,
        "side": "ProtectionBuyer", "currency": "USD", "notional": "10000000",
        "reference": {
            "entity_id": "Example.Corp", "seniority": "SeniorUnsecured",
            "document": {"id": "Example.Confirmation", "version": 1},
            "restructuring_clause": {"id": "Example.NoRestructuring", "version": 1},
            "recovery_fraction": 0.4
        },
        "model": "Standard183",
        "trade_date": "2022-06-21", "step_in_date": "2022-06-22",
        "protection_start_date": "2022-06-22", "cash_settlement_date": "2022-06-24",
        "settlement_lag_business_days": 3,
        "premium_schedule": {
            "start_date": "2022-06-21", "end_date": "2027-06-20",
            "frequency": "Quarterly", "stub_type": "ShortFirst",
            "end_of_month": false, "first_regular_date": null, "penultimate_date": null
        },
        "maturity_rule": "QuarterlyTwentieth",
        "coupon_decimal": "0.01", "premium_day_count": "Act360",
        "pay_accrual_on_default": true,
        "premium_calendar": {"id": "Example.Calendar", "version": 1},
        "settlement_calendar": {"id": "Example.Calendar", "version": 1},
        "discount_curve": {"id": "USD.DISCOUNT", "version": "1"},
        "survival_curve": {"id": "Example.Survival", "version": "1"},
        "initial_clean_upfront_fraction": null
    }"#)?;

    // 3% continuously compounded for ten years: exp(-0.3) = 0.7408182.
    let discount: CurveRecipe = serde_json::from_str(r#"{
        "key": {"id": "USD.DISCOUNT", "version": "1"},
        "role": {"kind": "Discount", "collateral_currency": "USD"},
        "currency": "USD", "reference_date": "2022-06-21",
        "time_basis": "Act365Fixed", "representation": "DiscountFactors",
        "interpolation": "LogLinear", "extrapolation": "None",
        "dependencies": [], "calibration": "Direct",
        "source": {"Nodes": [
            {"pillar": {"Date": "2022-06-21"},
             "quote": {"value": 1.0, "unit": "DiscountFactor"}},
            {"pillar": {"Date": "2032-06-21"},
             "quote": {"value": 0.7408182, "unit": "DiscountFactor"}}
        ]}
    }"#)?;
    let curves = build_curve_set(&[discount], &CurveSet::new())?.curves;

    let calendar = DynamicCalendar::from_dates(
        "no-holidays", WeekendType::SaturdaySunday, Vec::new());
    let contract = terms.build(&CdsCalendars {
        premium: CalendarBinding { pin: &terms.premium_calendar, calendar: &calendar },
        settlement: CalendarBinding { pin: &terms.settlement_calendar, calendar: &calendar },
    })?;

    let context = terms.trade_context(InitialPaymentState::NotContracted);
    let result = convert_cds(
        &contract,
        &context,
        &curves,
        CdsQuote::RunningSpread { decimal_rate: 0.005 },
        ConversionSettings::default(),
    )?;

    let v = &result.valuation;
    println!("hazard rate:      {:.5}", result.hazard_per_act365f_year);
    println!("clean upfront:    {:.4}% of notional", v.clean_upfront_fraction * 100.0);
    println!("accrued premium:  {:.2}", v.accrued_premium);
    println!("cash to settle:   {:.2}", v.cash_settlement_amount);
    assert!(v.clean_upfront_fraction < 0.0); // 50 bp spread, 100 bp coupon
    Ok(())
}
```

Because the spread is below the 1% coupon, the buyer receives an upfront
payment, so the clean upfront fraction is negative. Pass the same contract and
context to `price_cds` with a survival curve in the curve set to value it
without converting a quote.

## Links

- API documentation: <https://docs.rs/convex-credit>
- CDS valuation notes: <https://github.com/sujitn/convex/blob/main/docs/CDS-VALUATION.md>
- Repository: <https://github.com/sujitn/convex>

## License

MIT. See LICENSE.

This application is based on the ISDA CDS Standard Model (version 1.8.3), developed and supported in collaboration with Markit Group Ltd.

The ISDA CDS Standard Model Public License ships in this crate as
LICENSE-ISDA-CDS-STANDARD-MODEL.txt. This is an independent Rust
implementation; ISDA and Markit have not reviewed or endorsed it.

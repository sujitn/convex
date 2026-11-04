# convex-curves

Yield curves and credit curves for Rust. A curve here is a set of dated or
tenor-based points plus an interpolation method, and you ask it for discount
factors, zero rates, forward rates or survival probabilities. The crate also
fits curves to market quotes, bumps them for sensitivities, and holds
curves for several currencies and indices together.

Use it directly if you need a term structure for your own pricing code. Most
people reach it through the bond and swap crates.

## Where it sits

`convex-curves` builds on `convex-math` (interpolation, solvers) and
`convex-core` (dates, day counts, compounding). `convex-rates`,
`convex-credit` and `convex-bonds` build on it, and `convex-analytics` sits
above those.

## Install

```toml
[dependencies]
convex-curves = "0.14"
convex-core = "0.14"
```

## What's in it

- `curves`: `DiscreteCurve` and the builders `DiscountCurveBuilder` and
  `ZeroCurveBuilder`, plus `SegmentedCurve`, `DelegatedCurve` and
  `DerivedCurve` for combining or shifting curves.
- `wrappers`: `RateCurve` (discount factors, zero rates, forwards) and
  `CreditCurve` (survival probability, hazard rate) on top of any curve.
- `builder`: `CurveBuilder`, a step-by-step way to assemble a curve from
  segments.
- `calibration`: `GlobalFitter` and `SequentialBootstrapper`, with
  `Deposit`, `Fra`, `Future`, `Swap` and `Ois` instruments.
- `bumping`: parallel, key-rate and scenario shifts of a curve.
- `multicurve`: `MultiCurveEnvironment`, keyed by currency and rate index.
- `contracts`: curves described entirely as data (a `CurveRecipe`), built
  with `build_curve_set` and read with `CurveQuery`. Supports node curves,
  market-quote fitting, parametric models (Nelson-Siegel, Svensson), Monotone
  Convex and ultimate-forward-rate extension.
- `interpolation`: interpolators re-exported from `convex-math`.

Constructors check their input (sorted pillars, positive discount factors,
finite values) and return `Result`. A curve that exists is a valid curve.

## Example: a discount curve

```rust
use convex_core::types::{Compounding, Date};
use convex_curves::{DiscountCurveBuilder, InterpolationMethod};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let today = Date::from_ymd(2024, 1, 2)?;

    // Pillars are (years, continuously compounded zero rate).
    let curve = DiscountCurveBuilder::new(today)
        .add_zero_rate(1.0, 0.040)
        .add_zero_rate(2.0, 0.042)
        .add_zero_rate(5.0, 0.045)
        .with_interpolation(InterpolationMethod::Linear)
        .build()?;

    let df = curve.discount_factor_at_tenor(2.0)?;
    let zero = curve.zero_rate_at_tenor(2.0, Compounding::Continuous)?;
    assert!((zero - 0.042).abs() < 1e-9);
    assert!((df - (-0.042_f64 * 2.0).exp()).abs() < 1e-9);
    Ok(())
}
```

## Example: a curve described as data

The `contracts` module takes a recipe, which can be written as JSON, and
returns a built curve you can query by date or by tenor.

```rust
use convex_core::{daycounts::DayCountConvention, types::Currency};
use convex_curves::contracts::*;
use convex_curves::{ExtrapolationMethod, InterpolationMethod};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key = CurveKey { id: "USD.OIS".into(), version: "v1".into() };
    let node = |years: f64, discount: f64| CurveNode {
        pillar: CurvePoint::Tenor(years),
        quote: MarketQuote { value: discount, unit: QuoteUnit::DiscountFactor },
    };
    let recipe = CurveRecipe {
        key: key.clone(),
        role: CurveRole::Discount { collateral_currency: Currency::USD },
        currency: Currency::USD,
        reference_date: convex_core::types::Date::from_ymd(2024, 1, 2)?,
        time_basis: DayCountConvention::Act365Fixed,
        representation: CurveRepresentation::DiscountFactors,
        interpolation: InterpolationMethod::LogLinear,
        extrapolation: ExtrapolationMethod::None,
        dependencies: vec![],
        calibration: CalibrationMode::Direct,
        source: CurveSource::Nodes(vec![node(0.0, 1.0), node(1.0, 0.96), node(2.0, 0.92)]),
    };

    let built = build_curve_set(&[recipe], &CurveSet::new())?;
    let curve = built.curves.get(&key)?;
    let df = curve.query(&CurveQuery::DiscountFactor { point: CurvePoint::Tenor(1.5) })?;
    assert!(df.value > 0.92 && df.value < 0.96);
    Ok(())
}
```

## Links

- API docs: <https://docs.rs/convex-curves>
- Curve inputs and queries: <https://github.com/sujitn/convex/blob/main/docs/CURVE-CONTRACTS.md>
- Curve methods: <https://github.com/sujitn/convex/blob/main/docs/CURVE-METHODS.md>
- Ultimate forward rate: <https://github.com/sujitn/convex/blob/main/docs/CURVE-UFR.md>
- Repository: <https://github.com/sujitn/convex>

## License

MIT. See LICENSE.

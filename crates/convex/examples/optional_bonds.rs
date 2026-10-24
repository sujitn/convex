//! Pure CVX-059 quote calibration, callable price, OAS and fixed-model risk.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use convex::{
    bond_contracts::*, cashflows::*, curve_contracts::*, optional_bond::*,
    BusinessDayConvention as Bdc, CalendarId, Compounding, Currency, Date,
    DayCountConvention as Dc, ExtrapolationMethod as Ex, FixedRateBond, Frequency,
    InterpolationMethod as Int,
};
fn require(condition: bool, reason: &str) -> Result<(), Box<dyn std::error::Error>> {
    if condition {
        Ok(())
    } else {
        Err(OptionalBondError::Invalid {
            field: "example",
            reason: reason.into(),
        }
        .into())
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = Date::parse("2025-01-01")?;
    let exercise = Date::parse("2026-01-01")?;
    let maturity = Date::parse("2027-01-01")?;
    let calendar = CalendarId::weekend_only().try_to_calendar()?;
    let recipe = CurveRecipe {
        key: CurveKey {
            id: "supplied-discount".into(),
            version: "1".into(),
        },
        role: CurveRole::Discount {
            collateral_currency: Currency::USD,
        },
        currency: Currency::USD,
        reference_date: root,
        time_basis: Dc::Act365Fixed,
        representation: CurveRepresentation::ZeroRates {
            compounding: Compounding::Continuous,
        },
        interpolation: Int::Linear,
        extrapolation: Ex::Flat,
        dependencies: vec![],
        calibration: CalibrationMode::Direct,
        source: CurveSource::Nodes(vec![
            CurveNode {
                pillar: CurvePoint::Tenor(0.01),
                quote: MarketQuote {
                    value: 0.04,
                    unit: QuoteUnit::DecimalRate,
                },
            },
            CurveNode {
                pillar: CurvePoint::Tenor(5.0),
                quote: MarketQuote {
                    value: 0.04,
                    unit: QuoteUnit::DecimalRate,
                },
            },
        ]),
    };
    let curve_key = recipe.key.clone();
    let set = build_curve_set(&[recipe], &CurveSet::new())?;
    let discount = set.curves.get(&curve_key)?;
    let explicit = Hw1fParameters {
        mean_reversion: 0.03,
        volatility: 0.012,
    };
    let mut quote = SwaptionCalibrationQuote {
        id: "synthetic-model-implied-normal-vol".into(),
        exercise_date: exercise,
        fixed_schedule: ScheduleSpec {
            start_date: exercise,
            end_date: maturity,
            frequency: Frequency::Annual,
            stub_type: StubType::ShortLast,
            end_of_month: false,
            first_regular_date: None,
            penultimate_date: None,
        },
        dates: DateRules::default(),
        fixed_day_count: Dc::Act365Fixed,
        strike_rate: 0.04,
        side: OptionSide::Put,
        volatility: SwaptionVolatility::Normal {
            annualized_decimal: 0.01,
        },
        native_tolerance: 1e-8,
        scale: 0.01,
        weight: 1.0,
    };
    // A declared synthetic demonstration quote, not fabricated market data.
    let synthetic = reprice_swaption(&quote, root, discount, calendar.as_ref(), explicit)?;
    quote.volatility = SwaptionVolatility::Normal {
        annualized_decimal: synthetic.model_quote,
    };
    let model = calibrate_sigma(SigmaCalibrationInputs {
        model_key: CurveKey {
            id: "supplied-hw1f".into(),
            version: "synthetic-1".into(),
        },
        valuation_date: root,
        currency: Currency::USD,
        discount,
        calendar: calendar.as_ref(),
        mean_reversion: 0.03,
        quotes: &[quote],
        settings: SigmaCalibrationSettings {
            initial_sigma: 0.008,
            lower_sigma: 0.001,
            upper_sigma: 0.05,
            max_iterations: 100,
            objective_tolerance: 1e-6,
            jacobian_step: 1e-6,
            minimum_singular_ratio: 1e-10,
            svd_iterations: 1000,
        },
    })?;
    require(
        (model.parameters().volatility - explicit.volatility).abs() < 1e-7,
        "synthetic sigma recovery",
    )?;
    let base = FixedRateBond::builder()
        .cusip_unchecked("SUPPLIED-CVX059")
        .issue_date(root)
        .maturity(maturity)
        .coupon_rate("0.06".parse()?)
        .face_value(5000.into())
        .redemption_value(5000.into())
        .frequency(Frequency::Annual)
        .day_count(Dc::Act365Fixed)
        .calendar(CalendarId::weekend_only())
        .business_day_convention(Bdc::Unadjusted)
        .end_of_month(false)
        .build()?;
    let mut terms = base.contract_terms()?;
    terms.exercise = vec![ExerciseTerms {
        right: ExerciseRight::Call,
        style: ExerciseStyle::European { date: exercise },
        price_per_100: 101.into(),
        notice_offset: DateOffset::CalendarDays(0),
        settlement_lag: DateOffset::CalendarDays(0),
        settlement_adjustment: Bdc::Unadjusted,
        accrued: ExerciseAccrued::PayAccrued,
        protection_end: None,
    }];
    let bond = terms.build()?;
    let settings = OptionalBondSettings {
        grid: GridSettings {
            base_steps: 128,
            refinements: 3,
            absolute_price_tolerance: 0.004,
            relative_price_tolerance: 1e-8,
        },
        exercise_priority: ExercisePriority::IssuerLast,
    };
    let prepared = prepare_fixed(
        &bond,
        OptionalBondContext {
            settlement: root,
            on_payment: PaymentDateEntitlement::Exclude,
            exercise_state: ExerciseState::AliveUnexercised,
        },
        OptionalBondInputs {
            discount,
            projection: None,
            model: &model,
            exercise_calendar: calendar.as_ref(),
        },
        settings,
    )?;
    let target = prepared.price(0.00123456)?;
    let inverse = prepared.solve_oas(
        target.dirty_price,
        OasSolveSettings {
            lower: -0.05,
            upper: 0.10,
            price_tolerance: 1e-9,
            max_iterations: 128,
        },
    )?;
    let risk = prepared.risk(
        inverse.oas_decimal,
        RiskAxis::Discount,
        0.01,
        RiskSettings {
            absolute_duration_tolerance: 0.02,
            relative_duration_tolerance: 0.01,
            absolute_convexity_tolerance: 0.25,
            relative_convexity_tolerance: 0.01,
        },
    )?;
    require(
        (inverse.oas_decimal - 0.00123456).abs() < 1e-10,
        "fractional OAS recovery",
    )?;
    require(
        risk.up.model.parameters() == risk.down.model.parameters()
            && risk.up.oas_decimal == risk.down.oas_decimal,
        "risk holds model and OAS",
    )?;
    println!(
        "sigma={:.12} dirty={:.12} oas_bps={:.9} duration={:.12} grids={} certified=false",
        model.parameters().volatility,
        inverse.price.dirty_price,
        inverse.oas_bps,
        risk.duration,
        inverse.price.grids.len()
    );
    Ok(())
}

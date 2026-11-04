//! CVX-061 full native workflows, independent inverses and explicit failures.
#[path = "../../convex/examples/support/quote_fixture.rs"]
mod cash_fixture;
mod credit_support;
#[path = "support/futures.rs"]
mod futures_fixture;
#[path = "../../convex/examples/support/rate_fixture.rs"]
mod rate_fixture;
use convex_analytics::{quote_risk::*, rate_calibration::*, workflow::*};
use convex_bonds::contracts::*;
use convex_core::{
    cashflows::{CashRounding, PaymentDateEntitlement},
    contracts::*,
    daycounts::DayCountConvention,
    fixings::*,
    types::Compounding,
    Currency, Date,
};
use convex_curves::{contracts::*, ExtrapolationMethod, InterpolationMethod};
use rust_decimal::Decimal;
use serde_json::Value;
type Error = Box<dyn std::error::Error>;
fn settings(lower: f64, upper: f64) -> BondInverseSettings {
    BondInverseSettings {
        lower,
        upper,
        price_tolerance: 1e-9,
        parameter_tolerance: 1e-12,
        max_iterations: 256,
    }
}
fn convention() -> DatedYieldConvention {
    DatedYieldConvention {
        day_count: DayCountConvention::Act365Fixed,
        compounding: Compounding::SemiAnnual,
    }
}
fn close(a: f64, b: f64, t: f64) {
    assert!(
        a.is_finite() && b.is_finite() && (a - b).abs() <= t,
        "{a} != {b} bound{t}"
    );
}
fn fixed(f: &cash_fixture::Fixture) -> Result<BondContract, Error> {
    let CashHelperResources::Bond { calendars, .. } = f.resources() else {
        return Err("missing full bond calendars".into());
    };
    Ok(f.helper.terms.clone().build_with_calendars(&calendars)?)
}
use convex_analytics::cash_calibration::CashHelperResources;
fn discount() -> Result<(CurveSet, CurveKey), Error> {
    let key = CurveKey {
        id: "standalone.flat.original".into(),
        version: "1".into(),
    };
    let recipe = CurveRecipe {
        key: key.clone(),
        role: CurveRole::Discount {
            collateral_currency: Currency::USD,
        },
        currency: Currency::USD,
        reference_date: Date::parse("2026-10-07")?,
        time_basis: DayCountConvention::Act365Fixed,
        representation: CurveRepresentation::ZeroRates {
            compounding: Compounding::Continuous,
        },
        interpolation: InterpolationMethod::Linear,
        extrapolation: ExtrapolationMethod::None,
        dependencies: vec![],
        calibration: CalibrationMode::Direct,
        source: CurveSource::Nodes(vec![
            CurveNode {
                pillar: CurvePoint::Tenor(0.0),
                quote: MarketQuote {
                    value: 0.035,
                    unit: QuoteUnit::DecimalRate,
                },
            },
            CurveNode {
                pillar: CurvePoint::Tenor(5.0),
                quote: MarketQuote {
                    value: 0.035,
                    unit: QuoteUnit::DecimalRate,
                },
            },
        ]),
    };
    Ok((build_curve_set(&[recipe], &CurveSet::new())?.curves, key))
}
#[test]
fn actual_stepped_amortizing_yield_and_z_inverses_match_independent_decimal() -> Result<(), Error> {
    let f = cash_fixture::Fixture::load()?;
    let bond = fixed(&f)?;
    let cash = PreparedBondCash::fixed(&bond, f.helper.settlement, f.helper.on_payment)?;
    let (curves, key) = discount()?;
    let reference = curves.get(&key)?;
    let data: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/workflow/standalone.json"
    ))?;
    for row in data["cases"].as_array().ok_or("missing cases")? {
        let number = |field: &str| -> Result<f64, Error> {
            Ok(row[field]
                .as_str()
                .ok_or("missing independent number")?
                .parse()?)
        };
        let value = if row["kind"] == "yield" {
            cash.price_yield(number("rate")?, convention())?
        } else {
            cash.price_spread(reference, BondDiscountRole::Collateral, number("rate")?)?
        };
        close(value.dirty_per100, number("dirty")?, 1e-9);
        close(value.clean_per100, number("clean")?, 1e-9);
        close(value.accrued_per100, number("accrued")?, 1e-9);
        assert_eq!(value.cash.principal.len(), 2);
        for target in [
            BondPriceTarget::Clean(number("clean")?),
            BondPriceTarget::Dirty(number("dirty")?),
        ] {
            let result = if row["kind"] == "yield" {
                solve_bond_yield(&cash, convention(), target, settings(-0.9, 1.0))?
            } else {
                solve_bond_spread(
                    &cash,
                    reference,
                    BondDiscountRole::Collateral,
                    target,
                    settings(-0.1, 0.2),
                )?
            };
            close(result.decimal_parameter, number("inverse")?, 1e-10);
            assert!(result.native_residual.abs() <= result.settings.price_tolerance);
            assert!(result.price_evaluations >= 3);
        }
    }
    Ok(())
}
#[test]
fn inverse_no_root_non_unique_invalid_and_convergence_are_distinct() -> Result<(), Error> {
    let f = cash_fixture::Fixture::load()?;
    let bond = fixed(&f)?;
    let cash = PreparedBondCash::fixed(&bond, f.helper.settlement, f.helper.on_payment)?;
    assert!(matches!(
        solve_bond_yield(
            &cash,
            convention(),
            BondPriceTarget::Dirty(1e6),
            settings(0.0, 0.1)
        ),
        Err(WorkflowError::NoRoot { .. })
    ));
    let mut bounded = settings(-0.9, 1.0);
    bounded.max_iterations = 1;
    assert!(matches!(
        solve_bond_yield(&cash, convention(), BondPriceTarget::Dirty(97.0), bounded),
        Err(WorkflowError::NoConvergence { .. })
    ));
    let matured = PreparedBondCash::fixed(
        &bond,
        Date::parse("2030-01-01")?,
        PaymentDateEntitlement::Exclude,
    )?;
    assert!(matches!(
        solve_bond_yield(
            &matured,
            convention(),
            BondPriceTarget::Dirty(0.0),
            settings(0.0, 0.1)
        ),
        Err(WorkflowError::NonUnique(_))
    ));
    assert!(matches!(
        solve_bond_yield(
            &matured,
            convention(),
            BondPriceTarget::Dirty(1.0),
            settings(0.0, 0.1)
        ),
        Err(WorkflowError::NoRoot { .. })
    ));
    for mut s in [settings(-3.0, 0.1), settings(-0.9, 1.0)] {
        if s.lower > -1.0 {
            s.parameter_tolerance = 0.0;
        }
        assert!(!matches!(
            solve_bond_yield(&cash, convention(), BondPriceTarget::Dirty(100.0), s),
            Ok(_) | Err(WorkflowError::NoRoot { .. })
        ));
    }
    let mut terms = bond.terms().clone();
    for r in &mut terms.profiles.coupon {
        r.value = Decimal::new(-2, 1);
    }
    let CashHelperResources::Bond { calendars, .. } = f.resources() else {
        return Err("missing calendars".into());
    };
    let negative = terms.build_with_calendars(&calendars)?;
    let negative = PreparedBondCash::fixed(&negative, f.helper.settlement, f.helper.on_payment)?;
    assert!(matches!(
        solve_bond_yield(
            &negative,
            convention(),
            BondPriceTarget::Dirty(100.0),
            settings(0.0, 0.1)
        ),
        Err(WorkflowError::Unsupported(_))
    ));
    Ok(())
}
#[test]
fn cash_projection_has_no_discount_dependency_and_native_quantity_scales_once() -> Result<(), Error>
{
    let f = cash_fixture::Fixture::load()?;
    let bond = fixed(&f)?;
    let cash = PreparedBondCash::fixed(&bond, f.helper.settlement, f.helper.on_payment)?;
    let empty = CurveSet::new();
    let history = FixingSeries::default();
    let pubs = PublicationSchedule::new(vec![])?;
    let absent = CurveKey {
        id: "required.original".into(),
        version: "7".into(),
    };
    let input = ValuationInputs::Bond {
        cash: &cash,
        method: BondPriceMethod::DiscountSpread {
            curve: absent.clone(),
            role: BondDiscountRole::Collateral,
            decimal_spread: 0.0,
        },
        market: SuppliedMarket {
            curves: &empty,
            fixings: &history,
            publications: &pubs,
        },
    };
    assert!(matches!(cashflows(&input)?, ExpectedCashflows::Bond(_)));
    assert!(
        matches!(price(&input),Err(WorkflowError::MissingInput(v)) if v==vec![MissingInput::Curve(absent)])
    );
    let (curves, key) = discount()?;
    let input = ValuationInputs::Bond {
        cash: &cash,
        method: BondPriceMethod::DiscountSpread {
            curve: key,
            role: BondDiscountRole::Collateral,
            decimal_spread: 0.0,
        },
        market: SuppliedMarket {
            curves: &curves,
            fixings: &history,
            publications: &pubs,
        },
    };
    let position = cash_fixture::position("-10000", "5000", RiskQuantityUnit::Notional)?;
    let value = revalue_curves(&input, &curves, position.clone(), cash_fixture::snapshot()?)?;
    close(
        value.baseline_amount.value().get(),
        input.monetary_value()?.value().get() * -2.0,
        1e-9,
    );
    close(value.change.value().get(), 0.0, 0.0);
    let mut bad = position;
    bad.reference_quantity = ExactDecimal::parse("10000")?;
    assert!(revalue_curves(&input, &curves, bad, cash_fixture::snapshot()?).is_err());
    assert_eq!(
        capability(&input, WorkflowOperation::CdsConversion).status,
        WorkflowCapabilityStatus::NotApplicable
    );
    assert!(matches!(
        inverse(InverseRequest::SwapFairRate(&input)),
        Err(WorkflowError::NotApplicable(_))
    ));
    Ok(())
}
#[test]
fn native_irs_and_ois_uniform_fair_rate_roundtrip_full_terms_and_history() -> Result<(), Error> {
    let f = rate_fixture::Fixture::load("nonflat-positive")?;
    let built = f.fit(&f.recipes)?;
    let pubs = PublicationSchedule::new(built.publications.clone())?;
    let market = SuppliedMarket {
        curves: &built.curves,
        fixings: &f.history,
        publications: &pubs,
    };
    for recipe in &f.recipes {
        for h in &recipe.helpers {
            if let RateCalibrationTerms::Swap(t) = &h.terms {
                let calendars = f.swap_calendars(t);
                let ConstructedInstrument::Swap(contract) = construct(ConstructionInput::Swap {
                    terms: t,
                    calendars: &calendars,
                })?
                else {
                    return Err("native construction mismatch".into());
                };
                let input = ValuationInputs::Swap {
                    contract: &contract,
                    context: f.context()?.cashflows,
                    market,
                };
                let InverseValue::Swap(value) = inverse(InverseRequest::SwapFairRate(&input))?
                else {
                    return Err("native inverse mismatch".into());
                };
                let convex_rates::pricing::SwapParQuotes::Available { fixed_rate, .. } = value.par
                else {
                    return Err("no fair rate".into());
                };
                let mut new = t.as_ref().clone();
                for r in &mut new.fixed.rates {
                    r.value = Decimal::try_from(fixed_rate.value().get())?;
                }
                let new_contract = new.clone().build(&f.swap_calendars(&new))?;
                let par_input = ValuationInputs::Swap {
                    contract: &new_contract,
                    context: f.context()?.cashflows,
                    market,
                };
                close(par_input.monetary_value()?.value().get(), 0.0, 1e-6);
                let ExpectedCashflows::Swap(cash) = cashflows(&input)? else {
                    return Err("missing native cash".into());
                };
                assert_eq!(cash.rates, value.cashflows.rates);
                let r = swap_requirements(&contract, f.context()?.cashflows, &market)?;
                assert!(r.missing.is_empty());
                assert!(!r.projected.is_empty());
                let mut rounded = t.as_ref().clone();
                rounded.fixed.leg.rounding = CashRounding::NearestEven(2);
                let rounded = rounded.clone().build(&f.swap_calendars(&rounded))?;
                let bad = ValuationInputs::Swap {
                    contract: &rounded,
                    context: f.context()?.cashflows,
                    market,
                };
                assert!(matches!(
                    inverse(InverseRequest::SwapFairRate(&bad)),
                    Err(WorkflowError::Unsupported(_))
                ));
            }
        }
    }
    Ok(())
}
#[test]
fn missing_publication_and_due_history_remain_distinct_from_permitted_forecasts(
) -> Result<(), Error> {
    let f = rate_fixture::Fixture::load("nonflat-positive")?;
    let built = f.fit(&f.recipes)?;
    let t = f
        .recipes
        .iter()
        .flat_map(|r| &r.helpers)
        .find_map(|h| {
            if let RateCalibrationTerms::Swap(t) = &h.terms {
                Some(t)
            } else {
                None
            }
        })
        .ok_or("missing supplied swap")?;
    let contract = t.as_ref().clone().build(&f.swap_calendars(t))?;
    let empty_pubs = PublicationSchedule::new(vec![])?;
    let market = SuppliedMarket {
        curves: &built.curves,
        fixings: &f.history,
        publications: &empty_pubs,
    };
    assert!(
        matches!(swap_requirements(&contract,f.context()?.cashflows,&market),Err(WorkflowError::MissingInput(v)) if matches!(&v[0],MissingInput::Publication(..)))
    );
    let pubs = PublicationSchedule::new(built.publications.clone())?;
    let history = FixingSeries::default();
    let market = SuppliedMarket {
        curves: &built.curves,
        fixings: &history,
        publications: &pubs,
    };
    let r = swap_requirements(&contract, f.context()?.cashflows, &market)?;
    assert!(!r.missing.is_empty());
    assert!(!r.projected.is_empty());
    let input = ValuationInputs::Swap {
        contract: &contract,
        context: f.context()?.cashflows,
        market,
    };
    assert!(
        matches!(price(&input),Err(WorkflowError::MissingInput(v)) if v.iter().all(|m|matches!(m,MissingInput::Fixing(_))))
    );
    assert!(PublicationSchedule::new(vec![r.required[0].clone(), r.required[0].clone()]).is_err());
    Ok(())
}
#[test]
fn full_standard_running_clean_dirty_upfront_conversion_roundtrips_and_no_root() -> Result<(), Error>
{
    let data = credit_support::inputs()?;
    let calendar = credit_support::calendar(&data);
    let terms = &data.template;
    let bindings = credit_support::binding(terms, &calendar);
    let ConstructedInstrument::Cds(contract) = construct(ConstructionInput::Cds {
        terms,
        calendars: &bindings,
    })?
    else {
        return Err("native mismatch".into());
    };
    let curves = credit_support::supplied(&data)?;
    let context = contract
        .terms()
        .trade_context(convex_credit::InitialPaymentState::NotContracted);
    let request = |quote| {
        inverse(InverseRequest::Cds {
            contract: &contract,
            context: &context,
            curves: &curves,
            quote,
            settings: convex_credit::ConversionSettings::default(),
        })
    };
    let InverseValue::Cds(base) = request(convex_credit::CdsQuote::RunningSpread {
        decimal_rate: 0.01234567,
    })?
    else {
        return Err("inverse mismatch".into());
    };
    for quote in [
        convex_credit::CdsQuote::CleanUpfront {
            fraction: base.valuation.clean_upfront_fraction,
        },
        convex_credit::CdsQuote::DirtyUpfront {
            fraction: base.valuation.dirty_upfront_fraction,
        },
    ] {
        let InverseValue::Cds(result) = request(quote)? else {
            return Err("inverse mismatch".into());
        };
        close(
            result.conventional_spread_decimal,
            base.conventional_spread_decimal,
            1e-10,
        );
        close(
            result.hazard_per_act365f_year,
            base.hazard_per_act365f_year,
            1e-10,
        );
        assert!(result.native_quote_residual.abs() <= result.settings.native_quote_tolerance);
    }
    assert!(matches!(
        request(convex_credit::CdsQuote::CleanUpfront { fraction: 10.0 }),
        Err(WorkflowError::NoRoot { .. })
    ));
    let absent = CurveSet::new();
    assert!(matches!(
        inverse(InverseRequest::Cds {
            contract: &contract,
            context: &context,
            curves: &absent,
            quote: convex_credit::CdsQuote::RunningSpread { decimal_rate: 0.01 },
            settings: convex_credit::ConversionSettings::default()
        }),
        Err(WorkflowError::MissingInput(_))
    ));
    Ok(())
}
#[test]
fn futures_keep_mark_level_meaning_full_fixed_non_identification_and_native_guards(
) -> Result<(), Error> {
    let data = futures_fixture::fixture();
    for case in data["rates"]["cases"]
        .as_array()
        .ok_or("cases")?
        .iter()
        .filter(|r| r["averaging"] == "compounded")
    {
        let pack = futures_fixture::RatePack::new(case);
        let native = pack.inputs();
        let requirements =
            convex_analytics::futures::rate_futures_requirements(&native, |i, d| {
                pack.publication(i, d)
            })?;
        let publications = PublicationSchedule::new(requirements)?;
        let input = ValuationInputs::RateFuture {
            inputs: &native,
            publications: &publications,
        };
        let Valuation::RateFuture(value) = price(&input)? else {
            return Err("mismatch".into());
        };
        assert_eq!(input.basis(), ValuationBasis::FuturesQuoteLevel);
        assert!(matches!(
            cashflows(&input),
            Err(WorkflowError::NotApplicable(_))
        ));
        let position = cash_fixture::position("3", "1", RiskQuantityUnit::Contracts)?;
        input.validate_position(&position)?;
        let r = inverse(InverseRequest::RateFuture {
            contract: &pack.contract,
            valuation: &value,
            price_points: value.model_price_points,
        });
        if value.projected_days == 0 {
            assert!(matches!(r, Err(WorkflowError::NonUnique(_))));
        } else {
            assert!(matches!(r?, InverseValue::RemainingFutureRate(_)));
        }
        let mut corrupt = value.as_ref().clone();
        corrupt.known_days += 1;
        assert!(inverse(InverseRequest::RateFuture {
            contract: &pack.contract,
            valuation: &corrupt,
            price_points: value.model_price_points
        })
        .is_err());
    }
    let pack = futures_fixture::BondPack::new();
    let deliverables = pack.deliverables();
    let native = pack.inputs(&deliverables);
    let input = ValuationInputs::BondFuture(&native);
    let Valuation::BondFuture(value) = price(&input)? else {
        return Err("mismatch".into());
    };
    assert_eq!(value.alternatives.len(), 10);
    assert_eq!(input.basis(), ValuationBasis::FuturesQuoteLevel);
    assert_eq!(
        capability(&input, WorkflowOperation::CurveQuoteRisk).status,
        WorkflowCapabilityStatus::Unsupported
    );
    Ok(())
}

#[test]
fn floating_revaluation_reprojects_future_cash_and_discount_margin_has_full_cash_inverse(
) -> Result<(), Error> {
    use convex_core::cashflows::{CouponPrincipal, DateOffset};
    let f = cash_fixture::Fixture::load()?;
    let mut terms = f.helper.terms.clone();
    terms.profiles = BondProfiles::constant(
        terms.schedule.start_date,
        terms.schedule.end_date,
        Decimal::ZERO,
        Decimal::new(2, 3),
        Decimal::ONE,
        terms.face_value,
    );
    terms.coupon_principal = CouponPrincipal::AccrualProfile;
    terms.coupon = BondCoupon::Term(TermRateTerms {
        index_id: IndexId::new("USD.Supplied.Term")?,
        resets: terms.schedule.clone(),
        reset_adjustment: convex_core::calendars::BusinessDayConvention::Unadjusted,
        fixing_offset: DateOffset::CalendarDays(0),
        projection_day_count: DayCountConvention::Act360,
    });
    let CashHelperResources::Bond { calendars, .. } = f.resources() else {
        return Err("calendars".into());
    };
    let bond = terms.clone().build_with_calendars(&calendars)?;
    let context = BondProjectionContext {
        settlement: terms.schedule.start_date,
        on_payment: PaymentDateEntitlement::Exclude,
        fixing: FixingContext {
            valuation_date: terms.schedule.start_date.checked_add_days(-1)?,
            known_at: UtcTimestamp::parse(&format!(
                "{}T18:00:00Z",
                terms.schedule.start_date.checked_add_days(-1)?
            ))?,
            unpublished: UnpublishedFixingPolicy::Forecast,
            final_only: true,
        },
        overnight_accrual_cutoffs: vec![],
    };
    let requirements =
        bond.fixing_requirements(context.settlement, context.on_payment, |_, date| {
            UtcTimestamp::parse(&format!("{date}T16:00:00Z"))
                .map_err(|e| convex_core::cashflows::CashflowError::Date(e.to_string()))
        })?;
    let pubs = PublicationSchedule::new(requirements)?;
    let history = FixingSeries::default();
    let mut d = futures_fixture::curve(
        CurveRole::Discount {
            collateral_currency: Currency::USD,
        },
        0.03,
        context.settlement,
    )
    .recipe()
    .clone();
    d.key = CurveKey {
        id: "floating.discount".into(),
        version: "1".into(),
    };
    let mut p = d.clone();
    p.key = CurveKey {
        id: "floating.projection".into(),
        version: "1".into(),
    };
    p.role = CurveRole::Projection {
        index_id: "USD.Supplied.Term".into(),
    };
    let original = build_curve_set(&[d.clone(), p.clone()], &CurveSet::new())?.curves;
    let market = SuppliedMarket {
        curves: &original,
        fixings: &history,
        publications: &pubs,
    };
    let prepared = PreparedBondCash::floating(&bond, &context, &market, Some(&p.key))?;
    assert!(matches!(
        prepared.price_yield(0.04, convention()),
        Err(WorkflowError::NotApplicable(_))
    ));
    let target = prepared.price_spread(
        original.get(&d.key)?,
        BondDiscountRole::Collateral,
        0.004321,
    )?;
    let result = solve_bond_spread(
        &prepared,
        original.get(&d.key)?,
        BondDiscountRole::Collateral,
        BondPriceTarget::Dirty(target.dirty_per100),
        settings(-0.1, 0.2),
    )?;
    close(result.decimal_parameter, 0.004321, 1e-10);
    assert!(
        matches!(PreparedBondCash::floating(&bond,&context,&market,None),Err(WorkflowError::MissingInput(v)) if v==vec![MissingInput::Projection])
    );
    let mut changed = p.clone();
    let CurveSource::Nodes(nodes) = &mut changed.source else {
        return Err("nodes".into());
    };
    for node in nodes {
        node.quote.value += 0.01;
    }
    let bumped = build_curve_set(&[d.clone(), changed], &CurveSet::new())?.curves;
    let input = ValuationInputs::FloatingBond {
        contract: &bond,
        context: &context,
        market,
        projection: Some(p.key),
        discount: d.key,
        discount_margin: 0.0,
    };
    let value = revalue_curves(
        &input,
        &bumped,
        cash_fixture::position("5000", "5000", RiskQuantityUnit::Notional)?,
        cash_fixture::snapshot()?,
    )?;
    assert!(value.change.value().get() > 0.0);
    let (Valuation::Bond(a), Valuation::Bond(b)) = (&value.baseline, &value.shocked) else {
        return Err("bond dispatch".into());
    };
    assert!(b.cash.coupons[0].amount > a.cash.coupons[0].amount);
    Ok(())
}

#[test]
fn optional_price_oas_and_scenario_use_original_grid_and_risk_guards() -> Result<(), Error> {
    use convex_analytics::optional_bond::*;
    use convex_core::{
        calendars::{BusinessDayConvention, WeekendCalendar},
        cashflows::DateOffset,
        types::Frequency,
    };
    let settle = Date::parse("2025-01-01")?;
    let mut terms = convex_bonds::instruments::FixedRateBond::builder()
        .cusip_unchecked("SUPPLIED-CVX061-OPTION")
        .issue_date(settle)
        .maturity(Date::parse("2028-01-01")?)
        .coupon_rate(Decimal::new(6, 2))
        .frequency(Frequency::Annual)
        .day_count(DayCountConvention::Act365Fixed)
        .calendar(convex_bonds::types::CalendarId::weekend_only())
        .business_day_convention(BusinessDayConvention::Unadjusted)
        .face_value(Decimal::ONE_HUNDRED)
        .redemption_value(Decimal::ONE_HUNDRED)
        .end_of_month(false)
        .build()?
        .contract_terms()?;
    terms.exercise = vec![ExerciseTerms {
        right: ExerciseRight::Call,
        style: ExerciseStyle::Bermudan {
            dates: vec![Date::parse("2026-01-01")?, Date::parse("2027-01-01")?],
        },
        price_per_100: Decimal::from(101),
        notice_offset: DateOffset::CalendarDays(0),
        settlement_lag: DateOffset::CalendarDays(0),
        settlement_adjustment: BusinessDayConvention::Unadjusted,
        accrued: ExerciseAccrued::PayAccrued,
        protection_end: None,
    }];
    let bond = terms.build()?;
    let discount = futures_fixture::curve(
        CurveRole::Discount {
            collateral_currency: Currency::USD,
        },
        0.04,
        settle,
    );
    let model = OptionalBondModel::assumed(
        CurveKey {
            id: "HW1F.CVX061".into(),
            version: "original-v1".into(),
        },
        Hw1fParameters {
            mean_reversion: 0.03,
            volatility: 0.01,
        },
        "original supplied synthetic assumption".into(),
    )?;
    let prepared = prepare_fixed(
        &bond,
        OptionalBondContext {
            settlement: settle,
            on_payment: PaymentDateEntitlement::Exclude,
            exercise_state: ExerciseState::AliveUnexercised,
        },
        OptionalBondInputs {
            discount: &discount,
            projection: None,
            model: &model,
            exercise_calendar: &WeekendCalendar,
        },
        OptionalBondSettings {
            grid: GridSettings {
                base_steps: 128,
                refinements: 3,
                absolute_price_tolerance: 0.004,
                relative_price_tolerance: 1e-8,
            },
            exercise_priority: ExercisePriority::IssuerLast,
        },
    )?;
    let input = ValuationInputs::OptionalBond {
        prepared: &prepared,
        oas_decimal: 0.00123456,
    };
    let Valuation::Optional(value) = price(&input)? else {
        return Err("optional dispatch".into());
    };
    assert_eq!(value.model.parameters(), model.parameters());
    let InverseValue::Optional(inverted) = inverse(InverseRequest::OptionalOas {
        prepared: &prepared,
        target_dirty: value.dirty_price,
        settings: OasSolveSettings {
            lower: -0.1,
            upper: 0.1,
            price_tolerance: 1e-9,
            max_iterations: 256,
        },
    })?
    else {
        return Err("inverse dispatch".into());
    };
    close(inverted.oas_decimal, 0.00123456, 1e-10);
    assert!(inverted.residual_per_100.abs() <= 1e-9);
    let scenario = revalue_optional(
        &prepared,
        0.00123456,
        CurveShift {
            discount: 0.001,
            projection: 0.0,
        },
        cash_fixture::position("-2", "1", RiskQuantityUnit::TradeCopies)?,
        cash_fixture::snapshot()?,
    )?;
    assert!(scenario.change.value().get() > 0.0);
    let native = convex_analytics::optional_bond::RiskSettings {
        absolute_duration_tolerance: 0.02,
        relative_duration_tolerance: 0.01,
        absolute_convexity_tolerance: 0.25,
        relative_convexity_tolerance: 0.01,
    };
    assert!(prepared
        .risk(0.00123456, RiskAxis::Oas, 0.0001, native)
        .is_err());
    assert!(matches!(
        revalue_curves(
            &input,
            &CurveSet::new(),
            cash_fixture::position("1", "1", RiskQuantityUnit::TradeCopies)?,
            cash_fixture::snapshot()?
        ),
        Err(WorkflowError::Unsupported(_))
    ));
    Ok(())
}

#[test]
fn graph_risk_checks_original_information_set_and_retains_missing_or_inapplicable(
) -> Result<(), Error> {
    let f = cash_fixture::Fixture::load()?;
    let bond = fixed(&f)?;
    let cash = PreparedBondCash::fixed(&bond, f.helper.settlement, f.helper.on_payment)?;
    let (curves, curve) = discount()?;
    let history = FixingSeries::default();
    let publications = PublicationSchedule::new(vec![])?;
    let input = ValuationInputs::Bond {
        cash: &cash,
        method: BondPriceMethod::DiscountSpread {
            curve: curve.clone(),
            role: BondDiscountRole::Collateral,
            decimal_spread: 0.001,
        },
        market: SuppliedMarket {
            curves: &curves,
            fixings: &history,
            publications: &publications,
        },
    };
    let graph = prepare_quote_risk_graph(
        QuoteRiskGraphInput {
            rates: &[],
            credit: &[],
            cash: &[],
            supplied: &curves,
            history: &history,
            rate_context: None,
            controls: FitNumericalControls::default(),
        },
        publications.callback(),
    )?;
    let key = cash_fixture::key(
        QuoteType::ZeroRate,
        RiskCoordinate::Zero {
            curve: curve.clone(),
            node: None,
        },
        QuoteUnit::DecimalRate,
        RiskResponse::FrozenCurves,
        "parallel",
    )?;
    let quantity = cash_fixture::position("5000", "5000", RiskQuantityUnit::Notional)?;
    let result = curve_quote_risk(
        &input,
        &graph,
        key.clone(),
        quantity.clone(),
        cash_fixture::snapshot()?,
        cash_fixture::settings(0.0001)?,
    )?;
    assert!(result.value < 0.0);
    let scenario = evaluate_quote_graph(&graph, &key, 0.01)?;
    let mismatched = prepare_quote_risk_graph(
        QuoteRiskGraphInput {
            rates: &[],
            credit: &[],
            cash: &[],
            supplied: &scenario.curves,
            history: &history,
            rate_context: None,
            controls: FitNumericalControls::default(),
        },
        publications.callback(),
    )?;
    assert!(matches!(
        curve_quote_risk(
            &input,
            &mismatched,
            key.clone(),
            quantity.clone(),
            cash_fixture::snapshot()?,
            cash_fixture::settings(0.0001)?
        ),
        Err(WorkflowError::Invalid {
            field: "risk baseline",
            ..
        })
    ));
    let direct = ValuationInputs::Bond {
        cash: &cash,
        method: BondPriceMethod::Yield {
            decimal_rate: 0.031,
            convention: convention(),
        },
        market: SuppliedMarket {
            curves: &curves,
            fixings: &history,
            publications: &publications,
        },
    };
    assert!(matches!(
        curve_quote_risk(
            &direct,
            &graph,
            key,
            quantity,
            cash_fixture::snapshot()?,
            cash_fixture::settings(0.0001)?
        ),
        Err(WorkflowError::NotApplicable(_))
    ));
    let absent = CurveSet::new();
    let input = ValuationInputs::Bond {
        cash: &cash,
        method: BondPriceMethod::DiscountSpread {
            curve,
            role: BondDiscountRole::Collateral,
            decimal_spread: 0.0,
        },
        market: SuppliedMarket {
            curves: &absent,
            fixings: &history,
            publications: &publications,
        },
    };
    assert!(matches!(price(&input), Err(WorkflowError::MissingInput(_))));
    Ok(())
}

#[test]
fn inverse_admits_original_price_tolerance_at_a_closed_endpoint_and_settings_json_is_strict(
) -> Result<(), Error> {
    let f = cash_fixture::Fixture::load()?;
    let bond = fixed(&f)?;
    let cash = PreparedBondCash::fixed(&bond, f.helper.settlement, f.helper.on_payment)?;
    let boundary = cash.price_yield(0.0, convention())?.dirty_per100;
    let result = solve_bond_yield(
        &cash,
        convention(),
        BondPriceTarget::Dirty(boundary + 2e-10),
        settings(0.0, 0.1),
    )?;
    assert_eq!(result.decimal_parameter, 0.0);
    assert_eq!(result.iterations, 0);
    assert!(result.native_residual.abs() <= 1e-9);
    assert_eq!(result.price_evaluations, 3);
    let mut data = serde_json::to_value(settings(-0.1, 0.2))?;
    data["ignored_model_parameter"] = true.into();
    assert!(serde_json::from_value::<BondInverseSettings>(data).is_err());
    Ok(())
}

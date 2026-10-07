//! Full supplied quotes -> native curves -> mixed prices/risk -> indicative hedge.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]
#[path = "support/cash_fixture.rs"]
mod cash_curves;
#[path = "support/quote_fixture.rs"]
mod cash_fixture;
#[path = "support/rate_fixture.rs"]
mod rate_fixture;
use convex::{
    contracts::*, credit::*, credit_calibration::*, curve_contracts::*, futures::*,
    portfolio::signed_risk::*, quote_risk::*, rate_calibration::*, workflow::*, Compounding,
    Currency, Date, DayCountConvention, ExtrapolationMethod, InterpolationMethod,
};
use convex_core::{
    calendars::{DynamicCalendar, WeekendType},
    futures::EligibilityContext,
};
use serde_json::{json, Value};
type Error = Box<dyn std::error::Error>;
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, Error> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("missing supplied {key}").into())
}
fn credit_recipe(data: &Value) -> Result<(CurveRecipe, CdsCurveRecipe), Error> {
    let template: CdsTerms = serde_json::from_value(data["cds_template"].clone())?;
    let reference = Date::parse(text(&data["cds_discount"], "reference_date")?)?;
    let rate: f64 = text(&data["cds_discount"], "continuous_rate")?.parse()?;
    let tenor: f64 = text(&data["cds_discount"], "final_tenor")?.parse()?;
    let discount = CurveRecipe {
        key: template.discount_curve.clone(),
        role: CurveRole::Discount {
            collateral_currency: template.currency,
        },
        currency: template.currency,
        reference_date: reference,
        time_basis: DayCountConvention::Act365Fixed,
        representation: CurveRepresentation::DiscountFactors,
        interpolation: InterpolationMethod::LogLinear,
        extrapolation: ExtrapolationMethod::None,
        dependencies: vec![],
        calibration: CalibrationMode::Direct,
        source: CurveSource::Nodes(vec![
            CurveNode {
                pillar: CurvePoint::Tenor(0.0),
                quote: MarketQuote {
                    value: 1.0,
                    unit: QuoteUnit::DiscountFactor,
                },
            },
            CurveNode {
                pillar: CurvePoint::Tenor(tenor),
                quote: MarketQuote {
                    value: (-rate * tenor).exp(),
                    unit: QuoteUnit::DiscountFactor,
                },
            },
        ]),
    };
    let mut helpers = Vec::new();
    let mut pillars = Vec::new();
    for (i, row) in data["cds_quotes"]
        .as_array()
        .ok_or("missing full CDS quotes")?
        .iter()
        .enumerate()
    {
        let mut terms = template.clone();
        let date = Date::parse(text(row, "maturity")?)?;
        terms.premium_schedule.end_date = date;
        let rate: f64 = text(row, "decimal_spread")?.parse()?;
        pillars.push(date);
        helpers.push(CdsCalibrationHelper {
            id: format!("standalone.cds.quote.{i}"),
            quote_version: Version::new(1)?,
            pillar: date,
            context: terms.trade_context(InitialPaymentState::NotContracted),
            terms,
            quote_kind: CdsQuoteKind::RunningSpread,
            quote: MarketQuote {
                value: rate,
                unit: QuoteUnit::DecimalRate,
            },
            tolerance: 1e-11,
        });
    }
    let mut credit = discount.clone();
    credit.key = template.survival_curve.clone();
    credit.role = CurveRole::Credit {
        entity_id: template.reference.entity_id.as_str().into(),
        seniority: template.reference.seniority.as_str().into(),
        recovery: template.reference.recovery_fraction.get(),
    };
    credit.representation = CurveRepresentation::SurvivalProbabilities;
    credit.dependencies = vec![template.discount_curve.clone()];
    credit.calibration = CalibrationMode::Piecewise;
    credit.source = CurveSource::Helpers(helpers.iter().map(|h| h.id.clone()).collect());
    Ok((
        discount,
        CdsCurveRecipe {
            curve: credit,
            helpers,
            pillars,
            bootstrap: Some(CdsBootstrapSettings {
                lower_hazard: 0.0,
                upper_hazard: 1.0,
                max_iterations: 100,
            }),
        },
    ))
}
fn credit_calendars<'a>(terms: &'a CdsTerms, calendar: &'a DynamicCalendar) -> CdsCalendars<'a> {
    CdsCalendars {
        premium: CalendarBinding {
            pin: &terms.premium_calendar,
            calendar,
        },
        settlement: CalendarBinding {
            pin: &terms.settlement_calendar,
            calendar,
        },
    }
}
fn factor(
    q: QuoteType,
    curve: &CurveKey,
    helper: &str,
    version: Version,
    unit: QuoteUnit,
    reference: Date,
    pillar: Date,
) -> Result<QuoteRiskKey, Error> {
    Ok(QuoteRiskKey::new(QuoteRiskDefinition {
        factor: RiskFactorKey::new(FactorDefinition {
            family: if q == QuoteType::CdsSpread {
                FactorFamily::CreditSpread
            } else {
                FactorFamily::InterestRate
            },
            target: FactorTarget::Curve {
                curve_id: CurveId::new(&curve.id)?,
            },
            currency: Currency::USD,
            tenor: FactorTenor::new(format!("{}D", reference.days_between(&pillar)))?,
            quote_type: q,
            basis: RiskBasis::MarketQuoteRecalibrated,
            convention_version: Version::new(1)?,
            model_version: ModelId::new("standalone.synthetic.CVX061.full-native.v1")?,
        })?,
        coordinate: RiskCoordinate::Quote {
            curve: curve.clone(),
            helper_id: helper.into(),
            quote_version: version,
        },
        native_unit: unit,
        response: RiskResponse::RebuildDependents,
        unit: QuoteRiskUnit::CurrencyPerBp,
    })?)
}
fn native_position(
    quantity: &str,
    reference: &str,
    unit: RiskQuantityUnit,
) -> Result<RiskPosition, Error> {
    Ok(RiskPosition {
        quantity: ExactDecimal::parse(quantity)?,
        reference_quantity: ExactDecimal::parse(reference)?,
        quantity_unit: unit,
        currency: Currency::USD,
    })
}
/// Execute the complete public workflow. Every value is explicitly synthetic
/// or the retained native schema/fixture input; no repository or server exists.
pub fn run() -> Result<Value, Error> {
    let data: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/workflow/mixed-book.json"
    ))?;
    let rates = rate_fixture::Fixture::load("nonflat-positive")?;
    let cash = cash_fixture::Fixture::load()?;
    let cash_curves = cash_curves::Fixture::load("positive")?;
    let (credit_discount, credit) = credit_recipe(&data)?;
    let mut direct = cash_curves.direct_funding.clone();
    direct.push(credit_discount);
    let supplied = build_curve_set(&direct, &CurveSet::new())?.curves;
    let resources = rates.resources(&rates.recipes);
    let versions: Vec<_> = rates
        .recipes
        .iter()
        .map(|r| {
            r.helpers
                .iter()
                .map(|_| Version::new(1))
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<_, _>>()?;
    let rate_inputs: Vec<_> = rates
        .recipes
        .iter()
        .zip(&resources)
        .zip(&versions)
        .map(|((recipe, resources), quote_versions)| RateRiskCurveInput {
            build: RateCurveBuildInput { recipe, resources },
            quote_versions,
        })
        .collect();
    let cash_resources = cash_curves.resources(&cash_curves.recipes);
    let cash_inputs: Vec<_> = cash_curves
        .recipes
        .iter()
        .zip(&cash_resources)
        .map(
            |(recipe, resources)| convex::cash_calibration::CashCurveBuildInput {
                recipe,
                resources,
            },
        )
        .collect();
    let calendar = DynamicCalendar::from_dates(
        "explicit standalone synthetic CDS",
        WeekendType::SaturdaySunday,
        serde_json::from_value::<Vec<Date>>(rates.data["holidays"].clone())?,
    );
    let bindings: Vec<_> = credit
        .helpers
        .iter()
        .map(|h| credit_calendars(&h.terms, &calendar))
        .collect();
    let credit_inputs = [CdsCurveBuildInput {
        recipe: &credit,
        calendars: &bindings,
    }];
    let graph = prepare_quote_risk_graph(
        QuoteRiskGraphInput {
            rates: &rate_inputs,
            credit: &credit_inputs,
            cash: &cash_inputs,
            supplied: &supplied,
            history: &rates.history,
            rate_context: Some(rates.context()?),
            controls: FitNumericalControls::default(),
        },
        |i, d| rates.publication(i, d),
    )?;
    let mut publication_entries = std::collections::BTreeMap::new();
    for entry in &graph.baseline().publications {
        if let Some(previous) = publication_entries.insert(
            (entry.index_id.clone(), entry.observation_date),
            entry.clone(),
        ) {
            if previous != *entry {
                return Err("conflicting native administrator publication decisions".into());
            }
        }
    }
    let publications = PublicationSchedule::new(publication_entries.into_values().collect())?;
    let market = SuppliedMarket {
        curves: &graph.baseline().curves,
        fixings: &rates.history,
        publications: &publications,
    };
    let convex::cash_calibration::CashHelperResources::Bond { calendars, .. } = cash.resources()
    else {
        return Err("full cash calendars missing".into());
    };
    let constructed = construct(ConstructionInput::Bond {
        terms: &cash.helper.terms,
        calendars: &calendars,
    })?;
    let ConstructedInstrument::Bond(bond) = constructed else {
        return Err("native bond mismatch".into());
    };
    let bond = PreparedBondCash::fixed(&bond, cash.helper.settlement, cash.helper.on_payment)?;
    let mut swaps = Vec::new();
    for recipe in &rates.recipes {
        for h in &recipe.helpers {
            if let RateCalibrationTerms::Swap(t) = &h.terms {
                let mut t = t.as_ref().clone();
                let shift = if matches!(t.floating, convex::rates::FloatingLegTerms::Term(_)) {
                    text(&data, "irs_fixed_rate_shift")?
                } else {
                    text(&data, "ois_fixed_rate_shift")?
                };
                let shift = ExactDecimal::parse(shift)?.decimal();
                for r in &mut t.fixed.rates {
                    r.value = r.value.checked_add(shift).ok_or("rate profile overflow")?;
                }
                let c = t.clone().build(&rates.swap_calendars(&t))?;
                swaps.push(c);
            }
        }
    }
    if swaps.len() != 2 {
        return Err("complete IRS/OIS native examples required".into());
    }
    let cds_terms: CdsTerms = serde_json::from_value(data["cds_template"].clone())?;
    let cds = cds_terms.build(&credit_calendars(&cds_terms, &calendar))?;
    let cds_context = cds
        .terms()
        .trade_context(InitialPaymentState::NotContracted);
    let future = rates
        .recipes
        .iter()
        .flat_map(|r| &r.helpers)
        .find_map(|h| {
            if let RateCalibrationTerms::RateFuture(t) = &h.terms {
                Some(t.as_ref())
            } else {
                None
            }
        })
        .ok_or("full rate future missing")?;
    let future_inputs = RateFuturesInputs {
        contract: &future.contract,
        calendar: FuturesCalendar {
            calendar: &rates.calendar,
            stamp: &rates.calendar_stamp,
        },
        clearing: &future.clearing,
        fixings: &rates.futures_history,
        fixing_stamp: &future.fixing_stamp,
        projection: Some(FuturesCurve {
            curve: market.curves.get(&future.projection_curve)?,
            stamp: &future.projection_stamp,
        }),
        convexity_model: future.convexity_model.as_ref(),
        context: EligibilityContext {
            as_of: rates.context()?.as_of,
            known_at: rates.context()?.cashflows.fixings.known_at,
        },
        unpublished: rates.context()?.cashflows.fixings.unpublished,
        final_only: true,
    };
    let inputs = [
        ValuationInputs::Bond {
            cash: &bond,
            method: BondPriceMethod::DiscountSpread {
                curve: rates.recipes[0].curve.key.clone(),
                role: BondDiscountRole::Collateral,
                decimal_spread: text(&data, "bond_spread_decimal")?.parse()?,
            },
            market,
        },
        ValuationInputs::Swap {
            contract: &swaps[0],
            context: rates.context()?.cashflows,
            market,
        },
        ValuationInputs::Swap {
            contract: &swaps[1],
            context: rates.context()?.cashflows,
            market,
        },
        ValuationInputs::Cds {
            contract: &cds,
            context: &cds_context,
            curves: market.curves,
        },
        ValuationInputs::RateFuture {
            inputs: &future_inputs,
            publications: &publications,
        },
    ];
    let risk_helper = rates.recipes[0]
        .helpers
        .iter()
        .find(|h| matches!(h.terms, RateCalibrationTerms::RateFuture(_)))
        .ok_or("complete futures quote missing")?;
    let ir = factor(
        QuoteType::RateFutureRate,
        &rates.recipes[0].curve.key,
        &risk_helper.id,
        Version::new(1)?,
        risk_helper.quote.unit,
        rates.recipes[0].curve.reference_date,
        risk_helper.pillar,
    )?;
    let last = credit.helpers.last().ok_or("missing CDS helper")?;
    let cs = factor(
        QuoteType::CdsSpread,
        &credit.curve.key,
        &last.id,
        last.quote_version,
        last.quote.unit,
        credit.curve.reference_date,
        last.pillar,
    )?;
    let factors = [ir.clone(), cs];
    let snapshot = SnapshotId::new(text(&data, "snapshot_id")?)?;
    let entity = EntityId::new(text(&data, "legal_entity")?)?;
    let settings = QuoteRiskSettings {
        bump: text(&data, "risk_bump")?.parse()?,
        stability: RiskTolerance {
            absolute: text(&data, "risk_stability_absolute")?.parse()?,
            relative: text(&data, "risk_stability_relative")?.parse()?,
        },
        policy_version: Version::new(1)?,
    };
    let positions = [
        native_position("10000", "5000", RiskQuantityUnit::Notional)?,
        native_position("1", "1", RiskQuantityUnit::TradeCopies)?,
        native_position("-2", "1", RiskQuantityUnit::TradeCopies)?,
        native_position("1000000", "1000000", RiskQuantityUnit::Notional)?,
        native_position("3", "1", RiskQuantityUnit::Contracts)?,
    ];
    let mut trades = Vec::new();
    let mut prices = Vec::new();
    let mut future_unit_risk = None;
    for (i, (input, position)) in inputs.iter().zip(&positions).enumerate() {
        let value = input.monetary_value()?;
        prices.push(json!({"trade":i,"basis":format!("{:?}",input.basis()),"reference_currency_amount":value.value().get(),"currency":value.currency(),"quantity":position}));
        let outcomes = factors
            .iter()
            .map(|key| {
                curve_quote_risk(
                    input,
                    &graph,
                    key.clone(),
                    position.clone(),
                    snapshot.clone(),
                    settings.clone(),
                )
                .map(|sensitivity| TradeFactorRisk::Present {
                    sensitivity: Box::new(sensitivity),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if i == 4 {
            future_unit_risk = Some(curve_quote_risk(
                input,
                &graph,
                ir.clone(),
                native_position("1", "1", RiskQuantityUnit::Contracts)?,
                snapshot.clone(),
                settings.clone(),
            )?);
        }
        trades.push(TradeSignedRisk {
            trade_id: TradeId::new(format!("standalone.synthetic.trade.{i}"))?,
            trade_version: Version::new(1)?,
            entity: entity.clone(),
            currency: Currency::USD,
            // The reporting policy is explicitly supplied in local currency;
            // physical face, trade copies and contracts cannot be added together.
            gross_exposure: Some(ExactDecimal::parse(
                data["gross_exposure_usd"][i]
                    .as_str()
                    .ok_or("missing supplied gross USD exposure")?,
            )?),
            required_factors: factors.to_vec(),
            outcomes,
        });
    }
    let fx = RiskFxPin {
        from: Currency::USD,
        to: Currency::EUR,
        rate: ExactDecimal::parse(text(&data, "usd_eur_fx")?)?,
        version: Version::new(1)?,
        snapshot_id: snapshot.clone(),
    };
    let scope = RiskEntityScope::Single { entity };
    let complete = aggregate_signed_quote_risk(
        &trades,
        snapshot.clone(),
        scope.clone(),
        Currency::EUR,
        std::slice::from_ref(&fx),
    )?;
    let mut unknown_trades = trades.clone();
    unknown_trades[1].gross_exposure = None;
    unknown_trades[2].gross_exposure = None;
    let unknown_gross = aggregate_signed_quote_risk(
        &unknown_trades,
        snapshot.clone(),
        scope.clone(),
        Currency::EUR,
        std::slice::from_ref(&fx),
    )?;
    let mut missing = trades[0].clone();
    missing.trade_id = TradeId::new("standalone.synthetic.missing-trade")?;
    missing.outcomes.clear();
    missing.gross_exposure = None;
    trades.push(missing);
    let incomplete =
        aggregate_signed_quote_risk(&trades, snapshot.clone(), scope, Currency::EUR, &[fx])?;
    if complete.status != SignedRiskCoverageStatus::Complete
        || incomplete.status != SignedRiskCoverageStatus::Incomplete
        || unknown_gross.status != SignedRiskCoverageStatus::Incomplete
    {
        return Err("coverage failure".into());
    }
    let unit = future_unit_risk.ok_or("missing futures risk")?;
    let target = complete
        .local
        .iter()
        .find(|r| r.key == ir)
        .and_then(|r| r.known_value)
        .ok_or("missing original IR risk")?;
    let rounded = round_futures_hedge(
        ExactDecimal::parse(&target.to_string())?,
        ExactDecimal::parse(&unit.value.to_string())?,
        FuturesRounding::NearestUp,
    )?;
    let shocked = evaluate_quote_graph(&graph, &ir, 0.001)?;
    let scenario = revalue_curves(
        &inputs[0],
        &shocked.curves,
        positions[0].clone(),
        snapshot.clone(),
    )?;
    // Demonstrate a full fixed-cash inverse through the same native facade.
    let quote = bond.price_yield(
        0.031,
        DatedYieldConvention {
            day_count: DayCountConvention::Act365Fixed,
            compounding: Compounding::SemiAnnual,
        },
    )?;
    let inverse = solve_bond_yield(
        &bond,
        DatedYieldConvention {
            day_count: DayCountConvention::Act365Fixed,
            compounding: Compounding::SemiAnnual,
        },
        BondPriceTarget::Clean(quote.clean_per100),
        BondInverseSettings {
            lower: -0.9,
            upper: 1.0,
            price_tolerance: 1e-9,
            parameter_tolerance: 1e-12,
            max_iterations: 256,
        },
    )?;
    Ok(
        json!({"task":"CVX-061","certified":false,"qualification":"Supplied synthetic engineering example; futures/hedge/scenarios are indicative, expected effects are not posted cash. No human/market/model/host/library/production/latency certificate.","snapshot":snapshot,"as_of":text(&data,"as_of")?,"model_notice":STANDARD_MODEL_NOTICE,"built_curves":graph.baseline().build_order,"full_native_repricing":graph.baseline().repricing,"price_evaluations":graph.baseline().price_evaluations,"gross_exposure_policy":text(&data,"gross_exposure_policy")?,"prices":prices,"complete_risk":complete,"unknown_gross_risk":unknown_gross,"incomplete_risk":incomplete,"indicative_integer_hedge":{"contract":future.contract.instrument_id,"risk_factor":ir,"effect":rounded,"execution_status":"indicative_only","costs_and_execution_eligibility":"not supplied; not executable"},"full_revaluation_bond_change":scenario.change,"yield_inverse":{"decimal_rate":inverse.decimal_parameter,"residual_per100":inverse.native_residual,"actual_price_evaluations":inverse.price_evaluations}}),
    )
}
pub fn main() -> Result<(), Error> {
    println!("{}", serde_json::to_string(&run()?)?);
    Ok(())
}

//! Native valuation and rejection tests against separately computed fixtures.
mod support;
use convex_core::{
    cashflows::{CashRounding, CashflowError, PaymentDateEntitlement},
    contracts::{
        DataQuality, InputVersion, ModelId, Provenance, RequestId, SnapshotId, SourceId,
        ValuationSubject, Version,
    },
    fixings::{FixingObservation, FixingSeries, RateOrigin},
};
use convex_curves::contracts::{build_curve_set, CurveContractError, CurveRecipe, CurveSet};
use convex_rates::{
    pricing::{price_swap, SwapParQuotes, SwapValuationInputs},
    results::{swap_pricing_capabilities, CurveVersionBinding, SwapCapabilityStatus},
    FloatingLegTerms, SwapError, SwapSide,
};
use rust_decimal::{prelude::ToPrimitive, Decimal};
use serde_json::Value;
use std::collections::BTreeMap;
use support::*;

fn fixtures() -> Value {
    serde_json::from_str(include_str!("fixtures/valuation.json")).unwrap()
}
fn close(actual: f64, expected: &Value, bound: f64) {
    let expected = expected.as_str().unwrap().parse::<f64>().unwrap();
    assert!(
        (actual - expected).abs() <= bound,
        "{actual:.16e} vs {expected:.16e}, bound {bound}"
    );
}

#[test]
fn independent_seasoned_amortized_stub_holiday_and_negative_price_cash_accrued() {
    let fixture = fixtures();
    assert_eq!(fixture["certified"], false);
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 20);
    for row in fixture["cases"].as_array().unwrap() {
        let source = case(row["source_case"].as_str().unwrap());
        let terms = terms(&source);
        let calendars = calendars(&source);
        let contract = build(&terms, &calendars);
        let curves: Vec<CurveRecipe> = serde_json::from_value(row["curves"].clone()).unwrap();
        let curves = if curves.is_empty() {
            CurveSet::new()
        } else {
            build_curve_set(&curves, &CurveSet::new()).unwrap().curves
        };
        let history = FixingSeries::new(
            serde_json::from_value::<Vec<FixingObservation>>(row["fixings"].clone()).unwrap(),
        )
        .unwrap();
        let ctx = context(d(row["valuation_date"].as_str().unwrap()));
        assert_eq!(
            ctx.fixings.known_at,
            serde_json::from_value(row["known_at"].clone()).unwrap()
        );
        let value = price_swap(
            &contract,
            &SwapValuationInputs {
                curves: &curves,
                fixings: &history,
            },
            ctx,
            publication,
        )
        .unwrap();
        let expected = &row["expected"];
        close(value.present_value.value().get(), &expected["pv"], 1e-7);
        close(
            value.fixed_present_value.value().get(),
            &expected["fixed_pv"],
            1e-7,
        );
        close(
            value.floating_present_value.value().get(),
            &expected["floating_pv"],
            1e-7,
        );
        for (accrued, key) in [
            (&value.fixed_accrued, "fixed_accrued"),
            (&value.floating_accrued, "floating_accrued"),
        ] {
            let amount = accrued;
            close(
                amount.value().decimal().to_f64().unwrap(),
                &expected[key],
                1e-7,
            );
        }
        for (cash, discounted, key) in [
            (&value.cashflows.fixed, &value.fixed, "fixed"),
            (&value.cashflows.floating, &value.floating, "floating"),
        ] {
            let rows = expected[key].as_array().unwrap();
            assert_eq!(cash.len(), rows.len(), "{}", row["id"]);
            for ((cash, pv), row) in cash.iter().zip(discounted).zip(rows) {
                assert_eq!(
                    pv.payment_date.to_string(),
                    row["payment"].as_str().unwrap()
                );
                close(cash.signed_amount.to_f64().unwrap(), &row["amount"], 1e-7);
                close(pv.discount_ratio.get(), &row["discount_ratio"], 1e-13);
                close(pv.present_value.value().get(), &row["pv"], 1e-7);
            }
        }
        if expected["par"].is_null() {
            assert!(matches!(value.par, SwapParQuotes::Unavailable { .. }));
            if value.cashflows.fixed.is_empty() && value.cashflows.floating.is_empty() {
                assert!(value.consumed_curves.is_empty());
            }
        } else {
            let SwapParQuotes::Available {
                fixed_rate,
                floating_spread,
                fixed_rate_shift,
                floating_spread_shift,
                fixed_annuity,
                floating_annuity,
            } = value.par
            else {
                panic!("par unavailable");
            };
            for (actual, key) in [
                (fixed_rate.value().get(), "fixed_rate"),
                (floating_spread.value().get(), "floating_spread"),
                (fixed_rate_shift.value().get(), "fixed_rate_shift"),
                (floating_spread_shift.value().get(), "floating_spread_shift"),
            ] {
                close(actual, &expected["par"][key], 1e-12);
            }
            close(
                fixed_annuity.value().get(),
                &expected["par"]["fixed_annuity"],
                1e-7,
            );
            close(
                floating_annuity.value().get(),
                &expected["par"]["floating_annuity"],
                1e-7,
            );
        }
    }
}

#[test]
fn par_quotes_cancel_pv_and_signed_payer_receiver_offset_with_steps() {
    for id in [
        "regular-eom",
        "amortized-stepped-multiple-resets",
        "ois-shift",
        "ois-lockout",
    ] {
        let case = case(id);
        let mut terms = terms(&case);
        terms.fixed.leg.rounding = CashRounding::Unrounded;
        match &mut terms.floating {
            FloatingLegTerms::Term(t) => t.leg.rounding = CashRounding::Unrounded,
            FloatingLegTerms::Overnight(t) => t.leg.rounding = CashRounding::Unrounded,
        };
        let calendars = calendars(&case);
        let ctx = context(d(if id.starts_with("ois") {
            "2024-04-07"
        } else {
            "2024-03-15"
        }));
        let contract = build(&terms, &calendars);
        let hist = history(&case, &contract, ctx);
        let curves = flat_curve(&terms, d("2023-10-01"), 0.02, 0.038);
        let inputs = SwapValuationInputs {
            curves: &curves,
            fixings: &hist,
        };
        let original = price_swap(&contract, &inputs, ctx, publication).unwrap();
        terms.side = SwapSide::ReceiveFixed;
        let opposite = price_swap(&build(&terms, &calendars), &inputs, ctx, publication).unwrap();
        assert_eq!(
            original.present_value.value().get(),
            -opposite.present_value.value().get()
        );
        assert_eq!(
            original.fixed_present_value.value().get(),
            -opposite.fixed_present_value.value().get()
        );
        assert_eq!(original.par, opposite.par);
        let SwapParQuotes::Available {
            fixed_rate,
            fixed_rate_shift,
            floating_spread,
            floating_spread_shift,
            ..
        } = original.par
        else {
            panic!("unrounded inverse expected");
        };
        for parallel in [false, true] {
            let mut par = terms.clone();
            for level in &mut par.fixed.rates {
                level.value = if parallel {
                    level.value + Decimal::from_f64_retain(fixed_rate_shift.value().get()).unwrap()
                } else {
                    Decimal::from_f64_retain(fixed_rate.value().get()).unwrap()
                };
            }
            assert!(
                price_swap(&build(&par, &calendars), &inputs, ctx, publication)
                    .unwrap()
                    .present_value
                    .value()
                    .get()
                    .abs()
                    < 1e-7
            );
            let spreads = match &mut par.floating {
                FloatingLegTerms::Term(t) => &mut t.profiles.spreads,
                FloatingLegTerms::Overnight(t) => &mut t.profiles.spreads,
            };
            for level in spreads {
                level.value = if parallel {
                    level.value
                        + Decimal::from_f64_retain(floating_spread_shift.value().get()).unwrap()
                } else {
                    Decimal::from_f64_retain(floating_spread.value().get()).unwrap()
                };
            }
            // Test spread inverse against the original fixed profile.
            par.fixed.rates.clone_from(&terms.fixed.rates);
            assert!(
                price_swap(&build(&par, &calendars), &inputs, ctx, publication)
                    .unwrap()
                    .present_value
                    .value()
                    .get()
                    .abs()
                    < 1e-7
            );
        }
    }
}

#[test]
fn known_history_is_curve_invariant_and_missing_history_precedes_market_queries() {
    let case = case("regular-eom");
    let terms = terms(&case);
    let calendars = calendars(&case);
    let contract = build(&terms, &calendars);
    let ctx = context(d("2024-03-15"));
    let history = history(&case, &contract, ctx);
    let curves = flat_curve(&terms, d("2023-10-01"), 0.02, 0.04);
    let bumped = flat_curve(&terms, d("2023-10-01"), 0.02, 0.09);
    let base = price_swap(
        &contract,
        &SwapValuationInputs {
            curves: &curves,
            fixings: &history,
        },
        ctx,
        publication,
    )
    .unwrap();
    let bump = price_swap(
        &contract,
        &SwapValuationInputs {
            curves: &bumped,
            fixings: &history,
        },
        ctx,
        publication,
    )
    .unwrap();
    for (a, b) in base.cashflows.rates.iter().zip(&bump.cashflows.rates) {
        if matches!(a.resolved.origin, RateOrigin::Published(_)) {
            assert_eq!(a, b);
        }
    }
    assert_ne!(base.floating_present_value, bump.floating_present_value);
    assert!(matches!(
        price_swap(
            &contract,
            &SwapValuationInputs {
                curves: &CurveSet::new(),
                fixings: &FixingSeries::new(vec![]).unwrap()
            },
            ctx,
            publication
        ),
        Err(SwapError::Cashflow(CashflowError::MissingFixings(_)))
    ));
}

#[test]
fn exact_projection_currency_role_version_and_discount_mapping_fail_closed() {
    let case = case("regular-eom");
    let terms = terms(&case);
    let calendars = calendars(&case);
    let contract = build(&terms, &calendars);
    let ctx = context(d("2024-01-15"));
    let history = FixingSeries::new(vec![]).unwrap();
    let fixture = fixtures();
    let row = &fixture["cases"][15];
    let base: Vec<CurveRecipe> = serde_json::from_value(row["curves"].clone()).unwrap();
    for change in [0, 1, 2, 3, 4] {
        let mut recipes = base.clone();
        match change {
            0 => recipes[1].key.version = "wrong".into(),
            1 => recipes[1].currency = convex_core::Currency::EUR,
            2 => {
                recipes[1].role = convex_curves::contracts::CurveRole::Discount {
                    collateral_currency: convex_core::Currency::USD,
                }
            }
            3 => {
                recipes[1].role = convex_curves::contracts::CurveRole::Projection {
                    index_id: "OTHER".into(),
                }
            }
            _ => {
                recipes[0].role = convex_curves::contracts::CurveRole::Projection {
                    index_id: "USD.TERM".into(),
                }
            }
        }
        let curves = build_curve_set(&recipes, &CurveSet::new()).unwrap().curves;
        assert!(price_swap(
            &contract,
            &SwapValuationInputs {
                curves: &curves,
                fixings: &history
            },
            ctx,
            publication
        )
        .is_err());
    }
    let curves = CurveSet::new();
    assert!(matches!(
        price_swap(
            &contract,
            &SwapValuationInputs {
                curves: &curves,
                fixings: &history
            },
            ctx,
            publication
        ),
        Err(SwapError::Curve(CurveContractError::MissingDependency(_)))
    ));
}

#[test]
fn rounded_cash_has_no_silent_analytic_par_inverse_and_payment_entitlement_is_explicit() {
    let case = case("regular-eom");
    let mut terms = terms(&case);
    let calendars = calendars(&case);
    terms.fixed.leg.rounding = CashRounding::NearestEven(2);
    let contract = build(&terms, &calendars);
    let ctx = context(d("2024-08-02"));
    let hist = history(&case, &contract, ctx);
    let curves = flat_curve(&terms, d("2023-10-01"), 0.03, 0.04);
    let inputs = SwapValuationInputs {
        curves: &curves,
        fixings: &hist,
    };
    let included = price_swap(&contract, &inputs, ctx, publication).unwrap();
    assert!(matches!(included.par, SwapParQuotes::Unavailable { .. }));
    assert_eq!(included.fixed[0].discount_ratio.get(), 1.0);
    let accrued = included.fixed_accrued;
    assert_eq!(
        accrued.value().decimal(),
        included.cashflows.fixed[0].signed_amount
    );
    let mut exclude = ctx;
    exclude.on_payment = PaymentDateEntitlement::Exclude;
    let excluded = price_swap(
        &contract,
        &SwapValuationInputs {
            curves: &CurveSet::new(),
            fixings: &FixingSeries::new(vec![]).unwrap(),
        },
        exclude,
        publication,
    )
    .unwrap();
    assert_eq!(excluded.present_value.value().get(), 0.0);
    assert!(excluded.valuation_discount.is_none());
    assert!(excluded.consumed_curves.is_empty());
}

#[test]
fn common_price_result_checks_trade_time_and_all_consumed_revisions() {
    let case = case("regular-eom");
    let terms = terms(&case);
    let calendars = calendars(&case);
    let contract = build(&terms, &calendars);
    let ctx = context(d("2024-03-15"));
    let hist = history(&case, &contract, ctx);
    let curves = flat_curve(&terms, d("2023-10-01"), 0.02, 0.04);
    let priced = price_swap(
        &contract,
        &SwapValuationInputs {
            curves: &curves,
            fixings: &hist,
        },
        ctx,
        publication,
    )
    .unwrap();
    let version = Version::new(1).unwrap();
    let mut inputs = BTreeMap::new();
    let mut add = |kind: &str, id: &str, revision: Version| {
        inputs.insert(
            (kind.to_owned(), id.to_owned()),
            InputVersion {
                kind: SourceId::new(kind).unwrap(),
                id: SourceId::new(id).unwrap(),
                version: revision,
            },
        );
    };
    for id in ["ACCRUAL", "PAYMENT", "RESET", "FIXING"] {
        add("calendar", id, version);
    }
    add("collateral", "CSA.USD", version);
    let bindings: Vec<_> = priced
        .consumed_curves
        .iter()
        .map(|curve| {
            add("curve", &curve.key.id, version);
            CurveVersionBinding {
                curve: curve.key.clone(),
                record_version: version,
            }
        })
        .collect();
    for rate in &priced.cashflows.rates {
        if let RateOrigin::Published(obs) = &rate.resolved.origin {
            add(
                "fixing",
                &format!("{}/{}", obs.index_id.as_str(), obs.observation_date),
                obs.revision,
            );
        }
    }
    let provenance = Provenance {
        subject: ValuationSubject::TradeVersion {
            trade_id: terms.trade_id.clone(),
            version: terms.version,
        },
        snapshot_id: SnapshotId::new("swap-test").unwrap(),
        configuration_version: version,
        valuation_time: ts("2024-03-15T12:00:00Z"),
        known_at: ctx.fixings.known_at,
        calculated_at: ts("2024-03-16T00:00:00Z"),
        model_version: ModelId::new("supplied-swap-v1").unwrap(),
        build_id: ModelId::new("test").unwrap(),
        input_versions: inputs.into_values().collect(),
    };
    let request = RequestId::new("test").unwrap();
    let result = priced
        .price_result(
            request.clone(),
            provenance.clone(),
            DataQuality::Indicative,
            &bindings,
        )
        .unwrap();
    assert_eq!(result.results().len(), 3);
    assert_eq!(result.quality(), DataQuality::Indicative);
    assert_eq!(result.provenance(), &provenance);
    let wire = serde_json::to_value(&result).unwrap();
    assert_eq!(
        wire["results"]["pv"]["value"]["value"],
        priced.present_value.value().get()
    );
    for change in 0..4 {
        let mut wrong = provenance.clone();
        match change {
            0 => wrong.known_at = ts("2024-03-14T23:59:59Z"),
            1 => wrong.valuation_time = ts("2024-03-14T12:00:00Z"),
            2 => {
                wrong.subject = ValuationSubject::TradeVersion {
                    trade_id: terms.trade_id.clone(),
                    version: Version::new(2).unwrap(),
                }
            }
            _ => wrong.input_versions.pop().map(|_| ()).unwrap(),
        };
        assert!(priced
            .price_result(request.clone(), wrong, DataQuality::Valid, &bindings)
            .is_err());
    }
    assert!(priced
        .price_result(request, provenance, DataQuality::Valid, &[])
        .is_err());
    let capabilities = swap_pricing_capabilities();
    assert!(capabilities
        .iter()
        .any(|c| c.operation == "pv" && c.status == SwapCapabilityStatus::Supported));
    assert!(capabilities
        .iter()
        .any(|c| c.operation == "quote_sensitivities"
            && c.status == SwapCapabilityStatus::Unsupported));
}

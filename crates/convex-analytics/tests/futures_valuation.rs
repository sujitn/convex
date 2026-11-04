#[path = "support/futures.rs"]
mod support;
use convex_analytics::{futures::*, risk::HedgeExecutionStatus};
use convex_core::{
    calendars::{DynamicCalendar, WeekendType},
    contracts::*,
    daycounts::DayCountConvention,
    fixings::*,
    futures::*,
    Currency,
};
use convex_curves::contracts::CurveRole;
use rust_decimal::Decimal;
use serde_json::json;
use support::*;

fn indicative(report: &FuturesInputReport) {
    assert_eq!(
        report.execution_status,
        HedgeExecutionStatus::IndicativeOnly
    );
    assert!(!report.assessment.eligible);
    assert!(report
        .assessment
        .issues
        .iter()
        .any(|i| i.reason == HedgeInputBlock::Synthetic));
    assert!(report
        .stamps
        .windows(2)
        .all(|s| (&s[0].id, s[0].version) < (&s[1].id, s[1].version)));
}

#[test]
fn ten_actual_pinned_members_have_independent_full_cash_carry_and_invoice_references() {
    let pack = BondPack::new();
    let members = pack.deliverables();
    let f = fixture();
    let result = price_bond_future(&pack.inputs(&members)).unwrap();
    assert_eq!(result.alternatives.len(), 10);
    indicative(&result.inputs);
    assert!(result
        .inputs
        .assessment
        .issues
        .iter()
        .any(|i| i.reason == HedgeInputBlock::Historical));
    assert_eq!(result.model_limitations.len(), 2);
    for m in f["bond"]["members"].as_array().unwrap() {
        let a = result
            .alternatives
            .iter()
            .find(|a| a.instrument.id.as_str() == m["instrument"]["id"].as_str().unwrap())
            .unwrap();
        let actual = serde_json::to_value(a).unwrap();
        for field in [
            "spot_clean_per100",
            "spot_dirty_per100",
            "settlement_accrued_per100",
            "delivery_accrued_per100",
            "coupons_per100",
            "coupon_delivery_value_per100",
            "repo_cost_per100",
            "own_cash_cost_per100",
            "forward_price_points",
            "gross_basis_per100",
            "net_basis_per100",
            "implied_repo_decimal",
        ] {
            close_decimal(
                e(actual[field].as_str().unwrap()).decimal(),
                &m["expected"][field],
                "0.00000000000000000001",
            );
        }
        close_decimal(
            a.delivery_invoice_per_contract.value().decimal(),
            &m["expected"]["delivery_invoice_per_contract"],
            "0",
        );
        close_decimal(
            a.unrounded_basis_profit_per_contract.value().decimal(),
            &m["expected"]["unrounded_basis_profit_per_contract"],
            "0.000000000000000001",
        );
    }
    assert!(result
        .alternatives
        .iter()
        .any(|a| a.coupons_per100.decimal() > Decimal::ZERO));
    assert!(result
        .alternatives
        .iter()
        .any(|a| a.financing.repo_decimal.decimal() < Decimal::ZERO));
}

#[test]
fn independent_ctd_criteria_and_identity_order_are_retained() {
    let pack = BondPack::new();
    let mut members = pack.deliverables();
    let f = fixture();
    for (method, key) in [
        (FuturesCtdMethod::MinimumNetBasis, "net_basis_per100"),
        (FuturesCtdMethod::MaximumImpliedRepo, "implied_repo_decimal"),
        (
            FuturesCtdMethod::MinimumForwardPrice,
            "forward_price_points",
        ),
    ] {
        let mut inputs = pack.inputs(&members);
        inputs.selection = method;
        let a = price_bond_future(&inputs).unwrap();
        assert_eq!(
            a.selected.id.as_str(),
            f["bond"]["selected"][key].as_str().unwrap()
        );
        assert_eq!(
            a.fair_deliverable.id.as_str(),
            f["bond"]["selected"]["forward_price_points"]
                .as_str()
                .unwrap()
        );
        assert!(a.selection_gap.unwrap().decimal() >= Decimal::ZERO);
        members.reverse();
        let mut inputs = pack.inputs(&members);
        inputs.selection = method;
        assert_eq!(price_bond_future(&inputs).unwrap(), a);
    }
}

#[test]
fn missing_duplicate_mismatched_and_unavailable_bond_economics_fail() {
    let pack = BondPack::new();
    let mut members = pack.deliverables();
    members.pop();
    assert!(price_bond_future(&pack.inputs(&members)).is_err());
    let mut members = pack.deliverables();
    members[1].security = members[0].security;
    assert!(price_bond_future(&pack.inputs(&members)).is_err());
    for n in 0..7 {
        let mut p = BondPack::new();
        match n {
            0 => p.own[0] = None,
            1 => p.financing[0].end = d("2026-12-07"),
            2 => p.financing[0].instrument.version = Version::new(2).unwrap(),
            3 => p.quote_stamps[0].available_at = t("2026-10-07T13:00:00Z"),
            4 => p.bond_stamps[0].source_at = t("2026-10-08T00:00:00Z"),
            5 => p.calendar_stamp.version = Version::new(2).unwrap(),
            _ => {
                p.coupon[0] = FuturesCouponCarry::HeldCash {
                    evidence: String::new(),
                }
            }
        }
        let m = p.deliverables();
        assert!(price_bond_future(&p.inputs(&m)).is_err(), "case {n}");
    }
    let m = pack.deliverables();
    for delivery in ["2026-11-30", "2026-12-05", "2027-01-01"] {
        let mut inputs = pack.inputs(&m);
        inputs.delivery = d(delivery);
        assert!(price_bond_future(&inputs).is_err());
    }
}

#[test]
fn independent_discount_repo_and_basis_risk_grids_reprice_the_full_basket() {
    let p = BondPack::new();
    let members = p.deliverables();
    let f = fixture();
    for r in f["bond"]["risk"].as_array().unwrap() {
        let axis = match r["axis"].as_str().unwrap() {
            "zeros" => BondFuturesRiskAxis::DiscountZeros {
                discount: FuturesCurve {
                    curve: &p.discount,
                    stamp: &p.curve_stamp,
                },
            },
            "repo" => BondFuturesRiskAxis::RepoRates,
            _ => BondFuturesRiskAxis::SecurityBasis {
                instrument: &p.basket.bonds[0].instrument,
            },
        };
        let risk = bond_futures_risk(&p.inputs(&members), axis, 3, e(r["bump"].as_str().unwrap()))
            .unwrap();
        close_risk(
            risk.reselected_sensitivity.value().decimal(),
            &r["reselected"],
        );
        close_risk(
            risk.frozen_deliverable_sensitivity.value().decimal(),
            &r["frozen"],
        );
        assert_eq!(
            risk.base.fair_deliverable.id.as_str(),
            r["base_fair"].as_str().unwrap()
        );
        assert_eq!(
            risk.up.fair_deliverable.id.as_str(),
            r["up_fair"].as_str().unwrap()
        );
        assert_eq!(
            risk.down.fair_deliverable.id.as_str(),
            r["down_fair"].as_str().unwrap()
        );
        if r["axis"] == "zeros" {
            assert_eq!(risk.frozen_residual_basis_per100.len(), 10);
            for (id, basis) in &risk.frozen_residual_basis_per100 {
                let m = f["bond"]["members"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|m| m["instrument"]["id"] == id.id.as_str())
                    .unwrap();
                close_decimal(
                    basis.decimal(),
                    &m["expected"]["frozen_residual_basis_per100"],
                    "0.00000001",
                );
            }
        }
        assert_eq!(
            risk.risk_unit,
            if r["axis"] == "basis" {
                "currency_per_clean_point"
            } else {
                "currency_per_bp"
            }
        );
    }
}

#[test]
fn close_ctd_switch_is_visible_and_frozen_risk_is_separate() {
    let mut p = BondPack::new();
    // Supplied control makes member1 just more expensive than member0. Both
    // candidate levels are chosen analytically before the actual scenario run.
    for i in 2..p.prices.len() {
        p.prices[i] = e("200");
    }
    let m = p.deliverables();
    let base = price_bond_future(&p.inputs(&m)).unwrap();
    let a = base
        .alternatives
        .iter()
        .find(|a| a.instrument == p.basket.bonds[0].instrument)
        .unwrap();
    let b = base
        .alternatives
        .iter()
        .find(|a| a.instrument == p.basket.bonds[1].instrument)
        .unwrap();
    let slope = (Decimal::ONE
        + p.financing[1].repo_decimal.decimal() * e("0.1694444444444444444444444444").decimal())
        / b.conversion_factor.decimal();
    p.prices[1] = ExactDecimal::new(
        p.prices[1].decimal()
            + (a.forward_price_points.decimal() + e("0.001").decimal()
                - b.forward_price_points.decimal())
                / slope,
    );
    let m = p.deliverables();
    let risk = bond_futures_risk(
        &p.inputs(&m),
        BondFuturesRiskAxis::SecurityBasis {
            instrument: &p.basket.bonds[0].instrument,
        },
        1,
        e("0.01"),
    )
    .unwrap();
    assert!(risk.fair_deliverable_switch);
    assert_ne!(risk.up.fair_deliverable, risk.down.fair_deliverable);
    assert_ne!(
        risk.reselected_sensitivity,
        risk.frozen_deliverable_sensitivity
    );
}

#[test]
fn published_exchange_normal_margin_examples_round_each_contract_value_first() {
    for r in exchange()["variation"].as_array().unwrap() {
        let mut p = BondPack::new();
        p.contract.currency_per_price_point = e(r["multiplier"].as_str().unwrap());
        if let FuturesSettlement::Bond { contract_face, .. } = &mut p.contract.settlement {
            *contract_face = ExactDecimal::new(
                p.contract.currency_per_price_point.decimal() * Decimal::from(100),
            );
        }
        p.contract.ticks[0].increment = e(r["tick"].as_str().unwrap());
        let rules = rules(&p.contract, None);
        let before = mark(
            &p.contract,
            r["trade"].as_str().unwrap(),
            "2026-10-07",
            FuturesMarkKind::Trade,
        );
        let after = mark(
            &p.contract,
            r["settlement"].as_str().unwrap(),
            "2026-10-07",
            FuturesMarkKind::DailySettlement,
        );
        let effect = futures_settlement_effect(
            &p.contract,
            &rules,
            FuturesCalendar {
                calendar: &p.calendar,
                stamp: &p.calendar_stamp,
            },
            &before,
            &after,
            r["contracts"].as_i64().unwrap(),
            ctx(),
        )
        .unwrap();
        assert_eq!(
            effect.proposed_variation.value(),
            e(r["expected"].as_str().unwrap())
        );
        indicative(&effect.inputs);
        let wrong = (after.price_points.decimal() - before.price_points.decimal())
            * p.contract.currency_per_price_point.decimal()
            * Decimal::from(r["contracts"].as_i64().unwrap());
        assert_ne!(
            effect.proposed_variation.value().decimal(),
            wrong.round_dp(2)
        );
    }
}

#[test]
fn margin_is_additive_telescopes_and_rejects_duplicate_or_invalid_transitions() {
    let mut p = BondPack::new();
    p.contract.stamp = stamp(p.contract.instrument_id.as_str());
    let rules = rules(&p.contract, None);
    let cal = FuturesCalendar {
        calendar: &p.calendar,
        stamp: &p.calendar_stamp,
    };
    let context = EligibilityContext {
        as_of: t("2026-10-09T12:00:00Z"),
        known_at: t("2026-10-09T12:00:00Z"),
    };
    let a = mark(&p.contract, "100", "2026-10-07", FuturesMarkKind::Trade);
    let b = mark(
        &p.contract,
        "100.000001",
        "2026-10-08",
        FuturesMarkKind::DailySettlement,
    );
    let c = mark(
        &p.contract,
        "100.001006",
        "2026-10-09",
        FuturesMarkKind::DailySettlement,
    );
    for n in 1..=512 {
        let v = |x, y, q| {
            futures_settlement_effect(&p.contract, &rules, cal, x, y, q, context)
                .unwrap()
                .proposed_variation
                .value()
                .decimal()
        };
        assert_eq!(v(&a, &b, n) + v(&b, &c, n), v(&a, &c, n));
        assert_eq!(v(&a, &c, n) + v(&a, &c, -n), Decimal::ZERO);
        assert_eq!(v(&a, &c, 1) * Decimal::from(n), v(&a, &c, n));
    }
    assert!(futures_settlement_effect(&p.contract, &rules, cal, &b, &b, 1, context).is_err());
    assert!(futures_settlement_effect(&p.contract, &rules, cal, &c, &b, 1, context).is_err());
    assert!(futures_settlement_effect(&p.contract, &rules, cal, &a, &a, 1, context).is_err());
    for n in [0, i64::MIN, 1_000_000_001] {
        assert!(futures_settlement_effect(&p.contract, &rules, cal, &a, &c, n, context).is_err());
    }
}

#[test]
fn published_hypothetical_cme_sofr_examples_have_exact_final_prices() {
    let f = exchange();
    for name in ["sr3", "sr1"] {
        let q = &f[name];
        let mut contract: FuturesContract =
            serde_json::from_value(reference()["rate_contract"].clone()).unwrap();
        contract.instrument_id = format!("CME-{name}-hypothetical-2017").try_into().unwrap();
        contract.stamp = stamp(contract.instrument_id.as_str());
        contract.product_code = name.to_uppercase();
        contract.contract_code = format!("{name}-illustrative-2017");
        contract.contract_month = ContractMonth {
            year: 2017,
            month: if name == "sr3" { 9 } else { 7 },
        };
        contract.listed_from = d("2017-06-01");
        contract.last_trade = d(if name == "sr3" {
            "2017-09-19"
        } else {
            "2017-07-31"
        });
        contract.expiry = d(q["end"].as_str().unwrap());
        contract.ticks = vec![TickRegime {
            from: contract.listed_from,
            until: None,
            increment: e("0.0025"),
        }];
        if let FuturesSettlement::Rate {
            accrual,
            convexity,
            final_settlement,
        } = &mut contract.settlement
        {
            *final_settlement = contract.expiry;
            *convexity = FuturesConvexityPolicy::NotAdmitted;
            if let RateAccrual::Overnight {
                start,
                end,
                averaging,
                ..
            } = accrual
            {
                *start = d(q["start"].as_str().unwrap());
                *end = d(q["end"].as_str().unwrap());
                *averaging = if name == "sr3" {
                    RateAveraging::Compounded
                } else {
                    RateAveraging::Arithmetic
                };
            }
        }
        let mut calendar =
            DynamicCalendar::new("CME example calendar", WeekendType::SaturdaySunday);
        calendar.add_holidays([d("2017-07-04").date(), d("2017-09-04").date()]);
        let calstamp = stamp("CME-calendar-reference");
        let hs = stamp("published-hypothetical-history");
        let observations = q["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| FixingObservation {
                index_id: "USD.SOFR".to_string().try_into().unwrap(),
                observation_date: d(r["observation"].as_str().unwrap()).date(),
                rate: e(r["decimal_rate"].as_str().unwrap()).decimal(),
                published_at: t(&format!("{}T08:00:00Z", r["publication"].as_str().unwrap())),
                known_at: t(&format!("{}T08:00:00Z", r["publication"].as_str().unwrap())),
                revision: Version::new(1).unwrap(),
                status: FixingStatus::Final,
                source_id: "engineering".to_string().try_into().unwrap(),
            })
            .collect();
        let history =
            FuturesFixings::new(observations, FuturesFixingSelection::FirstPublished).unwrap();
        let rules = rules(
            &contract,
            Some(q["final_decimal_rate_increment"].as_str().unwrap()),
        );
        let inputs = RateFuturesInputs {
            contract: &contract,
            calendar: FuturesCalendar {
                calendar: &calendar,
                stamp: &calstamp,
            },
            clearing: &rules,
            fixings: &history,
            fixing_stamp: &hs,
            projection: None,
            convexity_model: None,
            context: ctx(),
            unpublished: UnpublishedFixingPolicy::RequirePublished,
            final_only: true,
        };
        let publication = |_: &IndexId, at| {
            let row = q["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| d(r["observation"].as_str().unwrap()).date() == at)
                .unwrap();
            Ok(t(&format!(
                "{}T08:00:00Z",
                row["publication"].as_str().unwrap()
            )))
        };
        let result = price_rate_future(&inputs, publication).unwrap();
        assert_eq!(
            result.expected_final_price_points.unwrap(),
            e(q["expected_final_price"].as_str().unwrap())
        );
        close_decimal(
            result.quoted_decimal_rate.decimal(),
            &q["expected_decimal_rate"],
            "0.000000000001",
        );
        assert_eq!(result.projected_days, 0);
        assert!(result.projection_queries.is_empty());
        indicative(&result.inputs);
        if name == "sr1" {
            assert_eq!(
                result.observations[0].requirement.observation_date,
                d("2017-06-30").date()
            );
            assert_eq!(result.observations[0].weight_start, d("2017-07-01").date());
        }
    }
}

#[test]
fn partly_fixed_overnight_risk_matches_independent_six_case_eighteen_bump_grid() {
    let f = fixture();
    let mut arithmetic_risks = Vec::new();
    for case in f["rates"]["cases"].as_array().unwrap() {
        let p = RatePack::new(case);
        let inputs = p.inputs();
        let a = price_rate_future(&inputs, |i, d| p.publication(i, d)).unwrap();
        indicative(&a.inputs);
        close_decimal(
            a.quoted_decimal_rate.decimal(),
            &case["expected_decimal_rate"],
            "0.000000000001",
        );
        close_decimal(
            a.model_price_points.decimal(),
            &case["expected_price_points"],
            "0.0000000001",
        );
        assert_eq!(a.known_days, case["known_days"].as_i64().unwrap());
        assert_eq!(a.projected_days, case["projected_days"].as_i64().unwrap());
        assert_eq!(a.known_days + a.projected_days, 91);
        assert!(a
            .observations
            .iter()
            .any(|o| o.weight_start.days_between(&o.weight_end) == 4));
        for r in case["risks"].as_array().unwrap() {
            let risk = rate_futures_risk(
                &inputs,
                |i, d| p.publication(i, d),
                3,
                e(r["bump"].as_str().unwrap()),
            )
            .unwrap();
            close_risk(
                risk.currency_per_forward_bp.value().decimal(),
                &r["currency_per_forward_bp"],
            );
            if case["averaging"] == "arithmetic" && r["bump"] == "0.0001" {
                arithmetic_risks.push((
                    case["coverage"].as_str().unwrap(),
                    risk.currency_per_forward_bp.value().decimal(),
                ));
            }
        }
        let inverse = remaining_rate_from_price(&p.contract, &a, a.model_price_points).unwrap();
        assert_eq!(inverse.is_none(), case["coverage"] == "fixed");
        if let Some(r) = inverse {
            assert!(r.decimal() > Decimal::ZERO);
        }
        let plus = rate_futures_risk(&inputs, |i, d| p.publication(i, d), 1, e("0.0001")).unwrap();
        let minus =
            rate_futures_risk(&inputs, |i, d| p.publication(i, d), -1, e("0.0001")).unwrap();
        assert_eq!(
            plus.currency_per_forward_bp.value().decimal()
                + minus.currency_per_forward_bp.value().decimal(),
            Decimal::ZERO
        );
        if case["coverage"] == "fixed" {
            let mut i = p.inputs();
            i.projection = None;
            assert!(price_rate_future(&i, |a, b| p.publication(a, b)).is_ok());
            assert_eq!(
                plus.currency_per_forward_bp.value().decimal(),
                Decimal::ZERO
            );
            assert!(remaining_rate_from_price(&p.contract, &a, e("90")).is_err());
        }
    }
    let risk = |s| {
        arithmetic_risks
            .iter()
            .find(|(name, _)| *name == s)
            .unwrap()
            .1
            .abs()
    };
    close_risk(risk("future"), &json!("75"));
    assert!(risk("partial") < risk("future"));
    assert_eq!(risk("fixed"), Decimal::ZERO);
}

#[test]
fn missing_due_history_fails_before_projection_or_convexity() {
    let mut p = RatePack::new(&fixture()["rates"]["cases"][0]);
    p.observations.remove(0);
    p.history = FuturesFixings::new(
        p.observations.clone(),
        FuturesFixingSelection::FirstPublished,
    )
    .unwrap();
    let mut inputs = p.inputs();
    inputs.projection = None;
    let err = price_rate_future(&inputs, |i, d| p.publication(i, d)).unwrap_err();
    assert!(matches!(err, FuturesError::Cashflow(_)));
    assert!(err.to_string().contains("missing") || err.to_string().contains("Missing"));
    assert!(!err.to_string().contains("missing exact index projection"));
}

#[test]
fn original_publications_corrections_and_cutoff_are_explicit() {
    let mut p = RatePack::new(&fixture()["rates"]["cases"][0]);
    let original = price_rate_future(&p.inputs(), |i, d| p.publication(i, d)).unwrap();
    let mut correction = p.observations[0].clone();
    correction.revision = Version::new(2).unwrap();
    correction.rate = e("0.09").decimal();
    correction.published_at = t("2026-09-18T09:00:00Z");
    correction.known_at = correction.published_at;
    p.observations.push(correction.clone());
    p.history = FuturesFixings::new(
        p.observations.clone(),
        FuturesFixingSelection::FirstPublished,
    )
    .unwrap();
    assert_eq!(
        price_rate_future(&p.inputs(), |i, d| p.publication(i, d))
            .unwrap()
            .model_price_points,
        original.model_price_points
    );
    p.history = FuturesFixings::new(
        p.observations.clone(),
        FuturesFixingSelection::LatestEligible,
    )
    .unwrap();
    assert!(price_rate_future(&p.inputs(), |i, d| p.publication(i, d)).is_err());
    p.clearing.fixing_selection = Some(FuturesFixingSelection::LatestEligible);
    let corrected = price_rate_future(&p.inputs(), |i, d| p.publication(i, d)).unwrap();
    assert_ne!(corrected.model_price_points, original.model_price_points);
    let mut only = p.observations.clone();
    only.remove(0);
    assert!(FuturesFixings::new(only, FuturesFixingSelection::FirstPublished).is_err());
    let mut future = p.observations.clone();
    future.last_mut().unwrap().known_at = t("2026-10-07T13:00:00Z");
    p.history = FuturesFixings::new(future, FuturesFixingSelection::LatestEligible).unwrap();
    assert_eq!(
        price_rate_future(&p.inputs(), |i, d| p.publication(i, d))
            .unwrap()
            .model_price_points,
        original.model_price_points
    );
}

#[test]
fn explicit_convexity_sign_lineage_and_remaining_rate_inverse_are_consistent() {
    let mut p = RatePack::new(&fixture()["rates"]["cases"][0]);
    let original_contract = p.contract.clone();
    let base = price_rate_future(&p.inputs(), |i, d| p.publication(i, d)).unwrap();
    if let FuturesSettlement::Rate { convexity, .. } = &mut p.contract.settlement {
        *convexity = FuturesConvexityPolicy::NotAdmitted;
    }
    assert!(price_rate_future(&p.inputs(), |i, d| p.publication(i, d)).is_err());
    let ms = stamp("convexity-control");
    if let FuturesSettlement::Rate { convexity, .. } = &mut p.contract.settlement {
        *convexity = FuturesConvexityPolicy::Supplied {
            decimal_rate: e("0.0002"),
            model: ResourcePin {
                id: ms.id.clone(),
                version: ms.version,
            },
            evidence: "Explicit full-quote adjustment".into(),
        };
    }
    assert!(price_rate_future(&p.inputs(), |i, d| p.publication(i, d)).is_err());
    let mut inputs = p.inputs();
    inputs.convexity_model = Some(&ms);
    let a = price_rate_future(&inputs, |i, d| p.publication(i, d)).unwrap();
    assert_eq!(
        a.model_price_points.decimal() - base.model_price_points.decimal(),
        e("0.02").decimal()
    );
    assert_eq!(a.applied_forward_adjustment, e("0.0002"));
    assert_eq!(
        remaining_rate_from_price(&p.contract, &a, a.model_price_points).unwrap(),
        remaining_rate_from_price(&original_contract, &base, base.model_price_points).unwrap()
    );
    let mut wrong = ms.clone();
    wrong.version = Version::new(2).unwrap();
    inputs.convexity_model = Some(&wrong);
    assert!(price_rate_future(&inputs, |i, d| p.publication(i, d)).is_err());
}

#[test]
fn negative_rates_and_unpublished_policies_are_preserved() {
    let mut p = RatePack::new(&fixture()["rates"]["cases"][0]);
    for o in &mut p.observations {
        o.rate = e("-0.001").decimal();
    }
    p.history = FuturesFixings::new(
        p.observations.clone(),
        FuturesFixingSelection::FirstPublished,
    )
    .unwrap();
    p.projection = curve(
        CurveRole::Projection {
            index_id: "USD.SOFR".into(),
        },
        -0.001,
        d("2026-09-01").date(),
    );
    let a = price_rate_future(&p.inputs(), |i, d| p.publication(i, d)).unwrap();
    assert!(a.model_price_points.decimal() > Decimal::from(100));
    assert!(a
        .observations
        .iter()
        .all(|o| o.decimal_rate.decimal() < Decimal::ZERO));
    let mut inputs = p.inputs();
    inputs.unpublished = UnpublishedFixingPolicy::RequirePublished;
    assert!(price_rate_future(&inputs, |i, d| p.publication(i, d)).is_err());
}

#[test]
fn term_fixing_risk_is_all_or_none_before_the_separate_value_interval() {
    let mut p = RatePack::new(&fixture()["rates"]["cases"][1]);
    if let FuturesSettlement::Rate { accrual, .. } = &mut p.contract.settlement {
        let RateAccrual::Overnight {
            index, calendar, ..
        } = accrual.clone()
        else {
            panic!()
        };
        *accrual = RateAccrual::Term {
            index,
            calendar,
            fixing: d("2026-12-16"),
            start: d("2026-12-18"),
            end: d("2027-03-18"),
            day_count: DayCountConvention::Act360,
        };
    }
    p.contract.validate().unwrap();
    let before = rate_futures_risk(
        &p.inputs(),
        |_, _| Ok(t("2026-12-16T08:00:00Z")),
        1,
        e("0.0001"),
    )
    .unwrap();
    assert_eq!(before.currency_per_forward_bp.value(), e("-25"));
    assert_eq!(before.base.observations.len(), 1);
    assert_eq!(
        before.base.observations[0].rate_start,
        d("2026-12-18").date()
    );
    p.context = EligibilityContext {
        as_of: t("2026-12-16T12:00:00Z"),
        known_at: t("2026-12-16T12:00:00Z"),
    };
    p.history = FuturesFixings::new(
        vec![FixingObservation {
            index_id: "USD.SOFR".to_string().try_into().unwrap(),
            observation_date: d("2026-12-16").date(),
            rate: e("0.035").decimal(),
            published_at: t("2026-12-16T08:00:00Z"),
            known_at: t("2026-12-16T08:00:00Z"),
            revision: Version::new(1).unwrap(),
            status: FixingStatus::Final,
            source_id: "engineering".to_string().try_into().unwrap(),
        }],
        FuturesFixingSelection::FirstPublished,
    )
    .unwrap();
    let mut inputs = p.inputs();
    inputs.projection = None;
    let after = rate_futures_risk(
        &inputs,
        |_, _| Ok(t("2026-12-16T08:00:00Z")),
        1,
        e("0.0001"),
    )
    .unwrap();
    assert_eq!(after.currency_per_forward_bp.value(), e("0"));
    assert_eq!(after.base.expected_final_price_points, Some(e("96.5")));
    assert_eq!(after.base.known_days, 90);
}

fn cash_control(p: &BondPack) -> CashHedgeUniverse {
    CashHedgeUniverse {
        stamp: stamp("cash-control-universe"),
        currency: Currency::USD,
        candidates: vec![CashHedgeCandidate {
            instrument: p.basket.bonds[0].instrument.clone(),
            quote: stamp("cash-control-quote"),
            currency: Currency::USD,
            settlement: d("2026-10-08"),
            lot_face: e("1000"),
            bid_per100: Some(e("94")),
            ask_per100: Some(e("94.1")),
            bid_size_face: Some(e("1000000")),
            ask_size_face: Some(e("1000000")),
            financing: Some(p.financing[0].clone()),
            borrow: Some(BorrowInput {
                stamp: stamp("stock-loan-control"),
                available_face: e("500000"),
                until: d("2026-12-31"),
                fee_decimal: e("0.001"),
                day_count: DayCountConvention::Act360,
            }),
            costs: Some(CashTradingCost {
                stamp: stamp("cash-control-cost"),
                currency_per100_face: e("0.01"),
            }),
        }],
    }
}
fn cash_inputs<'a>(
    p: &'a BondPack,
    u: &'a CashHedgeUniverse,
    side: CashHedgeSide,
) -> CashHedgePricingInputs<'a> {
    CashHedgePricingInputs {
        universe: u,
        security: FuturesBond {
            instrument: &p.basket.bonds[0].instrument,
            stamp: &p.bond_stamps[0],
            bond: &p.bonds[0],
        },
        request: CashHedgeRequest {
            side,
            face: e("100000"),
            owned_face: e("100000"),
            horizon: d("2026-12-08"),
        },
        discount: FuturesCurve {
            curve: &p.discount,
            stamp: &p.curve_stamp,
        },
        own_cash: p.own[0].as_ref(),
        borrow_fee_basis: if side == CashHedgeSide::Short {
            Some(FuturesBorrowFeeBasis::Face)
        } else {
            None
        },
        context: ctx(),
    }
}

#[test]
fn actual_cash_side_fair_value_fee_funding_and_signed_risk_are_distinct() {
    let p = BondPack::new();
    let u = cash_control(&p);
    let buy = price_cash_hedge(&cash_inputs(&p, &u, CashHedgeSide::Buy)).unwrap();
    let sell = price_cash_hedge(&cash_inputs(&p, &u, CashHedgeSide::Sell)).unwrap();
    let short = price_cash_hedge(&cash_inputs(&p, &u, CashHedgeSide::Short)).unwrap();
    let expected = &fixture()["bond"]["members"][0]["expected"];
    close_decimal(
        buy.model_dirty_per100.decimal(),
        &expected["model_dirty_per100"],
        "0.00000001",
    );
    close_decimal(
        buy.accrued_per100.decimal(),
        &expected["settlement_accrued_per100"],
        "0.000000000001",
    );
    assert_ne!(buy.side_clean_per100, Some(buy.model_clean_per100));
    assert_eq!(buy.side_clean_per100, Some(e("94.1")));
    assert_eq!(sell.side_clean_per100, Some(e("94")));
    assert_eq!(
        buy.signed_model_value.value().decimal() + sell.signed_model_value.value().decimal(),
        Decimal::ZERO
    );
    assert_eq!(short.signed_model_value, sell.signed_model_value);
    assert_eq!(buy.transaction_costs.as_ref().unwrap().value(), e("10"));
    assert!(buy.proposed_settlement_cash.unwrap().value().decimal() < Decimal::ZERO);
    assert!(sell.proposed_settlement_cash.unwrap().value().decimal() > Decimal::ZERO);
    assert!(short.borrow_fee_estimate.unwrap().value().decimal() > Decimal::ZERO);
    let buy = cash_hedge_risk(&cash_inputs(&p, &u, CashHedgeSide::Buy), e("0.0001")).unwrap();
    let short = cash_hedge_risk(&cash_inputs(&p, &u, CashHedgeSide::Short), e("0.0001")).unwrap();
    close_risk(
        buy.currency_per_discount_zero_bp.value().decimal(),
        &expected["cash_discount_zero_bp_at_face100000"],
    );
    assert!(buy.currency_per_discount_zero_bp.value().decimal() < Decimal::ZERO);
    assert_eq!(
        buy.currency_per_discount_zero_bp.value().decimal()
            + short.currency_per_discount_zero_bp.value().decimal(),
        Decimal::ZERO
    );
    indicative(&buy.base.inputs);
}

#[test]
fn missing_cash_economics_are_absent_estimates_with_visible_blocks() {
    let p = BondPack::new();
    for n in 0..5 {
        let mut u = cash_control(&p);
        let c = &mut u.candidates[0];
        match n {
            0 => c.ask_per100 = None,
            1 => c.costs = None,
            2 => c.financing = None,
            3 => c.financing.as_mut().unwrap().end = d("2026-12-07"),
            _ => c.borrow.as_mut().unwrap().until = d("2026-12-07"),
        };
        let side = if n == 4 {
            CashHedgeSide::Short
        } else {
            CashHedgeSide::Buy
        };
        let mut inputs = cash_inputs(&p, &u, side);
        if n == 2 {
            inputs.own_cash = None;
        }
        let a = price_cash_hedge(&inputs).unwrap();
        indicative(&a.inputs);
        match n {
            0 => {
                assert!(a.side_dirty_per100.is_none());
                assert!(a.proposed_settlement_cash.is_none());
                assert!(a.initial_balance_financing_estimate.is_none());
            }
            1 => assert!(a.proposed_settlement_cash.is_none()),
            2 | 3 => assert!(a.initial_balance_financing_estimate.is_none()),
            _ => assert!(a.borrow_fee_estimate.is_none()),
        }
    }
    let u = cash_control(&p);
    let mut inputs = cash_inputs(&p, &u, CashHedgeSide::Short);
    inputs.borrow_fee_basis = None;
    assert!(price_cash_hedge(&inputs).is_err());
}

#[test]
fn dated_ticks_quote_delta_signed_rounding_and_integer_residual_are_explicit() {
    let p = BondPack::new();
    let q = futures_quote_risk(&p.contract, d("2026-10-07"), -3).unwrap();
    assert_eq!(q.currency_per_price_point.value(), e("-3000"));
    assert_eq!(q.tick_currency_per_contract.value(), e("15.625"));
    for (rule, want) in [
        (FuturesRounding::Down, "100"),
        (FuturesRounding::Up, "100.015625"),
        (FuturesRounding::NearestAwayFromZero, "100.015625"),
        (FuturesRounding::NearestUp, "100.015625"),
    ] {
        assert_eq!(
            round_futures_price(&p.contract, d("2026-10-07"), e("100.0078125"), rule)
                .unwrap()
                .rounded,
            e(want)
        );
    }
    let nearest =
        round_futures_hedge(e("25"), e("10"), FuturesRounding::NearestAwayFromZero).unwrap();
    assert_eq!(nearest.contracts, -3);
    assert_eq!(nearest.residual_currency_per_bp, e("-5"));
    assert_eq!(
        round_futures_hedge(e("25"), e("10"), FuturesRounding::NearestUp)
            .unwrap()
            .contracts,
        -2
    );
    assert_eq!(
        round_futures_hedge(e("1"), e("10"), FuturesRounding::TowardZero)
            .unwrap()
            .contracts,
        0
    );
    assert!(round_futures_hedge(e("1"), e("0"), FuturesRounding::NearestUp).is_err());
    assert!(round_futures_price(
        &p.contract,
        d("2026-12-22"),
        e("100"),
        FuturesRounding::Down
    )
    .is_err());
}

fn common_factor() -> RiskFactorKey {
    serde_json::from_value(json!({"family":"interest_rate","target":{"kind":"curve","curve_id":"USD.SOFR"},"tenor":"parallel","currency":"USD","quote_type":"zero_rate","basis":"zero_node","convention_version":1,"model_version":"explicit-futures-risk-v1"})).unwrap()
}

#[test]
fn supplied_front_back_roll_keeps_named_contracts_costs_and_risk_residual() {
    let p = RatePack::new(&fixture()["rates"]["cases"][0]);
    let front = &p.contract;
    let mut back = front.clone();
    back.instrument_id = "CME-SR3Z6-control".to_string().try_into().unwrap();
    back.stamp = stamp(back.instrument_id.as_str());
    back.contract_code = "SR3Z6-control".into();
    back.contract_month.month = 12;
    let frontquote = mark(front, "96", "2026-10-07", FuturesMarkKind::Trade);
    let backquote = mark(&back, "95.5", "2026-10-07", FuturesMarkKind::Trade);
    let cost = stamp("roll-cost-control");
    let fr = FuturesRollRisk {
        contract: pin(front),
        factor: common_factor(),
        per_contract_currency_per_bp: Money::new(e("-20"), Currency::USD),
        stamp: stamp("front-risk-control"),
    };
    let mut br = fr.clone();
    br.contract = pin(&back);
    br.per_contract_currency_per_bp = Money::new(e("-25"), Currency::USD);
    br.stamp = stamp("back-risk-control");
    let a = FuturesRollLeg {
        contract: front,
        basket: None,
        quote: &frontquote,
        risk: &fr,
        costs_per_contract: e("1.5"),
        costs_stamp: &cost,
    };
    let b = FuturesRollLeg {
        contract: &back,
        basket: None,
        quote: &backquote,
        risk: &br,
        costs_per_contract: e("2"),
        costs_stamp: &cost,
    };
    let result = plan_futures_roll(&a, &b, 3, FuturesRounding::NearestAwayFromZero, ctx()).unwrap();
    assert_eq!(result.close_front_contracts, -3);
    assert_eq!(result.open_back.contracts, 2);
    assert_eq!(result.open_back.residual_currency_per_bp, e("10"));
    assert_eq!(result.transaction_costs.value(), e("8.5"));
    assert_eq!(result.calendar_spread_points, e("-0.5"));
    indicative(&result.inputs);
    assert!(plan_futures_roll(&a, &a, 3, FuturesRounding::NearestUp, ctx()).is_err());
    let later = EligibilityContext {
        as_of: t("2026-12-16T12:00:00Z"),
        known_at: t("2026-12-16T12:00:00Z"),
    };
    assert!(plan_futures_roll(&a, &b, 3, FuturesRounding::NearestUp, later).is_err());
}

#[test]
fn strict_wire_results_preserve_decimal_lineage_and_refuse_promotion() {
    let p = BondPack::new();
    let m = p.deliverables();
    let a = price_bond_future(&p.inputs(&m)).unwrap();
    let mut v = serde_json::to_value(&a).unwrap();
    assert_eq!(
        serde_json::from_value::<BondFuturesValuation>(v.clone()).unwrap(),
        a
    );
    v["invented"] = json!(0);
    assert!(serde_json::from_value::<BondFuturesValuation>(v).is_err());
    let mut v = serde_json::to_value(&a).unwrap();
    v["inputs"]["execution_status"] = json!("executable");
    assert!(serde_json::from_value::<BondFuturesValuation>(v).is_err());
    let p = RatePack::new(&fixture()["rates"]["cases"][0]);
    let a = price_rate_future(&p.inputs(), |i, d| p.publication(i, d)).unwrap();
    let v = serde_json::to_value(&a).unwrap();
    assert_eq!(
        serde_json::from_value::<RateFuturesValuation>(v).unwrap(),
        a
    );
    for n in 0..5 {
        let mut wrong = a.clone();
        match n {
            0 => wrong.projected_days = -1,
            1 => wrong.known_factor = e("2"),
            2 => wrong.observations[0].weight_start = d("2026-09-17").date(),
            3 => wrong.period_year_fraction = e("1"),
            _ => wrong.applied_forward_adjustment = e("0.001"),
        };
        assert!(remaining_rate_from_price(&p.contract, &wrong, a.model_price_points).is_err());
    }
}

#[test]
fn final_cash_settlement_has_separate_precision_and_cannot_reopen() {
    let p = RatePack::new(&fixture()["rates"]["cases"][2]);
    let previous = mark(
        &p.contract,
        "96",
        "2026-12-15",
        FuturesMarkKind::DailySettlement,
    );
    let final_mark = mark(
        &p.contract,
        "96.1234",
        "2026-12-16",
        FuturesMarkKind::FinalSettlement,
    );
    let cal = FuturesCalendar {
        calendar: &p.calendar,
        stamp: &p.calendar_stamp,
    };
    assert!(p
        .contract
        .validate_order_units(d("2026-12-15"), 1, final_mark.price_points)
        .is_err());
    let result = futures_settlement_effect(
        &p.contract,
        &p.clearing,
        cal,
        &previous,
        &final_mark,
        2,
        p.context,
    )
    .unwrap();
    assert_eq!(result.proposed_variation.value(), e("617"));
    assert!(futures_settlement_effect(
        &p.contract,
        &p.clearing,
        cal,
        &final_mark,
        &final_mark,
        2,
        p.context
    )
    .is_err());
    let mut wrong = final_mark.clone();
    wrong.price_points = e("96.12345");
    assert!(futures_settlement_effect(
        &p.contract,
        &p.clearing,
        cal,
        &previous,
        &wrong,
        2,
        p.context
    )
    .is_err());
    wrong = final_mark;
    wrong.date = d("2026-12-15");
    assert!(futures_settlement_effect(
        &p.contract,
        &p.clearing,
        cal,
        &previous,
        &wrong,
        2,
        p.context
    )
    .is_err());
}

#[test]
fn eligible_inputs_still_have_no_execution_authority_and_stale_data_stays_visible() {
    let mut p = BondPack::new();
    let provider = |s: &mut ReferenceStamp| {
        s.quality = ReferenceQuality::Provider;
    };
    provider(&mut p.contract.stamp);
    provider(&mut p.basket.stamp);
    provider(&mut p.calendar_stamp);
    provider(&mut p.market.stamp);
    for s in p.bond_stamps.iter_mut().chain(&mut p.quote_stamps) {
        provider(s);
    }
    for f in &mut p.financing {
        provider(&mut f.stamp);
    }
    for r in p.own.iter_mut().flatten() {
        provider(&mut r.stamp);
    }
    for c in &mut p.coupon {
        if let FuturesCouponCarry::Reinvested { rate } = c {
            provider(&mut rate.stamp);
        }
    }
    let m = p.deliverables();
    let a = price_bond_future(&p.inputs(&m)).unwrap();
    assert!(a.inputs.assessment.eligible);
    assert_eq!(
        a.inputs.execution_status,
        HedgeExecutionStatus::IndicativeOnly
    );
    p.quote_stamps[0].valid_until = ctx().as_of;
    let m = p.deliverables();
    let a = price_bond_future(&p.inputs(&m)).unwrap();
    assert!(!a.inputs.assessment.eligible);
    assert!(a
        .inputs
        .assessment
        .issues
        .iter()
        .any(|i| i.reason == HedgeInputBlock::Stale));
}

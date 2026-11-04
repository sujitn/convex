use convex_analytics::futures_reference::*;
use convex_analytics::risk::hedging::types::{HedgeExecutionStatus, HedgeProposal};
use convex_core::contracts::{BusinessDate, ExactDecimal, UtcTimestamp, Version};
use convex_core::daycounts::DayCountConvention;
use serde_json::{json, Value};

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/futures/cme-2026-09-24.json"
    ))
    .unwrap()
}
fn bond() -> FuturesContract {
    serde_json::from_value(fixture()["contract"].clone()).unwrap()
}
fn rate() -> FuturesContract {
    serde_json::from_value(fixture()["rate_contract"].clone()).unwrap()
}
fn basket() -> DeliveryBasket {
    serde_json::from_value(fixture()["basket"].clone()).unwrap()
}
fn cash() -> CashHedgeUniverse {
    serde_json::from_value(fixture()["cash_universe"].clone()).unwrap()
}
fn d(s: &str) -> BusinessDate {
    BusinessDate::parse(s).unwrap()
}
fn e(s: &str) -> ExactDecimal {
    ExactDecimal::parse(s).unwrap()
}
fn t(s: &str) -> UtcTimestamp {
    UtcTimestamp::parse(s).unwrap()
}
fn context() -> EligibilityContext {
    EligibilityContext {
        as_of: t("2026-10-07T12:00:00Z"),
        known_at: t("2026-10-07T12:00:00Z"),
    }
}
fn actual_stamp(stamp: &mut ReferenceStamp) {
    // Explicit generated control, not a claim that the archived fixture has become a live feed.
    stamp.quality = ReferenceQuality::Provider;
}
fn live_cash_control() -> CashHedgeUniverse {
    let mut u = cash();
    actual_stamp(&mut u.stamp);
    for c in &mut u.candidates {
        actual_stamp(&mut c.quote);
        c.bid_per100 = Some(e("99.125"));
        c.ask_per100 = Some(e("99.25"));
        c.bid_size_face = Some(e("1000000"));
        c.ask_size_face = Some(e("500000"));
        let mut stamp = c.quote.clone();
        stamp.id = format!("funding-{}", c.instrument.id.as_str())
            .try_into()
            .unwrap();
        c.financing = Some(FinancingInput {
            stamp: stamp.clone(),
            instrument: c.instrument.clone(),
            currency: c.currency,
            start: c.settlement,
            end: d("2026-12-31"),
            repo_decimal: e("-0.001"),
            day_count: DayCountConvention::Act360,
            haircut_fraction: e("0.02"),
        });
        stamp.id = format!("costs-{}", c.instrument.id.as_str())
            .try_into()
            .unwrap();
        c.costs = Some(CashTradingCost {
            stamp,
            currency_per100_face: e("0.01"),
        });
    }
    u
}
fn request(side: CashHedgeSide) -> CashHedgeRequest {
    CashHedgeRequest {
        side,
        face: e("100000"),
        owned_face: e("100000"),
        horizon: d("2026-12-01"),
    }
}
fn has(report: &HedgeInputEligibility, reason: HedgeInputBlock) -> bool {
    !report.eligible && report.issues.iter().any(|i| i.reason == reason)
}

#[test]
fn published_cme_contract_month_security_and_factors_are_exact() {
    let f = fixture();
    let b = bond();
    let list = basket();
    b.validate().unwrap();
    list.validate_contract(&b).unwrap();
    assert_eq!(b.contract_code, "ZNZ6");
    assert_eq!(list.bonds.len(), 10);
    assert_eq!(
        b.contract_month,
        ContractMonth {
            year: 2026,
            month: 12
        }
    );
    assert_eq!(list.bonds[0].instrument.id.as_str(), "UST-91282CQW4");
    assert_eq!(list.bonds[0].conversion_factor, e("0.9069"));
    for (record, original) in list.bonds.iter().zip(f["securities"].as_array().unwrap()) {
        assert_eq!(
            record.instrument.id.as_str(),
            format!("UST-{}", original["cusip"].as_str().unwrap())
        );
        assert_eq!(
            record.conversion_factor,
            e(original["conversion_factor"].as_str().unwrap())
        );
    }
    let encoded = serde_json::to_string(&list).unwrap();
    assert_eq!(
        serde_json::from_str::<DeliveryBasket>(&encoded).unwrap(),
        list
    );
    let r = rate();
    r.validate().unwrap();
    assert_eq!(
        r.contract_month,
        ContractMonth {
            year: 2026,
            month: 9
        }
    );
    assert_eq!(r.last_trade, d("2026-12-15"));
    assert_eq!(r.expiry, d("2026-12-16"));
    let FuturesSettlement::Rate { accrual, .. } = r.settlement else {
        panic!()
    };
    assert_eq!(accrual.window(), (d("2026-09-16"), d("2026-12-16")));
}

#[test]
fn historical_synthetic_stale_incomplete_and_future_known_baskets_never_admit() {
    let mut b = bond();
    let mut list = basket();
    assert!(has(
        &assess_futures_reference(&b, Some(&list), context()).unwrap(),
        HedgeInputBlock::Historical
    ));
    actual_stamp(&mut b.stamp);
    actual_stamp(&mut list.stamp);
    assert!(
        assess_futures_reference(&b, Some(&list), context())
            .unwrap()
            .eligible
    );
    for (quality, reason) in [
        (ReferenceQuality::Synthetic, HedgeInputBlock::Synthetic),
        (ReferenceQuality::Indicative, HedgeInputBlock::Indicative),
        (ReferenceQuality::Historical, HedgeInputBlock::Historical),
    ] {
        let mut mutated = list.clone();
        mutated.stamp.quality = quality;
        let result = assess_futures_reference(&b, Some(&mutated), context()).unwrap();
        assert!(has(&result, reason));
        assert_eq!(result.issues[0].resource, list.stamp.id);
    }
    let mut mutated = list.clone();
    mutated.complete = false;
    assert!(has(
        &assess_futures_reference(&b, Some(&mutated), context()).unwrap(),
        HedgeInputBlock::IncompleteBasket
    ));
    let mut c = context();
    c.as_of = list.stamp.valid_until;
    assert!(has(
        &assess_futures_reference(&b, Some(&list), c).unwrap(),
        HedgeInputBlock::Stale
    ));
    c = context();
    c.known_at = t("2026-09-24T00:00:00Z");
    assert!(has(
        &assess_futures_reference(&b, Some(&list), c).unwrap(),
        HedgeInputBlock::NotYetKnown
    ));
    c = context();
    c.as_of = t("2026-09-23T00:00:00Z");
    assert!(has(
        &assess_futures_reference(&b, Some(&list), c).unwrap(),
        HedgeInputBlock::NotYetEffective
    ));
}

#[test]
fn incompatible_contract_basket_versions_month_currency_and_rate_ctd_fail() {
    let b = bond();
    let list = basket();
    for n in 0..5 {
        let mut m = list.clone();
        match n {
            0 => m.contract.version = Version::new(2).unwrap(),
            1 => m.stamp.version = Version::new(2).unwrap(),
            2 => m.contract_month.month = 9,
            3 => m.currency = convex_core::Currency::GBP,
            _ => m.contract.id = "CME-ZNH7".to_string().try_into().unwrap(),
        }
        assert!(m.validate_contract(&b).is_err());
    }
    assert!(assess_futures_reference(&b, None, context()).is_err());
    assert!(assess_futures_reference(&rate(), Some(&list), context()).is_err());
    assert!(bond().rate_from_price(e("100")).is_err());
    assert!(bond_delivery_invoice(
        &rate(),
        &list,
        &list.bonds[0].instrument,
        d("2026-12-01"),
        e("100"),
        e("0")
    )
    .is_err());
}

#[test]
fn strict_bond_rate_and_observation_json_reject_inapplicable_or_dropped_terms() {
    let f = fixture();
    for field in ["accrual", "convexity", "final_settlement", "ctd"] {
        let mut b = f["contract"].clone();
        b["settlement"][field] = json!({});
        assert!(
            serde_json::from_value::<FuturesContract>(b).is_err(),
            "{field}"
        );
    }
    for field in ["delivery", "basket", "contract_face", "ctd"] {
        let mut r = f["rate_contract"].clone();
        r["settlement"][field] = json!({});
        assert!(
            serde_json::from_value::<FuturesContract>(r).is_err(),
            "{field}"
        );
    }
    let mut r = f["rate_contract"].clone();
    r["settlement"]["accrual"]["lookback"] = json!(2);
    assert!(serde_json::from_value::<FuturesContract>(r).is_err());
    let duplicate = serde_json::to_string(&f["contract"]).unwrap().replacen(
        "\"currency\":\"USD\"",
        "\"currency\":\"USD\",\"currency\":\"GBP\"",
        1,
    );
    assert!(serde_json::from_str::<FuturesContract>(&duplicate).is_err());
    let mut b = f["basket"].clone();
    b["bonds"][0]["conversion_factor"] = json!(0.9069);
    assert!(
        serde_json::from_value::<DeliveryBasket>(b).is_err(),
        "exact Decimal string required even with serde-float features"
    );
}

#[test]
fn exact_ticks_integer_contracts_rate_transform_and_explicit_adjustment() {
    let b = bond();
    let mut r = rate();
    assert_eq!(b.tick_at(d("2026-10-07")).unwrap(), e("0.015625"));
    assert_eq!(r.tick_at(d("2026-08-14")).unwrap(), e("0.005"));
    assert_eq!(r.tick_at(d("2026-08-15")).unwrap(), e("0.0025"));
    assert_eq!(
        r.currency_per_price_point.decimal() * r.tick_at(d("2026-10-07")).unwrap().decimal(),
        e("6.25").decimal()
    );
    b.validate_order_units(d("2026-10-07"), -37, e("100.796875"))
        .unwrap();
    assert!(b
        .validate_order_units(d("2026-10-07"), 0, e("100"))
        .is_err());
    assert!(b
        .validate_order_units(d("2026-10-07"), i64::MIN, e("100"))
        .is_err());
    assert!(b
        .validate_order_units(d("2026-10-07"), 1, e("100.01"))
        .is_err());
    assert!(b.tick_at(d("2026-12-22")).is_err());
    assert!(r
        .validate_order_units(d("2026-10-07"), 1, e("300"))
        .is_err());
    assert_eq!(r.rate_from_price(e("96.1550")).unwrap(), e("0.03845"));
    assert_eq!(r.rate_from_price(e("100.125")).unwrap(), e("-0.00125"));
    assert!(r.calibration_rate(e("96.155")).is_err());
    if let FuturesSettlement::Rate { convexity, .. } = &mut r.settlement {
        *convexity = FuturesConvexityPolicy::Supplied {
            decimal_rate: e("-0.0002"),
            model: ResourcePin {
                id: "supplied-model".to_string().try_into().unwrap(),
                version: Version::new(3).unwrap(),
            },
            evidence: "Explicit deterministic adjustment control".into(),
        };
    }
    assert_eq!(r.calibration_rate(e("96.155")).unwrap(), e("0.03825"));
}

#[test]
fn invoice_published_factor_and_exchange_half_cent_rounding_are_exact() {
    let b = bond();
    let mut list = basket();
    let instrument = list.bonds[0].instrument.clone();
    assert_eq!(
        bond_delivery_invoice(
            &b,
            &list,
            &instrument,
            d("2026-12-01"),
            e("100"),
            e("123.45")
        )
        .unwrap(),
        e("90813.45")
    );
    // Independent Chapter19 Rule19101.B example; factor is explicitly the rule example,
    // not a claim that this altered value belongs to the archived ZNZ6 basket.
    list.bonds[0].conversion_factor = e("0.9633");
    assert_eq!(
        bond_delivery_invoice(
            &b,
            &list,
            &instrument,
            d("2026-12-01"),
            e("100.796875"),
            e("0")
        )
        .unwrap(),
        e("97097.63")
    );
    list.bonds[0].conversion_factor = e("0.00010005");
    assert_eq!(
        bond_delivery_invoice(&b, &list, &instrument, d("2026-12-01"), e("100"), e("0")).unwrap(),
        e("10.01")
    );
    assert!(
        bond_delivery_invoice(&b, &list, &instrument, d("2026-11-30"), e("100"), e("0")).is_err()
    );
    assert!(
        bond_delivery_invoice(&b, &list, &instrument, d("2027-01-01"), e("100"), e("0")).is_err()
    );
    assert!(bond_delivery_invoice(
        &b,
        &list,
        &instrument,
        d("2026-12-01"),
        e("100"),
        e("0.001")
    )
    .is_err());
}

#[test]
fn cash_sizes_lots_costs_funding_and_owned_vs_borrowed_short_are_explicit() {
    let mut u = live_cash_control();
    let i = u.candidates[0].instrument.clone();
    assert!(
        assess_cash_hedge(&u, &i, request(CashHedgeSide::Buy), context())
            .unwrap()
            .eligible
    );
    assert!(
        assess_cash_hedge(&u, &i, request(CashHedgeSide::Sell), context())
            .unwrap()
            .eligible
    );
    assert!(has(
        &assess_cash_hedge(&u, &i, request(CashHedgeSide::Short), context()).unwrap(),
        HedgeInputBlock::MissingBorrow
    ));
    let mut req = request(CashHedgeSide::Sell);
    req.owned_face = e("99999");
    assert!(has(
        &assess_cash_hedge(&u, &i, req, context()).unwrap(),
        HedgeInputBlock::InsufficientSize
    ));
    req = request(CashHedgeSide::Buy);
    req.face = e("100001");
    assert!(has(
        &assess_cash_hedge(&u, &i, req, context()).unwrap(),
        HedgeInputBlock::OffLot
    ));
    req.face = e("501000");
    assert!(has(
        &assess_cash_hedge(&u, &i, req, context()).unwrap(),
        HedgeInputBlock::InsufficientSize
    ));
    let mut stamp = u.candidates[0].quote.clone();
    stamp.id = "borrow-control".to_string().try_into().unwrap();
    u.candidates[0].borrow = Some(BorrowInput {
        stamp,
        available_face: e("100000"),
        until: d("2026-12-01"),
        fee_decimal: e("0.001"),
        day_count: DayCountConvention::Act360,
    });
    assert!(
        assess_cash_hedge(&u, &i, request(CashHedgeSide::Short), context())
            .unwrap()
            .eligible
    );
    let c = &mut u.candidates[0];
    c.borrow.as_mut().unwrap().until = d("2026-11-30");
    assert!(has(
        &assess_cash_hedge(&u, &i, request(CashHedgeSide::Short), context()).unwrap(),
        HedgeInputBlock::BorrowHorizon
    ));
}

#[test]
fn missing_indicative_and_stale_cash_dependencies_remain_visible() {
    let u = live_cash_control();
    let i = u.candidates[0].instrument.clone();
    for n in 0..6 {
        let mut m = u.clone();
        let c = &mut m.candidates[0];
        let reason = match n {
            0 => {
                c.ask_per100 = None;
                HedgeInputBlock::MissingQuote
            }
            1 => {
                c.financing = None;
                HedgeInputBlock::MissingFinancing
            }
            2 => {
                c.costs = None;
                HedgeInputBlock::MissingCosts
            }
            3 => {
                c.financing.as_mut().unwrap().end = d("2026-11-30");
                HedgeInputBlock::FinancingHorizon
            }
            4 => {
                c.costs.as_mut().unwrap().stamp.quality = ReferenceQuality::Indicative;
                HedgeInputBlock::Indicative
            }
            _ => {
                c.quote.valid_until = context().as_of;
                HedgeInputBlock::Stale
            }
        };
        assert!(has(
            &assess_cash_hedge(&m, &i, request(CashHedgeSide::Buy), context()).unwrap(),
            reason
        ));
    }
    let archived = cash();
    let r = assess_cash_hedge(&archived, &i, request(CashHedgeSide::Buy), context()).unwrap();
    for reason in [
        HedgeInputBlock::Historical,
        HedgeInputBlock::MissingQuote,
        HedgeInputBlock::InsufficientSize,
        HedgeInputBlock::MissingCosts,
        HedgeInputBlock::MissingFinancing,
    ] {
        assert!(has(&r, reason));
    }
}

#[test]
fn invalid_or_oversized_terms_fail_before_economic_use() {
    for n in 0..7 {
        let mut b = bond();
        match n {
            0 => b.contract_month.month = 13,
            1 => b.ticks.clear(),
            2 => b.ticks = vec![b.ticks[0].clone(); 33],
            3 => b.ticks[0].increment = e("0"),
            4 => b.currency_per_price_point = e("1001"),
            5 => b.stamp.available_at = b.stamp.valid_until,
            _ => b.product_code = "x".repeat(513),
        }
        assert!(b.validate().is_err());
    }
    let mut list = basket();
    list.bonds.push(list.bonds[0].clone());
    assert!(list.validate().is_err());
    for factor in ["0", "-1", "10.0001"] {
        let mut list = basket();
        list.bonds[0].conversion_factor = e(factor);
        assert!(list.validate().is_err());
    }
    let mut u = live_cash_control();
    u.candidates[0].ask_per100 = Some(e("98"));
    assert!(u.validate().is_err());
    u = live_cash_control();
    u.candidates[0].lot_face = e("0");
    assert!(u.validate().is_err());
    u = live_cash_control();
    u.candidates[0]
        .financing
        .as_mut()
        .unwrap()
        .instrument
        .version = Version::new(2).unwrap();
    assert!(u.validate().is_err());
    u = live_cash_control();
    u.candidates[0].financing.as_mut().unwrap().currency = convex_core::Currency::GBP;
    assert!(u.validate().is_err());
}

#[test]
fn term_rate_dates_overnight_windows_and_trading_boundaries_are_distinct() {
    let mut r = rate();
    let FuturesSettlement::Rate {
        accrual,
        final_settlement,
        ..
    } = &mut r.settlement
    else {
        panic!()
    };
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
    *final_settlement = d("2026-12-16");
    r.validate().unwrap();
    if let FuturesSettlement::Rate {
        accrual: RateAccrual::Term { fixing, .. },
        ..
    } = &mut r.settlement
    {
        *fixing = d("2026-12-19");
    }
    assert!(r.validate().is_err());
    r = rate();
    if let FuturesSettlement::Rate {
        accrual: RateAccrual::Overnight { end, .. },
        ..
    } = &mut r.settlement
    {
        *end = d("2026-09-16");
    }
    assert!(r.validate().is_err());
    let mut b = bond();
    let mut list = basket();
    actual_stamp(&mut b.stamp);
    actual_stamp(&mut list.stamp);
    b.stamp.valid_until = t("2027-01-01T00:00:00Z");
    list.stamp.valid_until = b.stamp.valid_until;
    let c = EligibilityContext {
        as_of: t("2026-12-21T00:00:00Z"),
        ..context()
    };
    assert!(has(
        &assess_futures_reference(&b, Some(&list), c).unwrap(),
        HedgeInputBlock::OutsideTradingWindow
    ));
    b.ticks[0].until = Some(d("2026-12-01"));
    b.ticks.push(TickRegime {
        from: d("2026-12-02"),
        until: None,
        increment: e("0.015625"),
    });
    assert!(b.validate().is_err(), "tick coverage gap must fail");
}

#[test]
fn bounded_exact_decimal_rate_and_tick_properties_replay_without_rounding() {
    let r = rate();
    let mut state = 22_u64;
    for _ in 0..512 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let ticks = (state % 20_000) as i64;
        let price = rust_decimal::Decimal::from(75)
            + rust_decimal::Decimal::from(ticks) * e("0.0025").decimal();
        let value = ExactDecimal::new(price);
        r.validate_order_units(d("2026-10-07"), 1, value).unwrap();
        let rate = r.rate_from_price(value).unwrap();
        assert_eq!(
            rust_decimal::Decimal::from(100) - rust_decimal::Decimal::from(100) * rate.decimal(),
            price
        );
        let wire = serde_json::to_string(&value).unwrap();
        assert_eq!(serde_json::from_str::<ExactDecimal>(&wire).unwrap(), value);
    }
}

#[test]
fn indicative_advisor_wire_outputs_cannot_be_promoted_to_executable() {
    let v = json!({"strategy":"historical-control","trades":[],"residual": {
        "residual_dv01":0.0,"residual_krd_l1_norm":0.0},"cost_bps":0.0,"cost_total":0.0});
    let p: HedgeProposal = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(p.execution_status, HedgeExecutionStatus::IndicativeOnly);
    assert_eq!(
        serde_json::to_value(p).unwrap()["execution_status"],
        "indicative_only"
    );
    let mut claimed = v;
    claimed["execution_status"] = json!("executable");
    assert!(serde_json::from_value::<HedgeProposal>(claimed).is_err());
}

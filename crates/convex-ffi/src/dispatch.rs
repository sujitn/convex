//! Stateless RPC dispatchers.
//!
//! Each `pub fn` here is a one-shot: take a JSON request string, route it
//! through the right analytics call, return a JSON envelope. They never
//! panic; everything that can go wrong (missing handle, non-finite math,
//! wrong bond shape, …) lands in a typed [`DispatchError`] and serialises
//! back to a `{"ok":"false","error":{...}}` body.
//!
//! Bond-shape routing happens once per entry point. Fixed-coupon shapes
//! (FixedRate, Callable, SinkingFund) share a single generic body via
//! `with_fixed_bond!`. FRN and ZeroCoupon don't impl `FixedCouponBond` and
//! get dedicated dispatchers (their analytics surfaces differ from a
//! YTM-driven bullet).

use convex_analytics::dto::{
    CashflowEntry, CashflowRequest, CashflowResponse, CompareRequest, CompareResponse,
    CurveQueryKind, CurveQueryRequest, CurveQueryResponse, HedgeRequest, HedgeStrategyKind,
    KeyRate, MakeWholeRequest, MakeWholeResponse, MarkInput, PricingRequest, PricingResponse,
    RiskProfileRequest, RiskRequest, RiskResponse, ScenarioBumpSpec, ScenarioRequest,
    ScenarioResponse, ScenarioRow, SpreadRequest, SpreadResponse, YasRequest, YasResponse,
};
use convex_analytics::pricing::price_from_mark;
use convex_analytics::risk::{
    barbell_futures, cash_bond_pair, compare_hedges, compute_callable_position_risk,
    compute_position_risk, duration_futures, interest_rate_swap, key_rate_futures, narrate,
    HedgeProposal, RiskProfile,
};
use convex_analytics::spreads::{
    GSpreadCalculator, ISpreadCalculator, OASCalculator, ParParAssetSwap, ProceedsAssetSwap,
    ZSpreadCalculator,
};
use convex_bonds::instruments::{
    CallableBond, FixedRateBond, FloatingRateNote, SinkingFundBond, ZeroCouponBond,
};
use convex_bonds::traits::{Bond, CashFlowType, FixedCouponBond};
use convex_core::types::{Compounding, Frequency, Mark, Price, SpreadType, Yield};
use convex_curves::bumping::ParallelBump;
use convex_curves::{DiscreteCurve, RateCurve, RateCurveDyn};
use rust_decimal::Decimal;

use crate::registry::{self, BondKind, Handle, ObjectKind};

// ---- Error type ----------------------------------------------------------

/// Routes to one of the three error codes the FFI envelope exposes.
enum DispatchError {
    /// Bad JSON shape, missing required field, or wrong bond/curve type.
    InvalidInput {
        message: String,
        field: Option<&'static str>,
    },
    /// Handle not in the registry, or wrong kind for the requested op.
    InvalidHandle(String),
    /// Analytics call failed (solver did not converge, settle ≥ maturity, …).
    Analytics(String),
}

impl DispatchError {
    fn input(message: impl Into<String>) -> Self {
        Self::InvalidInput {
            message: message.into(),
            field: None,
        }
    }
    fn input_field(field: &'static str, message: impl Into<String>) -> Self {
        Self::InvalidInput {
            message: message.into(),
            field: Some(field),
        }
    }
    fn handle(message: impl Into<String>) -> Self {
        Self::InvalidHandle(message.into())
    }
    fn analytics(message: impl Into<String>) -> Self {
        Self::Analytics(message.into())
    }
}

impl<E: std::fmt::Display> From<E> for DispatchError {
    /// Conversions from analytics errors (`AnalyticsError`, `BondError`, …)
    /// flatten to `Analytics`. Use `DispatchError::handle` / `::input`
    /// explicitly for the other arms.
    fn from(e: E) -> Self {
        Self::Analytics(e.to_string())
    }
}

fn to_envelope<T: serde::Serialize>(r: Result<T, DispatchError>) -> String {
    match r {
        Ok(v) => crate::ok_envelope(&v),
        Err(DispatchError::InvalidInput {
            message,
            field: Some(f),
        }) => crate::err_envelope_field("invalid_input", &message, f),
        Err(DispatchError::InvalidInput {
            message,
            field: None,
        }) => crate::err_envelope("invalid_input", &message),
        Err(DispatchError::InvalidHandle(m)) => crate::err_envelope("invalid_handle", &m),
        Err(DispatchError::Analytics(m)) => crate::err_envelope("analytics", &m),
    }
}

// ---- Ticker-style reference resolution -------------------------------------
//
// Requests may reference bonds/curves by handle (`"bond": 101`) or by name
// (`"bond": "912828YK0"`). Only this crate owns the registry, so names are
// resolved in the request JSON before deserialization, keeping the DTOs'
// `Handle` fields untouched. Unknown names map to `invalid_handle`.

/// Field paths that carry registry references, per request shape.
const PRICE_REFS: &[&[&str]] = &[&["bond"], &["curve"], &["forward_curve"]];
const RISK_REFS: &[&[&str]] = &[&["bond"], &["curve"], &["forward_curve"]];
const SPREAD_REFS: &[&[&str]] = &[&["bond"], &["curve"], &["params", "forward_curve"]];
const CASHFLOWS_REFS: &[&[&str]] = &[&["bond"]];
const CURVE_QUERY_REFS: &[&[&str]] = &[&["curve"]];
const MAKE_WHOLE_REFS: &[&[&str]] = &[&["bond"]];
const RISK_PROFILE_REFS: &[&[&str]] = &[&["bond"], &["curve"]];
const HEDGE_REFS: &[&[&str]] = &[&["curve"]];
const YAS_REFS: &[&[&str]] = &[&["bond"], &["curve"], &["swap_curve"]];
const SCENARIO_REFS: &[&[&str]] = &[&["bond"], &["curve"]];

fn field_mut<'a>(
    value: &'a mut serde_json::Value,
    path: &[&str],
) -> Option<&'a mut serde_json::Value> {
    let mut cur = value;
    for seg in path {
        cur = cur.get_mut(*seg)?;
    }
    Some(cur)
}

/// Parse the request once, rewriting string references at `paths` to
/// resolved handles; the value feeds the handler via `from_value`, so a
/// request costs one parse.
fn resolve_refs(request_json: &str, paths: &[&[&str]]) -> Result<serde_json::Value, DispatchError> {
    let mut v: serde_json::Value = serde_json::from_str(request_json)
        .map_err(|e| DispatchError::input(format!("request: {e}")))?;
    for path in paths {
        if let Some(slot) = field_mut(&mut v, path) {
            if let serde_json::Value::String(name) = &*slot {
                match registry::resolve_alias(name) {
                    Some(h) => *slot = serde_json::Value::from(h),
                    None => {
                        return Err(DispatchError::handle(format!(
                            "no bond or curve registered under '{name}' — build it first \
                             or check the identifier"
                        )))
                    }
                }
            }
        }
    }
    Ok(v)
}

fn with_resolved_refs<T: serde::Serialize>(
    request_json: &str,
    paths: &[&[&str]],
    inner: impl FnOnce(serde_json::Value) -> Result<T, DispatchError>,
) -> String {
    to_envelope(resolve_refs(request_json, paths).and_then(inner))
}

// ---- describe ------------------------------------------------------------

/// Returns enough JSON for an Object Browser to actually inform the user:
/// bond fields (coupon, maturity, frequency, day count, currency, face),
/// curve fields (ref date, tenor count, max tenor, value kind).
pub fn describe(handle: Handle) -> String {
    let kind = match registry::kind_of(handle) {
        Some(k) => k,
        None => {
            return crate::err_envelope("invalid_handle", &format!("handle {handle} not found"))
        }
    };
    let mut payload = serde_json::json!({
        "handle": handle,
        "kind": kind.tag(),
        "name": registry::name_of(handle),
    });

    match kind {
        ObjectKind::Bond(BondKind::FixedRate) => {
            registry::with_object::<FixedRateBond, _, _>(handle, |b| extend_fixed(&mut payload, b));
        }
        ObjectKind::Bond(BondKind::Callable) => {
            registry::with_object::<CallableBond, _, _>(handle, |cb| {
                extend_fixed(&mut payload, cb.base_bond());
                if let Some(ct) = cb.call_type() {
                    payload["call_type"] = serde_json::json!(format!("{:?}", ct));
                }
            });
        }
        ObjectKind::Bond(BondKind::FloatingRate) => {
            registry::with_object::<FloatingRateNote, _, _>(handle, |f| {
                payload["spread_bps"] = serde_json::json!(dec_to_f64(f.spread_bps()));
                payload["maturity"] = serde_json::json!(f.maturity_date().to_string());
                payload["issue"] = serde_json::json!(f.get_issue_date().to_string());
                payload["frequency"] = serde_json::json!(format!("{:?}", f.frequency()));
                payload["day_count"] = serde_json::json!(format!("{:?}", f.day_count()));
            });
        }
        ObjectKind::Bond(BondKind::ZeroCoupon) => {
            registry::with_object::<ZeroCouponBond, _, _>(handle, |z| {
                payload["maturity"] = serde_json::json!(z.maturity_date().to_string());
                payload["compounding"] = serde_json::json!(format!("{:?}", z.compounding()));
                payload["day_count"] = serde_json::json!(format!("{:?}", z.day_count()));
            });
        }
        ObjectKind::Bond(BondKind::SinkingFund) => {
            registry::with_object::<SinkingFundBond, _, _>(handle, |s| {
                extend_fixed(&mut payload, s.base_bond());
                payload["original_face"] = serde_json::json!(dec_to_f64(s.original_face()));
            });
        }
        ObjectKind::Curve => {
            registry::with_object::<RateCurve<DiscreteCurve>, _, _>(handle, |c| {
                let inner = c.inner();
                payload["ref_date"] = serde_json::json!(inner.get_reference_date().to_string());
                payload["tenor_count"] = serde_json::json!(inner.tenors().len());
                payload["max_tenor"] =
                    serde_json::json!(inner.tenors().last().copied().unwrap_or(0.0));
            });
        }
    }

    serde_json::json!({"ok": "true", "result": payload}).to_string()
}

fn extend_fixed(payload: &mut serde_json::Value, b: &FixedRateBond) {
    payload["coupon_rate"] = serde_json::json!(dec_to_f64(b.coupon_rate_decimal()));
    if let Some(m) = b.maturity() {
        payload["maturity"] = serde_json::json!(m.to_string());
    }
    payload["frequency"] = serde_json::json!(format!("{:?}", b.frequency()));
    payload["day_count"] = serde_json::json!(format!("{:?}", b.day_count()));
    payload["currency"] = serde_json::json!(format!("{:?}", b.currency()));
    payload["face_value"] = serde_json::json!(dec_to_f64(b.face_value()));
}

// ---- price ---------------------------------------------------------------

pub fn price(request_json: &str) -> String {
    with_resolved_refs(request_json, PRICE_REFS, price_inner)
}

fn price_inner(request: serde_json::Value) -> Result<PricingResponse, DispatchError> {
    let req: PricingRequest = serde_json::from_value(request)
        .map_err(|e| DispatchError::input(format!("PricingRequest: {e}")))?;
    let mark = parse_mark(&req.mark)?;

    match registry::kind_of(req.bond) {
        Some(ObjectKind::Bond(BondKind::FloatingRate)) => price_frn(&req, &mark),
        Some(ObjectKind::Bond(BondKind::ZeroCoupon)) => price_zero(&req, &mark),
        _ => price_fixed_coupon(&req, &mark),
    }
}

fn price_fixed_coupon(req: &PricingRequest, mark: &Mark) -> Result<PricingResponse, DispatchError> {
    if matches!(mark, Mark::Spread {value,..} if value.spread_type()==SpreadType::GSpread) {
        return Err(DispatchError::input_field("mark", "G-spread price marks require actual government benchmarks via native quote_fixed; v1 PricingRequest cannot supply them"));
    }
    let curve = optional_curve(req.curve)?;
    with_fixed_bond!(req.bond, bond, {
        let r = price_from_mark(
            bond,
            req.settlement,
            mark,
            curve.as_deref(),
            req.quote_frequency,
        )?;
        Ok(PricingResponse {
            clean_price: r.clean_price_per_100,
            dirty_price: r.dirty_price_per_100,
            accrued: r.accrued_per_100,
            ytm_decimal: r.ytm_decimal,
            z_spread_bps: r.z_spread_bps,
        })
    })
}

fn missing_frn_fixings<T>() -> Result<T, DispatchError> {
    Err(DispatchError::input_field(
        "fixings",
        "FRN analytics require eligible fixing history and an explicit projection curve; the v1 request cannot supply fixing history (CVX-055/CVX-063)",
    ))
}

fn price_frn(_req: &PricingRequest, _mark: &Mark) -> Result<PricingResponse, DispatchError> {
    missing_frn_fixings()
}

fn price_zero(req: &PricingRequest, mark: &Mark) -> Result<PricingResponse, DispatchError> {
    with_zero(req.bond, |z| {
        let (p, _) = convex_analytics::bond_quote::quote_zero(z, req.settlement, mark)?;
        Ok(PricingResponse {
            clean_price: p.clean_price_per_100,
            dirty_price: p.dirty_price_per_100,
            accrued: p.accrued_per_100,
            ytm_decimal: p.ytm_decimal,
            z_spread_bps: None,
        })
    })?
}

// ---- risk ----------------------------------------------------------------

pub fn risk(request_json: &str) -> String {
    with_resolved_refs(request_json, RISK_REFS, risk_inner)
}

fn risk_inner(request: serde_json::Value) -> Result<RiskResponse, DispatchError> {
    let req: RiskRequest = serde_json::from_value(request)
        .map_err(|e| DispatchError::input(format!("RiskRequest: {e}")))?;
    let mark = parse_mark(&req.mark)?;

    match registry::kind_of(req.bond) {
        Some(ObjectKind::Bond(BondKind::FloatingRate)) => risk_frn(&req, &mark),
        Some(ObjectKind::Bond(BondKind::ZeroCoupon)) => risk_zero(&req, &mark),
        _ => {
            let curve = optional_curve(req.curve)?;
            with_fixed_bond!(
                req.bond,
                bond,
                risk_fixed(bond, &mark, &req, curve.as_deref())
            )
        }
    }
}

fn risk_fixed<B: Bond + FixedCouponBond>(
    bond: &B,
    mark: &Mark,
    req: &RiskRequest,
    curve: Option<&dyn RateCurveDyn>,
) -> Result<RiskResponse, DispatchError> {
    if matches!(mark, Mark::Spread {value,..} if value.spread_type()==SpreadType::GSpread) {
        return Err(DispatchError::input_field(
            "mark",
            "G-spread price marks require actual government benchmarks",
        ));
    }
    let priced = price_from_mark(bond, req.settlement, mark, curve, req.quote_frequency)?;
    let r = convex_analytics::bond_quote::yield_risk(
        bond,
        req.settlement,
        &priced,
        req.quote_frequency,
    )?;
    let key_rates = if req.key_rate_tenors.is_empty() {
        vec![]
    } else {
        compute_krd(bond, mark, req)?
    };

    Ok(RiskResponse {
        modified_duration: r.modified_duration,
        macaulay_duration: r.macaulay_duration,
        convexity: r.convexity,
        dv01: r.dv01_loss,
        spread_duration: priced.z_spread_bps.map(|_| r.modified_duration),
        key_rates,
    })
}

fn risk_frn(_req: &RiskRequest, _mark: &Mark) -> Result<RiskResponse, DispatchError> {
    missing_frn_fixings()
}

// Closed-form: macaulay = years to maturity; modified = mac/(1+y/f);
// convexity = T·(T+1/f)/(1+y/f)²; DV01 = mod·P·1e-4.
fn risk_zero(req: &RiskRequest, mark: &Mark) -> Result<RiskResponse, DispatchError> {
    with_zero(req.bond, |z| {
        if !req.key_rate_tenors.is_empty() {
            return Err(DispatchError::input(
                "zero key-rate convention not available",
            ));
        }
        let (_, r) = convex_analytics::bond_quote::quote_zero(z, req.settlement, mark)?;
        Ok(RiskResponse {
            modified_duration: r.modified_duration,
            macaulay_duration: r.macaulay_duration,
            convexity: r.convexity,
            dv01: r.dv01_loss,
            spread_duration: None,
            key_rates: vec![],
        })
    })?
}

// KRD: hold the implied Z-spread fixed, ±1bp triangular bump at each key
// tenor, reprice. KRD = (P_- − P_+) / (2·P0·Δy).
fn compute_krd<B: Bond + FixedCouponBond>(
    bond: &B,
    mark: &Mark,
    req: &RiskRequest,
) -> Result<Vec<KeyRate>, DispatchError> {
    let curve_handle = req.curve.ok_or_else(|| {
        DispatchError::input_field("curve", "key_rate_tenors require a discount curve")
    })?;
    let curve = required_curve(curve_handle)?;
    let priced = price_from_mark(
        bond,
        req.settlement,
        mark,
        Some(&*curve),
        req.quote_frequency,
    )?;
    let dirty = Decimal::from_f64_retain(priced.dirty_price_per_100)
        .ok_or_else(|| DispatchError::analytics("nonfinite price"))?;
    let z = dec_to_f64(
        ZSpreadCalculator::new(&*curve)
            .calculate(bond, dirty, req.settlement)?
            .as_decimal(),
    );
    let r = convex_analytics::bond_quote::curve_risk(
        bond,
        req.settlement,
        &*curve,
        z,
        1.0,
        &req.key_rate_tenors,
    )?;
    Ok(r.key_rates
        .into_iter()
        .map(|(tenor, duration)| KeyRate { tenor, duration })
        .collect())
}

// ---- spread --------------------------------------------------------------

pub fn spread(request_json: &str) -> String {
    with_resolved_refs(request_json, SPREAD_REFS, spread_inner)
}

fn spread_inner(request: serde_json::Value) -> Result<SpreadResponse, DispatchError> {
    let req: SpreadRequest = serde_json::from_value(request)
        .map_err(|e| DispatchError::input(format!("SpreadRequest: {e}")))?;
    let mark = parse_mark(&req.mark)?;

    match (req.spread_type, registry::kind_of(req.bond)) {
        (SpreadType::OAS, Some(ObjectKind::Bond(BondKind::Callable))) => spread_oas(&req, &mark),
        (SpreadType::OAS, _) => Err(DispatchError::input(format!(
            "OAS requires a callable bond (handle {} is not callable)",
            req.bond
        ))),
        (SpreadType::DiscountMargin, Some(ObjectKind::Bond(BondKind::FloatingRate))) => {
            spread_dm(&req)
        }
        (SpreadType::DiscountMargin, _) => Err(DispatchError::input(format!(
            "DiscountMargin requires an FRN (handle {} is not an FRN)",
            req.bond
        ))),
        _ => {
            let curve = required_curve(req.curve)?;
            with_fixed_bond!(req.bond, bond, spread_fixed(bond, &mark, &req, &*curve))
        }
    }
}

fn spread_fixed<B: Bond + FixedCouponBond>(
    bond: &B,
    mark: &Mark,
    req: &SpreadRequest,
    curve: &dyn RateCurveDyn,
) -> Result<SpreadResponse, DispatchError> {
    let priced = price_from_mark(
        bond,
        req.settlement,
        mark,
        Some(curve),
        req.params.quote_frequency,
    )?;
    let dirty = Decimal::from_f64_retain(priced.dirty_price_per_100)
        .ok_or_else(|| DispatchError::analytics("non-finite dirty price"))?;
    let clean = Decimal::from_f64_retain(priced.clean_price_per_100)
        .ok_or_else(|| DispatchError::analytics("non-finite clean price"))?;

    match req.spread_type {
        SpreadType::ZSpread | SpreadType::Credit => {
            let calc = ZSpreadCalculator::new(curve);
            let s = calc.calculate(bond, dirty, req.settlement)?;
            let dv01 = calc.spread_dv01(bond, s, req.settlement)?;
            Ok(bps_only(dec_to_f64(s.as_bps()), Some(dec_to_f64(dv01))))
        }
        SpreadType::ISpread => {
            let s = ISpreadCalculator::new(curve).calculate(
                bond,
                yield_typed(priced.ytm_decimal, req.params.quote_frequency)?,
                req.settlement,
            )?;
            Ok(bps_only(dec_to_f64(s.as_bps()), None))
        }
        SpreadType::GSpread => {
            let supplied = req.params.government.as_ref().ok_or_else(|| {
                DispatchError::input_field(
                    "params.government",
                    "G-spread requires actual government benchmarks",
                )
            })?;
            let gov_curve = convex_analytics::bond_quote::government_curve(
                supplied,
                req.settlement,
                bond.currency(),
            )?;
            let maturity = bond
                .maturity()
                .ok_or_else(|| DispatchError::input("bond maturity required"))?;
            let benchmark = gov_curve.supplied_yield(req.settlement, maturity)?;
            let s = GSpreadCalculator::new(&gov_curve)
                .with_benchmark(convex_analytics::spreads::BenchmarkSpec::ExplicitYield(
                    benchmark,
                ))
                .calculate(
                    bond,
                    yield_typed(priced.ytm_decimal, req.params.quote_frequency)?,
                    req.settlement,
                )?;
            Ok(bps_only(dec_to_f64(s.as_bps()), None))
        }
        SpreadType::AssetSwapPar => {
            let s = ParParAssetSwap::new(curve).calculate(
                bond,
                Price::from_decimal(clean, bond.currency()),
                req.settlement,
            )?;
            Ok(bps_only(dec_to_f64(s.as_bps()), None))
        }
        SpreadType::AssetSwapProceeds => {
            let zero_curve = clone_typed_curve(req.curve)?;
            let s = ProceedsAssetSwap::new(&zero_curve).calculate(
                bond,
                Price::from_decimal(clean, bond.currency()),
                req.settlement,
            )?;
            Ok(bps_only(dec_to_f64(s.as_bps()), None))
        }
        SpreadType::OAS | SpreadType::DiscountMargin => unreachable!("routed in spread_inner"),
    }
}

// OAS via HW1F trinomial. Effective dur/cvx are ±1bp parallel curve shifts at
// constant OAS (not spread DV01).
fn spread_oas(req: &SpreadRequest, mark: &Mark) -> Result<SpreadResponse, DispatchError> {
    let typed_curve = clone_typed_curve(req.curve)?;
    let vol = req.params.volatility.ok_or_else(|| {
        DispatchError::input_field(
            "params.volatility",
            "OAS requires short-rate volatility (decimal)",
        )
    })?;

    let dirty = with_callable(req.bond, |cb| {
        let p = price_from_mark(
            cb.base_bond(),
            req.settlement,
            mark,
            Some(&typed_curve),
            Frequency::SemiAnnual,
        )?;
        Ok::<f64, DispatchError>(p.dirty_price_per_100)
    })??;

    with_callable(req.bond, |cb| {
        let calc = OASCalculator::default_hull_white(vol);
        let dirty_dec = Decimal::from_f64_retain(dirty)
            .ok_or_else(|| DispatchError::analytics("non-finite dirty price"))?;
        let oas = calc.calculate(cb, dirty_dec, &typed_curve, req.settlement)?;
        let oas_decimal = dec_to_f64(oas.as_decimal());

        let inner = typed_curve.inner();
        let curve_up = RateCurve::new(ParallelBump::new(1.0).apply(inner));
        let curve_dn = RateCurve::new(ParallelBump::new(-1.0).apply(inner));

        let p0 = calc.price_with_oas(cb, &typed_curve, oas_decimal, req.settlement)?;
        let p_up = calc.price_with_oas(cb, &curve_up, oas_decimal, req.settlement)?;
        let p_dn = calc.price_with_oas(cb, &curve_dn, oas_decimal, req.settlement)?;
        let dy = 1e-4;
        let eff_dur = (p_dn - p_up) / (2.0 * p0 * dy);
        let eff_cvx = (p_up + p_dn - 2.0 * p0) / (p0 * dy * dy);

        // Bullet PV at OAS minus callable PV — same curve and OAS, so the
        // optionality cost is isolated cleanly.
        let opt_val = calc.option_value(cb, &typed_curve, oas_decimal, req.settlement)?;

        Ok::<SpreadResponse, DispatchError>(SpreadResponse {
            spread_bps: dec_to_f64(oas.as_bps()),
            spread_dv01: None,
            spread_duration: None,
            option_value: Some(opt_val),
            effective_duration: Some(eff_dur),
            effective_convexity: Some(eff_cvx),
        })
    })?
}

fn spread_dm(_req: &SpreadRequest) -> Result<SpreadResponse, DispatchError> {
    missing_frn_fixings()
}

// ---- cashflows -----------------------------------------------------------

pub fn cashflows(request_json: &str) -> String {
    with_resolved_refs(request_json, CASHFLOWS_REFS, cashflows_inner)
}

fn cashflows_inner(request: serde_json::Value) -> Result<CashflowResponse, DispatchError> {
    let req: CashflowRequest = serde_json::from_value(request)
        .map_err(|e| DispatchError::input(format!("CashflowRequest: {e}")))?;

    let to_entries = |bond: &dyn Bond| -> CashflowResponse {
        CashflowResponse {
            flows: bond
                .cash_flows(req.settlement)
                .into_iter()
                .map(|cf| CashflowEntry {
                    date: cf.date,
                    amount: cf.amount.try_into().unwrap_or(0.0),
                    kind: cashflow_kind_tag(cf.flow_type).to_string(),
                })
                .collect(),
        }
    };

    match registry::kind_of(req.bond) {
        Some(ObjectKind::Bond(BondKind::FloatingRate)) => missing_frn_fixings(),
        Some(ObjectKind::Bond(BondKind::ZeroCoupon)) => with_zero(req.bond, |z| to_entries(z)),
        _ => with_fixed_bond!(req.bond, bond, Ok(to_entries(bond))),
    }
}

// ---- make_whole ---------------------------------------------------------

pub fn make_whole(request_json: &str) -> String {
    with_resolved_refs(request_json, MAKE_WHOLE_REFS, make_whole_inner)
}

fn make_whole_inner(request: serde_json::Value) -> Result<MakeWholeResponse, DispatchError> {
    let req: MakeWholeRequest = serde_json::from_value(request)
        .map_err(|e| DispatchError::input(format!("MakeWholeRequest: {e}")))?;

    if !req.treasury_rate.is_finite() {
        return Err(DispatchError::input_field(
            "treasury_rate",
            "must be finite",
        ));
    }

    with_callable(req.bond, |cb| {
        let spread_bps = cb.make_whole_spread().ok_or_else(|| {
            DispatchError::input_field(
                "bond",
                "callable bond has no make-whole spread on its call schedule",
            )
        })?;
        let price = cb
            .make_whole_call_price(req.call_date, req.treasury_rate)
            .map_err(|e| DispatchError::analytics(e.to_string()))?;
        let price_f64 = dec_to_f64(price);
        let discount_rate = req.treasury_rate + spread_bps / 10_000.0;
        if !spread_bps.is_finite() || !discount_rate.is_finite() || !price_f64.is_finite() {
            return Err(DispatchError::analytics(
                "non-finite make-whole result (spread_bps / discount_rate / price)",
            ));
        }
        Ok::<MakeWholeResponse, DispatchError>(MakeWholeResponse {
            price: price_f64,
            discount_rate,
            spread_bps,
        })
    })?
}

// ---- risk profile / hedge advisor ----------------------------------------

/// Build a position `RiskProfile`. Response is the profile JSON itself.
pub fn risk_profile(request_json: &str) -> String {
    with_resolved_refs(request_json, RISK_PROFILE_REFS, risk_profile_inner)
}

fn risk_profile_inner(request: serde_json::Value) -> Result<RiskProfile, DispatchError> {
    let req: RiskProfileRequest = serde_json::from_value(request)
        .map_err(|e| DispatchError::input(format!("RiskProfileRequest: {e}")))?;
    let mark = parse_mark(&req.mark)?;
    let curve = clone_typed_curve(req.curve)?;
    let tenors: Option<&[f64]> = if req.key_rate_tenors.is_empty() {
        None
    } else {
        Some(&req.key_rate_tenors)
    };

    match registry::kind_of(req.bond) {
        Some(ObjectKind::Bond(BondKind::Callable)) => {
            let vol = req.volatility.ok_or_else(|| {
                DispatchError::input_field(
                    "volatility",
                    "callable position risk requires short-rate volatility (decimal)",
                )
            })?;
            with_callable(req.bond, |cb| {
                compute_callable_position_risk(
                    cb,
                    req.settlement,
                    &mark,
                    req.notional_face,
                    &curve,
                    &req.curve_id,
                    req.quote_frequency,
                    tenors,
                    req.position_id.clone(),
                    vol,
                )
                .map_err(DispatchError::from)
            })?
        }
        Some(ObjectKind::Bond(BondKind::FloatingRate)) => Err(DispatchError::input(
            "position risk is not defined for FRNs (no YTM); use convex_risk for DM-space metrics",
        )),
        Some(ObjectKind::Bond(BondKind::ZeroCoupon)) => Err(DispatchError::input(
            "position risk is not yet wired for zero-coupon bonds",
        )),
        // FixedRate + full SinkingFund cash. Curve/None handled by the macro.
        _ => with_fixed_bond!(req.bond, bond, {
            compute_position_risk(
                bond,
                req.settlement,
                &mark,
                req.notional_face,
                &curve,
                &req.curve_id,
                req.quote_frequency,
                tenors,
                req.position_id.clone(),
            )
            .map_err(DispatchError::from)
        }),
    }
}

/// Propose a hedge for a position profile. Response is a `HedgeProposal`.
pub fn hedge(request_json: &str) -> String {
    with_resolved_refs(request_json, HEDGE_REFS, hedge_inner)
}

fn hedge_inner(request: serde_json::Value) -> Result<HedgeProposal, DispatchError> {
    let req: HedgeRequest = serde_json::from_value(request)
        .map_err(|e| DispatchError::input(format!("HedgeRequest: {e}")))?;
    let curve = clone_typed_curve(req.curve)?;
    // cost_feed = None → strategies fall back to the built-in heuristic model.
    let proposal = match req.strategy {
        HedgeStrategyKind::DurationFutures => duration_futures(
            &req.position,
            &req.constraints,
            &curve,
            &req.curve_id,
            req.settlement,
            &req.basket_overrides,
            None,
        ),
        HedgeStrategyKind::BarbellFutures => barbell_futures(
            &req.position,
            &req.constraints,
            &curve,
            &req.curve_id,
            req.settlement,
            &req.basket_overrides,
            None,
        ),
        HedgeStrategyKind::KeyRateFutures => key_rate_futures(
            &req.position,
            &req.constraints,
            &curve,
            &req.curve_id,
            req.settlement,
            &req.basket_overrides,
            None,
        ),
        HedgeStrategyKind::CashBondPair => cash_bond_pair(
            &req.position,
            &req.constraints,
            &curve,
            &req.curve_id,
            req.settlement,
            None,
        ),
        HedgeStrategyKind::InterestRateSwap => interest_rate_swap(
            &req.position,
            &req.constraints,
            &curve,
            &req.curve_id,
            req.settlement,
            None,
        ),
    }?;
    Ok(proposal)
}

/// Compare proposals against a position. Response is a `CompareResponse`.
pub fn compare(request_json: &str) -> String {
    to_envelope(compare_inner(request_json))
}

fn compare_inner(request_json: &str) -> Result<CompareResponse, DispatchError> {
    let req: CompareRequest = serde_json::from_str(request_json)
        .map_err(|e| DispatchError::input(format!("CompareRequest: {e}")))?;
    let report = compare_hedges(&req.position, &req.proposals, &req.constraints)?;
    let narrative = if req.narrate {
        Some(narrate(&report))
    } else {
        None
    };
    Ok(CompareResponse { report, narrative })
}

// ---- curve_query --------------------------------------------------------

// ---- YAS -------------------------------------------------------------------

pub fn yas(request_json: &str) -> String {
    with_resolved_refs(request_json, YAS_REFS, yas_inner)
}

/// One-call YAS analysis: the mark resolves to a clean price via the same
/// `price_from_mark` path `convex_price` uses, then the YAS engine produces
/// yields, spreads, risk, and the settlement invoice in one computation.
fn yas_inner(request: serde_json::Value) -> Result<YasResponse, DispatchError> {
    use convex_analytics::yas::YASCalculator;

    let req: YasRequest = serde_json::from_value(request)
        .map_err(|e| DispatchError::input(format!("YasRequest: {e}")))?;
    let mark = parse_mark(&req.mark)?;

    // This convenience request cannot bind government benchmark securities.
    if matches!(&mark, Mark::Spread { value, .. } if value.spread_type() == SpreadType::GSpread) {
        return Err(DispatchError::input_field(
            "mark",
            "G-spread marks are not admitted here; library government_spread calculates spreads from supplied securities",
        ));
    }
    let spot = clone_typed_curve(req.curve)?;
    let swap = match req.swap_curve {
        Some(h) if h != 0 => Some(clone_typed_curve(h)?),
        _ => None,
    };
    let price_curve: &dyn RateCurveDyn = match &mark {
        Mark::Spread { value, .. } if value.spread_type() == SpreadType::ISpread => swap
            .as_ref()
            .map(|s| s as &dyn RateCurveDyn)
            .unwrap_or(&spot),
        _ => &spot,
    };

    with_fixed_bond!(req.bond, bond, {
        let priced = price_from_mark(
            bond,
            req.settlement,
            &mark,
            Some(price_curve),
            req.quote_frequency,
        )?;
        let clean = Decimal::from_f64_retain(priced.clean_price_per_100)
            .ok_or_else(|| DispatchError::analytics("non-finite clean price"))?;

        let mut calc = YASCalculator::new(&spot);
        if let Some(s) = &swap {
            calc = calc.with_swap_curve(s);
        }
        calc = calc.with_frequency(req.quote_frequency.periods_per_year());

        let r = calc
            .analyze(bond, req.settlement.into(), clean)
            .map_err(|e| DispatchError::analytics(format!("YAS: {e}")))?;

        Ok(YasResponse {
            clean_price: priced.clean_price_per_100,
            dirty_price: dec_to_f64(r.invoice.dirty_price),
            accrued: dec_to_f64(r.invoice.accrued_interest),
            accrued_days: r.invoice.accrued_days,
            ytm_pct: dec_to_f64(r.ytm),
            current_yield_pct: dec_to_f64(r.current_yield),
            simple_yield_pct: dec_to_f64(r.simple_yield),
            money_market_yield_pct: r.money_market_yield.map(dec_to_f64),
            z_spread_bps: dec_to_f64(r.z_spread.as_bps()),
            modified_duration: dec_to_f64(r.risk.modified_duration.years()),
            macaulay_duration: dec_to_f64(r.risk.macaulay_duration.years()),
            convexity: dec_to_f64(r.risk.convexity.value()),
            dv01_per_100: dec_to_f64(r.risk.dv01_per_100.value()),
            principal_amount: dec_to_f64(r.invoice.principal_amount),
            accrued_amount: dec_to_f64(r.invoice.accrued_amount),
            settlement_amount: dec_to_f64(r.invoice.settlement_amount),
        })
    })
}

// ---- Scenarios --------------------------------------------------------------

pub fn scenario(request_json: &str) -> String {
    with_resolved_refs(request_json, SCENARIO_REFS, scenario_inner)
}

/// One base pricing + Z solve, then one reprice per scenario against the
/// bumped curve holding Z fixed (the KRD methodology, generalized).
fn scenario_inner(request: serde_json::Value) -> Result<ScenarioResponse, DispatchError> {
    use convex_curves::bumping::{Scenario, ScenarioBump};

    let req: ScenarioRequest = serde_json::from_value(request)
        .map_err(|e| DispatchError::input(format!("ScenarioRequest: {e}")))?;
    if req.scenarios.is_empty() {
        return Err(DispatchError::input_field(
            "scenarios",
            "at least one scenario is required",
        ));
    }
    let mark = parse_mark(&req.mark)?;
    let base_inner =
        registry::with_object::<RateCurve<DiscreteCurve>, _, _>(req.curve, |c| c.inner().clone())
            .ok_or_else(|| DispatchError::handle(format!("curve handle {} not found", req.curve)))?;
    let base_wrapper = RateCurve::new(base_inner.clone());

    with_fixed_bond!(req.bond, bond, {
        let priced = price_from_mark(
            bond,
            req.settlement,
            &mark,
            Some(&base_wrapper),
            req.quote_frequency,
        )?;
        let dirty = Decimal::from_f64_retain(priced.dirty_price_per_100)
            .ok_or_else(|| DispatchError::analytics("non-finite dirty price"))?;
        let z = ZSpreadCalculator::new(&base_wrapper).calculate(bond, dirty, req.settlement)?;
        let z_decimal = dec_to_f64(z.as_decimal());
        let accrued = priced.dirty_price_per_100 - priced.clean_price_per_100;

        // Anchor the ladder at the mark: absorb the sub-cent Z-solve residual
        // so the zero-shift row reproduces the marked price exactly.
        let dirty0 = ZSpreadCalculator::new(&base_wrapper).price_with_spread(
            bond,
            z_decimal,
            req.settlement,
        );
        let basis = priced.dirty_price_per_100 - dirty0;

        let mut rows = Vec::with_capacity(req.scenarios.len());
        for (i, spec) in req.scenarios.iter().enumerate() {
            let mut sc = Scenario::new(
                spec.name
                    .clone()
                    .unwrap_or_else(|| format!("scenario {}", i + 1)),
            );
            for b in &spec.bumps {
                sc = sc.with_bump(match *b {
                    ScenarioBumpSpec::Parallel { shift_bps } => ScenarioBump::parallel(shift_bps),
                    ScenarioBumpSpec::Steepener {
                        short_shift_bps,
                        long_shift_bps,
                        pivot_tenor,
                    } => ScenarioBump::steepener(short_shift_bps, long_shift_bps, pivot_tenor),
                    ScenarioBumpSpec::Flattener {
                        short_shift_bps,
                        long_shift_bps,
                        pivot_tenor,
                    } => ScenarioBump::flattener(short_shift_bps, long_shift_bps, pivot_tenor),
                    ScenarioBumpSpec::KeyRate { tenor, shift_bps } => {
                        ScenarioBump::key_rate(tenor, shift_bps)
                    }
                    ScenarioBumpSpec::CreditSpread { shift_bps } => {
                        ScenarioBump::credit_spread(shift_bps)
                    }
                });
            }
            let bumped = RateCurve::new(sc.apply(&base_inner));
            let dirty_s =
                ZSpreadCalculator::new(&bumped).price_with_spread(bond, z_decimal, req.settlement)
                    + basis;
            let clean_s = dirty_s - accrued;
            // Yield at the scenario price, for the ladder display.
            let ytm_s = price_from_mark(
                bond,
                req.settlement,
                &Mark::Price {
                    value: Decimal::from_f64_retain(clean_s)
                        .ok_or_else(|| DispatchError::analytics("non-finite scenario price"))?,
                    kind: convex_core::types::PriceKind::Clean,
                },
                None,
                req.quote_frequency,
            )
            .map(|p| p.ytm_decimal)
            .unwrap_or(f64::NAN);
            rows.push(ScenarioRow {
                name: sc.name().to_string(),
                clean_price: clean_s,
                dirty_price: dirty_s,
                delta_clean: clean_s - priced.clean_price_per_100,
                ytm_decimal: ytm_s,
            });
        }

        Ok(ScenarioResponse {
            base_clean: priced.clean_price_per_100,
            base_ytm_decimal: priced.ytm_decimal,
            z_spread_bps: dec_to_f64(z.as_bps()),
            rows,
        })
    })
}

pub fn curve_query(request_json: &str) -> String {
    with_resolved_refs(request_json, CURVE_QUERY_REFS, curve_query_inner)
}

fn curve_query_inner(request: serde_json::Value) -> Result<CurveQueryResponse, DispatchError> {
    let req: CurveQueryRequest = serde_json::from_value(request)
        .map_err(|e| DispatchError::input(format!("CurveQueryRequest: {e}")))?;

    let result = registry::with_object::<RateCurve<DiscreteCurve>, _, _>(req.curve, |c| match req
        .query
    {
        CurveQueryKind::Zero => c
            .zero_rate_at_tenor(req.tenor, Compounding::Continuous)
            .map_err(|e| DispatchError::analytics(format!("zero_rate query failed: {e}"))),
        CurveQueryKind::Df => c
            .discount_factor_at_tenor(req.tenor)
            .map_err(|e| DispatchError::analytics(format!("discount_factor query failed: {e}"))),
        CurveQueryKind::Forward => {
            let end = req.tenor_end.unwrap_or(req.tenor + 0.25);
            c.forward_rate_at_tenors(req.tenor, end, Compounding::Continuous)
                .map_err(|e| DispatchError::analytics(format!("forward_rate query failed: {e}")))
        }
    });

    match result {
        Some(Ok(v)) => Ok(CurveQueryResponse { value: v }),
        Some(Err(e)) => Err(e),
        None => Err(DispatchError::handle(format!(
            "curve handle {} not found",
            req.curve
        ))),
    }
}

// ---- helpers -------------------------------------------------------------

fn cashflow_kind_tag(t: CashFlowType) -> &'static str {
    match t {
        CashFlowType::Coupon => "coupon",
        CashFlowType::Principal => "redemption",
        CashFlowType::CouponAndPrincipal => "coupon-and-redemption",
        CashFlowType::Fee => "fee",
    }
}

fn dec_to_f64(d: Decimal) -> f64 {
    d.try_into().unwrap_or(0.0)
}

fn yield_typed(decimal: f64, frequency: Frequency) -> Result<Yield, DispatchError> {
    let value = Decimal::from_f64_retain(decimal)
        .ok_or_else(|| DispatchError::analytics("yield is nonfinite or outside decimal range"))?;
    let compounding = Compounding::try_from_periods_per_year(frequency.periods_per_year())
        .ok_or_else(|| DispatchError::input_field("quote_frequency", "unsupported frequency"))?;
    Ok(Yield::new(value, compounding))
}

fn parse_mark(input: &MarkInput) -> Result<Mark, DispatchError> {
    match input {
        MarkInput::Parsed(m) => Ok(m.clone()),
        MarkInput::Text(s) => s
            .parse::<Mark>()
            .map_err(|e| DispatchError::input_field("mark", e.to_string())),
    }
}

type CurveBox = Box<dyn RateCurveDyn>;

fn optional_curve(handle: Option<Handle>) -> Result<Option<CurveBox>, DispatchError> {
    match handle {
        None | Some(0) => Ok(None),
        Some(h) => clone_curve(h).map(Some),
    }
}

fn required_curve(handle: Handle) -> Result<CurveBox, DispatchError> {
    if handle == 0 {
        return Err(DispatchError::input_field(
            "curve",
            "curve handle is required",
        ));
    }
    clone_curve(handle)
}

fn clone_curve(handle: Handle) -> Result<CurveBox, DispatchError> {
    registry::with_object::<RateCurve<DiscreteCurve>, _, _>(handle, |c| {
        Box::new(c.clone()) as CurveBox
    })
    .ok_or_else(|| DispatchError::handle(format!("curve handle {handle} not found")))
}

fn clone_typed_curve(handle: Handle) -> Result<RateCurve<DiscreteCurve>, DispatchError> {
    registry::with_object::<RateCurve<DiscreteCurve>, _, _>(handle, |c| c.clone())
        .ok_or_else(|| DispatchError::handle(format!("curve handle {handle} not found")))
}

// ---- typed-bond access ---------------------------------------------------

fn with_zero<R>(handle: Handle, f: impl FnOnce(&ZeroCouponBond) -> R) -> Result<R, DispatchError> {
    registry::with_object::<ZeroCouponBond, _, _>(handle, f)
        .ok_or_else(|| DispatchError::handle(format!("zero-coupon handle {handle} not found")))
}

fn with_callable<R>(
    handle: Handle,
    f: impl FnOnce(&CallableBond) -> R,
) -> Result<R, DispatchError> {
    registry::with_object::<CallableBond, _, _>(handle, f)
        .ok_or_else(|| DispatchError::handle(format!("callable handle {handle} not found")))
}

/// Look up `handle`, dispatch to the matching fixed-coupon bond struct, and
/// run `body` with `bond` bound to a borrowed `&dyn Bond + FixedCouponBond`.
/// FRN and ZeroCoupon errors out — those have dedicated dispatchers.
macro_rules! with_fixed_bond {
    ($handle:expr, $bond:ident, $body:expr) => {{
        let h = $handle;
        match registry::kind_of(h) {
            Some(ObjectKind::Bond(BondKind::FixedRate)) => {
                registry::with_object::<FixedRateBond, _, _>(h, |$bond| $body).ok_or_else(|| {
                    DispatchError::handle(format!("bond handle {} downcast failed", h))
                })?
            }
            Some(ObjectKind::Bond(BondKind::Callable)) => {
                registry::with_object::<CallableBond, _, _>(h, |cb| {
                    let $bond = cb.base_bond();
                    $body
                })
                .ok_or_else(|| {
                    DispatchError::handle(format!("bond handle {} downcast failed", h))
                })?
            }
            Some(ObjectKind::Bond(BondKind::SinkingFund)) => {
                registry::with_object::<SinkingFundBond, _, _>(h, |sb| {
                    let $bond = sb;
                    $body
                })
                .ok_or_else(|| {
                    DispatchError::handle(format!("bond handle {} downcast failed", h))
                })?
            }
            Some(ObjectKind::Bond(BondKind::FloatingRate)) => {
                return Err(DispatchError::handle(format!(
                    "FRN analytics route through dedicated FRN paths (handle {h})"
                )))
            }
            Some(ObjectKind::Bond(BondKind::ZeroCoupon)) => {
                return Err(DispatchError::handle(format!(
                    "zero-coupon analytics route through dedicated zero paths (handle {h})"
                )))
            }
            Some(ObjectKind::Curve) => {
                return Err(DispatchError::handle(format!(
                    "handle {h} is a curve, not a bond"
                )))
            }
            None => return Err(DispatchError::handle(format!("bond handle {h} not found"))),
        }
    }};
}
pub(crate) use with_fixed_bond;

/// Helper: build a `SpreadResponse` carrying only the headline bps + an
/// optional spread DV01. Used for spread families that don't have effective
/// duration / option-value fields.
fn bps_only(spread_bps: f64, spread_dv01: Option<f64>) -> SpreadResponse {
    SpreadResponse {
        spread_bps,
        spread_dv01,
        spread_duration: None,
        option_value: None,
        effective_duration: None,
        effective_convexity: None,
    }
}

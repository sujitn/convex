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

use std::sync::Arc;

use convex_analytics::dto::{
    CashflowEntry, CashflowRequest, CashflowResponse, CurveQueryKind, CurveQueryRequest,
    CurveQueryResponse, KeyRate, MakeWholeRequest, MakeWholeResponse, MarkInput, PricingRequest,
    PricingResponse, RiskRequest, RiskResponse, SpreadRequest, SpreadResponse,
};
use convex_analytics::pricing::price_from_mark;
use convex_analytics::spreads::{
    DiscountMarginCalculator, GSpreadCalculator, ISpreadCalculator, OASCalculator, ParParAssetSwap,
    ProceedsAssetSwap, ZSpreadCalculator,
};
use convex_bonds::instruments::{
    CallableBond, FixedRateBond, FloatingRateNote, SinkingFundBond, ZeroCouponBond,
};
use convex_bonds::traits::{Bond, CashFlowType, FixedCouponBond};
use convex_core::types::{Compounding, Frequency, Mark, Price, SpreadType, Yield};
use convex_curves::bumping::{KeyRateBump, ParallelBump};
use convex_curves::curves::ForwardCurve;
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
    to_envelope(price_inner(request_json))
}

fn price_inner(request_json: &str) -> Result<PricingResponse, DispatchError> {
    let req: PricingRequest = serde_json::from_str(request_json)
        .map_err(|e| DispatchError::input(format!("PricingRequest: {e}")))?;
    let mark = parse_mark(&req.mark)?;

    match registry::kind_of(req.bond) {
        Some(ObjectKind::Bond(BondKind::FloatingRate)) => price_frn(&req, &mark),
        Some(ObjectKind::Bond(BondKind::ZeroCoupon)) => price_zero(&req, &mark),
        _ => price_fixed_coupon(&req, &mark),
    }
}

fn price_fixed_coupon(req: &PricingRequest, mark: &Mark) -> Result<PricingResponse, DispatchError> {
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

fn price_frn(req: &PricingRequest, mark: &Mark) -> Result<PricingResponse, DispatchError> {
    let accrued = with_frn(req.bond, |f| dec_to_f64(f.accrued_interest(req.settlement)))?;

    match mark {
        Mark::Price { value, kind } => {
            let v = dec_to_f64(*value);
            let (clean, dirty) = match kind {
                convex_core::types::PriceKind::Clean => (v, v + accrued),
                convex_core::types::PriceKind::Dirty => (v - accrued, v),
            };
            Ok(PricingResponse {
                clean_price: clean,
                dirty_price: dirty,
                accrued,
                ytm_decimal: f64::NAN,
                z_spread_bps: None,
            })
        }
        Mark::Spread { value, .. } if value.spread_type() == SpreadType::DiscountMargin => {
            let discount_handle = req.curve.ok_or_else(|| {
                DispatchError::input_field("curve", "FRN DM mark requires a discount curve")
            })?;
            let discount = required_curve(discount_handle)?;
            let forward_curve = build_forward_curve(req.forward_curve.unwrap_or(discount_handle))?;
            let dm_decimal = dec_to_f64(value.as_decimal());
            let dirty = with_frn(req.bond, |frn| {
                DiscountMarginCalculator::new(&forward_curve, &*discount).price_with_dm(
                    frn,
                    dm_decimal,
                    req.settlement,
                )
            })?;
            Ok(PricingResponse {
                clean_price: dirty - accrued,
                dirty_price: dirty,
                accrued,
                ytm_decimal: f64::NAN,
                z_spread_bps: None,
            })
        }
        Mark::Yield { .. } => Err(DispatchError::input_field(
            "mark",
            "FRN: yield marks not defined; pass a price or DM spread",
        )),
        Mark::Spread { value, .. } => Err(DispatchError::input_field(
            "mark",
            format!(
                "FRN: only DiscountMargin spread marks supported (got {:?})",
                value.spread_type()
            ),
        )),
    }
}

fn price_zero(req: &PricingRequest, mark: &Mark) -> Result<PricingResponse, DispatchError> {
    with_zero(req.bond, |z| match mark {
        Mark::Price { value, .. } => {
            let yld = z.yield_from_price(*value, req.settlement);
            Ok(PricingResponse {
                clean_price: dec_to_f64(*value),
                dirty_price: dec_to_f64(*value),
                accrued: 0.0,
                ytm_decimal: dec_to_f64(yld),
                z_spread_bps: None,
            })
        }
        Mark::Yield { value, .. } => {
            let p = z.price_from_yield(*value, req.settlement);
            Ok(PricingResponse {
                clean_price: dec_to_f64(p),
                dirty_price: dec_to_f64(p),
                accrued: 0.0,
                ytm_decimal: dec_to_f64(*value),
                z_spread_bps: None,
            })
        }
        Mark::Spread { .. } => Err(DispatchError::input_field(
            "mark",
            "Zero-coupon: spread marks not supported (use a price or yield)",
        )),
    })?
}

// ---- risk ----------------------------------------------------------------

pub fn risk(request_json: &str) -> String {
    to_envelope(risk_inner(request_json))
}

fn risk_inner(request_json: &str) -> Result<RiskResponse, DispatchError> {
    let req: RiskRequest = serde_json::from_str(request_json)
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
    use convex_analytics::functions::{convexity, dv01, macaulay_duration, modified_duration};

    let priced = price_from_mark(bond, req.settlement, mark, curve, req.quote_frequency)?;
    let ytm = priced.ytm_decimal;
    let modified_dur = modified_duration(bond, req.settlement, ytm, req.quote_frequency)?;
    let macaulay_dur = macaulay_duration(bond, req.settlement, ytm, req.quote_frequency)?;
    let cvx = convexity(bond, req.settlement, ytm, req.quote_frequency)?;
    let dv01_val = dv01(
        bond,
        req.settlement,
        ytm,
        priced.dirty_price_per_100,
        req.quote_frequency,
    )?;

    let key_rates = if req.key_rate_tenors.is_empty() {
        vec![]
    } else {
        compute_krd(bond, mark, req)?
    };

    Ok(RiskResponse {
        modified_duration: modified_dur,
        macaulay_duration: macaulay_dur,
        convexity: cvx,
        dv01: dv01_val,
        spread_duration: priced.z_spread_bps.map(|_| modified_dur),
        key_rates,
    })
}

// FRN risk lives in DM-space: spread duration / DV01 against the DM mark.
fn risk_frn(req: &RiskRequest, mark: &Mark) -> Result<RiskResponse, DispatchError> {
    let Mark::Spread { value, .. } = mark else {
        return Err(DispatchError::input_field(
            "mark",
            "FRN risk requires a DM spread mark (e.g. \"75 DM@USD.SOFR\")",
        ));
    };
    if value.spread_type() != SpreadType::DiscountMargin {
        return Err(DispatchError::input_field(
            "mark",
            "FRN risk requires a DiscountMargin spread mark",
        ));
    }
    let discount_handle = req
        .curve
        .ok_or_else(|| DispatchError::input_field("curve", "FRN risk requires a discount curve"))?;
    let discount = required_curve(discount_handle)?;
    let forward_curve = build_forward_curve(req.forward_curve.unwrap_or(discount_handle))?;
    let dm = *value;

    with_frn(req.bond, |frn| {
        let calc = DiscountMarginCalculator::new(&forward_curve, &*discount);
        let dv01 = calc.spread_dv01(frn, dm, req.settlement);
        let dur = calc.spread_duration(frn, dm, req.settlement);
        let eff_dur = calc.effective_duration(frn, dm, req.settlement, 0.0001);
        RiskResponse {
            modified_duration: dec_to_f64(eff_dur),
            macaulay_duration: f64::NAN,
            convexity: f64::NAN,
            dv01: dec_to_f64(dv01),
            spread_duration: Some(dec_to_f64(dur)),
            key_rates: vec![],
        }
    })
}

// Closed-form: macaulay = years to maturity; modified = mac/(1+y/f);
// convexity = T·(T+1/f)/(1+y/f)²; DV01 = mod·P·1e-4.
fn risk_zero(req: &RiskRequest, mark: &Mark) -> Result<RiskResponse, DispatchError> {
    with_zero(req.bond, |z| -> Result<RiskResponse, DispatchError> {
        let (clean, ytm) = match mark {
            Mark::Price { value, .. } => {
                let y = z.yield_from_price(*value, req.settlement);
                (dec_to_f64(*value), dec_to_f64(y))
            }
            Mark::Yield { value, .. } => {
                let p = z.price_from_yield(*value, req.settlement);
                (dec_to_f64(p), dec_to_f64(*value))
            }
            Mark::Spread { .. } => {
                return Err(DispatchError::input_field(
                    "mark",
                    "Zero-coupon: spread marks not supported",
                ));
            }
        };
        let years = req.settlement.days_between(&z.maturity_date()) as f64 / 365.0;
        let f = z.compounding().periods_per_year_opt().unwrap_or(2) as f64;
        let denom = 1.0 + ytm / f;
        let mod_dur = years / denom;
        let convexity = years * (years + 1.0 / f) / denom.powi(2);
        let dv01 = mod_dur * clean * 1e-4;
        Ok(RiskResponse {
            modified_duration: mod_dur,
            macaulay_duration: years,
            convexity,
            dv01,
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
    let base_inner = registry::with_object::<RateCurve<DiscreteCurve>, _, _>(curve_handle, |c| {
        c.inner().clone()
    })
    .ok_or_else(|| DispatchError::handle(format!("curve handle {curve_handle} not found")))?;
    let base_wrapper = RateCurve::new(base_inner.clone());

    let priced = price_from_mark(
        bond,
        req.settlement,
        mark,
        Some(&base_wrapper),
        req.quote_frequency,
    )?;
    let dirty = Decimal::from_f64_retain(priced.dirty_price_per_100)
        .ok_or_else(|| DispatchError::analytics("non-finite dirty price"))?;
    let z = ZSpreadCalculator::new(&base_wrapper).calculate(bond, dirty, req.settlement)?;
    let z_decimal = dec_to_f64(z.as_decimal());
    // Bloomberg KRD divides by dirty (numerator already cancels accrued).
    let p0_dirty = priced.dirty_price_per_100;

    let bump_bps = 1.0;
    let dy = bump_bps * 1e-4;
    let mut out = Vec::with_capacity(req.key_rate_tenors.len());
    for &tenor in &req.key_rate_tenors {
        let up = RateCurve::new(KeyRateBump::new(tenor, bump_bps).apply(&base_inner));
        let dn = RateCurve::new(KeyRateBump::new(tenor, -bump_bps).apply(&base_inner));
        let dirty_up =
            ZSpreadCalculator::new(&up).price_with_spread(bond, z_decimal, req.settlement);
        let dirty_dn =
            ZSpreadCalculator::new(&dn).price_with_spread(bond, z_decimal, req.settlement);
        let krd = (dirty_dn - dirty_up) / (2.0 * p0_dirty * dy);
        out.push(KeyRate {
            tenor,
            duration: krd,
        });
    }
    Ok(out)
}

// ---- spread --------------------------------------------------------------

pub fn spread(request_json: &str) -> String {
    to_envelope(spread_inner(request_json))
}

fn spread_inner(request_json: &str) -> Result<SpreadResponse, DispatchError> {
    let req: SpreadRequest = serde_json::from_str(request_json)
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
        Frequency::SemiAnnual,
    )?;
    let dirty = Decimal::from_f64_retain(priced.dirty_price_per_100)
        .ok_or_else(|| DispatchError::analytics("non-finite dirty price"))?;
    let clean = Decimal::from_f64_retain(priced.clean_price_per_100)
        .ok_or_else(|| DispatchError::analytics("non-finite clean price"))?;

    match req.spread_type {
        SpreadType::ZSpread | SpreadType::Credit => {
            let calc = ZSpreadCalculator::new(curve);
            let s = calc.calculate(bond, dirty, req.settlement)?;
            let dv01 = calc.spread_dv01(bond, s, req.settlement);
            Ok(bps_only(dec_to_f64(s.as_bps()), Some(dec_to_f64(dv01))))
        }
        SpreadType::ISpread => {
            let s = ISpreadCalculator::new(curve).calculate(
                bond,
                yield_typed(priced.ytm_decimal),
                req.settlement,
            )?;
            Ok(bps_only(dec_to_f64(s.as_bps()), None))
        }
        SpreadType::GSpread => {
            let gov_handle = req.params.govt_curve.ok_or_else(|| {
                DispatchError::input_field(
                    "params.govt_curve",
                    "G-spread requires a separate government curve handle",
                )
            })?;
            let gov_curve = build_government_curve(gov_handle)?;
            let s = GSpreadCalculator::new(&gov_curve).calculate(
                bond,
                yield_typed(priced.ytm_decimal),
                req.settlement,
            )?;
            Ok(bps_only(dec_to_f64(s.as_bps()), None))
        }
        SpreadType::AssetSwapPar => {
            let zero_curve = clone_typed_curve(req.curve)?;
            let s = ParParAssetSwap::new(&zero_curve).calculate(
                bond,
                Price::from_decimal(clean, convex_core::types::Currency::USD),
                req.settlement,
            )?;
            Ok(bps_only(dec_to_f64(s.as_bps()), None))
        }
        SpreadType::AssetSwapProceeds => {
            let zero_curve = clone_typed_curve(req.curve)?;
            let s = ProceedsAssetSwap::new(&zero_curve).calculate(
                bond,
                Price::from_decimal(clean, convex_core::types::Currency::USD),
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

fn spread_dm(req: &SpreadRequest) -> Result<SpreadResponse, DispatchError> {
    let discount = required_curve(req.curve)?;
    let forward_curve = build_forward_curve(req.params.forward_curve.unwrap_or(req.curve))?;

    // Simple-margin shortcut: if the user supplied params.current_index,
    // return the closed-form simple margin instead of the iterative DM.
    if let Some(idx) = req.params.current_index {
        return with_frn(req.bond, |frn| {
            let dirty = mark_to_frn_dirty(req, frn)?;
            let dirty_dec = Decimal::from_f64_retain(dirty)
                .ok_or_else(|| DispatchError::analytics("non-finite dirty price"))?;
            let idx_dec = Decimal::from_f64_retain(idx)
                .ok_or_else(|| DispatchError::input_field("params.current_index", "non-finite"))?;
            let s =
                convex_analytics::spreads::simple_margin(frn, dirty_dec, idx_dec, req.settlement);
            Ok::<SpreadResponse, DispatchError>(bps_only(dec_to_f64(s.as_bps()), None))
        })?;
    }

    with_frn(req.bond, |frn| {
        let calc = DiscountMarginCalculator::new(&forward_curve, &*discount);
        let dirty = mark_to_frn_dirty(req, frn)?;
        let dirty_dec = Decimal::from_f64_retain(dirty)
            .ok_or_else(|| DispatchError::analytics("non-finite dirty price"))?;
        let dm = calc.calculate(frn, dirty_dec, req.settlement)?;
        let dv01 = calc.spread_dv01(frn, dm, req.settlement);
        let dur = calc.spread_duration(frn, dm, req.settlement);
        Ok::<SpreadResponse, DispatchError>(SpreadResponse {
            spread_bps: dec_to_f64(dm.as_bps()),
            spread_dv01: Some(dec_to_f64(dv01)),
            spread_duration: Some(dec_to_f64(dur)),
            option_value: None,
            effective_duration: None,
            effective_convexity: None,
        })
    })?
}

fn mark_to_frn_dirty(req: &SpreadRequest, frn: &FloatingRateNote) -> Result<f64, DispatchError> {
    match parse_mark(&req.mark)? {
        Mark::Price { value, kind } => {
            let v = dec_to_f64(value);
            Ok(match kind {
                convex_core::types::PriceKind::Dirty => v,
                convex_core::types::PriceKind::Clean => {
                    v + dec_to_f64(frn.accrued_interest(req.settlement))
                }
            })
        }
        _ => Err(DispatchError::input_field(
            "mark",
            "FRN spread dispatcher accepts only price marks",
        )),
    }
}

// ---- cashflows -----------------------------------------------------------

pub fn cashflows(request_json: &str) -> String {
    to_envelope(cashflows_inner(request_json))
}

fn cashflows_inner(request_json: &str) -> Result<CashflowResponse, DispatchError> {
    let req: CashflowRequest = serde_json::from_str(request_json)
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
        Some(ObjectKind::Bond(BondKind::FloatingRate)) => with_frn(req.bond, |f| to_entries(f)),
        Some(ObjectKind::Bond(BondKind::ZeroCoupon)) => with_zero(req.bond, |z| to_entries(z)),
        _ => with_fixed_bond!(req.bond, bond, Ok(to_entries(bond))),
    }
}

// ---- make_whole ---------------------------------------------------------

pub fn make_whole(request_json: &str) -> String {
    to_envelope(make_whole_inner(request_json))
}

fn make_whole_inner(request_json: &str) -> Result<MakeWholeResponse, DispatchError> {
    let req: MakeWholeRequest = serde_json::from_str(request_json)
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

// ---- curve_query --------------------------------------------------------

pub fn curve_query(request_json: &str) -> String {
    to_envelope(curve_query_inner(request_json))
}

fn curve_query_inner(request_json: &str) -> Result<CurveQueryResponse, DispatchError> {
    let req: CurveQueryRequest = serde_json::from_str(request_json)
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

fn yield_typed(decimal: f64) -> Yield {
    Yield::new(
        Decimal::from_f64_retain(decimal).unwrap_or_default(),
        Compounding::SemiAnnual,
    )
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

fn build_forward_curve(handle: Handle) -> Result<ForwardCurve, DispatchError> {
    let arc: Arc<dyn RateCurveDyn> =
        registry::with_object::<RateCurve<DiscreteCurve>, _, _>(handle, |c| {
            Arc::new(c.clone()) as Arc<dyn RateCurveDyn>
        })
        .ok_or_else(|| DispatchError::handle(format!("forward curve handle {handle} not found")))?;
    Ok(ForwardCurve::new(arc, 0.25))
}

/// Wrap a discount-curve handle as a `GovernmentCurve` whose interpolated
/// yield at any tenor is the curve's zero rate at that tenor. The user is
/// expected to register a real government curve here (UST yields), not a
/// SOFR/swap curve — that's what makes this G-spread different from I-spread.
fn build_government_curve(
    handle: Handle,
) -> Result<convex_analytics::spreads::GovernmentCurve, DispatchError> {
    use convex_analytics::spreads::{GovernmentBenchmark, GovernmentCurve, Sovereign};
    use convex_bonds::types::Tenor;

    let curve = clone_typed_curve(handle)?;
    let ref_date = curve.reference_date();
    let mut g = GovernmentCurve::new(Sovereign::UST, ref_date);
    let standard: &[(Tenor, f64)] = &[
        (Tenor::M3, 0.25),
        (Tenor::M6, 0.5),
        (Tenor::Y1, 1.0),
        (Tenor::Y2, 2.0),
        (Tenor::Y3, 3.0),
        (Tenor::Y5, 5.0),
        (Tenor::Y7, 7.0),
        (Tenor::Y10, 10.0),
        (Tenor::Y20, 20.0),
        (Tenor::Y30, 30.0),
    ];
    for (tenor, t_years) in standard {
        let r = curve.zero_rate_at_tenor(*t_years, Compounding::SemiAnnual)?;
        let maturity = ref_date.add_days((t_years * 365.25) as i64);
        let bench = GovernmentBenchmark::with_cusip_unchecked(
            Sovereign::UST,
            *tenor,
            "GOVT00000",
            maturity,
            Decimal::from_f64_retain(r).unwrap_or_default(),
            Yield::new(
                Decimal::from_f64_retain(r).unwrap_or_default(),
                Compounding::SemiAnnual,
            ),
        );
        g = g.with_benchmark(bench);
    }
    Ok(g)
}

// ---- typed-bond access ---------------------------------------------------

fn with_frn<R>(handle: Handle, f: impl FnOnce(&FloatingRateNote) -> R) -> Result<R, DispatchError> {
    registry::with_object::<FloatingRateNote, _, _>(handle, f)
        .ok_or_else(|| DispatchError::handle(format!("FRN handle {handle} not found")))
}

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
                    let $bond = sb.base_bond();
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

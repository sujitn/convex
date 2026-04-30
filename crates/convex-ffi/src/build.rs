//! Spec → object construction.
//!
//! Each `*_from_json` entry deserializes a `*Spec`, builds the matching
//! library type, and registers it. Failures land in the thread-local
//! last-error slot and the function returns `INVALID_HANDLE`.

use convex_analytics::dto::{
    BondIdentifier, BondSpec, CallableSpec, CouponSpec, CurveInstrument, CurveSpec, CurveValueKind,
    DiscreteCurveSpec, FixedRateSpec, FloatingRateSpec, InterpolationMethodCode, RateIndexCode,
    SinkingFundSpec, ZeroCouponSpec,
};
use convex_bonds::instruments::{
    CallableBond, FixedRateBond, FloatingRateNote, SinkingFundBond, SinkingFundPayment,
    SinkingFundSchedule, ZeroCouponBond,
};
use convex_bonds::types::{CallEntry, CallSchedule, CallType};
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Date, Frequency};
use convex_curves::{
    Deposit, DiscreteCurve, Fra, GlobalFitter, InstrumentSet, InterpolationMethod, Ois, RateCurve,
    Swap, ValueType,
};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use crate::error::set_last_error;
use crate::registry::{self, BondKind, Handle, ObjectKind, INVALID_HANDLE};

pub fn bond_from_json(json: &str) -> Handle {
    let spec: BondSpec = match serde_json::from_str(json) {
        Ok(s) => s,
        Err(e) => {
            set_last_error(format!("invalid BondSpec: {e}"));
            return INVALID_HANDLE;
        }
    };

    match spec {
        BondSpec::FixedRate(s) => build_fixed_rate(s),
        BondSpec::Callable(s) => build_callable(s),
        BondSpec::FloatingRate(s) => build_floating_rate(s),
        BondSpec::ZeroCoupon(s) => build_zero_coupon(s),
        BondSpec::SinkingFund(s) => build_sinking_fund(s),
    }
}

pub fn curve_from_json(json: &str) -> Handle {
    let spec: CurveSpec = match serde_json::from_str(json) {
        Ok(s) => s,
        Err(e) => {
            set_last_error(format!("invalid CurveSpec: {e}"));
            return INVALID_HANDLE;
        }
    };

    match spec {
        CurveSpec::Discrete(s) => build_discrete_curve(s),
        CurveSpec::Bootstrap(s) => build_bootstrap_curve(s),
    }
}

// ---- Bond builders --------------------------------------------------------

fn name_from_id(id: &BondIdentifier) -> Option<String> {
    id.cusip
        .clone()
        .or_else(|| id.isin.clone())
        .or_else(|| id.name.clone())
}

/// Attaches whichever identifier the spec carries to a `FixedRateBondBuilder`.
/// `FixedRateBondBuilder::build()` requires *something* identifying — falling
/// back to `cusip_unchecked(name)` when only a free-form name is supplied
/// keeps the registry-key behaviour and the bond-side invariant in step.
fn apply_identifier_fixed(
    mut b: convex_bonds::instruments::FixedRateBondBuilder,
    id: &BondIdentifier,
) -> convex_bonds::instruments::FixedRateBondBuilder {
    if let Some(c) = id.cusip.as_deref() {
        b = b.cusip_unchecked(c);
    } else if let Some(i) = id.isin.as_deref() {
        // No `isin_unchecked` on the fixed-rate builder; route ISIN through
        // BondIdentifiers and `identifiers(...)`.
        let ids = convex_bonds::types::BondIdentifiers::new()
            .with_isin(convex_bonds::types::Isin::new_unchecked(i));
        b = b.identifiers(ids);
    } else if let Some(n) = id.name.as_deref() {
        // Free-form name → store on the bond as an unchecked CUSIP so the
        // builder's identifier invariant is satisfied.
        b = b.cusip_unchecked(n);
    } else {
        // Nothing supplied — give the bond a deterministic placeholder so it
        // builds. The registry name is `None`, matching the spec.
        b = b.cusip_unchecked("UNNAMED");
    }
    b
}

fn apply_coupon<B>(builder: B, coupon: &CouponSpec) -> B
where
    B: CouponBuilder,
{
    builder
        .coupon_rate(coupon.coupon_rate)
        .frequency(coupon.frequency)
        .maturity(coupon.maturity)
        .issue_date(coupon.issue)
        .day_count(coupon.day_count)
        .currency(coupon.currency)
        .face_value(coupon.face_value)
}

trait CouponBuilder: Sized {
    fn coupon_rate(self, v: Decimal) -> Self;
    fn frequency(self, v: Frequency) -> Self;
    fn maturity(self, v: Date) -> Self;
    fn issue_date(self, v: Date) -> Self;
    fn day_count(self, v: convex_core::daycounts::DayCountConvention) -> Self;
    fn currency(self, v: convex_core::types::Currency) -> Self;
    fn face_value(self, v: Decimal) -> Self;
}

impl CouponBuilder for convex_bonds::instruments::FixedRateBondBuilder {
    fn coupon_rate(self, v: Decimal) -> Self {
        self.coupon_rate(v)
    }
    fn frequency(self, v: Frequency) -> Self {
        self.frequency(v)
    }
    fn maturity(self, v: Date) -> Self {
        self.maturity(v)
    }
    fn issue_date(self, v: Date) -> Self {
        self.issue_date(v)
    }
    fn day_count(self, v: convex_core::daycounts::DayCountConvention) -> Self {
        self.day_count(v)
    }
    fn currency(self, v: convex_core::types::Currency) -> Self {
        self.currency(v)
    }
    fn face_value(self, v: Decimal) -> Self {
        self.face_value(v)
    }
}

fn build_fixed_rate(spec: FixedRateSpec) -> Handle {
    let mut b = FixedRateBond::builder();
    b = apply_identifier_fixed(b, &spec.id);
    b = apply_coupon(b, &spec.coupon);
    let bond = match b.build() {
        Ok(b) => b,
        Err(e) => {
            set_last_error(format!("FixedRateBond build error: {e}"));
            return INVALID_HANDLE;
        }
    };
    registry::register(
        bond,
        ObjectKind::Bond(BondKind::FixedRate),
        name_from_id(&spec.id),
    )
}

fn build_callable(spec: CallableSpec) -> Handle {
    // Build the underlying fixed bond first, then attach the schedule.
    let mut fb = FixedRateBond::builder();
    fb = apply_identifier_fixed(fb, &spec.id);
    fb = apply_coupon(fb, &spec.coupon);
    let base = match fb.build() {
        Ok(b) => b,
        Err(e) => {
            set_last_error(format!("CallableBond base build error: {e}"));
            return INVALID_HANDLE;
        }
    };

    use convex_analytics::dto::CallStyle;
    let call_type = match spec.call_style {
        CallStyle::American => CallType::American,
        CallStyle::European => CallType::European,
        CallStyle::Bermudan => CallType::Bermudan,
        CallStyle::MakeWhole => CallType::MakeWhole,
    };

    let mut schedule = if matches!(spec.call_style, CallStyle::MakeWhole) {
        CallSchedule::make_whole(spec.make_whole_spread_bps.unwrap_or(0.0))
    } else {
        CallSchedule::new(call_type)
    };
    for entry in &spec.call_schedule {
        let mut e = CallEntry::new(entry.date, entry.price);
        if let Some(end) = entry.end_date {
            e = e.with_end_date(end);
        }
        schedule.entries.push(e);
    }

    let bond = CallableBond::new(base, schedule);
    registry::register(
        bond,
        ObjectKind::Bond(BondKind::Callable),
        name_from_id(&spec.id),
    )
}

fn build_floating_rate(spec: FloatingRateSpec) -> Handle {
    use convex_curves::multicurve::RateIndex;
    let index = match spec.rate_index {
        RateIndexCode::Sofr => RateIndex::Sofr,
        RateIndexCode::Sonia => RateIndex::Sonia,
        RateIndexCode::Estr => RateIndex::Estr,
        RateIndexCode::Tonar => RateIndex::Tonar,
        RateIndexCode::Saron => RateIndex::Saron,
        RateIndexCode::Corra => RateIndex::Corra,
        RateIndexCode::Euribor3m => RateIndex::Euribor3M,
        RateIndexCode::Euribor6m => RateIndex::Euribor6M,
        RateIndexCode::Tibor3m => RateIndex::Tibor3M,
    };

    let mut b = FloatingRateNote::builder();
    if let Some(c) = spec.id.cusip.as_deref() {
        b = b.cusip_unchecked(c);
    } else if let Some(i) = spec.id.isin.as_deref() {
        b = b.isin_unchecked(i);
    }
    b = b
        .index(index)
        .spread_decimal(spec.spread_bps / Decimal::from(10_000))
        .maturity(spec.maturity)
        .issue_date(spec.issue)
        .frequency(spec.frequency)
        .day_count(spec.day_count)
        .currency(spec.currency)
        .face_value(spec.face_value);
    if let Some(cap) = spec.cap {
        b = b.cap(cap);
    }
    if let Some(floor) = spec.floor {
        b = b.floor(floor);
    }

    let bond = match b.build() {
        Ok(b) => b,
        Err(e) => {
            set_last_error(format!("FloatingRateNote build error: {e}"));
            return INVALID_HANDLE;
        }
    };
    registry::register(
        bond,
        ObjectKind::Bond(BondKind::FloatingRate),
        name_from_id(&spec.id),
    )
}

fn build_zero_coupon(spec: ZeroCouponSpec) -> Handle {
    use convex_bonds::instruments::Compounding as ZcbComp;
    use convex_core::types::Compounding as CoreComp;
    let comp = match spec.compounding {
        CoreComp::Annual => ZcbComp::Annual,
        CoreComp::SemiAnnual => ZcbComp::SemiAnnual,
        CoreComp::Quarterly => ZcbComp::Quarterly,
        CoreComp::Monthly => ZcbComp::Monthly,
        CoreComp::Continuous => ZcbComp::Continuous,
        // Simple/Daily aren't on the bond builder; nearest fallback.
        CoreComp::Simple => ZcbComp::Annual,
        CoreComp::Daily => ZcbComp::Continuous,
    };

    let mut builder = ZeroCouponBond::builder()
        .maturity(spec.maturity)
        .issue_date(spec.issue)
        .currency(spec.currency)
        .face_value(spec.face_value)
        .day_count(spec.day_count)
        .compounding(comp);
    if let Some(c) = &spec.id.cusip {
        builder = builder.cusip_unchecked(c);
    } else if let Some(i) = &spec.id.isin {
        builder = builder.isin_unchecked(i);
    }

    match builder.build() {
        Ok(bond) => registry::register(
            bond,
            ObjectKind::Bond(BondKind::ZeroCoupon),
            name_from_id(&spec.id),
        ),
        Err(e) => {
            set_last_error(format!("ZeroCouponBond build failed: {e}"));
            INVALID_HANDLE
        }
    }
}

fn build_sinking_fund(spec: SinkingFundSpec) -> Handle {
    if spec.schedule.is_empty() {
        set_last_error("sinking_fund schedule must have at least one payment");
        return INVALID_HANDLE;
    }

    // Build the underlying fixed bond.
    let mut fb = FixedRateBond::builder();
    fb = apply_identifier_fixed(fb, &spec.id);
    fb = apply_coupon(fb, &spec.coupon);
    let base = match fb.build() {
        Ok(b) => b,
        Err(e) => {
            set_last_error(format!("SinkingFundBond base build error: {e}"));
            return INVALID_HANDLE;
        }
    };

    // Build the sinking schedule.
    let mut schedule = SinkingFundSchedule::new();
    for p in &spec.schedule {
        let amount_pct = p.amount.to_f64().unwrap_or(0.0);
        let price_pct = p.price.to_f64().unwrap_or(100.0);
        schedule = schedule.with_payment(SinkingFundPayment::with_price(
            p.date, amount_pct, price_pct,
        ));
    }

    let bond = SinkingFundBond::new(base, schedule);
    registry::register(
        bond,
        ObjectKind::Bond(BondKind::SinkingFund),
        name_from_id(&spec.id),
    )
}

// ---- Curve builders -------------------------------------------------------

fn map_interp(code: InterpolationMethodCode) -> InterpolationMethod {
    match code {
        InterpolationMethodCode::Linear => InterpolationMethod::Linear,
        InterpolationMethodCode::LogLinear => InterpolationMethod::LogLinear,
        InterpolationMethodCode::CubicSpline => InterpolationMethod::CubicSpline,
        InterpolationMethodCode::MonotoneConvex => InterpolationMethod::MonotoneConvex,
    }
}

fn build_discrete_curve(spec: DiscreteCurveSpec) -> Handle {
    if spec.tenors.len() != spec.values.len() {
        set_last_error("tenors and values must have the same length");
        return INVALID_HANDLE;
    }
    let value_type = match spec.value_kind {
        CurveValueKind::ZeroRate => ValueType::ZeroRate {
            compounding: spec.compounding,
            day_count: spec.day_count,
        },
        CurveValueKind::DiscountFactor => ValueType::DiscountFactor,
    };
    let curve = match DiscreteCurve::new(
        spec.ref_date,
        spec.tenors.clone(),
        spec.values.clone(),
        value_type,
        map_interp(spec.interpolation),
    ) {
        Ok(c) => c,
        Err(e) => {
            set_last_error(format!("DiscreteCurve build error: {e}"));
            return INVALID_HANDLE;
        }
    };
    registry::register(RateCurve::new(curve), ObjectKind::Curve, spec.name)
}

fn build_bootstrap_curve(spec: convex_analytics::dto::BootstrapSpec) -> Handle {
    use convex_analytics::dto::BootstrapMethod;

    if spec.instruments.is_empty() {
        set_last_error("bootstrap requires at least one instrument");
        return INVALID_HANDLE;
    }

    let mut set = InstrumentSet::new();
    for inst in &spec.instruments {
        match *inst {
            CurveInstrument::Deposit { tenor, rate } => {
                set.add(Deposit::from_tenor(
                    spec.ref_date,
                    tenor,
                    rate,
                    spec.day_count,
                ));
            }
            CurveInstrument::Fra { tenor, rate } => {
                let start_m = (tenor.fract() * 12.0).round() as i32;
                let end_m = ((tenor + 0.25) * 12.0).round() as i32;
                set.add(Fra::from_tenors(
                    spec.ref_date,
                    start_m,
                    end_m,
                    rate,
                    spec.day_count,
                ));
            }
            CurveInstrument::Swap { tenor, rate } => {
                set.add(Swap::from_tenor(
                    spec.ref_date,
                    tenor,
                    rate,
                    Frequency::SemiAnnual,
                    DayCountConvention::Thirty360US,
                ));
            }
            CurveInstrument::Ois { tenor, rate } => {
                set.add(Ois::from_tenor(spec.ref_date, tenor, rate, spec.day_count));
            }
        }
    }

    match spec.method {
        BootstrapMethod::GlobalFit => {
            let fitter = GlobalFitter::new().interpolation(map_interp(spec.interpolation));
            match fitter.fit(spec.ref_date, &set) {
                Ok(result) => {
                    registry::register(RateCurve::new(result.curve), ObjectKind::Curve, spec.name)
                }
                Err(e) => {
                    set_last_error(format!("GlobalFit error: {e}"));
                    INVALID_HANDLE
                }
            }
        }
        BootstrapMethod::Piecewise => {
            set_last_error("piecewise bootstrap not yet wired in FFI");
            INVALID_HANDLE
        }
    }
}

// ---- Diagnostics ----------------------------------------------------------

#[allow(dead_code)]
pub(crate) fn fixed_rate_decimal_to_f64(d: Decimal) -> f64 {
    d.to_f64().unwrap_or(f64::NAN)
}

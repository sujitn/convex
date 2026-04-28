//! Data Transfer Objects: the JSON wire format for FFI / MCP / WASM.
//!
//! Every boundary call is a tagged-enum spec on input and a structured
//! response on output. Adding a bond shape, a curve type, a spread family,
//! or a pricing convention is a serde variant plus a dispatch arm — no new
//! C symbol, no new P/Invoke, no new Excel UDF.

#![allow(missing_docs)] // Field names + module-level types are self-documenting.

use convex_core::calendars::BusinessDayConvention;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Compounding, Currency, Date, Frequency, Mark, SpreadType};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Registry handle. `0` denotes "no handle".
pub type Handle = u64;

// ---- Bond construction ----------------------------------------------------

/// Tagged JSON: `{ "type": "fixed_rate" | "callable" | "floating_rate"
/// | "zero_coupon" | "sinking_fund", ... }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BondSpec {
    FixedRate(FixedRateSpec),
    Callable(CallableSpec),
    FloatingRate(FloatingRateSpec),
    ZeroCoupon(ZeroCouponSpec),
    SinkingFund(SinkingFundSpec),
}

/// Any one of CUSIP, ISIN, or free-form name suffices. The first non-empty
/// field is used as the registry key.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BondIdentifier {
    pub cusip: Option<String>,
    pub isin: Option<String>,
    pub name: Option<String>,
}

/// Fields shared across coupon-bearing bond shapes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouponSpec {
    /// Annual coupon as decimal (0.05 = 5%).
    pub coupon_rate: Decimal,
    #[serde(default)]
    pub frequency: Frequency,
    pub maturity: Date,
    pub issue: Date,
    #[serde(default = "default_thirty_360_us")]
    pub day_count: DayCountConvention,
    #[serde(default)]
    pub currency: Currency,
    /// Quoted unit, default 100.
    #[serde(default = "default_face")]
    pub face_value: Decimal,
    #[serde(default = "default_bdc")]
    pub business_day_convention: BusinessDayConvention,
}

fn default_thirty_360_us() -> DayCountConvention {
    DayCountConvention::Thirty360US
}
fn default_face() -> Decimal {
    Decimal::from(100)
}
fn default_bdc() -> BusinessDayConvention {
    BusinessDayConvention::ModifiedFollowing
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixedRateSpec {
    #[serde(default, flatten)]
    pub id: BondIdentifier,
    #[serde(flatten)]
    pub coupon: CouponSpec,
}

/// Price is % of par (102.0 = 102%).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallEntrySpec {
    pub date: Date,
    pub price: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_date: Option<Date>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallStyle {
    #[default]
    American,
    European,
    Bermudan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallableSpec {
    #[serde(default, flatten)]
    pub id: BondIdentifier,
    #[serde(flatten)]
    pub coupon: CouponSpec,
    pub call_schedule: Vec<CallEntrySpec>,
    #[serde(default)]
    pub call_style: CallStyle,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub put_schedule: Vec<CallEntrySpec>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RateIndexCode {
    #[default]
    Sofr,
    Sonia,
    Estr,
    Tonar,
    Saron,
    Corra,
    Euribor3m,
    Euribor6m,
    Tibor3m,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FloatingRateSpec {
    #[serde(default, flatten)]
    pub id: BondIdentifier,
    /// Spread over the index, in basis points.
    pub spread_bps: Decimal,
    #[serde(default)]
    pub rate_index: RateIndexCode,
    pub maturity: Date,
    pub issue: Date,
    #[serde(default = "default_quarterly")]
    pub frequency: Frequency,
    #[serde(default = "default_act_360")]
    pub day_count: DayCountConvention,
    #[serde(default)]
    pub currency: Currency,
    #[serde(default = "default_face")]
    pub face_value: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cap: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor: Option<Decimal>,
}

fn default_quarterly() -> Frequency {
    Frequency::Quarterly
}
fn default_act_360() -> DayCountConvention {
    DayCountConvention::Act360
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZeroCouponSpec {
    #[serde(default, flatten)]
    pub id: BondIdentifier,
    pub maturity: Date,
    pub issue: Date,
    #[serde(default)]
    pub compounding: Compounding,
    #[serde(default = "default_act_act_icma")]
    pub day_count: DayCountConvention,
    #[serde(default)]
    pub currency: Currency,
    #[serde(default = "default_face")]
    pub face_value: Decimal,
}

fn default_act_act_icma() -> DayCountConvention {
    DayCountConvention::ActActIcma
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SinkingFundPaymentSpec {
    pub date: Date,
    /// Notional retired on this date, % of original face.
    pub amount: Decimal,
    /// Sink price as % of par, default 100.
    #[serde(default = "default_par_price")]
    pub price: Decimal,
}

fn default_par_price() -> Decimal {
    Decimal::from(100)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SinkingFundSpec {
    #[serde(default, flatten)]
    pub id: BondIdentifier,
    #[serde(flatten)]
    pub coupon: CouponSpec,
    pub schedule: Vec<SinkingFundPaymentSpec>,
}

// ---- Curve construction ---------------------------------------------------

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterpolationMethodCode {
    #[default]
    Linear,
    /// Linear in log(df), i.e. linear forward.
    LogLinear,
    CubicSpline,
    /// Hagan-West, positive forwards.
    MonotoneConvex,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurveValueKind {
    /// Continuously-compounded zero rates by default.
    #[default]
    ZeroRate,
    DiscountFactor,
}

/// Specification for a yield/discount curve.
/// Tagged JSON: `{ "type": "discrete" | "bootstrap", ... }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CurveSpec {
    Discrete(DiscreteCurveSpec),
    Bootstrap(BootstrapSpec),
}

/// `tenors` and `values` are parallel arrays. `values` is interpreted per
/// `value_kind` (zero rates as decimal, or discount factors).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscreteCurveSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub ref_date: Date,
    pub tenors: Vec<f64>,
    pub values: Vec<f64>,
    #[serde(default)]
    pub value_kind: CurveValueKind,
    #[serde(default)]
    pub interpolation: InterpolationMethodCode,
    #[serde(default = "default_act_365")]
    pub day_count: DayCountConvention,
    /// Only consulted when `value_kind == ZeroRate`.
    #[serde(default = "default_continuous")]
    pub compounding: Compounding,
}

fn default_act_365() -> DayCountConvention {
    DayCountConvention::Act365Fixed
}
fn default_continuous() -> Compounding {
    Compounding::Continuous
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapMethod {
    /// Levenberg-Marquardt across all instruments at once.
    #[default]
    GlobalFit,
    /// Iterative Brent root-find, instrument by instrument.
    Piecewise,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CurveInstrument {
    /// Tenor in years; rate as decimal (0.05 = 5%).
    Deposit {
        tenor: f64,
        rate: f64,
    },
    Fra {
        tenor: f64,
        rate: f64,
    },
    Swap {
        tenor: f64,
        rate: f64,
    },
    Ois {
        tenor: f64,
        rate: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub ref_date: Date,
    #[serde(default)]
    pub method: BootstrapMethod,
    pub instruments: Vec<CurveInstrument>,
    #[serde(default)]
    pub interpolation: InterpolationMethodCode,
    #[serde(default = "default_act_360")]
    pub day_count: DayCountConvention,
}

// ---- Pricing --------------------------------------------------------------

/// Either a textual mark (parsed by `Mark::from_str` — see examples in
/// `convex_schema("Mark")`) or an already-parsed JSON `Mark`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MarkInput {
    Text(String),
    Parsed(Mark),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingRequest {
    pub bond: Handle,
    pub settlement: Date,
    pub mark: MarkInput,
    /// Discount curve handle. Required for spread marks and FRNs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<Handle>,
    /// Compounding for the derived YTM.
    #[serde(default)]
    pub quote_frequency: Frequency,
    /// FRN projection curve. Falls back to `curve` when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forward_curve: Option<Handle>,
}

/// Prices and accrued are per 100 face. YTM is decimal (0.05 = 5%).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingResponse {
    pub clean_price: f64,
    pub dirty_price: f64,
    pub accrued: f64,
    pub ytm_decimal: f64,
    /// Present iff input mark was a spread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub z_spread_bps: Option<f64>,
}

// ---- Risk -----------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskRequest {
    pub bond: Handle,
    pub settlement: Date,
    pub mark: MarkInput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<Handle>,
    #[serde(default)]
    pub quote_frequency: Frequency,
    /// KRD tenors in years. Empty → KRD not computed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key_rate_tenors: Vec<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forward_curve: Option<Handle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRate {
    pub tenor: f64,
    pub duration: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskResponse {
    pub modified_duration: f64,
    pub macaulay_duration: f64,
    pub convexity: f64,
    pub dv01: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread_duration: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key_rates: Vec<KeyRate>,
}

// ---- Spread ---------------------------------------------------------------

/// Optional per-family parameters. Each field documents which spread family
/// reads it; values are ignored for unrelated families.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpreadParams {
    /// OAS: short-rate volatility (decimal, e.g. 0.01 = 1%).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volatility: Option<f64>,
    /// DM: projection curve handle. If omitted, falls back to the discount curve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forward_curve: Option<Handle>,
    /// Simple margin: current index rate (decimal, 0.05 = 5%).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_index: Option<f64>,
    /// G-spread: government curve handle. Required for `GSpread` requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub govt_curve: Option<Handle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpreadRequest {
    pub bond: Handle,
    pub curve: Handle,
    pub settlement: Date,
    pub mark: MarkInput,
    pub spread_type: SpreadType,
    #[serde(default)]
    pub params: SpreadParams,
}

/// Spread response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpreadResponse {
    /// Spread in basis points.
    pub spread_bps: f64,
    /// Spread DV01 (P&L per 1bp spread shift).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread_dv01: Option<f64>,
    /// Spread duration (years equivalent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread_duration: Option<f64>,
    /// OAS only: option value (clean equivalent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_value: Option<f64>,
    /// OAS only: effective duration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_duration: Option<f64>,
    /// OAS only: effective convexity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_convexity: Option<f64>,
}

// ---- Cashflows ------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashflowRequest {
    pub bond: Handle,
    /// Only flows on or after this date are returned.
    pub settlement: Date,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashflowEntry {
    pub date: Date,
    /// Amount per 100 face.
    pub amount: f64,
    /// Stable kebab-case tag: "coupon", "redemption", "coupon-and-redemption", "fee".
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashflowResponse {
    pub flows: Vec<CashflowEntry>,
}

// ---- Curve queries --------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurveQueryKind {
    Zero,
    Df,
    Forward,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveQueryRequest {
    pub curve: Handle,
    pub query: CurveQueryKind,
    /// Primary tenor in years.
    pub tenor: f64,
    /// Forward queries only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenor_end: Option<f64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CurveQueryResponse {
    /// Rate as decimal, or DF in [0, 1].
    pub value: f64,
}

// ---- Top-level envelopes --------------------------------------------------

/// FFI response envelope. Carried as `{ "ok": "true", "result": … }` on
/// success and `{ "ok": "false", "error": … }` on failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "ok")]
pub enum Envelope<T> {
    #[serde(rename = "true")]
    Ok { result: T },
    #[serde(rename = "false")]
    Err { error: ErrorBody },
}

/// Structured error body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    /// Stable error code (`invalid_input`, `invalid_handle`, `analytics`, ...).
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Optional pointer to the offending field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::from_ymd(y, m, day).unwrap()
    }

    #[test]
    fn fixed_rate_spec_round_trip() {
        let spec = BondSpec::FixedRate(FixedRateSpec {
            id: BondIdentifier {
                cusip: Some("037833100".into()),
                ..Default::default()
            },
            coupon: CouponSpec {
                coupon_rate: dec!(0.05),
                frequency: Frequency::SemiAnnual,
                maturity: d(2035, 1, 15),
                issue: d(2025, 1, 15),
                day_count: DayCountConvention::Thirty360US,
                currency: Currency::USD,
                face_value: dec!(100),
                business_day_convention: BusinessDayConvention::ModifiedFollowing,
            },
        });
        let json = serde_json::to_string(&spec).unwrap();
        assert!(json.contains("\"type\":\"fixed_rate\""));
        let back: BondSpec = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&back).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn mark_input_accepts_text() {
        let m: MarkInput = serde_json::from_str("\"99.5C\"").unwrap();
        assert!(matches!(m, MarkInput::Text(_)));
    }

    #[test]
    fn mark_input_accepts_parsed() {
        let m: MarkInput =
            serde_json::from_str(r#"{"mark":"price","value":99.5,"kind":"Clean"}"#).unwrap();
        assert!(matches!(m, MarkInput::Parsed(Mark::Price { .. })));
    }

    #[test]
    fn pricing_request_round_trip() {
        let r = PricingRequest {
            bond: 101,
            settlement: d(2025, 4, 15),
            mark: MarkInput::Text("99.5C".into()),
            curve: None,
            quote_frequency: Frequency::SemiAnnual,
            forward_curve: None,
        };
        let json = serde_json::to_string(&r).unwrap();
        let _: PricingRequest = serde_json::from_str(&json).unwrap();
    }
}

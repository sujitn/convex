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

/// Strict native optional-model input/settings JSON. Full host schemas and
/// dispatch remain CVX-063; these reexports do not add a registry RPC.
pub use crate::optional_bond::{
    ExercisePriority, ExerciseState, GridSettings, Hw1fParameters, ModelKey, OasSolveSettings,
    OptionalBondContext, OptionalBondSettings, RiskAxis, RiskSettings, SigmaCalibrationSettings,
    SwaptionCalibrationQuote, SwaptionVolatility,
};
/// Lossless native/shared JSON economic terms. Reconstruct through the checked
/// build API. C ABI and client dispatch migration is separately owned by CVX-063.
pub use convex_bonds::contracts::{
    BondCalendarIds, BondCoupon, BondProfiles, BondTerms, ExerciseStyle, ExerciseTerms,
    OvernightRateTerms, TermRateTerms,
};

// ---- Bond construction ----------------------------------------------------

/// Tagged JSON: `{ "type": "fixed_rate" | "callable" | "floating_rate"
/// | "zero_coupon" | "sinking_fund", ... }`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum BondSpec {
    FixedRate(FixedRateSpec),
    Callable(CallableSpec),
    FloatingRate(FloatingRateSpec),
    ZeroCoupon(ZeroCouponSpec),
    SinkingFund(SinkingFundSpec),
}

/// Any one of CUSIP, ISIN, or free-form name identifies the security. When
/// `registry_key` is set it (not the identifier) becomes the registry's
/// eviction slot — a host like Excel passes its owning cell so two cells that
/// reference the same CUSIP don't evict each other. Falls back to
/// cusip → isin → name when absent.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct BondIdentifier {
    pub cusip: Option<String>,
    pub isin: Option<String>,
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_key: Option<String>,
}

/// Fields shared across coupon-bearing bond shapes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CouponSpec {
    /// Annual coupon as decimal (0.05 = 5%).
    #[serde(with = "convex_core::contracts::numeric_decimal")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "convex_core::contracts::numeric_decimal::schema")
    )]
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
    #[serde(with = "convex_core::contracts::numeric_decimal")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "convex_core::contracts::numeric_decimal::schema")
    )]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(deny_unknown_fields))]
pub struct FixedRateSpec {
    #[serde(default, flatten)]
    pub id: BondIdentifier,
    #[serde(flatten)]
    pub coupon: CouponSpec,
    /// Complete fixed-bond dates/entitlement/redemption; absent retains v1 defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terms: Option<FixedRateTerms>,
}

/// Exact R0 fixed-bond construction inputs. Every supplied field is consumed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct FixedRateTerms {
    pub dated_date: Date,
    pub first_coupon_date: Option<Date>,
    pub penultimate_coupon_date: Option<Date>,
    pub calendar: String,
    pub end_of_month: bool,
    pub settlement_days: u32,
    #[serde(with = "convex_core::contracts::numeric_decimal")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "convex_core::contracts::numeric_decimal::schema")
    )]
    pub redemption_value: Decimal,
    pub ex_coupon: Option<convex_core::cashflows::ExCouponRule>,
}

/// Price is % of par (102.0 = 102%).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CallEntrySpec {
    pub date: Date,
    pub price: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_date: Option<Date>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CallStyle {
    #[default]
    American,
    European,
    Bermudan,
    MakeWhole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(deny_unknown_fields))]
pub struct CallableSpec {
    #[serde(default, flatten)]
    pub id: BondIdentifier,
    #[serde(flatten)]
    pub coupon: CouponSpec,
    pub call_schedule: Vec<CallEntrySpec>,
    #[serde(default)]
    pub call_style: CallStyle,
    /// Make-whole spread in basis points. Only meaningful when
    /// `call_style == MakeWhole`. Read by `convex_make_whole` to compute the
    /// MW redemption price = treasury_rate + spread/10000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub make_whole_spread_bps: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub put_schedule: Vec<CallEntrySpec>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(deny_unknown_fields))]
pub struct FloatingRateSpec {
    #[serde(default, flatten)]
    pub id: BondIdentifier,
    /// Spread over the index, in basis points.
    #[serde(with = "convex_core::contracts::numeric_decimal")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "convex_core::contracts::numeric_decimal::schema")
    )]
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
    #[serde(with = "convex_core::contracts::numeric_decimal")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "convex_core::contracts::numeric_decimal::schema")
    )]
    pub face_value: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "convex_core::contracts::numeric_decimal::optional")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "convex_core::contracts::numeric_decimal::optional::schema")
    )]
    pub cap: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "convex_core::contracts::numeric_decimal::optional")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "convex_core::contracts::numeric_decimal::optional::schema")
    )]
    pub floor: Option<Decimal>,
}

fn default_quarterly() -> Frequency {
    Frequency::Quarterly
}
fn default_act_360() -> DayCountConvention {
    DayCountConvention::Act360
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(deny_unknown_fields))]
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
    #[serde(with = "convex_core::contracts::numeric_decimal")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "convex_core::contracts::numeric_decimal::schema")
    )]
    pub face_value: Decimal,
    /// Actual final cash redemption; omission retains the v1 face-value default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "convex_core::contracts::numeric_decimal::optional")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "convex_core::contracts::numeric_decimal::optional::schema")
    )]
    pub redemption_value: Option<Decimal>,
}

fn default_act_act_icma() -> DayCountConvention {
    DayCountConvention::ActActIcma
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SinkingFundPaymentSpec {
    pub date: Date,
    /// Notional retired on this date, % of original face.
    #[serde(with = "convex_core::contracts::numeric_decimal")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "convex_core::contracts::numeric_decimal::schema")
    )]
    pub amount: Decimal,
    /// Sink price as % of par, default 100.
    #[serde(default = "default_par_price")]
    #[serde(with = "convex_core::contracts::numeric_decimal")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "convex_core::contracts::numeric_decimal::schema")
    )]
    pub price: Decimal,
}

fn default_par_price() -> Decimal {
    Decimal::from(100)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(deny_unknown_fields))]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CurveValueKind {
    /// Continuously-compounded zero rates by default.
    #[default]
    ZeroRate,
    DiscountFactor,
}

/// Specification for a yield/discount curve.
/// Tagged JSON: `{ "type": "discrete" | "bootstrap", ... }`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CurveSpec {
    Discrete(DiscreteCurveSpec),
    Bootstrap(BootstrapSpec),
}

/// `tenors` and `values` are parallel arrays. `values` is interpreted per
/// `value_kind` (zero rates as decimal, or discount factors).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct DiscreteCurveSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Registry eviction slot (e.g. owning cell). Falls back to `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_key: Option<String>,
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum BootstrapMethod {
    /// Levenberg-Marquardt across all instruments at once.
    #[default]
    GlobalFit,
    /// Iterative Brent root-find, instrument by instrument.
    Piecewise,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct BootstrapSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Registry eviction slot (e.g. owning cell). Falls back to `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_key: Option<String>,
    pub ref_date: Date,
    #[serde(default)]
    pub method: BootstrapMethod,
    pub instruments: Vec<CurveInstrument>,
    #[serde(default)]
    pub interpolation: InterpolationMethodCode,
    #[serde(default = "default_act_360")]
    pub day_count: DayCountConvention,
}

/// Generate the structural bond-constructor schema from the actual input DTOs.
#[cfg(feature = "schemars")]
pub fn bond_spec_schema() -> serde_json::Value {
    schemars::schema_for!(BondSpec).to_value()
}

/// Generate the structural curve-constructor schema from the actual input DTOs.
#[cfg(feature = "schemars")]
pub fn curve_spec_schema() -> serde_json::Value {
    schemars::schema_for!(CurveSpec).to_value()
}

// ---- Pricing --------------------------------------------------------------

// The checked native curve contract uses actual immutable values rather than
// registry handles. Host/schema dispatch migration is owned by CVX-063.
pub use crate::curve_inputs::{CurveHelperInput, HelperCurveRecipe};
pub use convex_curves::contracts::{CurveQuery, CurveRecipe};

/// Either a textual mark (parsed by `Mark::from_str` — see examples in
/// `convex_schema("Mark")`) or an already-parsed JSON `Mark`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum MarkInput {
    Text(String),
    Parsed(Mark),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
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
    /// Explicit FRN projection curve; fixing-aware dispatch is pending.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forward_curve: Option<Handle>,
}

/// Prices and accrued are per 100 face. YTM is decimal (0.05 = 5%).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct KeyRate {
    pub tenor: f64,
    pub duration: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SpreadParams {
    /// Quoted-yield compounding for fixed-coupon spreads; preserves the v1 default.
    #[serde(default)]
    pub quote_frequency: Frequency,
    /// OAS: short-rate volatility (decimal, e.g. 0.01 = 1%).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volatility: Option<f64>,
    /// DM: explicit projection curve handle. Fixing-aware dispatch is pending.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forward_curve: Option<Handle>,
    /// Simple margin: current index rate (decimal, 0.05 = 5%).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_index: Option<f64>,
    /// Actual government benchmark yields; a generic zero curve is not a substitute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub government: Option<GovernmentCurveSpec>,
}

/// Caller-supplied government benchmarks, separate from discount and swap curves.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct GovernmentCurveSpec {
    /// Curve origin on the supplied benchmark record.
    pub reference_date: Date,
    /// Sovereign and currency of the actual benchmarks.
    pub sovereign: crate::spreads::Sovereign,
    /// Original benchmark identifiers, maturities and quoted yields.
    pub benchmarks: Vec<crate::spreads::GovernmentBenchmark>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
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

// ---- Make-whole call ------------------------------------------------------

/// Computes the make-whole call price at a future call date given a
/// reference treasury rate. The make-whole spread is read from the bond's
/// own call schedule. Discount uses the bond's day count and frequency.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct MakeWholeRequest {
    pub bond: Handle,
    /// Date of the hypothetical call exercise.
    pub call_date: Date,
    /// Treasury par yield at the relevant tenor (decimal, 0.05 = 5%).
    pub treasury_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct MakeWholeResponse {
    /// Make-whole price per 100 face, floored at the first call entry's price
    /// (typically par).
    pub price: f64,
    /// Discount rate actually applied = treasury_rate + spread/10000.
    pub discount_rate: f64,
    /// Make-whole spread in basis points read from the call schedule.
    pub spread_bps: f64,
}

// ---- Cashflows ------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CashflowRequest {
    pub bond: Handle,
    /// Only flows on or after this date are returned.
    pub settlement: Date,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CashflowEntry {
    pub date: Date,
    /// Amount per 100 face.
    pub amount: f64,
    /// Stable kebab-case tag: "coupon", "redemption", "coupon-and-redemption", "fee".
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CashflowResponse {
    pub flows: Vec<CashflowEntry>,
}

// ---- Curve queries --------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CurveQueryKind {
    Zero,
    Df,
    Forward,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
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
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CurveQueryResponse {
    /// Finite decimal rate, or a strictly positive DF; negative rates can give DF above one.
    pub value: f64,
}

// ---- YAS (one-call yield & spread analysis) --------------------------------

/// Convenience yield, Z/ASW spread, risk and settlement invoice analysis.
/// Government-security spreads require separate supplied benchmark securities.
/// Backed by `crate::yas::YASCalculator`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct YasRequest {
    pub bond: Handle,
    pub settlement: Date,
    pub mark: MarkInput,
    /// Spot/discount curve for pricing and Z-spread.
    pub curve: Handle,
    /// Swap curve for I-spread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swap_curve: Option<Handle>,
    #[serde(default)]
    pub quote_frequency: Frequency,
}

/// Yields in percent, spreads in basis points, prices/amounts per 100 face.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct YasResponse {
    pub clean_price: f64,
    pub dirty_price: f64,
    pub accrued: f64,
    pub accrued_days: i32,
    pub ytm_pct: f64,
    pub current_yield_pct: f64,
    pub simple_yield_pct: f64,
    pub z_spread_bps: f64,
    pub modified_duration: f64,
    pub macaulay_duration: f64,
    pub convexity: f64,
    pub dv01_per_100: f64,
    pub principal_amount: f64,
    pub accrued_amount: f64,
    pub settlement_amount: f64,
}

// ---- Scenario analysis ------------------------------------------------------

/// One bump component. Tagged JSON: `{ "kind": "parallel" | "steepener" |
/// "flattener" | "key_rate" | "credit_spread", ... }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum ScenarioBumpSpec {
    Parallel {
        shift_bps: f64,
    },
    Steepener {
        short_shift_bps: f64,
        long_shift_bps: f64,
        #[serde(default = "default_pivot_tenor")]
        pivot_tenor: f64,
    },
    Flattener {
        short_shift_bps: f64,
        long_shift_bps: f64,
        #[serde(default = "default_pivot_tenor")]
        pivot_tenor: f64,
    },
    KeyRate {
        tenor: f64,
        shift_bps: f64,
    },
    CreditSpread {
        shift_bps: f64,
    },
}

fn default_pivot_tenor() -> f64 {
    5.0
}

/// One named scenario = a list of bumps applied together.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ScenarioSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub bumps: Vec<ScenarioBumpSpec>,
}

/// Run N curve scenarios against one bond in a single call. Methodology:
/// the mark implies a Z-spread over the base curve; each scenario bumps the
/// curve and reprices holding that Z-spread fixed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ScenarioRequest {
    pub bond: Handle,
    pub curve: Handle,
    pub settlement: Date,
    pub mark: MarkInput,
    pub scenarios: Vec<ScenarioSpec>,
    #[serde(default)]
    pub quote_frequency: Frequency,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ScenarioRow {
    pub name: String,
    pub clean_price: f64,
    pub dirty_price: f64,
    /// Clean price change vs the base mark.
    pub delta_clean: f64,
    /// YTM at the scenario price, decimal.
    pub ytm_decimal: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ScenarioResponse {
    pub base_clean: f64,
    pub base_ytm_decimal: f64,
    pub z_spread_bps: f64,
    pub rows: Vec<ScenarioRow>,
}

// ---- Risk profile / hedge advisor -----------------------------------------
//
// `convex_risk_profile` returns a `RiskProfile`; being a serializable value it
// round-trips back into `convex_hedge` / `convex_compare` with no server-side
// handle. The `crate::risk` types already derive Serialize/Deserialize, so they
// are reused on the wire directly rather than mirrored.

use crate::risk::{BondFuture, ComparisonReport, Constraints, HedgeProposal, RiskProfile};

fn default_curve_id() -> String {
    "discount".to_string()
}

/// Build a position `RiskProfile` from a registered bond + discount curve.
///
/// Fixed-coupon bonds (fixed-rate, sinking-fund, and the bullet leg of a
/// callable) use the YTM-driven path; a callable additionally requires
/// `volatility` and is routed through the OAS-aware risk path.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RiskProfileRequest {
    pub bond: Handle,
    pub settlement: Date,
    pub mark: MarkInput,
    /// Position face amount (e.g. 10000000 for $10mm). Per-100 metrics are
    /// scaled to market value by the analytics layer.
    #[serde(with = "convex_core::contracts::numeric_decimal")]
    #[cfg_attr(
        feature = "schemars",
        schemars(schema_with = "convex_core::contracts::numeric_decimal::schema")
    )]
    pub notional_face: Decimal,
    /// Discount curve handle (required).
    pub curve: Handle,
    /// Stable curve identifier recorded in `RiskProfile::provenance`.
    #[serde(default = "default_curve_id")]
    pub curve_id: String,
    /// Compounding for the derived YTM. Defaults to the bond's frequency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote_frequency: Option<Frequency>,
    /// KRD ladder in years. Empty → advisor default (`ADVISOR_KEY_RATE_TENORS`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub key_rate_tenors: Vec<f64>,
    /// Optional position label echoed back on the profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_id: Option<String>,
    /// Short-rate volatility (decimal). Required for callable positions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volatility: Option<f64>,
}

/// Which hedge construction to run. Each maps to a `crate::risk` strategy fn.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum HedgeStrategyKind {
    /// Single liquid future sized to neutralise position DV01.
    DurationFutures,
    /// Two futures (front/back tenor) matching level + slope.
    BarbellFutures,
    /// On-the-run cash sovereign offset.
    CashBondPair,
    /// Vanilla single-currency interest-rate swap.
    InterestRateSwap,
    /// Futures ladder matching the full key-rate bucket vector.
    KeyRateFutures,
}

/// Propose a hedge for a previously-computed `RiskProfile`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct HedgeRequest {
    pub strategy: HedgeStrategyKind,
    /// The position to hedge — round-tripped from `convex_risk_profile`.
    pub position: RiskProfile,
    #[serde(default)]
    pub constraints: Constraints,
    /// Discount curve handle (used for instrument risk + carry).
    pub curve: Handle,
    #[serde(default = "default_curve_id")]
    pub curve_id: String,
    pub settlement: Date,
    /// Futures basket overrides for futures strategies. Empty → strategy
    /// default (a synthetic at-current-coupon deliverable). Ignored by the
    /// cash-bond and swap strategies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub basket_overrides: Vec<BondFuture>,
}

/// Compare proposals against a position, optionally with a text narrative.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CompareRequest {
    pub position: RiskProfile,
    pub proposals: Vec<HedgeProposal>,
    #[serde(default)]
    pub constraints: Constraints,
    /// When true, include the deterministic narrative in the response.
    #[serde(default)]
    pub narrate: bool,
}

/// Comparison report plus an optional human-readable narrative.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct CompareResponse {
    pub report: ComparisonReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub narrative: Option<String>,
}

// ---- Top-level envelopes --------------------------------------------------

/// FFI response envelope. Carried as `{ "ok": "true", "result": … }` on
/// success and `{ "ok": "false", "error": … }` on failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "ok")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Envelope<T> {
    #[serde(rename = "true")]
    Ok { result: T },
    #[serde(rename = "false")]
    Err { error: ErrorBody },
}

/// Structured error body.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ErrorBody {
    /// Stable error code (`invalid_input`, `invalid_handle`, `analytics`, ...).
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Optional pointer to the offending field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

/// Generate a complete structural convenience RPC schema from its actual DTO.
/// Semantic validation and registry-name resolution remain at their owning boundaries.
#[cfg(feature = "schemars")]
pub fn rpc_schema(name: &str) -> Option<serde_json::Value> {
    fn input<T: schemars::JsonSchema>() -> serde_json::Value {
        schemars::schema_for!(T).to_value()
    }
    fn output<T: schemars::JsonSchema>() -> serde_json::Value {
        schemars::generate::SchemaSettings::default()
            .for_serialize()
            .into_generator()
            .into_root_schema_for::<T>()
            .to_value()
    }
    Some(match name {
        "Mark" => input::<Mark>(),
        "PricingRequest" => input::<PricingRequest>(),
        "RiskRequest" => input::<RiskRequest>(),
        "SpreadRequest" => input::<SpreadRequest>(),
        "CashflowRequest" => input::<CashflowRequest>(),
        "CurveQueryRequest" => input::<CurveQueryRequest>(),
        "MakeWholeRequest" => input::<MakeWholeRequest>(),
        "YasRequest" => input::<YasRequest>(),
        "ScenarioRequest" => input::<ScenarioRequest>(),
        "RiskProfileRequest" => input::<RiskProfileRequest>(),
        "HedgeRequest" => input::<HedgeRequest>(),
        "CompareRequest" => input::<CompareRequest>(),
        "PricingResponse" => output::<PricingResponse>(),
        "RiskResponse" => output::<RiskResponse>(),
        "SpreadResponse" => output::<SpreadResponse>(),
        "CashflowResponse" => output::<CashflowResponse>(),
        "CurveQueryResponse" => output::<CurveQueryResponse>(),
        "MakeWholeResponse" => output::<MakeWholeResponse>(),
        "YasResponse" => output::<YasResponse>(),
        "ScenarioResponse" => output::<ScenarioResponse>(),
        "RiskProfile" => output::<RiskProfile>(),
        "HedgeProposal" => output::<HedgeProposal>(),
        "CompareResponse" => output::<CompareResponse>(),
        _ => return None,
    })
}

impl<'de> Deserialize<'de> for BondSpec {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut value = convex_core::contracts::json::deserialize_value(deserializer)?;
        let kind = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| serde::de::Error::custom("constructor requires a type string"))?;
        validate_constructor_fields(&value).map_err(serde::de::Error::custom)?;
        if let Some(fields) = value.as_object_mut() {
            fields.remove("type");
        }
        match kind.as_str() {
            "fixed_rate" => serde_json::from_value(value)
                .map(Self::FixedRate)
                .map_err(serde::de::Error::custom),
            "callable" => serde_json::from_value(value)
                .map(Self::Callable)
                .map_err(serde::de::Error::custom),
            "floating_rate" => serde_json::from_value(value)
                .map(Self::FloatingRate)
                .map_err(serde::de::Error::custom),
            "zero_coupon" => serde_json::from_value(value)
                .map(Self::ZeroCoupon)
                .map_err(serde::de::Error::custom),
            "sinking_fund" => serde_json::from_value(value)
                .map(Self::SinkingFund)
                .map_err(serde::de::Error::custom),
            _ => Err(serde::de::Error::custom("unsupported constructor type")),
        }
    }
}

impl<'de> Deserialize<'de> for CurveSpec {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut value = convex_core::contracts::json::deserialize_value(deserializer)?;
        let kind = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| serde::de::Error::custom("constructor requires a type string"))?;
        validate_constructor_fields(&value).map_err(serde::de::Error::custom)?;
        if let Some(fields) = value.as_object_mut() {
            fields.remove("type");
        }
        match kind.as_str() {
            "discrete" => serde_json::from_value(value)
                .map(Self::Discrete)
                .map_err(serde::de::Error::custom),
            "bootstrap" => serde_json::from_value(value)
                .map(Self::Bootstrap)
                .map_err(serde::de::Error::custom),
            _ => Err(serde::de::Error::custom("unsupported constructor type")),
        }
    }
}

fn validate_constructor_fields(value: &serde_json::Value) -> Result<(), String> {
    let Some(fields) = value.as_object() else {
        return Ok(());
    };
    let Some(kind) = fields.get("type").and_then(serde_json::Value::as_str) else {
        return Ok(());
    };
    const ID: &[&str] = &["type", "cusip", "isin", "name", "registry_key"];
    const COUPON: &[&str] = &[
        "coupon_rate",
        "frequency",
        "maturity",
        "issue",
        "day_count",
        "currency",
        "face_value",
        "business_day_convention",
    ];
    const CURVE: &[&str] = &[
        "type",
        "name",
        "registry_key",
        "ref_date",
        "interpolation",
        "day_count",
    ];
    let groups: &[&[&str]] = match kind {
        "fixed_rate" => &[ID, COUPON, &["terms"]],
        "callable" => &[
            ID,
            COUPON,
            &[
                "call_schedule",
                "call_style",
                "make_whole_spread_bps",
                "put_schedule",
            ],
        ],
        "sinking_fund" => &[ID, COUPON, &["schedule"]],
        "floating_rate" => &[
            ID,
            &[
                "spread_bps",
                "rate_index",
                "maturity",
                "issue",
                "frequency",
                "day_count",
                "currency",
                "face_value",
                "cap",
                "floor",
            ],
        ],
        "zero_coupon" => &[
            ID,
            &[
                "maturity",
                "issue",
                "compounding",
                "day_count",
                "currency",
                "face_value",
                "redemption_value",
            ],
        ],
        "discrete" => &[CURVE, &["tenors", "values", "value_kind", "compounding"]],
        "bootstrap" => &[CURVE, &["method", "instruments"]],
        _ => return Ok(()),
    };
    for key in fields.keys() {
        if !groups.iter().any(|group| group.contains(&key.as_str())) {
            return Err(format!("unsupported {kind} constructor field {key}"));
        }
    }
    Ok(())
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
            terms: None,
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

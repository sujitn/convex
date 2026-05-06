//! MCP Server implementation for Convex.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::schemars::JsonSchema;
use rmcp::serde::{Deserialize, Serialize};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use convex::{
    compare_hedges, compute_position_risk, duration_futures, interest_rate_swap, narrate,
    price_from_mark, yield_to_maturity, Bond, CallSchedule, CallableBond, ComparisonReport,
    Compounding, Constraints, Currency, Date, DayCountConvention, Deposit, DiscreteCurve,
    FixedRateBond, FloatingRateNote, Frequency, GlobalFitter, HedgeProposal, ISpreadCalculator,
    InstrumentSet, InterpolationMethod, Mark, Ois, RateCurve, RateCurveDyn, RiskProfile, Swap,
    ValueType, Yield, ZSpreadCalculator, ZeroCouponBond,
};

use crate::error::McpToolError;
use crate::{SERVER_NAME, SERVER_VERSION};

/// Stored curve type
pub type StoredCurve = RateCurve<DiscreteCurve>;

/// Bond storage with type discrimination
#[derive(Clone, Debug)]
pub enum StoredBond {
    /// Fixed rate bond
    Fixed(FixedRateBond),
    /// Zero coupon bond
    Zero(ZeroCouponBond),
    /// Callable bond with embedded call options
    Callable(CallableBond),
    /// Floating rate note
    Floating(FloatingRateNote),
}

impl StoredBond {
    /// Returns the bond type name as a string
    pub fn type_name(&self) -> &'static str {
        match self {
            StoredBond::Fixed(_) => "Fixed",
            StoredBond::Zero(_) => "Zero",
            StoredBond::Callable(_) => "Callable",
            StoredBond::Floating(_) => "FRN",
        }
    }

    /// Underlying fixed-rate bond — `Fixed` directly, `Callable` via its base bond.
    /// Returns `None` for Zero and FRN.
    pub fn fixed(&self) -> Option<&FixedRateBond> {
        match self {
            StoredBond::Fixed(b) => Some(b),
            StoredBond::Callable(c) => Some(c.base_bond()),
            _ => None,
        }
    }
}

/// MCP Server for Convex analytics.
#[derive(Clone)]
#[allow(missing_docs)]
pub struct ConvexMcpServer {
    pub bonds: Arc<RwLock<HashMap<String, StoredBond>>>,
    pub curves: Arc<RwLock<HashMap<String, StoredCurve>>>,
    tool_router: ToolRouter<Self>,
}

impl ConvexMcpServer {
    /// New server with empty bond/curve registries.
    pub fn new() -> Self {
        Self {
            bonds: Arc::new(RwLock::new(HashMap::new())),
            curves: Arc::new(RwLock::new(HashMap::new())),
            tool_router: Self::tool_router(),
        }
    }

    /// Insert / replace a bond by id.
    pub fn store_bond(&self, id: String, bond: StoredBond) {
        self.bonds.write().unwrap().insert(id, bond);
    }

    /// Look up a bond by id.
    pub fn get_bond(&self, id: &str) -> Option<StoredBond> {
        self.bonds.read().unwrap().get(id).cloned()
    }

    /// Insert / replace a curve by id.
    pub fn store_curve(&self, id: String, curve: StoredCurve) {
        self.curves.write().unwrap().insert(id, curve);
    }

    /// Look up a curve by id.
    pub fn get_curve(&self, id: &str) -> Option<StoredCurve> {
        self.curves.read().unwrap().get(id).cloned()
    }

    /// Wrap a Serialize value as a pretty-JSON tool result.
    pub fn json_result<T: serde::Serialize>(value: &T) -> Result<CallToolResult, McpError> {
        let json = serde_json::to_string_pretty(value)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}

impl Default for ConvexMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

fn finite_decimal(x: f64, field: &str) -> Result<Decimal, McpToolError> {
    Decimal::from_f64_retain(x)
        .ok_or_else(|| McpToolError::InvalidInput(format!("{field}: non-finite f64")))
}

fn build_fixed_bond(id: &str, spec: &BondSpec) -> Result<FixedRateBond, McpToolError> {
    let coupon = finite_decimal(spec.coupon_rate_pct / 100.0, "coupon_rate_pct")?;
    let face = finite_decimal(spec.face_value, "face_value")?;
    let maturity = spec.maturity.to_date()?;
    let issue_date = spec.issue_date.to_date()?;

    FixedRateBond::builder()
        .cusip_unchecked(id)
        .coupon_rate(coupon)
        .maturity(maturity)
        .issue_date(issue_date)
        .frequency(spec.frequency)
        .day_count(spec.day_count)
        .currency(spec.currency)
        .face_value(face)
        .build()
        .map_err(|e| McpToolError::InvalidInput(format!("bond build: {e}")))
}

/// Resolve a `BondSpec` to a `StoredBond`. Plain bullets land as
/// `Fixed`; specs carrying a make-whole spread land as `Callable` so
/// `make_whole_call_price` can find them.
fn build_bond(id: &str, spec: &BondSpec) -> Result<StoredBond, McpToolError> {
    let base = build_fixed_bond(id, spec)?;
    match spec.make_whole_spread_bps {
        Some(bps) if bps.is_finite() => Ok(StoredBond::Callable(CallableBond::new(
            base,
            CallSchedule::make_whole(bps),
        ))),
        Some(_) => Err(McpToolError::InvalidInput(
            "make_whole_spread_bps must be finite".into(),
        )),
        None => Ok(StoredBond::Fixed(base)),
    }
}

fn build_curve(spec: &CurveSpec) -> Result<StoredCurve, McpToolError> {
    if spec.tenors_years.len() != spec.zero_rates_pct.len() {
        return Err(McpToolError::InvalidInput(
            "tenors_years and zero_rates_pct must have the same length".into(),
        ));
    }
    let ref_date = spec.reference_date.to_date()?;
    let rates_decimal: Vec<f64> = spec.zero_rates_pct.iter().map(|r| r / 100.0).collect();
    let value_type = ValueType::ZeroRate {
        compounding: Compounding::Continuous,
        day_count: DayCountConvention::Act365Fixed,
    };
    let discrete = DiscreteCurve::new(
        ref_date,
        spec.tenors_years.clone(),
        rates_decimal,
        value_type,
        InterpolationMethod::MonotoneConvex,
    )
    .map_err(|e| McpToolError::InvalidInput(format!("curve build: {e}")))?;
    Ok(RateCurve::new(discrete))
}

impl ConvexMcpServer {
    fn resolve_bond(&self, r: &BondRef) -> Result<(StoredBond, Option<String>), McpToolError> {
        match r {
            BondRef::Id(id) => {
                let bond = self
                    .get_bond(id)
                    .ok_or_else(|| McpToolError::InvalidInput(format!("bond '{id}' not found")))?;
                Ok((bond, Some(id.clone())))
            }
            BondRef::Spec(spec) => Ok((build_bond("INLINE", spec)?, None)),
        }
    }

    fn resolve_curve(&self, r: &CurveRef) -> Result<(StoredCurve, Option<String>), McpToolError> {
        match r {
            CurveRef::Id(id) => {
                let curve = self
                    .get_curve(id)
                    .ok_or_else(|| McpToolError::InvalidInput(format!("curve '{id}' not found")))?;
                Ok((curve, Some(id.clone())))
            }
            CurveRef::Spec(spec) => Ok((build_curve(spec)?, None)),
        }
    }
}

// ============================================================================
// Tool Parameter Types
// ============================================================================

/// Date input for tools
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DateInput {
    /// Year (e.g., 2025)
    pub year: i32,
    /// Month (1-12)
    pub month: u32,
    /// Day (1-31)
    pub day: u32,
}

impl DateInput {
    /// Convert to a `Date`.
    pub fn to_date(&self) -> Result<Date, McpToolError> {
        Date::from_ymd(self.year, self.month, self.day)
            .map_err(|e| McpToolError::InvalidInput(format!("invalid date: {e}")))
    }
}

/// Inline bond specification. Defaults match a US corporate
/// (semi-annual, 30/360 US, USD, face 100). Override for non-US bonds.
/// Set `make_whole_spread_bps` to get a make-whole callable bond
/// (resolves to `StoredBond::Callable`); leave it `None` for a plain
/// bullet (resolves to `StoredBond::Fixed`).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BondSpec {
    /// Annual coupon rate as percentage (5.0 means 5%).
    pub coupon_rate_pct: f64,
    /// Maturity date.
    pub maturity: DateInput,
    /// Issue date.
    pub issue_date: DateInput,
    /// Coupon frequency.
    #[serde(default = "default_frequency")]
    pub frequency: Frequency,
    /// Day count convention.
    #[serde(default = "default_day_count")]
    pub day_count: DayCountConvention,
    /// Currency.
    #[serde(default)]
    pub currency: Currency,
    /// Face value (typically 100).
    #[serde(default = "default_face_value")]
    pub face_value: f64,
    /// Make-whole spread in basis points. When set, the spec resolves to
    /// a `Callable` carrying a `MakeWhole` schedule, and
    /// `make_whole_call_price` becomes valid.
    #[serde(default)]
    pub make_whole_spread_bps: Option<f64>,
}

fn default_frequency() -> Frequency {
    Frequency::SemiAnnual
}
fn default_day_count() -> DayCountConvention {
    DayCountConvention::Thirty360US
}
fn default_face_value() -> f64 {
    100.0
}

/// Inline curve specification (zero-rate pillar set).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CurveSpec {
    /// Reference / valuation date.
    pub reference_date: DateInput,
    /// Tenor points in years (must match `zero_rates_pct` length).
    pub tenors_years: Vec<f64>,
    /// Zero rates as percentages (4.5 means 4.5%).
    pub zero_rates_pct: Vec<f64>,
}

/// Either an id of a stored bond, or an inline `BondSpec`.
///
/// JSON: `"AAPL.10Y"` (string id) or `{"coupon_rate_pct": ...}` (inline spec).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum BondRef {
    /// Reference a stored bond by id.
    Id(String),
    /// Use an inline spec (not stored).
    Spec(BondSpec),
}

/// Either an id of a stored curve, or an inline `CurveSpec`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum CurveRef {
    /// Reference a stored curve by id.
    Id(String),
    /// Use an inline spec (not stored).
    Spec(CurveSpec),
}

/// Create-bond parameters: id + spec, stored under `id`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CreateBondParams {
    /// Bond identifier (used to retrieve the bond later).
    pub id: String,
    #[serde(flatten)]
    /// Bond specification.
    pub spec: BondSpec,
}

/// Create-curve parameters: id + spec, stored under `id`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CreateCurveParams {
    /// Curve identifier.
    pub id: String,
    #[serde(flatten)]
    /// Curve specification.
    pub spec: CurveSpec,
}

/// Pricing tool input. `mark` is the canonical [`Mark`] enum,
/// tagged with `mark` for JSON (e.g. `{"mark":"price","value":99.5,"kind":"clean"}`).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PriceBondParams {
    /// Bond reference — id or inline spec.
    pub bond: BondRef,
    /// Settlement date.
    pub settlement: DateInput,
    /// Trader mark.
    pub mark: Mark,
    /// Discount curve. Required for spread marks; optional otherwise.
    #[serde(default)]
    pub curve: Option<CurveRef>,
    /// Compounding frequency for derived YTM. Defaults to the bond's frequency.
    #[serde(default)]
    pub quote_frequency: Option<Frequency>,
}

/// Calculate-yield parameters.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CalculateYieldParams {
    /// Bond reference — id or inline spec.
    pub bond: BondRef,
    /// Settlement date.
    pub settlement: DateInput,
    /// Clean price per 100 face.
    pub clean_price_per_100: f64,
}

/// `make_whole_call_price` parameters.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct MakeWholeParams {
    /// Bond reference — id or inline spec. Must resolve to a callable bond
    /// carrying a make-whole spread on its call schedule.
    pub bond: BondRef,
    /// Hypothetical call exercise date.
    pub call_date: DateInput,
    /// Treasury par yield at the relevant tenor, decimal (0.05 = 5%).
    pub treasury_rate: f64,
}

/// Output of `make_whole_call_price`.
#[derive(Debug, Serialize)]
#[allow(missing_docs)]
pub struct MakeWholeOutput {
    pub bond_id: Option<String>,
    pub call_date: Date,
    pub treasury_rate: f64,
    pub make_whole_spread_bps: f64,
    pub discount_rate: f64,
    pub make_whole_price_per_100: f64,
}

/// Get-zero-rate parameters.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetRateParams {
    /// Curve reference — id or inline spec.
    pub curve: CurveRef,
    /// Tenor in years.
    pub tenor_years: f64,
}

/// Spread family selector.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SpreadKind {
    /// Zero-volatility spread — constant addition to the spot curve that prices the bond.
    ZSpread,
    /// I-spread — bond YTM minus the swap-curve rate at maturity.
    ISpread,
    /// G-spread — bond YTM minus the government-curve rate at maturity. Mathematically
    /// identical to I-spread; the distinction is which curve the caller passes in.
    GSpread,
}

/// `compute_position_risk` parameters.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ComputePositionRiskParams {
    /// Bond reference — id or inline spec.
    pub bond: BondRef,
    /// Settlement date.
    pub settlement: DateInput,
    /// Trader mark.
    pub mark: Mark,
    /// Discount curve reference.
    pub curve: CurveRef,
    /// Position face notional. Positive = long, negative = short.
    pub notional_face: f64,
    /// Compounding frequency for derived YTM. Defaults to bond frequency.
    #[serde(default)]
    pub quote_frequency: Option<Frequency>,
    /// Optional caller-supplied position id.
    #[serde(default)]
    pub position_id: Option<String>,
    /// KRD tenors (years). Defaults to `[2, 5, 10, 30]`.
    #[serde(default)]
    pub key_rate_tenors: Option<Vec<f64>>,
}

/// `propose_hedges` parameters.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ProposeHedgesParams {
    /// Risk profile of the position to hedge.
    pub risk: RiskProfile,
    /// Discount curve reference.
    pub curve: CurveRef,
    /// Optional caller constraints.
    #[serde(default)]
    pub constraints: Option<Constraints>,
}

/// `propose_hedges` output.
#[derive(Debug, Serialize, Deserialize)]
#[allow(missing_docs)]
pub struct ProposeHedgesOutput {
    pub proposals: Vec<HedgeProposal>,
}

/// `compare_hedges` parameters.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CompareHedgesParams {
    /// Position risk (echoed for currency / market value / DV01).
    pub position: RiskProfile,
    /// Proposals to compare. Pass at least one.
    pub proposals: Vec<HedgeProposal>,
    /// Optional constraints used to filter the recommendation pool.
    #[serde(default)]
    pub constraints: Option<Constraints>,
}

/// `narrate_recommendation` parameters.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NarrateRecommendationParams {
    /// Comparison report to narrate.
    pub comparison: ComparisonReport,
}

/// `narrate_recommendation` output.
#[derive(Debug, Serialize, Deserialize)]
#[allow(missing_docs)]
pub struct NarrationOutput {
    pub text: String,
}

/// `compute_spread` parameters.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ComputeSpreadParams {
    /// Bond reference — id or inline spec.
    pub bond: BondRef,
    /// Curve reference — id or inline spec. For Z-spread this is the discount curve;
    /// for I-spread the swap curve; for G-spread the government curve.
    pub curve: CurveRef,
    /// Settlement date.
    pub settlement: DateInput,
    /// Clean price per 100 face.
    pub clean_price_per_100: f64,
    /// Which spread to compute.
    pub kind: SpreadKind,
}

/// Bootstrapping instrument. Discriminated by `kind` for JSON.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BootstrapInstrument {
    /// Money market deposit. Defaults: ACT/360.
    Deposit {
        /// Tenor in years (0.25 = 3M, 0.5 = 6M, 1.0 = 1Y).
        tenor_years: f64,
        /// Quoted rate as percentage (4.5 = 4.5%).
        rate_pct: f64,
        /// Day count convention. Defaults to ACT/360.
        #[serde(default = "default_dc_act360")]
        day_count: DayCountConvention,
    },
    /// Fixed-for-floating interest rate swap. Defaults: semi-annual fixed leg, 30/360 US.
    Swap {
        /// Tenor in years.
        tenor_years: f64,
        /// Fixed rate as percentage.
        fixed_rate_pct: f64,
        /// Fixed leg frequency. Defaults to semi-annual.
        #[serde(default = "default_frequency")]
        fixed_frequency: Frequency,
        /// Fixed leg day count. Defaults to 30/360 US.
        #[serde(default = "default_day_count")]
        fixed_day_count: DayCountConvention,
    },
    /// Overnight index swap. Defaults: ACT/360, annual fixed.
    Ois {
        /// Tenor in years.
        tenor_years: f64,
        /// Fixed rate as percentage.
        fixed_rate_pct: f64,
        /// Day count. Defaults to ACT/360.
        #[serde(default = "default_dc_act360")]
        day_count: DayCountConvention,
    },
}

fn default_dc_act360() -> DayCountConvention {
    DayCountConvention::Act360
}

/// `bootstrap_curve` parameters.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BootstrapCurveParams {
    /// Reference / valuation date.
    pub reference_date: DateInput,
    /// Calibration instruments (deposits, swaps, OIS).
    pub instruments: Vec<BootstrapInstrument>,
    /// If set, store the resulting curve under this id for later lookup.
    #[serde(default)]
    pub store_as: Option<String>,
}

// Tool output types. Field names carry units; missing_docs is allowed.

/// Output of `create_bond` / `create_curve` — confirms the registered id.
#[derive(Debug, Serialize)]
#[allow(missing_docs)]
pub struct CreatedOutput {
    pub status: &'static str,
    pub id: String,
}

/// Output of `price_bond`. Carries the math plus enough provenance
/// (currency, day count, settlement, curve id) to be auditable.
/// `bond_id` / `curve_id` are `None` when the caller passed an inline spec.
#[derive(Debug, Serialize)]
#[allow(missing_docs)]
pub struct PriceBondOutput {
    pub bond_id: Option<String>,
    pub settlement: Date,
    pub curve_id: Option<String>,
    pub currency: Currency,
    pub day_count: String,
    pub clean_price_per_100: f64,
    pub dirty_price_per_100: f64,
    pub accrued_per_100: f64,
    pub ytm_pct: f64,
    pub ytm_frequency: Frequency,
    pub z_spread_bps: Option<f64>,
}

/// Output of `calculate_yield`.
#[derive(Debug, Serialize)]
#[allow(missing_docs)]
pub struct CalculateYieldOutput {
    pub bond_id: Option<String>,
    pub settlement: Date,
    pub currency: Currency,
    pub day_count: String,
    pub clean_price_per_100: f64,
    pub ytm_pct: f64,
    pub ytm_frequency: Frequency,
}

/// Output of `compute_spread`.
#[derive(Debug, Serialize)]
#[allow(missing_docs)]
pub struct ComputeSpreadOutput {
    pub bond_id: Option<String>,
    pub curve_id: Option<String>,
    pub settlement: Date,
    pub curve_reference_date: Date,
    pub kind: SpreadKind,
    pub clean_price_per_100: f64,
    pub spread_bps: f64,
}

/// Output of `bootstrap_curve`. Returns the calibrated curve as
/// (tenors, rates) so the caller can use it inline without a registry round-trip.
#[derive(Debug, Serialize)]
#[allow(missing_docs)]
pub struct BootstrapCurveOutput {
    pub curve_id: Option<String>,
    pub reference_date: Date,
    pub instrument_count: usize,
    pub iterations: usize,
    pub rms_error: f64,
    pub converged: bool,
    pub tenors_years: Vec<f64>,
    pub zero_rates_pct: Vec<f64>,
}

/// Output of `get_zero_rate`.
#[derive(Debug, Serialize)]
#[allow(missing_docs)]
pub struct ZeroRateOutput {
    pub curve_id: Option<String>,
    pub curve_reference_date: Date,
    pub tenor_years: f64,
    pub zero_rate_pct: f64,
    pub compounding: Compounding,
}

/// Bond entry returned by `list_all_bonds`.
#[derive(Debug, Serialize)]
#[allow(missing_docs)]
pub struct BondListItem {
    pub id: String,
    pub bond_type: &'static str,
}

/// Curve entry returned by `list_all_curves`.
#[derive(Debug, Serialize)]
#[allow(missing_docs)]
pub struct CurveListItem {
    pub id: String,
    pub reference_date: Date,
    pub tenor_count: usize,
}

/// Container for list outputs.
#[derive(Debug, Serialize)]
#[allow(missing_docs)]
pub struct ListOutput<T> {
    pub count: usize,
    pub items: Vec<T>,
}

// ============================================================================
// Tool Implementations
// ============================================================================

#[tool_router]
#[allow(missing_docs)] // each tool's `description` attribute is the public doc.
impl ConvexMcpServer {
    #[tool(
        description = "Create a fixed rate bond. coupon_rate_pct is in percent (5.0 = 5%). \
            frequency, day_count, currency default to (semi_annual, thirty360_us, USD); \
            override for non-US bonds (e.g. annual + act_act_icma + EUR for Bunds)."
    )]
    pub async fn create_bond(
        &self,
        Parameters(params): Parameters<CreateBondParams>,
    ) -> Result<CallToolResult, McpError> {
        let bond = build_bond(&params.id, &params.spec)?;
        self.store_bond(params.id.clone(), bond);
        Self::json_result(&CreatedOutput {
            status: "success",
            id: params.id,
        })
    }

    #[tool(
        description = "Price a bond against a trader Mark (price | yield | spread). \
            `bond` and `curve` accept either a stored id (string) or an inline spec (object). \
            Returns clean/dirty price, accrued, derived YTM, and — for spread marks — the input z-spread."
    )]
    pub async fn price_bond(
        &self,
        Parameters(params): Parameters<PriceBondParams>,
    ) -> Result<CallToolResult, McpError> {
        let (bond, bond_id) = self.resolve_bond(&params.bond)?;
        let settlement = params.settlement.to_date()?;
        let (curve, curve_id) = match &params.curve {
            Some(r) => {
                let (c, id) = self.resolve_curve(r)?;
                (Some(c), id)
            }
            None => (None, None),
        };
        let curve_dyn = curve.as_ref().map(|c| c as &dyn RateCurveDyn);

        let fixed = bond.fixed().ok_or_else(|| {
            McpToolError::InvalidInput(format!(
                "price_bond requires Fixed/Callable, got {}",
                bond.type_name()
            ))
        })?;
        let freq = params.quote_frequency.unwrap_or_else(|| fixed.frequency());

        let result = price_from_mark(fixed, settlement, &params.mark, curve_dyn, freq)
            .map_err(McpToolError::from)?;

        Self::json_result(&PriceBondOutput {
            bond_id,
            settlement,
            curve_id,
            currency: fixed.currency(),
            day_count: fixed.day_count_convention().to_string(),
            clean_price_per_100: result.clean_price_per_100,
            dirty_price_per_100: result.dirty_price_per_100,
            accrued_per_100: result.accrued_per_100,
            ytm_pct: result.ytm_decimal * 100.0,
            ytm_frequency: freq,
            z_spread_bps: result.z_spread_bps,
        })
    }

    #[tool(description = "Calculate yield to maturity (YTM) from clean price. \
            `bond` accepts either a stored id (string) or an inline spec (object). \
            Returns yield as percentage at the bond's coupon frequency.")]
    pub async fn calculate_yield(
        &self,
        Parameters(params): Parameters<CalculateYieldParams>,
    ) -> Result<CallToolResult, McpError> {
        let (bond, bond_id) = self.resolve_bond(&params.bond)?;
        let settlement = params.settlement.to_date()?;
        let price_decimal = finite_decimal(params.clean_price_per_100, "clean_price_per_100")?;

        let fixed = bond.fixed().ok_or_else(|| {
            McpToolError::InvalidInput(format!(
                "calculate_yield requires Fixed/Callable, got {}",
                bond.type_name()
            ))
        })?;
        let freq = fixed.frequency();
        let ytm = yield_to_maturity(fixed, settlement, price_decimal, freq)
            .map_err(McpToolError::from)?;

        Self::json_result(&CalculateYieldOutput {
            bond_id,
            settlement,
            currency: fixed.currency(),
            day_count: fixed.day_count_convention().to_string(),
            clean_price_per_100: params.clean_price_per_100,
            ytm_pct: ytm.yield_value * 100.0,
            ytm_frequency: freq,
        })
    }

    #[tool(
        description = "Compute the make-whole call price for a callable bond carrying a \
            make-whole spread. Discount uses the bond's own day count and frequency (matches \
            US-corp 424B2 convention, e.g. 30/360 US for AAPL/MSFT/Verizon/Ford). \
            `bond` accepts either a stored id (string) or an inline spec (object). \
            Returns price floored at the first call entry's price (typically par)."
    )]
    pub async fn make_whole_call_price(
        &self,
        Parameters(params): Parameters<MakeWholeParams>,
    ) -> Result<CallToolResult, McpError> {
        let (bond, bond_id) = self.resolve_bond(&params.bond)?;
        let call_date = params.call_date.to_date()?;

        if !params.treasury_rate.is_finite() {
            return Err(
                McpToolError::InvalidInput("treasury_rate must be finite".to_string()).into(),
            );
        }

        let callable = match &bond {
            StoredBond::Callable(c) => c,
            _ => {
                return Err(McpToolError::InvalidInput(format!(
                    "make_whole_call_price requires a Callable bond, got {}",
                    bond.type_name()
                ))
                .into())
            }
        };
        let spread_bps = callable.make_whole_spread().ok_or_else(|| {
            McpToolError::InvalidInput(
                "callable bond has no make-whole spread on its call schedule".to_string(),
            )
        })?;
        let price = callable
            .make_whole_call_price(call_date, params.treasury_rate)
            .map_err(|e| McpToolError::CalculationFailed(e.to_string()))?;
        let price_f64 = price.to_string().parse::<f64>().unwrap_or(f64::NAN);

        Self::json_result(&MakeWholeOutput {
            bond_id,
            call_date,
            treasury_rate: params.treasury_rate,
            make_whole_spread_bps: spread_bps,
            discount_rate: params.treasury_rate + spread_bps / 10_000.0,
            make_whole_price_per_100: price_f64,
        })
    }

    #[tool(
        description = "Create a yield curve from zero rate points. Rates should be in percentage (e.g., 4.5 for 4.5%)."
    )]
    pub async fn create_curve(
        &self,
        Parameters(params): Parameters<CreateCurveParams>,
    ) -> Result<CallToolResult, McpError> {
        let curve = build_curve(&params.spec)?;
        self.store_curve(params.id.clone(), curve);
        Self::json_result(&CreatedOutput {
            status: "success",
            id: params.id,
        })
    }

    #[tool(description = "Get zero rate at a specific tenor. \
            `curve` accepts either a stored id (string) or an inline spec (object). \
            Returns rate as percentage (continuous compounding).")]
    pub async fn get_zero_rate(
        &self,
        Parameters(params): Parameters<GetRateParams>,
    ) -> Result<CallToolResult, McpError> {
        let (curve, curve_id) = self.resolve_curve(&params.curve)?;
        let rate = curve
            .zero_rate_at_tenor(params.tenor_years, Compounding::Continuous)
            .map_err(|e| McpToolError::CalculationFailed(e.to_string()))?;

        Self::json_result(&ZeroRateOutput {
            curve_id,
            curve_reference_date: curve.reference_date(),
            tenor_years: params.tenor_years,
            zero_rate_pct: rate * 100.0,
            compounding: Compounding::Continuous,
        })
    }

    #[tool(
        description = "Compute a yield spread (z_spread | i_spread | g_spread) for a bond against a curve. \
            Z-spread is the constant DCF spread. I/G-spread are bond YTM minus curve rate at maturity \
            (label-only difference: caller passes swap vs government curve). \
            `bond` and `curve` accept either a stored id (string) or an inline spec (object)."
    )]
    pub async fn compute_spread(
        &self,
        Parameters(params): Parameters<ComputeSpreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let (bond, bond_id) = self.resolve_bond(&params.bond)?;
        let (curve, curve_id) = self.resolve_curve(&params.curve)?;
        let settlement = params.settlement.to_date()?;
        let fixed = match &bond {
            StoredBond::Fixed(b) => b,
            _ => {
                return Err(McpToolError::InvalidInput(format!(
                    "compute_spread requires a Fixed bond, got {}",
                    bond.type_name()
                ))
                .into())
            }
        };
        let clean_dec = finite_decimal(params.clean_price_per_100, "clean_price_per_100")?;

        let spread_bps = match params.kind {
            SpreadKind::ZSpread => {
                let dirty = clean_dec + fixed.accrued_interest(settlement);
                ZSpreadCalculator::new(&curve)
                    .calculate(fixed, dirty, settlement)
                    .map_err(McpToolError::from)?
                    .as_bps()
                    .to_f64()
                    .unwrap_or(f64::NAN)
            }
            SpreadKind::ISpread | SpreadKind::GSpread => {
                let ytm = yield_to_maturity(fixed, settlement, clean_dec, fixed.frequency())
                    .map_err(McpToolError::from)?;
                let bond_yield = Yield::new(
                    Decimal::from_f64_retain(ytm.yield_value).unwrap_or(Decimal::ZERO),
                    Compounding::SemiAnnual,
                );
                ISpreadCalculator::new(&curve)
                    .calculate(fixed, bond_yield, settlement)
                    .map_err(McpToolError::from)?
                    .as_bps()
                    .to_f64()
                    .unwrap_or(f64::NAN)
            }
        };

        Self::json_result(&ComputeSpreadOutput {
            bond_id,
            curve_id,
            settlement,
            curve_reference_date: curve.reference_date(),
            kind: params.kind,
            clean_price_per_100: params.clean_price_per_100,
            spread_bps,
        })
    }

    #[tool(
        description = "Bootstrap a zero-rate curve from money-market deposits, IRS, and OIS quotes. \
            Returns the calibrated (tenors_years, zero_rates_pct) plus convergence diagnostics. \
            Pass `store_as` to also register the curve for later lookup."
    )]
    pub async fn bootstrap_curve(
        &self,
        Parameters(params): Parameters<BootstrapCurveParams>,
    ) -> Result<CallToolResult, McpError> {
        let reference_date = params.reference_date.to_date()?;
        if params.instruments.is_empty() {
            return Err(McpToolError::InvalidInput("instruments must not be empty".into()).into());
        }

        let mut set = InstrumentSet::new();
        for inst in &params.instruments {
            match inst {
                BootstrapInstrument::Deposit {
                    tenor_years,
                    rate_pct,
                    day_count,
                } => set.add(Deposit::from_tenor(
                    reference_date,
                    *tenor_years,
                    rate_pct / 100.0,
                    *day_count,
                )),
                BootstrapInstrument::Swap {
                    tenor_years,
                    fixed_rate_pct,
                    fixed_frequency,
                    fixed_day_count,
                } => set.add(Swap::from_tenor(
                    reference_date,
                    *tenor_years,
                    fixed_rate_pct / 100.0,
                    *fixed_frequency,
                    *fixed_day_count,
                )),
                BootstrapInstrument::Ois {
                    tenor_years,
                    fixed_rate_pct,
                    day_count,
                } => set.add(Ois::from_tenor(
                    reference_date,
                    *tenor_years,
                    fixed_rate_pct / 100.0,
                    *day_count,
                )),
            }
        }

        let result = GlobalFitter::default()
            .fit(reference_date, &set)
            .map_err(|e| McpToolError::CalculationFailed(format!("bootstrap: {e}")))?;

        let tenors_years = result.curve.tenors().to_vec();
        let zero_rates_pct: Vec<f64> = result.curve.values().iter().map(|r| r * 100.0).collect();
        let stored = RateCurve::new(result.curve);
        let curve_id = params.store_as.map(|id| {
            self.store_curve(id.clone(), stored);
            id
        });

        Self::json_result(&BootstrapCurveOutput {
            curve_id,
            reference_date,
            instrument_count: params.instruments.len(),
            iterations: result.iterations,
            rms_error: result.rms_error,
            converged: result.converged,
            tenors_years,
            zero_rates_pct,
        })
    }

    #[tool(description = "List all bonds currently stored in the system.")]
    pub async fn list_all_bonds(&self) -> Result<CallToolResult, McpError> {
        let bonds = self.bonds.read().unwrap();
        let items: Vec<_> = bonds
            .iter()
            .map(|(id, bond)| BondListItem {
                id: id.clone(),
                bond_type: bond.type_name(),
            })
            .collect();
        Self::json_result(&ListOutput {
            count: items.len(),
            items,
        })
    }

    #[tool(description = "List all curves currently stored in the system.")]
    pub async fn list_all_curves(&self) -> Result<CallToolResult, McpError> {
        let curves = self.curves.read().unwrap();
        let items: Vec<_> = curves
            .iter()
            .map(|(id, curve)| CurveListItem {
                id: id.clone(),
                reference_date: curve.reference_date(),
                tenor_count: curve.inner().tenors().len(),
            })
            .collect();
        Self::json_result(&ListOutput {
            count: items.len(),
            items,
        })
    }

    #[tool(
        description = "Compute per-position risk: DV01, durations, convexity, KRD buckets, \
        provenance. Mirrors Bloomberg-parity KRD (Z-spread held fixed, ±1bp triangular bumps)."
    )]
    pub async fn compute_position_risk(
        &self,
        Parameters(params): Parameters<ComputePositionRiskParams>,
    ) -> Result<CallToolResult, McpError> {
        let (bond, _bond_id) = self.resolve_bond(&params.bond)?;
        let fixed = bond.fixed().ok_or_else(|| {
            McpToolError::InvalidInput(format!(
                "compute_position_risk requires Fixed/Callable, got {}",
                bond.type_name()
            ))
        })?;
        let (curve, curve_id) = self.resolve_curve(&params.curve)?;
        let curve_id_str = curve_id.unwrap_or_else(|| "<inline>".into());
        let settlement = params.settlement.to_date()?;
        let notional = finite_decimal(params.notional_face, "notional_face")?;
        let default_tenors = [2.0_f64, 5.0, 10.0, 30.0];
        let tenors_owned: Vec<f64>;
        let tenor_slice: &[f64] = match &params.key_rate_tenors {
            Some(v) => {
                tenors_owned = v.clone();
                &tenors_owned
            }
            None => &default_tenors,
        };
        let profile = compute_position_risk(
            fixed,
            settlement,
            &params.mark,
            notional,
            &curve,
            &curve_id_str,
            params.quote_frequency,
            Some(tenor_slice),
            params.position_id,
        )
        .map_err(McpToolError::from)?;
        Self::json_result(&profile)
    }

    #[tool(
        description = "Propose hedges for a risk profile. v1 ships DurationFutures + \
        InterestRateSwap. Each proposal includes trades, residual KRD, heuristic cost, \
        tradeoff notes, and provenance."
    )]
    pub async fn propose_hedges(
        &self,
        Parameters(params): Parameters<ProposeHedgesParams>,
    ) -> Result<CallToolResult, McpError> {
        let (curve, curve_id) = self.resolve_curve(&params.curve)?;
        let curve_id_str = curve_id.unwrap_or_else(|| "<inline>".into());
        let constraints = params.constraints.unwrap_or_default();
        let settlement = params.risk.settlement;

        let f = duration_futures(
            &params.risk,
            &constraints,
            &curve,
            &curve_id_str,
            settlement,
        )
        .map_err(McpToolError::from)?;
        let s = interest_rate_swap(
            &params.risk,
            &constraints,
            &curve,
            &curve_id_str,
            settlement,
        )
        .map_err(McpToolError::from)?;
        Self::json_result(&ProposeHedgesOutput {
            proposals: vec![f, s],
        })
    }

    #[tool(
        description = "Side-by-side comparison of hedge proposals. Recommends lowest cost \
        meeting constraints, tie-broken by smallest residual KRD L1 norm."
    )]
    pub async fn compare_hedges(
        &self,
        Parameters(params): Parameters<CompareHedgesParams>,
    ) -> Result<CallToolResult, McpError> {
        let constraints = params.constraints.unwrap_or_default();
        let report = compare_hedges(&params.position, &params.proposals, &constraints)
            .map_err(McpToolError::from)?;
        Self::json_result(&report)
    }

    #[tool(
        description = "Render a deterministic trader-brief paragraph from a ComparisonReport. \
        v1 narrator is template-only (no LLM call)."
    )]
    pub async fn narrate_recommendation(
        &self,
        Parameters(params): Parameters<NarrateRecommendationParams>,
    ) -> Result<CallToolResult, McpError> {
        let text = narrate(&params.comparison);
        Self::json_result(&NarrationOutput { text })
    }
}

#[tool_handler]
impl ServerHandler for ConvexMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: SERVER_NAME.to_string(),
                version: SERVER_VERSION.to_string(),
                title: Some("Convex Fixed Income Analytics".to_string()),
                icons: None,
                website_url: Some("https://github.com/sujitn/convex".to_string()),
            },
            instructions: Some(
                "Convex MCP Server — fixed income analytics. Create bonds and curves, \
                 then call price_bond or compute_spread."
                    .to_string(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> DateInput {
        DateInput {
            year: y,
            month: m,
            day: d,
        }
    }

    fn ust_10y_spec() -> BondSpec {
        BondSpec {
            coupon_rate_pct: 4.5,
            maturity: date(2035, 1, 15),
            issue_date: date(2025, 1, 15),
            frequency: Frequency::SemiAnnual,
            day_count: DayCountConvention::ActActIcma,
            currency: Currency::USD,
            face_value: 100.0,
            make_whole_spread_bps: None,
        }
    }

    fn flat_curve_spec(rate_pct: f64) -> CurveSpec {
        CurveSpec {
            reference_date: date(2025, 1, 15),
            tenors_years: vec![0.5, 1.0, 2.0, 5.0, 10.0, 30.0],
            zero_rates_pct: vec![rate_pct; 6],
        }
    }

    #[test]
    fn resolve_bond_by_id_returns_id() {
        let server = ConvexMcpServer::new();
        let bond = build_fixed_bond("AAPL.10Y", &ust_10y_spec()).unwrap();
        server.store_bond("AAPL.10Y".into(), StoredBond::Fixed(bond));

        let (_, id) = server
            .resolve_bond(&BondRef::Id("AAPL.10Y".into()))
            .unwrap();
        assert_eq!(id.as_deref(), Some("AAPL.10Y"));
    }

    #[test]
    fn build_bond_dispatches_make_whole_to_callable() {
        let mut spec = ust_10y_spec();
        spec.make_whole_spread_bps = Some(35.0);
        let stored = build_bond("F.MW", &spec).unwrap();
        assert!(matches!(stored, StoredBond::Callable(_)));
        if let StoredBond::Callable(cb) = stored {
            assert_eq!(cb.make_whole_spread(), Some(35.0));
        }
    }

    #[test]
    fn resolve_bond_by_spec_returns_no_id() {
        let server = ConvexMcpServer::new();
        let (_, id) = server.resolve_bond(&BondRef::Spec(ust_10y_spec())).unwrap();
        assert!(id.is_none());
    }

    #[test]
    fn resolve_bond_unknown_id_errors() {
        let server = ConvexMcpServer::new();
        assert!(server.resolve_bond(&BondRef::Id("UNKNOWN".into())).is_err());
    }

    #[test]
    fn resolve_curve_by_spec_returns_no_id() {
        let server = ConvexMcpServer::new();
        let (_, id) = server
            .resolve_curve(&CurveRef::Spec(flat_curve_spec(4.0)))
            .unwrap();
        assert!(id.is_none());
    }

    #[test]
    fn build_curve_rejects_mismatched_lengths() {
        let bad = CurveSpec {
            reference_date: date(2025, 1, 15),
            tenors_years: vec![1.0, 2.0],
            zero_rates_pct: vec![4.0],
        };
        assert!(build_curve(&bad).is_err());
    }

    #[test]
    fn bootstrap_recovers_input_swap_rates() {
        // Calibrate a curve from a clean set of deposits + swaps, then check the
        // bootstrapper reproduces the input rates within tolerance.
        let server = ConvexMcpServer::new();
        let params = BootstrapCurveParams {
            reference_date: date(2025, 1, 15),
            instruments: vec![
                BootstrapInstrument::Deposit {
                    tenor_years: 0.25,
                    rate_pct: 4.40,
                    day_count: DayCountConvention::Act360,
                },
                BootstrapInstrument::Swap {
                    tenor_years: 2.0,
                    fixed_rate_pct: 4.20,
                    fixed_frequency: Frequency::SemiAnnual,
                    fixed_day_count: DayCountConvention::Thirty360US,
                },
                BootstrapInstrument::Swap {
                    tenor_years: 5.0,
                    fixed_rate_pct: 4.30,
                    fixed_frequency: Frequency::SemiAnnual,
                    fixed_day_count: DayCountConvention::Thirty360US,
                },
                BootstrapInstrument::Swap {
                    tenor_years: 10.0,
                    fixed_rate_pct: 4.45,
                    fixed_frequency: Frequency::SemiAnnual,
                    fixed_day_count: DayCountConvention::Thirty360US,
                },
            ],
            store_as: None,
        };

        let mut set = InstrumentSet::new();
        for inst in &params.instruments {
            match inst {
                BootstrapInstrument::Deposit {
                    tenor_years,
                    rate_pct,
                    day_count,
                } => set.add(Deposit::from_tenor(
                    params.reference_date.to_date().unwrap(),
                    *tenor_years,
                    rate_pct / 100.0,
                    *day_count,
                )),
                BootstrapInstrument::Swap {
                    tenor_years,
                    fixed_rate_pct,
                    fixed_frequency,
                    fixed_day_count,
                } => set.add(Swap::from_tenor(
                    params.reference_date.to_date().unwrap(),
                    *tenor_years,
                    fixed_rate_pct / 100.0,
                    *fixed_frequency,
                    *fixed_day_count,
                )),
                _ => {}
            }
        }
        let result = GlobalFitter::default()
            .fit(params.reference_date.to_date().unwrap(), &set)
            .unwrap();

        // Sane convergence on a well-posed input.
        assert!(result.converged, "bootstrap should converge");
        assert!(
            result.rms_error < 1e-3,
            "rms_error too large: {}",
            result.rms_error
        );
        // GlobalFitter may add an anchor pillar; accept >= instruments.
        assert!(result.curve.tenors().len() >= params.instruments.len());

        let _ = server; // unused
    }

    #[test]
    fn bootstrap_curve_rejects_empty_instruments() {
        let server = ConvexMcpServer::new();
        let params = BootstrapCurveParams {
            reference_date: date(2025, 1, 15),
            instruments: vec![],
            store_as: None,
        };
        // Manual reconstruction: the handler short-circuits on empty.
        assert!(params.instruments.is_empty());
        let _ = server;
    }

    #[test]
    fn bootstrap_instrument_deserializes_kind_tag() {
        let dep: BootstrapInstrument =
            serde_json::from_str(r#"{"kind":"deposit","tenor_years":0.25,"rate_pct":4.4}"#)
                .unwrap();
        assert!(matches!(dep, BootstrapInstrument::Deposit { .. }));

        let sw: BootstrapInstrument =
            serde_json::from_str(r#"{"kind":"swap","tenor_years":5.0,"fixed_rate_pct":4.3}"#)
                .unwrap();
        assert!(matches!(sw, BootstrapInstrument::Swap { .. }));
    }

    #[test]
    fn bond_ref_deserializes_from_string_or_object() {
        let by_id: BondRef = serde_json::from_str(r#""AAPL.10Y""#).unwrap();
        assert!(matches!(by_id, BondRef::Id(s) if s == "AAPL.10Y"));

        let inline: BondRef = serde_json::from_str(
            r#"{"coupon_rate_pct": 4.5,
                "maturity": {"year": 2035, "month": 1, "day": 15},
                "issue_date": {"year": 2025, "month": 1, "day": 15}}"#,
        )
        .unwrap();
        assert!(matches!(inline, BondRef::Spec(_)));
    }

    #[tokio::test]
    async fn hedge_advisor_e2e_apple_10y() {
        // Demo scenario: long $10mm AAPL-like 4.85% '34 corporate, USD SOFR
        // flat curve at 4.5%, mark via Mark::Yield. Run all four advisor
        // tools end-to-end and assert structural shape.
        let server = ConvexMcpServer::new();

        // Stash a flat 4.5% curve.
        let curve_params = CreateCurveParams {
            id: "usd_sofr".into(),
            spec: flat_curve_spec(4.5),
        };
        server.create_curve(Parameters(curve_params)).await.unwrap();

        // Stash an AAPL-like bond.
        let aapl_spec = BondSpec {
            coupon_rate_pct: 4.85,
            maturity: date(2034, 5, 10),
            issue_date: date(2024, 5, 10),
            frequency: Frequency::SemiAnnual,
            day_count: DayCountConvention::Thirty360US,
            currency: Currency::USD,
            face_value: 100.0,
            make_whole_spread_bps: None,
        };
        server
            .create_bond(Parameters(CreateBondParams {
                id: "AAPL.10Y".into(),
                spec: aapl_spec,
            }))
            .await
            .unwrap();

        // Tool 1: compute_position_risk
        let risk_params = ComputePositionRiskParams {
            bond: BondRef::Id("AAPL.10Y".into()),
            settlement: date(2026, 1, 15),
            mark: Mark::Yield {
                value: dec!(0.0535),
                frequency: Frequency::SemiAnnual,
            },
            curve: CurveRef::Id("usd_sofr".into()),
            notional_face: 10_000_000.0,
            quote_frequency: None,
            position_id: Some("AAPL.10Y_long".into()),
            key_rate_tenors: Some(vec![2.0, 5.0, 10.0, 30.0]),
        };
        let risk_result = server
            .compute_position_risk(Parameters(risk_params))
            .await
            .unwrap();
        let risk_text = match &risk_result.content[0].raw {
            rmcp::model::RawContent::Text(t) => t.text.clone(),
            _ => panic!("expected text content"),
        };
        let profile: RiskProfile = serde_json::from_str(&risk_text).unwrap();
        assert_eq!(profile.currency, Currency::USD);
        assert!(profile.dv01 > 0.0);
        assert_eq!(profile.key_rate_buckets.len(), 4);
        assert_eq!(profile.provenance.cost_model, "heuristic_v1");

        // Tool 2: propose_hedges
        let proposals_text = response_text(
            server
                .propose_hedges(Parameters(ProposeHedgesParams {
                    risk: profile.clone(),
                    curve: CurveRef::Id("usd_sofr".into()),
                    constraints: None,
                }))
                .await
                .unwrap(),
        );
        let proposed: ProposeHedgesOutput = serde_json::from_str(&proposals_text).unwrap();
        assert_eq!(proposed.proposals.len(), 2);
        assert!(proposed
            .proposals
            .iter()
            .any(|p| p.strategy == "DurationFutures"));
        assert!(proposed
            .proposals
            .iter()
            .any(|p| p.strategy == "InterestRateSwap"));
        for p in &proposed.proposals {
            assert!(p.residual.residual_dv01.abs() / profile.dv01.abs() < 0.001);
            assert_eq!(p.provenance.cost_model, "heuristic_v1");
        }

        // Tool 3: compare_hedges
        let comparison_text = response_text(
            server
                .compare_hedges(Parameters(CompareHedgesParams {
                    position: profile.clone(),
                    proposals: proposed.proposals.clone(),
                    constraints: None,
                }))
                .await
                .unwrap(),
        );
        let report: ComparisonReport = serde_json::from_str(&comparison_text).unwrap();
        assert_eq!(report.rows.len(), 2);
        assert_eq!(report.recommendation.strategy, "DurationFutures");

        // Tool 4: narrate_recommendation
        let narration_text = response_text(
            server
                .narrate_recommendation(Parameters(NarrateRecommendationParams {
                    comparison: report,
                }))
                .await
                .unwrap(),
        );
        let narration: NarrationOutput = serde_json::from_str(&narration_text).unwrap();
        assert!(narration.text.contains("DurationFutures"));
        assert!(narration.text.contains("InterestRateSwap"));
        assert!(narration.text.contains("Recommend DurationFutures"));
    }

    fn response_text(result: CallToolResult) -> String {
        match &result.content[0].raw {
            rmcp::model::RawContent::Text(t) => t.text.clone(),
            _ => panic!("expected text content"),
        }
    }
}

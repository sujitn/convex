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

use convex_analytics::functions::{
    clean_price_from_yield, convexity, dv01, macaulay_duration, modified_duration,
    yield_to_maturity,
};
use convex_analytics::spreads::ZSpreadCalculator;
use convex_bonds::instruments::{CallableBond, FixedRateBond, FloatingRateNote, ZeroCouponBond};
use convex_bonds::traits::Bond;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Compounding, Currency, Date, Frequency};
use convex_curves::calibration::{Deposit, GlobalFitter, InstrumentSet, Ois, Swap};
use convex_curves::{DiscreteCurve, InterpolationMethod, RateCurve, ValueType};

use crate::demo::DemoData;
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
}

/// MCP Server for Convex analytics
#[derive(Clone)]
pub struct ConvexMcpServer {
    /// Stored bonds by ID
    pub bonds: Arc<RwLock<HashMap<String, StoredBond>>>,
    /// Stored curves by ID
    pub curves: Arc<RwLock<HashMap<String, StoredCurve>>>,
    /// Demo mode enabled
    demo_mode: bool,
    /// Demo data (loaded lazily)
    demo_data: Option<Arc<DemoData>>,
    /// Tool router for MCP tools
    tool_router: ToolRouter<Self>,
}

impl ConvexMcpServer {
    /// Create a new MCP server
    pub fn new() -> Self {
        Self {
            bonds: Arc::new(RwLock::new(HashMap::new())),
            curves: Arc::new(RwLock::new(HashMap::new())),
            demo_mode: false,
            demo_data: None,
            tool_router: Self::tool_router(),
        }
    }

    /// Create a new MCP server with demo mode enabled
    pub fn with_demo_mode() -> Self {
        let demo_data = DemoData::december_2025();
        let mut server = Self {
            bonds: Arc::new(RwLock::new(HashMap::new())),
            curves: Arc::new(RwLock::new(HashMap::new())),
            demo_mode: true,
            demo_data: Some(Arc::new(demo_data.clone())),
            tool_router: Self::tool_router(),
        };
        // Load demo data into storage
        server.load_demo_data(&demo_data);
        server
    }

    /// Load demo data into storage
    fn load_demo_data(&mut self, demo: &DemoData) {
        // Load demo bonds
        for (id, bond) in &demo.bonds {
            self.store_bond(id.clone(), bond.clone());
        }
        // Load demo curves
        for (id, curve) in &demo.curves {
            self.store_curve(id.clone(), curve.clone());
        }
    }

    /// Store a bond
    pub fn store_bond(&self, id: String, bond: StoredBond) {
        let mut bonds = self.bonds.write().unwrap();
        bonds.insert(id, bond);
    }

    /// Get a bond by ID
    pub fn get_bond(&self, id: &str) -> Option<StoredBond> {
        let bonds = self.bonds.read().unwrap();
        bonds.get(id).cloned()
    }

    /// List all bond IDs
    pub fn list_bonds(&self) -> Vec<String> {
        let bonds = self.bonds.read().unwrap();
        bonds.keys().cloned().collect()
    }

    /// Store a curve
    pub fn store_curve(&self, id: String, curve: StoredCurve) {
        let mut curves = self.curves.write().unwrap();
        curves.insert(id, curve);
    }

    /// Get a curve by ID
    pub fn get_curve(&self, id: &str) -> Option<StoredCurve> {
        let curves = self.curves.read().unwrap();
        curves.get(id).cloned()
    }

    /// List all curve IDs
    pub fn list_curves(&self) -> Vec<String> {
        let curves = self.curves.read().unwrap();
        curves.keys().cloned().collect()
    }

    /// Check if demo mode is enabled
    pub fn is_demo_mode(&self) -> bool {
        self.demo_mode
    }

    /// Get demo data reference
    pub fn demo_data(&self) -> Option<Arc<DemoData>> {
        self.demo_data.clone()
    }

    /// Create a success result with text content
    pub fn text_result(text: impl Into<String>) -> CallToolResult {
        CallToolResult::success(vec![Content::text(text.into())])
    }

    /// Create a success result with JSON content
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
    /// Convert to a Date type
    pub fn to_date(&self) -> Result<Date, String> {
        Date::from_ymd(self.year, self.month, self.day).map_err(|e| format!("Invalid date: {}", e))
    }
}

/// Create bond parameters
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CreateBondParams {
    /// Unique identifier for the bond
    pub id: String,
    /// Annual coupon rate as percentage (e.g., 5.0 for 5%)
    pub coupon_rate: f64,
    /// Maturity date
    pub maturity: DateInput,
    /// Issue date
    pub issue_date: DateInput,
}

/// Calculate yield parameters
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CalculateYieldParams {
    /// Bond identifier
    pub bond_id: String,
    /// Settlement date
    pub settlement: DateInput,
    /// Clean price as percentage of par
    pub clean_price: f64,
}

/// Create curve parameters
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CreateCurveParams {
    /// Unique identifier for the curve
    pub id: String,
    /// Reference date
    pub reference_date: DateInput,
    /// Tenor points in years
    pub tenors: Vec<f64>,
    /// Zero rates as percentages
    pub rates: Vec<f64>,
}

/// Get rate parameters
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GetRateParams {
    /// Curve identifier
    pub curve_id: String,
    /// Tenor in years
    pub tenor: f64,
}

/// Calculate Z-spread parameters
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ZSpreadParams {
    /// Bond identifier
    pub bond_id: String,
    /// Curve identifier
    pub curve_id: String,
    /// Settlement date
    pub settlement: DateInput,
    /// Clean price
    pub clean_price: f64,
}

// ============================================================================
// Tool Implementations
// ============================================================================

#[tool_router]
impl ConvexMcpServer {
    /// Create a fixed rate bond
    #[tool(
        description = "Create a new fixed rate bond. Coupon rate is in percentage (e.g., 5.0 for 5%)."
    )]
    pub async fn create_bond(
        &self,
        Parameters(params): Parameters<CreateBondParams>,
    ) -> Result<CallToolResult, McpError> {
        let maturity = params
            .maturity
            .to_date()
            .map_err(|e| McpError::invalid_params(e, None))?;
        let issue_date = params
            .issue_date
            .to_date()
            .map_err(|e| McpError::invalid_params(e, None))?;

        let coupon_decimal = Decimal::from_f64_retain(params.coupon_rate / 100.0)
            .ok_or_else(|| McpError::invalid_params("Invalid coupon rate", None))?;

        let bond = FixedRateBond::builder()
            .cusip_unchecked(&params.id)
            .coupon_rate(coupon_decimal)
            .maturity(maturity)
            .issue_date(issue_date)
            .frequency(Frequency::SemiAnnual)
            .day_count(DayCountConvention::Thirty360US)
            .currency(Currency::USD)
            .face_value(dec!(100))
            .build()
            .map_err(|e| McpError::internal_error(format!("Failed: {}", e), None))?;

        self.store_bond(params.id.clone(), StoredBond::Fixed(bond));

        let response = serde_json::json!({
            "status": "success",
            "bond_id": params.id,
        });

        Self::json_result(&response)
    }

    /// Calculate yield to maturity from price
    #[tool(
        description = "Calculate yield to maturity (YTM) from clean price. Returns yield as percentage."
    )]
    pub async fn calculate_yield(
        &self,
        Parameters(params): Parameters<CalculateYieldParams>,
    ) -> Result<CallToolResult, McpError> {
        let bond = self.get_bond(&params.bond_id).ok_or_else(|| {
            McpError::invalid_params(format!("Bond '{}' not found", params.bond_id), None)
        })?;

        let settlement = params
            .settlement
            .to_date()
            .map_err(|e| McpError::invalid_params(e, None))?;

        let price_decimal = Decimal::from_f64_retain(params.clean_price)
            .ok_or_else(|| McpError::invalid_params("Invalid price", None))?;

        let ytm = match bond {
            StoredBond::Fixed(b) => {
                yield_to_maturity(&b, settlement, price_decimal, Frequency::SemiAnnual)
                    .map_err(|e| McpError::internal_error(format!("YTM failed: {}", e), None))?
            }
            StoredBond::Callable(c) => yield_to_maturity(
                c.base_bond(),
                settlement,
                price_decimal,
                Frequency::SemiAnnual,
            )
            .map_err(|e| McpError::internal_error(format!("YTM failed: {}", e), None))?,
            _ => {
                return Err(McpError::invalid_params(
                    "YTM only for fixed/callable bonds",
                    None,
                ))
            }
        };

        let response = serde_json::json!({
            "bond_id": params.bond_id,
            "settlement": settlement.to_string(),
            "clean_price": params.clean_price,
            "ytm_pct": ytm.yield_value * 100.0,
        });

        Self::json_result(&response)
    }

    /// Create a yield curve from zero rates
    #[tool(
        description = "Create a yield curve from zero rate points. Rates should be in percentage (e.g., 4.5 for 4.5%)."
    )]
    pub async fn create_curve(
        &self,
        Parameters(params): Parameters<CreateCurveParams>,
    ) -> Result<CallToolResult, McpError> {
        if params.tenors.len() != params.rates.len() {
            return Err(McpError::invalid_params(
                "Tenors and rates must match",
                None,
            ));
        }

        let ref_date = params
            .reference_date
            .to_date()
            .map_err(|e| McpError::invalid_params(e, None))?;

        let rates_decimal: Vec<f64> = params.rates.iter().map(|r| r / 100.0).collect();

        let value_type = ValueType::ZeroRate {
            compounding: Compounding::Continuous,
            day_count: DayCountConvention::Act365Fixed,
        };

        let discrete = DiscreteCurve::new(
            ref_date,
            params.tenors.clone(),
            rates_decimal,
            value_type,
            InterpolationMethod::MonotoneConvex,
        )
        .map_err(|e| McpError::internal_error(format!("Curve failed: {}", e), None))?;

        let curve = RateCurve::new(discrete);
        self.store_curve(params.id.clone(), curve);

        let response = serde_json::json!({
            "status": "success",
            "curve_id": params.id,
            "tenor_count": params.tenors.len(),
        });

        Self::json_result(&response)
    }

    /// Get zero rate at a tenor
    #[tool(description = "Get zero rate at a specific tenor. Returns rate as percentage.")]
    pub async fn get_zero_rate(
        &self,
        Parameters(params): Parameters<GetRateParams>,
    ) -> Result<CallToolResult, McpError> {
        let curve = self.get_curve(&params.curve_id).ok_or_else(|| {
            McpError::invalid_params(format!("Curve '{}' not found", params.curve_id), None)
        })?;

        let rate = curve
            .zero_rate_at_tenor(params.tenor, Compounding::Continuous)
            .map_err(|e| McpError::internal_error(format!("Rate query failed: {}", e), None))?;

        let response = serde_json::json!({
            "curve_id": params.curve_id,
            "tenor": params.tenor,
            "zero_rate_pct": rate * 100.0,
        });

        Self::json_result(&response)
    }

    /// Calculate Z-spread
    #[tool(description = "Calculate Z-spread for a bond. Returns spread in basis points.")]
    pub async fn calculate_z_spread(
        &self,
        Parameters(params): Parameters<ZSpreadParams>,
    ) -> Result<CallToolResult, McpError> {
        let bond = self.get_bond(&params.bond_id).ok_or_else(|| {
            McpError::invalid_params(format!("Bond '{}' not found", params.bond_id), None)
        })?;

        let curve = self.get_curve(&params.curve_id).ok_or_else(|| {
            McpError::invalid_params(format!("Curve '{}' not found", params.curve_id), None)
        })?;

        let settlement = params
            .settlement
            .to_date()
            .map_err(|e| McpError::invalid_params(e, None))?;

        let z_spread_bps = match bond {
            StoredBond::Fixed(b) => {
                let accrued = b.accrued_interest(settlement);
                let dirty =
                    Decimal::from_f64_retain(params.clean_price).unwrap_or_default() + accrued;
                let calc = ZSpreadCalculator::new(&curve);
                calc.calculate(&b, dirty, settlement)
                    .map_err(|e| McpError::internal_error(format!("Z-spread failed: {}", e), None))?
                    .as_bps()
                    .to_f64()
                    .unwrap_or(f64::NAN)
            }
            _ => {
                return Err(McpError::invalid_params(
                    "Z-spread only for fixed bonds",
                    None,
                ))
            }
        };

        let response = serde_json::json!({
            "bond_id": params.bond_id,
            "curve_id": params.curve_id,
            "z_spread_bps": z_spread_bps,
        });

        Self::json_result(&response)
    }

    /// List all stored bonds
    #[tool(description = "List all bonds currently stored in the system.")]
    pub async fn list_all_bonds(&self) -> Result<CallToolResult, McpError> {
        let bonds = self.bonds.read().unwrap();
        let result: Vec<_> = bonds
            .iter()
            .map(|(id, bond)| {
                serde_json::json!({
                    "id": id,
                    "type": bond.type_name(),
                })
            })
            .collect();

        let response = serde_json::json!({
            "count": result.len(),
            "bonds": result,
        });

        Self::json_result(&response)
    }

    /// List all stored curves
    #[tool(description = "List all curves currently stored in the system.")]
    pub async fn list_all_curves(&self) -> Result<CallToolResult, McpError> {
        let curves = self.curves.read().unwrap();
        let result: Vec<_> = curves
            .iter()
            .map(|(id, curve)| {
                serde_json::json!({
                    "id": id,
                    "reference_date": curve.reference_date().to_string(),
                    "tenor_count": curve.inner().tenors().len(),
                })
            })
            .collect();

        let response = serde_json::json!({
            "count": result.len(),
            "curves": result,
        });

        Self::json_result(&response)
    }

    /// Get demo market snapshot
    #[tool(description = "Get the demo market snapshot with key rates. Only in demo mode.")]
    pub async fn get_market_snapshot(&self) -> Result<CallToolResult, McpError> {
        if !self.demo_mode {
            return Err(McpError::invalid_params("Demo mode not enabled", None));
        }

        let demo = self
            .demo_data()
            .ok_or_else(|| McpError::internal_error("No demo data", None))?;

        let response = serde_json::json!({
            "reference_date": demo.reference_date.to_string(),
            "description": demo.market_description,
            "bonds_available": demo.bonds.len(),
            "curves_available": demo.curves.len(),
        });

        Self::json_result(&response)
    }

    /// List demo bonds
    #[tool(description = "List all demo bonds with details. Only in demo mode.")]
    pub async fn list_demo_bonds(&self) -> Result<CallToolResult, McpError> {
        if !self.demo_mode {
            return Err(McpError::invalid_params("Demo mode not enabled", None));
        }

        let demo = self
            .demo_data()
            .ok_or_else(|| McpError::internal_error("No demo data", None))?;
        let bonds = demo.list_demo_bonds();

        Self::json_result(&bonds)
    }

    /// List demo curves
    #[tool(description = "List all demo curves with details. Only in demo mode.")]
    pub async fn list_demo_curves(&self) -> Result<CallToolResult, McpError> {
        if !self.demo_mode {
            return Err(McpError::invalid_params("Demo mode not enabled", None));
        }

        let demo = self
            .demo_data()
            .ok_or_else(|| McpError::internal_error("No demo data", None))?;
        let curves = demo.list_demo_curves();

        Self::json_result(&curves)
    }
}

#[tool_handler]
impl ServerHandler for ConvexMcpServer {
    fn get_info(&self) -> ServerInfo {
        let instructions = if self.demo_mode {
            "Convex MCP Server (DEMO MODE) - Fixed income analytics with December 2025 sample data. \
             Use list_demo_bonds and list_demo_curves to see available data."
        } else {
            "Convex MCP Server - High-performance fixed income analytics. \
             Create bonds and curves, then calculate yields, durations, and spreads."
        };

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
            instructions: Some(instructions.to_string()),
        }
    }
}

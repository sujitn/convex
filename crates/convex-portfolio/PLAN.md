# Convex Portfolio Module - Implementation Plan

## 1. Codebase Analysis

### 1.1 Existing Architecture

The Convex library follows a clean layered architecture:

```
convex-core        - Foundational types (Date, Price, Yield, Currency, Spread)
convex-math        - Mathematical utilities (solvers, interpolation)
convex-curves      - Term structure framework (TermStructure trait, RateCurve)
convex-bonds       - Bond instruments ONLY (no analytics - pure domain objects)
convex-analytics   - All calculation logic (yields, pricing, risk, spreads)
convex-ffi         - C FFI layer for Excel
convex-wasm        - WebAssembly bindings
```

**Key Design Principle**: `convex-bonds` provides instruments only, `convex-analytics` provides calculations. This separation ensures bond types are lightweight. The portfolio module should follow this pattern.

### 1.2 Module Organization Patterns

Each crate follows a consistent structure:
- `lib.rs` - Public API and prelude
- `error.rs` - Custom error types with `thiserror`
- `types/` - Domain types and enums
- `traits/` - Core trait hierarchy
- Domain-specific modules (e.g., `pricing/`, `risk/`, `spreads/`)

### 1.3 Existing Portfolio-Related Code

Located in `convex-analytics/src/risk/hedging/portfolio.rs`:

```rust
pub struct PortfolioRisk {
    pub market_value: Decimal,
    pub weighted_duration: Duration,
    pub total_dv01: DV01,
    pub position_count: usize,
}

pub struct Position {
    pub id: String,
    pub market_value: f64,
    pub duration: Duration,
    pub dv01: DV01,
}

pub fn aggregate_portfolio_risk(positions: &[Position]) -> PortfolioRisk
```

This provides a foundation but is limited to basic risk aggregation.

### 1.4 Key Types Available for Reuse

**From `convex-core`:**
- `Date` - Calendar dates with business day support
- `Currency` - ISO 4217 currency codes (USD, EUR, GBP, JPY, etc.)
- `Price` - Price with currency
- `Yield` - Yield with compounding convention
- `Spread` / `SpreadType` - Spread values in basis points
- `Frequency` / `Compounding` - Payment frequency and compounding
- `CashFlow` / `CashFlowSchedule` - Generic cash flow types

**From `convex-analytics`:**
- `Duration` - Modified/Macaulay duration wrapper (newtype over Decimal)
- `DV01` - Dollar value of a basis point (newtype over Decimal)
- `KeyRateDuration` / `KeyRateDurations` - Key rate profile
- `Convexity` - Convexity value wrapper
- `BondRiskMetrics` - Aggregated risk metrics for a single bond
- `VaRResult` / `VaRMethod` - Value at Risk types
- All spread calculators (ZSpread, GSpread, ISpread, OAS, ASW)

**From `convex-bonds`:**
- `Bond` trait - Core interface for all bonds
- `FixedCouponBond`, `FloatingCouponBond`, `EmbeddedOptionBond` - Extension traits
- `BondCashFlow` - Individual bond cash flows
- `BondIdentifiers` - ISIN, CUSIP, SEDOL, FIGI
- `BondType` - Classification enum (50+ variants)

### 1.5 Error Handling Pattern

Each crate defines:
```rust
pub type XxxResult<T> = Result<T, XxxError>;

#[derive(Error, Debug, Clone)]
pub enum XxxError {
    #[error("...")]
    VariantName { ... },

    #[error("Upstream error: {0}")]
    UpstreamError(#[from] UpstreamError),
}
```

### 1.6 Testing Patterns

- Inline `#[cfg(test)] mod tests` with unit tests
- Helper functions like `fn date(y, m, d) -> Date`
- Use of `approx::assert_relative_eq!` for floating point
- Property-based tests with `proptest` where appropriate

---

## 2. Dependency Mapping

### 2.1 Crate Dependencies

```
convex-portfolio
├── convex-core (Date, Currency, Price, Yield, Spread, Frequency)
├── convex-bonds (Bond trait, BondCashFlow, BondIdentifiers, BondType)
├── convex-analytics (Duration, DV01, Convexity, KeyRateDurations, spread calculators)
├── convex-curves (RateCurveDyn for curve-based calculations)
├── rust_decimal (Decimal type)
├── serde (serialization)
├── thiserror (error handling)
└── chrono (optional, for time series)
```

### 2.2 Types to Reuse

| Category | Type | From Crate |
|----------|------|------------|
| Dates | `Date` | convex-core |
| Money | `Currency`, `Price`, `Decimal` | convex-core |
| Rates | `Yield`, `Spread`, `SpreadType` | convex-core |
| Risk | `Duration`, `DV01`, `Convexity`, `KeyRateDurations` | convex-analytics |
| VaR | `VaRResult`, `VaRMethod` | convex-analytics |
| Bonds | `Bond`, `BondIdentifiers`, `BondCashFlow` | convex-bonds |
| Curves | `RateCurveDyn` | convex-curves |

### 2.3 Types to Create

| Type | Purpose |
|------|---------|
| `Holding` | Single position with bond reference and quantity |
| `HoldingAnalytics` | Pre-calculated analytics for a holding |
| `CashPosition` | Cash with currency and optional FX rate |
| `Portfolio` | Collection of holdings and cash |
| `PortfolioAnalytics` | Aggregated portfolio-level analytics |
| `WeightingMethod` | Enum for MV, Par, Equal weighting |
| `Bucket` / `BucketDefinition` | Bucketing configuration |
| `SectorClassification` | Sector/rating/maturity classification |
| `Contribution` | Risk/return contribution by holding |
| `BenchmarkComparison` | Active weights and tracking |
| `ETFMetrics` | ETF-specific metrics (NAV, premium/discount) |

---

## 3. Module Design

### 3.1 Proposed File Structure

```
crates/convex-portfolio/
├── Cargo.toml
├── src/
│   ├── lib.rs                    # Public API, prelude, re-exports
│   ├── error.rs                  # PortfolioError, PortfolioResult
│   │
│   ├── types/                    # Domain types
│   │   ├── mod.rs
│   │   ├── holding.rs            # Holding, HoldingAnalytics
│   │   ├── cash.rs               # CashPosition, FxRate
│   │   ├── weighting.rs          # WeightingMethod enum
│   │   ├── classification.rs     # Sector, Rating, MaturityBucket
│   │   └── contribution.rs       # RiskContribution, ReturnContribution
│   │
│   ├── portfolio/                # Core portfolio structure
│   │   ├── mod.rs
│   │   ├── builder.rs            # PortfolioBuilder
│   │   └── portfolio.rs          # Portfolio struct
│   │
│   ├── analytics/                # Portfolio-level analytics
│   │   ├── mod.rs
│   │   ├── nav.rs                # NAV, iNAV calculation
│   │   ├── yields.rs             # Weighted yields (YTM, YTW, YTC)
│   │   ├── risk.rs               # Duration, DV01, convexity aggregation
│   │   ├── key_rates.rs          # Key rate duration aggregation
│   │   ├── spreads.rs            # Weighted spread metrics
│   │   ├── credit.rs             # Credit quality distribution
│   │   └── liquidity.rs          # Liquidity metrics
│   │
│   ├── bucketing/                # Classification and bucketing
│   │   ├── mod.rs
│   │   ├── sector.rs             # Sector classification
│   │   ├── rating.rs             # Credit rating buckets
│   │   ├── maturity.rs           # Maturity buckets
│   │   └── custom.rs             # User-defined bucketing
│   │
│   ├── contribution/             # Contribution analysis
│   │   ├── mod.rs
│   │   ├── risk.rs               # Duration/DV01/spread contribution
│   │   └── attribution.rs        # Return attribution helpers
│   │
│   ├── benchmark/                # Benchmark comparison
│   │   ├── mod.rs
│   │   ├── tracking.rs           # Tracking error, active weights
│   │   └── comparison.rs         # Duration/spread differences
│   │
│   ├── stress/                   # Stress testing support
│   │   ├── mod.rs
│   │   ├── scenarios.rs          # Scenario definitions
│   │   └── impact.rs             # Scenario impact calculations
│   │
│   └── etf/                      # ETF-specific analytics
│       ├── mod.rs
│       ├── nav.rs                # NAV, iNAV, premium/discount
│       ├── basket.rs             # Creation/redemption basket
│       └── sec.rs                # SEC yield, compliance metrics
```

### 3.2 Public API Surface

```rust
// lib.rs - Key exports
pub use error::{PortfolioError, PortfolioResult};
pub use types::{
    Holding, HoldingAnalytics, CashPosition, FxRate,
    WeightingMethod, Sector, CreditRating, MaturityBucket,
    RiskContribution, ReturnAttribution,
};
pub use portfolio::{Portfolio, PortfolioBuilder};
pub use analytics::{
    PortfolioAnalytics, PortfolioSummary,
    nav, weighted_ytm, weighted_duration, weighted_spread,
    key_rate_profile, credit_distribution, sector_distribution,
};
pub use bucketing::{bucket_by_sector, bucket_by_rating, bucket_by_maturity};
pub use contribution::{duration_contribution, spread_contribution};
pub use benchmark::{active_weights, tracking_error, benchmark_comparison};
pub use stress::{parallel_shift_impact, key_rate_shift_impact, spread_shock_impact};
pub use etf::{calculate_nav, calculate_inav, premium_discount, sec_30_day_yield};
```

### 3.3 Core Types Design

#### 3.3.1 Flexible Classification System

The classification system follows a consistent pattern:
- **Composite enum** → Normalized value for analytics
- **Provider map** → Preserves source data from any provider (Bloomberg, GICS, internal)
- **User controls mapping** → They set composite based on their logic

```rust
// =============================================================================
// SECTOR CLASSIFICATION
// =============================================================================

/// Normalized sector for analytics
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Sector {
    Government,
    Agency,
    Corporate,
    Financial,
    Utility,
    Municipal,
    Supranational,
    AssetBacked,
    MortgageBacked,
    CoveredBond,
    Other,
}

/// Source sector classifications from any provider
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SectorInfo {
    /// Normalized sector for analytics
    pub composite: Option<Sector>,

    /// Hierarchical classification by provider
    /// Key: "BICS", "GICS", "ICB", "Internal", etc.
    /// Value: levels as vector (most general → most specific)
    pub by_provider: HashMap<String, Vec<String>>,
}

impl SectorInfo {
    pub fn new() -> Self { Self::default() }

    /// Add classification from a provider (BICS, GICS, Internal, etc.)
    pub fn with_classification(mut self, provider: &str, levels: &[&str]) -> Self {
        self.by_provider.insert(
            provider.to_string(),
            levels.iter().map(|s| s.to_string()).collect()
        );
        self
    }

    pub fn with_composite(mut self, sector: Sector) -> Self {
        self.composite = Some(sector);
        self
    }

    /// Get specific level from a provider (0-indexed)
    pub fn level(&self, provider: &str, depth: usize) -> Option<&str> {
        self.by_provider
            .get(provider)
            .and_then(|levels| levels.get(depth))
            .map(|s| s.as_str())
    }
}

// Usage examples:
// Simple:     SectorInfo::new().with_composite(Sector::Financial)
// Bloomberg:  SectorInfo::new()
//               .with_classification("BICS", &["Financials", "Banking", "Commercial Banking"])
//               .with_composite(Sector::Financial)
// GICS:       SectorInfo::new()
//               .with_classification("GICS", &["40", "4010", "401010", "40101010"])
//               .with_composite(Sector::Financial)

// =============================================================================
// CREDIT RATING
// =============================================================================

/// Normalized rating for analytics (agency-agnostic)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CreditRating {
    AAA,
    AAPlus,   // AA+
    AA,
    AAMinus,  // AA-
    APlus,    // A+
    A,
    AMinus,   // A-
    BBBPlus,  // BBB+
    BBB,
    BBBMinus, // BBB- (lowest investment grade)
    BBPlus,   // BB+  (highest high yield)
    BB,
    BBMinus,
    BPlus,
    B,
    BMinus,
    CCCPlus,
    CCC,
    CCCMinus,
    CC,
    C,
    D,        // Default
    NotRated,
}

impl CreditRating {
    /// Numeric score (1 = AAA, 22 = D)
    pub fn score(&self) -> u8 { /* 1-22 mapping */ }

    /// Is investment grade (BBB- or better)?
    pub fn is_investment_grade(&self) -> bool {
        *self <= CreditRating::BBBMinus && *self != CreditRating::NotRated
    }

    /// Rating bucket for reporting
    pub fn bucket(&self) -> RatingBucket { /* AAA, AA, A, BBB, BB, B, CCC, D, NR */ }
}

/// Source ratings from any provider (flexible - no hardcoded agency list)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RatingInfo {
    /// Normalized rating for analytics (user provides or derives)
    pub composite: Option<CreditRating>,

    /// Original ratings by provider (any agency)
    /// Key: "SP", "Moodys", "Fitch", "DBRS", "JCR", "Kroll", "Internal", etc.
    /// Value: Original rating string as provided
    pub by_provider: HashMap<String, String>,
}

impl RatingInfo {
    pub fn new() -> Self { Self::default() }

    pub fn with_rating(mut self, provider: &str, rating: &str) -> Self {
        self.by_provider.insert(provider.to_string(), rating.to_string());
        self
    }

    pub fn with_composite(mut self, rating: CreditRating) -> Self {
        self.composite = Some(rating);
        self
    }
}

// Usage examples:
// Single agency:  RatingInfo::new().with_rating("Moodys", "Baa2").with_composite(CreditRating::BBB)
// Multi-agency:   RatingInfo::new()
//                   .with_rating("SP", "BBB+").with_rating("Moodys", "Baa1")
//                   .with_composite(CreditRating::BBBPlus)
// Regional:       RatingInfo::new().with_rating("JCR", "A").with_composite(CreditRating::A)

// =============================================================================
// SENIORITY (handles AT1, CoCo, bank capital structure)
// =============================================================================

/// Normalized seniority for analytics
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Seniority {
    SeniorSecured,
    SeniorUnsecured,
    SeniorNonPreferred,  // EU MREL
    Subordinated,
    JuniorSubordinated,
    Hybrid,              // AT1, CoCo, Preferred
    Equity,
}

impl Seniority {
    /// Typical recovery rate assumption
    pub fn typical_recovery(&self) -> f64 {
        match self {
            Self::SeniorSecured => 0.60,
            Self::SeniorUnsecured => 0.40,
            Self::SeniorNonPreferred => 0.35,
            Self::Subordinated => 0.20,
            Self::JuniorSubordinated => 0.10,
            Self::Hybrid => 0.05,
            Self::Equity => 0.0,
        }
    }
}

/// Detailed seniority with capital structure info
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SeniorityInfo {
    /// Normalized for analytics
    pub composite: Option<Seniority>,

    /// Regulatory capital tier: "CET1", "AT1", "Tier2", "MREL", etc.
    pub capital_tier: Option<String>,

    /// Specific instrument type: "CoCo", "Preferred", "Legacy T1", etc.
    pub instrument_type: Option<String>,

    /// For CoCos/AT1: trigger level (e.g., 0.05125 for 5.125% CET1)
    pub trigger_level: Option<f64>,

    /// Trigger type: "MechanicalWritedown", "PON", "Conversion", "EquityConversion"
    pub trigger_type: Option<String>,

    /// Bail-in rank (lower = first loss)
    pub bailin_rank: Option<u8>,

    /// Structural position: "HoldCo", "OpCo"
    pub structural_position: Option<String>,

    /// Provider-specific codes
    pub by_provider: HashMap<String, String>,
}

impl SeniorityInfo {
    pub fn new() -> Self { Self::default() }

    pub fn with_composite(mut self, seniority: Seniority) -> Self {
        self.composite = Some(seniority);
        self
    }

    pub fn with_capital_tier(mut self, tier: &str) -> Self {
        self.capital_tier = Some(tier.to_string());
        self
    }

    pub fn with_coco_trigger(mut self, level: f64, trigger_type: &str) -> Self {
        self.trigger_level = Some(level);
        self.trigger_type = Some(trigger_type.to_string());
        self
    }

    pub fn with_structural_position(mut self, position: &str) -> Self {
        self.structural_position = Some(position.to_string());
        self
    }

    /// Is this bail-inable under BRRD/TLAC?
    pub fn is_bailin_eligible(&self) -> bool {
        matches!(
            self.composite,
            Some(Seniority::SeniorNonPreferred)
            | Some(Seniority::Subordinated)
            | Some(Seniority::JuniorSubordinated)
            | Some(Seniority::Hybrid)
        )
    }
}

// Usage examples:
// Corporate:  SeniorityInfo::new().with_composite(Seniority::SeniorUnsecured)
// Bank T2:    SeniorityInfo::new()
//               .with_composite(Seniority::Subordinated)
//               .with_capital_tier("Tier2")
// AT1 CoCo:   SeniorityInfo::new()
//               .with_composite(Seniority::Hybrid)
//               .with_capital_tier("AT1")
//               .with_coco_trigger(0.05125, "MechanicalWritedown")
// EU HoldCo:  SeniorityInfo::new()
//               .with_composite(Seniority::SeniorUnsecured)
//               .with_structural_position("HoldCo")
//               .with_capital_tier("MREL")

// =============================================================================
// UNIFIED CLASSIFICATION
// =============================================================================

/// Complete classification for a holding
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Classification {
    /// Sector classification
    pub sector: SectorInfo,

    /// Credit rating
    pub rating: RatingInfo,

    /// Seniority / capital structure
    pub seniority: SeniorityInfo,

    /// Issuer information
    pub issuer: Option<String>,
    pub issuer_id: Option<String>,  // LEI, Bloomberg ID, etc.

    /// Geography
    pub country: Option<String>,    // ISO 3166-1 alpha-2
    pub region: Option<String>,     // Americas, EMEA, APAC

    /// Fully custom fields (user-defined)
    pub custom: HashMap<String, String>,
}
```

#### 3.3.2 Holding and Portfolio Types

```rust
/// A single holding in a portfolio
#[derive(Debug, Clone)]
pub struct Holding {
    /// Unique identifier for this position
    pub id: String,
    /// Bond identifiers (ISIN, CUSIP, etc.)
    pub identifiers: BondIdentifiers,
    /// Par/face amount held
    pub par_amount: Decimal,
    /// Market price (clean, as % of par)
    pub market_price: Decimal,
    /// Accrued interest per unit
    pub accrued_interest: Decimal,
    /// FX rate to portfolio base currency (1.0 if same currency)
    pub fx_rate: Decimal,
    /// Pre-calculated analytics (caller provides)
    pub analytics: HoldingAnalytics,
    /// Classification metadata
    pub classification: Classification,
    /// Currency of the bond
    pub currency: Currency,
}

impl Holding {
    /// Market value in bond currency
    pub fn market_value_local(&self) -> Decimal {
        self.par_amount * self.market_price / Decimal::ONE_HUNDRED
    }

    /// Market value in portfolio base currency
    pub fn market_value(&self) -> Decimal {
        self.market_value_local() * self.fx_rate
    }

    /// Total value including accrued
    pub fn total_value(&self) -> Decimal {
        (self.market_value_local() + self.par_amount * self.accrued_interest / Decimal::ONE_HUNDRED)
            * self.fx_rate
    }
}

/// Pre-calculated analytics for a holding (caller provides)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HoldingAnalytics {
    // Yields
    pub ytm: Option<f64>,
    pub ytw: Option<f64>,
    pub ytc: Option<f64>,
    pub current_yield: Option<f64>,

    // Duration
    pub modified_duration: Option<f64>,
    pub effective_duration: Option<f64>,
    pub macaulay_duration: Option<f64>,
    pub spread_duration: Option<f64>,

    // Convexity
    pub convexity: Option<f64>,
    pub effective_convexity: Option<f64>,

    // DV01 (per unit of par)
    pub dv01: Option<f64>,

    // Key rate durations
    pub key_rate_durations: Option<KeyRateDurations>,

    // Spreads (in basis points)
    pub z_spread: Option<f64>,
    pub oas: Option<f64>,
    pub g_spread: Option<f64>,
    pub i_spread: Option<f64>,
    pub asw: Option<f64>,

    // Credit spread sensitivity
    pub cs01: Option<f64>,

    // Liquidity (optional)
    pub bid_ask_spread: Option<f64>,  // bps
    pub liquidity_score: Option<f64>, // 0-100
}

/// Cash position with optional FX rate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashPosition {
    pub amount: Decimal,
    pub currency: Currency,
    /// FX rate to base currency (1.0 if same as base)
    pub fx_rate: Decimal,
}

impl CashPosition {
    pub fn value_in_base(&self) -> Decimal {
        self.amount * self.fx_rate
    }
}

/// Portfolio weighting method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum WeightingMethod {
    #[default]
    MarketValue,
    ParValue,
    EqualWeight,
}

/// The portfolio itself
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Portfolio {
    pub id: String,
    pub name: String,
    pub base_currency: Currency,
    pub as_of_date: Date,
    pub holdings: Vec<Holding>,
    pub cash: Vec<CashPosition>,
    pub shares_outstanding: Option<Decimal>,
    pub liabilities: Option<Decimal>,
}
```

### 3.4 Parallel Processing Configuration

Portfolio analytics can benefit from parallel processing for large portfolios. We use `rayon` with config-driven behavior.

#### 3.4.1 Configuration Struct

```rust
/// Configuration for portfolio analytics computation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsConfig {
    /// Enable parallel processing (requires 'parallel' feature)
    pub parallel: bool,

    /// Minimum holdings count to trigger parallel processing
    /// Below this threshold, sequential is faster due to thread overhead
    pub parallel_threshold: usize,

    /// Weighting method for aggregations
    pub weighting: WeightingMethod,

    /// Include holdings with missing analytics in aggregations
    pub include_incomplete: bool,

    /// Key rate tenors to use (defaults to STANDARD_KEY_RATE_TENORS)
    pub key_rate_tenors: Option<Vec<f64>>,
}

impl Default for AnalyticsConfig {
    fn default() -> Self {
        Self {
            parallel: true,
            parallel_threshold: 100,  // Use parallel if >100 holdings
            weighting: WeightingMethod::MarketValue,
            include_incomplete: true,
            key_rate_tenors: None,
        }
    }
}

impl AnalyticsConfig {
    pub fn new() -> Self { Self::default() }

    pub fn sequential() -> Self {
        Self { parallel: false, ..Self::default() }
    }

    pub fn with_parallel(mut self, enabled: bool) -> Self {
        self.parallel = enabled;
        self
    }

    pub fn with_threshold(mut self, threshold: usize) -> Self {
        self.parallel_threshold = threshold;
        self
    }

    pub fn with_weighting(mut self, method: WeightingMethod) -> Self {
        self.weighting = method;
        self
    }

    /// Should we use parallel for this collection size?
    pub fn should_parallelize(&self, count: usize) -> bool {
        cfg!(feature = "parallel") && self.parallel && count >= self.parallel_threshold
    }
}
```

#### 3.4.2 Parallel Iterator Pattern

```rust
use rayon::prelude::*;

/// Internal helper for conditional parallel iteration
pub(crate) fn maybe_parallel_map<T, U, F>(
    items: &[T],
    config: &AnalyticsConfig,
    f: F,
) -> Vec<U>
where
    T: Sync,
    U: Send,
    F: Fn(&T) -> U + Sync,
{
    #[cfg(feature = "parallel")]
    if config.should_parallelize(items.len()) {
        return items.par_iter().map(f).collect();
    }

    items.iter().map(f).collect()
}

/// Parallel fold/reduce pattern
pub(crate) fn maybe_parallel_fold<T, U, F, R>(
    items: &[T],
    config: &AnalyticsConfig,
    identity: U,
    fold: F,
    reduce: R,
) -> U
where
    T: Sync,
    U: Send + Clone,
    F: Fn(U, &T) -> U + Sync,
    R: Fn(U, U) -> U + Sync,
{
    #[cfg(feature = "parallel")]
    if config.should_parallelize(items.len()) {
        return items
            .par_iter()
            .fold(|| identity.clone(), |acc, item| fold(acc, item))
            .reduce(|| identity.clone(), reduce);
    }

    items.iter().fold(identity, |acc, item| fold(acc, item))
}
```

#### 3.4.3 Usage in Analytics Functions

```rust
/// Calculate weighted average yield with configurable parallelism
pub fn weighted_ytm(
    holdings: &[Holding],
    config: &AnalyticsConfig,
) -> Option<f64> {
    let (sum_weighted, sum_weights) = maybe_parallel_fold(
        holdings,
        config,
        (0.0, 0.0),
        |(sum_w, sum_wt), h| {
            if let Some(ytm) = h.analytics.ytm {
                let weight = h.weight(config.weighting);
                (sum_w + ytm * weight, sum_wt + weight)
            } else if config.include_incomplete {
                (sum_w, sum_wt)  // Skip but continue
            } else {
                (sum_w, sum_wt)
            }
        },
        |(a, b), (c, d)| (a + c, b + d),
    );

    if sum_weights > 0.0 {
        Some(sum_weighted / sum_weights)
    } else {
        None
    }
}

/// Example: Bucket by sector with parallel aggregation
pub fn bucket_by_sector(
    holdings: &[Holding],
    config: &AnalyticsConfig,
) -> HashMap<Sector, BucketMetrics> {
    // Group holdings (parallel if enabled)
    let groups = maybe_parallel_fold(
        holdings,
        config,
        HashMap::new(),
        |mut acc, h| {
            if let Some(sector) = h.classification.sector.composite {
                acc.entry(sector).or_insert_with(Vec::new).push(h);
            }
            acc
        },
        |mut a, b| {
            for (k, v) in b {
                a.entry(k).or_insert_with(Vec::new).extend(v);
            }
            a
        },
    );

    // Aggregate each group
    groups
        .into_iter()
        .map(|(sector, group)| {
            let metrics = aggregate_bucket(&group, config);
            (sector, metrics)
        })
        .collect()
}
```

#### 3.4.4 Feature Flag in Cargo.toml

```toml
[features]
default = []
parallel = ["rayon"]

[dependencies]
rayon = { workspace = true, optional = true }
```

#### 3.4.5 Performance Guidelines

| Holdings | Recommendation | Reason |
|----------|----------------|--------|
| < 50 | Sequential | Thread overhead dominates |
| 50-100 | Either | Marginal benefit |
| 100-500 | Parallel | Good speedup |
| > 500 | Parallel | Significant speedup |
| > 5000 | Parallel + chunking | Memory locality matters |

The default threshold of 100 provides a safe balance. Users can tune via config.

---

## 4. Implementation Order

### Phase 1: Foundation (Core Types)
**Estimated complexity: Low**

1. Create crate skeleton (`Cargo.toml`, `lib.rs`, `error.rs`)
2. Implement `types/weighting.rs` - `WeightingMethod` enum
3. Implement `types/classification.rs` - `Sector`, `CreditRating`, `MaturityBucket`
4. Implement `types/holding.rs` - `Holding`, `HoldingAnalytics`, `HoldingClassification`
5. Implement `types/cash.rs` - `CashPosition`, `FxRate`
6. Implement `portfolio/portfolio.rs` - `Portfolio` struct
7. Implement `portfolio/builder.rs` - `PortfolioBuilder`

**Deliverable**: Can create and represent portfolios.

### Phase 2: Core Analytics (Aggregation)
**Estimated complexity: Medium**

1. Implement `analytics/nav.rs` - NAV, component breakdown
2. Implement `analytics/yields.rs` - Weighted YTM, YTW, YTC, current yield
3. Implement `analytics/risk.rs` - Weighted duration, DV01, convexity
4. Implement `analytics/spreads.rs` - Weighted OAS, Z-spread, etc.
5. Create `PortfolioAnalytics` summary struct

**Deliverable**: Full weighted average analytics.

### Phase 3: Key Rate & Advanced Risk
**Estimated complexity: Medium**

1. Implement `analytics/key_rates.rs` - Aggregate KRD profile
2. Implement `stress/scenarios.rs` - Scenario definitions
3. Implement `stress/impact.rs` - Parallel shift, KR shift, spread shock

**Deliverable**: Key rate profiles and stress testing.

### Phase 4: Classification & Bucketing
**Estimated complexity: Low-Medium**

1. Implement `bucketing/sector.rs` - Sector bucketing
2. Implement `bucketing/rating.rs` - Credit rating distribution
3. Implement `bucketing/maturity.rs` - Maturity bucket distribution
4. Implement `bucketing/custom.rs` - User-defined buckets
5. Implement `analytics/credit.rs` - Credit quality metrics

**Deliverable**: Full classification and distribution analytics.

### Phase 5: Contribution Analysis
**Estimated complexity: Medium**

1. Implement `contribution/risk.rs` - Duration/DV01/spread contribution
2. Implement `contribution/attribution.rs` - Return attribution helpers
3. Add contribution by sector/rating aggregations

**Deliverable**: Risk contribution and attribution.

### Phase 6: Benchmark Comparison
**Estimated complexity: Medium**

1. Implement `benchmark/tracking.rs` - Active weights, tracking error
2. Implement `benchmark/comparison.rs` - Duration/spread differences

**Deliverable**: Benchmark-relative analytics.

### Phase 7: ETF-Specific Analytics
**Estimated complexity: Medium**

1. Implement `etf/nav.rs` - NAV, iNAV, premium/discount
2. Implement `etf/basket.rs` - Creation/redemption basket analytics
3. Implement `etf/sec.rs` - SEC 30-day yield, compliance helpers
4. Implement `analytics/liquidity.rs` - Liquidity metrics

**Deliverable**: Full ETF analytics.

### Phase 8: Integration & Polish
**Estimated complexity: Low**

1. Add comprehensive documentation
2. Add integration tests with realistic portfolios
3. Add property-based tests for invariants
4. Performance benchmarks

---

## 5. Open Questions

### 5.1 Design Decisions (RESOLVED)

| Decision | Resolution |
|----------|------------|
| Trait vs Struct for Portfolio | **Struct** - Start simple, add trait later if needed |
| Pre-calculated vs Lazy Analytics | **Pre-calculated** - Caller provides via `HoldingAnalytics` (pure functions) |
| Benchmark Representation | **Same `Portfolio` type** - Consistency, reuse analytics |
| FX Handling | **Supported from Phase 1** - `fx_rate` on Holding and CashPosition |
| Sector Taxonomy | **Flexible** - Normalized `Sector` enum + provider map for BICS/GICS |
| Credit Rating Scale | **Flexible** - Normalized `CreditRating` enum + provider map for any agency |
| Seniority/Capital Structure | **Flexible** - `SeniorityInfo` handles AT1/CoCo/bail-in |
| Parallel Processing | **Config-driven rayon** - Threshold-based with feature flag |

### 5.2 Remaining Clarifications

1. **Key Rate Tenor Standard**
   - Use existing `STANDARD_KEY_RATE_TENORS` (3M, 6M, 1Y, 2Y, 3Y, 5Y, 7Y, 10Y, 20Y, 30Y)?
   - Any additional tenors for specific markets (e.g., 15Y for mortgages)?

2. **SEC 30-Day Yield Formula**
   - Standard formula well-defined
   - Edge cases: new funds with <30 days history?
   - Distribution yield: trailing 12M vs annualized last distribution?

3. **Incremental Updates**
   - Option A: Always recompute from scratch (simpler, pure)
   - Option B: Support delta updates for large portfolios
   - **Leaning toward A** - aligns with pure function philosophy

### 5.3 Performance Considerations

| Aspect | Decision |
|--------|----------|
| Parallel threshold | Default 100 holdings, configurable via `AnalyticsConfig` |
| Feature flag | `parallel` feature enables rayon, off by default |
| Large portfolios (>5000) | Consider chunked processing for memory locality |

---

## 6. Testing Strategy

### 6.1 Unit Tests

Each module will have inline tests for:
- Happy path calculations
- Edge cases (empty portfolio, single holding, all cash)
- Error conditions (invalid inputs, missing data)
- Numerical accuracy (compare to known values)

### 6.2 Integration Tests

Create test portfolios:
- `tests/fixtures/sample_portfolio.rs` - 10-20 holding test portfolio
- `tests/fixtures/etf_portfolio.rs` - ETF-style portfolio with ~100 holdings
- `tests/fixtures/multi_currency.rs` - Multi-currency portfolio

### 6.3 Property-Based Tests

Key invariants to test:
- Sum of contribution weights = 100%
- Sum of sector weights = 100%
- Portfolio DV01 = sum of holding DV01s
- NAV = Securities MV + Cash + Accrued - Liabilities
- Weighted duration × Market value ≈ sum of (duration_i × MV_i)

### 6.4 Benchmark Tests

Performance targets:
- 100 holdings: <100μs for full analytics
- 500 holdings: <500μs for full analytics
- 1000 holdings: <1ms for full analytics

---

## 7. Documentation Requirements

### 7.1 Rustdoc

Every public item will have:
- One-line summary
- Detailed description with formula where applicable
- Example usage
- Error conditions

### 7.2 Formulas

Document all formulas in rustdoc, matching industry standards:
```rust
/// Weighted average yield to maturity.
///
/// ## Formula
///
/// ```text
/// YTM_portfolio = Σ(w_i × YTM_i)
/// ```
///
/// Where:
/// - `w_i` = market value weight of holding i
/// - `YTM_i` = yield to maturity of holding i
pub fn weighted_ytm(holdings: &[Holding], method: WeightingMethod) -> f64
```

---

## 8. Success Criteria

The portfolio module is complete when:

1. **Functionality**: All 12 categories of analytics from requirements are implemented
2. **Purity**: All functions are pure (no I/O, no caching, no side effects)
3. **Performance**: Meets sub-millisecond target for typical portfolios
4. **Accuracy**: Uses industry-standard portfolio methodology for risk metrics
5. **Testing**: >90% code coverage with unit, integration, and property tests
6. **Documentation**: Full rustdoc with formulas and examples
7. **Integration**: Clean integration with existing Convex crates

---

## Appendix A: Industry Reference

### Industry-Standard Portfolio Methodology

- Duration: Uses modified duration with market value weighting
- DV01: Aggregated as sum of individual DV01s
- OAS: Market-value weighted average
- Key Rate: Summed contributions at each tenor

### SEC 30-Day Yield Formula

```
SEC Yield = 2 × ((a-b+c)/(cd)) + 1)^6 - 1)

Where:
a = dividends and interest collected
b = accrued expenses
c = shares outstanding
d = maximum offer price per share
```

### CFA Fixed Income Attribution

- Income return = coupon/price
- Treasury return = -(duration × Δy) + convexity adjustment
- Spread return = -(spread duration × Δspread)
- Residual = total - income - treasury - spread

# Claude Code Prompts for Convex Development

> **Note**: This file contains implementation GUIDANCE (code templates, API patterns).
> For project STATE (decisions, progress, validation), see `memory.md`.

## Essential: Session Start Prompt

**Always begin each Claude Code session with this:**

```
Please read these project files in order:
1. .claude/context.md - Domain knowledge, accuracy requirements
2. .claude/architecture.md - System design, crate structure
3. .claude/conventions.md - Coding standards (if exists)
4. .claude/memory.md - Decisions and progress

Then run: tree -L 2 src/

Confirm you understand the Convex project before we proceed.
```

---

## Core Infrastructure Prompts

### 1. Core Types

```markdown
## Task: Implement Core Domain Types in convex-core

### Pre-Implementation
Read .claude/context.md sections: Accuracy Targets, Numerical Precision

### Requirements

Implement these newtypes in `convex-core/src/types/`:

**Price Types:**
- CleanPrice (always positive, percentage of par)
- DirtyPrice (includes accrued)

**Yield Types:**
- Yield (annual, as decimal e.g., 0.05 = 5%)

**Spread Types:**
- Spread (basis points, can be negative)

**Rate Types:**
- Rate (generic: coupon, discount, forward)

**Risk Types:**
- Duration, Convexity, DV01

### Implementation Pattern

```rust
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Yield(Decimal);

impl Yield {
    pub fn from_percent(pct: f64) -> Self;
    pub fn from_decimal(dec: Decimal) -> Self;
    pub fn as_decimal(&self) -> Decimal;
    pub fn as_percent(&self) -> f64;
}
```

### Validation
- All types must use Decimal for precision
- Implement Display with appropriate precision
- Include unit tests for all conversions
- Test edge cases (zero, negative where valid)
```

### 2. Day Count Conventions

```markdown
## Task: Implement Day Count Conventions in convex-core

### Pre-Implementation
Read .claude/context.md section: Day Count Conventions

### Requirements

Implement ALL day count conventions exactly matching Bloomberg:

**ACT Family:**
- ACT/360, ACT/365F, ACT/365L
- ACT/ACT ICMA (period-based)
- ACT/ACT ISDA (year-based)
- ACT/ACT AFB (French)

**30 Family:**
- 30/360 US (with exact month-end rules)
- 30E/360 (Eurobond)
- 30E/360 ISDA
- 30/360 German

### Critical: 30/360 US Month-End Rules

```rust
pub fn thirty_360_us(d1: Date, d2: Date) -> Decimal {
    // Rule 1: If D1 is last day of Feb → D1 = 30
    // Rule 2: If D1 = 31 → D1 = 30
    // Rule 3: If D2 is last day of Feb AND D1 was adjusted → D2 = 30
    // Rule 4: If D2 = 31 AND D1 >= 30 → D2 = 30
}
```

### Tests Required
- Bloomberg-verified test cases for each convention
- Leap year edge cases
- Month-end boundaries
- Same-day (zero fraction)
- Cross-year boundaries
```

### 3. Holiday Calendars

```markdown
## Task: Implement Holiday Calendar System in convex-core

### Pre-Implementation
Read .claude/context.md section: Settlement Conventions

### Requirements

Implement in `convex-core/src/calendar/`:

**Calendar Struct:**
- Bitmap storage for O(1) lookups
- Support 1970-2100 range
- Weekend rules (Sat/Sun, Fri/Sat, etc.)

**Calendars to Implement:**
- SIFMA (US bond market)
- US Government (Treasury)
- TARGET2 (Eurozone)
- UK Bank Holidays
- Japan

**Date Roll Conventions:**
- Following, ModifiedFollowing
- Preceding, ModifiedPreceding
- Unadjusted

**Methods:**
```rust
impl HolidayCalendar {
    fn is_business_day(&self, date: Date) -> bool;
    fn adjust(&self, date: Date, convention: DateRoll) -> Date;
    fn add_business_days(&self, date: Date, days: i32) -> Date;
    fn settlement_date(&self, trade: Date, days: u32) -> Date;
}
```

### Performance Target
- is_business_day: O(1), < 10ns
```

---

## Mathematical Engine Prompts

### 4. Solvers

```markdown
## Task: Implement Root-Finding Solvers in convex-math

### Requirements

Implement in `convex-math/src/solvers/`:

**Newton-Raphson:**
- Tolerance: 1e-10 (configurable)
- Max iterations: 100
- Analytical derivative when available

**Brent's Method:**
- For Z-spread and OAS
- Guaranteed convergence
- Tolerance: 1e-10

**Hybrid Solver:**
- Start with Newton
- Fall back to Brent if diverging

```rust
pub trait Solver {
    fn solve<F, D>(
        &self,
        f: F,
        derivative: Option<D>,
        initial_guess: f64,
        bounds: Option<(f64, f64)>,
    ) -> Result<f64, SolverError>
    where
        F: Fn(f64) -> f64,
        D: Fn(f64) -> f64;
}
```

### Performance Targets
- YTM solve: < 1μs
- Z-spread solve: < 50μs
```

### 5. Interpolation Methods

```markdown
## Task: Implement Interpolation Methods in convex-math

### Pre-Implementation
Read .claude/context.md section: Interpolation Methods

### Requirements

Implement ALL interpolation methods:

**On Zero Rates:**
- Linear
- Log-Linear
- Cubic Spline (Natural)
- Monotone Convex (Hagan) ← PRODUCTION DEFAULT

**Parametric:**
- Nelson-Siegel
- Svensson

```rust
pub trait Interpolator: Send + Sync {
    fn new(x: &[f64], y: &[f64]) -> Result<Self, InterpolationError>;
    fn interpolate(&self, x: f64) -> f64;
    fn derivative(&self, x: f64) -> f64;
}
```

### Monotone Convex Implementation
Critical for production - must ensure:
- Positive forward rates
- No oscillation
- C1 continuity

### Tests Required
- Accuracy vs known analytical solutions
- Edge cases (at/near pillars)
- Derivative accuracy
- Positive forward rate validation
```

### 6. Extrapolation Methods

```markdown
## Task: Implement Extrapolation Methods in convex-math

### Requirements

**Methods:**
- Flat (constant from last point)
- Linear (slope continuation)
- Smith-Wilson (regulatory standard)

**Smith-Wilson Implementation:**
```rust
pub struct SmithWilson {
    pub ultimate_forward_rate: f64,  // e.g., 4.2% for EUR
    pub convergence_speed: f64,      // Alpha
    pub last_liquid_point: f64,      // e.g., 20Y
}
```

Must match EIOPA specification exactly for regulatory curves.

### Tests
- Convergence to UFR
- Smoothness at transition point
- Match regulatory test cases
```

---

## Curve Construction Prompts

### 7. Curve Bootstrap

```markdown
## Task: Implement Curve Bootstrapping in convex-curves

### Pre-Implementation
Read .claude/context.md section: Yield Curve Construction
Read .claude/memory.md section: Milestone 3 Detailed Specification

### Phase 7.1: Core Curve Infrastructure

**Curve Trait:**
```rust
pub trait Curve: Send + Sync {
    /// Discount factor from reference date to time t
    fn discount_factor(&self, t: f64) -> MathResult<f64>;

    /// Zero rate with specified compounding
    fn zero_rate(&self, t: f64, compounding: Compounding) -> MathResult<f64>;

    /// Forward rate between t1 and t2
    fn forward_rate(&self, t1: f64, t2: f64) -> MathResult<f64>;

    /// Instantaneous forward rate at time t
    fn instantaneous_forward(&self, t: f64) -> MathResult<f64>;

    /// Reference/valuation date
    fn reference_date(&self) -> Date;

    /// Maximum maturity with market data
    fn max_date(&self) -> Date;
}

pub enum Compounding {
    Continuous,
    Annual,
    SemiAnnual,
    Quarterly,
    Monthly,
    Simple,
}
```

**Curve Types:**
```rust
pub struct DiscountCurve {
    reference_date: Date,
    pillar_dates: Vec<Date>,
    discount_factors: Vec<f64>,
    interpolator: Box<dyn Interpolator>,
    extrapolator: Box<dyn Extrapolator>,
}

pub struct ForwardCurve {
    base_curve: Arc<DiscountCurve>,
    spread: Option<SpreadCurve>,
}

pub struct SpreadCurve {
    spread_type: SpreadType,  // Additive or Multiplicative
    pillars: Vec<(Date, f64)>,
    interpolator: Box<dyn Interpolator>,
}
```

### Phase 7.2: Curve Instruments

**Instrument Trait:**
```rust
pub trait CurveInstrument: Send + Sync {
    /// Maturity date of the instrument
    fn maturity(&self) -> Date;

    /// Pillar date (may differ from maturity for some instruments)
    fn pillar_date(&self) -> Date;

    /// Calculate PV given a discount curve (should be 0 at par)
    fn pv(&self, curve: &dyn Curve) -> MathResult<f64>;

    /// Implied discount factor at pillar (for sequential bootstrap)
    fn implied_df(
        &self,
        curve: &dyn Curve,
        target_pv: f64,
    ) -> MathResult<f64>;

    /// Instrument type for sorting/grouping
    fn instrument_type(&self) -> InstrumentType;
}

pub enum InstrumentType {
    Deposit,
    FRA,
    Future,
    Swap,
    OIS,
    BasisSwap,
    Bond,
}
```

**Money Market Deposit:**
```rust
pub struct Deposit {
    start_date: Date,
    end_date: Date,
    rate: f64,                    // Simple rate (ACT/360 or ACT/365)
    day_count: DayCountConvention,
    notional: f64,
}

impl CurveInstrument for Deposit {
    fn implied_df(&self, curve: &dyn Curve, _target: f64) -> MathResult<f64> {
        // DF(end) = DF(start) / (1 + r × τ)
        let df_start = curve.discount_factor(self.start_date)?;
        let tau = self.day_count.year_fraction(self.start_date, self.end_date);
        Ok(df_start / (1.0 + self.rate * tau))
    }
}
```

**Forward Rate Agreement (FRA):**
```rust
pub struct FRA {
    trade_date: Date,
    start_date: Date,      // Accrual start (e.g., 3M from trade)
    end_date: Date,        // Accrual end (e.g., 6M from trade)
    fixed_rate: f64,       // Contracted rate
    day_count: DayCountConvention,
    notional: f64,
}

impl CurveInstrument for FRA {
    fn pv(&self, curve: &dyn Curve) -> MathResult<f64> {
        // FRA PV = N × τ × (F - K) × DF(payment)
        // where F = (DF(start)/DF(end) - 1) / τ
        let df_start = curve.discount_factor(self.start_date)?;
        let df_end = curve.discount_factor(self.end_date)?;
        let tau = self.day_count.year_fraction(self.start_date, self.end_date);
        let forward = (df_start / df_end - 1.0) / tau;
        let df_pay = curve.discount_factor(self.start_date)?;  // FRA settles at start
        Ok(self.notional * tau * (forward - self.fixed_rate) * df_pay)
    }
}
```

**SOFR/Eurodollar Futures:**
```rust
pub struct RateFuture {
    future_type: FutureType,      // SOFR1M, SOFR3M, Eurodollar
    last_trading_date: Date,      // IMM date
    accrual_start: Date,
    accrual_end: Date,
    price: f64,                   // e.g., 95.25 → rate = 4.75%
    convexity_adjustment: f64,    // Futures vs forward adjustment
}

pub enum FutureType {
    SOFR1M,
    SOFR3M,
    Eurodollar,  // Legacy, for historical curves
}

impl RateFuture {
    pub fn implied_rate(&self) -> f64 {
        (100.0 - self.price) / 100.0 - self.convexity_adjustment
    }
}
```

**Interest Rate Swap (IRS):**
```rust
pub struct Swap {
    effective_date: Date,
    termination_date: Date,
    fixed_rate: f64,
    fixed_frequency: Frequency,
    fixed_day_count: DayCountConvention,
    float_index: RateIndex,       // SOFR, EURIBOR, etc.
    float_frequency: Frequency,
    float_day_count: DayCountConvention,
    notional: f64,
}

impl CurveInstrument for Swap {
    fn pv(&self, curve: &dyn Curve) -> MathResult<f64> {
        // Fixed Leg: Σ c × τi × DF(Ti)
        // Float Leg: DF(T0) - DF(Tn)  (telescoping under single curve)
        let fixed_pv = self.fixed_leg_pv(curve)?;
        let float_pv = self.float_leg_pv(curve)?;
        Ok(fixed_pv - float_pv)
    }

    fn implied_df(&self, curve: &dyn Curve, target_pv: f64) -> MathResult<f64> {
        // For bootstrap: solve Σ c×τi×DF(Ti) = DF(T0) - DF(Tn)
        // Only DF(Tn) is unknown
        let sum_known = self.sum_known_fixed_leg(curve)?;
        let df_start = curve.discount_factor(self.effective_date)?;
        let last_tau = self.fixed_day_count.year_fraction(
            self.second_to_last_date(), self.termination_date
        );
        // sum_known + c×τn×DF(Tn) = DF(T0) - DF(Tn)
        // DF(Tn) × (1 + c×τn) = DF(T0) - sum_known
        Ok((df_start - sum_known) / (1.0 + self.fixed_rate * last_tau))
    }
}
```

**Overnight Index Swap (OIS):**
```rust
pub struct OIS {
    effective_date: Date,
    termination_date: Date,
    fixed_rate: f64,
    day_count: DayCountConvention,  // ACT/360 for SOFR
    payment_lag: u32,               // 2 days for SOFR
    notional: f64,
}

impl CurveInstrument for OIS {
    fn implied_df(&self, curve: &dyn Curve, _target: f64) -> MathResult<f64> {
        // OIS approximation: DF(end) = DF(start) / (1 + c × τ)
        let df_start = curve.discount_factor(self.effective_date)?;
        let tau = self.day_count.year_fraction(
            self.effective_date, self.termination_date
        );
        Ok(df_start / (1.0 + self.fixed_rate * tau))
    }
}
```

**Basis Swap:**
```rust
pub struct BasisSwap {
    effective_date: Date,
    termination_date: Date,
    pay_index: RateIndex,         // e.g., SOFR 1M
    receive_index: RateIndex,     // e.g., SOFR 3M
    spread: f64,                  // Basis spread on pay leg
    notional: f64,
}
```

**Treasury Bill (Discount Instrument):**
```rust
pub struct TreasuryBill {
    cusip: String,
    settlement_date: Date,
    maturity_date: Date,
    price: f64,                   // e.g., 99.50 per 100 face
    face_value: f64,              // Usually 100
}

impl TreasuryBill {
    /// Bank discount rate
    pub fn discount_rate(&self) -> f64 {
        let days = (self.maturity_date - self.settlement_date).num_days() as f64;
        (self.face_value - self.price) / self.face_value * (360.0 / days)
    }

    /// Bond equivalent yield
    pub fn bond_equivalent_yield(&self) -> f64 {
        let days = (self.maturity_date - self.settlement_date).num_days() as f64;
        (self.face_value - self.price) / self.price * (365.0 / days)
    }
}

impl CurveInstrument for TreasuryBill {
    fn pillar_date(&self) -> Date {
        self.maturity_date
    }

    fn pv(&self, curve: &dyn Curve) -> MathResult<f64> {
        let t = year_fraction(self.settlement_date, self.maturity_date);
        let df = curve.discount_factor(t)?;
        let theoretical = self.face_value * df;
        Ok(theoretical - self.price)  // Should be ~0 when curve is correct
    }

    fn implied_df(&self, _curve: &dyn Curve, _target: f64) -> MathResult<f64> {
        // Direct: DF = Price / Face
        Ok(self.price / self.face_value)
    }
}
```

**Treasury Note/Bond (Coupon Instrument):**
```rust
pub struct TreasuryBond {
    cusip: String,
    settlement_date: Date,
    maturity_date: Date,
    coupon_rate: f64,             // e.g., 0.045 = 4.5%
    frequency: Frequency,          // SemiAnnual for US Treasuries
    day_count: DayCountConvention, // ACT/ACT ICMA
    clean_price: f64,
    face_value: f64,
}

impl TreasuryBond {
    pub fn cash_flows(&self) -> Vec<CashFlow> {
        let coupon = self.face_value * self.coupon_rate / 2.0;
        let mut flows = Vec::new();
        let mut date = self.maturity_date;

        while date > self.settlement_date {
            let amount = if date == self.maturity_date {
                coupon + self.face_value
            } else {
                coupon
            };
            flows.push(CashFlow { date, amount });
            date = date - Months(6);
        }
        flows.reverse();
        flows
    }

    pub fn accrued_interest(&self) -> f64 {
        // ACT/ACT ICMA accrued calculation
        ...
    }

    pub fn dirty_price(&self) -> f64 {
        self.clean_price + self.accrued_interest()
    }
}

impl CurveInstrument for TreasuryBond {
    fn pillar_date(&self) -> Date {
        self.maturity_date
    }

    fn pv(&self, curve: &dyn Curve) -> MathResult<f64> {
        let mut theoretical = 0.0;
        for cf in self.cash_flows() {
            let t = year_fraction(self.settlement_date, cf.date);
            let df = curve.discount_factor(t)?;
            theoretical += cf.amount * df;
        }
        Ok(theoretical - self.dirty_price())
    }

    fn implied_df(&self, curve: &dyn Curve, _target: f64) -> MathResult<f64> {
        // Solve for DF at maturity given known DFs for earlier coupons
        let flows = self.cash_flows();
        let dirty = self.dirty_price();

        // PV of all coupons except final cash flow
        let mut known_pv = 0.0;
        for cf in flows.iter().take(flows.len() - 1) {
            let t = year_fraction(self.settlement_date, cf.date);
            let df = curve.discount_factor(t)?;
            known_pv += cf.amount * df;
        }

        // Solve: dirty = known_pv + final_cf × DF(maturity)
        let final_cf = flows.last().unwrap().amount;
        Ok((dirty - known_pv) / final_cf)
    }
}
```

**TIPS (Inflation-Protected):**
```rust
pub struct TIPS {
    cusip: String,
    settlement_date: Date,
    maturity_date: Date,
    real_coupon_rate: f64,
    base_cpi: f64,                // Reference CPI at issue
    frequency: Frequency,
    clean_price: f64,             // Real price
    face_value: f64,
}

impl TIPS {
    pub fn index_ratio(&self, ref_cpi: f64) -> f64 {
        (ref_cpi / self.base_cpi).max(1.0)  // Deflation floor
    }

    pub fn adjusted_principal(&self, ref_cpi: f64) -> f64 {
        self.face_value * self.index_ratio(ref_cpi)
    }
}

impl CurveInstrument for TIPS {
    fn pillar_date(&self) -> Date {
        self.maturity_date
    }

    fn pv(&self, curve: &dyn Curve) -> MathResult<f64> {
        // For real rate curve bootstrap
        // Similar to TreasuryBond but with real rates
        ...
    }

    fn implied_df(&self, curve: &dyn Curve, _target: f64) -> MathResult<f64> {
        // Same pattern as TreasuryBond
        ...
    }
}
```

**Generic Usage - Mix Any Instruments:**
```rust
// The bootstrapper is GENERIC - works with any instrument type
let curve = CurveBuilder::new(settlement)
    .with_interpolation(MonotoneConvex)
    .with_extrapolation(SmithWilson::eiopa_usd())
    // T-Bills for short end
    .add(TreasuryBill::new("3M", 99.50))
    .add(TreasuryBill::new("6M", 98.75))
    // Treasury Notes/Bonds for medium/long
    .add(TreasuryBond::new("2Y", 0.045, 99.25))
    .add(TreasuryBond::new("5Y", 0.0425, 100.50))
    .add(TreasuryBond::new("10Y", 0.0410, 98.00))
    .add(TreasuryBond::new("30Y", 0.0400, 95.50))
    // Can even mix with swaps if needed
    .add(OIS::new("50Y", 0.0380))
    .bootstrap()?;
```

### Phase 7.3: Bootstrap Methods

**Sequential Bootstrap (Primary):**
```rust
pub struct SequentialBootstrapper {
    interpolator: InterpolatorType,
    extrapolator: ExtrapolatorType,
    tolerance: f64,           // 1e-12
    max_iterations: u32,      // 50
}

impl SequentialBootstrapper {
    pub fn bootstrap(
        &self,
        instruments: Vec<Box<dyn CurveInstrument>>,
        reference_date: Date,
    ) -> MathResult<DiscountCurve> {
        // 1. Sort instruments by pillar date
        let sorted = self.sort_by_pillar(instruments);

        // 2. Initialize with DF(0) = 1.0
        let mut pillars = vec![(reference_date, 1.0)];

        // 3. Bootstrap each instrument sequentially
        for instrument in sorted {
            let partial_curve = self.build_partial_curve(&pillars)?;
            let df = instrument.implied_df(&partial_curve, 0.0)?;
            pillars.push((instrument.pillar_date(), df));
        }

        // 4. Build final curve with interpolation
        self.build_final_curve(pillars, reference_date)
    }
}
```

**Global Fitting (Levenberg-Marquardt):**
```rust
pub struct GlobalBootstrapper {
    curve_type: GlobalCurveType,
    optimizer: LevenbergMarquardt,
    tolerance: f64,
}

pub enum GlobalCurveType {
    /// Zero rates at pillar points (most flexible)
    PiecewiseZero,
    /// Nelson-Siegel parameters (4 params)
    NelsonSiegel,
    /// Svensson parameters (6 params)
    Svensson,
    /// Penalized spline with smoothness objective
    SmoothSpline { roughness_penalty: f64 },
}

impl GlobalBootstrapper {
    pub fn bootstrap(
        &self,
        instruments: Vec<Box<dyn CurveInstrument>>,
        reference_date: Date,
    ) -> MathResult<DiscountCurve> {
        // Objective: min Σ wi × (PVi(curve))²
        // Subject to: curve smoothness constraints
        let objective = |params: &[f64]| -> f64 {
            let curve = self.build_curve_from_params(params);
            instruments.iter()
                .map(|inst| inst.pv(&curve).unwrap_or(1e10).powi(2))
                .sum()
        };

        let initial_guess = self.initial_params(&instruments)?;
        let optimal = self.optimizer.minimize(objective, &initial_guess)?;
        self.build_curve_from_params(&optimal)
    }
}
```

**Iterative Multi-Curve Bootstrap:**
```rust
pub struct IterativeMultiCurveBootstrapper {
    max_iterations: u32,      // 10
    tolerance: f64,           // 1e-10
}

impl IterativeMultiCurveBootstrapper {
    pub fn bootstrap(
        &self,
        ois_instruments: Vec<Box<dyn CurveInstrument>>,
        projection_instruments: Vec<Box<dyn CurveInstrument>>,
        reference_date: Date,
    ) -> MathResult<(DiscountCurve, ForwardCurve)> {
        // Initial guess: flat curve
        let mut discount_curve = self.flat_curve(reference_date, 0.04)?;
        let mut projection_curve = self.flat_curve(reference_date, 0.04)?;

        for _ in 0..self.max_iterations {
            let prev_discount = discount_curve.clone();

            // Bootstrap discount curve using projection curve
            discount_curve = self.bootstrap_discount(
                &ois_instruments, &projection_curve, reference_date
            )?;

            // Bootstrap projection curve using discount curve
            projection_curve = self.bootstrap_projection(
                &projection_instruments, &discount_curve, reference_date
            )?;

            // Check convergence
            if self.curves_converged(&discount_curve, &prev_discount) {
                break;
            }
        }

        Ok((discount_curve, projection_curve))
    }
}
```

### Phase 7.4: CurveBuilder API (Fluent Interface)

```rust
pub struct CurveBuilder {
    reference_date: Date,
    instruments: Vec<Box<dyn CurveInstrument>>,
    interpolator: InterpolatorType,
    extrapolator: ExtrapolatorType,
    bootstrap_method: BootstrapMethod,
    calendar: Box<dyn Calendar>,
}

impl CurveBuilder {
    pub fn new(reference_date: Date) -> Self { ... }

    pub fn with_interpolation(mut self, interp: InterpolatorType) -> Self {
        self.interpolator = interp;
        self
    }

    pub fn with_extrapolation(mut self, extrap: ExtrapolatorType) -> Self {
        self.extrapolator = extrap;
        self
    }

    // Add instruments with market conventions
    pub fn add_deposit(mut self, tenor: &str, rate: f64) -> Self { ... }
    pub fn add_fra(mut self, start: &str, end: &str, rate: f64) -> Self { ... }
    pub fn add_future(mut self, contract: &str, price: f64) -> Self { ... }
    pub fn add_swap(mut self, tenor: &str, rate: f64) -> Self { ... }
    pub fn add_ois(mut self, tenor: &str, rate: f64) -> Self { ... }

    pub fn bootstrap(self) -> MathResult<DiscountCurve> { ... }
}

// Usage Example
let curve = CurveBuilder::new(date!(2024-11-29))
    .with_interpolation(InterpolatorType::MonotoneConvex)
    .with_extrapolation(ExtrapolatorType::SmithWilson(SmithWilson::eiopa_eur()))
    // Short end: deposits
    .add_deposit("O/N", 0.0530)
    .add_deposit("1W", 0.0532)
    .add_deposit("1M", 0.0535)
    .add_deposit("3M", 0.0538)
    // Medium: SOFR futures
    .add_future("SFRZ4", 94.75)  // Dec 2024
    .add_future("SFRH5", 95.00)  // Mar 2025
    .add_future("SFRM5", 95.25)  // Jun 2025
    // Long end: OIS swaps
    .add_ois("2Y", 0.0425)
    .add_ois("3Y", 0.0420)
    .add_ois("5Y", 0.0415)
    .add_ois("7Y", 0.0412)
    .add_ois("10Y", 0.0410)
    .add_ois("15Y", 0.0408)
    .add_ois("20Y", 0.0405)
    .add_ois("30Y", 0.0400)
    .bootstrap()?;
```

### Phase 7.5: Validation & Quality Checks

```rust
pub struct CurveValidator {
    reprice_tolerance: f64,      // 0.0001 bps
    forward_floor: f64,          // 0.0 (positive forwards)
    smoothness_threshold: f64,   // Max |d²f/dt²|
}

impl CurveValidator {
    pub fn validate(&self, curve: &DiscountCurve, instruments: &[Box<dyn CurveInstrument>])
        -> MathResult<ValidationReport>
    {
        let mut report = ValidationReport::new();

        // Check 1: All instruments reprice to par
        for inst in instruments {
            let pv = inst.pv(curve)?;
            if pv.abs() > self.reprice_tolerance {
                report.add_error(ValidationError::RepriceFailed {
                    instrument: inst.description(),
                    pv,
                    tolerance: self.reprice_tolerance,
                });
            }
        }

        // Check 2: Forward rates positive
        for t in (0..3600).map(|m| m as f64 / 12.0) {  // Monthly for 300Y
            let fwd = curve.instantaneous_forward(t)?;
            if fwd < self.forward_floor {
                report.add_error(ValidationError::NegativeForward { time: t, rate: fwd });
            }
        }

        // Check 3: Discount factors decreasing
        let mut prev_df = 1.0;
        for t in (1..600).map(|m| m as f64 / 12.0) {
            let df = curve.discount_factor(t)?;
            if df >= prev_df {
                report.add_error(ValidationError::NonMonotonicDF { time: t });
            }
            prev_df = df;
        }

        // Check 4: Smoothness (no wild oscillations)
        self.check_smoothness(curve, &mut report)?;

        Ok(report)
    }
}
```

### Currency-Specific Conventions

**USD (Post-LIBOR):**
```rust
pub mod conventions {
    pub mod usd {
        pub const SPOT_DAYS: u32 = 2;
        pub const DEPOSIT_DAY_COUNT: DayCountConvention = DayCountConvention::Act360;
        pub const SWAP_FIXED_FREQ: Frequency = Frequency::Annual;
        pub const SWAP_FIXED_DC: DayCountConvention = DayCountConvention::Act360;
        pub const SWAP_FLOAT_FREQ: Frequency = Frequency::Annual;  // SOFR
        pub const CALENDAR: CalendarType = CalendarType::SIFMA;

        pub fn deposit(tenor: &str, rate: f64, ref_date: Date) -> Deposit {
            let start = add_business_days(ref_date, SPOT_DAYS, &SIFMA);
            let end = add_tenor(start, tenor, &SIFMA);
            Deposit::new(start, end, rate, DEPOSIT_DAY_COUNT)
        }

        pub fn ois_swap(tenor: &str, rate: f64, ref_date: Date) -> OIS {
            // SOFR OIS: Annual fixed, daily SOFR compounded
            ...
        }
    }
}
```

**EUR:**
```rust
pub mod conventions {
    pub mod eur {
        pub const SPOT_DAYS: u32 = 2;
        pub const DEPOSIT_DAY_COUNT: DayCountConvention = DayCountConvention::Act360;
        pub const SWAP_FIXED_FREQ: Frequency = Frequency::Annual;
        pub const SWAP_FIXED_DC: DayCountConvention = DayCountConvention::ThirtyE360;
        pub const SWAP_FLOAT_FREQ: Frequency = Frequency::SemiAnnual;  // 6M EURIBOR
        pub const CALENDAR: CalendarType = CalendarType::TARGET2;
    }
}
```

**GBP:**
```rust
pub mod conventions {
    pub mod gbp {
        pub const SPOT_DAYS: u32 = 0;  // Same day settlement
        pub const DEPOSIT_DAY_COUNT: DayCountConvention = DayCountConvention::Act365F;
        pub const SWAP_FIXED_FREQ: Frequency = Frequency::Annual;
        pub const SWAP_FIXED_DC: DayCountConvention = DayCountConvention::Act365F;
        pub const CALENDAR: CalendarType = CalendarType::UK;
    }
}
```

### Performance Targets
- Deposit bootstrap: < 1μs per instrument
- Swap bootstrap: < 10μs per instrument
- Full curve (50 instruments): < 1ms
- Curve interpolation: < 50ns

### Bloomberg Validation
Compare against Bloomberg FWCV/SWDF:
- Discount factors within 1e-8
- Zero rates within 0.01 bps
- Forward rates within 0.05 bps
```

### 8. Multi-Curve Framework

```markdown
## Task: Implement Multi-Curve Framework in convex-curves

### Pre-Implementation
Read .claude/memory.md section: Multi-Curve Architecture

### Phase 8.1: Curve Set Container

```rust
pub struct CurveSet {
    reference_date: Date,
    discount_curve: Arc<DiscountCurve>,
    projection_curves: HashMap<RateIndex, Arc<ForwardCurve>>,
    fx_curves: HashMap<CurrencyPair, Arc<FxForwardCurve>>,
}

impl CurveSet {
    /// Get discount factor from the OIS curve
    pub fn discount_factor(&self, date: Date) -> MathResult<f64> {
        self.discount_curve.discount_factor(date)
    }

    /// Get forward rate for a specific index
    pub fn forward_rate(
        &self,
        index: &RateIndex,
        start: Date,
        end: Date
    ) -> MathResult<f64> {
        let curve = self.projection_curves.get(index)
            .ok_or(CurveError::MissingCurve(index.clone()))?;
        curve.forward_rate(start, end)
    }

    /// Get FX forward rate
    pub fn fx_forward(&self, pair: &CurrencyPair, date: Date) -> MathResult<f64> {
        ...
    }
}
```

### Phase 8.2: Rate Indices

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RateIndex {
    // Overnight RFRs
    SOFR,
    ESTR,
    SONIA,
    TONA,
    SARON,
    CORRA,

    // Term rates
    TermSOFR { tenor: Tenor },
    EURIBOR { tenor: Tenor },
    TIBOR { tenor: Tenor },

    // Legacy (for existing trades)
    LIBOR { currency: Currency, tenor: Tenor },
}

impl RateIndex {
    pub fn day_count(&self) -> DayCountConvention { ... }
    pub fn fixing_lag(&self) -> u32 { ... }
    pub fn payment_lag(&self) -> u32 { ... }
    pub fn calendar(&self) -> CalendarType { ... }
    pub fn compounding(&self) -> CompoundingType { ... }
}
```

### Phase 8.3: Multi-Curve Builder

```rust
pub struct MultiCurveBuilder {
    reference_date: Date,
    discount_instruments: Vec<Box<dyn CurveInstrument>>,
    projection_instruments: HashMap<RateIndex, Vec<Box<dyn CurveInstrument>>>,
    basis_instruments: Vec<BasisSwap>,
}

impl MultiCurveBuilder {
    pub fn new(reference_date: Date) -> Self { ... }

    /// Add OIS instruments for discount curve
    pub fn add_ois(mut self, tenor: &str, rate: f64) -> Self { ... }

    /// Add projection curve instruments (e.g., SOFR swaps, EURIBOR swaps)
    pub fn add_projection(
        mut self,
        index: RateIndex,
        tenor: &str,
        rate: f64
    ) -> Self { ... }

    /// Add basis swap for tenor basis
    pub fn add_basis_swap(
        mut self,
        pay_index: RateIndex,
        receive_index: RateIndex,
        tenor: &str,
        spread: f64,
    ) -> Self { ... }

    /// Build all curves with iterative bootstrap
    pub fn build(self) -> MathResult<CurveSet> {
        let bootstrapper = IterativeMultiCurveBootstrapper::new();

        // 1. Build discount curve (OIS)
        let discount = bootstrapper.bootstrap_discount(&self.discount_instruments)?;

        // 2. Build projection curves relative to discount
        let mut projections = HashMap::new();
        for (index, instruments) in &self.projection_instruments {
            let proj = bootstrapper.bootstrap_projection(
                instruments, &discount, index
            )?;
            projections.insert(index.clone(), Arc::new(proj));
        }

        // 3. Apply basis adjustments if any
        if !self.basis_instruments.is_empty() {
            self.apply_basis_adjustments(&mut projections)?;
        }

        Ok(CurveSet {
            reference_date: self.reference_date,
            discount_curve: Arc::new(discount),
            projection_curves: projections,
            fx_curves: HashMap::new(),
        })
    }
}

// Usage Example
let curves = MultiCurveBuilder::new(date!(2024-11-29))
    // Discount curve (SOFR OIS)
    .add_ois("1M", 0.0530)
    .add_ois("3M", 0.0525)
    .add_ois("6M", 0.0520)
    .add_ois("1Y", 0.0510)
    .add_ois("2Y", 0.0480)
    .add_ois("5Y", 0.0450)
    .add_ois("10Y", 0.0420)
    .add_ois("30Y", 0.0400)
    // Term SOFR 3M projection curve
    .add_projection(RateIndex::TermSOFR { tenor: Tenor::M3 }, "1Y", 0.0515)
    .add_projection(RateIndex::TermSOFR { tenor: Tenor::M3 }, "2Y", 0.0485)
    .add_projection(RateIndex::TermSOFR { tenor: Tenor::M3 }, "5Y", 0.0455)
    // Basis: 1M vs 3M SOFR
    .add_basis_swap(
        RateIndex::TermSOFR { tenor: Tenor::M1 },
        RateIndex::TermSOFR { tenor: Tenor::M3 },
        "5Y",
        0.0008,  // 8 bps
    )
    .build()?;
```

### Phase 8.4: Cross-Currency Framework

```rust
pub struct CrossCurrencyBuilder {
    domestic_curves: CurveSet,
    foreign_curves: CurveSet,
    fx_spot: f64,
    xccy_basis_instruments: Vec<CrossCurrencyBasisSwap>,
}

pub struct CrossCurrencyBasisSwap {
    domestic_currency: Currency,
    foreign_currency: Currency,
    tenor: Tenor,
    basis_spread: f64,  // Spread on domestic leg
    domestic_index: RateIndex,
    foreign_index: RateIndex,
}

impl CrossCurrencyBuilder {
    pub fn build_fx_forward_curve(&self) -> MathResult<FxForwardCurve> {
        // FX forward from interest rate parity + basis
        // F(t) = S × DF_for(t) / DF_dom(t) × basis_adjustment(t)
        ...
    }
}
```

### Phase 8.5: Curve Sensitivities

```rust
pub struct CurveSensitivityCalculator {
    bump_size: f64,  // 1 bp = 0.0001
    bump_type: BumpType,
}

pub enum BumpType {
    Parallel,           // All pillars by same amount
    KeyRate(Tenor),     // Single pillar
    Bucket(Tenor, Tenor), // Range of pillars
}

impl CurveSensitivityCalculator {
    /// Calculate DV01 to curve moves
    pub fn dv01(
        &self,
        instrument: &dyn Priceable,
        curves: &CurveSet,
        bump_curve: &str,  // Which curve to bump
    ) -> MathResult<f64> {
        let base_pv = instrument.pv(curves)?;

        let bumped_curves = self.bump_curve(curves, bump_curve, self.bump_size)?;
        let bumped_pv = instrument.pv(&bumped_curves)?;

        Ok((bumped_pv - base_pv) / self.bump_size)
    }

    /// Calculate key rate durations
    pub fn key_rate_durations(
        &self,
        instrument: &dyn Priceable,
        curves: &CurveSet,
        tenors: &[Tenor],  // e.g., [2Y, 5Y, 10Y, 30Y]
    ) -> MathResult<HashMap<Tenor, f64>> {
        let mut krds = HashMap::new();
        for tenor in tenors {
            let krd = self.dv01_to_tenor(instrument, curves, *tenor)?;
            krds.insert(*tenor, krd);
        }
        Ok(krds)
    }
}
```

### Tests Required
- Single curve bootstrap reprices all instruments
- Multi-curve iterative convergence
- FRA pricing under dual curve matches market
- Swap pricing with OIS discounting
- Basis swap spread correctly reflected
- Cross-currency FX forwards consistent
- Key rate durations sum to total DV01

### Performance Targets
- Single curve build: < 1ms
- Multi-curve build: < 10ms
- Curve bump: < 100μs
- Full sensitivity grid (50 pillars): < 50ms
```

---

## Bond Pricing Prompts

### 9. Fixed Rate Bond

```markdown
## Task: Implement Fixed Rate Bond Pricing in convex-bonds

### Pre-Implementation
Read .claude/context.md section: Pricing Methodologies

### Requirements

**Bond Struct:**
```rust
pub struct FixedRateBond {
    identifiers: BondIdentifiers,
    coupon_rate: Rate,
    maturity: Date,
    issue_date: Date,
    frequency: Frequency,
    day_count: DayCountConvention,
    settlement_days: u32,
    calendar: CalendarId,
}
```

**Implement:**
- Cash flow generation
- Accrued interest calculation
- Price from yield (closed form)
- Yield from price (Newton-Raphson)
- Clean/dirty price conversion

**Bloomberg Validation:**
Use Boeing 7.5% 06/15/2025 as primary test case:
- Settlement: 04/29/2020
- Price: 110.503
- Expected YTM: 4.905895%

### Tolerance
- Yield: ±0.00001%
- Price: ±0.0001
```

### 10. US Treasury Securities

```markdown
## Task: Implement US Treasury Securities in convex-bonds

### Requirements

**Treasury Note/Bond:**
- ACT/ACT ICMA day count
- Semi-annual frequency
- T+1 settlement
- 32nds price quote parsing

**Treasury Bill:**
```rust
pub fn tbill_price_from_discount(
    discount_rate: Rate,
    settlement: Date,
    maturity: Date,
) -> CleanPrice;

pub fn tbill_bond_equivalent_yield(
    price: CleanPrice,
    settlement: Date,
    maturity: Date,
) -> Yield;
```

**Price Quote Parsing:**
```rust
// "99-16+" = 99 + 16.5/32 = 99.515625
pub fn parse_treasury_price(quote: &str) -> Result<CleanPrice, ParseError>;
```

### Validation
- Compare to Treasury Direct
- Bloomberg UST pricing
```

### 11. TIPS (Inflation-Linked)

```markdown
## Task: Implement TIPS in convex-bonds

### Requirements

**TIPS Struct:**
```rust
pub struct Tips {
    cusip: String,
    real_coupon: Rate,           // Real (not nominal) coupon
    maturity: Date,
    base_cpi: Decimal,           // Reference CPI at issue
    deflation_floor: bool,       // Usually true
}
```

**Calculations:**
- Index ratio = Reference CPI / Base CPI
- Inflation-adjusted principal
- Real yield calculation
- Breakeven inflation

### CPI Indexation
- 3-month lag
- Linear interpolation between monthly values

### Validation
- Bloomberg TIPS pricing
- Treasury inflation calculations
```

### 12. Callable Bonds

```markdown
## Task: Implement Callable Bond Pricing in convex-bonds

### Requirements

**Call Schedule:**
```rust
pub struct CallSchedule {
    features: Vec<CallFeature>,
}

pub struct CallFeature {
    first_call_date: Date,
    call_price: Decimal,
    call_frequency: Option<Frequency>,
}
```

**Yield Calculations:**
- Yield to call (each call date)
- Yield to worst (minimum of all)

**OAS Calculation:**
```rust
pub struct OASCalculator {
    rate_model: RateModel,      // Hull-White or BDT
    tree_steps: usize,          // 100+ for accuracy
    volatility: f64,            // ATM swaption vol
}
```

**Binomial Tree:**
- Build interest rate tree
- Backward induction with exercise
- Solve for OAS matching market price

### Performance Target
- OAS: < 10ms (100 step tree)
```

### 13. Municipal Bonds

```markdown
## Task: Implement Municipal Bond Pricing in convex-bonds

### Requirements

**Tax-Equivalent Yield:**
```rust
pub fn taxable_equivalent_yield(
    tax_exempt_yield: Yield,
    federal_tax_rate: Decimal,
    state_tax_rate: Option<Decimal>,
    is_amt_subject: bool,
) -> Yield;
```

**De Minimis Rule:**
- Threshold: 0.25% × years to maturity
- Affects tax treatment of discount

**Bond Types:**
- General Obligation (GO)
- Revenue
- Pre-refunded

### Validation
- EMMA data
- Bloomberg MUNI pricing
```

### 14. MBS Pass-Through

```markdown
## Task: Implement MBS Pass-Through Pricing in convex-bonds

### Requirements

**MBS Structure:**
```rust
pub struct MBSPassThrough {
    pool_number: String,
    issuer: AgencyIssuer,       // GNMA, FNMA, FHLMC
    original_balance: Decimal,
    current_factor: Decimal,
    pass_through_rate: Rate,
    wam: u32,                   // Weighted avg maturity (months)
    warm: u32,                  // Weighted avg remaining maturity
}
```

**Prepayment Models:**
```rust
pub enum PrepaymentModel {
    CPR(f64),                   // Constant prepayment rate
    SMM(f64),                   // Single monthly mortality
    PSA(f64),                   // PSA speed (100 = standard)
    Vector(Vec<f64>),           // Custom vector
}
```

**Cash Flow Projection:**
- Monthly scheduled principal
- Prepayment calculation
- Interest calculation
- Factor adjustment

**Yield Table:**
- Yield at various PSA speeds
- Price sensitivity to prepayment

### Performance Target
- Full cash flow projection: < 100ms
```

---

## Spread Calculation Prompts

### 15. Z-Spread

```markdown
## Task: Implement Z-Spread Calculation in convex-spreads

### Requirements

**Definition:**
Z-spread is the constant spread over the spot curve that reprices the bond.

```rust
pub struct ZSpreadCalculator {
    tolerance: Decimal,      // 1e-8 bps
    max_iterations: u32,     // 50
}

impl ZSpreadCalculator {
    pub fn calculate(
        &self,
        bond: &impl Bond,
        settlement: Date,
        dirty_price: Decimal,
        spot_curve: &impl SpotCurve,
    ) -> Result<Spread, SpreadError>;
}
```

**Algorithm:**
- Use Brent's method (more robust than Newton)
- Bracket between -100bps and 2000bps
- Continuous discounting

### Bloomberg Validation
Boeing bond Z-spread: 444.7 bps (±0.1 bps)
```

### 16. OAS Calculation

```markdown
## Task: Implement OAS Calculation in convex-spreads

### Requirements

OAS = Spread over tree that prices callable bond correctly

**Algorithm:**
1. Build interest rate tree
2. Calibrate to ATM swaption vols
3. Backward induction with optimal call exercise
4. Solve for OAS matching market price

**Tree Implementation:**
```rust
pub struct BinomialTree {
    steps: usize,
    dt: f64,
    rates: Vec<Vec<f64>>,        // Rate at each node
    probabilities: Vec<Vec<f64>>, // Transition probs
}
```

**OAS vs Z-Spread:**
- OAS < Z-spread for callable bonds (option value)
- Difference = option cost in bps

### Performance Target
- 100 step tree: < 10ms
```

---

## Risk Calculation Prompts

### 17. Duration & Convexity

```markdown
## Task: Implement Risk Metrics in convex-risk

### Requirements

**Duration Types:**
```rust
// Macaulay: weighted average time
pub fn macaulay_duration(
    cash_flows: &[CashFlow],
    settlement: Date,
    yield_val: Yield,
    day_count: &impl DayCount,
) -> Duration;

// Modified: price sensitivity
pub fn modified_duration(macaulay: Duration, yield_val: Yield, freq: Frequency) -> Duration;

// Effective: for bonds with optionality
pub fn effective_duration(
    bond: &impl Bond,
    settlement: Date,
    curve: &impl Curve,
    bump_size: Decimal,
) -> Duration;
```

**Convexity:**
```rust
pub fn analytical_convexity(...) -> Convexity;
pub fn effective_convexity(...) -> Convexity;
```

**DV01:**
```rust
pub fn dv01(
    modified_duration: Duration,
    dirty_price: Decimal,
    face_value: Decimal,
) -> DV01;
```

### Bloomberg Validation
Boeing bond:
- Mod Duration: 4.209 (±0.001)
- Convexity: 0.219 (±0.001)
```

---

## YAS Replication Prompts

### 18. Full YAS Implementation

```markdown
## Task: Implement Bloomberg YAS Replication in convex-yas

### Requirements

**YAS Analysis Result:**
```rust
pub struct YasAnalysis {
    // Yields
    pub street_convention: Yield,
    pub true_yield: Yield,
    pub current_yield: Yield,
    
    // Spreads
    pub g_spread: Spread,
    pub i_spread: Spread,
    pub z_spread: Spread,
    pub asw_spread: Spread,
    
    // Risk
    pub modified_duration: Duration,
    pub convexity: Convexity,
    pub dv01: DV01,
    
    // Invoice
    pub invoice: SettlementInvoice,
}
```

**Sequential Roll-Forward:**
For bonds < 1 year, must use Bloomberg's exact methodology

### Bloomberg Validation
ALL fields must match within tolerance for Boeing bond:
- Street Convention: 4.905895% (±0.00001%)
- G-Spread: 448.5 bps (±0.1 bps)
- Z-Spread: 444.7 bps (±0.1 bps)
- Mod Duration: 4.209 (±0.001)
```

---

## Testing & Validation Prompts

### 19. Bloomberg Validation Suite

```markdown
## Task: Create Bloomberg Validation Test Suite

### Requirements

Create comprehensive validation tests in `tests/bloomberg_validation/`:

**Test Structure:**
```rust
#[test]
fn test_boeing_full_yas() {
    let bond = create_boeing_bond();
    let settlement = date!(2020-04-29);
    let price = CleanPrice::new(dec!(110.503)).unwrap();
    
    let yas = YasAnalysis::calculate(&bond, settlement, price, &curve).unwrap();
    
    assert_bloomberg_match!(yas.street_convention.as_percent(), 4.905895, 0.00001);
    assert_bloomberg_match!(yas.g_spread.as_bps(), 448.5, 0.1);
    // ... all fields
}
```

**Validation Macro:**
```rust
#[macro_export]
macro_rules! assert_bloomberg_match {
    ($actual:expr, $expected:expr, $tolerance:expr) => {
        let diff = ($actual - $expected).abs();
        assert!(diff <= $tolerance, 
            "Bloomberg mismatch: expected {}, got {}, diff {}", 
            $expected, $actual, diff);
    };
}
```

### Coverage Required
- All bond types
- All day count conventions
- All spread types
- All risk metrics
- All curve operations
```

### 20. Performance Benchmarks

```markdown
## Task: Create Performance Benchmark Suite

### Requirements

Create benchmarks in `benches/`:

```rust
use criterion::{criterion_group, criterion_main, Criterion, black_box};

fn bench_yield_calculation(c: &mut Criterion) {
    let bond = create_benchmark_bond();
    let settlement = date!(2024-06-15);
    let price = dec!(99.5);
    
    c.bench_function("yield_from_price", |b| {
        b.iter(|| calculate_yield(
            black_box(&bond),
            black_box(settlement),
            black_box(price),
        ))
    });
}

criterion_group!(benches, 
    bench_yield_calculation,
    bench_z_spread,
    bench_curve_bootstrap,
    bench_interpolation,
);
criterion_main!(benches);
```

### Targets
- Yield calculation: < 1μs
- Z-spread: < 50μs
- Curve bootstrap: < 100μs
- Portfolio (1000): < 100ms
```

---

## Quick Reference

### File Locations
- Domain knowledge: `.claude/context.md`
- Architecture: `.claude/architecture.md`
- Progress tracking: `.claude/memory.md`

### Commands
```bash
cargo test --workspace              # All tests
cargo test -p convex-bonds          # Crate tests
cargo test --test bloomberg         # Validation tests
cargo bench                         # Benchmarks
cargo clippy -- -D warnings         # Linting
cargo fmt                           # Formatting
cargo doc --open                    # Documentation
```

### Session End
```
Please update .claude/memory.md with:
- What was implemented
- Any decisions made
- Validation status
- Open issues
```

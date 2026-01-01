# Portfolio Module Refactoring Analysis

This document analyzes which components in `convex-portfolio` should be generalized and moved to shared locations versus those that are correctly placed in the portfolio module.

## Summary Table

| Component | Current Location | Recommendation | Rationale |
|-----------|------------------|----------------|-----------|
| `CreditRating` | portfolio/types/classification.rs | **Move to convex-bonds** | Fundamental bond attribute, needed for pricing/risk without portfolio |
| `RatingBucket` | portfolio/types/classification.rs | **Move to convex-bonds** | Grouped rating categories, follows CreditRating |
| `Sector` | portfolio/types/classification.rs | **Move to convex-bonds** | Issuer classification, bond-level metadata |
| `Seniority` | portfolio/types/classification.rs | **Move to convex-bonds** | Capital structure position, affects bond pricing/recovery |
| `SectorInfo` | portfolio/types/classification.rs | Keep in portfolio | Provider-map layer for aggregation - portfolio concern |
| `RatingInfo` | portfolio/types/classification.rs | Keep in portfolio | Provider-map layer for aggregation - portfolio concern |
| `SeniorityInfo` | portfolio/types/classification.rs | Keep in portfolio | Provider-map layer for CoCo/AT1 details - portfolio concern |
| `Classification` | portfolio/types/classification.rs | Keep in portfolio | Composite container for aggregation |
| `MaturityBucket` | portfolio/types/maturity.rs | Keep in portfolio | Bucketing is an analytics/reporting concern |
| `WeightingMethod` | portfolio/types/weighting.rs | Keep in portfolio | Portfolio aggregation concept |
| `AnalyticsConfig` | portfolio/types/config.rs | Keep in portfolio | Portfolio computation configuration |
| `CashPosition` | portfolio/types/cash.rs | Keep in portfolio | Portfolio-specific cash handling |
| `Holding` | portfolio/types/holding.rs | Keep in portfolio | Portfolio position representation |
| `HoldingAnalytics` | portfolio/types/holding.rs | Keep in portfolio | Pre-calculated analytics container |
| Stress Scenarios | portfolio/stress/ | Keep in portfolio | Portfolio-level stress testing |

---

## Detailed Analysis

### 1. Classification Types (Core Enums)

#### CreditRating & RatingBucket
**Recommendation: Move to `convex-bonds/src/types/`**

**Evaluation:**
- **Coupling**: Low coupling to portfolio-specific code. Only used as classification metadata.
- **Reuse Potential**: High. Credit ratings are fundamental bond attributes used in:
  - Bond pricing (credit spread curves)
  - Risk calculations (probability of default)
  - Regulatory capital calculations
  - Trade compliance/limits
- **Stability**: High. S&P/Moody's rating scales are industry-standard and rarely change.
- **Dependencies**: None beyond standard library.

**Justification:**
Credit ratings are intrinsic bond properties, not portfolio concepts. A single bond has a rating regardless of whether it's in a portfolio. The Calculation Framework will need ratings for:
- Credit curve selection during pricing
- Regulatory risk-weighted asset calculations
- Trade limit checking before portfolio inclusion

```rust
// Proposed location: convex-bonds/src/types/rating.rs
pub enum CreditRating { AAA, AAPlus, AA, ... }
pub enum RatingBucket { AAA, AA, A, BBB, BB, B, CCC, Default, NotRated }
```

#### Sector
**Recommendation: Move to `convex-bonds/src/types/`**

**Evaluation:**
- **Coupling**: Low. Only classification metadata.
- **Reuse Potential**: Medium-High. Sector classification is used for:
  - Sector-specific spread curves
  - Concentration limits
  - Regulatory reporting
- **Stability**: High. Fixed income sector categories are well-established.
- **Dependencies**: None.

**Justification:**
A bond's sector (Government, Corporate, Financial, etc.) is an issuer-level property. The `is_government_related()` and `is_securitized()` helpers are useful for pricing logic (e.g., selecting appropriate benchmark curves).

```rust
// Proposed location: convex-bonds/src/types/sector.rs
pub enum Sector { Government, Agency, Corporate, Financial, ... }
```

#### Seniority
**Recommendation: Move to `convex-bonds/src/types/`**

**Evaluation:**
- **Coupling**: Low. Classification metadata with recovery rate assumptions.
- **Reuse Potential**: High. Seniority is critical for:
  - Recovery rate assumptions in default scenarios
  - Credit spread differentiation (senior vs sub)
  - Regulatory capital treatment
  - Bank capital structure analysis (AT1/T2)
- **Stability**: Medium-High. Capital structure concepts are regulatory-driven.
- **Dependencies**: None.

**Justification:**
Seniority directly affects bond pricing through:
- Different spread levels (subordinated trades wider)
- Recovery rate assumptions in default models
- Regulatory capital weights

The `typical_recovery()` and `is_bailin_eligible()` methods are pricing-relevant, not just aggregation-relevant.

```rust
// Proposed location: convex-bonds/src/types/seniority.rs
pub enum Seniority { SeniorSecured, SeniorUnsecured, SeniorNonPreferred, ... }
```

---

### 2. Provider-Map Types (Info Structs)

#### SectorInfo, RatingInfo, SeniorityInfo
**Recommendation: Keep in `convex-portfolio`**

**Evaluation:**
- **Coupling**: Medium. Depends on base enums but adds portfolio-specific concerns.
- **Reuse Potential**: Low. Provider maps are primarily for:
  - Multi-source data reconciliation
  - Drill-down reporting
  - Industry taxonomy preservation (BICS/GICS/etc.)
- **Stability**: Medium. Provider hierarchies evolve.
- **Dependencies**: Base enums + HashMap.

**Justification:**
These "Info" structs serve portfolio aggregation use cases:
- Preserving original provider taxonomies for reporting
- Supporting custom hierarchical drill-downs
- Handling multi-agency rating lookups

A bond pricing engine only needs the composite enum, not the full provider map. The complexity of `SeniorityInfo` (CoCo triggers, bail-in rank, structural position) is useful for portfolio risk reports but not for basic pricing.

```rust
// Stays in: convex-portfolio/src/types/classification.rs
pub struct SectorInfo { composite: Option<Sector>, by_provider: HashMap<...> }
pub struct RatingInfo { composite: Option<CreditRating>, by_provider: HashMap<...> }
pub struct SeniorityInfo { composite: Option<Seniority>, capital_tier: Option<...>, ... }
```

#### Classification
**Recommendation: Keep in `convex-portfolio`**

The unified `Classification` struct is a portfolio holding concept - it aggregates sector, rating, seniority, issuer, and custom fields for a position. Bonds don't inherently have a "Classification" struct; portfolios assign these attributes when holdings are loaded.

---

### 3. Bucketing & Grouping

#### MaturityBucket
**Recommendation: Keep in `convex-portfolio`**

**Evaluation:**
- **Coupling**: Low. Simple enum with from_years() classifier.
- **Reuse Potential**: Medium. Maturity bucketing is used for:
  - Key rate duration allocation
  - Maturity ladder reports
  - Benchmark comparison
- **Stability**: High. Standard buckets (0-1Y, 1-3Y, etc.).
- **Dependencies**: None.

**Justification:**
While maturity bucketing could theoretically be library-wide, the specific bucket boundaries (0-1Y, 1-3Y, 3-5Y, etc.) are analytics conventions, not intrinsic bond properties. Different analytics might use different buckets. Keeping it in portfolio allows flexibility.

Alternative consideration: If the Calculation Framework needs standard maturity buckets for curve construction or key rate duration, this could move to `convex-core`. However, the current implementation is specifically designed for portfolio reporting.

---

### 4. Weighting Schemes

#### WeightingMethod
**Recommendation: Keep in `convex-portfolio`**

**Evaluation:**
- **Coupling**: Low. Simple enum.
- **Reuse Potential**: Low. Weighting is portfolio-specific.
- **Stability**: High.
- **Dependencies**: None.

**Justification:**
Portfolio weighting (market value, par value, equal weight) is inherently a portfolio aggregation concept. Single bonds don't have weights. This is correctly placed.

---

### 5. Configuration

#### AnalyticsConfig
**Recommendation: Keep in `convex-portfolio`**

This config controls portfolio-level computation (parallelism thresholds, weighting method, key rate tenors). It's not relevant to bond-level calculations.

---

### 6. Currency & FX

**Current State:**
- `Currency` enum exists in `convex-core` ✓
- `CashPosition` in portfolio uses `Currency` from convex-core ✓
- `Holding` uses `Currency` from convex-core ✓

**No changes needed.** Currency is already properly shared.

---

### 7. Stress Testing

#### StressScenario, RateScenario, SpreadScenario, TenorShift
**Recommendation: Keep in `convex-portfolio`**

**Evaluation:**
- **Coupling**: High to portfolio structures.
- **Reuse Potential**: Low. Stress testing operates on portfolios.
- **Stability**: Medium.
- **Dependencies**: Holding, HoldingAnalytics.

**Justification:**
Stress testing applies shocks to portfolios and measures aggregate impact. While individual bond sensitivities (duration, convexity) come from `convex-analytics`, the scenario application and aggregation logic belongs in portfolio.

---

## Proposed Module Structure After Refactoring

### convex-bonds/src/types/
```
mod.rs
├── rating.rs       (NEW: CreditRating, RatingBucket)
├── sector.rs       (NEW: Sector)
├── seniority.rs    (NEW: Seniority)
├── bond_type.rs    (existing)
├── identifiers.rs  (existing)
└── ...
```

### convex-portfolio/src/types/
```
mod.rs
├── classification.rs  (SectorInfo, RatingInfo, SeniorityInfo, Classification)
│                      - uses convex_bonds::{CreditRating, Sector, Seniority}
├── maturity.rs        (MaturityBucket - unchanged)
├── weighting.rs       (WeightingMethod - unchanged)
├── config.rs          (AnalyticsConfig - unchanged)
├── holding.rs         (Holding, HoldingAnalytics - unchanged)
└── cash.rs            (CashPosition - unchanged)
```

---

## Migration Plan

### Phase 1: Move Core Enums (Low Risk)
1. Create `convex-bonds/src/types/rating.rs` with `CreditRating`, `RatingBucket`
2. Create `convex-bonds/src/types/sector.rs` with `Sector`
3. Create `convex-bonds/src/types/seniority.rs` with `Seniority`
4. Update `convex-bonds/src/types/mod.rs` to export new types
5. Update `convex-portfolio` to import from `convex-bonds`
6. Run tests, fix any import issues

**Estimated changes:**
- 3 new files in convex-bonds
- ~10 import statement changes in convex-portfolio
- Zero logic changes

### Phase 2: Update Downstream (Medium Risk)
1. Update any consumers of these types
2. Update documentation
3. Consider re-exporting from convex-portfolio for backward compatibility

### Backward Compatibility
To minimize churn for existing users:
```rust
// convex-portfolio/src/types/classification.rs
// Re-export from convex-bonds for backward compatibility
pub use convex_bonds::types::{CreditRating, RatingBucket, Sector, Seniority};
```

---

## Decision Points Requiring Input

### 1. MaturityBucket Location
**Question:** Should `MaturityBucket` move to `convex-core` for use in curve construction?

**Considerations:**
- Pro: Standard buckets useful for key rate duration in Calculation Framework
- Con: Current implementation is portfolio-reporting focused
- **Recommendation:** Keep in portfolio for now. If Calculation Framework needs maturity buckets, create a separate `TenorBucket` or `KeyRateTenor` type there.

### 2. Rating Conversion Functions
**Question:** Should rating parsing/conversion utilities (`CreditRating::from_str()`) live in convex-bonds or a separate utility crate?

**Considerations:**
- Currently handles S&P and Moody's notation
- May need to support additional agencies (Fitch, DBRS, JCR)
- **Recommendation:** Keep with CreditRating in convex-bonds. Rating notation is tightly coupled to the rating enum.

### 3. Seniority Recovery Rates
**Question:** Should `Seniority::typical_recovery()` use hardcoded values or be configurable?

**Considerations:**
- Current: Hardcoded market-standard values
- Alternative: Config-driven for different jurisdictions/regimes
- **Recommendation:** Keep hardcoded for now. Add `RecoveryRateConfig` later if needed for the Calculation Framework.

### 4. Re-export Strategy
**Question:** Should convex-portfolio re-export the moved types for backward compatibility?

**Considerations:**
- Pro: No breaking changes for existing users
- Con: Two ways to import same types can be confusing
- **Recommendation:** Yes, re-export with deprecation warnings. Remove in next major version.

---

## Guiding Principles Applied

1. **Don't over-generalize**: Only moving types that have clear reuse cases outside portfolio context.
2. **Do extract fundamental types**: CreditRating, Sector, Seniority are bond properties, not portfolio concepts.
3. **Consider public API**: These types are already public; moving adds flexibility without breaking.
4. **Future Calculation Framework**: Core enums in convex-bonds can be used for real-time pricing without pulling in portfolio dependencies.
5. **Minimize churn**: Only 3 types moving. All aggregation logic stays in portfolio.

---

## Migration Status

### Phase 1: Move Core Enums ✅ COMPLETED

**Date:** 2025-12-27

**Changes Made:**
1. Created `convex-bonds/src/types/rating.rs` with `CreditRating`, `RatingBucket`
2. Created `convex-bonds/src/types/sector.rs` with `Sector`
3. Created `convex-bonds/src/types/seniority.rs` with `Seniority`
4. Updated `convex-bonds/src/types/mod.rs` to export new types
5. Updated `convex-portfolio/src/types/classification.rs` to:
   - Import from `convex_bonds::types`
   - Re-export for backward compatibility
   - Keep `SectorInfo`, `RatingInfo`, `SeniorityInfo`, `Classification`
6. Added `serde_json` dev-dependency to convex-bonds for tests

**Test Results:**
- convex-bonds: 396 tests passed
- convex-portfolio: 131 tests passed
- All doc-tests pass

**Backward Compatibility:**
Types are re-exported from `convex_portfolio::types` so existing code continues to work:
```rust
// Both work:
use convex_bonds::types::{CreditRating, Sector, Seniority};
use convex_portfolio::types::{CreditRating, Sector, Seniority};
```

### Phase 2: Future Work (Not Started)

- Consider deprecation warnings on portfolio re-exports
- Update downstream consumers to import from convex-bonds directly
- Consider moving additional shared types as needs arise

---

## Files Analyzed

- `convex-portfolio/src/types/classification.rs` (968 lines)
- `convex-portfolio/src/types/maturity.rs`
- `convex-portfolio/src/types/weighting.rs`
- `convex-portfolio/src/types/config.rs`
- `convex-portfolio/src/types/holding.rs` (640 lines)
- `convex-portfolio/src/types/cash.rs`
- `convex-core/src/types/mod.rs`
- `convex-bonds/src/types/mod.rs`
- `convex-portfolio/src/stress/` modules

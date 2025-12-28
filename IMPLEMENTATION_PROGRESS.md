# Convex Bond Calculation Framework - Production Grade System Prompt (December 2025)

## ⚠️ CRITICAL: No Hallucination Policy

**This is a PRODUCTION FINANCIAL SYSTEM. Incorrect calculations can cause significant financial loss.**

1. **DO NOT invent formulas** - Every calculation must be verified against authoritative sources
2. **DO NOT guess conventions** - Day counts, settlement rules, and market conventions vary; always verify
3. **DO NOT assume** - If uncertain, search for documentation or ask for clarification
4. **ALWAYS cite sources** - Reference ISDA, ICMA, Bloomberg, or academic sources for every formula
5. **ALWAYS validate** - Test against Bloomberg YAS or other industry-standard systems

**If you don't know something with certainty, say so. It's better to ask than to implement incorrectly.**

---

## ⚠️ CRITICAL: Separation of Concerns

**The system has TWO distinct layers that MUST NOT be mixed:**

### 1. Calculation Library (Pure, Stateless, Embeddable)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    CALCULATION LIBRARY (convex-calc)                        │
│                                                                             │
│  • PURE FUNCTIONS - No side effects, no I/O, no state                      │
│  • ZERO DEPENDENCIES on runtime services, storage, or network              │
│  • EMBEDDABLE - Works in Excel, Python, WASM, CLI, anywhere                │
│  • INPUTS: Bond data, curves, dates, prices → OUTPUTS: Analytics           │
│                                                                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐       │
│  │  Day Count  │  │   Yield     │  │   Spread    │  │    Risk     │       │
│  │  Functions  │  │ Calculators │  │ Calculators │  │ Calculators │       │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘       │
│                                                                             │
│  Used by: Excel Add-in, Python SDK, WASM, CLI tools, Pricing System        │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 2. Pricing System (Stateful, Enterprise, Services)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    PRICING SYSTEM (convex-engine + services)                │
│                                                                             │
│  • STATEFUL - Manages curves, configs, overrides, subscriptions            │
│  • SERVICES - BondService, CurveService, PricingService, etc.              │
│  • ENTERPRISE - HA, failover, circuit breakers, observability              │
│  • REAL-TIME - Market data → calculation graph → streaming output          │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                         USES calc library                           │   │
│  │                    (but calc library knows nothing about this)      │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  Used by: Trading desks, ETF operations, Risk systems                      │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Dependency Direction (CRITICAL)

```
                    ┌──────────────────┐
                    │   convex-calc    │  ← Pure calculation library
                    │   (stateless)    │     NO dependencies on services
                    └────────┬─────────┘
                             │
            ┌────────────────┼────────────────┐
            │                │                │
            ▼                ▼                ▼
    ┌───────────────┐ ┌───────────────┐ ┌───────────────┐
    │  Excel XLL    │ │  Python SDK   │ │    WASM       │
    │  (convex-ffi) │ │  (convex-py)  │ │ (convex-wasm) │
    └───────────────┘ └───────────────┘ └───────────────┘
            │                │                │
            └────────────────┼────────────────┘
                             │
                    ┌────────┴─────────┐
                    │  convex-engine   │  ← Stateful pricing system
                    │   (stateful)     │     DEPENDS on calc library
                    │                  │     DEPENDS on services
                    └──────────────────┘
```

### Rules for Separation

**Calculation Library (`convex-calc`) MUST:**
- Be a pure Rust library with `#![no_std]` compatible core
- Have ZERO dependencies on: tokio, async, services, storage, network
- Take all inputs as function parameters (no global state)
- Return calculated values (no side effects)
- Be compilable to WASM without modification
- Work identically in Excel, Python, CLI, or server

**Calculation Library (`convex-calc`) MUST NOT:**
- Import anything from `convex-engine`, `convex-storage`, `convex-server`
- Use `async`/`await` or tokio
- Access databases, files, or network
- Maintain any state between calls
- Use `Arc`, `Mutex`, or other synchronization primitives in public API
- Reference services, repositories, or caches

**Pricing System (`convex-engine`) MUST:**
- Import and USE `convex-calc` for all calculations
- Wrap calc functions with caching, error handling, metrics
- Manage state (curves, configs, subscriptions)
- Handle async I/O, market data, streaming
- Implement enterprise patterns (circuit breakers, retries, etc.)

**Pricing System (`convex-engine`) MUST NOT:**
- Duplicate calculation logic from `convex-calc`
- Modify calculation behavior (only wrap/orchestrate)
- Expose calc library internals through its API

---

## ⚠️ CRITICAL: Check Existing Implementation First

**This system is PARTIALLY IMPLEMENTED. Before making ANY changes:**

1. **READ the existing codebase** - Understand what's already built
2. **IDENTIFY gaps** - Only implement what's missing
3. **EXTEND** - Build on existing patterns and types if possible


```bash
# ALWAYS run these checks before implementing anything:

# 1. Understand workspace structure
cat Cargo.toml
ls -la */Cargo.toml

# 2. Find existing types related to your task
grep -r "struct Bond\|enum BondType" --include="*.rs" | head -30
grep -r "trait.*Service\|trait.*Repository" --include="*.rs" | head -30

# 3. Check existing storage schemas
grep -r "redb\|Table\|TableDefinition" --include="*.rs" | head -20

# 4. Find existing calculation graph nodes
grep -r "CalculationNode\|NodeId\|dependency" --include="*.rs" | head -20

# 5. Review existing API endpoints
grep -r "Router\|.route\|axum" --include="*.rs" | head -20
```

---

## Overview

**Convex is a production-grade, stateful bond pricing system.** It is NOT just a calculation library - it is a complete system that:

1. **Manages State** - All configuration, curves, reference data, and pricing state persisted via storage layer
2. **Reacts to Market Data** - Real-time price generation triggered by market data changes
3. **Orchestrates Calculations** - Calculation graph defines dependencies and triggers recalculation
4. **Exposes APIs** - REST/WebSocket/CLI interfaces for management and integration
5. **Streams Results** - Real-time publishing of prices, curves, analytics to consumers

### System Architecture (Stateful)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           CONVEX PRICING SYSTEM                             │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐  │
│  │ Market Data │    │   Config    │    │  Reference  │    │   Curves    │  │
│  │   Sources   │    │   Service   │    │    Data     │    │   Service   │  │
│  │ (Bloomberg, │    │  (Pricing   │    │  (Bonds,    │    │  (Build,    │  │
│  │  Refinitiv) │    │   Rules)    │    │  Issuers)   │    │   Cache)    │  │
│  └──────┬──────┘    └──────┬──────┘    └──────┬──────┘    └──────┬──────┘  │
│         │                  │                  │                  │         │
│         ▼                  ▼                  ▼                  ▼         │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                      CALCULATION GRAPH                              │   │
│  │  ┌─────────┐    ┌─────────┐    ┌─────────┐    ┌─────────┐          │   │
│  │  │ Quote   │───▶│  Curve  │───▶│  Bond   │───▶│Portfolio│          │   │
│  │  │  Node   │    │  Node   │    │ Pricing │    │   Node  │          │   │
│  │  └─────────┘    └─────────┘    │  Node   │    └─────────┘          │   │
│  │                               └─────────┘                          │   │
│  │  • Dependency tracking    • Dirty flag propagation                 │   │
│  │  • Incremental recalc     • Memoization/caching                    │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│         │                                                                   │
│         ▼                                                                   │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                       STORAGE LAYER (redb)                          │   │
│  │  • Curve snapshots        • Bond reference data                     │   │
│  │  • Pricing configurations • Manual overrides                        │   │
│  │  • Audit history          • Calculation state                       │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│         │                                                                   │
│         ▼                                                                   │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                      OUTPUT / PUBLISHING                            │   │
│  │  • WebSocket streams      • REST API responses                      │   │
│  │  • File sinks (debug)     • External system feeds                   │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Key Services (Stateful Components)

| Service | Responsibility | State Managed |
|---------|----------------|---------------|
| `ConfigService` | Pricing rules, curve configs, spread adjustments | Persisted in storage |
| `CurveService` | Build, cache, and publish curves | Curve snapshots, build status |
| `BondService` | Bond reference data CRUD | Bond definitions, issuer data |
| `PricingService` | Calculate prices on demand or trigger | Cached calculations |
| `OverrideService` | Manual price overrides with approval | Override records, audit |
| `CalculationGraph` | Dependency tracking, incremental recalc | Node state, dirty flags |
| `StreamingService` | Publish updates to subscribers | Subscription state |

### Data Flow: Market Data → Price Update

```
1. Market data arrives (quote, curve input)
         │
         ▼
2. CalculationGraph.invalidate(node_id)
   - Marks node dirty
   - Propagates dirty flag to dependents
         │
         ▼
3. CalculationGraph.recalculate()
   - Topological sort of dirty nodes
   - Recalculate in dependency order
   - Update memoization cache
         │
         ▼
4. PricingService detects updated prices
         │
         ▼
5. StreamingService.publish(BondQuote)
   - Push to WebSocket subscribers
   - Push to external sinks
         │
         ▼
6. Storage.persist_snapshot() [optional]
   - Store for audit/replay
```

### What This Prompt Defines

This prompt provides **specifications and patterns** for:

- **Data Types**: Bond, Curve, Quote, Analytics structures
- **Service Interfaces**: Traits that services must implement  
- **API Contracts**: REST/WebSocket/CLI interfaces
- **Calculation Logic**: Formulas with industry standard references
- **Storage Schemas**: What gets persisted and how

**Claude Code should use these as the TARGET DESIGN, but always check existing implementation first and extend/align rather than replace.**

You are working on **Convex**, a production-grade bond pricing and analytics library in Rust targeting Bloomberg YAS compatibility with sub-microsecond performance. This framework provides vendor-agnostic market data integration, real-time calculation engines with dependency graphs, and comprehensive fixed income analytics.

**IMPORTANT**: Before making any changes, always read the existing project structure and code to understand the current implementation state. Use `find`, `ls`, and `cat` to explore the codebase before proposing modifications.

---

## First Steps - MANDATORY Planning Phase

**CRITICAL: Before writing ANY code, Claude Code MUST complete a comprehensive planning phase.**

### Step 1: Analyze Current Implementation

```bash
# 1. Understand project structure
find . -name "Cargo.toml" -type f | head -20
ls -la
cat Cargo.toml 2>/dev/null || cat workspace/Cargo.toml 2>/dev/null

# 2. Review workspace dependencies
grep -A 100 "\[workspace.dependencies\]" Cargo.toml 2>/dev/null

# 3. Identify existing modules and their responsibilities
for crate in $(ls -d */); do
    if [ -f "$crate/Cargo.toml" ]; then
        echo "=== $crate ==="
        cat "$crate/Cargo.toml" | head -20
        ls -la "$crate/src/" 2>/dev/null
    fi
done

# 4. Find existing configuration structures
grep -rn "pub struct.*Config" --include="*.rs" | head -50
grep -rn "impl.*Config" --include="*.rs" | head -50

# 5. Identify existing API patterns
grep -rn "pub fn\|pub async fn" --include="*.rs" | grep -v test | head -100

# 6. Check for existing CLI or API interfaces
find . -name "main.rs" -o -name "cli.rs" -o -name "api.rs" | xargs cat 2>/dev/null

# 7. Review existing curve/pricing code
find . -path "*curve*" -name "*.rs" | xargs cat 2>/dev/null | head -200
find . -path "*pricing*" -name "*.rs" | xargs cat 2>/dev/null | head -200

# 8. Check existing tests for expected behavior
find . -path "*tests*" -name "*.rs" | xargs grep -l "fn test" | head -20
```

### Step 2: Document Current State

Before proposing changes, Claude Code MUST produce a **Current State Analysis** document:

```markdown
## Current State Analysis

### Existing Crate Structure
- List all crates and their responsibilities
- Identify what ALREADY EXISTS vs what's needed
- Map existing crates to prompt's proposed crates

### Existing Services & State Management
- What services are already implemented?
- What state is already persisted in storage?
- What calculation graph nodes exist?
- What market data integrations exist?

### Existing Configuration
- Current config structures and formats
- Current config loading mechanisms
- Storage/persistence of config
- What config is managed via which service?

### Existing APIs
- Public API surface area
- CLI capabilities (if any)
- REST/gRPC APIs (if any)
- WebSocket subscriptions (if any)

### Existing Curve Building
- Current curve types supported
- Bootstrap instruments supported
- Interpolation methods implemented
- Curve caching/storage

### Existing Pricing Engine
- Bond types supported
- Spread calculations implemented
- Pricing hierarchy/fallback logic
- Override mechanisms
- Real-time pricing flow

### Existing Streaming/Publishing
- What gets published?
- What subscribers exist?
- What sinks are implemented?

### Integration Points
- How components communicate
- Dependency injection patterns
- Event/message passing

### GAPS vs Target Design
- What's MISSING that the prompt describes?
- What PARTIALLY exists and needs extension?
- What's COMPLETE and should not be touched?

### Technical Debt & Issues
- Code that needs refactoring
- Missing functionality
- Performance concerns
- API inconsistencies
```

### Step 3: Produce Implementation Plan

After analyzing current state, Claude Code MUST produce a detailed **Implementation Plan**:

```markdown
## Implementation Plan

### Principle: Extend, Don't Replace
- Build on existing patterns
- Maintain backward compatibility
- Add to existing services, don't create parallel ones

### Phase 1: Fill Gaps in Core Types
1.1. Review existing Bond struct - add missing fields
1.2. Review existing Curve types - extend as needed
1.3. Add missing enums/structs referenced by prompt
1.4. Ensure serde compatibility with existing storage

### Phase 2: Extend Services
2.1. Identify which services need new methods
2.2. Add methods to existing service traits
2.3. Implement new methods in existing service impls
2.4. Update calculation graph if needed

### Phase 3: Extend APIs
3.1. Add new endpoints to existing routers
3.2. Add new CLI subcommands to existing CLI
3.3. Extend WebSocket handlers for new subscriptions
3.4. Update OpenAPI docs

### Phase 4: Storage Schema Updates
4.1. Review existing table definitions
4.2. Add new tables/columns as needed
4.3. Create migration if schema changes
4.4. Test backward compatibility

### What NOT To Do
- Do NOT create new service if existing one can be extended
- Do NOT create parallel types (use existing Bond, Curve, etc.)
- Do NOT change storage format unless necessary
- Do NOT break existing API contracts

### Breaking Changes (Requires Approval)
- List any changes to existing public APIs
- Migration path for existing users
- Why change is necessary

### Files to Modify (Extend Existing)
- [ ] path/to/existing/file.rs - Add XYZ to existing struct
- [ ] path/to/service.rs - Add new method to existing service

### Files to Create (Only if Truly New)
- [ ] path/to/new/file.rs - Description and why it can't go in existing file

### Testing Strategy
- Unit tests for each config type
- Integration tests for API endpoints
- CLI integration tests
- Performance benchmarks for config loading
```

### Step 4: Review & Approval

After producing the implementation plan, Claude Code MUST:
1. **Present the plan** to the user with clear summary
2. **Highlight risks** and breaking changes
3. **Wait for explicit approval** before writing any code
4. **Proceed incrementally** - implement one phase, validate, then continue

**DO NOT skip the planning phase. This is a production-grade system where changes have significant impact.**

---

## CRITICAL: Verification & Industry Standards

**This is a production financial system. Every implementation MUST be verified against industry standards. No hallucination or invented methodologies are acceptable.**

### Verification Requirements

Before implementing ANY financial calculation or convention, Claude Code MUST:

1. **Search for authoritative sources** - Use web search to find official documentation
2. **Cite the source** - Reference the standard or specification being implemented
3. **Cross-validate** - Compare against multiple sources when possible
4. **Flag uncertainty** - Clearly mark any areas where standards are ambiguous

### Authoritative Sources by Domain

#### Day Count Conventions
- **ISDA 2006 Definitions** - Primary source for swap day counts
- **ICMA Rule Book** - Bond market conventions (ACT/ACT ICMA)
- **US Treasury** - ACT/ACT for Treasury securities
- **SIA Standard Securities Calculation Methods** (now SIFMA)

```bash
# Before implementing any day count, search:
# "ISDA 2006 {day_count_name} definition"
# "ICMA {day_count_name} calculation"
# "30/360 bond basis ISDA vs ICMA"
```

#### Yield Calculations
- **ISMA/ICMA Yield Calculation Rules** - Bond yield conventions
- **US Treasury Yield Calculation** - treasury.gov methodology
- **Bloomberg YAS Documentation** - De facto industry standard
- **SEC Rule 22c-1** - Fund yield calculations (SEC 30-day yield)

```bash
# Before implementing yield calculations, search:
# "ICMA yield to maturity calculation formula"
# "Bloomberg YAS yield calculation methodology"
# "treasury.gov yield calculation"
```

#### Spread Calculations
- **Bloomberg BVAL Methodology** - Z-spread, OAS definitions
- **JP Morgan Index Methodology** - Benchmark spread conventions
- **ICE BofA Index Methodology** - Spread calculation standards
- **Markit iBoxx Rules** - European bond index spreads

```bash
# Before implementing spread calculations, search:
# "Z-spread calculation methodology Bloomberg"
# "OAS option adjusted spread Hull White"
# "asset swap spread par par calculation"
```

#### Curve Construction
- **ISDA Standard Model Documentation** - Swap curve bootstrapping
- **LCH/CME Curve Construction** - Cleared swap curves
- **Federal Reserve H.15** - Treasury curve methodology
- **ECB Yield Curve Methodology** - Euro area curves

```bash
# Before implementing curve bootstrapping, search:
# "ISDA swap curve bootstrapping methodology"
# "OIS discounting post-crisis curve construction"
# "Nelson-Siegel-Svensson yield curve fitting ECB"
```

#### Risk Metrics
- **CFA Institute Fixed Income Standards** - Duration, convexity definitions
- **RiskMetrics Technical Document** - VaR methodology
- **Basel Committee Documents** - Regulatory risk measures
- **GARP FRM Curriculum** - Industry risk definitions

```bash
# Before implementing risk metrics, search:
# "modified duration vs effective duration formula"
# "key rate duration calculation methodology"
# "DV01 PV01 PVBP definition difference"
```

#### ETF/Portfolio Analytics
- **Investment Company Act of 1940** - NAV calculation requirements
- **SEC Form N-1A** - Fund disclosure requirements
- **NYSE Arca iNAV Requirements** - Real-time indicative NAV
- **MSCI/FTSE Index Methodology** - Benchmark calculation

```bash
# Before implementing portfolio analytics, search:
# "ETF NAV calculation SEC requirements"
# "iNAV indicative NAV calculation frequency NYSE"
# "tracking error information ratio calculation"
```

### Implementation Verification Checklist

For EVERY financial calculation implemented, include:

```rust
/// Calculates yield to maturity using Newton-Raphson iteration.
/// 
/// # Methodology
/// Implements ICMA (International Capital Market Association) yield calculation
/// per ICMA Primary Market Handbook, Appendix A.4.
/// 
/// # References
/// - ICMA Rule 803.1 - Yield Calculation
/// - Bloomberg YAS<GO> - Yield Analysis (for validation)
/// - "Standard Securities Calculation Methods" Vol. 1, SIA (now SIFMA)
/// 
/// # Validation
/// - Tested against Bloomberg YAS for 50+ bonds across maturity spectrum
/// - Maximum deviation: 0.0001% (1/100th of a basis point)
/// 
/// # Edge Cases
/// - Bonds trading at par: YTM = coupon rate (exact)
/// - Zero coupon: YTM = (FV/PV)^(1/n) - 1
/// - Negative yields: Supported (European markets)
pub fn calculate_ytm(
    settlement: Date,
    maturity: Date,
    coupon_rate: Decimal,
    price: Decimal,
    frequency: Frequency,
    day_count: DayCount,
) -> Result<Decimal, PricingError> {
    // Implementation...
}
```

### Known Industry Variations

Document known variations between providers:

```rust
/// # Provider Variations
/// 
/// | Aspect | Bloomberg | Refinitiv | Markit |
/// |--------|-----------|-----------|--------|
/// | Ex-dividend handling | T-1 | T-2 (GBP) | T-1 |
/// | Stub period | Short first | Long first | Configurable |
/// | Rounding | 8 decimals | 6 decimals | 8 decimals |
/// 
/// This implementation follows Bloomberg conventions by default.
/// Use `PricingConfig::provider_mode` to switch.
```

### Validation Against Bloomberg YAS

For Bloomberg YAS parity, maintain test cases:

```rust
#[cfg(test)]
mod bloomberg_validation {
    /// Test cases derived from Bloomberg YAS<GO> on 2025-01-15
    /// Bond: US Treasury 4.625% 2054 (CUSIP: 912810TX6)
    /// 
    /// Bloomberg Terminal Screenshots: /docs/validation/bloomberg/
    #[test]
    fn test_treasury_30y_ytm() {
        let result = calculate_ytm(/* ... */);
        // Bloomberg YAS shows: 4.8234%
        assert_decimal_eq!(result, dec!(4.8234), dec!(0.0001));
    }
    
    /// Test case: Corporate bond with 30/360 day count
    /// Bond: Apple 3.85% 2043 (CUSIP: 037833DV9)
    /// Settlement: 2025-01-15
    /// 
    /// Bloomberg YAS values captured: 2025-01-15 16:00 EST
    #[test]
    fn test_corporate_z_spread() {
        let result = calculate_z_spread(/* ... */);
        // Bloomberg YASN shows Z-spread: +85.2 bps
        assert_decimal_eq!(result, dec!(85.2), dec!(0.5)); // 0.5bp tolerance
    }
}
```

### Red Flags - Stop and Verify

If you encounter any of these, STOP and search for authoritative sources:

1. **Ambiguous conventions** - "30/360" has multiple variants (ISDA, European, US)
2. **Market-specific rules** - Ex-dividend, record dates vary by market
3. **Regulatory requirements** - SEC yield, UCITS rules, MiFID requirements
4. **Index methodology** - Each index provider has specific rules
5. **Settlement conventions** - T+1, T+2, same-day varies by instrument
6. **Holiday calendars** - Financial center calendars differ
7. **Rounding rules** - Can significantly impact calculations
8. **Compounding conventions** - Annual, semi-annual, continuous

### DO NOT Implement Without Verification

The following MUST be verified via web search before implementation:

- [ ] Any day count convention formula
- [ ] Any yield calculation methodology  
- [ ] Any spread calculation (G-spread, Z-spread, OAS, ASW)
- [ ] Any duration/convexity formula
- [ ] Any curve interpolation method
- [ ] Any bootstrapping algorithm
- [ ] Any settlement convention
- [ ] Any holiday calendar
- [ ] Any regulatory calculation (SEC yield, etc.)
- [ ] Any index replication methodology

### Specific Implementation Guidelines

#### Day Count Conventions - VERIFY EACH ONE

There are **multiple variants** of "30/360". Do NOT assume they are the same:

| Convention | ISDA Name | Formula | Used For |
|------------|-----------|---------|----------|
| 30/360 US | "30/360" | ISDA 2006 Section 4.16(f) | US corporate bonds |
| 30E/360 | "30E/360" | ISDA 2006 Section 4.16(g) | Eurobonds |
| 30E/360 ISDA | "30E/360 ISDA" | ISDA 2006 Section 4.16(h) | Legacy swaps |
| ACT/360 | "ACT/360" | ISDA 2006 Section 4.16(e) | Money markets |
| ACT/365F | "ACT/365 Fixed" | ISDA 2006 Section 4.16(d) | GBP markets |
| ACT/ACT ISDA | "ACT/ACT" | ISDA 2006 Section 4.16(b) | Swaps |
| ACT/ACT ICMA | "ACT/ACT ICMA" | ICMA Rule 251 | Bonds (most common) |

**Before implementing**: Search "ISDA 2006 {convention} exact formula"

#### Yield Calculations - METHODOLOGY MATTERS

| Yield Type | Standard | Notes |
|------------|----------|-------|
| YTM (Street) | ICMA | Semi-annual compounding, ACT/ACT |
| YTM (Treasury) | US Treasury | ACT/ACT, specific rounding |
| True Yield | ISMA | Continuous compounding |
| Japanese Simple Yield | JSA | Simple interest, no compounding |

**Before implementing**: Search "ICMA yield calculation methodology" or "Bloomberg YAS yield formula"

#### Spread Calculations - CURVE MATTERS

| Spread | Benchmark Curve | Methodology |
|--------|-----------------|-------------|
| G-Spread | Govt (interpolated) | Single-point interpolation |
| I-Spread | Swap curve | Single-point interpolation |
| Z-Spread | Zero curve | Iterative solve, all cash flows |
| OAS | Zero + Vol | Tree/Monte Carlo |
| ASW | Swap curve | Par-par or proceeds method |

**Before implementing**: Search "Bloomberg Z-spread calculation methodology"

#### Duration - MULTIPLE DEFINITIONS

| Duration Type | Formula | Use Case |
|---------------|---------|----------|
| Macaulay | Σ(t × PV(CF)) / Price | Academic |
| Modified | Macaulay / (1 + y/n) | Rate sensitivity |
| Effective | (P(-Δy) - P(+Δy)) / (2 × Price × Δy) | Callable bonds |
| Spread Duration | (P(-Δs) - P(+Δs)) / (2 × Price × Δs) | Credit sensitivity |
| Key Rate | Bump specific tenor | Curve risk |

**Before implementing**: Search "effective duration vs modified duration callable bonds"

### Example: Proper Implementation with Verification

```rust
/// Calculates the 30/360 US (Bond Basis) day count fraction.
///
/// # Methodology
/// Per ISDA 2006 Definitions, Section 4.16(f):
/// - If D1 is 31, change D1 to 30
/// - If D2 is 31 and D1 is 30 or 31, change D2 to 30
/// 
/// DCF = (360*(Y2-Y1) + 30*(M2-M1) + (D2-D1)) / 360
///
/// # References
/// - ISDA 2006 Definitions Section 4.16(f)
/// - Bloomberg: DES <GO>, then F9 for day count details
/// - SIA Standard Securities Calculation Methods, Volume 1
///
/// # Verification
/// Validated against Bloomberg for dates:
/// - 2024-01-15 to 2024-07-15: 0.5 (exact)
/// - 2024-01-31 to 2024-03-31: 0.166667 (60/360)
/// - 2024-02-28 to 2024-08-31: 0.508333 (183/360)
///
/// # Edge Cases Verified
/// - End of February (leap year): Uses actual date
/// - 31st to 31st: Both adjusted to 30
/// - 30th to 31st: End date adjusted to 30
pub fn day_count_30_360_us(d1: Date, d2: Date) -> Decimal {
    let mut d1_day = d1.day();
    let mut d2_day = d2.day();
    
    // ISDA 2006: If D1 is 31, change D1 to 30
    if d1_day == 31 {
        d1_day = 30;
    }
    
    // ISDA 2006: If D2 is 31 AND (D1 is 30 or 31), change D2 to 30
    if d2_day == 31 && d1_day == 30 {
        d2_day = 30;
    }
    
    let days = 360 * (d2.year() - d1.year())
             + 30 * (d2.month() as i32 - d1.month() as i32)
             + (d2_day as i32 - d1_day as i32);
    
    Decimal::from(days) / Decimal::from(360)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    /// Test cases from ISDA 2006 Section 4.16(f) examples
    #[test]
    fn test_isda_examples() {
        // Standard case
        assert_eq!(
            day_count_30_360_us(date(2024, 1, 15), date(2024, 7, 15)),
            dec!(0.5)
        );
    }
    
    /// Bloomberg validation - captured 2025-01-15
    #[test]
    fn test_bloomberg_validation() {
        // Bloomberg DES: Apple 3.85% 2043
        // Settlement: 2025-01-15, Next coupon: 2025-05-04
        // Accrued days: 73, Day count: 30/360
        // Expected DCF: 73/360 = 0.202778
        assert_decimal_eq!(
            day_count_30_360_us(date(2024, 11, 4), date(2025, 1, 15)),
            dec!(0.202778),
            dec!(0.000001)
        );
    }
}
```

---

## Architecture Principles

### Core Design Philosophy
1. **Calculation/System Separation**: Pure calc library vs stateful pricing system (see above)
2. **Vendor Agnostic**: All external integrations via traits/adapters
3. **Zero Allocation Hot Path**: Ring buffers, arena allocators, pre-allocated buffers
4. **Sub-Microsecond Target**: Critical pricing paths < 1µs
5. **Pluggable Storage**: Embedded DB default, but trait-based for external providers
6. **Type-Safe Finance**: Compile-time validation of financial calculations
7. **API-First Configuration**: All configuration exposed via clean, typed APIs
8. **UI-Ready**: Every config operation available for management UIs
9. **CLI-Complete**: Full system management possible via command line

### Crate Responsibilities (Separated)

| Layer | Crate | Stateless? | Embeddable? | Purpose |
|-------|-------|------------|-------------|---------|
| **Calc Library** | `convex-core` | ✅ Yes | ✅ Yes | Date, Decimal, Currency types |
| **Calc Library** | `convex-daycount` | ✅ Yes | ✅ Yes | Day count conventions |
| **Calc Library** | `convex-calendar` | ✅ Yes | ✅ Yes | Holiday calendars |
| **Calc Library** | `convex-curves` | ✅ Yes | ✅ Yes | Curve math (interpolation, discount factors) |
| **Calc Library** | `convex-bonds` | ✅ Yes | ✅ Yes | Bond types, cashflow generation |
| **Calc Library** | `convex-calc` | ✅ Yes | ✅ Yes | All calculations (yields, spreads, risk) |
| **Bindings** | `convex-ffi` | ✅ Yes | ✅ Yes | C/Excel bindings (wraps calc) |
| **Bindings** | `convex-py` | ✅ Yes | ✅ Yes | Python bindings (wraps calc) |
| **Bindings** | `convex-wasm` | ✅ Yes | ✅ Yes | WASM bindings (wraps calc) |
| **Pricing System** | `convex-engine` | ❌ No | ❌ No | Stateful orchestration, calc graph |
| **Pricing System** | `convex-storage` | ❌ No | ❌ No | Persistence layer |
| **Pricing System** | `convex-server` | ❌ No | ❌ No | REST/WebSocket/CLI |
| **Pricing System** | `convex-config` | ❌ No | ❌ No | Configuration management |

### Calculation Library Structure (`convex-calc`)

```rust
// =============================================================================
// CALCULATION LIBRARY - PURE FUNCTIONS ONLY
// =============================================================================

/// All calculation functions are:
/// - Pure (same inputs → same outputs, always)
/// - Synchronous (no async, no tokio)
/// - Allocation-minimal (use stack where possible)
/// - Well-documented (methodology, references, edge cases)

pub mod daycount {
    /// Day count conventions - all pure functions
    pub fn dcf_30_360_us(d1: Date, d2: Date) -> Decimal;
    pub fn dcf_30e_360(d1: Date, d2: Date) -> Decimal;
    pub fn dcf_act_360(d1: Date, d2: Date) -> Decimal;
    pub fn dcf_act_365(d1: Date, d2: Date) -> Decimal;
    pub fn dcf_act_act_icma(d1: Date, d2: Date, freq: Frequency, ref_start: Date, ref_end: Date) -> Decimal;
    // ... etc
}

pub mod cashflows {
    /// Cashflow generation - pure functions
    pub fn generate_fixed_cashflows(bond: &Bond, settlement: Date) -> Vec<Cashflow>;
    pub fn generate_float_cashflows(bond: &Bond, settlement: Date, fixings: &[Fixing]) -> Vec<Cashflow>;
    pub fn generate_amortizing_cashflows(bond: &Bond, settlement: Date) -> Vec<Cashflow>;
}

pub mod yields {
    /// Yield calculations - pure functions
    /// All take inputs explicitly, no global state
    
    /// Calculate YTM from price using Newton-Raphson
    /// Reference: Fabozzi, "Bond Markets, Analysis, and Strategies", 4th ed, Ch 3
    pub fn ytm_from_price(
        cashflows: &[Cashflow],
        dirty_price: Decimal,
        settlement: Date,
        day_count: DayCount,
        frequency: Frequency,
    ) -> Result<Decimal, CalcError>;
    
    /// Calculate price from YTM
    pub fn price_from_ytm(
        cashflows: &[Cashflow],
        ytm: Decimal,
        settlement: Date,
        day_count: DayCount,
        frequency: Frequency,
    ) -> Decimal;
    
    /// Calculate yield to worst (callable bonds)
    pub fn ytw(
        cashflows: &[Cashflow],
        call_schedule: &CallSchedule,
        dirty_price: Decimal,
        settlement: Date,
        day_count: DayCount,
        frequency: Frequency,
    ) -> Result<(Decimal, Date), CalcError>;  // (yield, worst_date)
    
    /// Current yield (simple)
    pub fn current_yield(coupon_rate: Decimal, clean_price: Decimal) -> Decimal;
}

pub mod spreads {
    /// Spread calculations - pure functions
    /// All require curve data passed in explicitly
    
    /// G-spread: yield minus interpolated government yield
    pub fn g_spread(
        bond_ytm: Decimal,
        curve: &CurveData,  // Just the data, not a service
        maturity: Date,
    ) -> Decimal;
    
    /// I-spread: yield minus interpolated swap rate
    pub fn i_spread(
        bond_ytm: Decimal,
        swap_curve: &CurveData,
        maturity: Date,
    ) -> Decimal;
    
    /// Z-spread: spread that makes PV of cashflows equal dirty price
    pub fn z_spread(
        cashflows: &[Cashflow],
        dirty_price: Decimal,
        discount_curve: &CurveData,
        settlement: Date,
    ) -> Result<Decimal, CalcError>;
    
    /// OAS using Hull-White model (for callable bonds)
    pub fn oas_hull_white(
        cashflows: &[Cashflow],
        call_schedule: &CallSchedule,
        dirty_price: Decimal,
        discount_curve: &CurveData,
        volatility: Decimal,
        mean_reversion: Decimal,
        settlement: Date,
    ) -> Result<Decimal, CalcError>;
    
    /// Asset swap spread
    pub fn asset_swap_spread(
        bond_dirty_price: Decimal,
        bond_cashflows: &[Cashflow],
        swap_curve: &CurveData,
        settlement: Date,
    ) -> Decimal;
    
    /// Discount margin for FRNs
    pub fn discount_margin(
        frn_dirty_price: Decimal,
        projected_cashflows: &[Cashflow],
        discount_curve: &CurveData,
        settlement: Date,
    ) -> Result<Decimal, CalcError>;
}

pub mod risk {
    /// Risk metrics - pure functions
    
    /// Modified duration (analytical)
    pub fn modified_duration(
        cashflows: &[Cashflow],
        ytm: Decimal,
        settlement: Date,
        day_count: DayCount,
        frequency: Frequency,
    ) -> Decimal;
    
    /// Macaulay duration
    pub fn macaulay_duration(
        cashflows: &[Cashflow],
        ytm: Decimal,
        settlement: Date,
        day_count: DayCount,
        frequency: Frequency,
    ) -> Decimal;
    
    /// Effective duration (numerical, for callable bonds)
    pub fn effective_duration(
        price_fn: impl Fn(Decimal) -> Decimal,  // Price as function of yield
        current_yield: Decimal,
        shock_bps: Decimal,
    ) -> Decimal;
    
    /// Convexity
    pub fn convexity(
        cashflows: &[Cashflow],
        ytm: Decimal,
        settlement: Date,
        day_count: DayCount,
        frequency: Frequency,
    ) -> Decimal;
    
    /// DV01 (dollar value of 1bp)
    pub fn dv01(
        modified_duration: Decimal,
        dirty_price: Decimal,
        face_value: Decimal,
    ) -> Decimal;
    
    /// Key rate durations
    pub fn key_rate_durations(
        cashflows: &[Cashflow],
        discount_curve: &CurveData,
        settlement: Date,
        key_tenors: &[Tenor],
    ) -> Vec<(Tenor, Decimal)>;
}

pub mod curves {
    /// Curve mathematics - pure functions
    /// These work on curve DATA, not curve SERVICES
    
    /// Interpolate zero rate at tenor
    pub fn interpolate_zero(
        curve: &CurveData,
        tenor: Decimal,  // Years
        method: InterpolationMethod,
    ) -> Decimal;
    
    /// Get discount factor
    pub fn discount_factor(
        curve: &CurveData,
        settlement: Date,
        payment_date: Date,
    ) -> Decimal;
    
    /// Get forward rate
    pub fn forward_rate(
        curve: &CurveData,
        start_date: Date,
        end_date: Date,
        day_count: DayCount,
    ) -> Decimal;
    
    /// Bootstrap curve from instruments (pure - takes data, returns data)
    pub fn bootstrap(
        instruments: &[BootstrapInstrument],
        settlement: Date,
        config: &BootstrapConfig,
    ) -> Result<CurveData, CalcError>;
}

/// CurveData is just data - no behavior, no services
#[derive(Debug, Clone)]
pub struct CurveData {
    pub as_of: Date,
    pub currency: Currency,
    pub points: Vec<CurvePoint>,
}

#[derive(Debug, Clone, Copy)]
pub struct CurvePoint {
    pub tenor_years: Decimal,
    pub zero_rate: Decimal,
    pub discount_factor: Decimal,
}
```

### Pricing System Structure (`convex-engine`)

```rust
// =============================================================================
// PRICING SYSTEM - STATEFUL ORCHESTRATION
// Uses convex-calc for all actual calculations
// =============================================================================

use convex_calc::{yields, spreads, risk, curves, cashflows};

/// PricingEngine wraps the pure calculation library with:
/// - State management (curve cache, config cache)
/// - Service integration (bond service, curve service)
/// - Enterprise patterns (circuit breakers, retries, metrics)
/// - Streaming (subscriptions, publishing)
pub struct PricingEngine {
    // Services (stateful)
    bond_service: Arc<dyn BondService>,
    curve_service: Arc<dyn CurveService>,
    config_service: Arc<dyn ConfigService>,
    
    // Caches (stateful)
    curve_cache: Arc<CurveCache>,
    config_cache: Arc<ConfigCache>,
    
    // Enterprise (stateful)
    circuit_breaker: CircuitBreaker,
    metrics: PricingMetrics,
}

impl PricingEngine {
    /// Price a bond - uses calc library internally
    pub fn price(&self, instrument_id: &InstrumentId, settlement: Date) -> Result<BondQuote, PricingError> {
        // 1. Get bond from service (stateful)
        let bond = self.bond_service.get(instrument_id)?;
        
        // 2. Get config from service (stateful)
        let config = self.config_service.get_effective(&bond)?;
        
        // 3. Get curves from cache (stateful)
        let discount_curve = self.curve_cache.get(&config.discount_curve)?;
        let benchmark_curve = self.curve_cache.get(&config.benchmark_curve)?;
        
        // 4. Generate cashflows (PURE - from calc library)
        let cfs = cashflows::generate_fixed_cashflows(&bond, settlement);
        
        // 5. Calculate all analytics (PURE - from calc library)
        let ytm = yields::ytm_from_price(&cfs, dirty_price, settlement, bond.day_count, bond.frequency)?;
        let g_spread = spreads::g_spread(ytm, &benchmark_curve.data, bond.maturity)?;
        let z_spread = spreads::z_spread(&cfs, dirty_price, &discount_curve.data, settlement)?;
        let mod_dur = risk::modified_duration(&cfs, ytm, settlement, bond.day_count, bond.frequency);
        let convexity = risk::convexity(&cfs, ytm, settlement, bond.day_count, bond.frequency);
        let dv01 = risk::dv01(mod_dur, dirty_price, bond.face_value);
        
        // 6. Build result (stateful - has timestamps, sources, etc.)
        Ok(BondQuote {
            instrument_id: instrument_id.clone(),
            timestamp: jiff::Timestamp::now(),
            // ... all the calculated values
        })
    }
}
```

### Configuration Management Principles
- **Every config is an API**: No hardcoded values; all configuration exposed via typed APIs
- **Three interfaces, one source**: CLI, REST API, and SDK all use the same underlying config service
- **Versioned configs**: All configuration changes tracked with audit trail
- **Validation at boundaries**: Config validated on load/save with detailed error messages
- **Hierarchical defaults**: Global → Asset Class → Currency → Sector → Issuer → Bond level
- **Hot reload capable**: Configuration changes apply without restart where safe
- **Export/Import**: Full configuration can be exported to files and imported

### Threading Model
- **Thread-per-core** for I/O-bound market data (consider `monoio` or `tokio` current-thread)
- **Work-stealing** only for compute-heavy batch operations
- **Lock-free structures** for all shared state (DashMap, crossbeam queues)

---

## Expected Crate Structure

**IMPORTANT: Before creating any new crates, Claude Code MUST analyze the existing codebase.**

### Crate Mapping: Proposed → Existing

The prompt uses **proposed crate names** for clarity. When implementing, Claude Code MUST:

1. **Scan existing crates** in the workspace first
2. **Map proposed names to existing crates** where functionality overlaps
3. **Extend existing crates** rather than creating new ones
4. **Only create new crates** when functionality is genuinely new

| Proposed Name | Likely Existing Crate | Action |
|---------------|----------------------|--------|
| `convex-instruments` | `convex-bonds` | **USE EXISTING** - extend with missing bond types |
| `convex-conventions` | `convex-daycounts`, `convex-calendar` | Check if exists, consolidate or use existing |
| `convex-curves` | `convex-curves` | Likely exists, extend as needed |
| `convex-pricing` | `convex-pricing` or `convex-analytics` | Check existing, extend |
| `convex-core` | `convex-core` or `convex-common` | Likely exists |

### Crate Discovery Process

Before ANY implementation, Claude Code must run:

```bash
# 1. List all existing crates
ls -la */Cargo.toml
cat Cargo.toml | grep members -A 50

# 2. For each proposed crate, check if equivalent exists
# Example: Looking for bond/instrument definitions
grep -r "struct Bond" --include="*.rs" | head -20
grep -r "enum InstrumentType" --include="*.rs" | head -20
grep -r "CallSchedule\|SinkSchedule" --include="*.rs" | head -20

# 3. Check existing public APIs
cat convex-bonds/src/lib.rs  # or whatever the bond crate is called

# 4. Document findings before proceeding
```

### Decision Framework

```
IF existing crate covers >50% of proposed functionality:
    → EXTEND existing crate
    → Add missing types/functions
    → Maintain backward compatibility
    
IF existing crate covers <50% but exists:
    → DISCUSS with user before deciding
    → Consider refactoring vs new crate
    
IF no existing crate covers the functionality:
    → CREATE new crate
    → Follow proposed naming
```

### Example: Bond Types

If `convex-bonds` already exists with basic `Bond` struct:

```rust
// EXISTING in convex-bonds/src/lib.rs
pub struct Bond {
    pub isin: String,
    pub coupon: Decimal,
    pub maturity: Date,
    // ... basic fields
}
```

Claude Code should EXTEND it:

```rust
// ADD to convex-bonds, don't create convex-instruments
pub struct Bond {
    // ... existing fields unchanged ...
    
    // NEW: Add comprehensive bond features
    pub instrument_type: InstrumentType,
    pub call_schedule: Option<CallSchedule>,
    pub sink_schedule: Option<SinkSchedule>,
    pub floating_rate: Option<FloatingRateTerms>,
    // ...
}

// NEW: Add missing types to same crate
pub enum InstrumentType { ... }
pub struct CallSchedule { ... }
pub struct SinkSchedule { ... }
pub struct FloatingRateTerms { ... }
```

### Canonical Crate Names (Use Existing When Available)

The following are **proposed logical groupings**. Map to existing crates:

```
convex/
├── convex-core/           # Foundation types (likely exists)
├── convex-conventions/    # Day counts, calendars (may be split or use existing)
├── convex-bonds/          # Bond definitions ← USE THIS if it exists (prompt may say convex-instruments)
├── convex-curves/         # Yield curves (likely exists)
├── convex-pricing/        # Calculations (likely exists)
├── convex-portfolio/      # Portfolio analytics
├── convex-engine/         # Runtime, streaming, calc graph
├── convex-market-data/    # Provider abstraction
├── convex-storage/        # Persistence
├── convex-config/         # Configuration
├── convex-server/         # API + CLI
├── convex-ffi/            # Bindings
└── convex-wasm/           # Browser
```

### Consolidation Rationale

| Prompt Uses | Map To Existing | Reason |
|-------------|-----------------|--------|
| `convex-instruments` | `convex-bonds` | Same domain - extend existing |
| `convex-conventions` | `convex-daycounts` + `convex-calendar` | Consolidate if both exist, or extend one |
| `convex-engine` | May need new | Calc graph, streaming, runtime |
| `convex-server` | `convex-api` + `convex-cli` | Consolidate if both exist |

### Final Target: ~12 Crates

| Crate | Responsibility | Key Types |
|-------|----------------|-----------|
| `convex-core` | Foundation types | `Decimal`, `Date`, `Rate`, `Currency`, `Tenor` |
| `convex-conventions` | Market conventions | `DayCount`, `Calendar`, `BusinessDayRule`, `SettlementRule` |
| `convex-bonds` | Bond/instrument definitions | `Bond`, `CallSchedule`, `SinkSchedule`, `FloatingRateIndex` |
| `convex-curves` | Curve building | `Curve`, `CurveBuilder`, `Interpolator`, `BootstrapEngine` |
| `convex-pricing` | All calculations | `BondPricer`, `SpreadCalculator`, `RiskCalculator`, `BondQuote` |
| `convex-portfolio` | Portfolio analytics | `Portfolio`, `NavCalculator`, `ContributionAnalyzer` |
| `convex-engine` | Runtime infrastructure | `PricingEngine`, `QuoteStream`, `CurveCache`, `Diagnostics` |
| `convex-market-data` | Data providers | `MarketDataProvider`, `Quote`, `QuoteSource` |
| `convex-storage` | Persistence | `StorageAdapter`, `RedbStorage` |
| `convex-config` | Configuration | `ConfigService`, `CurveConfig`, `PricingConfig` |
| `convex-server` | Deployment | API routes, CLI commands, metrics |
| `convex-ffi` | Bindings | C API, Excel XLL, Python (PyO3) |
| `convex-curves` | Curve building | `Curve`, `CurveBuilder`, `Interpolator`, `BootstrapEngine` |
| `convex-pricing` | All calculations | `BondPricer`, `SpreadCalculator`, `RiskCalculator` |
| `convex-portfolio` | Portfolio analytics | `Portfolio`, `NavCalculator`, `ContributionAnalyzer` |
| `convex-engine` | Runtime infrastructure | `PricingEngine`, `QuoteStream`, `CurveCache`, `Diagnostics` |
| `convex-market-data` | Data providers | `MarketDataProvider`, `Quote`, `QuoteSource` |
| `convex-storage` | Persistence | `StorageAdapter`, `RedbStorage` |
| `convex-config` | Configuration | `ConfigService`, `CurveConfig`, `PricingConfig` |
| `convex-server` | Deployment | API routes, CLI commands, metrics |
| `convex-ffi` | Bindings | C API, Excel XLL, Python (PyO3) |

### Crate Dependency Hierarchy

```
                    convex-core
                        │
         ┌──────────────┼──────────────┐
         │              │              │
   convex-daycounts  convex-calendar  convex-config
         │              │              │
         └──────────────┼──────────────┘
                        │
                  convex-curves
                        │
         ┌──────────────┼──────────────┐
         │              │              │
    convex-bond    convex-market-data  convex-storage
         │              │              │
         └──────────────┼──────────────┘
                        │
                  convex-pricing
                        │
         ┌──────────────┼──────────────┐
         │              │              │
  convex-portfolio  convex-calc-engine convex-streaming
         │              │              │
         └──────────────┼──────────────┘
                        │
              ┌─────────┼─────────┐
              │         │         │
         convex-cli  convex-api  convex-ffi
```

**Note**: External storage implementations (PostgreSQL, MongoDB, etc.) are OUT OF SCOPE for this framework. The framework provides only the `StorageAdapter` trait and the embedded `redb` implementation. Users wanting external databases implement the trait themselves.

---

## Dependencies (Cargo.toml - December 2025 Best Practices)

```toml
[workspace]
members = [
    "convex-core",
    "convex-daycounts",
    "convex-calendar",
    "convex-curves",
    "convex-bond",
    "convex-pricing",
    "convex-market-data",
    "convex-calc-engine",
    "convex-storage",
    "convex-transport",
    "convex-refdata",
    "convex-config",
    "convex-metrics",
    "convex-ffi",
    "convex-wasm",
    "convex-py",
]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.83"
license = "MIT OR Apache-2.0"
repository = "https://github.com/yourorg/convex"

[workspace.dependencies]
# ============================================
# ASYNC RUNTIME (Choose ONE strategy)
# ============================================
# Option A: Standard production (recommended for most cases)
tokio = { version = "1.42", features = ["rt-multi-thread", "sync", "time", "macros"] }
# Option B: Thread-per-core for ultra-low latency (Linux 5.6+ io_uring)
# monoio = { version = "0.2", features = ["iouring"] }
# Option C: Cross-platform thread-per-core
# compio = { version = "0.12" }

async-trait = "0.1"

# ============================================
# NUMERIC & FINANCIAL
# ============================================
rust_decimal = { version = "1.36", features = ["maths", "serde"] }
rust_decimal_macros = "1.36"

# ============================================
# DATE/TIME (jiff is the 2025 standard)
# ============================================
jiff = { version = "0.2", features = ["std", "serde"] }
# chrono kept for legacy interop only
chrono = { version = "0.4", default-features = false, features = ["serde"] }

# ============================================
# SERIALIZATION
# ============================================
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# Zero-copy binary serialization (hot path)
rkyv = { version = "0.8", features = ["validation", "bytecheck"] }
# Alternative: Simple binary encoding for config/persistence
bincode = "2.0.0-rc"
# Human-readable config
toml = "0.8"

# ============================================
# CONCURRENCY & LOCK-FREE
# ============================================
crossbeam = { version = "0.8", features = ["crossbeam-channel", "crossbeam-queue"] }
dashmap = "6"
parking_lot = "0.12"
arc-swap = "1.7"

# ============================================
# EMBEDDED DATABASE (Choose ONE primary)
# ============================================
# Option A: Pure Rust, stable file format (RECOMMENDED)
redb = "2.4"
# Option B: LSM with time-travel (if versioning needed, still beta)
# surrealkv = "0.9"
# Option C: RocksDB wrapper (if you need RocksDB compatibility)
# rocksdb = { version = "0.22", default-features = false }

# ============================================
# INCREMENTAL COMPUTATION
# ============================================
# Salsa for dependency graph / memoization
salsa = "0.18"
# Graph data structures
petgraph = { version = "0.7", features = ["serde-1"] }

# ============================================
# OBSERVABILITY (OpenTelemetry stack)
# ============================================
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
tracing-opentelemetry = "0.28"
opentelemetry = { version = "0.27", features = ["metrics", "trace"] }
opentelemetry_sdk = { version = "0.27", features = ["rt-tokio"] }
opentelemetry-otlp = { version = "0.27", features = ["grpc-tonic"] }
# Direct Prometheus metrics (alternative to OTel)
prometheus = { version = "0.13", features = ["process"] }
metrics = "0.24"
metrics-exporter-prometheus = "0.16"

# ============================================
# ERROR HANDLING
# ============================================
thiserror = "2"
anyhow = "1"

# ============================================
# CONFIGURATION
# ============================================
config = { version = "0.14", features = ["toml"] }
dotenvy = "0.15"

# ============================================
# TESTING & BENCHMARKING
# ============================================
criterion = { version = "0.5", features = ["html_reports"] }
proptest = "1"
approx = "0.5"
test-case = "3"

# ============================================
# FFI BINDINGS
# ============================================
pyo3 = { version = "0.23", features = ["extension-module"] }
wasm-bindgen = "0.2"
abi_stable = "0.11"

# ============================================
# UTILITIES
# ============================================
once_cell = "1.20"
derive_more = { version = "1", features = ["full"] }
strum = { version = "0.26", features = ["derive"] }
uuid = { version = "1.11", features = ["v4", "serde"] }
indexmap = { version = "2", features = ["serde"] }
smallvec = { version = "1.13", features = ["serde"] }
arrayvec = { version = "0.7", features = ["serde"] }
bumpalo = { version = "3.16", features = ["collections"] }
tinyvec = { version = "1.8", features = ["alloc", "serde"] }
hashbrown = { version = "0.15", features = ["serde"] }

# ============================================
# NUMERICS & SIMD
# ============================================
num-traits = "0.2"
nalgebra = { version = "0.33", features = ["serde-serialize"] }
wide = "0.7"  # Portable SIMD
```

---

## Core Components

### 1. Market Data Provider Abstraction (`convex-market-data`)

```rust
use async_trait::async_trait;
use std::sync::Arc;

/// Instrument identifier supporting multiple ID schemes
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum InstrumentId {
    Isin(String),
    Cusip(String),
    Figi(String),
    BloombergId(String),
    ReutersRic(String),
    Internal(u64),
}

/// Market quote with full attribution
#[derive(Debug, Clone, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
#[rkyv(compare(PartialEq), derive(Debug))]
pub struct Quote {
    pub instrument_id: InstrumentId,
    pub bid: Option<Decimal>,
    pub ask: Option<Decimal>,
    pub mid: Option<Decimal>,
    pub bid_size: Option<u64>,
    pub ask_size: Option<u64>,
    pub timestamp: jiff::Timestamp,
    pub source: QuoteSource,
}

#[derive(Debug, Clone)]
pub enum QuoteSource {
    Bloomberg,
    Refinitiv,
    Ice,
    MarketAxess,
    Tradeweb,
    Internal,
}

/// Subscription handle for cleanup
pub struct Subscription {
    id: u64,
    cancel_tx: tokio::sync::oneshot::Sender<()>,
}

#[async_trait]
pub trait MarketDataProvider: Send + Sync {
    /// Subscribe to real-time updates
    async fn subscribe(
        &self,
        instruments: &[InstrumentId],
        callback: Arc<dyn Fn(Quote) + Send + Sync>,
    ) -> Result<Subscription, MarketDataError>;
    
    /// Get current snapshot
    async fn snapshot(
        &self,
        instruments: &[InstrumentId],
    ) -> Result<Vec<Quote>, MarketDataError>;
    
    /// Provider name for logging
    fn name(&self) -> &'static str;
    
    /// Health check
    async fn is_healthy(&self) -> bool;
}

/// Aggregator for multiple providers with failover
pub struct MarketDataAggregator {
    providers: Vec<(Priority, Arc<dyn MarketDataProvider>)>,
    conflation_window: std::time::Duration,
}
```

### 2. Calculation Engine with Dependency Graph (`convex-calc-engine`)

```rust
use dashmap::DashMap;
use petgraph::graph::DiGraph;
use std::sync::Arc;

/// Node in the calculation graph
pub trait CalculationNode: Send + Sync {
    fn node_id(&self) -> NodeId;
    fn dependencies(&self) -> &[NodeId];
    fn calculate(&self, ctx: &CalculationContext) -> Result<NodeValue, CalcError>;
    fn is_stale(&self, revision: Revision) -> bool;
}

/// Revision tracking for cache invalidation
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Revision(u64);

/// The calculation graph manages dependencies and memoization
pub struct CalculationGraph {
    /// Directed graph of dependencies
    graph: parking_lot::RwLock<DiGraph<NodeId, ()>>,
    
    /// Node implementations
    nodes: DashMap<NodeId, Arc<dyn CalculationNode>>,
    
    /// Memoized values with revision tracking
    cache: DashMap<NodeId, CachedValue>,
    
    /// Set of dirty nodes pending recalculation
    dirty: DashMap<NodeId, Revision>,
    
    /// Current global revision
    current_revision: std::sync::atomic::AtomicU64,
}

impl CalculationGraph {
    /// Mark input as changed, propagate dirty flags
    pub fn invalidate(&self, node_id: NodeId) {
        let rev = self.bump_revision();
        self.dirty.insert(node_id, rev);
        
        // Propagate to dependents using topological order
        let graph = self.graph.read();
        for dependent in self.dependents_of(node_id, &graph) {
            self.dirty.insert(dependent, rev);
        }
    }
    
    /// Get value, recomputing if stale
    pub fn get(&self, node_id: NodeId, ctx: &CalculationContext) -> Result<NodeValue, CalcError> {
        // Check cache first
        if let Some(cached) = self.cache.get(&node_id) {
            if !self.dirty.contains_key(&node_id) {
                return Ok(cached.value.clone());
            }
        }
        
        // Ensure dependencies are fresh
        let node = self.nodes.get(&node_id).ok_or(CalcError::NodeNotFound)?;
        for dep_id in node.dependencies() {
            self.get(*dep_id, ctx)?;
        }
        
        // Calculate and cache
        let value = node.calculate(ctx)?;
        self.cache.insert(node_id, CachedValue {
            value: value.clone(),
            revision: self.current_revision.load(Ordering::SeqCst),
        });
        self.dirty.remove(&node_id);
        
        Ok(value)
    }
}
```

---

## Curve Building & Pricing Configuration (`convex-curves`, `convex-pricing`)

This is the **critical configuration layer** that maps market data to curves, defines benchmark relationships, and controls pricing behavior for different bond types.

### Curve Configuration Architecture

```rust
// =============================================================================
// CURVE DEFINITIONS
// =============================================================================

/// Curve identifier
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct CurveId(pub String);

/// Types of curves supported
#[derive(Debug, Clone)]
pub enum CurveType {
    /// Government bond curve (Treasury, Bund, Gilt, JGB)
    Government { currency: Currency, country: Country },
    
    /// OIS/Risk-free curve (SOFR, ESTR, SONIA)
    Ois { currency: Currency, index: OvernightIndex },
    
    /// Swap curve (SOFR swaps, EURIBOR swaps)
    Swap { currency: Currency, index: FloatingIndex },
    
    /// Credit spread curve (issuer or sector-specific)
    Credit { 
        reference_curve: CurveId,  // Base curve to add spread to
        entity_type: CreditEntityType,
    },
    
    /// Basis curve (cross-currency, tenor basis)
    Basis { 
        base_curve: CurveId,
        quote_curve: CurveId,
    },
}

#[derive(Debug, Clone)]
pub enum CreditEntityType {
    Issuer(String),           // Specific issuer: "AAPL", "GS"
    Sector(Sector),           // Sector curve: Financials, Industrials
    RatingBucket(CreditRating), // Generic rating curve: AAA, AA, A, BBB
}

/// Interpolation methods for curve construction
#[derive(Debug, Clone, Copy, Default)]
pub enum InterpolationMethod {
    Linear,
    LogLinear,
    CubicSpline,
    #[default]
    MonotoneConvex,  // Hagan-West - preserves monotonicity
    NelsonSiegel,
    NelsonSiegelSvensson,
}

/// Curve segment configuration (different interpolation by tenor)
#[derive(Debug, Clone)]
pub struct CurveSegment {
    pub max_tenor_years: f64,
    pub interpolation: InterpolationMethod,
    pub rate_type: RateType,  // ZeroRate, DiscountFactor, ForwardRate
}

/// Full curve configuration
#[derive(Debug, Clone)]
pub struct CurveConfig {
    pub curve_id: CurveId,
    pub curve_type: CurveType,
    pub currency: Currency,
    
    /// Instruments to bootstrap from (in order)
    pub bootstrap_instruments: Vec<BootstrapInstrument>,
    
    /// Segmented interpolation (e.g., linear short end, cubic mid, flat long end)
    pub segments: Vec<CurveSegment>,
    
    /// Day count for zero rates
    pub day_count: DayCount,
    
    /// Compounding frequency
    pub compounding: Compounding,
    
    /// Turn-of-year adjustment
    pub turn_of_year: bool,
}

/// Bootstrap instrument types
#[derive(Debug, Clone)]
pub enum BootstrapInstrument {
    Deposit { tenor: Tenor, rate_source: MarketDataKey },
    Future { contract: String, price_source: MarketDataKey },
    Swap { tenor: Tenor, rate_source: MarketDataKey },
    Bond { isin: String, price_source: MarketDataKey },
    Ois { tenor: Tenor, rate_source: MarketDataKey },
    BasisSwap { tenor: Tenor, spread_source: MarketDataKey },
}

// =============================================================================
// BOND TYPE PRICING CONFIGURATION
// =============================================================================

/// Pricing configuration for a bond type
/// Maps bond characteristics to required curves and spread methodologies
#[derive(Debug, Clone)]
pub struct BondPricingConfig {
    /// Which bonds this config applies to
    pub applies_to: BondMatcher,
    
    /// Primary benchmark curve for spread calculations
    pub benchmark_curve: CurveId,
    
    /// Discount curve for PV calculations (often same as benchmark)
    pub discount_curve: CurveId,
    
    /// Additional curves needed
    pub additional_curves: Vec<CurveRequirement>,
    
    /// Default spread methodology
    pub default_spread_type: SpreadType,
    
    /// Spread adjustments to apply
    pub spread_adjustments: Vec<SpreadAdjustment>,
    
    /// Pricing hierarchy (fallback order)
    pub pricing_hierarchy: Vec<PricingSource>,
}

/// Matcher for bond characteristics
#[derive(Debug, Clone)]
pub struct BondMatcher {
    pub currency: Option<Currency>,
    pub issuer_type: Option<IssuerType>,
    pub sector: Option<Sector>,
    pub rating_range: Option<(CreditRating, CreditRating)>,
    pub bond_type: Option<BondType>,
}

#[derive(Debug, Clone, Copy)]
pub enum IssuerType {
    Sovereign,
    Supranational,
    Agency,
    Corporate,
    Financial,
    Municipal,
}

/// Curve requirements for pricing
#[derive(Debug, Clone)]
pub struct CurveRequirement {
    pub curve_id: CurveId,
    pub purpose: CurvePurpose,
    pub required: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum CurvePurpose {
    GSpreadBenchmark,      // Government curve for G-spread
    ISpreadBenchmark,      // Swap curve for I-spread
    ZSpreadDiscounting,    // Curve for Z-spread calculation
    OasDiscounting,        // Risk-free curve for OAS
    OasVolatility,         // Vol surface for OAS
    AssetSwapSwapRate,     // Swap curve for ASW calculation
    ForwardProjection,     // Forward curve for FRNs
    CrossCurrencyBasis,    // XCCY basis for cross-currency
}

// =============================================================================
// SPREAD ADJUSTMENTS
// =============================================================================

/// Spread adjustments that can be applied
#[derive(Debug, Clone)]
pub enum SpreadAdjustment {
    /// Fixed basis point adjustment
    Fixed { bps: Decimal, reason: String },
    
    /// Sector-based adjustment
    Sector { 
        sector: Sector, 
        adjustment_bps: Decimal,
    },
    
    /// Rating-based adjustment
    Rating {
        rating: CreditRating,
        adjustment_bps: Decimal,
    },
    
    /// Subordination adjustment (for financials)
    Subordination {
        tier: SubordinationTier,
        adjustment_bps: Decimal,
    },
    
    /// Liquidity adjustment based on trading volume
    Liquidity {
        volume_percentile_threshold: u8,
        adjustment_bps: Decimal,
    },
    
    /// New issue premium
    NewIssue {
        days_since_issue: u32,
        adjustment_bps: Decimal,
    },
    
    /// Cross-currency basis adjustment
    CrossCurrencyBasis {
        from_currency: Currency,
        to_currency: Currency,
        basis_curve: CurveId,
    },
    
    /// Custom/manual adjustment
    Manual {
        adjustment_bps: Decimal,
        reason: String,
        approved_by: Option<String>,
        expiry: Option<jiff::Timestamp>,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum SubordinationTier {
    Senior,
    SeniorPreferred,
    SeniorNonPreferred,  // SNP/HoldCo
    Tier2,
    AdditionalTier1,     // AT1/CoCo
    JuniorSubordinated,
}

// =============================================================================
// PRICING HIERARCHY & OVERRIDES
// =============================================================================

/// Pricing source in priority order
#[derive(Debug, Clone)]
pub enum PricingSource {
    /// Manual trader override (highest priority)
    ManualOverride,
    
    /// Live executable quote from venue
    ExecutableQuote { venues: Vec<String> },
    
    /// Indicative quote from dealer
    IndicativeQuote { dealers: Vec<String> },
    
    /// Composite price from multiple sources
    CompositePrice { sources: Vec<String>, method: CompositeMethod },
    
    /// Model price from spread
    ModelFromSpread { spread_type: SpreadType },
    
    /// Model price from comparable bonds
    ModelFromComparables { similarity_threshold: f64 },
    
    /// Stale price with adjustment
    StaleWithAdjustment { max_age_hours: u32, adjustment_bps: Decimal },
    
    /// Fallback to theoretical
    Theoretical,
}

#[derive(Debug, Clone, Copy)]
pub enum CompositeMethod {
    MidOfBest,
    WeightedAverage,
    Median,
    Vwap,
}

/// Manual price override
#[derive(Debug, Clone)]
pub struct PriceOverride {
    pub instrument_id: InstrumentId,
    pub override_type: OverrideType,
    pub value: Decimal,
    pub reason: String,
    pub entered_by: String,
    pub entered_at: jiff::Timestamp,
    pub expiry: Option<jiff::Timestamp>,
    pub approved_by: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum OverrideType {
    /// Override the clean price directly
    Price,
    /// Override the yield
    Yield,
    /// Override/adjust the spread
    Spread { spread_type: SpreadType },
    /// Add basis points to model price
    Adjustment,
}

// =============================================================================
// CONFIGURATION EXAMPLES
// =============================================================================

impl BondPricingConfig {
    /// USD Investment Grade Corporate bond pricing config
    pub fn usd_ig_corporate() -> Self {
        Self {
            applies_to: BondMatcher {
                currency: Some(Currency::USD),
                issuer_type: Some(IssuerType::Corporate),
                rating_range: Some((CreditRating::AAA, CreditRating::BBBMinus)),
                ..Default::default()
            },
            benchmark_curve: CurveId("USD.GOVT".into()),
            discount_curve: CurveId("USD.SOFR.OIS".into()),
            additional_curves: vec![
                CurveRequirement {
                    curve_id: CurveId("USD.SOFR.SWAP".into()),
                    purpose: CurvePurpose::ISpreadBenchmark,
                    required: true,
                },
                CurveRequirement {
                    curve_id: CurveId("USD.SOFR.SWAP".into()),
                    purpose: CurvePurpose::AssetSwapSwapRate,
                    required: true,
                },
            ],
            default_spread_type: SpreadType::ZSpread,
            spread_adjustments: vec![
                SpreadAdjustment::Sector { 
                    sector: Sector::Financials, 
                    adjustment_bps: dec!(5.0),
                },
                SpreadAdjustment::Liquidity {
                    volume_percentile_threshold: 25,
                    adjustment_bps: dec!(3.0),
                },
            ],
            pricing_hierarchy: vec![
                PricingSource::ManualOverride,
                PricingSource::ExecutableQuote { venues: vec!["MKTX".into(), "TRDW".into()] },
                PricingSource::CompositePrice { 
                    sources: vec!["BVAL".into(), "CBBT".into()],
                    method: CompositeMethod::MidOfBest,
                },
                PricingSource::ModelFromSpread { spread_type: SpreadType::ZSpread },
                PricingSource::ModelFromComparables { similarity_threshold: 0.85 },
            ],
        }
    }
    
    /// EUR Corporate bond pricing config
    pub fn eur_corporate() -> Self {
        Self {
            applies_to: BondMatcher {
                currency: Some(Currency::EUR),
                issuer_type: Some(IssuerType::Corporate),
                ..Default::default()
            },
            benchmark_curve: CurveId("EUR.BUND".into()),  // German Bunds
            discount_curve: CurveId("EUR.ESTR.OIS".into()),
            additional_curves: vec![
                CurveRequirement {
                    curve_id: CurveId("EUR.EURIBOR.SWAP".into()),
                    purpose: CurvePurpose::ISpreadBenchmark,
                    required: true,
                },
                CurveRequirement {
                    curve_id: CurveId("EUR.EURIBOR.SWAP".into()),
                    purpose: CurvePurpose::AssetSwapSwapRate,
                    required: true,
                },
            ],
            default_spread_type: SpreadType::AssetSwap,  // EUR corporates often quoted in ASW
            spread_adjustments: vec![],
            pricing_hierarchy: vec![
                PricingSource::ManualOverride,
                PricingSource::ExecutableQuote { venues: vec!["MKTX".into(), "TRDW".into()] },
                PricingSource::ModelFromSpread { spread_type: SpreadType::AssetSwap },
            ],
        }
    }
    
    /// Financial institution bonds (subordinated debt)
    pub fn financial_subordinated() -> Self {
        Self {
            applies_to: BondMatcher {
                issuer_type: Some(IssuerType::Financial),
                ..Default::default()
            },
            benchmark_curve: CurveId("USD.GOVT".into()),
            discount_curve: CurveId("USD.SOFR.OIS".into()),
            additional_curves: vec![],
            default_spread_type: SpreadType::ZSpread,
            spread_adjustments: vec![
                // Subordination tier adjustments
                SpreadAdjustment::Subordination {
                    tier: SubordinationTier::SeniorNonPreferred,
                    adjustment_bps: dec!(25.0),
                },
                SpreadAdjustment::Subordination {
                    tier: SubordinationTier::Tier2,
                    adjustment_bps: dec!(50.0),
                },
                SpreadAdjustment::Subordination {
                    tier: SubordinationTier::AdditionalTier1,
                    adjustment_bps: dec!(150.0),
                },
            ],
            pricing_hierarchy: vec![
                PricingSource::ManualOverride,
                PricingSource::ModelFromSpread { spread_type: SpreadType::ZSpread },
            ],
        }
    }
    
    /// Floating Rate Notes
    pub fn floating_rate_note() -> Self {
        Self {
            applies_to: BondMatcher {
                bond_type: Some(BondType::FloatingRate),
                ..Default::default()
            },
            benchmark_curve: CurveId("USD.SOFR.OIS".into()),
            discount_curve: CurveId("USD.SOFR.OIS".into()),
            additional_curves: vec![
                CurveRequirement {
                    curve_id: CurveId("USD.SOFR.FORWARD".into()),
                    purpose: CurvePurpose::ForwardProjection,
                    required: true,
                },
            ],
            default_spread_type: SpreadType::DiscountMargin,
            spread_adjustments: vec![],
            pricing_hierarchy: vec![
                PricingSource::ManualOverride,
                PricingSource::ModelFromSpread { spread_type: SpreadType::DiscountMargin },
            ],
        }
    }
}

// =============================================================================
// CURVE CONFIGURATION REGISTRY
// =============================================================================

/// Central registry for all curve and pricing configurations
pub struct PricingConfigRegistry {
    /// Curve definitions
    curves: HashMap<CurveId, CurveConfig>,
    
    /// Bond type pricing configs (matched in order)
    bond_configs: Vec<BondPricingConfig>,
    
    /// Manual overrides
    overrides: DashMap<InstrumentId, PriceOverride>,
    
    /// Market data key mappings
    market_data_mappings: HashMap<MarketDataKey, MarketDataSource>,
}

impl PricingConfigRegistry {
    /// Load configuration from TOML/JSON
    pub fn from_config(config_path: &Path) -> Result<Self>;
    
    /// Get pricing config for a specific bond
    pub fn get_config_for_bond(&self, bond: &Bond) -> Option<&BondPricingConfig> {
        self.bond_configs.iter().find(|cfg| cfg.applies_to.matches(bond))
    }
    
    /// Get all curves required for a bond
    pub fn required_curves(&self, bond: &Bond) -> Vec<CurveId> {
        if let Some(config) = self.get_config_for_bond(bond) {
            let mut curves = vec![
                config.benchmark_curve.clone(),
                config.discount_curve.clone(),
            ];
            curves.extend(config.additional_curves.iter().map(|c| c.curve_id.clone()));
            curves
        } else {
            vec![]
        }
    }
    
    /// Apply manual override
    pub fn set_override(&self, override_: PriceOverride) {
        self.overrides.insert(override_.instrument_id.clone(), override_);
    }
    
    /// Clear expired overrides
    pub fn clear_expired_overrides(&self, now: jiff::Timestamp) {
        self.overrides.retain(|_, v| {
            v.expiry.map(|exp| exp > now).unwrap_or(true)
        });
    }
}

// =============================================================================
// CONFIGURATION FILE FORMAT (TOML)
// =============================================================================

/*
Example configuration file: pricing_config.toml

[curves.USD_GOVT]
type = "government"
currency = "USD"
country = "US"
segments = [
    { max_tenor = 2.0, interpolation = "linear", rate_type = "zero" },
    { max_tenor = 10.0, interpolation = "monotone_convex", rate_type = "zero" },
    { max_tenor = 50.0, interpolation = "flat_forward", rate_type = "forward" },
]
bootstrap_instruments = [
    { type = "bond", isin = "912828ZT4", source = "BBG:GT2" },
    { type = "bond", isin = "91282CJN6", source = "BBG:GT5" },
    { type = "bond", isin = "91282CJP1", source = "BBG:GT10" },
    { type = "bond", isin = "912810TM0", source = "BBG:GT30" },
]

[curves.USD_SOFR_OIS]
type = "ois"
currency = "USD"
index = "SOFR"
bootstrap_instruments = [
    { type = "deposit", tenor = "1D", source = "BBG:SOFRRATE" },
    { type = "future", contract = "SFR1", source = "BBG:SFR1" },
    { type = "ois", tenor = "1Y", source = "BBG:USOSFR1" },
    { type = "ois", tenor = "2Y", source = "BBG:USOSFR2" },
    { type = "ois", tenor = "5Y", source = "BBG:USOSFR5" },
    { type = "ois", tenor = "10Y", source = "BBG:USOSFR10" },
]

[curves.EUR_BUND]
type = "government"
currency = "EUR"
country = "DE"
# ... similar structure

[[bond_pricing]]
name = "USD IG Corporate"
[bond_pricing.applies_to]
currency = "USD"
issuer_type = "corporate"
rating_min = "BBB-"
rating_max = "AAA"

benchmark_curve = "USD_GOVT"
discount_curve = "USD_SOFR_OIS"
default_spread = "z_spread"

[[bond_pricing.additional_curves]]
curve = "USD_SOFR_SWAP"
purpose = "i_spread"

[[bond_pricing.spread_adjustments]]
type = "liquidity"
volume_percentile = 25
adjustment_bps = 3.0

[[bond_pricing.pricing_hierarchy]]
source = "manual_override"

[[bond_pricing.pricing_hierarchy]]
source = "executable_quote"
venues = ["MKTX", "TRDW"]

[[bond_pricing.pricing_hierarchy]]
source = "model_from_spread"
spread_type = "z_spread"

# Manual overrides section
[[overrides]]
isin = "US037833DV96"
type = "spread"
spread_type = "z_spread"
value = 125.0  # bps
reason = "Illiquid, last trade +15bps to model"
entered_by = "jsmith"
expiry = "2025-01-15T17:00:00Z"
*/
```

### Market Data to Curve Mapping

```rust
/// Market data key for curve inputs
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct MarketDataKey(pub String);

/// Source of market data
#[derive(Debug, Clone)]
pub enum MarketDataSource {
    /// Bloomberg field
    Bloomberg { ticker: String, field: String },
    /// Refinitiv RIC
    Refinitiv { ric: String, field: String },
    /// Internal system
    Internal { source: String, key: String },
    /// Calculated from other data
    Derived { formula: String, inputs: Vec<MarketDataKey> },
}

/// Curve builder that wires market data to curves
pub struct CurveBuilder {
    config: CurveConfig,
    market_data: Arc<dyn MarketDataProvider>,
}

impl CurveBuilder {
    /// Build curve from live market data
    pub async fn build(&self, as_of: jiff::Timestamp) -> Result<BuiltCurve, CurveError> {
        // 1. Fetch all required market data
        let mut instruments = Vec::new();
        for instr in &self.config.bootstrap_instruments {
            let data = self.fetch_instrument_data(instr).await?;
            instruments.push(data);
        }
        
        // 2. Bootstrap the curve
        let curve = self.bootstrap(instruments)?;
        
        // 3. Apply interpolation per segment
        let interpolated = self.apply_segmented_interpolation(curve)?;
        
        Ok(BuiltCurve {
            curve_id: self.config.curve_id.clone(),
            as_of,
            curve: interpolated,
            build_time: jiff::Timestamp::now(),
        })
    }
}
```

---

## Configuration Management API (`convex-config`)

The configuration layer is designed for **production use** with clean APIs that support CLI, REST, UI, and programmatic access. All configuration operations go through a unified API surface.

### Core Configuration Traits

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// =============================================================================
// CONFIGURATION TRAIT - All configs implement this
// =============================================================================

/// Base trait for all configuration types
pub trait Configuration: Serialize + for<'de> Deserialize<'de> + Clone + Send + Sync {
    /// Unique identifier type for this config
    type Id: ConfigId;
    
    /// Get the unique identifier
    fn id(&self) -> &Self::Id;
    
    /// Validate the configuration
    fn validate(&self) -> Result<(), ValidationErrors>;
    
    /// Get configuration metadata
    fn metadata(&self) -> &ConfigMetadata;
    
    /// Configuration type name for routing/display
    fn config_type() -> &'static str;
}

/// Trait for configuration identifiers
pub trait ConfigId: Clone + Eq + std::hash::Hash + Serialize + for<'de> Deserialize<'de> + Send + Sync {
    fn as_str(&self) -> &str;
    fn from_str(s: &str) -> Result<Self, ConfigError>;
}

/// Metadata attached to all configurations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigMetadata {
    pub created_at: jiff::Timestamp,
    pub created_by: String,
    pub updated_at: jiff::Timestamp,
    pub updated_by: String,
    pub version: u64,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub enabled: bool,
}

// =============================================================================
// VALIDATION ERRORS
// =============================================================================

/// Detailed validation errors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationErrors {
    pub errors: Vec<ValidationError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
    pub code: String,
    pub severity: ValidationSeverity,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ValidationSeverity {
    Error,    // Blocks save
    Warning,  // Allows save with confirmation
    Info,     // Informational
}

impl ValidationErrors {
    pub fn has_errors(&self) -> bool {
        self.errors.iter().any(|e| matches!(e.severity, ValidationSeverity::Error))
    }
}

// =============================================================================
// CONFIGURATION STORE TRAIT
// =============================================================================

/// Storage abstraction for configurations
#[async_trait]
pub trait ConfigStore<C: Configuration>: Send + Sync {
    /// Get a configuration by ID
    async fn get(&self, id: &C::Id) -> Result<Option<C>, ConfigError>;
    
    /// Get a configuration at a specific version
    async fn get_version(&self, id: &C::Id, version: u64) -> Result<Option<C>, ConfigError>;
    
    /// List all configurations with optional filtering
    async fn list(&self, filter: &ConfigFilter) -> Result<Vec<C>, ConfigError>;
    
    /// Create a new configuration
    async fn create(&self, config: C, user: &str) -> Result<C, ConfigError>;
    
    /// Update an existing configuration
    async fn update(&self, config: C, user: &str) -> Result<C, ConfigError>;
    
    /// Delete a configuration (soft delete)
    async fn delete(&self, id: &C::Id, user: &str) -> Result<(), ConfigError>;
    
    /// Get version history
    async fn history(&self, id: &C::Id) -> Result<Vec<ConfigVersion<C>>, ConfigError>;
    
    /// Diff two versions
    async fn diff(&self, id: &C::Id, v1: u64, v2: u64) -> Result<ConfigDiff, ConfigError>;
    
    /// Import configurations from file/stream
    async fn import(&self, configs: Vec<C>, user: &str, mode: ImportMode) -> Result<ImportResult, ConfigError>;
    
    /// Export configurations to serializable format
    async fn export(&self, filter: &ConfigFilter, format: ExportFormat) -> Result<Vec<u8>, ConfigError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigFilter {
    pub ids: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub enabled: Option<bool>,
    pub created_after: Option<jiff::Timestamp>,
    pub updated_after: Option<jiff::Timestamp>,
    pub search: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct ConfigVersion<C> {
    pub config: C,
    pub version: u64,
    pub changed_at: jiff::Timestamp,
    pub changed_by: String,
    pub change_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigDiff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub modified: Vec<FieldDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDiff {
    pub path: String,
    pub old_value: serde_json::Value,
    pub new_value: serde_json::Value,
}

#[derive(Debug, Clone, Copy)]
pub enum ImportMode {
    /// Fail if any config already exists
    CreateOnly,
    /// Update existing, create new
    Upsert,
    /// Only update existing, skip new
    UpdateOnly,
    /// Replace all (delete existing not in import)
    Replace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub created: usize,
    pub updated: usize,
    pub skipped: usize,
    pub errors: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy)]
pub enum ExportFormat {
    Json,
    Toml,
    Yaml,
    Csv,
}

// =============================================================================
// CONFIGURATION SERVICE - Unified API
// =============================================================================

/// Main configuration service providing unified access to all config types
pub struct ConfigService {
    /// Curve configurations
    pub curves: Arc<dyn ConfigStore<CurveConfig>>,
    
    /// Bond pricing configurations
    pub bond_pricing: Arc<dyn ConfigStore<BondPricingConfig>>,
    
    /// Price overrides
    pub overrides: Arc<dyn ConfigStore<PriceOverride>>,
    
    /// Market data mappings
    pub market_data: Arc<dyn ConfigStore<MarketDataMapping>>,
    
    /// Spread adjustments
    pub spread_adjustments: Arc<dyn ConfigStore<SpreadAdjustmentConfig>>,
    
    /// Event publisher for config changes
    event_tx: tokio::sync::broadcast::Sender<ConfigEvent>,
}

impl ConfigService {
    /// Subscribe to configuration change events
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<ConfigEvent> {
        self.event_tx.subscribe()
    }
    
    /// Validate a configuration without saving
    pub fn validate<C: Configuration>(&self, config: &C) -> Result<(), ValidationErrors> {
        config.validate()
    }
    
    /// Reload all configurations from storage
    pub async fn reload_all(&self) -> Result<(), ConfigError>;
    
    /// Get all curves required for a bond
    pub async fn curves_for_bond(&self, bond: &Bond) -> Result<Vec<CurveConfig>, ConfigError>;
    
    /// Get effective price (with overrides applied)
    pub async fn get_effective_price(
        &self,
        instrument_id: &InstrumentId,
        model_price: Decimal,
    ) -> Result<EffectivePrice, ConfigError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfigEvent {
    Created { config_type: String, id: String },
    Updated { config_type: String, id: String, version: u64 },
    Deleted { config_type: String, id: String },
    Imported { config_type: String, count: usize },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectivePrice {
    pub price: Decimal,
    pub source: PriceSource,
    pub overrides_applied: Vec<OverrideInfo>,
    pub adjustments_applied: Vec<AdjustmentInfo>,
}
```

### CLI Interface Design (`convex-cli`)

```rust
use clap::{Parser, Subcommand};

/// Convex CLI - Bond Pricing Configuration Management
#[derive(Parser)]
#[command(name = "convex")]
#[command(about = "Production-grade bond pricing configuration management")]
pub struct Cli {
    /// Configuration file path
    #[arg(short, long, env = "CONVEX_CONFIG")]
    pub config: Option<PathBuf>,
    
    /// Output format
    #[arg(short, long, default_value = "table")]
    pub format: OutputFormat,
    
    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,
    
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Curve configuration management
    Curve {
        #[command(subcommand)]
        action: CurveCommands,
    },
    
    /// Bond pricing configuration management
    Pricing {
        #[command(subcommand)]
        action: PricingCommands,
    },
    
    /// Price override management
    Override {
        #[command(subcommand)]
        action: OverrideCommands,
    },
    
    /// Market data mapping management
    MarketData {
        #[command(subcommand)]
        action: MarketDataCommands,
    },
    
    /// Import/Export operations
    Io {
        #[command(subcommand)]
        action: IoCommands,
    },
    
    /// Interactive configuration mode
    Interactive,
    
    /// Validate configuration files
    Validate {
        /// Files to validate
        files: Vec<PathBuf>,
    },
    
    /// Start API server
    Serve {
        #[arg(short, long, default_value = "0.0.0.0:8080")]
        bind: String,
    },
}

#[derive(Subcommand)]
pub enum CurveCommands {
    /// List all curve configurations
    List {
        #[arg(short, long)]
        currency: Option<String>,
        #[arg(short, long)]
        curve_type: Option<String>,
    },
    
    /// Get curve configuration details
    Get {
        /// Curve ID
        id: String,
        /// Show specific version
        #[arg(short, long)]
        version: Option<u64>,
    },
    
    /// Create new curve configuration
    Create {
        /// Path to config file (JSON/TOML/YAML)
        #[arg(short, long)]
        file: Option<PathBuf>,
        /// Create interactively
        #[arg(short, long)]
        interactive: bool,
    },
    
    /// Update curve configuration
    Update {
        /// Curve ID
        id: String,
        /// Path to config file
        #[arg(short, long)]
        file: Option<PathBuf>,
    },
    
    /// Delete curve configuration
    Delete {
        /// Curve ID
        id: String,
        /// Skip confirmation
        #[arg(short, long)]
        force: bool,
    },
    
    /// Show version history
    History {
        /// Curve ID
        id: String,
    },
    
    /// Diff two versions
    Diff {
        /// Curve ID
        id: String,
        /// First version
        v1: u64,
        /// Second version
        v2: u64,
    },
    
    /// Build and preview curve
    Build {
        /// Curve ID
        id: String,
        /// As-of timestamp
        #[arg(short, long)]
        as_of: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum OverrideCommands {
    /// List active overrides
    List {
        #[arg(short, long)]
        expired: bool,
    },
    
    /// Set price override
    Set {
        /// Instrument ID (ISIN, CUSIP, etc.)
        instrument: String,
        /// Override type: price, yield, spread, adjustment
        #[arg(short = 't', long)]
        override_type: String,
        /// Value
        value: f64,
        /// Reason for override
        #[arg(short, long)]
        reason: String,
        /// Expiry (ISO timestamp or duration like "1d", "4h")
        #[arg(short, long)]
        expiry: Option<String>,
    },
    
    /// Remove override
    Remove {
        /// Instrument ID
        instrument: String,
    },
    
    /// Clear all expired overrides
    ClearExpired,
}

#[derive(Subcommand)]
pub enum IoCommands {
    /// Import configurations from file
    Import {
        /// Input file path
        file: PathBuf,
        /// Import mode: create, upsert, update, replace
        #[arg(short, long, default_value = "upsert")]
        mode: String,
        /// Dry run (validate only)
        #[arg(short, long)]
        dry_run: bool,
    },
    
    /// Export configurations to file
    Export {
        /// Output file path
        file: PathBuf,
        /// Config types to export (curves, pricing, overrides, all)
        #[arg(short = 't', long, default_value = "all")]
        types: String,
        /// Filter by tags
        #[arg(long)]
        tags: Option<Vec<String>>,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum OutputFormat {
    Table,
    Json,
    Yaml,
    Csv,
}

// =============================================================================
// CLI EXAMPLES
// =============================================================================

/*
# List all USD curves
convex curve list --currency USD

# Get curve details with history
convex curve get USD.GOVT
convex curve history USD.GOVT

# Create curve from file
convex curve create --file curves/usd_govt.toml

# Interactive curve creation
convex curve create --interactive

# Build and preview curve
convex curve build USD.GOVT --as-of 2025-01-15T16:00:00Z

# Set price override
convex override set US037833DV96 --type spread --value 125.0 \
    --reason "Illiquid, +15bps to model" --expiry 1d

# List active overrides
convex override list

# Import configurations
convex io import ./config/production.toml --mode upsert --dry-run
convex io import ./config/production.toml --mode upsert

# Export all configurations
convex io export ./backup/config_$(date +%Y%m%d).json --types all

# Start API server
convex serve --bind 0.0.0.0:8080

# Interactive mode
convex interactive
*/
```

### REST API Design (`convex-api`)

```rust
use axum::{
    Router,
    routing::{get, post, put, delete},
    extract::{Path, Query, State, Json},
    response::IntoResponse,
};

/// API Router construction
pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Curve endpoints
        .route("/api/v1/curves", get(list_curves).post(create_curve))
        .route("/api/v1/curves/:id", get(get_curve).put(update_curve).delete(delete_curve))
        .route("/api/v1/curves/:id/history", get(curve_history))
        .route("/api/v1/curves/:id/diff", get(curve_diff))
        .route("/api/v1/curves/:id/build", post(build_curve))
        .route("/api/v1/curves/:id/validate", post(validate_curve))
        
        // Pricing config endpoints
        .route("/api/v1/pricing", get(list_pricing).post(create_pricing))
        .route("/api/v1/pricing/:id", get(get_pricing).put(update_pricing).delete(delete_pricing))
        
        // Override endpoints
        .route("/api/v1/overrides", get(list_overrides).post(create_override))
        .route("/api/v1/overrides/:instrument", get(get_override).delete(delete_override))
        .route("/api/v1/overrides/expired", delete(clear_expired_overrides))
        
        // Market data mapping endpoints
        .route("/api/v1/market-data", get(list_market_data).post(create_market_data))
        .route("/api/v1/market-data/:id", get(get_market_data).put(update_market_data))
        
        // Spread adjustment endpoints
        .route("/api/v1/adjustments", get(list_adjustments).post(create_adjustment))
        .route("/api/v1/adjustments/:id", get(get_adjustment).put(update_adjustment).delete(delete_adjustment))
        
        // Bulk operations
        .route("/api/v1/import", post(import_configs))
        .route("/api/v1/export", get(export_configs))
        .route("/api/v1/validate", post(validate_configs))
        
        // Real-time updates (WebSocket)
        .route("/api/v1/ws/events", get(ws_events_handler))
        
        // Health and metrics
        .route("/health", get(health_check))
        .route("/metrics", get(metrics_handler))
        
        .with_state(state)
}

// =============================================================================
// API SCHEMAS (OpenAPI compatible)
// =============================================================================

/// API response wrapper
#[derive(Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<ApiError>,
    pub meta: Option<ResponseMeta>,
}

#[derive(Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    pub details: Option<Vec<ValidationError>>,
}

#[derive(Serialize, Deserialize)]
pub struct ResponseMeta {
    pub total: Option<usize>,
    pub page: Option<usize>,
    pub per_page: Option<usize>,
    pub version: Option<u64>,
}

/// Curve creation request
#[derive(Serialize, Deserialize)]
pub struct CreateCurveRequest {
    pub curve_id: String,
    pub curve_type: CurveType,
    pub currency: String,
    pub bootstrap_instruments: Vec<BootstrapInstrument>,
    pub segments: Vec<CurveSegment>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
}

/// Override creation request
#[derive(Serialize, Deserialize)]
pub struct CreateOverrideRequest {
    pub instrument_id: String,
    pub override_type: OverrideType,
    pub value: f64,
    pub reason: String,
    pub expiry: Option<String>,
}

// =============================================================================
// OPENAPI SPEC GENERATION
// =============================================================================

/*
OpenAPI spec is auto-generated using utoipa crate.
Access at: GET /api/v1/openapi.json
Swagger UI at: GET /api/v1/docs

Example endpoints:

GET /api/v1/curves
  Query params: currency, curve_type, enabled, limit, offset
  Response: ApiResponse<Vec<CurveConfig>>

POST /api/v1/curves
  Body: CreateCurveRequest
  Response: ApiResponse<CurveConfig>

GET /api/v1/curves/{id}
  Query params: version (optional)
  Response: ApiResponse<CurveConfig>

POST /api/v1/curves/{id}/build
  Body: { "as_of": "2025-01-15T16:00:00Z" }
  Response: ApiResponse<BuiltCurve>

WebSocket /api/v1/ws/events
  Sends: ConfigEvent (JSON)
  Use for real-time UI updates
*/
```

### UI Integration Points

```rust
// =============================================================================
// UI REQUIREMENTS
// =============================================================================

/*
The configuration UI should consume the REST API and WebSocket for real-time updates.

Required UI Views:

1. CURVE MANAGEMENT
   - List view with filtering (currency, type, status)
   - Detail view with version history
   - Form for create/edit with validation feedback
   - Visual curve preview after build
   - Diff view for comparing versions

2. BOND PRICING CONFIG
   - List view grouped by bond type
   - Config editor with benchmark curve selection
   - Spread adjustment management
   - Pricing hierarchy drag-and-drop ordering

3. PRICE OVERRIDES
   - Active overrides dashboard
   - Quick override creation form
   - Expiry countdown display
   - Bulk operations (extend, remove)

4. MARKET DATA MAPPING
   - Source configuration
   - Mapping table (internal key -> external key)
   - Health/connectivity status

5. IMPORT/EXPORT
   - File upload with preview
   - Validation results display
   - Conflict resolution UI
   - Export wizard

6. REAL-TIME FEATURES
   - Subscribe to ConfigEvent via WebSocket
   - Live update indicators
   - Optimistic UI updates
   - Conflict detection

API Contracts for UI:

All list endpoints support:
- ?limit=N&offset=M for pagination
- ?search=X for text search
- ?sort=field&order=asc|desc

All mutation endpoints return:
- Updated entity with new version
- Validation errors if failed
- Optimistic locking via version field

WebSocket protocol:
- Connect to /api/v1/ws/events
- Receive JSON ConfigEvent messages
- Reconnect with exponential backoff
*/
```
```
```

### 3. Storage Abstraction (`convex-storage`)

The storage layer uses a **trait-based design** allowing pluggable backends. The framework ships with an embedded `redb` implementation as the default. External database implementations (PostgreSQL, MongoDB, TimescaleDB, etc.) are **out of scope** - users implement the `StorageAdapter` trait for their preferred backend.

```rust
use async_trait::async_trait;

/// Storage backend type for configuration
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StorageBackendConfig {
    /// Embedded redb (default, included in framework)
    Redb { path: std::path::PathBuf },
    /// External PostgreSQL (user-implemented)
    Postgres { connection_string: String },
    /// External MongoDB (user-implemented)  
    Mongo { connection_string: String, database: String },
    /// External TimescaleDB (user-implemented)
    Timescale { connection_string: String },
    /// Custom user-provided backend
    Custom { provider_name: String, config: toml::Value },
}

/// Core storage trait - implement this for external databases
/// 
/// # Out of Scope
/// PostgreSQL, MongoDB, TimescaleDB implementations are NOT provided.
/// Users requiring these backends implement this trait themselves.
/// 
/// # Provided Implementations
/// - `RedbStorage` - Embedded, pure Rust, zero external dependencies
#[async_trait]
pub trait StorageAdapter: Send + Sync {
    /// Get the backend name for logging/metrics
    fn backend_name(&self) -> &'static str;
    
    /// Health check
    async fn is_healthy(&self) -> bool;
    
    // ==================== Security Master ====================
    async fn get_security(&self, id: &InstrumentId) -> Result<Option<SecurityDefinition>>;
    async fn put_security(&self, security: &SecurityDefinition) -> Result<()>;
    async fn list_securities(&self, filter: &SecurityFilter) -> Result<Vec<SecurityDefinition>>;
    
    // ==================== Quote History ====================
    async fn get_quotes(
        &self,
        id: &InstrumentId,
        range: TimeRange,
    ) -> Result<Vec<Quote>>;
    async fn append_quote(&self, quote: &Quote) -> Result<()>;
    async fn append_quotes_batch(&self, quotes: &[Quote]) -> Result<()>;
    
    // ==================== Curve Snapshots ====================
    async fn get_curve(&self, curve_id: &str, as_of: jiff::Timestamp) -> Result<Option<CurveSnapshot>>;
    async fn put_curve(&self, curve: &CurveSnapshot) -> Result<()>;
    async fn list_curve_versions(&self, curve_id: &str, limit: usize) -> Result<Vec<CurveVersion>>;
    
    // ==================== Calculation Cache (Optional) ====================
    async fn get_cached_calc(&self, key: &CalcCacheKey) -> Result<Option<CachedCalculation>> {
        Ok(None) // Default: no persistent calc cache
    }
    async fn put_cached_calc(&self, key: &CalcCacheKey, value: &CachedCalculation) -> Result<()> {
        Ok(()) // Default: no-op
    }
}

/// Factory for creating storage adapters from config
pub struct StorageFactory;

impl StorageFactory {
    /// Create storage adapter from configuration
    /// 
    /// Only `Redb` is implemented in-framework. Other backends return
    /// an error indicating user must provide their own implementation.
    pub fn create(config: &StorageBackendConfig) -> Result<Arc<dyn StorageAdapter>> {
        match config {
            StorageBackendConfig::Redb { path } => {
                Ok(Arc::new(RedbStorage::open(path)?))
            }
            StorageBackendConfig::Postgres { .. } => {
                Err(StorageError::NotImplemented(
                    "PostgreSQL adapter not included. Implement StorageAdapter trait for your backend."
                ))
            }
            StorageBackendConfig::Mongo { .. } => {
                Err(StorageError::NotImplemented(
                    "MongoDB adapter not included. Implement StorageAdapter trait for your backend."
                ))
            }
            StorageBackendConfig::Timescale { .. } => {
                Err(StorageError::NotImplemented(
                    "TimescaleDB adapter not included. Implement StorageAdapter trait for your backend."
                ))
            }
            StorageBackendConfig::Custom { provider_name, .. } => {
                Err(StorageError::NotImplemented(
                    format!("Custom provider '{}' must be registered via StorageFactory::register()", provider_name)
                ))
            }
        }
    }
    
    /// Register a custom storage adapter implementation
    pub fn register<T: StorageAdapter + 'static>(
        name: &str,
        factory_fn: fn(&toml::Value) -> Result<T>
    ) {
        // Plugin registration for user-provided backends
    }
}

// =============================================================================
// REDB IMPLEMENTATION (Included in Framework)
// =============================================================================

/// Primary storage implementation using redb (pure Rust, stable file format)
/// 
/// This is the DEFAULT and ONLY storage backend included in the Convex framework.
/// For production deployments requiring PostgreSQL, MongoDB, etc., implement
/// the `StorageAdapter` trait for your chosen database.
pub struct RedbStorage {
    db: redb::Database,
}

impl RedbStorage {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let db = redb::Database::create(path)?;
        Ok(Self { db })
    }
}

// Table definitions for redb
const SECURITIES: redb::TableDefinition<&str, &[u8]> = 
    redb::TableDefinition::new("securities");
const QUOTES: redb::MultimapTableDefinition<&str, &[u8]> = 
    redb::MultimapTableDefinition::new("quotes");
const CURVES: redb::TableDefinition<&str, &[u8]> = 
    redb::TableDefinition::new("curves");

#[async_trait]
impl StorageAdapter for RedbStorage {
    fn backend_name(&self) -> &'static str {
        "redb"
    }
    
    async fn is_healthy(&self) -> bool {
        self.db.begin_read().is_ok()
    }
    
    async fn get_security(&self, id: &InstrumentId) -> Result<Option<SecurityDefinition>> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(SECURITIES)?;
        
        if let Some(bytes) = table.get(id.as_key())? {
            // Zero-copy deserialize with rkyv
            let archived = rkyv::access::<ArchivedSecurityDefinition, rkyv::rancor::Error>(bytes.value())?;
            Ok(Some(rkyv::deserialize::<SecurityDefinition, _>(archived)?))
        } else {
            Ok(None)
        }
    }
    
    async fn put_security(&self, security: &SecurityDefinition) -> Result<()> {
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(security)?;
        
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(SECURITIES)?;
            table.insert(security.id.as_key(), bytes.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }
    
    async fn list_securities(&self, filter: &SecurityFilter) -> Result<Vec<SecurityDefinition>> {
        // Implementation with filtering
        todo!()
    }
    
    async fn get_quotes(&self, id: &InstrumentId, range: TimeRange) -> Result<Vec<Quote>> {
        // Implementation
        todo!()
    }
    
    async fn append_quote(&self, quote: &Quote) -> Result<()> {
        // Implementation
        todo!()
    }
    
    async fn append_quotes_batch(&self, quotes: &[Quote]) -> Result<()> {
        // Batch implementation for efficiency
        todo!()
    }
    
    async fn get_curve(&self, curve_id: &str, as_of: jiff::Timestamp) -> Result<Option<CurveSnapshot>> {
        // Implementation
        todo!()
    }
    
    async fn put_curve(&self, curve: &CurveSnapshot) -> Result<()> {
        // Implementation
        todo!()
    }
    
    async fn list_curve_versions(&self, curve_id: &str, limit: usize) -> Result<Vec<CurveVersion>> {
        // Implementation
        todo!()
    }
}

// =============================================================================
// EXAMPLE: User-Implemented PostgreSQL Adapter (OUT OF SCOPE)
// =============================================================================
// 
// Users requiring PostgreSQL would implement something like:
//
// ```rust
// // In user's crate, NOT in convex-storage
// use convex_storage::{StorageAdapter, StorageFactory};
// use sqlx::PgPool;
// 
// pub struct PostgresStorage {
//     pool: PgPool,
// }
// 
// #[async_trait]
// impl StorageAdapter for PostgresStorage {
//     fn backend_name(&self) -> &'static str { "postgres" }
//     // ... implement all trait methods
// }
// 
// // Register with factory
// StorageFactory::register("postgres", |config| {
//     let conn_str = config.get("connection_string")?;
//     PostgresStorage::new(conn_str)
// });
// ```
```

### 4. Transport Layer - Zero-Copy Binary Encoding (`convex-transport`)

```rust
use crossbeam::queue::ArrayQueue;
use std::sync::Arc;

/// Lock-free SPSC ring buffer for market data
pub struct MarketDataRingBuffer {
    buffer: Arc<ArrayQueue<QuoteUpdate>>,
    sequence: std::sync::atomic::AtomicU64,
}

impl MarketDataRingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Arc::new(ArrayQueue::new(capacity)),
            sequence: std::sync::atomic::AtomicU64::new(0),
        }
    }
    
    /// Push with backpressure handling
    pub fn push(&self, update: QuoteUpdate) -> Result<u64, BackpressureError> {
        match self.buffer.push(update) {
            Ok(()) => {
                let seq = self.sequence.fetch_add(1, Ordering::Release);
                Ok(seq)
            }
            Err(_) => Err(BackpressureError::BufferFull),
        }
    }
    
    /// Non-blocking pop
    pub fn pop(&self) -> Option<QuoteUpdate> {
        self.buffer.pop()
    }
}

/// Compact binary format for quotes (no allocation on hot path)
#[repr(C, packed)]
#[derive(Clone, Copy, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct PackedQuote {
    pub instrument_id: u64,       // Internal ID
    pub bid: i64,                 // Fixed-point (8 decimal places)
    pub ask: i64,                 // Fixed-point
    pub timestamp_nanos: i64,     // Unix nanos
    pub flags: u16,               // Bit flags for optional fields
}

impl PackedQuote {
    const SCALE: i64 = 100_000_000; // 8 decimal places
    
    pub fn from_quote(quote: &Quote, id_map: &InstrumentIdMap) -> Option<Self> {
        Some(Self {
            instrument_id: id_map.get_internal_id(&quote.instrument_id)?,
            bid: quote.bid.map(|d| (d * Decimal::from(Self::SCALE)).to_i64().unwrap()).unwrap_or(i64::MIN),
            ask: quote.ask.map(|d| (d * Decimal::from(Self::SCALE)).to_i64().unwrap()).unwrap_or(i64::MIN),
            timestamp_nanos: quote.timestamp.as_nanosecond(),
            flags: 0,
        })
    }
    
    pub fn to_quote(&self, id_map: &InstrumentIdMap) -> Option<Quote> {
        // Reconstruct Quote from packed format
        // ...
    }
}
```

### 5. Real-Time Streaming Infrastructure (`convex-streaming`)

The streaming layer provides **trait-based abstractions** for real-time data flow across ALL data types: quotes, curves, prices, analytics. The framework includes the infrastructure; users implement adapters for their specific vendors.

```rust
use async_trait::async_trait;
use tokio::sync::broadcast;

// =============================================================================
// STREAM DATA TYPES - Not just prices, but curves and analytics too
// =============================================================================

/// All streamable data types in the system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamEvent {
    /// Market quote update
    Quote(Quote),
    
    /// Curve update (after rebuild)
    CurveUpdate(CurveSnapshot),
    
    /// Bond price update (calculated)
    BondPrice(BondPriceUpdate),
    
    /// Portfolio/ETF analytics update
    PortfolioAnalytics(PortfolioAnalyticsUpdate),
    
    /// Risk metric update
    RiskUpdate(RiskMetricUpdate),
    
    /// Configuration change
    ConfigChange(ConfigChangeEvent),
    
    /// System health/status
    SystemStatus(SystemStatusEvent),
}

/// Curve snapshot for streaming/debugging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveSnapshot {
    pub curve_id: CurveId,
    pub as_of: jiff::Timestamp,
    pub build_time: jiff::Timestamp,
    pub build_duration_us: u64,
    pub points: Vec<CurvePoint>,
    pub input_instruments: Vec<CurveInputSnapshot>,
    pub interpolation_method: String,
    pub checksum: String,  // For detecting changes
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurvePoint {
    pub tenor: String,
    pub tenor_years: Decimal,
    pub zero_rate: Decimal,
    pub discount_factor: Decimal,
    pub forward_rate: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveInputSnapshot {
    pub instrument_type: String,
    pub tenor: String,
    pub market_data_key: String,
    pub value: Decimal,
    pub source: String,
    pub timestamp: jiff::Timestamp,
}

/// Bond price update with full breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondPriceUpdate {
    pub instrument_id: InstrumentId,
    pub timestamp: jiff::Timestamp,
    pub calculation_id: String,  // For tracing
    
    // Prices
    pub clean_price: Decimal,
    pub dirty_price: Decimal,
    pub accrued_interest: Decimal,
    
    // Yields
    pub ytm: Option<Decimal>,
    pub ytw: Option<Decimal>,
    
    // Spreads
    pub g_spread: Option<Decimal>,
    pub z_spread: Option<Decimal>,
    pub oas: Option<Decimal>,
    
    // Risk
    pub modified_duration: Option<Decimal>,
    pub dv01: Option<Decimal>,
    
    // Source attribution
    pub pricing_source: PricingSourceUsed,
    pub curves_used: Vec<CurveId>,
    
    // For debugging
    pub calculation_duration_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PricingSourceUsed {
    ManualOverride { entered_by: String, reason: String },
    ExecutableQuote { venue: String, quote_id: String },
    IndicativeQuote { dealer: String },
    CompositePrice { sources: Vec<String> },
    ModelFromSpread { spread_type: String, spread_value: Decimal },
    Stale { original_timestamp: jiff::Timestamp, adjustment_bps: Decimal },
}

// =============================================================================
// STREAM SOURCE ABSTRACTION
// =============================================================================

/// Generic stream source trait - implement for each data vendor
#[async_trait]
pub trait StreamSource: Send + Sync {
    type Item: Send + Clone;
    type Error: std::error::Error + Send + Sync;
    
    /// Subscribe to a stream of items
    async fn subscribe(&self) -> Result<StreamReceiver<Self::Item>, Self::Error>;
    
    /// Health check
    async fn is_connected(&self) -> bool;
    
    /// Reconnect if disconnected
    async fn reconnect(&self) -> Result<(), Self::Error>;
    
    /// Source name for logging/metrics
    fn name(&self) -> &'static str;
}

/// Receiver handle for stream items
pub struct StreamReceiver<T> {
    inner: broadcast::Receiver<T>,
    source_name: String,
}

// =============================================================================
// STREAM SINK ABSTRACTION (Publishing)
// =============================================================================

/// Generic stream sink trait - implement for each distribution channel
#[async_trait]
pub trait StreamSink: Send + Sync {
    type Item: Send;
    type Error: std::error::Error + Send + Sync;
    
    /// Publish an item
    async fn publish(&self, item: Self::Item) -> Result<(), Self::Error>;
    
    /// Publish batch (for efficiency)
    async fn publish_batch(&self, items: Vec<Self::Item>) -> Result<(), Self::Error> {
        for item in items {
            self.publish(item).await?;
        }
        Ok(())
    }
    
    /// Flush any buffered items
    async fn flush(&self) -> Result<(), Self::Error>;
    
    /// Sink name for logging/metrics
    fn name(&self) -> &'static str;
}

// =============================================================================
// SPECIALIZED PUBLISHERS
// =============================================================================

/// Bond price publisher
#[async_trait]
pub trait BondPricePublisher: StreamSink<Item = BondPriceUpdate> {
    /// Publish single bond price
    async fn publish_bond_price(&self, update: BondPriceUpdate) -> Result<(), Self::Error>;
    
    /// Publish batch of bond prices
    async fn publish_bond_prices(&self, updates: Vec<BondPriceUpdate>) -> Result<(), Self::Error>;
}

/// Curve publisher
#[async_trait]
pub trait CurvePublisher: StreamSink<Item = CurveSnapshot> {
    /// Publish curve update
    async fn publish_curve(&self, curve: CurveSnapshot) -> Result<(), Self::Error>;
}

/// Portfolio/ETF analytics publisher
#[async_trait]
pub trait PortfolioPublisher: StreamSink<Item = PortfolioAnalyticsUpdate> {
    /// Publish iNAV update (typically every 15 seconds)
    async fn publish_inav(&self, etf_id: &str, inav: &InavMetrics) -> Result<(), Self::Error>;
    
    /// Publish NAV (end of day)
    async fn publish_nav(&self, etf_id: &str, nav: &NavMetrics) -> Result<(), Self::Error>;
    
    /// Publish full portfolio analytics
    async fn publish_analytics(&self, update: PortfolioAnalyticsUpdate) -> Result<(), Self::Error>;
}

// =============================================================================
// DEBUGGING & DIAGNOSTICS (`convex-debug`)
// =============================================================================

/// Diagnostic snapshot for troubleshooting pricing issues
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingDiagnostic {
    pub diagnostic_id: String,
    pub timestamp: jiff::Timestamp,
    pub instrument_id: InstrumentId,
    
    // Bond reference data snapshot
    pub bond_snapshot: BondSnapshot,
    
    // All curves used
    pub curves: Vec<CurveSnapshot>,
    
    // Market data inputs
    pub market_data: Vec<MarketDataSnapshot>,
    
    // Calculation steps (for debugging)
    pub calculation_steps: Vec<CalculationStep>,
    
    // Final results
    pub results: PricingResults,
    
    // Timing breakdown
    pub timing: TimingBreakdown,
    
    // Any warnings or issues
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondSnapshot {
    pub instrument_id: InstrumentId,
    pub isin: Option<String>,
    pub cusip: Option<String>,
    pub issuer: String,
    pub coupon: Decimal,
    pub coupon_frequency: u8,
    pub day_count: String,
    pub maturity_date: String,
    pub issue_date: String,
    pub settlement_date: String,
    pub first_coupon_date: Option<String>,
    pub call_schedule: Option<Vec<CallDate>>,
    pub currency: String,
    pub face_value: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketDataSnapshot {
    pub key: String,
    pub value: Decimal,
    pub source: String,
    pub timestamp: jiff::Timestamp,
    pub staleness_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalculationStep {
    pub step_number: u32,
    pub step_name: String,
    pub description: String,
    pub inputs: serde_json::Value,
    pub output: serde_json::Value,
    pub duration_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingResults {
    pub clean_price: Decimal,
    pub dirty_price: Decimal,
    pub accrued_interest: Decimal,
    pub ytm: Option<Decimal>,
    pub modified_duration: Option<Decimal>,
    pub z_spread: Option<Decimal>,
    // ... other results
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingBreakdown {
    pub total_us: u64,
    pub curve_lookup_us: u64,
    pub cashflow_generation_us: u64,
    pub discounting_us: u64,
    pub yield_solve_us: u64,
    pub spread_calculation_us: u64,
    pub risk_calculation_us: u64,
}

/// Diagnostic service for troubleshooting
pub trait DiagnosticService: Send + Sync {
    /// Capture full diagnostic snapshot for a pricing
    fn capture_diagnostic(
        &self,
        instrument_id: &InstrumentId,
        include_curves: bool,
        include_market_data: bool,
    ) -> Result<PricingDiagnostic, DiagnosticError>;
    
    /// Replay a pricing from a diagnostic snapshot
    fn replay_pricing(
        &self,
        diagnostic: &PricingDiagnostic,
    ) -> Result<PricingResults, DiagnosticError>;
    
    /// Compare two diagnostics (for debugging differences)
    fn compare_diagnostics(
        &self,
        before: &PricingDiagnostic,
        after: &PricingDiagnostic,
    ) -> DiagnosticDiff;
    
    /// Export diagnostic to file
    fn export_diagnostic(
        &self,
        diagnostic: &PricingDiagnostic,
        path: &Path,
        format: ExportFormat,
    ) -> Result<(), DiagnosticError>;
    
    /// Import diagnostic from file
    fn import_diagnostic(
        &self,
        path: &Path,
    ) -> Result<PricingDiagnostic, DiagnosticError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticDiff {
    pub bond_changes: Vec<FieldDiff>,
    pub curve_changes: Vec<CurveDiff>,
    pub market_data_changes: Vec<MarketDataDiff>,
    pub result_changes: Vec<FieldDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDiff {
    pub field: String,
    pub before: String,
    pub after: String,
    pub change_pct: Option<Decimal>,
}

// =============================================================================
// CURVE SERIALIZATION FOR DEBUGGING
// =============================================================================

/// Serialize a curve for debugging/storage
pub trait CurveSerializer {
    /// Serialize curve to JSON (human-readable)
    fn to_json(&self, curve: &BuiltCurve) -> Result<String, SerializeError>;
    
    /// Serialize curve to binary (compact)
    fn to_binary(&self, curve: &BuiltCurve) -> Result<Vec<u8>, SerializeError>;
    
    /// Deserialize curve from JSON
    fn from_json(&self, json: &str) -> Result<BuiltCurve, SerializeError>;
    
    /// Deserialize curve from binary
    fn from_binary(&self, bytes: &[u8]) -> Result<BuiltCurve, SerializeError>;
    
    /// Generate curve checksum (for change detection)
    fn checksum(&self, curve: &BuiltCurve) -> String;
}

/// Curve history for debugging
pub struct CurveHistory {
    storage: Arc<dyn CurveHistoryStorage>,
}

impl CurveHistory {
    /// Store a curve snapshot
    pub async fn store(&self, curve: &CurveSnapshot) -> Result<(), StorageError>;
    
    /// Get curve at specific time
    pub async fn get_at(&self, curve_id: &CurveId, as_of: jiff::Timestamp) -> Result<Option<CurveSnapshot>, StorageError>;
    
    /// Get curve history for a date range
    pub async fn get_range(
        &self,
        curve_id: &CurveId,
        from: jiff::Timestamp,
        to: jiff::Timestamp,
    ) -> Result<Vec<CurveSnapshot>, StorageError>;
    
    /// Compare curve at two points in time
    pub async fn compare(
        &self,
        curve_id: &CurveId,
        time1: jiff::Timestamp,
        time2: jiff::Timestamp,
    ) -> Result<CurveDiff, StorageError>;
}

// =============================================================================
// CLI COMMANDS FOR DEBUGGING
// =============================================================================

/*
# Capture diagnostic for a bond
convex debug capture US037833DV9 --include-curves --include-market-data --output diag.json

# Replay pricing from diagnostic
convex debug replay diag.json --show-steps

# Compare two diagnostics
convex debug diff diag_before.json diag_after.json

# Dump current curve
convex curve dump USD.GOVT --format json --output usd_govt_curve.json

# Compare curve at two times
convex curve compare USD.GOVT --at "2025-01-15T10:00:00Z" --with "2025-01-15T16:00:00Z"

# Show curve history
convex curve history USD.GOVT --from "2025-01-14" --to "2025-01-15"

# Validate pricing against Bloomberg
convex debug validate US037833DV9 --bbg-price 98.50 --bbg-yield 4.25

# Export all curves for a date
convex curve export-all --as-of "2025-01-15T16:00:00Z" --output curves_eod.json
*/

// =============================================================================
// CONFLATION & THROTTLING
// =============================================================================

/// Conflation strategy for high-frequency updates
pub struct Conflator<T> {
    window: std::time::Duration,
    latest: DashMap<String, T>,
    last_emit: DashMap<String, jiff::Timestamp>,
}

impl<T: Clone> Conflator<T> {
    pub fn new(window: std::time::Duration) -> Self {
        Self {
            window,
            latest: DashMap::new(),
            last_emit: DashMap::new(),
        }
    }
    
    /// Update with new value, returns Some if should emit
    pub fn update(&self, key: &str, value: T) -> Option<T> {
        self.latest.insert(key.to_string(), value.clone());
        
        let now = jiff::Timestamp::now();
        let should_emit = self.last_emit
            .get(key)
            .map(|last| now.duration_since(*last).unwrap() >= self.window.into())
            .unwrap_or(true);
        
        if should_emit {
            self.last_emit.insert(key.to_string(), now);
            Some(value)
        } else {
            None
        }
    }
}

/// Rate limiter for publishing
pub struct RateLimiter {
    permits_per_second: u32,
    bucket: std::sync::atomic::AtomicU32,
    last_refill: std::sync::atomic::AtomicI64,
}

impl RateLimiter {
    pub fn new(permits_per_second: u32) -> Self {
        Self {
            permits_per_second,
            bucket: std::sync::atomic::AtomicU32::new(permits_per_second),
            last_refill: std::sync::atomic::AtomicI64::new(jiff::Timestamp::now().as_millisecond()),
        }
    }
    
    /// Try to acquire a permit, returns false if rate limited
    pub fn try_acquire(&self) -> bool {
        self.refill();
        self.bucket.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
            if current > 0 { Some(current - 1) } else { None }
        }).is_ok()
    }
    
    fn refill(&self) {
        // Refill logic based on elapsed time
    }
}

// =============================================================================
// REAL-TIME CALCULATION PIPELINE
// =============================================================================

/// Orchestrates real-time calculation flow for bonds, portfolios, curves
pub struct CalculationPipeline {
    /// Market data sources (pluggable)
    sources: Vec<Arc<dyn StreamSource<Item = Quote, Error = MarketDataError>>>,
    
    /// Calculation engine
    calc_engine: Arc<CalculationGraph>,
    
    /// Publishers by type
    bond_publishers: Vec<Arc<dyn BondPricePublisher<Error = PublishError>>>,
    curve_publishers: Vec<Arc<dyn CurvePublisher<Error = PublishError>>>,
    portfolio_publishers: Vec<Arc<dyn PortfolioPublisher<Error = PublishError>>>,
    
    /// Diagnostic service
    diagnostics: Arc<dyn DiagnosticService>,
    
    /// Conflation settings
    conflation_window: std::time::Duration,
    
    /// Metrics
    metrics: Arc<PipelineMetrics>,
    
    /// Debug mode (capture all diagnostics)
    debug_mode: bool,
}

impl CalculationPipeline {
    pub fn builder() -> CalculationPipelineBuilder {
        CalculationPipelineBuilder::default()
    }
    
    /// Start the pipeline
    pub async fn run(&self, shutdown: tokio::sync::watch::Receiver<bool>) -> Result<()> {
        // 1. Subscribe to all sources
        // 2. Merge streams
        // 3. Apply conflation
        // 4. Trigger calculations on update
        // 5. Publish results to appropriate publishers
        // 6. Capture diagnostics if debug mode enabled
        // 7. Handle errors and reconnection
        todo!()
    }
    
    /// Enable/disable debug mode at runtime
    pub fn set_debug_mode(&mut self, enabled: bool) {
        self.debug_mode = enabled;
    }
    
    /// Get current pipeline status
    pub fn status(&self) -> PipelineStatus {
        PipelineStatus {
            sources_connected: self.sources.iter().filter(|s| /* is_connected */).count(),
            total_sources: self.sources.len(),
            messages_processed: self.metrics.messages_processed.load(Ordering::Relaxed),
            calculations_performed: self.metrics.calculations_performed.load(Ordering::Relaxed),
            errors: self.metrics.errors.load(Ordering::Relaxed),
            debug_mode: self.debug_mode,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStatus {
    pub sources_connected: usize,
    pub total_sources: usize,
    pub messages_processed: u64,
    pub calculations_performed: u64,
    pub errors: u64,
    pub debug_mode: bool,
}

/// Builder for calculation pipeline
#[derive(Default)]
pub struct CalculationPipelineBuilder {
    sources: Vec<Arc<dyn StreamSource<Item = Quote, Error = MarketDataError>>>,
    bond_publishers: Vec<Arc<dyn BondPricePublisher<Error = PublishError>>>,
    curve_publishers: Vec<Arc<dyn CurvePublisher<Error = PublishError>>>,
    portfolio_publishers: Vec<Arc<dyn PortfolioPublisher<Error = PublishError>>>,
    conflation_window: Option<std::time::Duration>,
    debug_mode: bool,
}

impl CalculationPipelineBuilder {
    pub fn add_source<S>(mut self, source: S) -> Self 
    where 
        S: StreamSource<Item = Quote, Error = MarketDataError> + 'static 
    {
        self.sources.push(Arc::new(source));
        self
    }
    
    pub fn add_bond_publisher<P>(mut self, publisher: P) -> Self
    where
        P: BondPricePublisher<Error = PublishError> + 'static
    {
        self.bond_publishers.push(Arc::new(publisher));
        self
    }
    
    pub fn add_curve_publisher<P>(mut self, publisher: P) -> Self
    where
        P: CurvePublisher<Error = PublishError> + 'static
    {
        self.curve_publishers.push(Arc::new(publisher));
        self
    }
    
    pub fn add_portfolio_publisher<P>(mut self, publisher: P) -> Self
    where
        P: PortfolioPublisher<Error = PublishError> + 'static
    {
        self.portfolio_publishers.push(Arc::new(publisher));
        self
    }
    
    pub fn conflation_window(mut self, window: std::time::Duration) -> Self {
        self.conflation_window = Some(window);
        self
    }
    
    pub fn debug_mode(mut self, enabled: bool) -> Self {
        self.debug_mode = enabled;
        self
    }
    
    pub fn build(
        self, 
        calc_engine: Arc<CalculationGraph>,
        diagnostics: Arc<dyn DiagnosticService>,
    ) -> CalculationPipeline {
        CalculationPipeline {
            sources: self.sources,
            calc_engine,
            bond_publishers: self.bond_publishers,
            curve_publishers: self.curve_publishers,
            portfolio_publishers: self.portfolio_publishers,
            diagnostics,
            conflation_window: self.conflation_window.unwrap_or(std::time::Duration::from_millis(100)),
            metrics: Arc::new(PipelineMetrics::default()),
            debug_mode: self.debug_mode,
        }
    }
}

// =============================================================================
// FILE-BASED SINK FOR DEBUGGING
// =============================================================================

/// File sink that writes all events to disk for debugging
pub struct FileSink {
    base_path: PathBuf,
    rotate_size_mb: u64,
    compress: bool,
    current_file: parking_lot::Mutex<Option<std::fs::File>>,
}

impl FileSink {
    pub fn new(base_path: PathBuf, rotate_size_mb: u64, compress: bool) -> Self {
        Self {
            base_path,
            rotate_size_mb,
            compress,
            current_file: parking_lot::Mutex::new(None),
        }
    }
}

#[async_trait]
impl StreamSink for FileSink {
    type Item = StreamEvent;
    type Error = std::io::Error;
    
    async fn publish(&self, item: Self::Item) -> Result<(), Self::Error> {
        let json = serde_json::to_string(&item)?;
        // Write to file with rotation logic
        Ok(())
    }
    
    async fn flush(&self) -> Result<(), Self::Error> {
        // Flush file
        Ok(())
    }
    
    fn name(&self) -> &'static str { "file_sink" }
}

// =============================================================================
// EXAMPLE: Mock Implementations (Included for Testing)
// =============================================================================

/// Mock market data source for testing
pub struct MockMarketDataSource {
    quotes: Arc<parking_lot::RwLock<Vec<Quote>>>,
    tx: broadcast::Sender<Quote>,
}

#[async_trait]
impl StreamSource for MockMarketDataSource {
    type Item = Quote;
    type Error = MarketDataError;
    
    async fn subscribe(&self) -> Result<StreamReceiver<Quote>, MarketDataError> {
        Ok(StreamReceiver {
            inner: self.tx.subscribe(),
            source_name: "mock".to_string(),
        })
    }
    
    async fn is_connected(&self) -> bool { true }
    async fn reconnect(&self) -> Result<(), MarketDataError> { Ok(()) }
    fn name(&self) -> &'static str { "mock" }
}

// =============================================================================
// USER IMPLEMENTS: Bloomberg, Refinitiv, etc.
// =============================================================================
//
// Users implement StreamSource for their market data vendors:
//
// ```rust
// // In user's crate, NOT in convex
// pub struct BloombergBPipeSource { /* ... */ }
// 
// #[async_trait]
// impl StreamSource for BloombergBPipeSource {
//     type Item = Quote;
//     type Error = BloombergError;
//     // ... implementation
// }
// ```
//
// Similarly for publishers:
//
// ```rust
// pub struct ExchangeInavPublisher { /* ... */ }
// 
// #[async_trait]
// impl PortfolioPublisher for ExchangeInavPublisher {
//     // ... implementation
// }
// ```
```

---

## Real-Time Quote Analytics (`convex-pricing`)

**All real-time quote processing is part of `convex-pricing`.** The framework supports streaming of complete analytics (prices, yields, spreads, risk) on bid/ask/mid sides.

### Streaming Bond Quote Structure

```rust
// =============================================================================
// BOND QUOTE WITH FULL ANALYTICS - STREAMED IN REAL-TIME
// =============================================================================

/// Bond quote with complete analytics on bid/ask/mid
/// This is the primary structure streamed to trading systems
#[derive(Debug, Clone, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize)]
pub struct BondQuote {
    // Identification
    pub instrument_id: InstrumentId,
    pub isin: Option<String>,
    pub cusip: Option<String>,
    
    // Timing
    pub timestamp: jiff::Timestamp,
    pub sequence: u64,
    pub calculation_time_us: u64,  // For latency monitoring
    
    // Source attribution
    pub source: QuoteSource,
    pub venue: Option<String>,
    pub dealer: Option<String>,
    
    // === THE THREE SIDES - Each with complete analytics ===
    pub bid: Option<QuoteSide>,   // Where you can SELL
    pub ask: Option<QuoteSide>,   // Where you can BUY  
    pub mid: Option<QuoteSide>,   // Calculated midpoint
    
    // Quote metadata
    pub quote_condition: QuoteCondition,
    pub valid_until: Option<jiff::Timestamp>,
    pub min_size: Option<Decimal>,
    pub max_size: Option<Decimal>,
    
    // Staleness
    pub age_ms: u64,
    pub is_stale: bool,
    
    // Reference curves used (for debugging/attribution)
    pub benchmark_curve_id: Option<CurveId>,
    pub discount_curve_id: Option<CurveId>,
    pub benchmark_curve_time: Option<jiff::Timestamp>,
}

/// Complete analytics for one side of a quote
/// ALL fields are streamed in real-time
#[derive(Debug, Clone, Serialize, Deserialize, rkyv::Archive, rkyv::Serialize)]
pub struct QuoteSide {
    // =========================================================================
    // PRICES
    // =========================================================================
    pub clean_price: Decimal,
    pub dirty_price: Decimal,
    pub accrued_interest: Decimal,
    
    // =========================================================================
    // SIZE
    // =========================================================================
    pub size: Option<Decimal>,      // Face amount
    pub size_mm: Option<Decimal>,   // In millions
    
    // =========================================================================
    // YIELDS - All calculated from this side's price
    // =========================================================================
    pub ytm: Decimal,               // Yield to maturity
    pub ytw: Option<Decimal>,       // Yield to worst (if callable/putable)
    pub ytc: Option<Decimal>,       // Yield to first call
    pub ytp: Option<Decimal>,       // Yield to first put
    pub ytm_annual: Decimal,        // Annualized YTM
    pub current_yield: Decimal,     // Coupon / Clean Price
    pub simple_yield: Decimal,      // Japanese convention
    
    // =========================================================================
    // SPREADS - All calculated from this side's price
    // =========================================================================
    pub g_spread: Option<Decimal>,      // vs interpolated government
    pub g_spread_benchmark: Option<String>,  // e.g., "UST 10Y"
    pub i_spread: Option<Decimal>,      // vs interpolated swap
    pub z_spread: Option<Decimal>,      // Zero-volatility spread
    pub oas: Option<Decimal>,           // Option-adjusted spread
    pub asw_spread: Option<Decimal>,    // Asset swap spread
    pub discount_margin: Option<Decimal>, // For FRNs
    
    // =========================================================================
    // RISK METRICS - All calculated from this side's price
    // =========================================================================
    pub modified_duration: Decimal,
    pub effective_duration: Option<Decimal>,  // For callable
    pub macaulay_duration: Decimal,
    pub spread_duration: Option<Decimal>,
    pub dv01: Decimal,                  // Dollar value of 01 (per 1MM face)
    pub pv01: Decimal,                  // Present value of 01
    pub convexity: Decimal,
    pub effective_convexity: Option<Decimal>,
    
    // =========================================================================
    // RELATIVE VALUE CONTEXT
    // =========================================================================
    pub z_spread_percentile: Option<u8>,     // vs 30-day history
    pub z_spread_zscore: Option<Decimal>,    // Z-score vs history
    pub rich_cheap_bps: Option<Decimal>,     // vs fair value model
}

/// Quote condition/status
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum QuoteCondition {
    Executable,     // Firm, tradeable quote
    Indicative,     // Informational only
    Stale,          // Aged beyond threshold
    Suspended,      // Temporarily unavailable
    Closed,         // Market closed
}

/// Quote source types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuoteSource {
    /// Executable quote from electronic venue
    Venue { venue: String, quote_id: String },
    
    /// Dealer run (indicative)
    Dealer { dealer: String, run_id: Option<String> },
    
    /// Composite from multiple sources
    Composite { sources: Vec<String>, method: CompositeMethod },
    
    /// Evaluated pricing (BVAL, CBBT, etc.)
    Evaluated { provider: String, quality: Option<u8> },
    
    /// Internal model
    Model { model_id: String },
    
    /// Manual trader input
    Manual { trader_id: String, reason: Option<String> },
    
    /// Derived from trade
    Trade { trade_id: String, trade_time: jiff::Timestamp },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CompositeMethod {
    MidOfBest,
    WeightedAverage,
    Median,
    Vwap,
    Best,
}

// =============================================================================
// REAL-TIME STREAMING OF BOND QUOTES
// =============================================================================

/// Publisher for streaming BondQuote to consumers
#[async_trait]
pub trait BondQuotePublisher: Send + Sync {
    /// Publish a single bond quote update
    async fn publish(&self, quote: &BondQuote) -> Result<(), PublishError>;
    
    /// Publish batch of quotes
    async fn publish_batch(&self, quotes: &[BondQuote]) -> Result<(), PublishError>;
    
    /// Get publishing statistics
    fn stats(&self) -> PublishingStats;
}

/// Subscriber for receiving BondQuote updates
#[async_trait]
pub trait BondQuoteSubscriber: Send + Sync {
    /// Subscribe to updates for specific instruments
    async fn subscribe(&self, instruments: &[InstrumentId]) -> Result<QuoteReceiver, SubscribeError>;
    
    /// Subscribe to all instruments (firehose)
    async fn subscribe_all(&self) -> Result<QuoteReceiver, SubscribeError>;
    
    /// Unsubscribe from instruments
    async fn unsubscribe(&self, instruments: &[InstrumentId]) -> Result<(), SubscribeError>;
}

/// Receiver handle for quote stream
pub struct QuoteReceiver {
    rx: broadcast::Receiver<BondQuote>,
}

impl QuoteReceiver {
    /// Receive next quote (async)
    pub async fn recv(&mut self) -> Result<BondQuote, RecvError> {
        self.rx.recv().await.map_err(Into::into)
    }
    
    /// Try receive without blocking
    pub fn try_recv(&mut self) -> Option<BondQuote> {
        self.rx.try_recv().ok()
    }
}

// =============================================================================
// REAL-TIME PRICING ENGINE
// =============================================================================

/// Real-time pricing engine - calculates full BondQuote from raw market quotes
pub struct PricingEngine {
    /// Curve cache (auto-refreshed on curve updates)
    curves: Arc<CurveCache>,
    
    /// Bond reference data
    bonds: Arc<dyn BondRepository>,
    
    /// Pricing configurations
    config: Arc<PricingConfigRegistry>,
    
    /// Historical spread data (for percentiles)
    historical: Arc<dyn HistoricalSpreadService>,
    
    /// Metrics
    metrics: Arc<PricingMetrics>,
}

impl PricingEngine {
    /// Calculate full BondQuote from raw bid/ask prices
    /// HOT PATH - target < 100µs
    pub fn calculate_quote(
        &self,
        instrument_id: &InstrumentId,
        bid_price: Option<Decimal>,
        ask_price: Option<Decimal>,
        settlement: Date,
    ) -> Result<BondQuote, PricingError> {
        let start = std::time::Instant::now();
        
        // Get bond and config
        let bond = self.bonds.get(instrument_id)?;
        let config = self.config.get_config_for_bond(&bond)?;
        
        // Get curves from cache (no I/O)
        let discount_curve = self.curves.get(&config.discount_curve)?;
        let benchmark_curve = self.curves.get(&config.benchmark_curve)?;
        
        // Calculate each side
        let bid = bid_price.map(|p| self.calculate_side(&bond, p, settlement, &discount_curve, &benchmark_curve, config)).transpose()?;
        let ask = ask_price.map(|p| self.calculate_side(&bond, p, settlement, &discount_curve, &benchmark_curve, config)).transpose()?;
        
        // Calculate mid from bid/ask
        let mid = match (&bid, &ask) {
            (Some(b), Some(a)) => {
                let mid_price = (b.clean_price + a.clean_price) / dec!(2);
                Some(self.calculate_side(&bond, mid_price, settlement, &discount_curve, &benchmark_curve, config)?)
            }
            (Some(b), None) => Some(b.clone()),
            (None, Some(a)) => Some(a.clone()),
            (None, None) => None,
        };
        
        let calculation_time_us = start.elapsed().as_micros() as u64;
        
        Ok(BondQuote {
            instrument_id: instrument_id.clone(),
            isin: bond.isin.clone(),
            cusip: bond.cusip.clone(),
            timestamp: jiff::Timestamp::now(),
            sequence: self.next_sequence(),
            calculation_time_us,
            source: QuoteSource::Model { model_id: "convex".into() },
            venue: None,
            dealer: None,
            bid,
            ask,
            mid,
            quote_condition: QuoteCondition::Indicative,
            valid_until: None,
            min_size: None,
            max_size: None,
            age_ms: 0,
            is_stale: false,
            benchmark_curve_id: Some(config.benchmark_curve.clone()),
            discount_curve_id: Some(config.discount_curve.clone()),
            benchmark_curve_time: Some(benchmark_curve.as_of),
        })
    }
    
    /// Calculate all analytics for one side
    #[inline]
    fn calculate_side(
        &self,
        bond: &Bond,
        clean_price: Decimal,
        settlement: Date,
        discount_curve: &BuiltCurve,
        benchmark_curve: &BuiltCurve,
        config: &BondPricingConfig,
    ) -> Result<QuoteSide, PricingError> {
        // Accrued interest
        let accrued = bond.accrued_interest(settlement)?;
        let dirty_price = clean_price + accrued;
        
        // YTM (Newton-Raphson, typically 3-5 iterations)
        let ytm = bond.ytm_from_price(dirty_price, settlement)?;
        
        // Yield to worst for callable bonds
        let ytw = if bond.call_schedule.is_some() {
            Some(bond.ytw_from_price(dirty_price, settlement)?)
        } else {
            None
        };
        
        // Duration and convexity
        let modified_duration = bond.modified_duration(ytm, settlement)?;
        let macaulay_duration = bond.macaulay_duration(ytm, settlement)?;
        let convexity = bond.convexity(ytm, settlement)?;
        
        // DV01 / PV01
        let dv01 = modified_duration * dirty_price / dec!(100) * dec!(0.0001) * dec!(10000); // per 1MM
        let pv01 = dv01;
        
        // Spreads
        let g_spread = self.calculate_g_spread(bond, ytm, benchmark_curve, settlement)?;
        let z_spread = self.calculate_z_spread(bond, dirty_price, discount_curve, settlement)?;
        
        // Current yield
        let current_yield = if clean_price > Decimal::ZERO {
            bond.coupon_rate / clean_price * dec!(100)
        } else {
            Decimal::ZERO
        };
        
        // Historical context (async in background, use cached value)
        let z_spread_percentile = self.historical.get_percentile_cached(
            &bond.instrument_id, 
            z_spread
        );
        
        Ok(QuoteSide {
            clean_price,
            dirty_price,
            accrued_interest: accrued,
            size: None,
            size_mm: None,
            ytm,
            ytw,
            ytc: None, // Calculate if needed
            ytp: None,
            ytm_annual: ytm, // Adjust for frequency
            current_yield,
            simple_yield: ytm, // Japanese convention
            g_spread: Some(g_spread),
            g_spread_benchmark: Some(format!("{} interpolated", benchmark_curve.curve_id)),
            i_spread: None, // Requires swap curve
            z_spread: Some(z_spread),
            oas: None, // Requires option model
            asw_spread: None, // Requires swap curve
            discount_margin: None, // For FRNs
            modified_duration,
            effective_duration: None, // For callable
            macaulay_duration,
            spread_duration: Some(modified_duration), // Approximation
            dv01,
            pv01,
            convexity,
            effective_convexity: None,
            z_spread_percentile,
            z_spread_zscore: None,
            rich_cheap_bps: None,
        })
    }
    
    /// Batch calculate quotes (parallel)
    pub fn calculate_batch(
        &self,
        quotes: &[(InstrumentId, Option<Decimal>, Option<Decimal>)],
        settlement: Date,
    ) -> Vec<Result<BondQuote, PricingError>> {
        quotes
            .par_iter()
            .map(|(id, bid, ask)| self.calculate_quote(id, *bid, *ask, settlement))
            .collect()
    }
}

// =============================================================================
// QUOTE STREAM PROCESSOR
// =============================================================================

/// Processes raw market quotes and outputs enriched BondQuotes
pub struct QuoteStreamProcessor {
    /// Pricing engine
    engine: Arc<PricingEngine>,
    
    /// Publishers to send enriched quotes to
    publishers: Vec<Arc<dyn BondQuotePublisher>>,
    
    /// Conflation window
    conflation: Conflator<RawQuote>,
    
    /// Settlement date calculator
    settlement_calc: Arc<SettlementCalculator>,
}

impl QuoteStreamProcessor {
    /// Process incoming raw quote, calculate analytics, publish enriched quote
    pub async fn process(&self, raw: RawQuote) -> Result<(), ProcessError> {
        // Apply conflation (skip if same instrument updated recently)
        if self.conflation.should_skip(&raw) {
            return Ok(());
        }
        
        // Calculate settlement date
        let settlement = self.settlement_calc.calculate(&raw.instrument_id)?;
        
        // Calculate full quote with all analytics
        let quote = self.engine.calculate_quote(
            &raw.instrument_id,
            raw.bid_price,
            raw.ask_price,
            settlement,
        )?;
        
        // Publish to all subscribers
        for publisher in &self.publishers {
            publisher.publish(&quote).await?;
        }
        
        Ok(())
    }
}

// =============================================================================
// RAW QUOTE INPUT
// =============================================================================

/// Raw quote from market data feed (before analytics)
#[derive(Debug, Clone)]
pub struct RawQuote {
    pub instrument_id: InstrumentId,
    pub bid_price: Option<Decimal>,
    pub ask_price: Option<Decimal>,
    pub bid_size: Option<Decimal>,
    pub ask_size: Option<Decimal>,
    pub timestamp: jiff::Timestamp,
    pub source: String,
}

// =============================================================================
// WATCHLIST / BLOTTER
// =============================================================================

/// Watchlist with configurable columns - receives streaming BondQuote updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Watchlist {
    pub id: String,
    pub name: String,
    pub instruments: Vec<InstrumentId>,
    pub columns: Vec<WatchlistColumn>,
    pub sort_by: Option<WatchlistColumn>,
    pub sort_desc: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum WatchlistColumn {
    // Identification
    Symbol,
    Isin,
    Cusip,
    Issuer,
    
    // Bond terms
    Coupon,
    Maturity,
    
    // Bid side
    BidPrice,
    BidYield,
    BidGSpread,
    BidZSpread,
    BidSize,
    
    // Ask side
    AskPrice,
    AskYield,
    AskGSpread,
    AskZSpread,
    AskSize,
    
    // Mid
    MidPrice,
    MidYield,
    MidGSpread,
    MidZSpread,
    
    // Derived
    BidAskSpread,  // Ask - Bid in price
    BidAskSpreadBps,  // In yield terms
    
    // Risk
    Duration,
    DV01,
    Convexity,
    
    // Change
    PriceChange,
    YieldChange,
    SpreadChange,
    
    // Metadata
    Source,
    Age,
    Condition,
}

// =============================================================================
// RFQ ANALYTICS
// =============================================================================

/// RFQ analysis with market context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RfqAnalytics {
    pub rfq_id: String,
    pub instrument_id: InstrumentId,
    pub side: Side,
    pub size: Decimal,
    pub client_id: String,
    
    // Current market (full BondQuote)
    pub market: BondQuote,
    
    // Historical context
    pub spread_avg_30d: Option<Decimal>,
    pub spread_percentile: Option<u8>,
    pub last_trade: Option<TradeReference>,
    
    // Suggested response
    pub suggested_price: Option<Decimal>,
    pub suggested_spread: Option<Decimal>,
    
    // Risk impact
    pub position_dv01_impact: Decimal,
    pub desk_dv01_after: Decimal,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Side {
    Bid,  // Client selling to us
    Ask,  // Client buying from us
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeReference {
    pub price: Decimal,
    pub yield_: Decimal,
    pub size: Decimal,
    pub timestamp: jiff::Timestamp,
    pub venue: String,
}

// =============================================================================
// PERFORMANCE REQUIREMENTS
// =============================================================================

/*
LATENCY TARGETS:

| Operation | Target | Notes |
|-----------|--------|-------|
| RawQuote → BondQuote | < 100µs | Full analytics on bid/ask/mid |
| Price → YTM | < 50µs | Newton-Raphson |
| All spreads (G, I, Z) | < 100µs | Curve interpolation |
| Batch 100 bonds | < 5ms | Parallel |
| Quote publish latency | < 1ms | To WebSocket |

THROUGHPUT:

| Metric | Target |
|--------|--------|
| Quotes processed/sec | 10,000 |
| Bonds priced/sec | 1,000 |
| Concurrent subscriptions | 1,000 |

ALL ANALYTICS STREAMED:
- Every field in QuoteSide is calculated and streamed
- Bid, Ask, and Mid all have complete analytics
- Updates pushed on every market data change
- No request/response - pure streaming
*/

// =============================================================================
// CLI COMMANDS
// =============================================================================

/*
# Get quote with full analytics
convex quote US037833DV9

# Output:
# AAPL 3.85% 2043 (US037833DV9)
# ─────────────────────────────────────────────────────
#              Bid         Mid         Ask
# Price        98.250      98.375      98.500
# Yield        4.052%      4.037%      4.022%
# G-Spread    +125.2      +123.5      +121.8
# Z-Spread    +128.5      +126.8      +125.1
# Duration     12.45       12.44       12.43
# DV01        $1,223      $1,222      $1,221
# Convexity    189.2       189.1       189.0
# ─────────────────────────────────────────────────────
# Source: MKTX | Age: 2.3s | Executable

# Watch bonds (streaming)
convex quote watch US037833DV9 US38141GXZ2 --refresh 1s

# Price from yield
convex calc price US037833DV9 --yield 4.05 --settlement 2025-01-17

# Spread from price
convex calc spread US037833DV9 --price 98.25 --type z-spread

# Analyze RFQ
convex rfq analyze --bond US037833DV9 --side bid --size 5MM --client ACME
*/
```

### 6. Observability Stack (`convex-metrics`)

```rust
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::WithExportConfig;

/// Initialize full observability stack
pub fn init_observability(config: &ObservabilityConfig) -> Result<()> {
    // Create OTLP exporter
    let otlp_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&config.otlp_endpoint)
        .build()?;
    
    // Create tracer provider
    let tracer_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(otlp_exporter, opentelemetry_sdk::runtime::Tokio)
        .with_resource(opentelemetry_sdk::Resource::new(vec![
            opentelemetry::KeyValue::new("service.name", "convex"),
            opentelemetry::KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
        ]))
        .build();
    
    let tracer = tracer_provider.tracer("convex");
    
    // Create tracing layer
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    
    // JSON logging for production
    let json_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_current_span(true);
    
    // Initialize subscriber
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(otel_layer)
        .with(json_layer)
        .init();
    
    Ok(())
}

/// Custom metrics for financial calculations
pub struct ConvexMetrics {
    pub pricing_duration: prometheus::Histogram,
    pub curve_build_duration: prometheus::Histogram,
    pub cache_hit_rate: prometheus::Gauge,
    pub active_subscriptions: prometheus::IntGauge,
    pub quote_updates_total: prometheus::IntCounter,
    pub calculation_errors: prometheus::IntCounterVec,
}

impl ConvexMetrics {
    pub fn new() -> Self {
        Self {
            pricing_duration: prometheus::Histogram::with_opts(
                prometheus::HistogramOpts::new("convex_pricing_duration_seconds", "Bond pricing duration")
                    .buckets(vec![0.000001, 0.00001, 0.0001, 0.001, 0.01, 0.1])
            ).unwrap(),
            curve_build_duration: prometheus::Histogram::with_opts(
                prometheus::HistogramOpts::new("convex_curve_build_duration_seconds", "Curve bootstrapping duration")
                    .buckets(vec![0.0001, 0.001, 0.01, 0.1, 1.0])
            ).unwrap(),
            // ... other metrics
        }
    }
}
```

---

## Instrument Definitions (`convex-bonds` or existing bond crate)

**Note: This prompt uses `convex-instruments` as a logical name. When implementing, use your existing bond crate (e.g., `convex-bonds`) and extend it with these types.**

This crate contains **all bond and instrument reference data types**. These are pure data structures with no pricing logic.

### Comprehensive Bond Type Support

```rust
// =============================================================================
// CORE BOND STRUCTURE
// =============================================================================

/// Universal bond definition supporting all instrument types
/// This is COMPREHENSIVE - includes all fields needed for trading and pricing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bond {
    // =========================================================================
    // IDENTIFIERS
    // =========================================================================
    pub id: InstrumentId,
    pub isin: Option<String>,
    pub cusip: Option<String>,
    pub figi: Option<String>,
    pub sedol: Option<String>,
    pub ticker: Option<String>,
    pub bbg_id: Option<String>,         // Bloomberg ID (e.g., "EJ1234567")
    pub ric: Option<String>,            // Reuters RIC
    
    // =========================================================================
    // ISSUER INFORMATION
    // =========================================================================
    pub issuer: Issuer,
    pub guarantor: Option<Issuer>,      // For guaranteed bonds
    pub ultimate_parent: Option<String>, // Ultimate parent company
    
    // =========================================================================
    // INSTRUMENT TYPE & CLASSIFICATION
    // =========================================================================
    pub instrument_type: InstrumentType,
    pub asset_class: AssetClass,
    pub security_type: SecurityType,    // More granular than asset class
    
    // =========================================================================
    // CORE TERMS
    // =========================================================================
    pub currency: Currency,
    pub face_value: Decimal,
    pub min_piece: Decimal,             // Minimum denomination (e.g., 1000, 100000, 200000)
    pub min_increment: Decimal,         // Trading increment
    pub issue_date: Date,
    pub maturity_date: Option<Date>,    // None for perpetuals
    pub dated_date: Option<Date>,       // Interest accrual start
    pub first_coupon_date: Option<Date>,
    pub first_settle_date: Option<Date>,
    pub announce_date: Option<Date>,
    
    // =========================================================================
    // ISSUE SIZE & OUTSTANDING
    // =========================================================================
    pub issue_amount: Decimal,          // Original issue size
    pub amount_outstanding: Decimal,    // Current outstanding (after buybacks/calls)
    pub amount_issued_local: Decimal,   // In local currency
    
    // =========================================================================
    // COUPON STRUCTURE
    // =========================================================================
    pub coupon: CouponStructure,
    pub coupon_rate: Decimal,           // Current/initial coupon rate
    pub ex_dividend_days: u8,           // Days before payment when goes ex-div
    pub record_date_days: u8,           // Days before payment for record date
    
    // =========================================================================
    // DAY COUNT AND PAYMENT
    // =========================================================================
    pub day_count: DayCount,
    pub payment_frequency: Frequency,
    pub payment_calendar: CalendarId,
    pub payment_business_day_rule: BusinessDayRule,
    pub accrual_calendar: Option<CalendarId>,
    
    // =========================================================================
    // SETTLEMENT
    // =========================================================================
    pub settlement_days: u8,
    pub settlement_calendar: CalendarId,
    pub settlement_type: SettlementType,
    
    // =========================================================================
    // REDEMPTION
    // =========================================================================
    pub redemption_value: Decimal,      // Usually 100
    pub redemption_currency: Option<Currency>, // If different from issue currency
    
    // =========================================================================
    // EMBEDDED OPTIONS
    // =========================================================================
    pub call_schedule: Option<CallSchedule>,
    pub put_schedule: Option<PutSchedule>,
    pub is_callable: bool,
    pub is_putable: bool,
    pub is_convertible: bool,
    pub is_exchangeable: bool,
    
    // =========================================================================
    // SINKING FUND / AMORTIZATION
    // =========================================================================
    pub sink_schedule: Option<SinkSchedule>,
    pub amortization_schedule: Option<AmortizationSchedule>,
    pub is_amortizing: bool,
    
    // =========================================================================
    // SPREAD & BENCHMARK INFORMATION (Critical for Corporate Bonds)
    // =========================================================================
    pub benchmark: Option<BenchmarkInfo>,
    pub spread_at_issue: Option<SpreadAtIssue>,
    pub pricing_benchmark: Option<PricingBenchmark>,
    
    // =========================================================================
    // CREDIT INFORMATION
    // =========================================================================
    pub credit_ratings: CreditRatings,
    pub seniority: Seniority,
    pub security_type_debt: DebtType,
    pub is_secured: bool,
    pub collateral_type: Option<CollateralType>,
    pub covenants: Option<CovenantInfo>,
    
    // =========================================================================
    // CLASSIFICATION & SECTOR
    // =========================================================================
    pub sector: Option<Sector>,
    pub industry_group: Option<String>,
    pub industry_subgroup: Option<String>,
    pub bics_level_1: Option<String>,   // Bloomberg Industry Classification
    pub bics_level_2: Option<String>,
    pub bics_level_3: Option<String>,
    pub gics_sector: Option<String>,    // MSCI/S&P GICS
    pub gics_industry: Option<String>,
    
    // =========================================================================
    // REGULATORY & COMPLIANCE
    // =========================================================================
    pub is_144a: bool,                  // SEC Rule 144A (private placement)
    pub is_reg_s: bool,                 // Regulation S (offshore)
    pub is_green_bond: bool,
    pub is_social_bond: bool,
    pub is_sustainability_bond: bool,
    pub esg_rating: Option<String>,
    pub mifid_liquidity: Option<MifidLiquidity>,
    pub country_of_risk: Option<String>,
    pub country_of_domicile: Option<String>,
    
    // =========================================================================
    // TRADING INFORMATION
    // =========================================================================
    pub trading_status: TradingStatus,
    pub listing_exchange: Option<String>,
    pub primary_exchange: Option<String>,
    pub trading_lot_size: Option<Decimal>,
    pub is_index_eligible: IndexEligibility,
    
    // =========================================================================
    // TAX TREATMENT
    // =========================================================================
    pub tax_status: TaxStatus,
    pub is_oid: bool,                   // Original Issue Discount
    pub is_tax_exempt: bool,
    pub withholding_tax_rate: Option<Decimal>,
    
    // =========================================================================
    // SPECIAL FEATURES
    // =========================================================================
    pub features: BondFeatures,
    
    // =========================================================================
    // METADATA
    // =========================================================================
    pub created_at: jiff::Timestamp,
    pub updated_at: jiff::Timestamp,
    pub data_source: String,
}

// =============================================================================
// BENCHMARK & SPREAD INFORMATION
// =============================================================================

/// Benchmark security for spread calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkInfo {
    /// Benchmark type
    pub benchmark_type: BenchmarkType,
    
    /// Specific benchmark security (e.g., "UST 4.5% 2034", "DBR 0% 2034")
    pub benchmark_security: Option<String>,
    
    /// Benchmark ISIN if applicable
    pub benchmark_isin: Option<String>,
    
    /// Benchmark curve for interpolation
    pub benchmark_curve: Option<String>,  // e.g., "USD.GOVT", "EUR.GOVT", "USD.SWAP"
    
    /// Interpolated tenor on curve
    pub interpolated_tenor: Option<Tenor>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum BenchmarkType {
    /// Specific treasury bond (on-the-run or off-the-run)
    Treasury,
    
    /// Interpolated treasury curve
    TreasuryInterpolated,
    
    /// Swap rate
    Swap,
    
    /// Interpolated swap curve
    SwapInterpolated,
    
    /// Gilt (UK)
    Gilt,
    
    /// Bund (Germany)
    Bund,
    
    /// OAT (France)
    Oat,
    
    /// BTP (Italy)
    Btp,
    
    /// JGB (Japan)
    Jgb,
    
    /// Other sovereign
    Sovereign { country: String },
    
    /// Money market rate (for short-dated)
    MoneyMarket,
    
    /// No benchmark (standalone)
    None,
}

/// Spread at issue - critical for relative value analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpreadAtIssue {
    /// G-spread at issue (vs government)
    pub g_spread: Option<Decimal>,
    
    /// T-spread at issue (vs treasury benchmark)
    pub t_spread: Option<Decimal>,
    
    /// I-spread at issue (vs swap)
    pub i_spread: Option<Decimal>,
    
    /// Z-spread at issue
    pub z_spread: Option<Decimal>,
    
    /// OAS at issue (for callable)
    pub oas: Option<Decimal>,
    
    /// Spread type used for pricing
    pub primary_spread_type: SpreadType,
    
    /// The spread value used for initial pricing
    pub primary_spread_value: Decimal,
    
    /// Reoffer yield at issue
    pub reoffer_yield: Option<Decimal>,
    
    /// Reoffer price at issue
    pub reoffer_price: Option<Decimal>,
    
    /// Issue date for reference
    pub issue_date: Date,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SpreadType {
    GSpread,
    TSpread,
    ISpread,
    ZSpread,
    Oas,
    DiscountMargin,  // For FRNs
    AssetSwap,
}

/// Pricing benchmark configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingBenchmark {
    /// Which curve to use for G-spread
    pub g_spread_curve: String,
    
    /// Which curve to use for I-spread
    pub i_spread_curve: String,
    
    /// Which curve to use for Z-spread discounting
    pub z_spread_curve: String,
    
    /// Benchmark bond for direct comparison (if applicable)
    pub benchmark_bond: Option<InstrumentId>,
    
    /// Sector curve for sector-relative analysis
    pub sector_curve: Option<String>,
}

// =============================================================================
// CREDIT RATINGS
// =============================================================================

/// Comprehensive credit ratings from all major agencies
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CreditRatings {
    // Moody's
    pub moodys_rating: Option<String>,       // e.g., "Aa2", "Baa1"
    pub moodys_outlook: Option<RatingOutlook>,
    pub moodys_watch: Option<RatingWatch>,
    pub moodys_date: Option<Date>,
    
    // S&P
    pub sp_rating: Option<String>,           // e.g., "AA", "BBB+"
    pub sp_outlook: Option<RatingOutlook>,
    pub sp_watch: Option<RatingWatch>,
    pub sp_date: Option<Date>,
    
    // Fitch
    pub fitch_rating: Option<String>,        // e.g., "AA-", "BBB"
    pub fitch_outlook: Option<RatingOutlook>,
    pub fitch_watch: Option<RatingWatch>,
    pub fitch_date: Option<Date>,
    
    // DBRS
    pub dbrs_rating: Option<String>,
    pub dbrs_date: Option<Date>,
    
    // Composite
    pub composite_rating: Option<String>,    // Derived composite
    pub is_investment_grade: bool,
    pub is_high_yield: bool,
    pub is_crossover: bool,                  // BBB-/BB+ boundary
    pub is_fallen_angel: bool,               // Was IG, now HY
    pub is_rising_star: bool,                // Was HY, now IG
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum RatingOutlook {
    Positive,
    Stable,
    Negative,
    Developing,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum RatingWatch {
    UpgradePossible,
    DowngradePossible,
    UncertainDirection,
}

// =============================================================================
// ASSET CLASS & SECURITY TYPE
// =============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AssetClass {
    Government,
    SupraNational,     // World Bank, EIB, etc.
    Agency,            // Freddie, Fannie, etc.
    Municipal,
    CorporateIG,       // Investment Grade
    CorporateHY,       // High Yield
    EmergingMarket,
    Covered,           // Covered bonds
    Abs,               // Asset-backed
    Mbs,               // Mortgage-backed
    Cmo,               // Collateralized mortgage
    Clo,               // Collateralized loan
    Convertible,
    PreferredStock,
    MoneyMarket,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecurityType {
    // Government
    TreasuryBill,
    TreasuryNote,
    TreasuryBond,
    Tips,              // Treasury Inflation-Protected
    Strips,            // Separately Traded Registered Interest and Principal
    SovereignBond,
    
    // Agency
    AgencyDebenture,
    AgencyMbs,
    AgencyCmo,
    
    // Corporate
    SeniorUnsecured,
    SeniorSecured,
    Subordinated,
    JuniorSubordinated,
    HybridCapital,
    Tier1Capital,
    Tier2Capital,
    AdditionalTier1,   // AT1 / CoCo
    
    // Municipal
    GeneralObligation,
    RevenueBond,
    TaxAllocation,
    
    // Structured
    AssetBacked,
    MortgageBacked,
    CollateralizedLoan,
    
    // Other
    ConvertibleBond,
    ExchangeableBond,
    PrivatePlacement,
    MediumTermNote,
}

// =============================================================================
// SENIORITY & DEBT TYPE
// =============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Seniority {
    Senior,
    SeniorSecured,
    SeniorUnsecured,
    SeniorNonPreferred,   // EU MREL/TLAC
    SeniorSubordinated,
    Subordinated,
    JuniorSubordinated,
    Tier1,
    Tier2,
    AdditionalTier1,      // AT1 CoCos
    Equity,               // Preferred stock treated as debt
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DebtType {
    Bond,
    Note,
    Debenture,
    CommercialPaper,
    MediumTermNote,
    PrivatePlacement,
    Loan,
    PreferredStock,
    ConvertibleNote,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CollateralType {
    Unsecured,
    FirstLien,
    SecondLien,
    ThirdLien,
    AssetBacked,
    MortgageBacked,
    EquipmentTrust,
    GuaranteedByParent,
    LetterOfCredit,
    GovernmentGuaranteed,
}

// =============================================================================
// TRADING & COMPLIANCE
// =============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TradingStatus {
    Active,
    Called,
    Matured,
    Defaulted,
    Suspended,
    Delisted,
    TenderOffer,
    ExchangeOffer,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SettlementType {
    Regular,       // T+1, T+2, etc. based on market
    Cash,          // T+0
    Corporate,     // Corporate action settlement
    WhenIssued,    // Pre-issue trading
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexEligibility {
    pub bloomberg_aggregate: bool,
    pub bloomberg_us_credit: bool,
    pub bloomberg_euro_aggregate: bool,
    pub ice_bofa_us_corp: bool,
    pub ice_bofa_euro_corp: bool,
    pub jp_morgan_embi: bool,
    pub jp_morgan_cembi: bool,
    pub markit_iboxx: bool,
    pub ftse_wgbi: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MifidLiquidity {
    Liquid,
    Illiquid,
    NotAssessed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TaxStatus {
    FullyTaxable,
    TaxExempt,
    AltMinTaxSubject,    // AMT for munis
    FederalTaxExempt,
    StateTaxExempt,
    TaxDeferred,
}

// =============================================================================
// COVENANTS
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CovenantInfo {
    pub has_change_of_control: bool,
    pub change_of_control_put_price: Option<Decimal>,
    pub has_cross_default: bool,
    pub has_cross_acceleration: bool,
    pub has_negative_pledge: bool,
    pub has_limitation_on_liens: bool,
    pub has_limitation_on_debt: bool,
    pub has_restricted_payments: bool,
    pub covenant_score: Option<u8>,      // 1-10 scale
    pub covenant_source: Option<String>, // e.g., "Moody's CIQ"
}

// =============================================================================
// SPECIAL FEATURES
// =============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BondFeatures {
    pub is_perpetual: bool,
    pub is_pik: bool,                  // Payment-in-kind
    pub is_toggle: bool,               // PIK toggle
    pub is_covered: bool,              // Covered bond
    pub is_pfandbrief: bool,           // German covered bond
    pub is_sukuk: bool,                // Islamic bond
    pub is_dual_currency: bool,
    pub is_index_linked: bool,
    pub is_asset_backed: bool,
    pub is_project_finance: bool,
    pub is_structured: bool,
    pub is_hybrid: bool,
    pub is_contingent_capital: bool,   // CoCo
    pub is_bail_in_eligible: bool,     // MREL/TLAC
    pub has_make_whole_call: bool,
    pub has_par_call: bool,
    pub has_extraordinary_redemption: bool,
    pub has_mandatory_redemption: bool,
    pub has_extension_option: bool,    // For AT1/Tier2
    pub extension_date: Option<Date>,
}

// =============================================================================
// INSTRUMENT TYPES
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InstrumentType {
    /// Fixed rate bond
    FixedRate,
    
    /// Zero coupon / discount bond
    ZeroCoupon,
    
    /// Floating rate note
    FloatingRate {
        index: FloatingRateIndex,
        spread: Decimal,  // bps
        reset_frequency: Frequency,
        reset_in_arrears: bool,
        lookback_days: Option<u8>,
        observation_shift: Option<u8>,
        cap: Option<Decimal>,
        floor: Option<Decimal>,
        compounding_method: Option<CompoundingMethod>,
    },
    
    /// Step-up / Step-down coupon
    StepUp {
        schedule: Vec<StepCoupon>,
    },
    
    /// Inflation-linked (TIPS, Linkers)
    InflationLinked {
        index: InflationIndex,
        base_cpi: Decimal,
        lag_months: u8,
        floor: Option<Decimal>,  // Deflation floor
        is_capital_indexed: bool,
        is_interest_indexed: bool,
    },
    
    /// Callable bond
    Callable,
    
    /// Putable bond
    Putable,
    
    /// Convertible bond
    Convertible {
        underlying: String,
        conversion_ratio: Decimal,
        conversion_price: Decimal,
        conversion_start: Date,
        conversion_end: Date,
        is_mandatory: bool,
    },
    
    /// Exchangeable bond
    Exchangeable {
        underlying: String,
        exchange_ratio: Decimal,
    },
    
    /// Perpetual (no maturity)
    Perpetual {
        first_call_date: Option<Date>,
        step_up_date: Option<Date>,
        step_up_spread: Option<Decimal>,
    },
    
    /// Amortizing
    Amortizing {
        schedule: AmortizationSchedule,
    },
    
    /// Payment-in-kind
    Pik {
        pik_rate: Decimal,
        cash_rate: Decimal,
        is_toggle: bool,
    },
    
    /// Range accrual
    RangeAccrual {
        reference_index: FloatingRateIndex,
        lower_barrier: Decimal,
        upper_barrier: Decimal,
        accrual_rate: Decimal,
    },
    
    /// Fixed-to-Float (common for bank capital)
    FixedToFloat {
        fixed_rate: Decimal,
        fixed_end_date: Date,
        float_index: FloatingRateIndex,
        float_spread: Decimal,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CompoundingMethod {
    None,
    Flat,
    Straight,
    SpreadExclusive,
    SpreadInclusive,
}

// =============================================================================
// COUPON STRUCTURES
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CouponStructure {
    /// Fixed coupon rate
    Fixed {
        rate: Decimal,  // Annual rate as decimal (0.05 = 5%)
    },
    
    /// Zero coupon
    Zero,
    
    /// Floating rate
    Floating {
        index: FloatingRateIndex,
        spread: Decimal,
        cap: Option<Decimal>,
        floor: Option<Decimal>,
        multiplier: Decimal,  // Usually 1.0, but can be leveraged
    },
    
    /// Step-up/Step-down
    Stepped {
        steps: Vec<StepCoupon>,
    },
    
    /// Range accrual
    RangeAccrual {
        base_rate: Decimal,
        reference_index: FloatingRateIndex,
        lower_barrier: Decimal,
        upper_barrier: Decimal,
    },
    
    /// Inflation-linked
    Inflation {
        real_coupon: Decimal,
        index: InflationIndex,
    },
    
    /// Fixed-to-Float
    FixedToFloat {
        fixed_rate: Decimal,
        switch_date: Date,
        float_index: FloatingRateIndex,
        float_spread: Decimal,
    },
    
    /// Payment-in-kind
    Pik {
        cash_rate: Decimal,
        pik_rate: Decimal,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepCoupon {
    pub effective_date: Date,
    pub rate: Decimal,
    pub reason: Option<StepReason>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum StepReason {
    Scheduled,
    RatingDowngrade,
    ChangeOfControl,
    NonCall,           // Step-up if not called at first call date
}

// =============================================================================
// ISSUER
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issuer {
    pub id: String,
    pub name: String,
    pub short_name: Option<String>,
    pub lei: Option<String>,           // Legal Entity Identifier
    pub ticker: Option<String>,
    pub parent_id: Option<String>,
    pub ultimate_parent_id: Option<String>,
    pub country: String,
    pub sector: Option<Sector>,
    pub industry: Option<String>,
    pub issuer_type: IssuerType,
    pub is_financial_institution: bool,
    pub is_government_related: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum IssuerType {
    Sovereign,
    SubSovereign,      // States, provinces, municipalities
    Supranational,
    Agency,
    CentralBank,
    PublicCompany,
    PrivateCompany,
    FinancialInstitution,
    Bank,
    InsuranceCompany,
    Spv,               // Special Purpose Vehicle
    Trust,
}

// =============================================================================
// FLOATING RATE INDICES (Expanded)
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FloatingRateIndex {
    // USD
    Sofr,
    SofrCompounded,
    SofrAverage { tenor: Tenor },
    TermSofr { tenor: Tenor },
    FedFunds,
    FedFundsEffective,
    Prime,
    LiborUsd { tenor: Tenor },  // Legacy
    Ameribor,
    
    // EUR
    Estr,
    EstrCompounded,
    Euribor { tenor: Tenor },
    LiborEur { tenor: Tenor },  // Legacy
    
    // GBP
    Sonia,
    SoniaCompounded,
    LiborGbp { tenor: Tenor },  // Legacy
    
    // JPY
    Tonar,
    Tibor { tenor: Tenor },
    
    // CHF
    Saron,
    SaronCompounded,
    
    // AUD
    Aonia,
    Bbsw { tenor: Tenor },
    
    // CAD
    Corra,
    Cdor { tenor: Tenor },
    
    // Other
    Custom { name: String, currency: Currency },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InflationIndex {
    // US
    CpiU,           // CPI-U (Urban)
    CpiUNsa,        // CPI-U Non-Seasonally Adjusted
    
    // UK
    Rpi,            // Retail Price Index
    Cpih,           // CPI including Housing
    UkCpi,
    
    // Eurozone
    Hicp,           // Harmonized Index (all items)
    HicpExTobacco,
    FrenchCpi,
    
    // Other
    JapanCpi,
    AustraliaCpi,
    CanadaCpi,
    
    Custom { name: String, country: String },
}

// =============================================================================
// CALL/PUT SCHEDULES
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallSchedule {
    pub call_type: CallType,
    pub dates: Vec<CallDate>,
    pub notice_days: u8,
    pub partial_call_allowed: bool,
    pub make_whole: Option<MakeWholeProvision>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CallType {
    American,      // Callable any time after first call date
    European,      // Callable only on specific dates
    Bermudan,      // Callable on schedule of dates
    Continuous,    // Callable any time
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallDate {
    pub date: Date,
    pub price: Decimal,         // Call price (100 = par)
    pub price_type: CallPriceType,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CallPriceType {
    Clean,          // Clean price
    Dirty,          // Includes accrued
    ParPlusAccrued, // Par + accrued interest
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MakeWholeProvision {
    pub spread_bps: Decimal,
    pub reference_curve: String,  // e.g., "UST", "GILT"
    pub floor_price: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutSchedule {
    pub dates: Vec<PutDate>,
    pub notice_days: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutDate {
    pub date: Date,
    pub price: Decimal,
}

// =============================================================================
// SINKING FUND
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SinkSchedule {
    pub sink_type: SinkType,
    pub dates: Vec<SinkDate>,
    pub delivery_option: bool,  // Can deliver bonds instead of cash
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SinkType {
    Mandatory,      // Must redeem
    Optional,       // Issuer's choice
    Purchase,       // Open market purchase allowed
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SinkDate {
    pub date: Date,
    pub amount: SinkAmount,
    pub price: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SinkAmount {
    Percentage(Decimal),    // % of original issue
    FaceAmount(Decimal),    // Fixed face amount
}

// =============================================================================
// AMORTIZATION
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmortizationSchedule {
    pub amort_type: AmortizationType,
    pub schedule: Vec<AmortizationPayment>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AmortizationType {
    Bullet,         // No amortization (standard)
    Amortizing,     // Principal reduces over time
    AccretingOID,   // Original issue discount
    CustomSchedule,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmortizationPayment {
    pub date: Date,
    pub principal_payment: Decimal,
    pub remaining_balance: Decimal,
}

// =============================================================================
// BOND FEATURES
// =============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BondFeatures {
    /// Guaranteed by parent/government
    pub guaranteed: bool,
    pub guarantor: Option<String>,
    
    /// Secured by collateral
    pub secured: bool,
    pub collateral_type: Option<String>,
    
    /// Subordination
    pub subordinated: bool,
    pub subordination_tier: Option<SubordinationTier>,
    
    /// CoCo/AT1 features
    pub contingent_convertible: bool,
    pub write_down_trigger: Option<Decimal>,  // CET1 ratio trigger
    pub write_down_type: Option<WriteDownType>,
    
    /// Tax features
    pub tax_exempt: bool,  // Munis
    pub qualified_small_issue: bool,
    
    /// Strip eligibility
    pub strip_eligible: bool,
    
    /// Ex-dividend rules
    pub ex_div_days: u8,
    
    /// Odd first/last coupon
    pub first_coupon_date: Option<Date>,
    pub first_coupon_type: CouponType,
    pub last_coupon_type: CouponType,
    
    /// Regulatory
    pub mifid_complexity: ComplexityLevel,
    pub eligible_for_repo: bool,
    pub central_bank_eligible: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SubordinationTier {
    Senior,
    SeniorSecured,
    SeniorUnsecured,
    SeniorPreferred,
    SeniorNonPreferred,  // HoldCo / SNP
    Tier2,
    Tier2Lower,
    AdditionalTier1,     // AT1 / CoCo
    JuniorSubordinated,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum WriteDownType {
    PermanentWriteDown,
    TemporaryWriteDown,
    ConversionToEquity,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CouponType {
    Regular,
    Short,
    Long,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ComplexityLevel {
    NonComplex,
    Complex,
}

// =============================================================================
// ISSUER
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issuer {
    pub id: String,
    pub name: String,
    pub short_name: Option<String>,
    pub lei: Option<String>,  // Legal Entity Identifier
    pub issuer_type: IssuerType,
    pub country: String,
    pub sector: Option<Sector>,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum IssuerType {
    Sovereign,
    Supranational,
    Agency,
    Municipal,
    CorporateFinancial,
    CorporateNonFinancial,
    SpecialPurposeVehicle,
}

// =============================================================================
// ENUMS
// =============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Frequency {
    Annual,
    SemiAnnual,
    Quarterly,
    Monthly,
    Weekly,
    Daily,
    AtMaturity,  // Zero coupon
    Continuous,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Seniority {
    Secured,
    Senior,
    Subordinated,
    Junior,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Sector {
    Government,
    Agency,
    Supranational,
    Financials,
    Industrials,
    Utilities,
    Technology,
    Healthcare,
    ConsumerDiscretionary,
    ConsumerStaples,
    Energy,
    Materials,
    RealEstate,
    Telecommunications,
    Municipal,
}
```

### Instrument Validation

```rust
impl Bond {
    /// Validate bond definition
    pub fn validate(&self) -> Result<(), Vec<ValidationError>> {
        let mut errors = Vec::new();
        
        // Maturity must be after issue date
        if self.maturity_date <= self.issue_date {
            errors.push(ValidationError::new("maturity_date", "must be after issue_date"));
        }
        
        // Call dates must be before maturity
        if let Some(ref calls) = self.call_schedule {
            for call in &calls.dates {
                if call.date >= self.maturity_date {
                    errors.push(ValidationError::new("call_schedule", "call date must be before maturity"));
                }
            }
        }
        
        // FRN must have floating coupon
        if matches!(self.instrument_type, InstrumentType::FloatingRate { .. }) {
            if !matches!(self.coupon, CouponStructure::Floating { .. }) {
                errors.push(ValidationError::new("coupon", "FRN must have floating coupon structure"));
            }
        }
        
        // Sink dates must be before maturity
        if let Some(ref sinks) = self.sink_schedule {
            for sink in &sinks.dates {
                if sink.date >= self.maturity_date {
                    errors.push(ValidationError::new("sink_schedule", "sink date must be before maturity"));
                }
            }
        }
        
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}
```

---

## Bond Analytics & Real-Time Streaming

### `BondQuote` - Production Naming

```rust
/// Bond quote with full analytics on each side
/// This is the primary output type for real-time pricing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondQuote {
    pub instrument_id: InstrumentId,
    pub timestamp: jiff::Timestamp,
    pub sequence: u64,
    
    // Source attribution
    pub source: QuoteSource,
    
    // Bid side (dealer buys / you sell)
    pub bid: Option<QuoteSide>,
    
    // Ask/Offer side (dealer sells / you buy)  
    pub ask: Option<QuoteSide>,
    
    // Mid (calculated)
    pub mid: Option<QuoteSide>,
    
    // Quote metadata
    pub quote_condition: QuoteCondition,
    pub size: Option<QuoteSize>,
    
    // Staleness
    pub age_ms: u64,
}

/// Full analytics for one side of a quote
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteSide {
    // Prices
    pub clean_price: Decimal,
    pub dirty_price: Decimal,
    pub accrued_interest: Decimal,
    
    // Yields
    pub yield_to_maturity: Decimal,
    pub yield_to_worst: Option<Decimal>,
    pub yield_to_call: Option<Decimal>,
    pub current_yield: Decimal,
    
    // Spreads
    pub g_spread: Option<Decimal>,
    pub i_spread: Option<Decimal>,
    pub z_spread: Option<Decimal>,
    pub oas: Option<Decimal>,
    pub asw_spread: Option<Decimal>,
    pub discount_margin: Option<Decimal>,  // For FRNs
    
    // Risk
    pub modified_duration: Decimal,
    pub effective_duration: Option<Decimal>,
    pub macaulay_duration: Decimal,
    pub convexity: Decimal,
    pub dv01: Decimal,
    pub spread_duration: Option<Decimal>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum QuoteCondition {
    Firm,           // Executable
    Indicative,     // Subject to confirmation
    Stale,          // Old price
    Closed,         // Market closed
    Halted,         // Trading halted
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteSize {
    pub bid_size: Option<Decimal>,
    pub ask_size: Option<Decimal>,
    pub min_size: Option<Decimal>,
    pub max_size: Option<Decimal>,
}
```

### Real-Time Quote Streaming

```rust
/// Quote stream for real-time analytics
/// Publishes BondQuote with full analytics on every market data update
#[async_trait]
pub trait QuoteStream: Send + Sync {
    /// Subscribe to bond quotes
    async fn subscribe(
        &self,
        instruments: &[InstrumentId],
    ) -> Result<QuoteReceiver, StreamError>;
    
    /// Get current quote (non-blocking)
    fn get_quote(&self, instrument_id: &InstrumentId) -> Option<BondQuote>;
    
    /// Get all current quotes
    fn get_all_quotes(&self) -> Vec<BondQuote>;
    
    /// Unsubscribe
    async fn unsubscribe(&self, instruments: &[InstrumentId]) -> Result<(), StreamError>;
}

/// Receiver for quote updates
pub struct QuoteReceiver {
    inner: broadcast::Receiver<BondQuote>,
}

impl QuoteReceiver {
    /// Receive next quote update
    pub async fn recv(&mut self) -> Result<BondQuote, RecvError>;
    
    /// Try receive (non-blocking)
    pub fn try_recv(&mut self) -> Option<BondQuote>;
}
```

### Curve Streaming

```rust
/// Curve snapshot with all points
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveSnapshot {
    pub curve_id: CurveId,
    pub as_of: jiff::Timestamp,
    pub currency: Currency,
    pub curve_type: CurveType,
    
    // All curve points
    pub points: Vec<CurvePoint>,
    
    // Build metadata
    pub build_time: jiff::Timestamp,
    pub build_duration_us: u64,
    pub input_count: usize,
    
    // For change detection
    pub checksum: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurvePoint {
    pub tenor: Tenor,
    pub years: Decimal,
    pub zero_rate: Decimal,
    pub discount_factor: Decimal,
    pub forward_rate: Option<Decimal>,
}

/// Curve stream for real-time curve updates
#[async_trait]
pub trait CurveStream: Send + Sync {
    /// Subscribe to curve updates
    async fn subscribe(&self, curve_ids: &[CurveId]) -> Result<CurveReceiver, StreamError>;
    
    /// Get current curve
    fn get_curve(&self, curve_id: &CurveId) -> Option<CurveSnapshot>;
}
```

### What Gets Streamed in Real-Time

| Data Type | Trigger | Latency Target | Contents |
|-----------|---------|----------------|----------|
| `BondQuote` | Market data update | < 100µs | Full bid/ask/mid analytics |
| `CurveSnapshot` | Curve rebuild | < 1ms | All curve points |
| `PortfolioAnalytics` | Any holding price change | < 5ms | NAV, duration, DV01 |
| `RiskMetrics` | Curve or price change | < 10ms | Full risk breakdown |

---

## Spread Methodologies

| Spread | Description | Benchmark | When to Use |
|--------|-------------|-----------|-------------|
| **G-Spread** | Yield minus interpolated government yield | Treasury/Bund/Gilt | Quick comparison, trading |
| **I-Spread** | Yield minus interpolated swap rate | SOFR/ESTR swap | Swap-relative value |
| **Z-Spread** | Constant spread over zero curve | OIS zeros | Accurate valuation |
| **OAS** | Z-spread adjusted for optionality | Zero + vol | Callable/putable bonds |
| **ASW** | Asset swap spread | Par swap rate | Swap trading basis |
| **DM** | Discount margin | Forward index | Floating rate notes |

---

## Risk Metrics

| Metric | Formula | Use Case |
|--------|---------|----------|
| **Modified Duration** | Macaulay / (1 + y/n) | Rate sensitivity (parallel) |
| **Effective Duration** | (P₋ - P₊) / (2 × P × Δy) | Callable bonds |
| **Key Rate Duration** | Bump specific tenor | Curve risk |
| **DV01** | Mod Dur × Dirty Price × 0.0001 | Dollar risk |
| **Convexity** | Second derivative | Large rate moves |
| **Spread Duration** | Price sensitivity to spread | Credit risk |
| **CS01** | Price change per 1bp spread | Credit risk |

---

## Portfolio & ETF Analytics (`convex-portfolio`)

The portfolio module provides **pure aggregation functions** over pre-calculated bond analytics. All functions receive holdings with their bond-level analytics already computed - the portfolio layer only aggregates.

### Module Structure

```
convex/src/portfolio/
├── mod.rs              # Public exports
├── composition.rs      # Portfolio/ETF composition types
├── nav.rs              # NAV and iNAV calculations  
├── aggregation.rs      # Weighted average calculations
├── yield_metrics.rs    # Portfolio yield aggregations
├── risk_metrics.rs     # Duration, DV01, KRD aggregation
├── spread_metrics.rs   # Spread metric aggregation
├── contribution.rs     # Risk contribution & attribution
├── tracking.rs         # Tracking error, benchmark comparison
├── basket.rs           # Creation/redemption basket analytics
└── liquidity.rs        # Liquidity metrics aggregation
```

### Core Types

```rust
/// Weighting scheme for aggregations
#[derive(Debug, Clone, Copy, Default)]
pub enum WeightingScheme {
    #[default]
    MarketValue,
    ParValue,
    Equal,
    Custom,
}

/// A single holding in a portfolio/ETF
#[derive(Debug, Clone)]
pub struct Holding {
    pub security_id: InstrumentId,
    pub par_amount: Decimal,
    pub market_value: Decimal,
    pub accrued_interest: Decimal,
    pub weight: Decimal,  // Pre-calculated weight
    
    // Pre-calculated bond analytics (from convex-pricing)
    pub analytics: BondAnalytics,
    
    // Classification
    pub sector: Option<Sector>,
    pub credit_rating: Option<CreditRating>,
    pub currency: Currency,
}

/// Pre-calculated bond-level analytics
#[derive(Debug, Clone)]
pub struct BondAnalytics {
    // Yields
    pub ytm: Option<Decimal>,
    pub ytw: Option<Decimal>,
    pub ytc: Option<Decimal>,
    pub current_yield: Option<Decimal>,
    
    // Duration & Convexity
    pub modified_duration: Option<Decimal>,
    pub effective_duration: Option<Decimal>,
    pub macaulay_duration: Option<Decimal>,
    pub convexity: Option<Decimal>,
    pub effective_convexity: Option<Decimal>,
    
    // DV01 & Key Rates
    pub dv01: Option<Decimal>,
    pub key_rate_durations: Option<KeyRateDurations>,
    
    // Spreads
    pub oas: Option<Decimal>,
    pub z_spread: Option<Decimal>,
    pub g_spread: Option<Decimal>,
    pub i_spread: Option<Decimal>,
    pub asw_spread: Option<Decimal>,
    
    // Credit
    pub spread_duration: Option<Decimal>,
    pub cs01: Option<Decimal>,
}

/// Portfolio/ETF composition
#[derive(Debug, Clone)]
pub struct PortfolioComposition {
    pub id: String,
    pub name: String,
    pub as_of: jiff::Timestamp,
    
    // Holdings
    pub holdings: Vec<Holding>,
    
    // Cash positions
    pub cash_positions: Vec<CashPosition>,
    
    // ETF-specific
    pub shares_outstanding: Option<u64>,
    pub management_fee_accrual: Decimal,
    pub other_liabilities: Decimal,
    
    // Benchmark reference
    pub benchmark_id: Option<String>,
}

/// Cash position in a specific currency
#[derive(Debug, Clone)]
pub struct CashPosition {
    pub currency: Currency,
    pub amount: Decimal,
    pub fx_rate_to_base: Decimal,
}
```

### Category 1: NAV & Valuation

```rust
/// NAV calculation results
pub struct NavMetrics {
    pub total_nav: Decimal,
    pub nav_per_share: Decimal,
    pub securities_value: Decimal,
    pub cash_value: Decimal,
    pub accrued_income: Decimal,
    pub liabilities: Decimal,
    pub as_of: jiff::Timestamp,
}

/// iNAV calculation results  
pub struct InavMetrics {
    pub inav_per_share: Decimal,
    pub inav_bid: Option<Decimal>,
    pub inav_ask: Option<Decimal>,
    pub staleness_score: StalenessScore,
    pub last_update: jiff::Timestamp,
}

/// Premium/discount analysis
pub struct PremiumDiscount {
    pub etf_price: Decimal,
    pub nav: Decimal,
    pub premium_discount_pct: Decimal,
    pub premium_discount_abs: Decimal,
}

impl PortfolioComposition {
    /// Calculate end-of-day NAV
    /// NAV = (Securities MV + Cash + Accruals - Liabilities) / Shares Outstanding
    pub fn calculate_nav(&self) -> NavMetrics;
    
    /// Calculate indicative NAV (caller provides live prices)
    pub fn calculate_inav(&self, live_prices: &HashMap<InstrumentId, Decimal>) -> InavMetrics;
    
    /// Calculate premium/discount given ETF market price
    pub fn premium_discount(&self, etf_price: Decimal, nav: Decimal) -> PremiumDiscount;
}
```

### Category 2: Yield Metrics Aggregation

```rust
/// Aggregated yield metrics
pub struct PortfolioYieldMetrics {
    pub weighted_avg_ytm: Decimal,
    pub weighted_avg_ytw: Decimal,
    pub weighted_avg_ytc: Option<Decimal>,
    pub weighted_avg_current_yield: Decimal,
    pub weighted_avg_coupon: Decimal,
    pub weighted_avg_maturity_years: Decimal,
    pub weighted_avg_life: Decimal,  // For amortizing
    pub weighting_scheme: WeightingScheme,
}

impl PortfolioComposition {
    /// Calculate weighted average yield metrics
    pub fn yield_metrics(&self, scheme: WeightingScheme) -> PortfolioYieldMetrics;
}
```

### Category 3: Duration & Convexity Aggregation

```rust
/// Aggregated duration metrics
pub struct PortfolioDurationMetrics {
    pub modified_duration: Decimal,
    pub effective_duration: Decimal,
    pub macaulay_duration: Decimal,
    pub spread_duration: Decimal,
    pub duration_to_worst: Decimal,
    pub dollar_duration: Decimal,  // Duration × MV
    pub convexity: Decimal,
    pub effective_convexity: Decimal,
    pub money_convexity: Decimal,
}

impl PortfolioComposition {
    /// Calculate weighted average duration/convexity
    pub fn duration_metrics(&self, scheme: WeightingScheme) -> PortfolioDurationMetrics;
}
```

### Category 4: DV01 & Key Rate Duration

```rust
/// Standard KRD tenors
pub const KRD_TENORS: [&str; 10] = ["3M", "6M", "1Y", "2Y", "3Y", "5Y", "7Y", "10Y", "20Y", "30Y"];

/// Portfolio DV01 and KRD profile
pub struct PortfolioDv01Metrics {
    pub total_dv01: Decimal,
    pub dv01_per_share: Option<Decimal>,
    pub dv01_by_sector: HashMap<Sector, Decimal>,
    pub dv01_by_rating: HashMap<CreditRating, Decimal>,
    pub dv01_by_maturity_bucket: HashMap<MaturityBucket, Decimal>,
    pub key_rate_durations: HashMap<String, Decimal>,  // Tenor -> KRD
    pub cs01: Decimal,  // Credit spread sensitivity
}

impl PortfolioComposition {
    /// Calculate portfolio DV01 = Σ(holding DV01)
    pub fn dv01_metrics(&self) -> PortfolioDv01Metrics;
    
    /// Aggregate key rate durations
    pub fn key_rate_duration_profile(&self) -> HashMap<String, Decimal>;
}
```

### Category 5: Spread Metrics Aggregation

```rust
/// Aggregated spread metrics
pub struct PortfolioSpreadMetrics {
    pub weighted_avg_oas: Decimal,
    pub weighted_avg_z_spread: Decimal,
    pub weighted_avg_g_spread: Decimal,
    pub weighted_avg_i_spread: Decimal,
    pub weighted_avg_asw: Decimal,
    pub spread_duration: Decimal,
    pub cs01: Decimal,
}

impl PortfolioComposition {
    /// Calculate weighted average spreads
    pub fn spread_metrics(&self, scheme: WeightingScheme) -> PortfolioSpreadMetrics;
}
```

### Category 6: Contribution & Attribution Analysis

```rust
/// Risk contribution breakdown
pub struct ContributionAnalysis {
    pub duration_by_sector: HashMap<Sector, Decimal>,
    pub duration_by_rating: HashMap<CreditRating, Decimal>,
    pub duration_by_maturity: HashMap<MaturityBucket, Decimal>,
    pub dv01_by_issuer: HashMap<String, Decimal>,
    pub spread_contribution_by_sector: HashMap<Sector, Decimal>,
    pub top_n_contributors: Vec<HoldingContribution>,
}

/// Single holding's contribution
pub struct HoldingContribution {
    pub security_id: InstrumentId,
    pub weight: Decimal,
    pub duration_contribution: Decimal,
    pub dv01_contribution: Decimal,
    pub spread_contribution: Decimal,
}

/// Return attribution components
pub struct ReturnAttribution {
    pub income_return: Decimal,
    pub price_return: Decimal,
    pub duration_effect: Decimal,
    pub curve_effect: Decimal,
    pub spread_effect: Decimal,
    pub residual: Decimal,
}

impl PortfolioComposition {
    /// Break down risk by classification
    pub fn contribution_analysis(&self) -> ContributionAnalysis;
    
    /// Calculate return attribution (given returns series)
    pub fn return_attribution(
        &self,
        period_return: Decimal,
        yield_change: Decimal,
        spread_change: Decimal,
    ) -> ReturnAttribution;
}
```

### Category 7: Tracking & Benchmark Comparison

```rust
/// Tracking metrics vs benchmark
pub struct TrackingMetrics {
    pub tracking_error: Decimal,
    pub tracking_difference: Decimal,
    pub information_ratio: Decimal,
    pub active_duration: Decimal,  // Portfolio duration - Benchmark duration
    pub active_spread: Decimal,
}

/// Active exposures vs benchmark
pub struct ActiveExposures {
    pub duration_difference: Decimal,
    pub spread_difference: Decimal,
    pub sector_overweights: HashMap<Sector, Decimal>,
    pub rating_overweights: HashMap<CreditRating, Decimal>,
}

impl PortfolioComposition {
    /// Calculate tracking error given return series
    pub fn tracking_error(
        &self,
        portfolio_returns: &[Decimal],
        benchmark_returns: &[Decimal],
        annualization_factor: Decimal,
    ) -> TrackingMetrics;
    
    /// Compare exposures to benchmark
    pub fn active_exposures(&self, benchmark: &PortfolioComposition) -> ActiveExposures;
}
```

### Category 8: Creation/Redemption Basket (ETF-Specific)

```rust
/// Basket analytics
pub struct BasketAnalytics {
    pub basket_duration: Decimal,
    pub basket_oas: Decimal,
    pub basket_ytw: Decimal,
    pub duration_match_score: Decimal,  // vs ETF/Index
    pub spread_match_score: Decimal,
    pub replication_ratio: Decimal,  // % of index risk captured
}

/// Creation/redemption basket
pub struct CreationRedemptionBasket {
    pub holdings: Vec<BasketHolding>,
    pub cash_component: Decimal,
    pub analytics: BasketAnalytics,
}

impl PortfolioComposition {
    /// Analyze proposed basket vs ETF characteristics
    pub fn analyze_basket(&self, basket: &[BasketHolding]) -> BasketAnalytics;
    
    /// Generate optimal creation basket (given constraints)
    pub fn generate_creation_basket(&self, constraints: &BasketConstraints) -> CreationRedemptionBasket;
}
```

### Category 9: Liquidity Metrics

```rust
/// Portfolio liquidity metrics
pub struct LiquidityMetrics {
    pub weighted_avg_bid_ask_spread: Decimal,
    pub liquidity_score: Decimal,
    pub days_to_liquidate: HashMap<LiquidationScenario, Decimal>,
    pub concentration_by_issuer: HashMap<String, Decimal>,
}

impl PortfolioComposition {
    /// Calculate liquidity metrics
    pub fn liquidity_metrics(&self) -> LiquidityMetrics;
}
```

### Category 10: Stress Testing & Scenario Analysis

```rust
/// Scenario definition
pub enum Scenario {
    ParallelShift(Decimal),  // +/- bps
    Steepening { short_change: Decimal, long_change: Decimal },
    Flattening { short_change: Decimal, long_change: Decimal },
    SpreadWidening(Decimal),  // bps
    Custom(HashMap<String, Decimal>),  // Tenor -> change
}

/// Scenario results
pub struct ScenarioResult {
    pub scenario: Scenario,
    pub portfolio_pnl: Decimal,
    pub pnl_by_holding: HashMap<InstrumentId, Decimal>,
    pub pnl_by_sector: HashMap<Sector, Decimal>,
    pub new_duration: Decimal,
}

impl PortfolioComposition {
    /// Calculate P&L under scenario
    /// Uses: ΔP ≈ -Duration × Δy + ½ × Convexity × (Δy)²
    pub fn apply_scenario(&self, scenario: &Scenario) -> ScenarioResult;
    
    /// Run multiple scenarios
    pub fn stress_test(&self, scenarios: &[Scenario]) -> Vec<ScenarioResult>;
}
```

### Performance Targets for Portfolio Analytics

| Operation | Target Latency | Notes |
|-----------|---------------|-------|
| NAV calculation (500 holdings) | < 1ms | Pre-calculated analytics |
| iNAV update | < 500µs | Price update only |
| Full analytics suite | < 5ms | All aggregations |
| Contribution analysis | < 2ms | Bucketing and sorting |
| Scenario analysis (10 scenarios) | < 10ms | Parallel execution |
| Key rate duration profile | < 1ms | Sum of KRDs |

### Design Principles for Portfolio Module

1. **Pure Functions**: All functions receive pre-calculated bond analytics as input. No I/O, no side effects, no caching.

2. **Compose with Bond Layer**: Never recalculate what the bond layer already provides:
```rust
// Good: Use pre-calculated analytics
let portfolio_duration = holdings.iter()
    .map(|h| h.weight * h.analytics.modified_duration.unwrap_or_default())
    .sum();

// Bad: Recalculating from scratch
let portfolio_duration = holdings.iter()
    .map(|h| calculate_duration(&h.bond))  // Don't do this
    .sum();
```

3. **Explicit Weighting**: All aggregation functions accept a weighting scheme parameter.

4. **Handle Missing Data**: If a holding lacks required analytics (e.g., no OAS), exclude it from that specific aggregation with appropriate logging.

5. **Match Bloomberg PORT**: Portfolio duration, DV01, and other metrics should match Bloomberg PORT methodology.

---

## Configuration Management (`convex-config`, `convex-api`, `convex-cli`)

This is the **production-grade configuration management layer** that provides clean APIs for managing all aspects of curve building, pricing, and portfolio analytics. All configuration is accessible via CLI, REST API, and Rust SDK.

### Configuration API Architecture

```rust
// =============================================================================
// CORE CONFIGURATION SERVICE TRAIT
// =============================================================================

/// Central configuration service - all interfaces (CLI, REST, SDK) use this
#[async_trait]
pub trait ConfigService: Send + Sync {
    // -------------------------------------------------------------------------
    // CURVE CONFIGURATION
    // -------------------------------------------------------------------------
    
    /// List all curve definitions
    async fn list_curves(&self, filter: Option<CurveFilter>) -> Result<Vec<CurveConfigSummary>>;
    
    /// Get a single curve configuration
    async fn get_curve(&self, curve_id: &CurveId) -> Result<CurveConfig>;
    
    /// Create or update a curve configuration
    async fn save_curve(&self, config: CurveConfig) -> Result<CurveConfig>;
    
    /// Delete a curve configuration
    async fn delete_curve(&self, curve_id: &CurveId) -> Result<()>;
    
    /// Validate a curve configuration without saving
    async fn validate_curve(&self, config: &CurveConfig) -> Result<ValidationResult>;
    
    /// Get curve build status (last build time, errors, etc.)
    async fn get_curve_status(&self, curve_id: &CurveId) -> Result<CurveBuildStatus>;
    
    // -------------------------------------------------------------------------
    // BOND PRICING CONFIGURATION
    // -------------------------------------------------------------------------
    
    /// List all bond pricing configurations
    async fn list_pricing_configs(&self, filter: Option<PricingConfigFilter>) -> Result<Vec<BondPricingConfigSummary>>;
    
    /// Get pricing config for a specific bond type
    async fn get_pricing_config(&self, config_id: &str) -> Result<BondPricingConfig>;
    
    /// Save pricing configuration
    async fn save_pricing_config(&self, config: BondPricingConfig) -> Result<BondPricingConfig>;
    
    /// Delete pricing configuration
    async fn delete_pricing_config(&self, config_id: &str) -> Result<()>;
    
    /// Get effective pricing config for a specific bond (after matching rules)
    async fn get_effective_pricing_config(&self, bond: &BondRef) -> Result<BondPricingConfig>;
    
    // -------------------------------------------------------------------------
    // SPREAD ADJUSTMENTS
    // -------------------------------------------------------------------------
    
    /// List all spread adjustments
    async fn list_spread_adjustments(&self, filter: Option<AdjustmentFilter>) -> Result<Vec<SpreadAdjustment>>;
    
    /// Add a spread adjustment
    async fn add_spread_adjustment(&self, adjustment: SpreadAdjustment) -> Result<SpreadAdjustment>;
    
    /// Update a spread adjustment
    async fn update_spread_adjustment(&self, id: &str, adjustment: SpreadAdjustment) -> Result<SpreadAdjustment>;
    
    /// Delete a spread adjustment
    async fn delete_spread_adjustment(&self, id: &str) -> Result<()>;
    
    // -------------------------------------------------------------------------
    // MANUAL OVERRIDES
    // -------------------------------------------------------------------------
    
    /// List all active overrides
    async fn list_overrides(&self, filter: Option<OverrideFilter>) -> Result<Vec<PriceOverride>>;
    
    /// Get override for a specific instrument
    async fn get_override(&self, instrument_id: &InstrumentId) -> Result<Option<PriceOverride>>;
    
    /// Create or update an override
    async fn save_override(&self, override_: PriceOverride) -> Result<PriceOverride>;
    
    /// Delete an override
    async fn delete_override(&self, instrument_id: &InstrumentId) -> Result<()>;
    
    /// Approve a pending override (if approval workflow enabled)
    async fn approve_override(&self, instrument_id: &InstrumentId, approver: &str) -> Result<PriceOverride>;
    
    /// Clear all expired overrides
    async fn clear_expired_overrides(&self) -> Result<u32>;
    
    // -------------------------------------------------------------------------
    // MARKET DATA MAPPINGS
    // -------------------------------------------------------------------------
    
    /// List all market data source mappings
    async fn list_market_data_mappings(&self) -> Result<Vec<MarketDataMapping>>;
    
    /// Get mapping for a specific key
    async fn get_market_data_mapping(&self, key: &MarketDataKey) -> Result<MarketDataSource>;
    
    /// Save market data mapping
    async fn save_market_data_mapping(&self, mapping: MarketDataMapping) -> Result<MarketDataMapping>;
    
    // -------------------------------------------------------------------------
    // PORTFOLIO CONFIGURATION
    // -------------------------------------------------------------------------
    
    /// List portfolio definitions
    async fn list_portfolios(&self) -> Result<Vec<PortfolioConfigSummary>>;
    
    /// Get portfolio configuration
    async fn get_portfolio_config(&self, portfolio_id: &str) -> Result<PortfolioConfig>;
    
    /// Save portfolio configuration
    async fn save_portfolio_config(&self, config: PortfolioConfig) -> Result<PortfolioConfig>;
    
    // -------------------------------------------------------------------------
    // BULK OPERATIONS
    // -------------------------------------------------------------------------
    
    /// Export all configuration to a bundle
    async fn export_config(&self, format: ExportFormat) -> Result<ConfigBundle>;
    
    /// Import configuration from a bundle
    async fn import_config(&self, bundle: ConfigBundle, mode: ImportMode) -> Result<ImportResult>;
    
    /// Diff two configurations
    async fn diff_config(&self, from: &ConfigBundle, to: &ConfigBundle) -> Result<ConfigDiff>;
    
    // -------------------------------------------------------------------------
    // AUDIT & HISTORY
    // -------------------------------------------------------------------------
    
    /// Get configuration change history
    async fn get_audit_log(&self, filter: AuditFilter) -> Result<Vec<AuditEntry>>;
    
    /// Get specific version of a config
    async fn get_config_version(&self, config_type: ConfigType, id: &str, version: u64) -> Result<ConfigSnapshot>;
    
    /// Rollback to a previous version
    async fn rollback_config(&self, config_type: ConfigType, id: &str, version: u64) -> Result<()>;
}

// =============================================================================
// FILTER & QUERY TYPES
// =============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CurveFilter {
    pub currency: Option<Currency>,
    pub curve_type: Option<CurveType>,
    pub name_contains: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PricingConfigFilter {
    pub currency: Option<Currency>,
    pub issuer_type: Option<IssuerType>,
    pub sector: Option<Sector>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OverrideFilter {
    pub instrument_id: Option<InstrumentId>,
    pub override_type: Option<OverrideType>,
    pub entered_by: Option<String>,
    pub include_expired: bool,
    pub pending_approval_only: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AdjustmentFilter {
    pub adjustment_type: Option<String>,
    pub sector: Option<Sector>,
    pub currency: Option<Currency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditFilter {
    pub config_type: Option<ConfigType>,
    pub changed_by: Option<String>,
    pub from_time: Option<jiff::Timestamp>,
    pub to_time: Option<jiff::Timestamp>,
    pub limit: Option<u32>,
}

// =============================================================================
// SUMMARY TYPES (for list operations)
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurveConfigSummary {
    pub curve_id: CurveId,
    pub curve_type: CurveType,
    pub currency: Currency,
    pub instrument_count: usize,
    pub last_modified: jiff::Timestamp,
    pub last_built: Option<jiff::Timestamp>,
    pub status: CurveStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondPricingConfigSummary {
    pub id: String,
    pub name: String,
    pub applies_to: BondMatcherSummary,
    pub default_spread_type: SpreadType,
    pub last_modified: jiff::Timestamp,
}

// =============================================================================
// VALIDATION
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub field: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationWarning {
    pub field: String,
    pub code: String,
    pub message: String,
}

// =============================================================================
// IMPORT/EXPORT
// =============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ExportFormat {
    Json,
    Toml,
    Yaml,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ImportMode {
    /// Fail if any config already exists
    CreateOnly,
    /// Update existing, create new
    Upsert,
    /// Replace all config (dangerous!)
    ReplaceAll,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigBundle {
    pub version: String,
    pub exported_at: jiff::Timestamp,
    pub exported_by: Option<String>,
    pub curves: Vec<CurveConfig>,
    pub pricing_configs: Vec<BondPricingConfig>,
    pub spread_adjustments: Vec<SpreadAdjustment>,
    pub overrides: Vec<PriceOverride>,
    pub market_data_mappings: Vec<MarketDataMapping>,
    pub portfolios: Vec<PortfolioConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportResult {
    pub curves_created: u32,
    pub curves_updated: u32,
    pub pricing_configs_created: u32,
    pub pricing_configs_updated: u32,
    pub errors: Vec<ImportError>,
}

// =============================================================================
// AUDIT
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: String,
    pub timestamp: jiff::Timestamp,
    pub config_type: ConfigType,
    pub config_id: String,
    pub action: AuditAction,
    pub changed_by: String,
    pub old_value: Option<serde_json::Value>,
    pub new_value: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum AuditAction {
    Create,
    Update,
    Delete,
    Approve,
    Rollback,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ConfigType {
    Curve,
    PricingConfig,
    SpreadAdjustment,
    Override,
    MarketDataMapping,
    Portfolio,
}
```

### CLI Interface (`convex-cli`)

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "convex")]
#[command(about = "Convex Bond Pricing Framework CLI")]
pub struct Cli {
    /// Configuration file path
    #[arg(short, long, default_value = "convex.toml")]
    pub config: PathBuf,
    
    /// Output format
    #[arg(short, long, default_value = "table")]
    pub format: OutputFormat,
    
    /// API endpoint (for remote operations)
    #[arg(long, env = "CONVEX_API_URL")]
    pub api_url: Option<String>,
    
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Curve configuration management
    Curve {
        #[command(subcommand)]
        action: CurveCommands,
    },
    
    /// Bond pricing configuration
    Pricing {
        #[command(subcommand)]
        action: PricingCommands,
    },
    
    /// Spread adjustment management
    Adjustment {
        #[command(subcommand)]
        action: AdjustmentCommands,
    },
    
    /// Manual override management
    Override {
        #[command(subcommand)]
        action: OverrideCommands,
    },
    
    /// Portfolio configuration
    Portfolio {
        #[command(subcommand)]
        action: PortfolioCommands,
    },
    
    /// Configuration import/export
    Config {
        #[command(subcommand)]
        action: ConfigCommands,
    },
    
    /// Real-time pricing operations
    Price {
        #[command(subcommand)]
        action: PriceCommands,
    },
    
    /// Server management
    Server {
        #[command(subcommand)]
        action: ServerCommands,
    },
}

#[derive(Subcommand)]
pub enum CurveCommands {
    /// List all curves
    List {
        #[arg(long)]
        currency: Option<Currency>,
        #[arg(long)]
        curve_type: Option<String>,
    },
    
    /// Get curve details
    Get {
        /// Curve ID
        curve_id: String,
    },
    
    /// Create a new curve
    Create {
        /// Path to curve config file (JSON/TOML/YAML)
        #[arg(short, long)]
        file: Option<PathBuf>,
        /// Or provide inline JSON
        #[arg(long)]
        json: Option<String>,
    },
    
    /// Update an existing curve
    Update {
        curve_id: String,
        #[arg(short, long)]
        file: Option<PathBuf>,
    },
    
    /// Delete a curve
    Delete {
        curve_id: String,
        #[arg(long)]
        force: bool,
    },
    
    /// Validate a curve configuration
    Validate {
        #[arg(short, long)]
        file: PathBuf,
    },
    
    /// Build/rebuild a curve
    Build {
        curve_id: String,
        #[arg(long)]
        as_of: Option<String>,
    },
    
    /// Show curve build status
    Status {
        curve_id: String,
    },
    
    /// Show curve points
    Points {
        curve_id: String,
        #[arg(long)]
        tenors: Option<Vec<String>>,
    },
}

#[derive(Subcommand)]
pub enum OverrideCommands {
    /// List active overrides
    List {
        #[arg(long)]
        include_expired: bool,
        #[arg(long)]
        pending_only: bool,
    },
    
    /// Get override for an instrument
    Get {
        instrument_id: String,
    },
    
    /// Set a price override
    Set {
        instrument_id: String,
        #[arg(long)]
        price: Option<Decimal>,
        #[arg(long)]
        yield_: Option<Decimal>,
        #[arg(long)]
        spread: Option<Decimal>,
        #[arg(long)]
        spread_type: Option<SpreadType>,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        expiry: Option<String>,
    },
    
    /// Remove an override
    Remove {
        instrument_id: String,
    },
    
    /// Approve a pending override
    Approve {
        instrument_id: String,
    },
    
    /// Clear all expired overrides
    ClearExpired,
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Export all configuration
    Export {
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long, default_value = "json")]
        format: ExportFormat,
    },
    
    /// Import configuration
    Import {
        #[arg(short, long)]
        input: PathBuf,
        #[arg(long, default_value = "upsert")]
        mode: ImportMode,
        #[arg(long)]
        dry_run: bool,
    },
    
    /// Compare two configuration files
    Diff {
        from: PathBuf,
        to: PathBuf,
    },
    
    /// Show audit log
    Audit {
        #[arg(long)]
        config_type: Option<ConfigType>,
        #[arg(long)]
        limit: Option<u32>,
    },
    
    /// Rollback to previous version
    Rollback {
        config_type: ConfigType,
        config_id: String,
        version: u64,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Table,
    Json,
    Yaml,
    Csv,
}
```

**CLI Usage Examples:**

```bash
# Curve management
convex curve list --currency USD
convex curve get USD.GOVT
convex curve create --file curves/usd_treasury.toml
convex curve build USD.GOVT --as-of 2025-12-28
convex curve points USD.GOVT --tenors 2Y,5Y,10Y,30Y

# Pricing configuration
convex pricing list --issuer-type corporate
convex pricing get usd-ig-corporate
convex pricing create --file pricing/usd_corporate.toml

# Spread adjustments
convex adjustment list --sector financials
convex adjustment add --type subordination --tier AT1 --bps 150 --reason "AT1 CoCo premium"

# Manual overrides
convex override list --pending-only
convex override set US037833DV96 --spread 125 --spread-type z_spread --reason "Illiquid, last trade wide"
convex override approve US037833DV96
convex override clear-expired

# Configuration management
convex config export --output backup.json --format json
convex config import --input backup.json --mode upsert --dry-run
convex config diff prod-config.json dev-config.json
convex config audit --config-type curve --limit 50
convex config rollback curve USD.GOVT 5

# Real-time pricing
convex price bond US037833DV96 --show-breakdown
convex price portfolio MAIN_PORTFOLIO --output portfolio_prices.csv

# Server management
convex server start --config convex.toml
convex server status
convex server reload-config
```

### REST API (`convex-api`)

```rust
use axum::{Router, routing::{get, post, put, delete}};

pub fn create_router(config_service: Arc<dyn ConfigService>) -> Router {
    Router::new()
        // -------------------------------------------------------------------------
        // CURVE ENDPOINTS
        // -------------------------------------------------------------------------
        .route("/api/v1/curves", get(list_curves).post(create_curve))
        .route("/api/v1/curves/:curve_id", get(get_curve).put(update_curve).delete(delete_curve))
        .route("/api/v1/curves/:curve_id/validate", post(validate_curve))
        .route("/api/v1/curves/:curve_id/build", post(build_curve))
        .route("/api/v1/curves/:curve_id/status", get(get_curve_status))
        .route("/api/v1/curves/:curve_id/points", get(get_curve_points))
        
        // -------------------------------------------------------------------------
        // PRICING CONFIG ENDPOINTS
        // -------------------------------------------------------------------------
        .route("/api/v1/pricing-configs", get(list_pricing_configs).post(create_pricing_config))
        .route("/api/v1/pricing-configs/:id", get(get_pricing_config).put(update_pricing_config).delete(delete_pricing_config))
        .route("/api/v1/pricing-configs/effective", post(get_effective_pricing_config))
        
        // -------------------------------------------------------------------------
        // SPREAD ADJUSTMENT ENDPOINTS
        // -------------------------------------------------------------------------
        .route("/api/v1/adjustments", get(list_adjustments).post(create_adjustment))
        .route("/api/v1/adjustments/:id", get(get_adjustment).put(update_adjustment).delete(delete_adjustment))
        
        // -------------------------------------------------------------------------
        // OVERRIDE ENDPOINTS
        // -------------------------------------------------------------------------
        .route("/api/v1/overrides", get(list_overrides).post(create_override))
        .route("/api/v1/overrides/:instrument_id", get(get_override).put(update_override).delete(delete_override))
        .route("/api/v1/overrides/:instrument_id/approve", post(approve_override))
        .route("/api/v1/overrides/clear-expired", post(clear_expired_overrides))
        
        // -------------------------------------------------------------------------
        // MARKET DATA MAPPING ENDPOINTS
        // -------------------------------------------------------------------------
        .route("/api/v1/market-data-mappings", get(list_mappings).post(create_mapping))
        .route("/api/v1/market-data-mappings/:key", get(get_mapping).put(update_mapping).delete(delete_mapping))
        
        // -------------------------------------------------------------------------
        // PORTFOLIO ENDPOINTS
        // -------------------------------------------------------------------------
        .route("/api/v1/portfolios", get(list_portfolios).post(create_portfolio))
        .route("/api/v1/portfolios/:id", get(get_portfolio).put(update_portfolio).delete(delete_portfolio))
        .route("/api/v1/portfolios/:id/analytics", get(get_portfolio_analytics))
        
        // -------------------------------------------------------------------------
        // PRICING ENDPOINTS
        // -------------------------------------------------------------------------
        .route("/api/v1/price/bond/:instrument_id", get(price_bond))
        .route("/api/v1/price/bonds", post(price_bonds_batch))
        .route("/api/v1/price/portfolio/:portfolio_id", get(price_portfolio))
        
        // -------------------------------------------------------------------------
        // CONFIG MANAGEMENT ENDPOINTS
        // -------------------------------------------------------------------------
        .route("/api/v1/config/export", get(export_config))
        .route("/api/v1/config/import", post(import_config))
        .route("/api/v1/config/diff", post(diff_config))
        .route("/api/v1/config/audit", get(get_audit_log))
        .route("/api/v1/config/:type/:id/versions", get(list_config_versions))
        .route("/api/v1/config/:type/:id/versions/:version", get(get_config_version))
        .route("/api/v1/config/:type/:id/rollback/:version", post(rollback_config))
        
        // -------------------------------------------------------------------------
        // WEBSOCKET FOR REAL-TIME UPDATES
        // -------------------------------------------------------------------------
        .route("/ws/prices", get(ws_price_updates))
        .route("/ws/config-changes", get(ws_config_changes))
        
        // -------------------------------------------------------------------------
        // HEALTH & METRICS
        // -------------------------------------------------------------------------
        .route("/health", get(health_check))
        .route("/ready", get(readiness_check))
        .route("/metrics", get(prometheus_metrics))
        
        .with_state(config_service)
}

// OpenAPI documentation
#[derive(OpenApi)]
#[openapi(
    paths(
        list_curves, get_curve, create_curve, update_curve, delete_curve,
        list_pricing_configs, get_pricing_config, create_pricing_config,
        list_overrides, get_override, create_override, approve_override,
        export_config, import_config,
        price_bond, price_bonds_batch, price_portfolio,
    ),
    components(schemas(
        CurveConfig, CurveConfigSummary, CurveBuildStatus,
        BondPricingConfig, BondPricingConfigSummary,
        SpreadAdjustment, PriceOverride,
        ValidationResult, ConfigBundle, ImportResult,
        AuditEntry,
    )),
    tags(
        (name = "curves", description = "Curve configuration management"),
        (name = "pricing", description = "Pricing configuration"),
        (name = "overrides", description = "Manual price overrides"),
        (name = "config", description = "Configuration import/export"),
    )
)]
pub struct ApiDoc;
```

**REST API Examples:**

```bash
# List curves
curl -X GET "http://localhost:8080/api/v1/curves?currency=USD"

# Create a curve
curl -X POST "http://localhost:8080/api/v1/curves" \
  -H "Content-Type: application/json" \
  -d @curves/usd_treasury.json

# Build a curve
curl -X POST "http://localhost:8080/api/v1/curves/USD.GOVT/build" \
  -H "Content-Type: application/json" \
  -d '{"as_of": "2025-12-28T16:00:00Z"}'

# Set an override
curl -X POST "http://localhost:8080/api/v1/overrides" \
  -H "Content-Type: application/json" \
  -d '{
    "instrument_id": "US037833DV96",
    "override_type": "spread",
    "spread_type": "z_spread",
    "value": 125.0,
    "reason": "Illiquid, last trade +15bps",
    "expiry": "2025-01-15T17:00:00Z"
  }'

# Export all config
curl -X GET "http://localhost:8080/api/v1/config/export?format=json" > backup.json

# Price a bond
curl -X GET "http://localhost:8080/api/v1/price/bond/US037833DV96?show_breakdown=true"
```

### Configuration File Format (TOML)

```toml
# config/convex.toml - Main configuration file

[service]
name = "convex-pricer"
instance_id = "${HOSTNAME}"
api_port = 8080
grpc_port = 9090

[storage]
type = "redb"
path = "/var/lib/convex/data"

[market_data]
primary_provider = "bloomberg"
failover_providers = ["refinitiv", "ice"]
conflation_window_ms = 100
reconnect_delay_ms = 5000

[calculation]
thread_pool_size = 8
batch_size = 1000
cache_ttl_seconds = 300

[observability]
otlp_endpoint = "http://otel-collector:4317"
metrics_port = 9090
log_level = "info"
log_format = "json"

[curves]
default_interpolation = "monotone_convex"
turn_of_year_handling = true

[pricing]
default_day_count = "ACT/ACT_ICMA"
yield_tolerance = 1e-10
max_iterations = 100

[overrides]
require_approval = true
max_expiry_days = 30
audit_retention_days = 365

[api]
enable_swagger = true
cors_origins = ["http://localhost:3000"]
rate_limit_per_minute = 1000
```

---

## Performance Targets

### Bond-Level Operations
| Operation | Target Latency | Notes |
|-----------|---------------|-------|
| Single bond price | < 1µs | Cached curve |
| YTM calculation | < 500ns | Newton-Raphson |
| Z-spread | < 5µs | Iterative solve |
| Curve bootstrap (50 points) | < 100µs | Parallel interpolation |
| Quote ingestion | < 10µs | Zero-copy decode |
| Batch pricing (1000 bonds) | < 1ms | Parallelized |

### Real-Time Trading Operations
| Operation | Target Latency | Notes |
|-----------|---------------|-------|
| Quote → Full Analytics (bid/ask/mid) | < 100µs | Single bond, cached curves |
| Quote subscription end-to-end | < 1ms | Market data to UI |
| YTM from price | < 50µs | Newton-Raphson, 3-5 iterations |
| All spreads (G, I, Z) | < 100µs | Curve interpolation |
| Batch price 100 bonds | < 5ms | Parallel processing |
| Curve cache hot swap | < 1ms | Atomic replace |
| RFQ analysis | < 10ms | Includes historical context |
| Watchlist update (50 bonds) | < 5ms | Full refresh |

### Trading Throughput
| Metric | Target | Notes |
|--------|--------|-------|
| Quote updates/second | 10,000 | Per instrument |
| Bonds priced/second | 1,000 | Batch mode |
| Concurrent subscriptions | 100 | Per instance |
| WebSocket clients | 1,000 | Per server |

### Portfolio/ETF Operations
| Operation | Target Latency | Notes |
|-----------|---------------|-------|
| NAV calculation (500 holdings) | < 1ms | Pre-calculated analytics |
| iNAV update | < 500µs | Price update only |
| Full portfolio analytics | < 5ms | All aggregations |
| Contribution analysis | < 2ms | Bucketing and sorting |
| Scenario analysis (10 scenarios) | < 10ms | Parallel execution |
| Key rate duration profile | < 1ms | Sum of KRDs |
| Creation basket optimization | < 50ms | Constrained optimization |

---

## Testing Strategy

```rust
// Property-based testing for numerical stability
#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use approx::assert_relative_eq;
    
    proptest! {
        #[test]
        fn yield_price_roundtrip(
            yield_pct in 0.001f64..0.15,
            coupon_pct in 0.0f64..0.10,
            years_to_maturity in 1u32..30,
        ) {
            let bond = create_test_bond(coupon_pct, years_to_maturity);
            let ytm = Yield::from_percent(yield_pct);
            
            let price = bond.price_from_yield(ytm)?;
            let recovered_yield = bond.yield_from_price(price)?;
            
            assert_relative_eq!(
                ytm.as_decimal(),
                recovered_yield.as_decimal(),
                epsilon = 1e-10
            );
        }
    }
}
```

---

## Bloomberg YAS Parity Checklist

**Every calculation MUST be validated against Bloomberg YAS before release.**

### Required Validation Matrix

| Metric | Bloomberg Field | Tolerance | Test Cases Required |
|--------|-----------------|-----------|---------------------|
| Clean Price | PX_CLEAN | 0.0001 | 50+ bonds |
| Dirty Price | PX_DIRTY | 0.0001 | 50+ bonds |
| Accrued Interest | INT_ACC | 0.0001 | All day counts |
| YTM | YLD_YTM_MID | 0.0001% | All frequencies |
| YTW | YLD_YTM_WORST | 0.0001% | Callable bonds |
| Modified Duration | DUR_ADJ_MID | 0.0001 | 50+ bonds |
| Effective Duration | DUR_ADJ_OAS_MID | 0.001 | Callable bonds |
| Macaulay Duration | DUR_MID | 0.0001 | 50+ bonds |
| Z-Spread | Z_SPRD_MID | 0.1bp | 50+ corporates |
| G-Spread | G_SPRD_MID | 0.1bp | 50+ corporates |
| I-Spread | I_SPRD_MID | 0.1bp | 50+ corporates |
| OAS | OAS_SPREAD_MID | 0.5bp | Callable bonds |
| ASW | ASSET_SWAP_SPD_MID | 0.5bp | 50+ corporates |
| Convexity | CONVEXITY | 0.001 | 50+ bonds |
| DV01 | RISK_MID | 0.0001 | 50+ bonds |

### Test Bond Universe

Maintain a test suite covering:

```rust
/// Test universe for Bloomberg validation
/// Update quarterly with fresh Bloomberg data
pub const VALIDATION_BONDS: &[ValidationBond] = &[
    // US Treasuries (all tenors)
    ValidationBond { cusip: "912797KT8", name: "T-Bill 3M", category: "UST" },
    ValidationBond { cusip: "91282CJN6", name: "Treasury 5Y", category: "UST" },
    ValidationBond { cusip: "91282CJP1", name: "Treasury 10Y", category: "UST" },
    ValidationBond { cusip: "912810TX6", name: "Treasury 30Y", category: "UST" },
    
    // US Corporates (IG, various sectors)
    ValidationBond { cusip: "037833DV9", name: "Apple 3.85% 2043", category: "IG_CORP" },
    ValidationBond { cusip: "38141GXZ2", name: "Goldman 6.75% 2037", category: "FIN" },
    
    // Callable bonds
    ValidationBond { cusip: "XXX", name: "Bank callable", category: "CALLABLE" },
    
    // EUR Government (Bunds)
    ValidationBond { isin: "DE0001102580", name: "DBR 0% 2052", category: "EUR_GOVT" },
    
    // GBP Gilts
    ValidationBond { isin: "GB00BDRHNP05", name: "UKT 1.5% 2047", category: "GBP_GILT" },
    
    // FRNs
    ValidationBond { cusip: "XXX", name: "SOFR FRN", category: "FRN" },
];
```

### Validation Workflow

```bash
# 1. Export Bloomberg reference data
convex validate export-bbg-data --bonds validation_universe.csv --output bbg_reference.json

# 2. Run validation suite
convex validate run --reference bbg_reference.json --report validation_report.html

# 3. Review failures
convex validate failures --threshold 0.01

# 4. Generate compliance report
convex validate compliance-report --output compliance.pdf
```

### Quarterly Validation Requirement

- **Every quarter**: Re-run full validation suite against fresh Bloomberg data
- **Every release**: Run validation before any release
- **Document exceptions**: Any tolerance breaches must be documented with explanation

---

## Development Workflow

```bash
# Run all tests with coverage
cargo tarpaulin --out Html

# Run benchmarks
cargo bench --bench pricing

# Lint and format
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check

# Security audit
cargo audit

# Generate documentation
cargo doc --no-deps --open

# Build release with LTO
cargo build --release --features "simd,full"

# Profile hot paths
cargo flamegraph --bench pricing -- --bench single_bond

# Run Bloomberg validation (requires Bloomberg Terminal access)
cargo test --test bloomberg_validation --features bbg-validation
```

---

## Migration Path from Existing System

1. **Phase 1**: Core library (pricing, curves, day counts)
2. **Phase 2**: Market data adapters (Bloomberg, Refinitiv)
3. **Phase 3**: Calculation engine with dependency graph
4. **Phase 4**: Storage layer and reference data
5. **Phase 5**: Observability and production hardening
6. **Phase 6**: FFI bindings (Excel, Python)
7. **Phase 7**: Real-time streaming and subscriptions

---

## Key References

**Textbooks (Implementation Authority):**
- **Fabozzi**: Fixed Income Mathematics, 4th ed - Day counts, yield calculations
- **Tuckman & Serrat**: Fixed Income Securities, 3rd ed - Risk metrics, curves
- **Hull**: Options, Futures, and Other Derivatives, 11th ed - OAS, tree models

**Standards Documents:**
- **ISDA 2006 Definitions** - Day count conventions, business days
- **ICMA Primary Market Handbook** - Yield calculation for bonds
- **SIFMA Standard Formulas** - US market conventions

**Technical Papers:**
- **Hagan & West (2006)**: "Interpolation Methods for Curve Construction" - Monotone convex
- **Andersen & Piterbarg**: Interest Rate Modeling - Multi-curve framework
- **Hull & White (1990)**: OAS model specification

**Regulatory:**
- **SEC Rule 22c-1**: Fund NAV requirements
- **MiFID II RTS 25**: Bond liquidity thresholds
- **Basel III/IV**: Risk metric definitions
- **Bloomberg YAS**: Terminal validation source

---

## December 2025 Technology Choices Summary

### Why These Choices?

| Component | Choice | Rationale |
|-----------|--------|-----------|
| **Date/Time** | `jiff 0.2` | Superior timezone handling, 1.0 coming Summer 2025, replacing chrono as community standard |
| **Decimal** | `rust_decimal 1.36` | No floating point errors in finance, full maths support |
| **Serialization** | `rkyv 0.8` | True zero-copy, fastest deserialization, essential for hot path |
| **Database** | `redb 2.4` (default) | Pure Rust, stable file format, LMDB-inspired performance, no C deps |
| **External DB** | Trait only | `StorageAdapter` trait provided; PostgreSQL/Mongo/etc implementations are user responsibility |
| **Lock-free** | `dashmap 6` + `crossbeam 0.8` | DashMap for concurrent maps, crossbeam for queues/channels |
| **Incremental** | `salsa 0.18` | Used by rust-analyzer, proven at scale, automatic cache invalidation |
| **Observability** | OpenTelemetry `0.27` | Unified traces/metrics/logs, vendor-neutral, industry standard |
| **Async Runtime** | `tokio 1.42` | Production proven; consider `monoio` for io_uring on Linux |
| **Error Handling** | `thiserror 2` | New major version with improved derive macros |

### Key 2025 Trends Incorporated

1. **jiff over chrono**: Better timezone/DST handling, cleaner API, actively maintained by BurntSushi
2. **redb maturity**: Now stable file format, production-ready, benchmarks competitive with LMDB
3. **rkyv 0.8**: New major version with improved validation API
4. **OpenTelemetry stabilizing**: Rust SDK approaching production-ready for all signals
5. **Thread-per-core awareness**: monoio/compio for extreme low-latency, but tokio still default
6. **Edition 2024**: Rust 1.83 as minimum, using latest language features
7. **Pluggable storage**: Trait-based design allows external DBs without framework bloat

---

## Notes

- **Always read existing code first** before proposing changes
- All dates use **jiff** (1.0 coming Summer 2025) - superior timezone handling
- **rust_decimal** for all monetary calculations (no floating point)
- **rkyv** for zero-copy serialization on hot paths
- **redb** as default embedded database (pure Rust, stable format)
- **External databases (PostgreSQL, MongoDB, etc.) are OUT OF SCOPE** - users implement `StorageAdapter` trait
- **salsa** for incremental computation / memoization
- OpenTelemetry for observability (traces, metrics, logs unified)
- Consider **monoio** for io_uring on Linux for extreme low latency

## Scope Boundaries

### In Scope (Included in Framework)

**Instruments (`convex-bonds` - extend existing):**
- Complete bond reference data model
- All bond types: fixed, zero, FRN, callable, putable, sinkable, inflation-linked, step-up
- Call/put schedules with make-whole, Bermudan, European, American
- Sink fund schedules (mandatory, optional)
- Amortization schedules
- Floating rate indices (SOFR, ESTR, SONIA, EURIBOR, etc.)
- Inflation indices (CPI-U, RPI, HICP, etc.)
- Subordination tiers (Senior, SNP, Tier2, AT1)
- CoCo/contingent convertible features
- Issuer reference data
- Benchmark information (spread at issue, benchmark curve)

**Conventions (`convex-conventions`):**
- All day count conventions (30/360, ACT/360, ACT/ACT ICMA, etc.)
- Holiday calendars by financial center
- Business day rules (Following, Modified Following, etc.)
- Settlement conventions by market

**Curves (`convex-curves`):**
- Government curves (Treasury, Bund, Gilt, JGB)
- OIS curves (SOFR, ESTR, SONIA)
- Swap curves
- Credit curves (issuer, sector, rating)
- Basis curves
- Bootstrapping from deposits, futures, swaps, bonds
- Interpolation methods (linear, monotone convex, cubic spline)
- Real-time curve streaming (`CurveSnapshot`, `CurveStream`)

**Pricing (`convex-pricing`):**
- All yield calculations (YTM, YTW, YTC, current yield)
- All spread calculations (G-spread, I-spread, Z-spread, OAS, ASW, DM)
- All duration types (Macaulay, modified, effective, key rate, spread)
- Convexity
- DV01, CS01
- FRN pricing (discount margin, projected cashflows)
- Callable/putable bond OAS (Hull-White)

**Portfolio (`convex-portfolio`):**
- NAV, iNAV calculations
- Weighted average metrics (yield, duration, spread)
- Portfolio DV01, key rate duration profile
- Contribution analysis
- Tracking error vs benchmark
- Creation/redemption basket analytics

**Engine (`convex-engine`):**
- Real-time pricing engine (< 100µs quote → analytics)
- `BondQuote` with full bid/ask/mid analytics
- `QuoteStream` for real-time subscriptions
- `CurveStream` for curve updates
- Calculation graph with dependency tracking
- Curve cache with hot swap
- Diagnostic capture/replay
- Timing breakdown
- Batch pricing (1000 bonds/second)

**Configuration (`convex-config`):**
- `CurveConfig` definitions
- `PricingConfig` per bond type
- Spread adjustments (sector, rating, subordination, liquidity)
- Pricing hierarchy and overrides
- Version history and audit trail
- Import/export

**Server (`convex-server`):**
- REST API for all operations
- WebSocket for real-time quotes and curves
- CLI for all operations
- OpenAPI documentation
- Prometheus metrics
- Health checks

**Storage (`convex-storage`):**
- `StorageAdapter` trait
- `RedbStorage` embedded implementation

**Bindings (`convex-ffi`, `convex-wasm`):**
- Python (PyO3)
- Excel (XLL)
- C API
- WebAssembly

---

## Stateful Services Architecture

**This is NOT a stateless library.** The system maintains state across:

---

## Enterprise Deployment Requirements

### Scalability & High Availability

All services MUST be designed for enterprise deployment with the following requirements:

```rust
// =============================================================================
// ENTERPRISE SERVICE PATTERNS
// =============================================================================

/// All services must implement health checking
#[async_trait]
pub trait HealthCheck: Send + Sync {
    /// Liveness check - is the service running?
    async fn is_alive(&self) -> bool;
    
    /// Readiness check - is the service ready to handle requests?
    async fn is_ready(&self) -> Result<(), HealthError>;
    
    /// Detailed health status for monitoring
    async fn health_status(&self) -> HealthStatus;
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthStatus {
    pub status: ServiceStatus,
    pub uptime_seconds: u64,
    pub last_error: Option<String>,
    pub dependencies: Vec<DependencyHealth>,
    pub metrics: HealthMetrics,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthMetrics {
    pub requests_total: u64,
    pub requests_failed: u64,
    pub avg_latency_ms: f64,
    pub p99_latency_ms: f64,
    pub active_connections: u32,
    pub queue_depth: u32,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum ServiceStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Starting,
    ShuttingDown,
}

/// Graceful shutdown support
#[async_trait]
pub trait GracefulShutdown: Send + Sync {
    /// Signal shutdown - stop accepting new work
    async fn shutdown(&self);
    
    /// Wait for in-flight work to complete (with timeout)
    async fn wait_for_completion(&self, timeout: Duration) -> bool;
    
    /// Force immediate shutdown
    async fn force_shutdown(&self);
}
```

### Circuit Breaker Pattern

```rust
/// Circuit breaker for external dependencies (market data, databases, etc.)
pub struct CircuitBreaker {
    state: AtomicU8,  // Closed=0, Open=1, HalfOpen=2
    failure_count: AtomicU32,
    success_count: AtomicU32,
    last_failure: AtomicU64,
    config: CircuitBreakerConfig,
}

#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening circuit
    pub failure_threshold: u32,
    
    /// Time to wait before trying half-open
    pub reset_timeout: Duration,
    
    /// Number of successes in half-open before closing
    pub success_threshold: u32,
    
    /// Timeout for individual calls
    pub call_timeout: Duration,
}

impl CircuitBreaker {
    pub async fn call<F, T, E>(&self, f: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: Future<Output = Result<T, E>>,
    {
        match self.state() {
            State::Open => Err(CircuitBreakerError::Open),
            State::HalfOpen | State::Closed => {
                match timeout(self.config.call_timeout, f).await {
                    Ok(Ok(result)) => {
                        self.record_success();
                        Ok(result)
                    }
                    Ok(Err(e)) => {
                        self.record_failure();
                        Err(CircuitBreakerError::ServiceError(e))
                    }
                    Err(_) => {
                        self.record_failure();
                        Err(CircuitBreakerError::Timeout)
                    }
                }
            }
        }
    }
}
```

### Retry with Backoff

```rust
/// Retry configuration with exponential backoff
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    
    /// Initial delay between retries
    pub initial_delay: Duration,
    
    /// Maximum delay between retries
    pub max_delay: Duration,
    
    /// Backoff multiplier (e.g., 2.0 for exponential)
    pub multiplier: f64,
    
    /// Add jitter to prevent thundering herd
    pub jitter: bool,
    
    /// Errors that should trigger retry
    pub retryable_errors: Vec<ErrorKind>,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            multiplier: 2.0,
            jitter: true,
            retryable_errors: vec![
                ErrorKind::Timeout,
                ErrorKind::ConnectionReset,
                ErrorKind::ServiceUnavailable,
            ],
        }
    }
}

/// Retry helper
pub async fn with_retry<F, Fut, T, E>(
    config: &RetryConfig,
    mut f: F,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: Into<ErrorKind>,
{
    let mut attempts = 0;
    let mut delay = config.initial_delay;
    
    loop {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) if attempts < config.max_retries && config.retryable_errors.contains(&e.into()) => {
                attempts += 1;
                let jitter = if config.jitter {
                    rand::thread_rng().gen_range(0..delay.as_millis() as u64 / 4)
                } else {
                    0
                };
                tokio::time::sleep(delay + Duration::from_millis(jitter)).await;
                delay = std::cmp::min(
                    Duration::from_secs_f64(delay.as_secs_f64() * config.multiplier),
                    config.max_delay,
                );
            }
            Err(e) => return Err(e),
        }
    }
}
```

### Rate Limiting

```rust
/// Rate limiter for API endpoints and external calls
pub struct RateLimiter {
    /// Token bucket algorithm
    tokens: AtomicU32,
    max_tokens: u32,
    refill_rate: f64,  // tokens per second
    last_refill: AtomicU64,
}

impl RateLimiter {
    pub fn try_acquire(&self) -> bool {
        self.refill();
        let tokens = self.tokens.load(Ordering::SeqCst);
        if tokens > 0 {
            self.tokens.fetch_sub(1, Ordering::SeqCst);
            true
        } else {
            false
        }
    }
    
    pub async fn acquire(&self) {
        while !self.try_acquire() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

/// Per-client rate limiting
pub struct ClientRateLimiter {
    limiters: DashMap<String, RateLimiter>,
    config: RateLimitConfig,
}

#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub requests_per_second: f64,
    pub burst_size: u32,
    pub per_client: bool,
}
```

### Bulkhead Pattern (Isolation)

```rust
/// Bulkhead for isolating failures between different operations
pub struct Bulkhead {
    /// Semaphore for limiting concurrent executions
    semaphore: Arc<Semaphore>,
    
    /// Queue for waiting requests
    queue_size: AtomicU32,
    max_queue_size: u32,
    
    /// Metrics
    active_count: AtomicU32,
    rejected_count: AtomicU64,
}

impl Bulkhead {
    pub async fn execute<F, T>(&self, f: F) -> Result<T, BulkheadError>
    where
        F: Future<Output = T>,
    {
        // Check queue capacity
        if self.queue_size.load(Ordering::SeqCst) >= self.max_queue_size {
            self.rejected_count.fetch_add(1, Ordering::SeqCst);
            return Err(BulkheadError::QueueFull);
        }
        
        self.queue_size.fetch_add(1, Ordering::SeqCst);
        
        // Acquire permit
        let permit = self.semaphore.acquire().await
            .map_err(|_| BulkheadError::Closed)?;
        
        self.queue_size.fetch_sub(1, Ordering::SeqCst);
        self.active_count.fetch_add(1, Ordering::SeqCst);
        
        let result = f.await;
        
        self.active_count.fetch_sub(1, Ordering::SeqCst);
        drop(permit);
        
        Ok(result)
    }
}

/// Bulkhead configuration per operation type
pub struct BulkheadConfig {
    /// Market data operations
    pub market_data: BulkheadSettings,
    
    /// Curve building operations
    pub curve_build: BulkheadSettings,
    
    /// Pricing operations
    pub pricing: BulkheadSettings,
    
    /// Storage operations
    pub storage: BulkheadSettings,
}

#[derive(Debug, Clone)]
pub struct BulkheadSettings {
    pub max_concurrent: u32,
    pub max_queue: u32,
}
```

### Failover & Redundancy

```rust
/// Primary/backup failover for critical services
pub struct FailoverService<S> {
    primary: Arc<S>,
    backup: Arc<S>,
    state: AtomicU8,  // 0=Primary, 1=Backup
    health_check_interval: Duration,
}

impl<S: HealthCheck> FailoverService<S> {
    pub async fn get_active(&self) -> &Arc<S> {
        match self.state.load(Ordering::SeqCst) {
            0 => &self.primary,
            _ => &self.backup,
        }
    }
    
    /// Background task to monitor primary and failover/failback
    pub async fn monitor(&self) {
        loop {
            tokio::time::sleep(self.health_check_interval).await;
            
            let primary_healthy = self.primary.is_ready().await.is_ok();
            let current_state = self.state.load(Ordering::SeqCst);
            
            match (current_state, primary_healthy) {
                (0, false) => {
                    // Primary failed, switch to backup
                    tracing::warn!("Primary service failed, switching to backup");
                    self.state.store(1, Ordering::SeqCst);
                }
                (1, true) => {
                    // Primary recovered, switch back
                    tracing::info!("Primary service recovered, switching back");
                    self.state.store(0, Ordering::SeqCst);
                }
                _ => {}
            }
        }
    }
}

/// Multi-region deployment support
#[derive(Debug, Clone)]
pub struct RegionConfig {
    pub region_id: String,
    pub is_primary: bool,
    pub endpoints: Vec<String>,
    pub failover_priority: u8,
}
```

### Connection Pooling

```rust
/// Connection pool for database and external service connections
pub struct ConnectionPool<C> {
    connections: Arc<ArrayQueue<C>>,
    factory: Arc<dyn ConnectionFactory<C>>,
    config: PoolConfig,
    stats: PoolStats,
}

#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub min_connections: u32,
    pub max_connections: u32,
    pub connection_timeout: Duration,
    pub idle_timeout: Duration,
    pub max_lifetime: Duration,
    pub health_check_interval: Duration,
}

impl<C: Send + 'static> ConnectionPool<C> {
    pub async fn get(&self) -> Result<PooledConnection<C>, PoolError> {
        // Try to get from pool
        if let Some(conn) = self.connections.pop() {
            return Ok(PooledConnection {
                conn: Some(conn),
                pool: self.connections.clone(),
            });
        }
        
        // Create new connection if under limit
        if self.stats.total.load(Ordering::SeqCst) < self.config.max_connections {
            let conn = timeout(
                self.config.connection_timeout,
                self.factory.create(),
            ).await??;
            
            self.stats.total.fetch_add(1, Ordering::SeqCst);
            return Ok(PooledConnection {
                conn: Some(conn),
                pool: self.connections.clone(),
            });
        }
        
        Err(PoolError::Exhausted)
    }
}
```

### Caching Strategy

```rust
/// Multi-tier caching for frequently accessed data
pub struct TieredCache<K, V> {
    /// L1: In-process cache (fastest)
    l1: Arc<DashMap<K, CacheEntry<V>>>,
    
    /// L2: Distributed cache (Redis, etc.) - optional
    l2: Option<Arc<dyn DistributedCache<K, V>>>,
    
    config: CacheConfig,
}

#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub l1_max_entries: usize,
    pub l1_ttl: Duration,
    pub l2_ttl: Duration,
    pub write_through: bool,
    pub refresh_ahead: bool,
    pub refresh_ahead_threshold: f64,  // e.g., 0.8 = refresh when 80% of TTL elapsed
}

impl<K: Hash + Eq + Clone, V: Clone> TieredCache<K, V> {
    pub async fn get(&self, key: &K) -> Option<V> {
        // Check L1
        if let Some(entry) = self.l1.get(key) {
            if !entry.is_expired() {
                // Refresh ahead if enabled
                if self.config.refresh_ahead && entry.should_refresh(self.config.refresh_ahead_threshold) {
                    self.refresh_in_background(key.clone());
                }
                return Some(entry.value.clone());
            }
        }
        
        // Check L2
        if let Some(l2) = &self.l2 {
            if let Some(value) = l2.get(key).await {
                // Populate L1
                self.l1.insert(key.clone(), CacheEntry::new(value.clone(), self.config.l1_ttl));
                return Some(value);
            }
        }
        
        None
    }
}

/// Cache specifically for curves (hot path)
pub struct CurveCache {
    curves: Arc<DashMap<CurveId, Arc<BuiltCurve>>>,
    versions: Arc<DashMap<CurveId, u64>>,
    max_age: Duration,
}

impl CurveCache {
    /// Atomic swap of curve (for real-time updates)
    pub fn swap(&self, curve_id: &CurveId, curve: BuiltCurve) -> Option<Arc<BuiltCurve>> {
        let new_curve = Arc::new(curve);
        let old = self.curves.insert(curve_id.clone(), new_curve);
        self.versions.entry(curve_id.clone())
            .and_modify(|v| *v += 1)
            .or_insert(1);
        old
    }
    
    /// Get curve with version check
    pub fn get_if_current(&self, curve_id: &CurveId, expected_version: u64) -> Option<Arc<BuiltCurve>> {
        let current_version = self.versions.get(curve_id)?.clone();
        if current_version == expected_version {
            self.curves.get(curve_id).map(|c| c.clone())
        } else {
            None
        }
    }
}
```

### Observability Requirements

```rust
/// All services must emit structured metrics
pub trait Instrumented {
    fn metrics(&self) -> &ServiceMetrics;
}

#[derive(Debug)]
pub struct ServiceMetrics {
    /// Request counters
    pub requests_total: Counter,
    pub requests_failed: Counter,
    
    /// Latency histograms
    pub request_duration: Histogram,
    
    /// Gauges
    pub active_requests: Gauge,
    pub queue_depth: Gauge,
    
    /// Business metrics
    pub bonds_priced: Counter,
    pub curves_built: Counter,
    pub cache_hits: Counter,
    pub cache_misses: Counter,
}

/// Distributed tracing context
#[derive(Debug, Clone)]
pub struct TraceContext {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub baggage: HashMap<String, String>,
}

/// All async operations should propagate trace context
pub async fn with_tracing<F, T>(
    ctx: &TraceContext,
    operation: &str,
    f: F,
) -> T
where
    F: Future<Output = T>,
{
    let span = tracing::info_span!(
        "operation",
        trace_id = %ctx.trace_id,
        span_id = %ctx.span_id,
        operation = %operation,
    );
    f.instrument(span).await
}
```

### Configuration for Enterprise Deployment

```rust
/// Enterprise deployment configuration
#[derive(Debug, Clone, Deserialize)]
pub struct EnterpriseConfig {
    /// Service identity
    pub service_name: String,
    pub instance_id: String,
    pub region: String,
    pub environment: Environment,
    
    /// High availability
    pub ha: HaConfig,
    
    /// Resilience
    pub resilience: ResilienceConfig,
    
    /// Observability
    pub observability: ObservabilityConfig,
    
    /// Security
    pub security: SecurityConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HaConfig {
    pub enable_failover: bool,
    pub health_check_interval_ms: u64,
    pub failover_threshold: u32,
    pub failback_delay_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResilienceConfig {
    pub circuit_breaker: CircuitBreakerConfig,
    pub retry: RetryConfig,
    pub rate_limit: RateLimitConfig,
    pub bulkhead: BulkheadConfig,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ObservabilityConfig {
    pub metrics_endpoint: String,
    pub tracing_endpoint: String,
    pub log_level: String,
    pub sample_rate: f64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub enum Environment {
    Development,
    Staging,
    Production,
}
```

### Deployment Patterns

```yaml
# Example Kubernetes deployment configuration
# convex-deployment.yaml

apiVersion: apps/v1
kind: Deployment
metadata:
  name: convex-pricing
spec:
  replicas: 3  # Minimum 3 for HA
  strategy:
    type: RollingUpdate
    rollingUpdate:
      maxUnavailable: 1
      maxSurge: 1
  template:
    spec:
      containers:
      - name: convex
        resources:
          requests:
            memory: "2Gi"
            cpu: "1000m"
          limits:
            memory: "4Gi"
            cpu: "2000m"
        livenessProbe:
          httpGet:
            path: /health/live
            port: 8080
          initialDelaySeconds: 10
          periodSeconds: 5
        readinessProbe:
          httpGet:
            path: /health/ready
            port: 8080
          initialDelaySeconds: 5
          periodSeconds: 3
        env:
        - name: CONVEX_INSTANCE_ID
          valueFrom:
            fieldRef:
              fieldPath: metadata.name
      affinity:
        podAntiAffinity:
          requiredDuringSchedulingIgnoredDuringExecution:
          - labelSelector:
              matchLabels:
                app: convex-pricing
            topologyKey: kubernetes.io/hostname
---
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: convex-pricing-pdb
spec:
  minAvailable: 2
  selector:
    matchLabels:
      app: convex-pricing
```

---

### Service Layer (Stateful Components)

**All services MUST implement:**
- `HealthCheck` trait for liveness/readiness
- `GracefulShutdown` trait for clean shutdown
- `Instrumented` trait for metrics
- Proper error handling with circuit breakers for external calls
- Retry logic for transient failures

```rust
// =============================================================================
// CORE SERVICES - These manage state and orchestrate the system
// =============================================================================

/// Bond reference data management
#[async_trait]
pub trait BondService: HealthCheck + GracefulShutdown + Instrumented + Send + Sync {
    // CRUD
    async fn get(&self, id: &InstrumentId) -> Result<Bond, BondError>;
    async fn list(&self, filter: BondFilter) -> Result<Vec<Bond>, BondError>;
    async fn save(&self, bond: &Bond) -> Result<(), BondError>;
    async fn delete(&self, id: &InstrumentId) -> Result<(), BondError>;
    
    // Bulk operations (with progress reporting)
    async fn import(&self, bonds: Vec<Bond>, progress: Option<ProgressCallback>) -> Result<ImportResult, BondError>;
    async fn export(&self, filter: BondFilter) -> Result<Vec<Bond>, BondError>;
    
    // Search
    async fn search(&self, query: &str) -> Result<Vec<Bond>, BondError>;
    async fn by_issuer(&self, issuer_id: &str) -> Result<Vec<Bond>, BondError>;
    async fn by_sector(&self, sector: Sector) -> Result<Vec<Bond>, BondError>;
    
    // Cache management
    fn invalidate_cache(&self, id: &InstrumentId);
    fn warm_cache(&self, ids: &[InstrumentId]) -> Result<(), BondError>;
}

/// Curve building and management
#[async_trait]
pub trait CurveService: HealthCheck + GracefulShutdown + Instrumented + Send + Sync {
    // Configuration
    async fn get_config(&self, curve_id: &CurveId) -> Result<CurveConfig, CurveError>;
    async fn save_config(&self, config: CurveConfig) -> Result<(), CurveError>;
    async fn list_configs(&self, filter: CurveFilter) -> Result<Vec<CurveConfig>, CurveError>;
    
    // Building (with circuit breaker for market data)
    async fn build(&self, curve_id: &CurveId, as_of: jiff::Timestamp) -> Result<BuiltCurve, CurveError>;
    async fn build_all(&self, as_of: jiff::Timestamp) -> Result<Vec<CurveId>, CurveError>;
    async fn rebuild_failed(&self) -> Result<Vec<CurveId>, CurveError>;  // Retry failed builds
    
    // Cache management (atomic swap for zero-downtime updates)
    fn get_cached(&self, curve_id: &CurveId) -> Option<Arc<BuiltCurve>>;
    fn get_cached_version(&self, curve_id: &CurveId) -> Option<(Arc<BuiltCurve>, u64)>;
    fn invalidate(&self, curve_id: &CurveId);
    fn invalidate_all(&self);
    fn cache_stats(&self) -> CacheStats;
    
    // History
    async fn get_snapshot(&self, curve_id: &CurveId, as_of: jiff::Timestamp) -> Result<CurveSnapshot, CurveError>;
    async fn list_snapshots(&self, curve_id: &CurveId, from: jiff::Timestamp, to: jiff::Timestamp) -> Result<Vec<CurveSnapshot>, CurveError>;
    
    // Streaming (backpressure-aware)
    fn subscribe(&self, curve_ids: &[CurveId]) -> broadcast::Receiver<CurveSnapshot>;
    fn subscriber_count(&self) -> usize;
}

/// Pricing configuration and execution
#[async_trait]
pub trait PricingService: HealthCheck + GracefulShutdown + Instrumented + Send + Sync {
    // Configuration
    async fn get_config(&self, id: &str) -> Result<BondPricingConfig, PricingError>;
    async fn save_config(&self, config: BondPricingConfig) -> Result<(), PricingError>;
    async fn get_effective_config(&self, bond: &Bond) -> Result<BondPricingConfig, PricingError>;
    
    // Pricing - single bond (uses bulkhead for isolation)
    fn price(&self, bond: &Bond, settlement: Date) -> Result<BondQuote, PricingError>;
    fn price_from_yield(&self, bond: &Bond, ytm: Decimal, settlement: Date) -> Result<BondQuote, PricingError>;
    fn price_from_spread(&self, bond: &Bond, spread: Decimal, spread_type: SpreadType, settlement: Date) -> Result<BondQuote, PricingError>;
    
    // Pricing - batch (parallel with concurrency limit)
    fn price_batch(&self, bonds: &[Bond], settlement: Date) -> Vec<Result<BondQuote, PricingError>>;
    async fn price_batch_async(&self, bonds: &[Bond], settlement: Date, concurrency: usize) -> Vec<Result<BondQuote, PricingError>>;
    
    // Real-time quote enrichment (hot path - no blocking)
    fn enrich_quote(&self, instrument_id: &InstrumentId, bid: Option<Decimal>, ask: Option<Decimal>) -> Result<BondQuote, PricingError>;
    
    // Streaming (with backpressure)
    fn subscribe(&self, instrument_ids: &[InstrumentId]) -> broadcast::Receiver<BondQuote>;
    fn subscribe_all(&self) -> broadcast::Receiver<BondQuote>;
    
    // Circuit breaker status
    fn circuit_breaker_status(&self) -> CircuitBreakerStatus;
}

/// Manual override management with approval workflow
#[async_trait]
pub trait OverrideService: HealthCheck + GracefulShutdown + Instrumented + Send + Sync {
    // CRUD
    async fn get(&self, instrument_id: &InstrumentId) -> Result<Option<PriceOverride>, OverrideError>;
    async fn list(&self, filter: OverrideFilter) -> Result<Vec<PriceOverride>, OverrideError>;
    async fn save(&self, override_: PriceOverride) -> Result<(), OverrideError>;
    async fn delete(&self, instrument_id: &InstrumentId) -> Result<(), OverrideError>;
    
    // Approval workflow (with notifications)
    async fn submit(&self, override_: PriceOverride) -> Result<String, OverrideError>;
    async fn approve(&self, override_id: &str, approver: &str) -> Result<(), OverrideError>;
    async fn reject(&self, override_id: &str, rejector: &str, reason: &str) -> Result<(), OverrideError>;
    async fn pending(&self) -> Result<Vec<PriceOverride>, OverrideError>;
    
    // Expiry
    async fn clear_expired(&self) -> Result<u32, OverrideError>;
    
    // Audit (immutable log)
    async fn history(&self, instrument_id: &InstrumentId) -> Result<Vec<OverrideAudit>, OverrideError>;
    async fn audit_log(&self, filter: AuditFilter) -> Result<Vec<AuditEntry>, OverrideError>;
}

/// Spread adjustment management
#[async_trait]
pub trait SpreadAdjustmentService: HealthCheck + GracefulShutdown + Instrumented + Send + Sync {
    async fn get(&self, id: &str) -> Result<SpreadAdjustment, AdjustmentError>;
    async fn list(&self, filter: AdjustmentFilter) -> Result<Vec<SpreadAdjustment>, AdjustmentError>;
    async fn save(&self, adjustment: SpreadAdjustment) -> Result<(), AdjustmentError>;
    async fn delete(&self, id: &str) -> Result<(), AdjustmentError>;
    
    // Calculate total adjustment for a bond (cached)
    fn calculate_adjustment(&self, bond: &Bond) -> Decimal;
    fn calculate_adjustment_breakdown(&self, bond: &Bond) -> Vec<AdjustmentComponent>;
    
    // Bulk recalculation
    async fn recalculate_all(&self) -> Result<u32, AdjustmentError>;
}

/// Portfolio and NAV management
#[async_trait]
pub trait PortfolioService: Send + Sync {
    async fn get(&self, id: &str) -> Result<Portfolio, PortfolioError>;
    async fn list(&self) -> Result<Vec<PortfolioSummary>, PortfolioError>;
    async fn save(&self, portfolio: Portfolio) -> Result<(), PortfolioError>;
    async fn delete(&self, id: &str) -> Result<(), PortfolioError>;
    
    // Analytics
    fn calculate_nav(&self, portfolio_id: &str) -> Result<NavResult, PortfolioError>;
    fn calculate_inav(&self, portfolio_id: &str) -> Result<InavResult, PortfolioError>;
    fn calculate_analytics(&self, portfolio_id: &str) -> Result<PortfolioAnalytics, PortfolioError>;
    
    // Streaming
    fn subscribe(&self, portfolio_id: &str) -> broadcast::Receiver<PortfolioUpdate>;
}

/// Diagnostic and debugging
pub trait DiagnosticService: Send + Sync {
    fn capture(&self, instrument_id: &InstrumentId) -> Result<PricingDiagnostic, DiagnosticError>;
    fn replay(&self, diagnostic: &PricingDiagnostic) -> Result<BondQuote, DiagnosticError>;
    fn compare(&self, before: &PricingDiagnostic, after: &PricingDiagnostic) -> DiagnosticDiff;
    fn export(&self, diagnostic: &PricingDiagnostic, path: &Path) -> Result<(), DiagnosticError>;
    fn import(&self, path: &Path) -> Result<PricingDiagnostic, DiagnosticError>;
}
```

### Calculation Graph (Dependency-Driven Recalculation)

```rust
/// The calculation graph manages dependencies between pricing inputs and outputs.
/// When market data changes, it propagates dirty flags and triggers recalculation.
pub struct CalculationGraph {
    /// Nodes in the graph
    nodes: DashMap<NodeId, Arc<dyn CalculationNode>>,
    
    /// Directed edges (dependency → dependent)
    edges: DashMap<NodeId, Vec<NodeId>>,
    
    /// Current values (memoized)
    values: DashMap<NodeId, CachedValue>,
    
    /// Dirty flags
    dirty: DashSet<NodeId>,
    
    /// Revision counter
    revision: AtomicU64,
}

impl CalculationGraph {
    /// Mark a node as dirty (e.g., when market data changes)
    /// Propagates dirty flag to all dependents
    pub fn invalidate(&self, node_id: &NodeId) {
        self.dirty.insert(node_id.clone());
        self.propagate_dirty(node_id);
    }
    
    /// Recalculate all dirty nodes in topological order
    pub fn recalculate(&self) -> Vec<NodeId> {
        let dirty_nodes: Vec<_> = self.dirty.iter().map(|n| n.clone()).collect();
        let sorted = self.topological_sort(&dirty_nodes);
        
        for node_id in &sorted {
            if let Some(node) = self.nodes.get(node_id) {
                let value = node.calculate(self);
                self.values.insert(node_id.clone(), CachedValue {
                    value,
                    revision: self.revision.load(Ordering::SeqCst),
                });
                self.dirty.remove(node_id);
            }
        }
        
        sorted
    }
    
    /// Get cached value without recalculation
    pub fn get_cached(&self, node_id: &NodeId) -> Option<NodeValue> {
        self.values.get(node_id).map(|v| v.value.clone())
    }
}

/// Node types in the calculation graph
pub enum NodeType {
    /// Raw market quote input
    Quote { instrument_id: InstrumentId },
    
    /// Curve input (deposit rate, swap rate, bond yield)
    CurveInput { curve_id: CurveId, instrument: String },
    
    /// Built curve (depends on curve inputs)
    Curve { curve_id: CurveId },
    
    /// Bond price (depends on curve, bond data, config)
    BondPrice { instrument_id: InstrumentId },
    
    /// Portfolio aggregate (depends on constituent bond prices)
    Portfolio { portfolio_id: String },
    
    /// Custom calculation node
    Custom { id: String },
}
```

### Storage Layer (Persistence)

```rust
/// All state is persisted via the storage layer
#[async_trait]
pub trait StorageAdapter: Send + Sync {
    // Bonds
    async fn get_bond(&self, id: &InstrumentId) -> Result<Option<Bond>, StorageError>;
    async fn save_bond(&self, bond: &Bond) -> Result<(), StorageError>;
    async fn delete_bond(&self, id: &InstrumentId) -> Result<(), StorageError>;
    async fn list_bonds(&self, filter: &BondFilter) -> Result<Vec<Bond>, StorageError>;
    
    // Curves
    async fn get_curve_config(&self, id: &CurveId) -> Result<Option<CurveConfig>, StorageError>;
    async fn save_curve_config(&self, config: &CurveConfig) -> Result<(), StorageError>;
    async fn get_curve_snapshot(&self, id: &CurveId, as_of: jiff::Timestamp) -> Result<Option<CurveSnapshot>, StorageError>;
    async fn save_curve_snapshot(&self, snapshot: &CurveSnapshot) -> Result<(), StorageError>;
    
    // Pricing config
    async fn get_pricing_config(&self, id: &str) -> Result<Option<BondPricingConfig>, StorageError>;
    async fn save_pricing_config(&self, config: &BondPricingConfig) -> Result<(), StorageError>;
    
    // Overrides
    async fn get_override(&self, id: &InstrumentId) -> Result<Option<PriceOverride>, StorageError>;
    async fn save_override(&self, override_: &PriceOverride) -> Result<(), StorageError>;
    async fn list_overrides(&self, filter: &OverrideFilter) -> Result<Vec<PriceOverride>, StorageError>;
    
    // Spread adjustments
    async fn get_adjustment(&self, id: &str) -> Result<Option<SpreadAdjustment>, StorageError>;
    async fn save_adjustment(&self, adjustment: &SpreadAdjustment) -> Result<(), StorageError>;
    
    // Audit
    async fn append_audit(&self, entry: &AuditEntry) -> Result<(), StorageError>;
    async fn list_audit(&self, filter: &AuditFilter) -> Result<Vec<AuditEntry>, StorageError>;
    
    // Portfolios
    async fn get_portfolio(&self, id: &str) -> Result<Option<Portfolio>, StorageError>;
    async fn save_portfolio(&self, portfolio: &Portfolio) -> Result<(), StorageError>;
}
```

---

**Market Data Providers:**
- Bloomberg B-PIPE
- Refinitiv
- ICE
- MarketAxess
- Tradeweb

**External Storage:**
- PostgreSQL
- MongoDB
- TimescaleDB
- Redis

**UI:**
- Web UI (framework provides all APIs)
- Desktop UI

**Note:** The framework provides complete **traits and interfaces**. Users implement adapters for their specific vendors and infrastructure.
- PostgreSQL `StorageAdapter`/`ConfigStore`
- MongoDB `StorageAdapter`/`ConfigStore`
- TimescaleDB `StorageAdapter`/`ConfigStore`
- Redis `StorageAdapter`/`ConfigStore`

**UI Implementation:**
- Web UI is out of scope for this framework
- Framework provides all API endpoints the UI needs
- UI can be built separately consuming the REST/WebSocket API

**Note:** The framework provides all the **traits, abstractions, CLI, and API** for production use. Users implement the specific adapters for their market data vendors, distribution channels, and external databases. This keeps the core framework vendor-agnostic while fully supporting production real-time use cases.
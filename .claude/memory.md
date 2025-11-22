# Convex Project Memory

## Project Status

**Current Phase**: Foundation & Initial Development  
**Started**: [Date will be filled when project starts]  
**Last Updated**: [Auto-updated by Claude]

## Key Decisions Log

### Architecture Decisions

#### AD-001: Workspace Structure (Pending)
- **Decision**: Use Cargo workspace with multiple crates
- **Rationale**: Enables modular development, independent compilation, clear separation of concerns
- **Alternatives Considered**: Single crate with modules (rejected for scalability)
- **Status**: Approved
- **Date**: TBD

#### AD-002: Numerical Type Choice (Pending)
- **Decision**: Use `rust_decimal::Decimal` for financial calculations, f64 for performance-critical math
- **Rationale**: Avoid floating-point precision issues in price/yield calculations
- **Alternatives Considered**: 
  - f64 only (rejected - precision issues)
  - Fixed-point arithmetic (rejected - complexity)
- **Trade-offs**: Slight performance overhead vs correctness
- **Status**: Approved
- **Date**: TBD

#### AD-003: Error Handling Strategy (Pending)
- **Decision**: Use `thiserror` for domain errors, never panic in library code
- **Rationale**: Ergonomic error handling, clear error types for consumers
- **Error Categories**:
  - `PricingError`: Invalid inputs, calculation failures
  - `CurveError`: Curve construction issues
  - `DateError`: Invalid date operations
- **Status**: Approved
- **Date**: TBD

#### AD-004: Interpolation Default (Pending)
- **Decision**: Cubic spline as default, linear as fallback
- **Rationale**: Balance between smoothness and stability
- **Performance Target**: < 1 microsecond per interpolation
- **Status**: Pending discussion
- **Date**: TBD

#### AD-005: Parallel Processing Strategy (Pending)
- **Decision**: Use Rayon for data parallelism in curve building
- **Rationale**: Ergonomic API, work-stealing scheduler
- **Use Cases**: 
  - Bootstrap multiple currencies
  - Batch bond pricing
  - Scenario analysis
- **Status**: Approved
- **Date**: TBD

### Technical Decisions

#### TD-001: Dependency Selection (Pending)
**Core Dependencies Approved**:
- chrono 0.4+ (date/time handling)
- rust_decimal 1.34+ (precise decimal math)
- serde 1.0+ (serialization)
- thiserror 1.0+ (error handling)

**Performance Dependencies**:
- rayon 1.10+ (parallel processing)
- ndarray 0.15+ (array operations)

**Math Dependencies**:
- nalgebra 0.32+ (linear algebra)
- approx 0.5+ (floating point comparisons)

**Status**: Approved
**Review Date**: TBD

#### TD-002: Testing Framework (Pending)
- **Unit Tests**: Standard Rust test framework
- **Property Tests**: proptest for invariant checking
- **Benchmarks**: Criterion.rs
- **Integration**: Separate tests/ directory
- **Status**: Approved
- **Date**: TBD

#### TD-003: Documentation Standard (Pending)
- **API Docs**: Rustdoc with LaTeX math notation
- **Examples**: Inline code examples for all public APIs
- **Architecture**: Markdown in .claude directory
- **Guides**: Separate docs/ directory for user guides
- **Status**: Approved
- **Date**: TBD

### Domain Decisions

#### DD-001: Day Count Convention Priority (Pending)
**Implementation Order**:
1. ACT/360 (Money markets)
2. ACT/365 (UK Gilts)
3. 30/360 US (Corporate bonds)
4. ACT/ACT ICMA (Government bonds)
5. ACT/ACT ISDA (Swaps)

**Rationale**: Frequency of use in major markets
**Status**: Approved
**Date**: TBD

#### DD-002: Yield Calculation Method (Pending)
- **Decision**: Bloomberg YAS methodology as reference
- **Method**: Sequential roll-forward with Newton-Raphson
- **Tolerance**: 1e-10 for yield convergence
- **Max Iterations**: 100
- **Status**: Approved - must match Bloomberg exactly
- **Date**: TBD

#### DD-003: Calendar Support (Pending)
**Phase 1 Calendars**:
- US Federal Reserve (SIFMA)
- UK (Bank of England)
- EUR (TARGET2)

**Phase 2 Calendars**:
- Japan (Tokyo Stock Exchange)
- Additional G7 markets

**Status**: Phase 1 approved
**Date**: TBD

## Implementation Progress

### Milestone 1: Core Infrastructure (0%)
- [ ] Create workspace structure
- [ ] Implement core types (Date, Price, Yield, Spread)
- [ ] Implement day count conventions
- [ ] Implement business day calendars
- [ ] Write comprehensive unit tests
- [ ] Create initial documentation

**Target**: Week 1-2  
**Status**: Not Started

### Milestone 2: Yield Curve Foundation (0%)
- [ ] Design curve traits and interfaces
- [ ] Implement linear interpolation
- [ ] Implement cubic spline interpolation
- [ ] Bootstrap from deposits
- [ ] Bootstrap from government bonds
- [ ] Validation against known test cases

**Target**: Week 3-4  
**Status**: Not Started

### Milestone 3: Basic Bond Pricing (0%)
- [ ] Fixed-rate bond implementation
- [ ] Zero-coupon bond implementation
- [ ] Cash flow generation engine
- [ ] YTM calculator (Newton-Raphson)
- [ ] Clean/dirty price calculations
- [ ] Accrued interest calculations

**Target**: Week 5-6  
**Status**: Not Started

### Milestone 4: Spread Analytics (0%)
- [ ] G-spread implementation
- [ ] I-spread implementation
- [ ] Z-spread solver
- [ ] Basic asset swap spread
- [ ] Validation against market data

**Target**: Week 7-8  
**Status**: Not Started

### Milestone 5: Risk Calculations (0%)
- [ ] Duration calculations (Macaulay, Modified, Effective)
- [ ] Convexity calculations
- [ ] DV01 calculations
- [ ] Key rate durations
- [ ] Comprehensive risk tests

**Target**: Week 9-10  
**Status**: Not Started

## Known Issues & Challenges

### Technical Challenges

#### TC-001: Float Precision in Yield Calculations
- **Issue**: Need exact replication of Bloomberg YAS
- **Challenge**: Floating point arithmetic differences
- **Solution**: Use Decimal type for all financial calculations
- **Status**: Planned mitigation
- **Priority**: Critical

#### TC-002: Performance vs Precision Trade-off
- **Issue**: Decimal math slower than f64
- **Challenge**: Meet < 1 microsecond pricing target
- **Solution**: Hybrid approach - Decimal for money, f64 for intermediate math
- **Status**: Under consideration
- **Priority**: High

#### TC-003: Calendar Complexity
- **Issue**: Complex holiday rules and exceptions
- **Challenge**: Maintain accuracy across markets
- **Solution**: Use authoritative sources, comprehensive testing
- **Status**: Requires research
- **Priority**: Medium

### Domain Challenges

#### DC-001: Bloomberg YAS Methodology Documentation
- **Issue**: Some Bloomberg methods are undocumented
- **Challenge**: Reverse-engineer exact behavior
- **Solution**: Compare outputs, iterative refinement
- **Status**: Ongoing
- **Priority**: Critical

#### DC-002: Market Convention Variations
- **Issue**: Different markets have subtle convention differences
- **Challenge**: Support all major markets correctly
- **Solution**: Configuration-driven approach
- **Status**: Design in progress
- **Priority**: High

#### DC-003: Edge Cases in Yield Calculations
- **Issue**: Negative yields, very short/long maturities
- **Challenge**: Numerical stability
- **Solution**: Robust error handling, fallback methods
- **Status**: Requires testing
- **Priority**: Medium

## Performance Metrics Tracking

### Target Metrics
- Bond pricing: < 1 microsecond
- YTM calculation: < 10 microseconds
- Bootstrap 50-point curve: < 100 microseconds
- Z-spread: < 50 microseconds
- DV01 calculation: < 5 microseconds

### Current Metrics
*Will be populated as implementation progresses*

### Benchmark Results
*Will be tracked here with dates and versions*

## Testing Coverage

### Unit Test Coverage
- Target: 90%+
- Current: TBD
- Last Updated: TBD

### Integration Test Status
- Total Tests: TBD
- Passing: TBD
- Failing: TBD

### Validation Test Status
- Bloomberg Comparison Tests: TBD
- Reuters Comparison Tests: TBD
- Known Case Tests: TBD

## Dependencies Management

### Dependency Updates Log
*Track dependency version updates and reasons*

### Security Audit Log
*Track cargo-audit results and resolutions*

## Questions & Discussions

### Q-001: Should we support negative interest rates in all calculations?
- **Context**: European bonds have negative yields
- **Options**: 
  1. Full support (complex)
  2. Limited support (flag issues)
- **Decision**: TBD
- **Owner**: TBD

### Q-002: How to handle settlement date variations?
- **Context**: Different markets, different conventions
- **Options**:
  1. Configuration per bond
  2. Global defaults with overrides
- **Decision**: TBD
- **Owner**: TBD

### Q-003: Async vs Sync API?
- **Context**: Future integration with async systems
- **Options**:
  1. Sync only (simpler)
  2. Sync + Async wrappers
  3. Async-first
- **Decision**: TBD
- **Owner**: TBD

## Future Enhancements Queue

### Short-term (Next 3 months)
1. Advanced interpolation methods (Nelson-Siegel)
2. Callable bond support
3. Floating rate notes
4. Python bindings

### Medium-term (3-6 months)
1. Java bindings
2. C# bindings
3. Excel plugin
4. REST API service
5. Inflation-linked bonds

### Long-term (6-12 months)
1. Convertible bonds
2. ABS/MBS support
3. CDS integration
4. Multi-curve frameworks
5. GPU acceleration

## References & Resources

### Bloomberg Documentation
- YAS Function Reference: [Location TBD]
- Fixed Income Analytics: [Location TBD]

### Academic Papers
- *To be added as referenced*

### Industry Standards
- ISDA definitions
- ICMA conventions
- ARRC SOFR documentation

## Team Notes

### Communication Channels
- GitHub Issues: For bug reports and feature requests
- Discussions: For design decisions
- Documentation: In .claude/ directory

### Code Review Guidelines
- All PRs require review
- Must pass all tests
- Must maintain >90% coverage
- Must include documentation updates

### Release Cadence
- TBD (suggestion: monthly minor releases, quarterly major releases)

---

*This memory file should be updated regularly to track progress and decisions*

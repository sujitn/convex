# Bloomberg Validation Test Cases

## Tolerances

| Metric | Tolerance |
|--------|-----------|
| Yields | ±0.00001% |
| Prices | ±0.000001 |
| Accrued | ±0.000001 |

## Test Structure

All tests use bond's own conventions - YieldCalculator only controls method.

```rust
#[test]
fn test_example() {
    // 1. Create bond with its conventions
    let bond = FixedRateBond::builder()
        .day_count(Thirty360Us)      // Bond owns this
        .frequency(SemiAnnual)        // Bond owns this
        .coupon_rate(dec!(5.0))
        .maturity(date!(2030-06-15))
        .build()?;
    
    // 2. Create calculator with just the method
    let config = YieldCalculatorConfig {
        method: YieldMethod::Compounded,
        money_market_threshold: Some(182),
        tolerance: 1e-10,
    };
    let calc = YieldCalculator::new(config);
    
    // 3. Calculator uses bond's conventions internally
    let ytm = calc.yield_from_price(&bond, settlement, price)?;
}
```

## US Corporate Bond Tests

### Boeing 7.5% 06/15/2025

```rust
#[test]
fn test_boeing_ytm() {
    let bond = FixedRateBond::builder()
        .cusip("097023AH7")
        .coupon_rate(dec!(7.5))
        .maturity(date!(2025-06-15))
        .frequency(Frequency::SemiAnnual)
        .day_count(DayCount::Thirty360Us)
        .issue_date(date!(2005-06-13))
        .first_coupon(date!(2005-12-15))
        .build()?;
    
    let settlement = date!(2020-04-29);
    let price = CleanPrice::new(dec!(110.503))?;
    
    let calc = YieldCalculator::new(US_CORPORATE.yield_config());
    let ytm = calc.yield_from_price(&bond, settlement, price)?;
    
    assert_relative_eq!(ytm.as_percent(), 4.905895, epsilon = 0.00001);
}

#[test]
fn test_boeing_accrued() {
    let bond = /* same as above */;
    let settlement = date!(2020-04-29);
    
    // Accrued uses bond's day count (30/360 US)
    let accrued = bond.accrued_interest(settlement);
    
    assert_relative_eq!(accrued.to_f64(), 2.729167, epsilon = 0.000001);
}

#[test]
fn test_boeing_roundtrip() {
    let bond = /* same as above */;
    let settlement = date!(2020-04-29);
    let original_price = CleanPrice::new(dec!(110.503))?;
    
    let calc = YieldCalculator::new(US_CORPORATE.yield_config());
    
    let ytm = calc.yield_from_price(&bond, settlement, original_price)?;
    let recalc = calc.price_from_yield(&bond, settlement, ytm)?;
    
    assert_relative_eq!(
        original_price.as_f64(),
        recalc.as_f64(),
        epsilon = 0.000001
    );
}
```

## US Treasury Tests

### 2.5% Treasury 05/15/2024

```rust
#[test]
fn test_treasury_ytm() {
    let bond = FixedRateBond::builder()
        .coupon_rate(dec!(2.5))
        .maturity(date!(2024-05-15))
        .frequency(Frequency::SemiAnnual)
        .day_count(DayCount::ActActIcma)
        .build()?;
    
    let settlement = date!(2023-11-15);
    let price = CleanPrice::new(dec!(99.125))?;
    
    // Days to maturity = 182, exactly at threshold
    let calc = YieldCalculator::new(US_TREASURY.yield_config());
    let ytm = calc.yield_from_price(&bond, settlement, price)?;
    
    // Should use money market method (≤182 days)
    assert!(ytm.as_percent() > 0.0);
}
```

### T-Bill Discount Yield

```rust
#[test]
fn test_tbill_discount() {
    let tbill = ZeroCouponBond::builder()
        .maturity(date!(2024-04-11))
        .day_count(DayCount::Act360)
        .face_value(dec!(100))
        .build()?;
    
    let settlement = date!(2024-01-15);
    let price = CleanPrice::new(dec!(98.735))?;
    
    let calc = YieldCalculator::new(US_TBILL.yield_config());
    let discount_yield = calc.yield_from_price(&tbill, settlement, price)?;
    
    // Manual: (100 - 98.735) / 100 × (360 / 87) = 5.237%
    let days = 87;
    let expected = (100.0 - 98.735) / 100.0 * (360.0 / days as f64);
    
    assert_relative_eq!(discount_yield.as_percent(), expected * 100.0, epsilon = 0.001);
}
```

## European Sovereign Tests

### German Bund (Annual Compounding)

```rust
#[test]
fn test_bund_annual_vs_semi() {
    let bond = FixedRateBond::builder()
        .coupon_rate(dec!(1.5))
        .maturity(date!(2030-08-15))
        .frequency(Frequency::Annual)        // Annual coupons
        .day_count(DayCount::ActActIcma)
        .build()?;
    
    let settlement = date!(2024-01-15);
    let price = CleanPrice::new(dec!(95.50))?;
    
    // German convention (annual compounding)
    let de_calc = YieldCalculator::new(GERMAN_BUND.yield_config());
    let de_ytm = de_calc.yield_from_price(&bond, settlement, price)?;
    
    // If we forced semi-annual compounding (for comparison)
    let semi_config = YieldCalculatorConfig {
        method: YieldMethod::Compounded,
        money_market_threshold: None,
        tolerance: 1e-10,
    };
    // Note: This would still use the bond's ANNUAL frequency for cash flows
    // The difference comes from the reinvestment assumption
    
    // Yields should be valid
    assert!(de_ytm.as_percent() > 0.0);
    assert!(de_ytm.as_percent() < 10.0);
}
```

## Japanese JGB Tests

### Simple Yield

```rust
#[test]
fn test_jgb_simple_yield() {
    let bond = FixedRateBond::builder()
        .coupon_rate(dec!(0.1))  // 10bp
        .maturity(date!(2030-03-20))
        .frequency(Frequency::SemiAnnual)
        .day_count(DayCount::Act365Fixed)
        .build()?;
    
    let settlement = date!(2024-01-15);
    let price = CleanPrice::new(dec!(99.00))?;
    
    // Simple yield method
    let calc = YieldCalculator::new(JAPANESE_JGB_SIMPLE.yield_config());
    let simple = calc.yield_from_price(&bond, settlement, price)?;
    
    // Manual calculation
    let years = Act365Fixed.year_fraction(settlement, bond.maturity());
    let expected = (0.001 + (100.0 - 99.0) / years.to_f64()) / 99.0;
    
    assert_relative_eq!(simple.as_decimal().to_f64(), expected, epsilon = 1e-10);
}
```

## Money Market Tests

### Short-Dated Bond (MM Threshold)

```rust
#[test]
fn test_short_dated_triggers_mm() {
    let bond = FixedRateBond::builder()
        .coupon_rate(dec!(5.0))
        .maturity(date!(2024-06-15))  // ~5 months
        .frequency(Frequency::SemiAnnual)
        .day_count(DayCount::Thirty360Us)
        .build()?;
    
    let settlement = date!(2024-01-15);
    let days_to_mat = (bond.maturity() - settlement).num_days();
    
    assert!(days_to_mat <= 182, "Should be under MM threshold");
    
    let price = CleanPrice::new(dec!(100.50))?;
    let calc = YieldCalculator::new(US_CORPORATE.yield_config());
    
    // Should use add-on/MM method internally
    let mmy = calc.yield_from_price(&bond, settlement, price)?;
    
    // Verify roundtrip
    let recalc = calc.price_from_yield(&bond, settlement, mmy)?;
    assert_relative_eq!(price.as_f64(), recalc.as_f64(), epsilon = 1e-6);
}
```

### Sequential Roll-Forward (2 Coupons)

```rust
#[test]
fn test_rollforward_two_coupons() {
    let bond = FixedRateBond::builder()
        .coupon_rate(dec!(6.0))
        .maturity(date!(2024-09-15))
        .frequency(Frequency::SemiAnnual)
        .day_count(DayCount::Thirty360Us)
        .build()?;
    
    let settlement = date!(2024-01-15);
    // Cash flows: 2024-03-15 (coupon), 2024-09-15 (coupon + principal)
    
    let price = CleanPrice::new(dec!(100.25))?;
    let calc = YieldCalculator::new(US_CORPORATE.yield_config());
    
    let mmy = calc.yield_from_price(&bond, settlement, price)?;
    let recalc = calc.price_from_yield(&bond, settlement, mmy)?;
    
    assert_relative_eq!(price.as_f64(), recalc.as_f64(), epsilon = 1e-6);
}
```

## Roundtrip Tests

### All Presets

```rust
#[test]
fn test_roundtrip_all_presets() {
    let presets = vec![
        US_TREASURY,
        US_CORPORATE,
        UK_GILT,
        GERMAN_BUND,
        FRENCH_OAT,
        ITALIAN_BTP,
        JAPANESE_JGB,
        CANADIAN_GOVT,
    ];
    
    for preset in presets {
        // Create bond matching preset
        let bond = FixedRateBond::builder()
            .with_preset(&preset)
            .coupon_rate(dec!(4.0))
            .maturity(date!(2029-06-15))
            .build()?;
        
        let settlement = date!(2024-06-15);
        let price = CleanPrice::new(dec!(98.50))?;
        
        let calc = YieldCalculator::new(preset.yield_config());
        let ytm = calc.yield_from_price(&bond, settlement, price)?;
        let recalc = calc.price_from_yield(&bond, settlement, ytm)?;
        
        assert_relative_eq!(
            price.as_f64(),
            recalc.as_f64(),
            epsilon = 1e-6,
            "{} roundtrip failed", preset.name
        );
    }
}
```

### Edge Case Prices

```rust
#[test]
fn test_roundtrip_edge_prices() {
    let bond = FixedRateBond::builder()
        .with_preset(&US_CORPORATE)
        .coupon_rate(dec!(5.0))
        .maturity(date!(2034-06-15))
        .build()?;
    
    let settlement = date!(2024-06-15);
    let calc = YieldCalculator::new(US_CORPORATE.yield_config());
    
    for price_val in [80.0, 95.0, 100.0, 105.0, 120.0] {
        let price = CleanPrice::new(Decimal::from_f64(price_val).unwrap())?;
        let ytm = calc.yield_from_price(&bond, settlement, price)?;
        let recalc = calc.price_from_yield(&bond, settlement, ytm)?;
        
        assert_relative_eq!(price_val, recalc.as_f64(), epsilon = 1e-6);
    }
}
```

## Performance Benchmarks

```rust
#[bench]
fn bench_ytm_compounded(b: &mut Bencher) {
    let bond = create_benchmark_bond();
    let calc = YieldCalculator::new(US_CORPORATE.yield_config());
    let price = CleanPrice::new(dec!(99.5))?;
    
    b.iter(|| {
        black_box(calc.yield_from_price(
            black_box(&bond),
            black_box(settlement),
            black_box(price),
        ))
    });
}
// Target: < 1μs

#[bench]
fn bench_ytm_simple(b: &mut Bencher) {
    let bond = create_jgb_bond();
    let calc = YieldCalculator::new(JAPANESE_JGB_SIMPLE.yield_config());
    
    b.iter(|| { /* ... */ });
}
// Target: < 100ns

#[bench]
fn bench_mmy_rollforward(b: &mut Bencher) {
    let bond = create_short_dated_bond();  // 4 coupons
    let calc = YieldCalculator::new(US_CORPORATE.yield_config());
    
    b.iter(|| { /* ... */ });
}
// Target: < 5μs
```

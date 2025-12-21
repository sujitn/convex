#!/usr/bin/env python3
"""
quantlib_test_generator.py

Generate reference test cases from QuantLib 1.40 for Convex validation.
Uses the correct QuantLib 1.40 API.

Usage:
    python quantlib_test_generator.py

Output:
    tests/fixtures/quantlib_reference_tests.json
"""

import QuantLib as ql
import json
from datetime import date
from typing import List, Dict, Any
import os


# ============================================================================
# HELPER FUNCTIONS
# ============================================================================

def ql_date_to_str(d: ql.Date) -> str:
    """Convert QuantLib Date to ISO format string."""
    return f"{d.year()}-{d.month():02d}-{d.dayOfMonth():02d}"


def frequency_to_str(freq: int) -> str:
    """Convert QuantLib frequency to string."""
    freq_map = {
        ql.Annual: "Annual",
        ql.Semiannual: "SemiAnnual",
        ql.Quarterly: "Quarterly",
        ql.Monthly: "Monthly",
    }
    return freq_map.get(freq, "Unknown")


def get_quantlib_version() -> str:
    """Get QuantLib version string."""
    try:
        return ql.QuantLib_VERSION
    except AttributeError:
        try:
            return ql.__version__
        except AttributeError:
            return "1.40"


def get_prev_coupon_date(bond, settlement_date):
    """Get previous coupon date from bond cashflows."""
    cfs = bond.cashflows()
    prev_date = bond.issueDate()
    for cf in cfs:
        if cf.date() <= settlement_date:
            prev_date = cf.date()
        else:
            break
    return prev_date


def get_next_coupon_date(bond, settlement_date):
    """Get next coupon date from bond cashflows."""
    cfs = bond.cashflows()
    for cf in cfs:
        if cf.date() > settlement_date:
            return cf.date()
    return bond.maturityDate()


# ============================================================================
# TEST CASE 1: Day Count Conventions
# ============================================================================

def generate_day_count_tests() -> List[Dict[str, Any]]:
    """Generate day count convention test cases."""

    results = []

    # Test date pairs covering edge cases
    date_pairs = [
        (ql.Date(15, 1, 2024), ql.Date(15, 7, 2024)),   # 6 months
        (ql.Date(1, 1, 2024), ql.Date(1, 1, 2025)),     # Full year (leap)
        (ql.Date(1, 1, 2023), ql.Date(1, 1, 2024)),     # Full year (non-leap)
        (ql.Date(28, 2, 2024), ql.Date(1, 3, 2024)),    # Leap year Feb
        (ql.Date(28, 2, 2023), ql.Date(1, 3, 2023)),    # Non-leap year Feb
        (ql.Date(31, 1, 2024), ql.Date(28, 2, 2024)),   # End of month to EOM
        (ql.Date(15, 3, 2024), ql.Date(15, 9, 2024)),   # 6 months exact
        (ql.Date(1, 1, 2024), ql.Date(31, 12, 2024)),   # Almost full year
        (ql.Date(31, 3, 2024), ql.Date(30, 6, 2024)),   # Quarter end to quarter end
        (ql.Date(29, 2, 2024), ql.Date(28, 2, 2025)),   # Leap day to non-leap
        (ql.Date(15, 12, 2019), ql.Date(29, 4, 2020)),  # Boeing accrued (134 days)
        (ql.Date(30, 4, 2024), ql.Date(31, 7, 2024)),   # 30th to 31st
        (ql.Date(31, 1, 2024), ql.Date(30, 4, 2024)),   # 31st to 30th
        (ql.Date(1, 2, 2024), ql.Date(1, 3, 2024)),     # Feb in leap year
    ]

    day_counts = [
        ("Act360", ql.Actual360()),
        ("Act365Fixed", ql.Actual365Fixed()),
        ("Thirty360US", ql.Thirty360(ql.Thirty360.USA)),
        ("Thirty360E", ql.Thirty360(ql.Thirty360.European)),
        ("ActActIsda", ql.ActualActual(ql.ActualActual.ISDA)),
    ]

    for start, end in date_pairs:
        for dc_name, dc in day_counts:
            day_count_value = dc.dayCount(start, end)
            year_fraction = dc.yearFraction(start, end)

            results.append({
                "test_type": "day_count",
                "convention": dc_name,
                "inputs": {
                    "start_date": ql_date_to_str(start),
                    "end_date": ql_date_to_str(end),
                },
                "expected": {
                    "day_count": day_count_value,
                    "year_fraction": round(year_fraction, 12),
                },
                "tolerance": 1e-10,
            })

    return results


# ============================================================================
# TEST CASE 2: Fixed Rate Bond with Duration/Convexity
# ============================================================================

def generate_fixed_rate_bond_tests() -> List[Dict[str, Any]]:
    """Generate test cases for fixed rate bonds using InterestRate API."""

    results = []

    # Set evaluation date
    today = ql.Date(15, 1, 2024)
    ql.Settings.instance().evaluationDate = today

    # Test bonds with different characteristics
    test_configs = [
        (0.05, 5, ql.Semiannual, "Thirty360US", ql.Thirty360(ql.Thirty360.USA)),
        (0.05, 10, ql.Semiannual, "Thirty360US", ql.Thirty360(ql.Thirty360.USA)),
        (0.03, 2, ql.Annual, "ActActIcma", ql.ActualActual(ql.ActualActual.ISMA)),
        (0.07, 30, ql.Semiannual, "Thirty360US", ql.Thirty360(ql.Thirty360.USA)),
        (0.04, 3, ql.Annual, "Act360", ql.Actual360()),
        (0.045, 7, ql.Semiannual, "Act365Fixed", ql.Actual365Fixed()),
    ]

    test_yields = [0.02, 0.03, 0.04, 0.05, 0.06, 0.07, 0.08]

    for coupon_rate, maturity_years, frequency, dc_name, day_count in test_configs:
        issue_date = today
        maturity_date = ql.Date(15, 1, 2024 + maturity_years)

        calendar = ql.UnitedStates(ql.UnitedStates.GovernmentBond)
        schedule = ql.Schedule(
            issue_date, maturity_date,
            ql.Period(frequency),
            calendar,
            ql.Unadjusted, ql.Unadjusted,
            ql.DateGeneration.Backward, False
        )

        settlement_days = 2
        face_value = 100.0
        bond = ql.FixedRateBond(
            settlement_days, face_value, schedule,
            [coupon_rate], day_count
        )

        settlement_date = calendar.advance(today, settlement_days, ql.Days)

        for ytm in test_yields:
            try:
                # Create InterestRate object
                rate = ql.InterestRate(ytm, day_count, ql.Compounded, frequency)

                # Calculate clean price from yield
                clean_price = ql.BondFunctions.cleanPrice(bond, rate, settlement_date)

                # Skip if price is unreasonable
                if clean_price <= 0 or clean_price > 500:
                    continue

                # Get accrued
                accrued = bond.accruedAmount(settlement_date)
                dirty_price = clean_price + accrued

                # Calculate duration and convexity using InterestRate
                mac_duration = ql.BondFunctions.duration(bond, rate, ql.Duration.Macaulay, settlement_date)
                mod_duration = ql.BondFunctions.duration(bond, rate, ql.Duration.Modified, settlement_date)
                convexity = ql.BondFunctions.convexity(bond, rate, settlement_date)
                bpv = ql.BondFunctions.basisPointValue(bond, rate, settlement_date)

                results.append({
                    "test_type": "fixed_rate_bond",
                    "description": f"{coupon_rate*100:.1f}% {maturity_years}Y bond at {ytm*100:.1f}% yield",
                    "inputs": {
                        "evaluation_date": ql_date_to_str(today),
                        "settlement_date": ql_date_to_str(settlement_date),
                        "issue_date": ql_date_to_str(issue_date),
                        "maturity_date": ql_date_to_str(maturity_date),
                        "coupon_rate": coupon_rate,
                        "frequency": frequency_to_str(frequency),
                        "day_count": dc_name,
                        "face_value": face_value,
                        "settlement_days": settlement_days,
                        "yield_to_maturity": ytm,
                    },
                    "expected": {
                        "clean_price": round(clean_price, 10),
                        "dirty_price": round(dirty_price, 10),
                        "accrued_interest": round(accrued, 10),
                        "macaulay_duration": round(mac_duration, 10),
                        "modified_duration": round(mod_duration, 10),
                        "convexity": round(convexity, 10),
                        "basis_point_value": round(bpv, 10),
                    },
                    "tolerances": {
                        "price": 0.0001,
                        "yield": 0.000001,
                        "duration": 0.001,
                        "convexity": 0.01,
                    }
                })
            except Exception as e:
                print(f"Skipped: {coupon_rate}, {maturity_years}Y, {ytm}: {e}")
                continue

    return results


# ============================================================================
# TEST CASE 3: Accrued Interest
# ============================================================================

def generate_accrued_interest_tests() -> List[Dict[str, Any]]:
    """Generate accrued interest test cases."""

    results = []

    today = ql.Date(15, 1, 2024)
    ql.Settings.instance().evaluationDate = today

    # Create a standard bond
    issue_date = ql.Date(15, 6, 2020)
    maturity_date = ql.Date(15, 6, 2030)
    coupon_rate = 0.05

    calendar = ql.UnitedStates(ql.UnitedStates.GovernmentBond)
    day_count = ql.Thirty360(ql.Thirty360.USA)

    schedule = ql.Schedule(
        issue_date, maturity_date,
        ql.Period(ql.Semiannual),
        calendar,
        ql.Unadjusted, ql.Unadjusted,
        ql.DateGeneration.Backward, False
    )

    bond = ql.FixedRateBond(2, 100.0, schedule, [coupon_rate], day_count)

    # Test various settlement dates
    settlement_dates = [
        ql.Date(15, 7, 2024),   # 1 month after coupon
        ql.Date(15, 9, 2024),   # 3 months after
        ql.Date(14, 12, 2024),  # 1 day before next coupon
        ql.Date(1, 8, 2024),    # Mid period
        ql.Date(15, 1, 2025),   # Different coupon period
    ]

    for settle_date in settlement_dates:
        try:
            accrued = bond.accruedAmount(settle_date)

            # Get previous and next coupon dates from cashflows
            prev_coupon = get_prev_coupon_date(bond, settle_date)
            next_coupon = get_next_coupon_date(bond, settle_date)

            # Calculate accrued days
            accrued_days = day_count.dayCount(prev_coupon, settle_date)

            results.append({
                "test_type": "accrued_interest",
                "description": f"5% bond, settlement {ql_date_to_str(settle_date)}",
                "inputs": {
                    "issue_date": ql_date_to_str(issue_date),
                    "maturity_date": ql_date_to_str(maturity_date),
                    "settlement_date": ql_date_to_str(settle_date),
                    "coupon_rate": coupon_rate,
                    "frequency": "SemiAnnual",
                    "day_count": "Thirty360US",
                    "face_value": 100.0,
                },
                "expected": {
                    "accrued_interest": round(accrued, 10),
                    "accrued_days": accrued_days,
                    "previous_coupon_date": ql_date_to_str(prev_coupon),
                    "next_coupon_date": ql_date_to_str(next_coupon),
                },
                "tolerance": 0.0001,
            })
        except Exception as e:
            print(f"Skipped accrued: {settle_date}: {e}")
            continue

    return results


# ============================================================================
# TEST CASE 4: Duration and Convexity Tests
# ============================================================================

def generate_duration_convexity_tests() -> List[Dict[str, Any]]:
    """Generate specific duration and convexity test cases."""

    results = []

    today = ql.Date(15, 1, 2024)
    ql.Settings.instance().evaluationDate = today

    calendar = ql.UnitedStates(ql.UnitedStates.GovernmentBond)
    settlement_date = calendar.advance(today, 2, ql.Days)

    # Test cases: (coupon, maturity_years, yield)
    test_cases = [
        (0.05, 5, 0.05),   # Par bond
        (0.02, 10, 0.08),  # Deep discount
        (0.08, 10, 0.04),  # Premium bond
        (0.05, 1, 0.05),   # Short maturity
        (0.05, 30, 0.05),  # Long maturity
    ]

    day_count = ql.Thirty360(ql.Thirty360.USA)

    for coupon_rate, years, ytm in test_cases:
        try:
            maturity_date = ql.Date(15, 1, 2024 + years)

            schedule = ql.Schedule(
                today, maturity_date,
                ql.Period(ql.Semiannual),
                calendar,
                ql.Unadjusted, ql.Unadjusted,
                ql.DateGeneration.Backward, False
            )

            bond = ql.FixedRateBond(2, 100.0, schedule, [coupon_rate], day_count)

            # Create InterestRate object
            rate = ql.InterestRate(ytm, day_count, ql.Compounded, ql.Semiannual)

            clean_price = ql.BondFunctions.cleanPrice(bond, rate, settlement_date)
            accrued = bond.accruedAmount(settlement_date)
            dirty_price = clean_price + accrued

            mac_duration = ql.BondFunctions.duration(bond, rate, ql.Duration.Macaulay, settlement_date)
            mod_duration = ql.BondFunctions.duration(bond, rate, ql.Duration.Modified, settlement_date)
            convexity = ql.BondFunctions.convexity(bond, rate, settlement_date)
            bpv = ql.BondFunctions.basisPointValue(bond, rate, settlement_date)

            # DV01 = Modified Duration * Dirty Price * 0.0001
            dv01 = mod_duration * dirty_price * 0.0001

            results.append({
                "test_type": "duration_convexity",
                "description": f"{coupon_rate*100:.0f}% {years}Y at {ytm*100:.0f}% yield",
                "inputs": {
                    "settlement_date": ql_date_to_str(settlement_date),
                    "maturity_date": ql_date_to_str(maturity_date),
                    "coupon_rate": coupon_rate,
                    "yield_to_maturity": ytm,
                    "face_value": 100.0,
                    "frequency": "SemiAnnual",
                    "day_count": "Thirty360US",
                },
                "expected": {
                    "clean_price": round(clean_price, 10),
                    "dirty_price": round(dirty_price, 10),
                    "macaulay_duration": round(mac_duration, 10),
                    "modified_duration": round(mod_duration, 10),
                    "convexity": round(convexity, 10),
                    "basis_point_value": round(bpv, 10),
                    "dv01": round(dv01, 10),
                },
                "tolerances": {
                    "price": 0.0001,
                    "duration": 0.0001,
                    "convexity": 0.001,
                }
            })
        except Exception as e:
            print(f"Skipped duration: {coupon_rate}, {years}Y, {ytm}: {e}")
            continue

    return results


# ============================================================================
# TEST CASE 5: Zero Coupon Bond Tests
# ============================================================================

def generate_zero_coupon_tests() -> List[Dict[str, Any]]:
    """Generate zero coupon bond test cases."""

    results = []

    today = ql.Date(15, 1, 2024)
    ql.Settings.instance().evaluationDate = today

    maturities = [1, 2, 3, 5, 10, 20, 30]
    test_yields = [0.03, 0.04, 0.05, 0.06]

    calendar = ql.UnitedStates(ql.UnitedStates.GovernmentBond)
    day_count = ql.ActualActual(ql.ActualActual.ISMA)
    settlement_date = calendar.advance(today, 2, ql.Days)

    for years in maturities:
        maturity_date = ql.Date(15, 1, 2024 + years)

        for ytm in test_yields:
            try:
                # Create zero coupon bond
                schedule = ql.Schedule(
                    today, maturity_date,
                    ql.Period(ql.Annual),
                    calendar,
                    ql.Unadjusted, ql.Unadjusted,
                    ql.DateGeneration.Backward, False
                )

                bond = ql.FixedRateBond(2, 100.0, schedule, [0.0], day_count)

                # Create InterestRate object
                rate = ql.InterestRate(ytm, day_count, ql.Compounded, ql.Semiannual)

                clean_price = ql.BondFunctions.cleanPrice(bond, rate, settlement_date)

                # Duration and convexity
                mac_duration = ql.BondFunctions.duration(bond, rate, ql.Duration.Macaulay, settlement_date)
                mod_duration = ql.BondFunctions.duration(bond, rate, ql.Duration.Modified, settlement_date)
                convexity = ql.BondFunctions.convexity(bond, rate, settlement_date)

                results.append({
                    "test_type": "zero_coupon_bond",
                    "description": f"Zero coupon {years}Y at {ytm*100:.1f}% yield",
                    "inputs": {
                        "settlement_date": ql_date_to_str(settlement_date),
                        "maturity_date": ql_date_to_str(maturity_date),
                        "yield_to_maturity": ytm,
                        "face_value": 100.0,
                        "day_count": "ActActIcma",
                    },
                    "expected": {
                        "clean_price": round(clean_price, 10),
                        "macaulay_duration": round(mac_duration, 10),
                        "modified_duration": round(mod_duration, 10),
                        "convexity": round(convexity, 10),
                    },
                    "tolerances": {
                        "price": 0.0001,
                        "duration": 0.001,
                    }
                })
            except Exception as e:
                print(f"Skipped zero coupon: {years}Y, {ytm}: {e}")
                continue

    return results


# ============================================================================
# TEST CASE 6: Curve Bootstrapping
# ============================================================================

def generate_curve_bootstrap_tests() -> List[Dict[str, Any]]:
    """Generate curve bootstrapping test cases."""

    results = []

    today = ql.Date(15, 1, 2024)
    ql.Settings.instance().evaluationDate = today

    calendar = ql.UnitedStates(ql.UnitedStates.GovernmentBond)
    settlement_days = 2
    settlement_date = calendar.advance(today, settlement_days, ql.Days)

    # Par yields for bootstrapping
    par_yields = [
        (ql.Period(3, ql.Months), 0.0525),
        (ql.Period(6, ql.Months), 0.0520),
        (ql.Period(1, ql.Years), 0.0485),
        (ql.Period(2, ql.Years), 0.0435),
        (ql.Period(3, ql.Years), 0.0415),
        (ql.Period(5, ql.Years), 0.0400),
        (ql.Period(7, ql.Years), 0.0405),
        (ql.Period(10, ql.Years), 0.0410),
    ]

    try:
        day_count = ql.ActualActual(ql.ActualActual.ISMA)

        # Deposit helpers for short end
        deposit_helpers = []
        for tenor, rate in par_yields[:2]:
            helper = ql.DepositRateHelper(
                ql.QuoteHandle(ql.SimpleQuote(rate)),
                tenor, settlement_days, calendar,
                ql.ModifiedFollowing, True, ql.Actual360()
            )
            deposit_helpers.append(helper)

        # Bond helpers for longer tenors
        bond_helpers = []
        for tenor, rate in par_yields[2:]:
            helper = ql.FixedRateBondHelper(
                ql.QuoteHandle(ql.SimpleQuote(100.0)),
                settlement_days, 100.0,
                ql.Schedule(
                    settlement_date,
                    calendar.advance(settlement_date, tenor),
                    ql.Period(ql.Semiannual),
                    calendar,
                    ql.Unadjusted, ql.Unadjusted,
                    ql.DateGeneration.Backward, False
                ),
                [rate], day_count
            )
            bond_helpers.append(helper)

        # Build curve
        rate_helpers = deposit_helpers + bond_helpers
        curve = ql.PiecewiseLogLinearDiscount(settlement_date, rate_helpers, day_count)
        curve.enableExtrapolation()

        # Extract zero rates and discount factors
        test_tenors = [0.25, 0.5, 1.0, 2.0, 3.0, 5.0, 7.0, 10.0]
        curve_points = []

        for t in test_tenors:
            months = int(t * 12)
            target_date = calendar.advance(settlement_date, months, ql.Months)
            df = curve.discount(target_date)
            zero_rate = curve.zeroRate(target_date, day_count, ql.Compounded, ql.Semiannual).rate()

            curve_points.append({
                "tenor_years": t,
                "date": ql_date_to_str(target_date),
                "discount_factor": round(df, 12),
                "zero_rate": round(zero_rate, 10),
            })

        results.append({
            "test_type": "curve_bootstrap",
            "description": "Treasury curve bootstrap from par yields",
            "inputs": {
                "evaluation_date": ql_date_to_str(today),
                "settlement_date": ql_date_to_str(settlement_date),
                "par_yields": [
                    {"tenor": str(tenor), "rate": rate}
                    for tenor, rate in par_yields
                ],
                "day_count": "ActActIcma",
                "interpolation": "LogLinearDiscount",
            },
            "expected": {
                "curve_points": curve_points,
            },
            "tolerances": {
                "discount_factor": 1e-8,
                "zero_rate": 1e-6,
            }
        })
    except Exception as e:
        print(f"Skipped curve bootstrap: {e}")

    return results


# ============================================================================
# TEST CASE 7: Real-World Bond (Boeing-like)
# ============================================================================

def generate_real_world_tests() -> List[Dict[str, Any]]:
    """Generate tests based on real-world bond examples."""

    results = []

    # Boeing-like 7.5% Corporate Bond
    today = ql.Date(29, 4, 2020)
    ql.Settings.instance().evaluationDate = today

    issue_date = ql.Date(8, 6, 2005)
    maturity_date = ql.Date(15, 6, 2025)
    coupon_rate = 0.075

    calendar = ql.UnitedStates(ql.UnitedStates.GovernmentBond)
    day_count = ql.Thirty360(ql.Thirty360.USA)

    schedule = ql.Schedule(
        issue_date, maturity_date,
        ql.Period(ql.Semiannual),
        calendar,
        ql.Unadjusted, ql.Unadjusted,
        ql.DateGeneration.Backward, False
    )

    settlement_days = 2
    bond = ql.FixedRateBond(settlement_days, 100.0, schedule, [coupon_rate], day_count)

    settlement_date = calendar.advance(today, settlement_days, ql.Days)

    try:
        # Accrued interest
        accrued = bond.accruedAmount(settlement_date)

        # Get accrued days
        prev_coupon = get_prev_coupon_date(bond, settlement_date)
        accrued_days = day_count.dayCount(prev_coupon, settlement_date)

        # Clean price and dirty price
        clean_price = 110.503
        dirty_price = clean_price + accrued

        # Use approximate yield for analytics
        approx_yield = 0.0490589
        rate = ql.InterestRate(approx_yield, day_count, ql.Compounded, ql.Semiannual)

        mac_duration = ql.BondFunctions.duration(bond, rate, ql.Duration.Macaulay, settlement_date)
        mod_duration = ql.BondFunctions.duration(bond, rate, ql.Duration.Modified, settlement_date)
        convexity = ql.BondFunctions.convexity(bond, rate, settlement_date)
        bpv = ql.BondFunctions.basisPointValue(bond, rate, settlement_date)

        results.append({
            "test_type": "real_world_bond",
            "description": "US Corporate 7.5% 06/2025 (Boeing-like)",
            "inputs": {
                "evaluation_date": ql_date_to_str(today),
                "settlement_date": ql_date_to_str(settlement_date),
                "issue_date": ql_date_to_str(issue_date),
                "maturity_date": ql_date_to_str(maturity_date),
                "coupon_rate": coupon_rate,
                "clean_price": clean_price,
                "frequency": "SemiAnnual",
                "day_count": "Thirty360US",
                "settlement_days": settlement_days,
            },
            "expected": {
                "accrued_interest": round(accrued, 10),
                "accrued_days": accrued_days,
                "dirty_price": round(dirty_price, 10),
                "macaulay_duration": round(mac_duration, 8),
                "modified_duration": round(mod_duration, 8),
                "convexity": round(convexity, 8),
                "basis_point_value": round(bpv, 10),
                "previous_coupon_date": ql_date_to_str(prev_coupon),
            },
            "tolerances": {
                "yield": 0.00001,
                "price": 0.0001,
                "duration": 0.001,
            },
            "notes": "Based on Boeing 7.5% 06/15/2025 bond structure. Accrued days should be 134."
        })
    except Exception as e:
        print(f"Skipped real world bond: {e}")

    return results


# ============================================================================
# MAIN
# ============================================================================

def main():
    """Generate all test cases and write to JSON file."""

    print("Generating QuantLib reference test cases...")
    ql_version = get_quantlib_version()
    print(f"QuantLib version: {ql_version}")

    all_tests = {
        "metadata": {
            "generator": "quantlib_test_generator.py",
            "quantlib_version": ql_version,
            "generated_date": str(date.today()),
            "description": "Reference test cases for Convex bond pricing library validation",
        },
        "test_suites": {
            "day_counts": generate_day_count_tests(),
            "fixed_rate_bonds": generate_fixed_rate_bond_tests(),
            "accrued_interest": generate_accrued_interest_tests(),
            "duration_convexity": generate_duration_convexity_tests(),
            "zero_coupon_bonds": generate_zero_coupon_tests(),
            "curve_bootstrap": generate_curve_bootstrap_tests(),
            "real_world_bonds": generate_real_world_tests(),
        }
    }

    # Summary
    total_tests = sum(len(suite) for suite in all_tests["test_suites"].values())
    print(f"\nGenerated {total_tests} test cases:")
    for suite_name, tests in all_tests["test_suites"].items():
        print(f"  - {suite_name}: {len(tests)} tests")

    # Determine output path
    script_dir = os.path.dirname(os.path.abspath(__file__))
    output_file = os.path.join(script_dir, "..", "fixtures", "quantlib_reference_tests.json")
    output_file = os.path.normpath(output_file)

    # Create directory if needed
    os.makedirs(os.path.dirname(output_file), exist_ok=True)

    # Write to JSON
    with open(output_file, "w") as f:
        json.dump(all_tests, f, indent=2)

    print(f"\nOutput written to: {output_file}")

    return all_tests


if __name__ == "__main__":
    main()

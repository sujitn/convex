package com.convex;

import org.junit.jupiter.api.Test;

import java.math.BigDecimal;
import java.time.LocalDate;
import java.util.List;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

/**
 * End-to-end smoke tests for the Java binding. These require the native
 * {@code convex-ffi} cdylib on the classpath (staged under
 * {@code src/main/resources/native/}); CI builds it before {@code mvn verify}.
 */
class SmokeTest {

    private static final LocalDate SETTLE = LocalDate.of(2025, 4, 15);
    private static final Mark PAR_ISH = Mark.cleanPrice(new BigDecimal("99.5"));

    private static Bond fixedRate5pct() {
        return FixedRateBond.builder()
                .cusip("TEST10Y5")
                .couponRate(new BigDecimal("0.05"))
                .frequency(Frequency.SEMI_ANNUAL)
                .issue(LocalDate.of(2025, 1, 15))
                .maturity(LocalDate.of(2035, 1, 15))
                .dayCount(DayCount.THIRTY_360_US)
                .currency(Currency.USD)
                .build();
    }

    private static YieldCurve flatCurve(double rate) {
        return YieldCurve.discrete()
                .referenceDate(LocalDate.of(2025, 1, 15))
                .point(0.5, rate).point(2.0, rate).point(5.0, rate)
                .point(10.0, rate).point(30.0, rate)
                .interpolation(Interpolation.LINEAR)
                .build();
    }

    @Test
    void versionIsNonEmpty() {
        assertTrue(!Convex.version().isBlank());
    }

    @Test
    void priceFromCleanMark() {
        try (Bond bond = fixedRate5pct()) {
            var r = ConvexAnalytics.price(bond, SETTLE, PAR_ISH);
            assertEquals(99.5, r.cleanPrice(), 1e-9);
            assertTrue(r.ytmDecimal() > 0.04 && r.ytmDecimal() < 0.06, "ytm=" + r.ytmDecimal());
            assertEquals(r.dirtyPrice(), r.cleanPrice() + r.accrued(), 1e-9);
        }
    }

    @Test
    void riskHasDv01AndKeyRates() {
        try (Bond bond = fixedRate5pct(); YieldCurve curve = flatCurve(0.04)) {
            var r = ConvexAnalytics.risk(bond, SETTLE, PAR_ISH, curve, 2, 5, 10, 30);
            assertNotEquals(0.0, r.dv01());
            assertTrue(r.modifiedDuration() > 0 && r.modifiedDuration() < 30);
            assertEquals(4, r.keyRates().size());
        }
    }

    @Test
    void zSpreadIsFinite() {
        try (Bond bond = fixedRate5pct(); YieldCurve curve = flatCurve(0.04)) {
            var s = ConvexAnalytics.spread(bond, curve, SETTLE, PAR_ISH, SpreadType.Z_SPREAD);
            assertTrue(Double.isFinite(s.spreadBps()));
            assertTrue(s.spreadDv01().isPresent());
        }
    }

    @Test
    void hedgeAdvisorEndToEnd() {
        try (Bond bond = fixedRate5pct(); YieldCurve curve = flatCurve(0.04)) {
            RiskProfile pos = HedgeAdvisor.positionRisk()
                    .bond(bond).settlement(SETTLE).mark(PAR_ISH)
                    .notionalFace(new BigDecimal("10000000"))
                    .curve(curve).curveId("USD.SOFR")
                    .keyRateTenors(2, 5, 10, 30)
                    .compute();

            assertNotEquals(0.0, pos.dv01());
            assertEquals(4, pos.keyRateBuckets().size());
            assertTrue(pos.marketValue() > 9_000_000 && pos.marketValue() < 11_000_000);

            HedgeProposal df = HedgeAdvisor.durationFutures(pos, curve, SETTLE);
            HedgeProposal sw = HedgeAdvisor.swap(pos, curve, SETTLE, Constraints.none());
            assertTrue(df.tradeCount() >= 1);
            assertTrue(Math.abs(df.residualDv01()) < Math.abs(pos.dv01()));

            ComparisonReport report = HedgeAdvisor.compare(pos, List.of(df, sw), Constraints.none(), true);
            assertEquals(2, report.rowCount());
            assertTrue(report.recommendedRowIndex() < report.rowCount());
            assertTrue(report.narrative().orElse("").length() > 0);
        }
    }

    @Test
    void buildsAllBondShapes() {
        // FRN, zero-coupon, and sinking-fund builders each produce a usable
        // handle with a non-empty cashflow schedule.
        try (Bond frn = FloatingRateNote.builder()
                .cusip("FRN00000A").spreadBps(new BigDecimal("75"))
                .rateIndex(RateIndex.SOFR)
                .issue(LocalDate.of(2025, 1, 15)).maturity(LocalDate.of(2030, 1, 15))
                .build();
             Bond zero = ZeroCouponBond.builder()
                .cusip("ZERO0000A")
                .issue(LocalDate.of(2025, 1, 15)).maturity(LocalDate.of(2030, 1, 15))
                .build();
             Bond sinker = SinkingFundBond.builder()
                .cusip("SINK0000A").couponRate(new BigDecimal("0.05"))
                .issue(LocalDate.of(2025, 1, 15)).maturity(LocalDate.of(2030, 1, 15))
                .sink(LocalDate.of(2028, 1, 15), 25.0)
                .sink(LocalDate.of(2029, 1, 15), 25.0)
                .build()) {

            assertTrue(ConvexAnalytics.cashflows(frn, SETTLE).size() >= 1);
            assertEquals(1, ConvexAnalytics.cashflows(zero, SETTLE).size(), "zero has a single redemption flow");
            assertTrue(ConvexAnalytics.cashflows(sinker, SETTLE).size() >= 1);
        }
    }

    @Test
    void handlesAreReleased() {
        Convex.clearAll();
        try (Bond bond = fixedRate5pct(); YieldCurve curve = flatCurve(0.04)) {
            assertEquals(2, Convex.objectCount());
        }
        assertEquals(0, Convex.objectCount(), "try-with-resources must release native objects");
    }
}

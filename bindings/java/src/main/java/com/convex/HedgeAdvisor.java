package com.convex;

import com.convex.internal.ConvexFfi;
import com.convex.internal.Json;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.ObjectNode;

import java.math.BigDecimal;
import java.time.LocalDate;
import java.util.List;

/**
 * The hedge advisor: build a position {@link RiskProfile}, propose hedges via a
 * range of strategies, and {@link #compare} them. Mirrors
 * {@code convex_analytics::risk}. See the module README for a usage example.
 */
public final class HedgeAdvisor {

    private HedgeAdvisor() {}

    // ---- position risk ------------------------------------------------------

    public static PositionRiskBuilder positionRisk() {
        return new PositionRiskBuilder();
    }

    public static final class PositionRiskBuilder {
        private Bond bond;
        private LocalDate settlement;
        private Mark mark;
        private BigDecimal notionalFace;
        private YieldCurve curve;
        private String curveId = "discount";
        private Frequency quoteFrequency;
        private double[] keyRateTenors;
        private String positionId;
        private Double volatility;

        public PositionRiskBuilder bond(Bond v) { this.bond = v; return this; }
        public PositionRiskBuilder settlement(LocalDate v) { this.settlement = v; return this; }
        public PositionRiskBuilder mark(Mark v) { this.mark = v; return this; }
        public PositionRiskBuilder notionalFace(BigDecimal v) { this.notionalFace = v; return this; }
        public PositionRiskBuilder curve(YieldCurve v) { this.curve = v; return this; }
        public PositionRiskBuilder curveId(String v) { this.curveId = v; return this; }
        public PositionRiskBuilder quoteFrequency(Frequency v) { this.quoteFrequency = v; return this; }
        public PositionRiskBuilder keyRateTenors(double... v) { this.keyRateTenors = v; return this; }
        public PositionRiskBuilder positionId(String v) { this.positionId = v; return this; }
        /** Required for callable positions (short-rate volatility, decimal). */
        public PositionRiskBuilder volatility(double v) { this.volatility = v; return this; }

        public RiskProfile compute() {
            Specs.require(bond, "bond");
            Specs.require(settlement, "settlement");
            Specs.require(mark, "mark");
            Specs.require(notionalFace, "notionalFace");
            Specs.require(curve, "curve");

            ObjectNode req = Json.object();
            req.put("bond", bond.handle());
            req.put("settlement", settlement.toString());
            req.put("mark", mark.wire());
            req.put("notional_face", notionalFace);
            req.put("curve", curve.handle());
            req.put("curve_id", curveId);
            if (quoteFrequency != null) {
                req.put("quote_frequency", quoteFrequency.wire());
            }
            if (keyRateTenors != null && keyRateTenors.length > 0) {
                ArrayNode arr = req.putArray("key_rate_tenors");
                for (double t : keyRateTenors) {
                    arr.add(t);
                }
            }
            if (positionId != null) {
                req.put("position_id", positionId);
            }
            if (volatility != null) {
                req.put("volatility", volatility);
            }
            return new RiskProfile(Json.unwrap(ConvexFfi.riskProfile(Json.write(req))));
        }
    }

    // ---- strategies ---------------------------------------------------------

    public static HedgeProposal durationFutures(RiskProfile p, YieldCurve curve, LocalDate settlement) {
        return durationFutures(p, curve, settlement, Constraints.none());
    }

    public static HedgeProposal durationFutures(RiskProfile p, YieldCurve curve, LocalDate settlement, Constraints c) {
        return strategy("duration_futures", p, curve, settlement, c);
    }

    public static HedgeProposal barbellFutures(RiskProfile p, YieldCurve curve, LocalDate settlement, Constraints c) {
        return strategy("barbell_futures", p, curve, settlement, c);
    }

    public static HedgeProposal cashBondPair(RiskProfile p, YieldCurve curve, LocalDate settlement, Constraints c) {
        return strategy("cash_bond_pair", p, curve, settlement, c);
    }

    /** Vanilla interest-rate-swap hedge. */
    public static HedgeProposal swap(RiskProfile p, YieldCurve curve, LocalDate settlement, Constraints c) {
        return strategy("interest_rate_swap", p, curve, settlement, c);
    }

    public static HedgeProposal keyRateFutures(RiskProfile p, YieldCurve curve, LocalDate settlement, Constraints c) {
        return strategy("key_rate_futures", p, curve, settlement, c);
    }

    private static HedgeProposal strategy(String kind, RiskProfile p, YieldCurve curve,
                                          LocalDate settlement, Constraints c) {
        ObjectNode req = Json.object();
        req.put("strategy", kind);
        req.set("position", p.node());
        req.put("curve", curve.handle());
        req.put("curve_id", "discount");
        req.put("settlement", settlement.toString());
        if (c != null) {
            req.set("constraints", c.toJson());
        }
        return new HedgeProposal(Json.unwrap(ConvexFfi.hedge(Json.write(req))));
    }

    // ---- compare ------------------------------------------------------------

    public static ComparisonReport compare(RiskProfile position, List<HedgeProposal> proposals,
                                           Constraints constraints, boolean narrate) {
        ObjectNode req = Json.object();
        req.set("position", position.node());
        ArrayNode arr = req.putArray("proposals");
        for (HedgeProposal hp : proposals) {
            arr.add(hp.node());
        }
        if (constraints != null) {
            req.set("constraints", constraints.toJson());
        }
        req.put("narrate", narrate);

        JsonNode result = Json.unwrap(ConvexFfi.compare(Json.write(req)));
        JsonNode report = result.get("report");
        String narrative = result.hasNonNull("narrative") ? result.get("narrative").asText() : null;
        return new ComparisonReport(report, narrative);
    }
}

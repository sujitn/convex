package com.convex;

import com.convex.internal.ConvexFfi;
import com.convex.internal.Json;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.node.ObjectNode;

import java.time.LocalDate;
import java.util.ArrayList;
import java.util.List;
import java.util.OptionalDouble;

/**
 * Stateless bond analytics — pricing, risk, spreads, cashflows, make-whole —
 * mirroring the standalone functions in {@code convex_analytics::functions}.
 *
 * <p>All methods are pure: they take a {@link Bond} (and optionally a
 * {@link YieldCurve}) plus a {@link Mark} and return an immutable result
 * record. Prices and accrued are per 100 face; yields/rates are decimals.
 */
public final class ConvexAnalytics {

    private ConvexAnalytics() {}

    // ---- result records -----------------------------------------------------

    public record PricingResult(double cleanPrice, double dirtyPrice, double accrued,
                                double ytmDecimal, OptionalDouble zSpreadBps) {}

    public record KeyRate(double tenorYears, double duration) {}

    public record RiskResult(double modifiedDuration, double macaulayDuration, double convexity,
                             double dv01, OptionalDouble spreadDuration, List<KeyRate> keyRates) {}

    public record SpreadResult(double spreadBps, OptionalDouble spreadDv01, OptionalDouble spreadDuration,
                               OptionalDouble optionValue, OptionalDouble effectiveDuration,
                               OptionalDouble effectiveConvexity) {}

    public record CashFlow(LocalDate date, double amount, String kind) {}

    public record MakeWholeResult(double price, double discountRate, double spreadBps) {}

    // ---- pricing ------------------------------------------------------------

    public static PricingResult price(Bond bond, LocalDate settlement, Mark mark) {
        return price(bond, settlement, mark, null);
    }

    public static PricingResult price(Bond bond, LocalDate settlement, Mark mark, YieldCurve curve) {
        ObjectNode req = base(bond, settlement, mark);
        if (curve != null) {
            req.put("curve", curve.handle());
        }
        JsonNode r = Json.unwrap(ConvexFfi.price(Json.write(req)));
        return new PricingResult(
                Json.dbl(r, "clean_price"),
                Json.dbl(r, "dirty_price"),
                Json.dbl(r, "accrued"),
                Json.dbl(r, "ytm_decimal"),
                Json.optDbl(r, "z_spread_bps"));
    }

    // ---- risk ---------------------------------------------------------------

    public static RiskResult risk(Bond bond, LocalDate settlement, Mark mark,
                                  YieldCurve curve, double... keyRateTenors) {
        ObjectNode req = base(bond, settlement, mark);
        if (curve != null) {
            req.put("curve", curve.handle());
        }
        if (keyRateTenors != null && keyRateTenors.length > 0) {
            var arr = req.putArray("key_rate_tenors");
            for (double t : keyRateTenors) {
                arr.add(t);
            }
        }
        JsonNode r = Json.unwrap(ConvexFfi.risk(Json.write(req)));
        List<KeyRate> krd = new ArrayList<>();
        JsonNode rates = r.get("key_rates");
        if (rates != null && rates.isArray()) {
            for (JsonNode k : rates) {
                krd.add(new KeyRate(k.get("tenor").asDouble(), k.get("duration").asDouble()));
            }
        }
        return new RiskResult(
                Json.dbl(r, "modified_duration"),
                Json.dbl(r, "macaulay_duration"),
                Json.dbl(r, "convexity"),
                Json.dbl(r, "dv01"),
                Json.optDbl(r, "spread_duration"),
                List.copyOf(krd));
    }

    // ---- spreads ------------------------------------------------------------

    /** Optional per-family spread parameters (OAS volatility, G-spread govt curve, …). */
    public static final class SpreadParams {
        private Double volatility;
        private YieldCurve govtCurve;
        private YieldCurve forwardCurve;
        private Double currentIndex;

        public static SpreadParams create() { return new SpreadParams(); }
        public SpreadParams volatility(double v) { this.volatility = v; return this; }
        public SpreadParams govtCurve(YieldCurve c) { this.govtCurve = c; return this; }
        public SpreadParams forwardCurve(YieldCurve c) { this.forwardCurve = c; return this; }
        public SpreadParams currentIndex(double v) { this.currentIndex = v; return this; }

        ObjectNode toJson() {
            ObjectNode p = Json.object();
            if (volatility != null) p.put("volatility", volatility);
            if (govtCurve != null) p.put("govt_curve", govtCurve.handle());
            if (forwardCurve != null) p.put("forward_curve", forwardCurve.handle());
            if (currentIndex != null) p.put("current_index", currentIndex);
            return p;
        }
    }

    public static SpreadResult spread(Bond bond, YieldCurve curve, LocalDate settlement,
                                      Mark mark, SpreadType type) {
        return spread(bond, curve, settlement, mark, type, null);
    }

    public static SpreadResult spread(Bond bond, YieldCurve curve, LocalDate settlement,
                                      Mark mark, SpreadType type, SpreadParams params) {
        ObjectNode req = base(bond, settlement, mark);
        req.put("curve", curve.handle());
        req.put("spread_type", type.wire());
        if (params != null) {
            req.set("params", params.toJson());
        }
        JsonNode r = Json.unwrap(ConvexFfi.spread(Json.write(req)));
        return new SpreadResult(
                Json.dbl(r, "spread_bps"),
                Json.optDbl(r, "spread_dv01"),
                Json.optDbl(r, "spread_duration"),
                Json.optDbl(r, "option_value"),
                Json.optDbl(r, "effective_duration"),
                Json.optDbl(r, "effective_convexity"));
    }

    // ---- cashflows ----------------------------------------------------------

    public static List<CashFlow> cashflows(Bond bond, LocalDate settlement) {
        ObjectNode req = Json.object();
        req.put("bond", bond.handle());
        req.put("settlement", settlement.toString());
        JsonNode r = Json.unwrap(ConvexFfi.cashflows(Json.write(req)));
        List<CashFlow> flows = new ArrayList<>();
        JsonNode arr = r.get("flows");
        if (arr != null && arr.isArray()) {
            for (JsonNode f : arr) {
                flows.add(new CashFlow(
                        LocalDate.parse(f.get("date").asText()),
                        f.get("amount").asDouble(),
                        f.get("kind").asText()));
            }
        }
        return List.copyOf(flows);
    }

    // ---- make-whole ---------------------------------------------------------

    public static MakeWholeResult makeWhole(Bond callable, LocalDate callDate, double treasuryRate) {
        ObjectNode req = Json.object();
        req.put("bond", callable.handle());
        req.put("call_date", callDate.toString());
        req.put("treasury_rate", treasuryRate);
        JsonNode r = Json.unwrap(ConvexFfi.makeWhole(Json.write(req)));
        return new MakeWholeResult(
                Json.dbl(r, "price"),
                Json.dbl(r, "discount_rate"),
                Json.dbl(r, "spread_bps"));
    }

    // ---- shared -------------------------------------------------------------

    private static ObjectNode base(Bond bond, LocalDate settlement, Mark mark) {
        ObjectNode req = Json.object();
        req.put("bond", bond.handle());
        req.put("settlement", settlement.toString());
        req.put("mark", mark.wire()); // MarkInput::Text
        return req;
    }
}

package com.convex;

import com.convex.internal.ConvexFfi;
import com.convex.internal.Json;
import com.convex.internal.NativeRef;
import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.ObjectNode;

import java.lang.ref.Cleaner;
import java.time.LocalDate;
import java.util.ArrayList;
import java.util.List;

/**
 * A discount / yield curve in the native registry. Build it with
 * {@link #discrete()}; values are continuously-compounded zero rates (decimal)
 * unless {@link DiscreteBuilder#asDiscountFactors()} is set. See the module
 * README for a usage example.
 */
public final class YieldCurve implements AutoCloseable {

    private final long handle;
    private final Cleaner.Cleanable cleanable;

    private YieldCurve(long handle) {
        this.handle = handle;
        this.cleanable = NativeRef.releaseOnClean(this, handle);
    }

    long handle() {
        return handle;
    }

    public static DiscreteBuilder discrete() {
        return new DiscreteBuilder();
    }

    // ---- queries ------------------------------------------------------------

    /** Zero rate (continuously compounded, decimal) at a tenor in years. */
    public double zeroRate(double tenorYears) {
        return query("zero", tenorYears, null);
    }

    /** Discount factor at a tenor in years. */
    public double discountFactor(double tenorYears) {
        return query("df", tenorYears, null);
    }

    /** Forward rate (continuously compounded, decimal) between two tenors in years. */
    public double forwardRate(double startYears, double endYears) {
        return query("forward", startYears, endYears);
    }

    private double query(String kind, double tenor, Double tenorEnd) {
        ObjectNode req = Json.object();
        req.put("curve", handle);
        req.put("query", kind);
        req.put("tenor", tenor);
        if (tenorEnd != null) {
            req.put("tenor_end", tenorEnd);
        }
        JsonNode result = Json.unwrap(ConvexFfi.curveQuery(Json.write(req)));
        return result.get("value").asDouble();
    }

    @Override
    public void close() {
        cleanable.clean();
    }

    // ---- builder ------------------------------------------------------------

    public static final class DiscreteBuilder {
        private String name;
        private LocalDate referenceDate;
        private final List<double[]> points = new ArrayList<>(); // {tenor, value}
        private Interpolation interpolation = Interpolation.LINEAR;
        private boolean discountFactors = false;

        public DiscreteBuilder name(String v) { this.name = v; return this; }
        public DiscreteBuilder referenceDate(LocalDate v) { this.referenceDate = v; return this; }
        public DiscreteBuilder interpolation(Interpolation v) { this.interpolation = v; return this; }

        /** Add a (tenorYears, zeroRateDecimal) node. */
        public DiscreteBuilder point(double tenorYears, double zeroRate) {
            points.add(new double[]{tenorYears, zeroRate});
            return this;
        }

        /** Interpret the supplied values as discount factors instead of zero rates. */
        public DiscreteBuilder asDiscountFactors() {
            this.discountFactors = true;
            return this;
        }

        public YieldCurve build() {
            Specs.require(referenceDate, "referenceDate");
            if (points.isEmpty()) {
                throw new IllegalArgumentException("a discrete curve needs at least one point");
            }
            ObjectNode spec = Json.object();
            spec.put("type", "discrete");
            if (name != null) {
                spec.put("name", name);
            }
            spec.put("ref_date", referenceDate.toString());
            ArrayNode tenors = spec.putArray("tenors");
            ArrayNode values = spec.putArray("values");
            for (double[] p : points) {
                tenors.add(p[0]);
                values.add(p[1]);
            }
            spec.put("value_kind", discountFactors ? "discount_factor" : "zero_rate");
            spec.put("interpolation", interpolation.wire());
            return new YieldCurve(ConvexFfi.buildCurve(Json.write(spec)));
        }
    }
}

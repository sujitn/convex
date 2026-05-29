package com.convex;

import com.convex.internal.Json;
import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.ObjectNode;

import java.util.ArrayList;
import java.util.List;

/**
 * Hedge constraints, mirroring {@code convex_analytics::risk::Constraints}.
 *
 * <p>All limits are optional; an empty {@code Constraints} (the default) imposes
 * none. Used to bound residual DV01 / cost and to express per-bucket KRD limits
 * for {@link HedgeAdvisor#compare}.
 */
public final class Constraints {

    private Double maxResidualDv01;
    private Double maxCostBps;
    private final List<String> allowedStrategies = new ArrayList<>();
    private final List<double[]> perBucketLimits = new ArrayList<>(); // {tenor_years, max_abs_dv01}

    public static Constraints none() {
        return new Constraints();
    }

    public Constraints maxResidualDv01(double v) { this.maxResidualDv01 = v; return this; }
    public Constraints maxCostBps(double v) { this.maxCostBps = v; return this; }

    public Constraints allowStrategy(String name) {
        allowedStrategies.add(name);
        return this;
    }

    public Constraints maxResidualAtTenor(double tenorYears, double maxAbsDv01) {
        perBucketLimits.add(new double[]{tenorYears, maxAbsDv01});
        return this;
    }

    ObjectNode toJson() {
        ObjectNode n = Json.object();
        if (maxResidualDv01 != null) {
            n.put("max_residual_dv01", maxResidualDv01);
        }
        if (maxCostBps != null) {
            n.put("max_cost_bps", maxCostBps);
        }
        if (!allowedStrategies.isEmpty()) {
            ArrayNode arr = n.putArray("allowed_strategies");
            allowedStrategies.forEach(arr::add);
        }
        if (!perBucketLimits.isEmpty()) {
            ArrayNode arr = n.putArray("max_residual_per_bucket");
            for (double[] lim : perBucketLimits) {
                ObjectNode e = Json.object();
                e.put("tenor_years", lim[0]);
                e.put("max_abs_dv01", lim[1]);
                arr.add(e);
            }
        }
        return n;
    }
}

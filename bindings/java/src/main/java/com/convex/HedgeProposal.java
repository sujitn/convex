package com.convex;

import com.fasterxml.jackson.databind.JsonNode;

/**
 * A proposed hedge for a {@link RiskProfile}, as returned by the
 * {@link HedgeAdvisor} strategy methods.
 *
 * <p>Backed by the verbatim native JSON so it round-trips into
 * {@link HedgeAdvisor#compare}. Typed getters cover the headline figures;
 * {@link #raw()} exposes the full trade list, residual buckets, and tradeoff
 * notes.
 */
public final class HedgeProposal {

    private final JsonNode node;

    HedgeProposal(JsonNode node) {
        this.node = node;
    }

    JsonNode node() {
        return node;
    }

    /** Strategy label (e.g. {@code "DurationFutures"}). */
    public String strategy() {
        return node.path("strategy").asText();
    }

    /** Round-trip cost as basis points of position market value. */
    public double costBps() {
        return node.path("cost_bps").asDouble(Double.NaN);
    }

    /** Residual DV01 after the hedge ({@code position.dv01 + Σ trade.dv01}). */
    public double residualDv01() {
        return node.path("residual").path("residual_dv01").asDouble(Double.NaN);
    }

    /** L1 norm of the residual key-rate vector. */
    public double residualKrdL1Norm() {
        return node.path("residual").path("residual_krd_l1_norm").asDouble(Double.NaN);
    }

    /** Number of trade legs in the proposal. */
    public int tradeCount() {
        JsonNode trades = node.get("trades");
        return trades == null ? 0 : trades.size();
    }

    public JsonNode raw() {
        return node;
    }
}

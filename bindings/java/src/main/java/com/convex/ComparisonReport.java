package com.convex;

import com.fasterxml.jackson.databind.JsonNode;

import java.util.Optional;

/**
 * The result of comparing several {@link HedgeProposal}s against a position —
 * one row per proposal plus a recommendation, and an optional deterministic
 * narrative.
 */
public final class ComparisonReport {

    private final JsonNode report;
    private final String narrative;

    ComparisonReport(JsonNode report, String narrative) {
        this.report = report;
        this.narrative = narrative;
    }

    /** Strategy label of the recommended proposal. */
    public String recommendedStrategy() {
        return report.path("recommendation").path("strategy").asText();
    }

    /** Index into the comparison rows (input order) of the recommendation. */
    public int recommendedRowIndex() {
        return report.path("recommendation").path("row_index").asInt();
    }

    /** Number of comparison rows (== number of proposals compared). */
    public int rowCount() {
        JsonNode rows = report.get("rows");
        return rows == null ? 0 : rows.size();
    }

    public double positionDv01() {
        return report.path("position_dv01").asDouble(Double.NaN);
    }

    /** The deterministic text narrative, if it was requested. */
    public Optional<String> narrative() {
        return Optional.ofNullable(narrative);
    }

    public JsonNode raw() {
        return report;
    }
}

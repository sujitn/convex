package com.convex;

import com.convex.internal.Json;
import com.fasterxml.jackson.databind.JsonNode;

import java.util.ArrayList;
import java.util.List;

/**
 * A position's risk profile, as produced by {@link HedgeAdvisor#positionRisk()}.
 *
 * <p>Backed by the exact JSON the native layer returned so it round-trips
 * losslessly into the hedge-strategy and compare calls (those take the full
 * profile by value). Typed getters expose the headline metrics; {@link #raw()}
 * is the escape hatch for fields not surfaced here.
 */
public final class RiskProfile {

    private final JsonNode node;

    RiskProfile(JsonNode node) {
        this.node = node;
    }

    /** A single key-rate bucket: tenor (years) and partial DV01. */
    public record Bucket(double tenorYears, double partialDv01) {}

    /** Package-private: the verbatim profile JSON for round-tripping. */
    JsonNode node() {
        return node;
    }

    public String currency()          { return node.path("currency").asText(); }
    public double notionalFace()      { return node.path("notional_face").asDouble(); }
    public double marketValue()       { return node.path("market_value").asDouble(); }
    public double cleanPrice()        { return Json.dbl(node, "clean_price_per_100"); }
    public double dirtyPrice()        { return Json.dbl(node, "dirty_price_per_100"); }
    public double ytmDecimal()        { return Json.dbl(node, "ytm_decimal"); }
    public double modifiedDuration()  { return Json.dbl(node, "modified_duration_years"); }
    public double macaulayDuration()  { return Json.dbl(node, "macaulay_duration_years"); }
    public double convexity()         { return Json.dbl(node, "convexity"); }
    public double dv01()              { return Json.dbl(node, "dv01"); }

    public List<Bucket> keyRateBuckets() {
        List<Bucket> out = new ArrayList<>();
        JsonNode arr = node.get("key_rate_buckets");
        if (arr != null && arr.isArray()) {
            for (JsonNode b : arr) {
                out.add(new Bucket(b.path("tenor_years").asDouble(), b.path("partial_dv01").asDouble()));
            }
        }
        return List.copyOf(out);
    }

    /** The full underlying JSON (read-only). */
    public JsonNode raw() {
        return node;
    }
}
